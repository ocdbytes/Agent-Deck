# deck

The display and Mac host for agent-deck. It shows Claude Code, pi and Codex
sessions on the ESP32 panel or in a desktop preview.

## Features

| Feature | Purpose |
| ------- | ------- |
| default | `no_std` display code for ESP32 and desktop |
| `host` | Mac session collection and the `agent-deck` serial binary |
| `sim` | Desktop preview using SDL2 |

## Run the host

Send this Mac's sessions to the deck over USB:

```sh
cargo run -p deck --features host --bin agent-deck [-- PORT]
```

If `PORT` is omitted, the binary uses the only supported USB serial port it
finds in `/dev`. See the [project README](../README.md) for flashing and
hardware requirements.

## Preview

Open a desktop preview; use ← and → to change pages:

```sh
cargo run -p deck --example preview --features sim
```

Save one frame as a PNG instead:

```sh
EG_SIMULATOR_DUMP=deck.png cargo run -p deck --example preview --features sim
```

## Assets

Fonts and logos live in `assets/`. The pets shown for pi and Codex are
OpenAI's artwork, so they are not in git: the asset tool downloads their
sprite sheets into the ignored `assets/gen/pets.bin`. Run it
once after cloning, and after changing a font, logo or the generated
character set; it also rebuilds `src/assets.rs` and the project README's
banner, `docs/banner.png`:

```sh
cargo run --manifest-path tools/assets/Cargo.toml
```

Font licenses are stored beside the fonts in `assets/fonts/`. Logos, Claude
Code's mascot and OpenAI's pets belong to their owners.
