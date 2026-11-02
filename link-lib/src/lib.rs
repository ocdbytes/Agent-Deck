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
//! frame    = crc " " body "\n"
//! crc      = 4 lowercase hex digits: CRC-16/IBM-3740 of body
//! body     = *(entry RS)
//! entry    = agent status US uptime US used US window US name *(US subagent)
//! subagent = text GS text            ; the subagent's type, then its description
//! agent    = "c" (Claude Code) / "p" (pi) / "x" (Codex)
//! status   = "b" (busy) / "i" (idle)
//! uptime   = decimal seconds the session has run
//! used     = decimal context tokens in use; empty when unknown
//! window   = decimal context window in tokens; empty when unknown
//! name     = text
//! text     = up to MAX_TEXT_LEN bytes of UTF-8 without control characters
//! ```
//!
//! `RS`, `US` and `GS` are the ASCII separator bytes 0x1E, 0x1F and 0x1D.
//!
//! A frame holds at most [`MAX_SESSIONS`] entries of at most
//! [`MAX_SUBAGENTS`] subagents each, so it is at most [`MAX_FRAME_LEN`] bytes
//! and the deck receives it into a fixed buffer.

#![no_std]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

use core::{fmt, mem, str};

/// Most sessions one frame carries.
pub const MAX_SESSIONS: usize = 16;

/// Most running subagents one session carries in a frame.
pub const MAX_SUBAGENTS: usize = 4;

/// Longest text in a frame (a session name, or a subagent's type or
/// description), in bytes.
pub const MAX_TEXT_LEN: usize = 32;

/// Longest frame, in bytes, including the closing newline.
pub const MAX_FRAME_LEN: usize = BODY_START + MAX_SESSIONS * MAX_ENTRY_LEN + 1;

/// Longest entry, in bytes: the two codes, three numbers with their
/// separators, the name, the subagents and the closing `RS`.
const MAX_ENTRY_LEN: usize =
    2 + 3 * (1 + MAX_DIGITS) + 1 + MAX_TEXT_LEN + MAX_SUBAGENTS * (2 + 2 * MAX_TEXT_LEN) + 1;

/// Most digits of a number in a frame: `u64::MAX` has 20.
const MAX_DIGITS: usize = 20;

/// Where the body starts: after the 4 checksum digits and a space.
const BODY_START: usize = 5;

/// Ends each entry in a frame body (ASCII record separator).
const RS: u8 = 0x1E;

/// Separates the fields of an entry (ASCII unit separator).
const US: u8 = 0x1F;

/// Separates a subagent's type from its description (ASCII group separator).
const GS: u8 = 0x1D;

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
    /// Returns the lowercase name shown for this agent.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Pi => "pi",
            Self::Codex => "codex",
        }
    }

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

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
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

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How much of its model's context window a session fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    used: u64,
    window: Option<u64>,
}

impl Context {
    /// Creates a context of `used` tokens in a window of `window` tokens, if
    /// the window is known.
    pub const fn new(used: u64, window: Option<u64>) -> Self {
        Self { used, window }
    }

    /// Returns the tokens of the session's last model request and its reply,
    /// which is what the next request starts from.
    pub const fn used(&self) -> u64 {
        self.used
    }

    /// Returns the model's context window in tokens, or `None` if unknown.
    pub const fn window(&self) -> Option<u64> {
        self.window
    }
}

/// One session as sent over the cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    agent: Agent,
    status: Status,
    name: &'a str,
    uptime: u64,
    context: Option<Context>,
    /// The subagent fields of a received entry; empty in a created one.
    subagents: &'a str,
}

impl<'a> Entry<'a> {
    /// Creates an entry with no uptime, context or subagents.
    /// [`Encoder::push`] shortens and cleans the name.
    pub const fn new(agent: Agent, status: Status, name: &'a str) -> Self {
        Self {
            agent,
            status,
            name,
            uptime: 0,
            context: None,
            subagents: "",
        }
    }

    /// Sets how long the session has run, in seconds.
    #[must_use]
    pub const fn with_uptime(mut self, secs: u64) -> Self {
        self.uptime = secs;
        self
    }

    /// Sets how much of its context window the session fills.
    #[must_use]
    pub const fn with_context(mut self, context: Option<Context>) -> Self {
        self.context = context;
        self
    }

    /// Returns the agent running the session.
    pub const fn agent(&self) -> Agent {
        self.agent
    }

    /// Returns what the session is doing.
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Returns the session name.
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// Returns how long the session has run, in seconds.
    pub const fn uptime(&self) -> u64 {
        self.uptime
    }

    /// Returns how much of its context window the session fills, or `None`
    /// before its first model reply.
    pub const fn context(&self) -> Option<Context> {
        self.context
    }

    /// Returns the session's running subagents, in the order they were sent.
    ///
    /// Only a received entry has them; [`Encoder::push`] takes a created
    /// entry's subagents separately.
    pub fn subagents(self) -> impl Iterator<Item = SubagentEntry<'a>> {
        // `parse` checked every subagent, so none is skipped here.
        subagent_fields(self.subagents).filter_map(SubagentEntry::parse)
    }

    /// Parses one entry of a received frame body.
    ///
    /// Returns `None` if a field is missing, a code or number is invalid, a
    /// text breaks the format, or there are too many subagents.
    fn parse(entry: &'a str) -> Option<Self> {
        let mut fields = entry.splitn(6, char::from(US));
        let [agent, status] = *fields.next()?.as_bytes() else {
            return None;
        };
        let agent = Agent::from_code(agent)?;
        let status = Status::from_code(status)?;
        let uptime = fields.next()?.parse().ok()?;
        let context = match (fields.next()?, fields.next()?) {
            ("", "") => None,
            (used, "") => Some(Context::new(used.parse().ok()?, None)),
            (used, window) => Some(Context::new(used.parse().ok()?, Some(window.parse().ok()?))),
        };
        let name = fields.next().filter(|name| is_text(name))?;
        let subagents = fields.next().unwrap_or_default();
        all_parse(
            subagent_fields(subagents),
            MAX_SUBAGENTS,
            SubagentEntry::parse,
        )
        .then_some(Self {
            agent,
            status,
            name,
            uptime,
            context,
            subagents,
        })
    }
}

/// One running subagent of a session as sent over the cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubagentEntry<'a> {
    agent_type: &'a str,
    description: &'a str,
}

impl<'a> SubagentEntry<'a> {
    /// Creates a subagent entry. [`Encoder::push`] shortens and cleans both
    /// texts.
    pub const fn new(agent_type: &'a str, description: &'a str) -> Self {
        Self {
            agent_type,
            description,
        }
    }

    /// Returns the kind of subagent, such as `general-purpose` or `Explore`.
    pub const fn agent_type(&self) -> &'a str {
        self.agent_type
    }

    /// Returns the short task description its session gave it.
    pub const fn description(&self) -> &'a str {
        self.description
    }

    /// Parses one subagent field of a received entry.
    fn parse(field: &'a str) -> Option<Self> {
        let (agent_type, description) = field.split_once(char::from(GS))?;
        (is_text(agent_type) && is_text(description)).then_some(Self::new(agent_type, description))
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

    /// Adds a session and its running subagents.
    ///
    /// Sessions after the first [`MAX_SESSIONS`] and subagents after the
    /// first [`MAX_SUBAGENTS`] are dropped. Each text is cut to
    /// [`MAX_TEXT_LEN`] bytes at a character boundary, and control characters
    /// in it become spaces.
    pub fn push<'s>(
        &mut self,
        entry: &Entry<'_>,
        subagents: impl IntoIterator<Item = SubagentEntry<'s>>,
    ) {
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
        self.put_text(entry.name);
        for subagent in subagents.into_iter().take(MAX_SUBAGENTS) {
            self.put(US);
            self.put_text(subagent.agent_type);
            self.put(GS);
            self.put_text(subagent.description);
        }
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
    fn parse(line: &'a [u8]) -> Result<Self, Error> {
        let (head, body) = line.split_at_checked(BODY_START).ok_or(Error::Malformed)?;
        let crc = match head {
            [hex @ .., b' '] => str::from_utf8(hex)
                .ok()
                .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                .ok_or(Error::Malformed)?,
            _ => return Err(Error::Malformed),
        };
        if crc16(body) != crc {
            return Err(Error::Checksum);
        }
        let body = str::from_utf8(body).map_err(|_| Error::Malformed)?;
        let entries = body.split_terminator(char::from(RS));
        if !all_parse(entries, MAX_SESSIONS, Entry::parse) {
            return Err(Error::Malformed);
        }
        Ok(Self { body })
    }
}

/// Why a received frame was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The frame was longer than [`MAX_FRAME_LEN`].
    TooLong,
    /// The checksum did not match: bytes were lost or changed on the cable.
    Checksum,
    /// The frame does not follow the format.
    Malformed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLong => "frame too long",
            Self::Checksum => "frame checksum mismatch",
            Self::Malformed => "malformed frame",
        })
    }
}

impl core::error::Error for Error {}

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

    /// Adds one received byte and returns the frame it completes, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when the byte ends a frame that was too long, damaged
    /// or malformed. The decoder then carries on with the next frame, so a
    /// frame cut off at power-up is dropped this way too.
    pub fn push(&mut self, byte: u8) -> Option<Result<Frame<'_>, Error>> {
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
            return Some(Err(Error::TooLong));
        }
        Some(Frame::parse(&self.buf[..len]))
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the subagent fields of an entry's subagent part.
fn subagent_fields(subagents: &str) -> impl Iterator<Item = &str> + Clone {
    subagents
        .split(char::from(US))
        .filter(|field| !field.is_empty())
}

/// Returns whether `text` fits a text field: at most [`MAX_TEXT_LEN`] bytes
/// without control characters.
fn is_text(text: &str) -> bool {
    text.len() <= MAX_TEXT_LEN && !text.chars().any(char::is_control)
}

/// Returns whether `items` holds at most `max` items and `parse` accepts
/// each.
fn all_parse<'a, T>(
    mut items: impl Iterator<Item = &'a str> + Clone,
    max: usize,
    parse: impl Fn(&'a str) -> Option<T>,
) -> bool {
    items.clone().count() <= max && items.all(|item| parse(item).is_some())
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
