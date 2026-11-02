//! Mac side of the deck: fetches the running sessions and sends them to the
//! ESP32 as [`link_lib`] frames.

use std::{
    convert::Infallible,
    io::{self, Write},
    thread,
    time::Duration,
};

use deck_lib::{Claude, Codex, Pi, SessionFetcher};
use link_lib::{Encoder, Entry, MAX_FRAME_LEN, SubagentEntry};

/// Fetches every agent's sessions, oldest first, and encodes them and their
/// running subagents into `buf` as one frame.
///
/// # Errors
///
/// Returns an error if an agent's sessions cannot be fetched.
pub fn frame(buf: &mut [u8; MAX_FRAME_LEN]) -> io::Result<&[u8]> {
    let fetchers: [&dyn SessionFetcher; 3] = [&Claude, &Pi, &Codex];
    let mut sessions = Vec::new();
    for fetcher in fetchers {
        sessions.extend(fetcher.sessions()?);
    }
    sessions.sort_by_key(|session| session.started());
    let mut encoder = Encoder::new(buf);
    for session in &sessions {
        let subagents = session.subagents().iter().map(SubagentEntry::from);
        encoder.push(&Entry::from(session), subagents);
    }
    Ok(encoder.finish())
}

/// Main loop: sends a fresh frame to `port` every `interval`, forever.
///
/// `port` is the deck's serial port, already open at the firmware's baud
/// rate.
///
/// # Errors
///
/// Returns an error if fetching the sessions or writing to `port` fails.
pub fn run(mut port: impl Write, interval: Duration) -> io::Result<Infallible> {
    let mut buf = [0; MAX_FRAME_LEN];
    loop {
        port.write_all(frame(&mut buf)?)?;
        port.flush()?;
        thread::sleep(interval);
    }
}
