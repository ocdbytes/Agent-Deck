//! pi keeps no list of running sessions and does not keep its transcript
//! open. This module finds `pi` processes, looks up each one's working
//! directory with `lsof`, and reads the most recently written transcript pi
//! saved for that directory under `~/.pi/agent/sessions/`. Context windows
//! come from pi's model list, `~/.pi/agent/models-store.json`.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde_json::Value;

use crate::{
    Agent, Context, Sealed, Session, SessionFetcher, Status, dir_name, home_path, lsof, processes,
};

/// Fetches the pi sessions running on this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pi;

impl Sealed for Pi {}

impl SessionFetcher for Pi {
    /// Returns the pi sessions running on this machine.
    ///
    /// # Errors
    ///
    /// Returns an error if `HOME` is not set or `ps` or `lsof` cannot run.
    fn sessions(&self) -> io::Result<Vec<Session>> {
        let started: HashMap<u32, SystemTime> = processes()?
            .into_iter()
            .filter(|process| is_pi(&process.args))
            .map(|process| (process.pid, process.started))
            .collect();
        if started.is_empty() {
            return Ok(Vec::new());
        }
        let pids: Vec<String> = started.keys().map(u32::to_string).collect();
        let root = home_path(".pi/agent/sessions")?;
        // The model list only adds context windows, so a missing one is fine.
        let models: Value = fs::read_to_string(home_path(".pi/agent/models-store.json")?)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Ok(lsof(&["-a", "-d", "cwd", "-p", &pids.join(",")])?
            .into_iter()
            .filter_map(|(pid, cwd)| Some(session(&root, &cwd, *started.get(&pid)?, &models)))
            .collect())
    }
}

/// Returns whether `args` is a pi command line.
///
/// pi sets its process title to `pi`; without that it shows as
/// `node .../bin/pi`.
fn is_pi(args: &str) -> bool {
    args.split_whitespace()
        .take(2)
        .any(|arg| Path::new(arg).file_name().is_some_and(|name| name == "pi"))
}

/// Builds the session of a pi process that started at `started` in `cwd`,
/// looking up context windows in pi's model list `models`.
///
/// pi creates a run's transcript on its first message, so a transcript last
/// written before the process started belongs to an earlier run and is
/// ignored.
// ponytail: two pi processes in one directory show the same transcript.
fn session(root: &Path, cwd: &Path, started: SystemTime, models: &Value) -> Session {
    let transcript = latest_transcript(&root.join(session_dir(cwd)))
        .filter(|(_, modified)| *modified >= started)
        .and_then(|(path, _)| fs::read_to_string(path).ok())
        .map(|text| read_transcript(&text))
        .unwrap_or_default();
    let context = transcript
        .reply
        .map(|reply| Context::new(reply.tokens, window(models, &reply.provider, &reply.model)));
    Session {
        agent: Agent::Pi,
        name: transcript.name.unwrap_or_else(|| dir_name(cwd)),
        status: transcript.status,
        started,
        context,
        subagents: Vec::new(),
    }
}

/// Returns the context window pi lists for `model` from `provider`.
fn window(models: &Value, provider: &str, model: &str) -> Option<u64> {
    models[provider]["models"]
        .as_array()?
        .iter()
        .find(|entry| entry["id"] == model)?["contextWindow"]
        .as_u64()
}

/// Returns the directory name pi stores `cwd`'s sessions under: the path
/// without its leading separator, with `/`, `\` and `:` replaced by `-`, and
/// wrapped in `--`.
fn session_dir(cwd: &Path) -> String {
    let cwd = cwd.to_string_lossy();
    let cwd = cwd.strip_prefix(['/', '\\']).unwrap_or(&cwd);
    format!("--{}--", cwd.replace(['/', '\\', ':'], "-"))
}

/// Returns the most recently written transcript in `dir` and when it was
/// last written.
fn latest_transcript(dir: &Path) -> Option<(PathBuf, SystemTime)> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| {
            let modified = path.metadata().and_then(|meta| meta.modified()).ok()?;
            Some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
}

/// What a pi transcript says about its session.
#[derive(Debug, Default)]
struct Transcript {
    /// From the last `session_info` entry.
    name: Option<String>,
    /// Idle when the last message is from the assistant and did not stop to
    /// call a tool.
    status: Status,
    /// The last reply that counts towards the context.
    reply: Option<Reply>,
}

/// The tokens of one pi reply and the model that wrote it.
#[derive(Debug)]
struct Reply {
    tokens: u64,
    provider: String,
    model: String,
}

/// Reads a pi transcript (JSON lines).
// ponytail: parses the whole transcript on every call; read from the end if
// large transcripts make polling slow.
fn read_transcript(text: &str) -> Transcript {
    let mut transcript = Transcript::default();
    for entry in text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
    {
        match entry["type"].as_str() {
            Some("session_info") => {
                transcript.name = entry["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned);
            }
            Some("message") => {
                let message = &entry["message"];
                transcript.status = match message["role"].as_str() {
                    Some("assistant") if message["stopReason"] != "toolUse" => Status::Idle,
                    Some("assistant" | "user" | "toolResult") => Status::Busy,
                    // System prompt entries say nothing about the run.
                    _ => transcript.status,
                };
                if let Some(reply) = reply(message) {
                    transcript.reply = Some(reply);
                }
            }
            _ => {}
        }
    }
    transcript
}

/// Returns the tokens and model of an assistant message, counted the way pi
/// counts context: the reported total, or else the sum of input, output and
/// cache tokens.
///
/// Returns `None` for other messages, and for aborted, failed or empty
/// replies, which pi skips too.
fn reply(message: &Value) -> Option<Reply> {
    if message["role"] != "assistant"
        || message["stopReason"] == "aborted"
        || message["stopReason"] == "error"
    {
        return None;
    }
    let usage = &message["usage"];
    let tokens = match usage["totalTokens"].as_u64() {
        Some(total) if total > 0 => total,
        _ => ["input", "output", "cacheRead", "cacheWrite"]
            .iter()
            .filter_map(|field| usage[*field].as_u64())
            .sum(),
    };
    (tokens > 0).then(|| Reply {
        tokens,
        provider: message["provider"].as_str().unwrap_or_default().to_owned(),
        model: message["model"].as_str().unwrap_or_default().to_owned(),
    })
}
