//! Claude Code writes `~/.claude/sessions/<pid>.json` for each running
//! session, with its name, status and start time. A crashed session can leave
//! its file behind, so only files whose PID is running count.
//!
//! Token usage comes from the session's transcript,
//! `~/.claude/projects/<project>/<session id>.jsonl`. Claude Code does not
//! record the context window, so it comes from [`WINDOWS`].
//!
//! Subagents are saved next to the transcript as
//! `<session id>/subagents/agent-<id>.jsonl`, each with an
//! `agent-<id>.meta.json` holding its type and description. The transcript
//! records when one finishes: a `<task-id>` notification for a background
//! subagent, a tool result with its `agentId` for a foreground one.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde_json::Value;

use crate::{
    Agent, Context, Sealed, Session, SessionFetcher, Status, Subagent, dir_name, home_path,
    processes,
};

/// Context window of Claude models by model ID prefix, from Anthropic's model
/// table of October 2026. Other models get no window.
// ponytail: hardcoded; add new models here, or read `max_input_tokens` from
// the Models API if keeping this current becomes a chore.
const WINDOWS: [(&str, u64); 10] = [
    ("claude-fable-5", 1_000_000),
    ("claude-mythos-5", 1_000_000),
    ("claude-opus-5", 1_000_000),
    ("claude-opus-4-8", 1_000_000),
    ("claude-opus-4-7", 1_000_000),
    ("claude-opus-4-6", 1_000_000),
    ("claude-sonnet-5", 1_000_000),
    ("claude-sonnet-4-6", 1_000_000),
    ("claude-haiku-5-5", 1_000_000),
    ("claude-haiku-4-5", 200_000),
];

/// How much of the end of a transcript to read for the last reply. Transcripts
/// grow to tens of megabytes; one entry rarely passes 150 KB.
// ponytail: a reply followed by more than this many bytes of entries shows no
// context until the next reply.
const TAIL_LEN: u64 = 1 << 20;

/// How recently a subagent must have written its transcript to count as
/// running. A working subagent writes at least this often, since its tool
/// calls time out after 10 minutes.
// ponytail: a subagent silent for longer counts as finished, and one whose
// finish was pushed out of the transcript tail shows as running until then.
const RECENT: Duration = Duration::from_secs(15 * 60);

/// Fetches the Claude Code sessions running on this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Claude;

impl Sealed for Claude {}

impl SessionFetcher for Claude {
    /// Returns the Claude Code sessions running on this machine.
    ///
    /// # Errors
    ///
    /// Returns an error if `HOME` is not set, the session directory exists but
    /// cannot be read, or `ps` cannot run.
    fn sessions(&self) -> io::Result<Vec<Session>> {
        let entries = match fs::read_dir(home_path(".claude/sessions")?) {
            Ok(entries) => entries,
            // No directory means Claude Code has not run on this machine.
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let running: HashSet<u32> = processes()?
            .into_iter()
            .map(|process| process.pid)
            .collect();
        let projects = home_path(".claude/projects")?;
        Ok(entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .filter_map(|path| session(&fs::read_to_string(path).ok()?, &running, &projects))
            .collect())
    }
}

/// Builds the session described by one session file.
///
/// Returns `None` if the text is not JSON, has no valid `pid`, or its process
/// is not in `running`.
fn session(json: &str, running: &HashSet<u32>, projects: &Path) -> Option<Session> {
    let v: Value = serde_json::from_str(json).ok()?;
    let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
    if !running.contains(&pid) {
        return None;
    }
    let name = v["name"].as_str().map_or_else(
        || dir_name(Path::new(v["cwd"].as_str().unwrap_or_default())),
        str::to_owned,
    );
    let status = if v["status"] == "busy" {
        Status::Busy
    } else {
        Status::Idle
    };
    let started =
        SystemTime::UNIX_EPOCH + Duration::from_millis(v["startedAt"].as_u64().unwrap_or(0));
    let transcript = v["sessionId"]
        .as_str()
        .and_then(|id| transcript(projects, id));
    let tail = transcript
        .as_deref()
        .and_then(|path| read_tail(path, TAIL_LEN).ok())
        .unwrap_or_default();
    Some(Session {
        agent: Agent::Claude,
        name,
        status,
        started,
        context: context(&tail),
        subagents: transcript
            .as_deref()
            .map(|path| subagents(path, &tail))
            .unwrap_or_default(),
    })
}

/// Returns the transcript of session `id`, from whichever project directory
/// holds it.
fn transcript(projects: &Path, id: &str) -> Option<PathBuf> {
    let file = format!("{id}.jsonl");
    fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|project| project.path().join(&file))
        .find(|path| path.is_file())
}

/// Returns the context of the last main-thread reply in `tail`, the end of a
/// transcript: its input, cache and output tokens.
///
/// Subagent replies and replies with no tokens (Claude Code's placeholders
/// for interrupted or failed requests) are skipped.
fn context(tail: &str) -> Option<Context> {
    tail.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|entry| {
            if entry["type"] != "assistant" || entry["isSidechain"] == true {
                return None;
            }
            let message = &entry["message"];
            let usage = &message["usage"];
            let used = [
                "input_tokens",
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
                "output_tokens",
            ]
            .iter()
            .filter_map(|field| usage[*field].as_u64())
            .sum::<u64>();
            (used > 0).then(|| Context::new(used, message["model"].as_str().and_then(window)))
        })
}

/// Returns the running subagents of the session whose transcript is at
/// `path`, given `tail`, the end of that transcript.
///
/// A subagent is running if it wrote its transcript within [`RECENT`] and
/// `tail` does not record it finishing.
// ponytail: only direct subagents are handled; a nested one (`spawnDepth`
// above 1) finishes in its parent subagent's transcript, not this one.
fn subagents(path: &Path, tail: &str) -> Vec<Subagent> {
    let dir = path.with_extension("").join("subagents");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name();
            let id = file
                .to_str()?
                .strip_prefix("agent-")?
                .strip_suffix(".meta.json")?;
            let written = dir
                .join(format!("agent-{id}.jsonl"))
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()?;
            if written.elapsed().unwrap_or_default() > RECENT || finished(tail, id) {
                return None;
            }
            let meta: Value = serde_json::from_str(&fs::read_to_string(entry.path()).ok()?).ok()?;
            Some(Subagent {
                agent_type: meta["agentType"].as_str().unwrap_or("?").to_owned(),
                description: meta["description"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// Returns whether `tail` records subagent `id` finishing: a `<task-id>`
/// notification, or a tool result for it other than a background launch.
fn finished(tail: &str, id: &str) -> bool {
    let notification = format!("<task-id>{id}</task-id>");
    tail.lines().filter(|line| line.contains(id)).any(|line| {
        line.contains(&notification)
            || serde_json::from_str::<Value>(line).is_ok_and(|entry| {
                let result = &entry["toolUseResult"];
                result["agentId"] == id && result["status"] != "async_launched"
            })
    })
}

/// Returns the context window of Claude model `model`, if it is in
/// [`WINDOWS`].
fn window(model: &str) -> Option<u64> {
    WINDOWS
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, window)| *window)
}

/// Returns the last `len` bytes of the file at `path`, without the partial
/// line they start in.
fn read_tail(path: &Path, len: u64) -> io::Result<String> {
    let mut file = File::open(path)?;
    let start = file.metadata()?.len().saturating_sub(len);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(match text.split_once('\n') {
        Some((_, rest)) if start > 0 => rest.to_owned(),
        _ => text.into_owned(),
    })
}
