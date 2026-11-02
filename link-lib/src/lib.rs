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

mod constants;
mod decoder;
mod encoder;

pub use constants::{MAX_FRAME_LEN, MAX_SESSIONS, MAX_TEXT_LEN};
pub use decoder::{Decoder, Frame};
pub use encoder::Encoder;

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
