//! Reads frames on the ESP32: [`Decoder`] splits the received bytes into
//! [`Frame`]s, and each frame parses into [`Entry`] values.

use core::{mem, str};

use super::{
    Agent, Clock, Context, Entry, SessionColor, Status,
    constants::{BODY_START, MAX_FRAME_LEN, MAX_SESSIONS, MAX_TEXT_LEN, RS, US},
    crc16,
};

impl<'a> Entry<'a> {
    /// Parses one entry of a received frame body.
    ///
    /// Returns `None` if a field is missing, a code or number is invalid, or
    /// the name breaks the format.
    fn parse(entry: &'a str) -> Option<Self> {
        let mut fields = entry.splitn(6, char::from(US));
        let [agent, status, color] = *fields.next()?.as_bytes() else {
            return None;
        };
        let color = match color {
            b'-' => None,
            code => Some(SessionColor::from_code(code)?),
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
            color,
            name,
            uptime,
            context,
            subagents,
        })
    }
}

/// A frame that arrived intact.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    clock: Clock,
    body: &'a str,
}

impl<'a> Frame<'a> {
    /// Returns the host's local time when it built this frame.
    pub const fn clock(self) -> Clock {
        self.clock
    }

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
        let (clock, body) = body.split_once(char::from(US))?;
        let [h1, h2, m1, m2] = *clock.as_bytes() else {
            return None;
        };
        if !clock.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let clock = Clock {
            hour: (h1 - b'0') * 10 + h2 - b'0',
            minute: (m1 - b'0') * 10 + m2 - b'0',
        };
        if clock.hour > 23 || clock.minute > 59 {
            return None;
        }
        let mut entries = body.split_terminator(char::from(RS));
        let valid = entries.clone().count() <= MAX_SESSIONS
            && entries.all(|entry| Entry::parse(entry).is_some());
        valid.then_some(Self { clock, body })
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
