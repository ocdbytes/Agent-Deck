//! The protocol that carries the session list from the Mac to the ESP32 over
//! the USB serial cable.
//!
//! The Mac sends the whole list as one frame each time it polls. The deck
//! replaces its list with every frame that arrives intact, so a frame lost or
//! damaged on the cable only delays the next update.
//!
//! The crate is `no_std` so the firmware decodes with the same code the Mac
//! encodes with: [`Encoder`] on the Mac, [`Decoder`] on the ESP32.
//!
//! # Frame format
//!
//! A frame is one line of text:
//!
//! ```text
//! frame     = crc " " body "\n"
//! crc       = 4 lowercase hex digits: CRC-16/IBM-3740 of body
//! body      = *(entry RS)
//! entry     = agent status US uptime US used US window US subagents US name
//! agent     = "c" (Claude Code) / "p" (pi) / "x" (Codex)
//! status    = "b" (busy) / "i" (idle)
//! uptime    = decimal seconds the session has run
//! used      = decimal context tokens in use; empty when unknown
//! window    = decimal context window in tokens; empty when unknown
//! subagents = decimal count of the session's running subagents
//! name      = up to MAX_TEXT_LEN bytes of UTF-8 without control characters
//! ```
//!
//! `RS` and `US` are the ASCII separator bytes 0x1E and 0x1F.
//!
//! A frame holds at most [`MAX_SESSIONS`] entries, so it is at most
//! [`MAX_FRAME_LEN`] bytes and the deck receives it into a fixed buffer.

#![no_std]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

use core::{mem, str};

/// Most sessions one frame carries.
pub const MAX_SESSIONS: usize = 16;

/// Longest session name in a frame, in bytes.
pub const MAX_TEXT_LEN: usize = 32;

/// Longest frame, in bytes, including the closing newline.
pub const MAX_FRAME_LEN: usize = BODY_START + MAX_SESSIONS * MAX_ENTRY_LEN + 1;

/// Longest entry, in bytes: the two codes, four numbers and the name with
/// their separators, and the closing `RS`.
const MAX_ENTRY_LEN: usize = 2 + 4 * (1 + MAX_DIGITS) + 1 + MAX_TEXT_LEN + 1;

/// Most digits of a number in a frame: `u64::MAX` has 20.
const MAX_DIGITS: usize = 20;

/// Where the body starts: after the 4 checksum digits and a space.
const BODY_START: usize = 5;

/// Ends each entry in a frame body (ASCII record separator).
const RS: u8 = 0x1E;

/// Separates the fields of an entry (ASCII unit separator).
const US: u8 = 0x1F;

/// A coding agent whose sessions the deck shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Agent {
    /// Claude Code.
    Claude,
    /// The pi coding agent.
    Pi,
    /// OpenAI Codex.
    Codex,
}

impl Agent {
    /// Returns the byte that stands for this agent in a frame.
    const fn code(self) -> u8 {
        match self {
            Self::Claude => b'c',
            Self::Pi => b'p',
            Self::Codex => b'x',
        }
    }

    /// Returns the agent a frame byte stands for.
    const fn from_code(code: u8) -> Option<Self> {
        match code {
            b'c' => Some(Self::Claude),
            b'p' => Some(Self::Pi),
            b'x' => Some(Self::Codex),
            _ => None,
        }
    }
}

/// What a session is doing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Status {
    /// The agent is working on a request.
    Busy,
    /// The agent is waiting for input.
    #[default]
    Idle,
}

impl Status {
    /// Returns the lowercase name shown for this status.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Idle => "idle",
        }
    }

    /// Returns the byte that stands for this status in a frame.
    const fn code(self) -> u8 {
        match self {
            Self::Busy => b'b',
            Self::Idle => b'i',
        }
    }

    /// Returns the status a frame byte stands for.
    const fn from_code(code: u8) -> Option<Self> {
        match code {
            b'b' => Some(Self::Busy),
            b'i' => Some(Self::Idle),
            _ => None,
        }
    }
}

/// How much of its model's context window a session fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// Tokens of the session's last model request and its reply, which is
    /// what the next request starts from.
    pub used: u64,
    /// The model's context window in tokens, or `None` if unknown.
    pub window: Option<u64>,
}

/// One session as sent over the cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The agent running the session.
    pub agent: Agent,
    /// What the session is doing.
    pub status: Status,
    /// The session name. [`Encoder::push`] cuts it to [`MAX_TEXT_LEN`] bytes
    /// at a character boundary and turns control characters into spaces.
    pub name: &'a str,
    /// How long the session has run, in seconds.
    pub uptime: u64,
    /// How much of its context window the session fills, or `None` before
    /// its first model reply.
    pub context: Option<Context>,
    /// How many subagents the session is running.
    pub subagents: usize,
}

impl<'a> Entry<'a> {
    /// Parses one entry of a received frame body.
    ///
    /// Returns `None` if a field is missing, a code or number is invalid, or
    /// the name breaks the format.
    fn parse(entry: &'a str) -> Option<Self> {
        let mut fields = entry.splitn(6, char::from(US));
        let [agent, status] = *fields.next()?.as_bytes() else {
            return None;
        };
        let uptime = fields.next()?.parse().ok()?;
        let context = match (fields.next()?, fields.next()?) {
            ("", "") => None,
            (used, "") => Some(Context {
                used: used.parse().ok()?,
                window: None,
            }),
            (used, window) => Some(Context {
                used: used.parse().ok()?,
                window: Some(window.parse().ok()?),
            }),
        };
        let subagents = fields.next()?.parse().ok()?;
        let name = fields
            .next()
            .filter(|name| name.len() <= MAX_TEXT_LEN && !name.chars().any(char::is_control))?;
        Some(Self {
            agent: Agent::from_code(agent)?,
            status: Status::from_code(status)?,
            name,
            uptime,
            context,
            subagents,
        })
    }
}

/// Writes a frame into a buffer, one session at a time.
#[derive(Debug)]
pub struct Encoder<'b> {
    buf: &'b mut [u8; MAX_FRAME_LEN],
    len: usize,
    sessions: usize,
}

impl<'b> Encoder<'b> {
    /// Starts an empty frame in `buf`.
    pub fn new(buf: &'b mut [u8; MAX_FRAME_LEN]) -> Self {
        Self {
            buf,
            len: BODY_START,
            sessions: 0,
        }
    }

    /// Adds a session. Sessions after the first [`MAX_SESSIONS`] are
    /// dropped.
    pub fn push(&mut self, entry: &Entry<'_>) {
        if self.sessions == MAX_SESSIONS {
            return;
        }
        self.sessions += 1;
        self.put(entry.agent.code());
        self.put(entry.status.code());
        self.put(US);
        self.put_number(entry.uptime);
        self.put(US);
        if let Some(context) = entry.context {
            self.put_number(context.used);
        }
        self.put(US);
        if let Some(window) = entry.context.and_then(|context| context.window) {
            self.put_number(window);
        }
        self.put(US);
        self.put_number(entry.subagents as u64);
        self.put(US);
        self.put_text(entry.name);
        self.put(RS);
    }

    /// Finishes the frame and returns it.
    pub fn finish(self) -> &'b [u8] {
        let Self { buf, len, .. } = self;
        let crc = crc16(&buf[BODY_START..len]);
        for (i, digit) in buf[..4].iter_mut().enumerate() {
            *digit = b"0123456789abcdef"[usize::from(crc >> (12 - 4 * i) & 0xF)];
        }
        buf[4] = b' ';
        buf[len] = b'\n';
        &buf[..=len]
    }

    // invariant: `push` writes at most MAX_ENTRY_LEN bytes per session and at
    // most MAX_SESSIONS sessions, which MAX_FRAME_LEN fits with the checksum
    // and newline, so the indexing in these writers stays in bounds.

    /// Writes one byte.
    fn put(&mut self, byte: u8) {
        self.buf[self.len] = byte;
        self.len += 1;
    }

    /// Writes `number` in decimal.
    fn put_number(&mut self, mut number: u64) {
        let mut digits = [0; MAX_DIGITS];
        let mut start = MAX_DIGITS;
        loop {
            start -= 1;
            digits[start] = b'0' + (number % 10) as u8;
            number /= 10;
            if number == 0 {
                break;
            }
        }
        for &digit in &digits[start..] {
            self.put(digit);
        }
    }

    /// Writes `text`, cut to MAX_TEXT_LEN bytes with control characters as
    /// spaces.
    fn put_text(&mut self, text: &str) {
        let mut text_len = 0;
        for c in text.chars() {
            let c = if c.is_control() { ' ' } else { c };
            text_len += c.len_utf8();
            if text_len > MAX_TEXT_LEN {
                break;
            }
            self.len += c.encode_utf8(&mut self.buf[self.len..]).len();
        }
    }
}

/// A frame that arrived intact.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    body: &'a str,
}

impl<'a> Frame<'a> {
    /// Returns the sessions in this frame, in the order they were sent.
    pub fn entries(self) -> impl Iterator<Item = Entry<'a>> {
        // `parse` checked every entry, so none is skipped here.
        self.body
            .split_terminator(char::from(RS))
            .filter_map(Entry::parse)
    }

    /// Checks one received line (without its newline) and wraps it.
    ///
    /// Returns `None` if the line does not follow the format or its checksum
    /// does not match: bytes were lost or changed on the cable.
    fn parse(line: &'a [u8]) -> Option<Self> {
        let (head, body) = line.split_at_checked(BODY_START)?;
        let [hex @ .., b' '] = head else {
            return None;
        };
        let crc = u16::from_str_radix(str::from_utf8(hex).ok()?, 16).ok()?;
        if crc16(body) != crc {
            return None;
        }
        let body = str::from_utf8(body).ok()?;
        let mut entries = body.split_terminator(char::from(RS));
        let valid = entries.clone().count() <= MAX_SESSIONS
            && entries.all(|entry| Entry::parse(entry).is_some());
        valid.then_some(Self { body })
    }
}

/// Splits the bytes received from the cable into frames.
#[derive(Debug)]
pub struct Decoder {
    buf: [u8; MAX_FRAME_LEN],
    len: usize,
    /// The current frame outgrew `buf`; its bytes are dropped until it ends.
    too_long: bool,
}

impl Decoder {
    /// Creates a decoder that has received nothing.
    pub const fn new() -> Self {
        Self {
            buf: [0; MAX_FRAME_LEN],
            len: 0,
            too_long: false,
        }
    }

    /// Adds one received byte and returns the intact frame it completes, if
    /// any.
    ///
    /// A frame that is too long, damaged or malformed is dropped, and the
    /// decoder carries on with the next one, so a frame cut off at power-up
    /// is dropped this way too.
    pub fn push(&mut self, byte: u8) -> Option<Frame<'_>> {
        if byte != b'\n' {
            match self.buf.get_mut(self.len) {
                Some(slot) => {
                    *slot = byte;
                    self.len += 1;
                }
                None => self.too_long = true,
            }
            return None;
        }
        let len = mem::take(&mut self.len);
        if mem::take(&mut self.too_long) {
            return None;
        }
        Frame::parse(&self.buf[..len])
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the CRC-16/IBM-3740 (also called CRC-16/CCITT-FALSE) of `bytes`.
fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0xFFFF, |crc, &byte| {
        (0..8).fold(crc ^ (u16::from(byte) << 8), |crc, _| {
            if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_it_encodes() {
        let sent = [
            Entry {
                agent: Agent::Claude,
                status: Status::Busy,
                name: "agent-deck",
                uptime: 3_600,
                context: Some(Context {
                    used: 250_000,
                    window: Some(1_000_000),
                }),
                subagents: 5,
            },
            Entry {
                agent: Agent::Pi,
                status: Status::Idle,
                name: "notes",
                uptime: 0,
                context: None,
                subagents: 0,
            },
        ];
        let mut buf = [0; MAX_FRAME_LEN];
        let mut encoder = Encoder::new(&mut buf);
        sent.iter().for_each(|entry| encoder.push(entry));
        let (end, line) = encoder.finish().split_last().unwrap();

        let mut decoder = Decoder::new();
        assert!(line.iter().all(|&byte| decoder.push(byte).is_none()));
        let frame = decoder.push(*end).expect("an intact frame");
        assert!(frame.entries().eq(sent));
    }
}
