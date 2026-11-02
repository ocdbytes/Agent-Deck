# deck

The agent deck itself: the display that draws the session list on a 2.8"
240x320 color LCD, and the Mac loop that feeds it.

## Features

The Mac side needs `std` and can't build for the ESP32, so it is behind
features:

| Build | Contains | Builds for |
| ----- | -------- | ---------- |
| default | `Deck`, `SIZE` (`no_std`) | ESP32 and desktop |
| `--features host` | Adds `host::frame`, `host::run` and the `agent-deck` binary (uses `serde_json` and `serialport`) | Mac |
| `--features sim` | Adds the desktop preview example (builds SDL2 with CMake) | Mac |

## The display

`Deck` is a [`frames::Scene`](https://github.com/ocdbytes/esp32-libs) with
Rgb565 colors. You give it a function that returns the next byte received
from the Mac, or `None` when no byte is waiting; it must not block. Each
frame, `Deck` decodes the waiting bytes with `link-lib` and keeps the
sessions of the last intact frame.

```rust
let mut deck = Deck::new(|| uart.read_byte());   // non-blocking read
loop {
    deck.frame(&mut framebuffer)?;                // decode, clear, draw
    // send `framebuffer` to the panel, then wait for the next frame
}
```

### Layout

The panel is a 2×2 grid of 120x160 tiles. Three tiles per page show
sessions, and the fourth is the summary.

- **Session tile:** the agent's logo, its running subagents at the top
  right, `● 稼働 busy` in green or `○ 待機 idle`
  in grey, the name in `「」` (cut with `…` if long), the percentage of the
  context window in big pixel numerals with a bar under it, and `ctx` (tokens
  used / window) and `up` (uptime) rows. From 80 % context
  (`WARN_PERCENT`) the percentage and bar turn vermilion. Subagents show
  as one small logo each, up to 3 (`MAX_SUBAGENT_LOGOS`); more show as one
  small logo and a count, such as `x5`.
- **Summary tile:** `今 now` with the page (`1/3`), the number of busy and
  idle sessions, and `tok`, the context tokens of all sessions together.

`next_page()` and `prev_page()` turn the pages; on the panel, wire them to
buttons.

### Fonts and logos

| Use | Asset |
| --- | ----- |
| Body text | IBM Plex Mono at 35/3 px (7 px wide) |
| Big numerals, `%` | DotGothic16 at 32 px and 16 px |
| Kanji | [Ark Pixel](https://github.com/TakWolf/ark-pixel-font) 12px Japanese, drawn on a 12 px grid so dense kanji stay sharp |
| Logos | pi's official logo from the [pi.dev press kit](https://pi.dev/press-kit), in its own colors; Claude and OpenAI from [Simple Icons](https://simpleicons.org) 15.22.0, tinted orange and white |

The fonts and SVGs live in `assets/`. `tools/assets` renders them into
`assets/gen/` and writes `src/assets.rs`:

- **Glyphs:** 4-bit coverage bitmaps that the deck blends over the black
  background, so text is anti-aliased.
- **Logos:** 32x32 pixel art in RGB565, and 8x8 copies for subagents
  (`SMALL_LOGO_SIZE` in `tools/assets`): each pixel is off or fully on, so
  edges stay sharp on the panel. A pixel is on when at least 35% of it is
  covered, measured at 8× resolution, which keeps thin strokes unbroken.
  Each logo's shapes are scaled to fill the square, whatever margin its SVG
  has.

Together they take about 13 KB of flash. Logo tints are set in
`tools/assets`. After changing a font, size,
character set or logo, run:

```sh
cargo run --manifest-path tools/assets/Cargo.toml
```

The Ark Pixel file is cut down to the kanji the deck shows (`稼働待機今`),
from 5 MB to 2 KB. To add kanji, take `ark-pixel-12px-monospaced-ja.ttf`
from its [release](https://github.com/TakWolf/ark-pixel-font/releases)
(the `12px-monospaced-ttf` zip) and cut it again with fontTools:

```sh
python3 -m fontTools.subset ark-pixel-12px-monospaced-ja.ttf --text='稼働待機今' \
  --output-file=assets/fonts/arkpixel/ark-pixel-12px-monospaced-ja.ttf
```

Then add the kanji to `KANJI` in `tools/assets` and rerun it.

All three fonts are under the SIL Open Font License; their license files are
next to them in `assets/fonts/`. The logos are trademarks of their owners.

## The Mac side

- **`host::frame(&mut buf)`:** fetches every agent's sessions (oldest first)
  and encodes them, with their running subagent counts, into one frame.
- **`host::run(port, interval)`:** the main loop. It writes a fresh frame to
  `port` every `interval`, forever. `port` is the deck's serial port,
  already open at the firmware's baud rate.
- **`agent-deck [PORT]`:** the binary that does this over the USB cable,
  at 115200 baud, every second:

  ```sh
  cargo run -p deck --features host --bin agent-deck [-- PORT]
  ```

  Without `PORT` it uses the only USB serial port in `/dev` (CP210x, CH340
  or native USB). It releases DTR and RTS after opening the port, so the
  board's auto-reset circuit lets the ESP32 run.

### Where the data comes from

`host` has one module per agent. All of these files are internal to the
agents and can change between versions.

| | Claude Code | pi | Codex |
| - | ----------- | -- | ----- |
| Running sessions | `~/.claude/sessions/<pid>.json` whose process is running | `pi` processes, their working directory (`lsof`), and the newest transcript in `~/.pi/agent/sessions/--<cwd>--/` written since the process started | Lock files held open in `~/.codex/thread-writer-locks/<thread id>.lock` |
| Name | `name` in the session file | Last `session_info` entry, else the directory name | `~/.codex/session_index.jsonl`, else the working directory's name |
| Status | `status` in the session file | Last message: idle after an assistant reply that did not call a tool | Rollout events: busy from `task_started` to `task_complete` or `turn_aborted` |
| Start time | `startedAt` in the session file | Process start time (`ps`) | Lock file creation time |
| Context used | Last main-thread reply in the last 1 MiB of `~/.claude/projects/*/<session id>.jsonl` | Last reply, counted the way pi does | Last `token_count` event in the rollout |
| Context window | Table of model ID prefixes in `host/claude.rs` | `~/.pi/agent/models-store.json` | `model_context_window` in the rollout |
| Subagents | `<session id>/subagents/`, see below | pi has none | Not read yet |

A Claude Code subagent counts as running if it wrote its transcript in the
last 15 minutes and the session transcript's last 1 MiB has no finish record
for it (a `<task-id>` notification, or a tool result with its `agentId`).

### Known limits

These are marked with `ponytail:` comments in the code.

- **Claude Code:** context windows are a hardcoded table; a new model shows
  its token count with an unknown window until it is added.
- **Claude Code subagents:** one silent for over 15 minutes counts as
  finished, and nested subagents are not handled.
- **pi:** two pi processes in the same directory show the same transcript.
- **Codex:** a thread's rollout is found by walking `~/.codex/sessions`,
  newest dates first. Codex subagents are not read yet.
- **pi and Codex:** each poll parses the whole transcript.

## Preview

```sh
cargo run -p deck --example preview --features sim
```

This opens a window that looks like the panel and shows the sessions running
on this Mac, refreshed every second; ← and → turn the pages. The bytes go
through the same `link-lib` encoder and decoder as on the cable. To save one
frame as a PNG instead of opening a window:

```sh
EG_SIMULATOR_DUMP=deck.png cargo run -p deck --example preview --features sim
```
