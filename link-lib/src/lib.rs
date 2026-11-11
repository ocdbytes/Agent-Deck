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
//! body      = clock US *(entry RS)
//! clock     = 4 decimal digits: local time as HHMM
//! entry     = agent status color US uptime US used US window US subagents US name
//! agent     = "c" (Claude Code) / "p" (pi) / "x" (Codex)
//! status    = "b" (busy) / "i" (idle)
//! color     = "r" / "o" / "y" / "g" / "c" / "b" / "p" / "k" / "-" (none)
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

/// The color assigned to a session by its coding agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SessionColor {
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Cyan.
    Cyan,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
    /// Pink.
    Pink,
}

impl SessionColor {
    /// Returns the byte that stands for this color in a frame.
    const fn code(self) -> u8 {
        match self {
            Self::Red => b'r',
            Self::Orange => b'o',
            Self::Yellow => b'y',
            Self::Green => b'g',
            Self::Cyan => b'c',
            Self::Blue => b'b',
            Self::Purple => b'p',
            Self::Pink => b'k',
        }
    }

    /// Returns the color a frame byte stands for.
    const fn from_code(code: u8) -> Option<Self> {
        match code {
            b'r' => Some(Self::Red),
            b'o' => Some(Self::Orange),
            b'y' => Some(Self::Yellow),
            b'g' => Some(Self::Green),
            b'c' => Some(Self::Cyan),
            b'b' => Some(Self::Blue),
            b'p' => Some(Self::Purple),
            b'k' => Some(Self::Pink),
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

/// The host's local time when it built a frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// Hour from 0 to 23.
    pub hour: u8,
    /// Minute from 0 to 59.
    pub minute: u8,
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
    /// The color assigned to the session, or `None` if its agent has none.
    pub color: Option<SessionColor>,
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
                color: Some(SessionColor::Purple),
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
                color: None,
                name: "notes",
                uptime: 0,
                context: None,
                subagents: 0,
            },
        ];
        let mut buf = [0; MAX_FRAME_LEN];
        let mut encoder = Encoder::new(
            &mut buf,
            Clock {
                hour: 18,
                minute: 25,
            },
        );
        sent.iter().for_each(|entry| encoder.push(entry));
        let (end, line) = encoder.finish().split_last().unwrap();

        let mut decoder = Decoder::new();
        assert!(line.iter().all(|&byte| decoder.push(byte).is_none()));
        let frame = decoder.push(*end).expect("an intact frame");
        assert_eq!(
            frame.clock(),
            Clock {
                hour: 18,
                minute: 25
            }
        );
        assert!(frame.entries().eq(sent));
    }

    #[test]
    fn maximum_frame_fits_the_fixed_buffer() {
        let entry = Entry {
            agent: Agent::Claude,
            status: Status::Busy,
            color: Some(SessionColor::Purple),
            name: "12345678901234567890123456789012",
            uptime: u64::MAX,
            context: Some(Context {
                used: u64::MAX,
                window: Some(u64::MAX),
            }),
            subagents: usize::MAX,
        };
        let mut buf = [0; MAX_FRAME_LEN];
        let mut encoder = Encoder::new(
            &mut buf,
            Clock {
                hour: 23,
                minute: 59,
            },
        );
        for _ in 0..MAX_SESSIONS {
            encoder.push(&entry);
        }
        let (end, line) = encoder.finish().split_last().unwrap();

        let mut decoder = Decoder::new();
        assert!(line.iter().all(|&byte| decoder.push(byte).is_none()));
        assert_eq!(
            decoder
                .push(*end)
                .expect("maximum frame must decode")
                .entries()
                .count(),
            MAX_SESSIONS
        );
    }
}
