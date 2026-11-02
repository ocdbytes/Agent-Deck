//! Shows the deck in a desktop window with the sessions running on this Mac.
//! ← and → turn the pages.
//!
//! Run with `cargo run -p deck --example preview --features sim`.

use std::time::{Duration, Instant};

use deck::{Deck, SIZE, host};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{
    OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window, sdl2::Keycode,
};
use frames::Scene;
use link_lib::MAX_FRAME_LEN;

/// How often to fetch the sessions, like the Mac side does for the deck.
const INTERVAL: Duration = Duration::from_secs(1);

/// Preview frame rate; keep it the same as the firmware's.
const FPS: u32 = 30;

/// Window pixels per panel pixel.
const SCALE: u32 = 2;

fn main() {
    let mut buf = [0; MAX_FRAME_LEN];
    let (mut len, mut pos) = (0, 0);
    let mut next_fetch = Instant::now();
    // Stands in for the serial port: hands out the bytes of a fresh frame
    // every `INTERVAL`.
    let read = move || {
        if Instant::now() >= next_fetch {
            next_fetch = Instant::now() + INTERVAL;
            len = host::frame(&mut buf).expect("cannot fetch sessions").len();
            pos = 0;
        }
        let byte = buf[..len].get(pos).copied()?;
        pos += 1;
        Some(byte)
    };

    let mut deck = Deck::new(read);
    let mut display = SimulatorDisplay::<Rgb565>::new(SIZE);
    let mut window = Window::new(
        "agent-deck",
        &OutputSettingsBuilder::new().scale(SCALE).build(),
    );
    window.set_max_fps(FPS);
    loop {
        let Ok(()) = deck.frame(&mut display);
        window.update(&display);
        for event in window.events() {
            match event {
                SimulatorEvent::Quit => return,
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Left,
                    ..
                } => deck.prev_page(),
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Right,
                    ..
                } => deck.next_page(),
                _ => {}
            }
        }
    }
}
