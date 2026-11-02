# link-lib

The protocol that carries the session list from the Mac to the ESP32 over
the USB serial cable. It is `no_std`, so the firmware decodes with the same
code the Mac encodes with.

## Usage

On the Mac, build a frame with `Encoder`:

```rust
use link_lib::{Agent, Context, Encoder, Entry, MAX_FRAME_LEN, Status, SubagentEntry};

let mut buf = [0; MAX_FRAME_LEN];
let mut encoder = Encoder::new(&mut buf);
let entry = Entry::new(Agent::Claude, Status::Busy, "agent-deck")
    .with_uptime(3600)
    .with_context(Some(Context::new(250_000, Some(1_000_000))));
encoder.push(&entry, [SubagentEntry::new("Explore", "Read the docs")]);
port.write_all(encoder.finish())?;
```

On the ESP32, feed each received byte to a `Decoder`:

```rust
use link_lib::Decoder;

let mut decoder = Decoder::new();
if let Some(Ok(frame)) = decoder.push(byte) {
    for entry in frame.entries() {
        // entry.agent(), .status(), .name(), .uptime(), .context()
        for subagent in entry.subagents() { /* .agent_type(), .description() */ }
    }
}
```

`push` returns an `Error` for a frame that is too long (`TooLong`), damaged
(`Checksum`) or malformed (`Malformed`), then carries on with the next frame.

## Frame format

A frame is one line of text:

```text
frame    = crc " " body "\n"
crc      = 4 lowercase hex digits: CRC-16/IBM-3740 of body
body     = *(entry RS)
entry    = agent status US uptime US used US window US name *(US subagent)
subagent = text GS text            ; the subagent's type, then its description
agent    = "c" (Claude Code) / "p" (pi) / "x" (Codex)
status   = "b" (busy) / "i" (idle)
uptime   = decimal seconds the session has run
used     = decimal context tokens in use; empty when unknown
window   = decimal context window in tokens; empty when unknown
name     = text
text     = up to MAX_TEXT_LEN bytes of UTF-8 without control characters
```

`RS`, `US` and `GS` are the ASCII separator bytes `0x1E`, `0x1F` and `0x1D`.
For example, with `␞`, `␟` and `␝` standing for them:

```text
70ea cb␟3883␟434611␟1000000␟Agent Deck Dev␟Explore␝Summarize esp32 book chapters␞pi␟1800␟␟␟agent-deck␞
```

- **Checksum:** frames that fail it are dropped.
- **Recovery:** a newline ends every frame, so the decoder recovers at the
  next line, including after a half-received frame at power-up.
- **Resending:** the Mac sends the full list every poll, so a dropped frame
  only delays the next update.

## Limits

| Constant | Value | Meaning |
| -------- | ----- | ------- |
| `MAX_SESSIONS` | 16 | Sessions per frame; the encoder drops the rest |
| `MAX_SUBAGENTS` | 4 | Subagents per session; the encoder drops the rest |
| `MAX_TEXT_LEN` | 32 | Bytes per text; the encoder cuts at a character boundary and turns control characters into spaces |
| `MAX_FRAME_LEN` | 5814 | Largest possible frame, so both sides use a fixed buffer |
