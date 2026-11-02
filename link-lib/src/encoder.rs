//! Builds frames on the Mac.

use super::{
    Entry,
    constants::{BODY_START, MAX_DIGITS, MAX_FRAME_LEN, MAX_SESSIONS, MAX_TEXT_LEN, RS, US},
    crc16,
};

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
