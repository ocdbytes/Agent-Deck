//! Codex holds `~/.codex/thread-writer-locks/<thread id>.lock` open while a
//! thread runs, so the lock files open in that directory are the running
//! sessions, and each lock was created when its session started.
//!
//! From its first prompt on, a thread is saved to
//! `~/.codex/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`, which
//! gives its status, token usage, context window and working directory.
//! Thread names come from `~/.codex/session_index.jsonl`.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::{Agent, Context, Session, Status, created, dir_name, home_path, lsof};

/// Returns the Codex sessions running on this machine.
///
/// # Errors
///
/// Returns an error if `HOME` is not set or `lsof` cannot run.
pub(super) fn sessions() -> io::Result<Vec<Session>> {
    let locks = home_path(".codex/thread-writer-locks")?;
    let mut threads: Vec<(u32, PathBuf)> = lsof(&["+d", &locks.to_string_lossy()])?
        .into_iter()
        .filter(|(_, path)| is_thread_lock(path))
        .collect();
    if threads.is_empty() {
        return Ok(Vec::new());
    }
    // A process can hold one file open more than once.
    threads.sort();
    threads.dedup();
    let rollouts = home_path(".codex/sessions")?;
    let names = thread_names(&home_path(".codex/session_index.jsonl")?);
    Ok(threads
        .iter()
        .map(|(pid, lock)| session(*pid, lock, &rollouts, &names))
        .collect())
}

/// Returns whether `path` is a thread's lock, not Codex's own
/// `.coordination.lock`.
fn is_thread_lock(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "lock")
        && path
            .file_name()
            .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
}

/// Maps thread IDs to their names in Codex's session index. Later lines win.
fn thread_names(index: &Path) -> HashMap<String, String> {
    // Names are optional, so a missing or unreadable index gives none.
    let text = fs::read_to_string(index).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            Some((
                v["id"].as_str()?.to_owned(),
                v["thread_name"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// Builds the session of the thread whose `lock` process `pid` holds.
///
/// Before the thread's first prompt there is no rollout yet: the session is
/// idle, has no context, and is named after the process's working directory.
fn session(pid: u32, lock: &Path, rollouts: &Path, names: &HashMap<String, String>) -> Session {
    let id = lock.file_stem().unwrap_or_default().to_string_lossy();
    let rollout = find_rollout(rollouts, &format!("-{id}.jsonl"))
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| read_rollout(&text))
        .unwrap_or_default();
    let name = names
        .get(id.as_ref())
        .cloned()
        .or_else(|| rollout.cwd.as_deref().map(dir_name))
        .or_else(|| process_cwd(pid).as_deref().map(dir_name))
        .unwrap_or_else(|| "?".to_owned());
    Session {
        agent: Agent::Codex,
        name,
        status: rollout.status,
        started: created(lock),
        context: rollout.context,
        subagents: 0,
    }
}

/// Returns the working directory of process `pid`.
fn process_cwd(pid: u32) -> Option<PathBuf> {
    let (_, cwd) = lsof(&["-a", "-d", "cwd", "-p", &pid.to_string()])
        .ok()?
        .into_iter()
        .next()?;
    Some(cwd)
}

/// Returns the rollout under `dir` whose file name ends with `suffix`,
/// searching the newest `YYYY/MM/DD` directories first.
// ponytail: walks the whole tree for a thread with no rollout yet; look only
// at dates from the lock's creation on if that gets slow.
fn find_rollout(dir: &Path, suffix: &str) -> Option<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    paths.sort_unstable_by(|a, b| b.cmp(a));
    paths.into_iter().find_map(|path| {
        if path.is_dir() {
            find_rollout(&path, suffix)
        } else {
            path.to_string_lossy().ends_with(suffix).then_some(path)
        }
    })
}

/// What a rollout says about its thread.
#[derive(Debug, Default)]
struct Rollout {
    /// From the `session_meta` entry.
    cwd: Option<PathBuf>,
    /// Busy after a `task_started` event until a `task_complete` or
    /// `turn_aborted` event.
    status: Status,
    /// The last request of the last `token_count` event.
    context: Option<Context>,
}

/// Reads a rollout (JSON lines).
// ponytail: parses the whole rollout on every call; read from the end if
// large rollouts make polling slow.
fn read_rollout(text: &str) -> Rollout {
    let mut rollout = Rollout::default();
    for entry in text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
    {
        let payload = &entry["payload"];
        match entry["type"].as_str() {
            Some("session_meta") => rollout.cwd = payload["cwd"].as_str().map(PathBuf::from),
            Some("event_msg") => match payload["type"].as_str() {
                Some("task_started") => rollout.status = Status::Busy,
                Some("task_complete" | "turn_aborted") => rollout.status = Status::Idle,
                Some("token_count") => {
                    let info = &payload["info"];
                    if let Some(used) = info["last_token_usage"]["total_tokens"].as_u64() {
                        rollout.context = Some(Context {
                            used,
                            window: info["model_context_window"].as_u64(),
                        });
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    rollout
}
