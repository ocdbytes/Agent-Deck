# agent-deck

A desk display that shows the AI coding-agent sessions running on a Mac:
Claude Code, pi and Codex. The Mac collects the sessions and sends them over
the USB cable to an ESP32, which draws them on a 2.8" 240x320 SPI TFT LCD
(ILI9341).

For each session the deck gets:

- the agent (Claude Code, pi or Codex) and its name
- whether it is busy or idle
- how long it has been running
- how much of the model's context window it fills
- how many subagents it is running (Claude Code only)

## How it works

```text
Mac                                                  ESP32
───                                                  ─────
deck host ── finds sessions ─┐
                              ▼
link-lib  Encoder ── frame ── USB serial ──▶ link-lib Decoder
                                                     │
                                                     ▼
                                             deck (Deck scene) ──▶ LCD
```

The Mac sends the whole session list as one frame every poll. The deck keeps
the last frame that arrived intact, so a lost or damaged frame only delays
the next update.

## Crates

| Crate | Runs on | What it does |
| ----- | ------- | ------------ |
| [`link-lib`](link-lib/) | Mac and ESP32 (`no_std`) | The wire protocol: frame format, `Encoder` and `Decoder`. |
| [`deck`](deck/) | ESP32 and Mac | The display (`Deck`, a `frames::Scene`); with the `host` feature, the Mac side that finds running sessions and sends them (`agent-deck`), and a desktop preview. |
| [`firmware`](firmware/) | ESP32 | Reads frames from the USB cable and draws them with `Deck`. Built on its own, outside the workspace. |

`tools/assets` is a separate tool, outside the workspace, that renders the
deck's fonts and logos into bitmaps; see [`deck`](deck/#fonts-and-logos).

`deck` draws with [`frames`](https://github.com/ocdbytes/esp32-libs) from
esp32-libs, which runs the same scene on the panel and in a desktop window.

## Flash the deck

Wire the LCD and the two buttons as in [`firmware`](firmware/#wiring), plug
the ESP32 into the Mac, then from this directory:

```sh
scripts/flash.sh                          # pick the port when asked
scripts/flash.sh /dev/cu.usbserial-0001   # or name it
```

This builds the firmware, flashes it with `espflash`, and starts
`agent-deck`, which sends this Mac's sessions to the deck every second until
you stop it with Ctrl-C. To start sending again later without flashing:

```sh
cargo run -p deck --features host --bin agent-deck [-- PORT]
```

The ESP32 toolchain (`espup`, `espflash`) comes first; see the setup
chapter of [esp32-libs' book](https://github.com/ocdbytes/esp32-libs/tree/main/rust-esp32-book).

## Preview

Preview the deck in a desktop window with the sessions running on this Mac
(← and → turn the pages):

```sh
cargo run -p deck --example preview --features sim
```

The preview builds SDL2 from source, so it needs CMake.
`.cargo/config.toml` sets `CMAKE_POLICY_VERSION_MINIMUM` so CMake 4 accepts
SDL2's build files.

## Checks

```sh
cargo fmt --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --workspace --all-features
cargo +1.85 check --workspace --all-features          # minimum Rust version
cargo +esp check -p deck -p link-lib \
  --target xtensa-esp32-none-elf -Zbuild-std=core     # ESP32 build
```

The ESP32 check needs the `esp` toolchain; see the setup chapter of
[esp32-libs' book](https://github.com/ocdbytes/esp32-libs/tree/main/rust-esp32-book).

## Requirements

- Rust 1.85 or newer.
- macOS or Linux on the host: `deck`'s `host` feature runs `ps` and `lsof`.
- An ESP32 board, a 2.8" 240x320 ILI9341 SPI TFT and two 4-pin push buttons
  for the deck.

## Status

- The display follows the "sumi" dark design: a 2×2 grid of session tiles
  and a summary tile, a page at a time.
- The firmware and `agent-deck` build and run up to the hardware, but have
  not yet been tested on a board.
- Two buttons turn the pages: left (GPIO32) back, right (GPIO33) forward.
