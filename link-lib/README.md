# link-lib

The protocol that carries the session list from the Mac to the ESP32 over
the USB serial cable. It is `no_std`, so the firmware decodes with the same
code the Mac encodes with.

## Usage

On the Mac, build a frame with `Encoder`:

```rust
use link_lib::{Agent, Clock, Context, Encoder, Entry, MAX_FRAME_LEN, SessionColor, Status};

let mut buf = [0; MAX_FRAME_LEN];
let mut encoder = Encoder::new(&mut buf, Clock { hour: 18, minute: 25 });
encoder.push(&Entry {
    agent: Agent::Claude,
    status: Status::Busy,
    color: Some(SessionColor::Purple),
    name: "agent-deck",
    uptime: 3600,
    context: Some(Context { used: 250_000, window: Some(1_000_000) }),
    subagents: 2,
});
port.write_all(encoder.finish())?;
```

On the ESP32, feed each received byte to a `Decoder`:

```rust
use link_lib::Decoder;

let mut decoder = Decoder::new();
if let Some(frame) = decoder.push(byte) {
    let clock = frame.clock();
    for entry in frame.entries() {
        // entry.agent, .status, .color, .name, .uptime, .context, .subagents
    }
}
```

`push` drops a frame that is too long, damaged or malformed, then carries on
with the next frame.

## Frame format

A frame is one line of text:

```text
frame     = crc " " body "\n"
crc       = 4 lowercase hex digits: CRC-16/IBM-3740 of body
body      = clock US *(entry RS)
clock     = 4 decimal digits: local time as HHMM
entry     = agent status color US uptime US used US window US subagents US name
agent     = "c" (Claude Code) / "p" (pi) / "x" (Codex)
status    = "b" (busy) / "i" (idle)
color     = "r" / "o" / "y" / "g" / "c" / "b" / "p" / "k" / "-" (none)
uptime    = decimal seconds the session has run
used      = decimal context tokens in use; empty when unknown
window    = decimal context window in tokens; empty when unknown
subagents = decimal count of the session's running subagents
name      = up to MAX_TEXT_LEN bytes of UTF-8 without control characters
```

`RS` and `US` are the ASCII separator bytes `0x1E` and `0x1F`. For example,
with `␞` and `␟` standing for them:

```text
32d6 1825␟cbp␟3883␟434611␟1000000␟1␟Agent Deck Dev␞pi-␟1800␟␟␟0␟agent-deck␞
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
| `MAX_TEXT_LEN` | 32 | Bytes per session name; the encoder cuts at a character boundary and turns control characters into spaces |
| `MAX_FRAME_LEN` | 1947 | Largest possible frame, so both sides use a fixed buffer |
