//! Mac side of the deck: finds the coding-agent sessions running on this
//! machine and sends them to the ESP32 as [`link_lib`] frames.
//!
//! Each agent stores its state differently, so each has its own module:
//!
//! - `claude`: Claude Code writes one file per running session.
//! - `pi`: finds `pi` processes and reads their transcripts.
//! - `codex`: finds the transcripts `codex` processes have open.
//!
//! These files are internal to each agent and may change between versions.
//! The modules run `ps` and `lsof`, so they work on macOS and Linux.
//!
//! Each module has a unit struct implementing `Sessions`. To add an agent:
//!
//! 1. Add its variant and frame byte to [`Agent`].
//! 2. Add its logo to the `match` in `draw_session`.
//! 3. Add a module with a unit struct implementing `Sessions`.
//! 4. List that struct in `AGENTS`.

mod claude;
mod codex;
mod pi;

use std::{
    cmp::Reverse,
    convert::Infallible,
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime},
};

use link_lib::{Agent, Clock, Context, Encoder, Entry, MAX_FRAME_LEN, SessionColor, Status};

/// Fetches every agent's sessions, busy first and newest first within each
/// status, and encodes them into `buf` as one frame.
///
/// # Errors
///
/// Returns an error if an agent's sessions cannot be fetched.
pub fn frame(buf: &mut [u8; MAX_FRAME_LEN]) -> io::Result<&[u8]> {
    let mut sessions = Vec::new();
    for agent in AGENTS {
        sessions.extend(agent.sessions()?);
    }
    prioritise(&mut sessions);
    let mut encoder = Encoder::new(buf, local_clock());
    for session in &sessions {
        encoder.push(&Entry {
            agent: session.agent,
            status: session.status,
            color: session.color,
            name: &session.name,
            uptime: session.started.elapsed().unwrap_or_default().as_secs(),
            context: session.context,
            subagents: session.subagents,
        });
    }
    Ok(encoder.finish())
}

/// Main loop: sends a fresh frame to `port` every `interval`, forever.
///
/// `port` is the deck's serial port, already open at the firmware's baud
/// rate.
///
/// # Errors
///
/// Returns an error if fetching the sessions or writing to `port` fails.
pub fn run(mut port: impl Write, interval: Duration) -> io::Result<Infallible> {
    let mut buf = [0; MAX_FRAME_LEN];
    loop {
        port.write_all(frame(&mut buf)?)?;
        port.flush()?;
        thread::sleep(interval);
    }
}

/// A coding agent whose running sessions the host can fetch.
///
/// Each agent module defines one unit struct that implements this trait.
/// Only implementors can be listed in [`AGENTS`], so a new agent's sessions
/// reach the deck only once it implements this trait.
trait Sessions {
    /// Returns the agent's sessions running on this machine.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent's session state cannot be read.
    fn sessions(&self) -> io::Result<Vec<Session>>;
}

/// Every agent whose sessions are sent to the deck.
const AGENTS: [&dyn Sessions; 3] = [&claude::Claude, &pi::Pi, &codex::Codex];

/// One running agent session.
struct Session {
    agent: Agent,
    /// The session name, or its working directory's name if it has none.
    name: String,
    status: Status,
    color: Option<SessionColor>,
    started: SystemTime,
    context: Option<Context>,
    subagents: usize,
}

/// Puts busy sessions first, then the newest session in each status group.
fn prioritise(sessions: &mut [Session]) {
    sessions.sort_by_key(|session| (session.status != Status::Busy, Reverse(session.started)));
}

/// Returns the Mac's local hour and minute.
fn local_clock() -> Clock {
    let output = stdout("date", &["+%H%M"]).unwrap_or_default();
    let bytes = output.trim().as_bytes();
    let [h1, h2, m1, m2] = bytes else {
        return Clock::default();
    };
    Clock {
        hour: (h1.saturating_sub(b'0')) * 10 + h2.saturating_sub(b'0'),
        minute: (m1.saturating_sub(b'0')) * 10 + m2.saturating_sub(b'0'),
    }
}

/// Returns `path` inside the home directory.
fn home_path(path: &str) -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(path))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

/// Returns the last component of `path`, used as the name of a session that
/// has none.
fn dir_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || "?".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Returns when the file at `path` was created, or the epoch if unknown.
fn created(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|meta| meta.created())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Runs `program` with `args` and returns its standard output.
///
/// The exit status is ignored because `lsof` exits with 1 when it finds
/// nothing.
fn stdout(program: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(program).args(args).output()?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A running process, as listed by `ps`.
struct Process {
    pid: u32,
    started: SystemTime,
    args: String,
}

/// Returns every running process.
/// Not the best option but the lightest option
/// for our purposes.
/// SAFE FOR CURRENT PROJECT USECASE
fn processes() -> io::Result<Vec<Process>> {
    let output = stdout("ps", &["-axww", "-o", "pid=,etime=,args="])?;
    let now = SystemTime::now();
    Ok(output
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(' ')?;
            let (elapsed, args) = rest.trim_start().split_once(' ')?;
            Some(Process {
                pid: pid.parse().ok()?,
                started: now.checked_sub(parse_elapsed(elapsed)?)?,
                args: args.trim_start().to_owned(),
            })
        })
        .collect())
}

/// Parses the time since a process started, as `ps` prints it:
/// `[[dd-]hh:]mm:ss`.
fn parse_elapsed(elapsed: &str) -> Option<Duration> {
    let (days, clock) = elapsed.split_once('-').unwrap_or(("0", elapsed));
    let secs = clock.split(':').try_fold(0, |secs: u64, part| {
        Some(secs * 60 + part.parse::<u64>().ok()?)
    })?;
    Some(Duration::from_secs(
        days.parse::<u64>().ok()? * 86_400 + secs,
    ))
}

/// Runs `lsof` with `args` and returns the PID and path of each file it
/// lists.
fn lsof(args: &[&str]) -> io::Result<Vec<(u32, PathBuf)>> {
    let output = stdout("lsof", &[args, &["-Fpn"]].concat())?;
    let mut pid = 0;
    let mut files = Vec::new();
    // `-Fpn` prints a `p<pid>` line for each process, then an `n<path>` line
    // for each of its files.
    for line in output.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().unwrap_or(0);
        } else if let Some(path) = line.strip_prefix('n') {
            files.push((pid, PathBuf::from(path)));
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_sessions_win_then_newest_sessions() {
        let session = |name: &str, status, seconds| Session {
            agent: Agent::Pi,
            name: name.to_owned(),
            status,
            color: None,
            started: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            context: None,
            subagents: 0,
        };
        let mut sessions = vec![
            session("old idle", Status::Idle, 1),
            session("new idle", Status::Idle, 4),
            session("old busy", Status::Busy, 2),
            session("new busy", Status::Busy, 3),
        ];

        prioritise(&mut sessions);

        assert_eq!(
            sessions
                .iter()
                .map(|session| session.name.as_str())
                .collect::<Vec<_>>(),
            ["new busy", "old busy", "new idle", "old idle"]
        );
    }
}
