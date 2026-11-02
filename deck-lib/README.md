# deck-lib

Finds the AI coding-agent sessions running on this machine: Claude Code, pi
and Codex. Runs on the Mac (macOS or Linux), since it reads the agents' files
and runs `ps` and `lsof`.

## Usage

Each agent has a fetcher that implements the `SessionFetcher` trait:

```rust
use deck_lib::{Claude, Codex, Pi, SessionFetcher};

let fetchers: [&dyn SessionFetcher; 3] = [&Claude, &Pi, &Codex];
for fetcher in fetchers {
    for session in fetcher.sessions()? {
        println!("{} {} {}", session.agent(), session.status(), session.name());
    }
}
```

The trait is sealed: only this crate can build a `Session`.

A `Session` gives:

| Method | Returns |
| ------ | ------- |
| `agent()` | `Claude`, `Pi` or `Codex` |
| `name()` | The session name, or its working directory's name |
| `status()` | `Busy` or `Idle` |
| `started()`, `uptime()` | When the session started, and how long ago |
| `context()` | Tokens of the last model request and reply, and the context window if known; `None` before the first reply |
| `subagents()` | Running subagents: type and description |

`Agent`, `Status` and `Context` come from [`link-lib`](../link-lib/), so they
are the same types the deck receives. A `Session` converts to a `link-lib`
`Entry`, and a `Subagent` to a `SubagentEntry`, with `From`.

## Where the data comes from

All of these files are internal to the agents and can change between
versions.

| | Claude Code | pi | Codex |
| - | ----------- | -- | ----- |
| Running sessions | `~/.claude/sessions/<pid>.json` whose process is running | `pi` processes, their working directory (`lsof`), and the newest transcript in `~/.pi/agent/sessions/--<cwd>--/` written since the process started | Lock files held open in `~/.codex/thread-writer-locks/<thread id>.lock` |
| Name | `name` in the session file | Last `session_info` entry, else the directory name | `~/.codex/session_index.jsonl`, else the working directory's name |
| Status | `status` in the session file | Last message: idle after an assistant reply that did not call a tool | Rollout events: busy from `task_started` to `task_complete` or `turn_aborted` |
| Start time | `startedAt` in the session file | Process start time (`ps`) | Lock file creation time |
| Context used | Last main-thread reply in the last 1 MiB of `~/.claude/projects/*/<session id>.jsonl` | Last reply, counted the way pi does | Last `token_count` event in the rollout |
| Context window | Table of model ID prefixes in `claude.rs` | `~/.pi/agent/models-store.json` | `model_context_window` in the rollout |
| Subagents | `<session id>/subagents/`, see below | pi has none | Not read yet |

A Claude Code subagent counts as running if it wrote its transcript in the
last 15 minutes and the session transcript's last 1 MiB has no finish record
for it (a `<task-id>` notification, or a tool result with its `agentId`).

## Known limits

These are marked with `ponytail:` comments in the code.

- **Claude Code:** context windows are a hardcoded table; a new model shows
  its token count with an unknown window until it is added.
- **Claude Code subagents:** one silent for over 15 minutes counts as
  finished, and nested subagents are not handled.
- **pi:** two pi processes in the same directory show the same transcript.
- **Codex:** a thread's rollout is found by walking `~/.codex/sessions`,
  newest dates first. Codex subagents are not read yet.
- **pi and Codex:** each poll parses the whole transcript.
