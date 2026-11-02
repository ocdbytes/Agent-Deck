//! Finds the coding-agent sessions running on this machine.
//!
//! Each agent stores its state differently, so each has its own
//! [`SessionFetcher`]:
//!
//! - [`Claude`]: Claude Code writes one file per running session.
//! - [`Pi`]: finds `pi` processes and reads their transcripts.
//! - [`Codex`]: finds the transcripts `codex` processes have open.
//!
//! These files are internal to each agent and may change between versions.
//! The fetchers run `ps` and `lsof`, so they work on macOS and Linux.

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

mod claude;
mod codex;
mod pi;

use std::{
    env, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

pub use claude::Claude;
pub use codex::Codex;
pub use link_lib::{Agent, Context, Status};
use link_lib::{Entry, SubagentEntry};
pub use pi::Pi;
use private::Sealed;

/// Lists the running sessions of one coding agent.
///
/// [`Claude`], [`Pi`] and [`Codex`] implement it. It is sealed because only
/// this crate can build a [`Session`].
///
/// # Examples
///
/// ```no_run
/// use deck_lib::{Claude, Codex, Pi, SessionFetcher};
///
/// let fetchers: [&dyn SessionFetcher; 3] = [&Claude, &Pi, &Codex];
/// for fetcher in fetchers {
///     for session in fetcher.sessions()? {
///         println!("{} {} {}", session.status(), session.agent(), session.name());
///     }
/// }
/// # Ok::<(), std::io::Error>(())
/// ```
pub trait SessionFetcher: Sealed {
    /// Returns the agent's running sessions.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent's state cannot be read. Each
    /// implementation lists what it reads.
    fn sessions(&self) -> io::Result<Vec<Session>>;
}

/// Keeps [`SessionFetcher`] from being implemented outside this crate.
mod private {
    pub trait Sealed {}
}

/// One running agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    agent: Agent,
    name: String,
    status: Status,
    started: SystemTime,
    context: Option<Context>,
    subagents: Vec<Subagent>,
}

/// A subagent a session started that is still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subagent {
    agent_type: String,
    description: String,
}

impl Subagent {
    /// Returns the kind of subagent, such as `general-purpose` or `Explore`.
    pub fn agent_type(&self) -> &str {
        &self.agent_type
    }

    /// Returns the short task description the session gave it.
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl Session {
    /// Returns the agent running this session.
    pub fn agent(&self) -> Agent {
        self.agent
    }

    /// Returns the session name, or its working directory's name if it has none.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns what the session is doing.
    pub fn status(&self) -> Status {
        self.status
    }

    /// Returns when the session started.
    pub fn started(&self) -> SystemTime {
        self.started
    }

    /// Returns how long the session has been running.
    pub fn uptime(&self) -> Duration {
        self.started.elapsed().unwrap_or_default()
    }

    /// Returns how much of its context window the session fills, or `None`
    /// before its first model reply.
    pub fn context(&self) -> Option<Context> {
        self.context
    }

    /// Returns the subagents the session started that are still running.
    ///
    /// Only Claude Code reports them; pi has no subagents, and Codex's are
    /// not read yet.
    pub fn subagents(&self) -> &[Subagent] {
        &self.subagents
    }
}

impl<'a> From<&'a Session> for Entry<'a> {
    fn from(session: &'a Session) -> Self {
        Entry::new(session.agent, session.status, &session.name)
            .with_uptime(session.uptime().as_secs())
            .with_context(session.context)
    }
}

impl<'a> From<&'a Subagent> for SubagentEntry<'a> {
    fn from(subagent: &'a Subagent) -> Self {
        SubagentEntry::new(&subagent.agent_type, &subagent.description)
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
fn run(program: &str, args: &[&str]) -> io::Result<String> {
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
fn processes() -> io::Result<Vec<Process>> {
    let output = run("ps", &["-axww", "-o", "pid=,etime=,args="])?;
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
    let output = run("lsof", &[args, &["-Fpn"]].concat())?;
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
