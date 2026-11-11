//! The agent deck: shows the agent sessions running on the Mac on a 2.8"
//! 240x320 color LCD.
//!
//! [`Deck`] is the display. It is a [`frames::Scene`] that reads the session
//! list the Mac sends over the serial cable as [`link_lib`] frames and draws
//! it, so it runs on the ESP32 and, with the `sim` feature, in a desktop
//! window.
//!
//! The panel is a 2×2 grid: three session tiles per page and a summary tile.
//! Busy sessions come first, then idle ones, each newest first.
//! A session tile shows the agent's logo, its running subagents, its status,
//! its name, how much of its context window it fills and how long it has run.
//! In place of a logo, Claude Code shows its animated mascot tinted with the
//! session's color, while pi and Codex show OpenAI pets picked at random.
//!
//! Animations advance one tick per [`Scene::update`], [`FPS`] times a second.
//! Firmware that redraws the whole panel only on changes calls [`Deck::tick`]
//! itself and in between redraws only [`Deck::sprite_areas`], with
//! [`Deck::draw_sprites`].
//! The summary shows the Mac's local time, busy sessions and total context
//! tokens.
//!
//! With the `host` feature, [`host`] is the Mac side: its main loop finds the
//! sessions running on the Mac and sends them to the deck.

#![cfg_attr(not(feature = "host"), no_std)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

mod assets;
mod draw;
#[cfg(feature = "host")]
pub mod host;
mod ui;

use core::fmt;

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use frames::Scene;
use heapless::{String, Vec};
use link_lib::{Agent, Clock, Context, Decoder, MAX_SESSIONS, MAX_TEXT_LEN, SessionColor, Status};

use crate::ui::PER_PAGE;

/// Panel size in pixels, in portrait orientation.
pub const SIZE: Size = Size::new(240, 320);

/// Animation ticks per second: how often to call [`Deck::tick`], or
/// [`Scene::update`], for animations to run at their speed.
pub const FPS: u32 = 30;

/// One session as the deck keeps it between frames.
#[derive(Debug)]
struct Row {
    agent: Agent,
    status: Status,
    color: Option<SessionColor>,
    name: String<MAX_TEXT_LEN>,
    uptime: u64,
    context: Option<Context>,
    subagents: usize,
    /// Picks the session's random animation; see [`ui::session_hash`].
    hash: u32,
}

/// The deck display: keeps the latest session list and draws it, a page at
/// a time.
pub struct Deck<R> {
    read: R,
    decoder: Decoder,
    rows: Vec<Row, MAX_SESSIONS>,
    clock: Clock,
    page: usize,
    /// Picks each session's random animation; see [`ui::session_hash`].
    seed: u32,
    /// Animation ticks so far.
    tick: u32,
}

impl<R> Deck<R> {
    /// Shows the next page of sessions, or the first after the last.
    pub fn next_page(&mut self) {
        self.page = (self.page + 1) % self.pages();
    }

    /// Shows the previous page of sessions, or the last before the first.
    pub fn prev_page(&mut self) {
        self.page = (self.page + self.pages() - 1) % self.pages();
    }

    /// Advances the animations by one tick, a [`FPS`]th of a second.
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Returns the areas of the panel that animate, which
    /// [`draw_sprites`](Self::draw_sprites) redraws.
    pub fn sprite_areas(&self) -> impl Iterator<Item = Rectangle> + '_ {
        ui::sprite_areas(&self.rows, self.page)
    }

    /// Redraws only the animations, onto a `target` that holds the last
    /// full [`Scene::draw`]: clears each of
    /// [`sprite_areas`](Self::sprite_areas) and draws its current frame.
    ///
    /// # Errors
    ///
    /// Returns the target's error if a draw call fails.
    pub fn draw_sprites<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        ui::draw_sprites(target, &self.rows, self.page, self.tick)
    }

    /// Returns how many pages the sessions fill; at least one.
    fn pages(&self) -> usize {
        self.rows.len().div_ceil(PER_PAGE).max(1)
    }
}

impl<R: FnMut() -> Option<u8>> Deck<R> {
    /// Creates a deck that gets the bytes the Mac sends from `read`.
    ///
    /// `read` returns the next received byte, or `None` when none is waiting.
    /// It must not block, or the display stops updating.
    ///
    /// `seed` picks each session's random animation; a random one gives
    /// different picks on each run.
    pub fn new(read: R, seed: u32) -> Self {
        Self {
            read,
            decoder: Decoder::new(),
            rows: Vec::new(),
            clock: Clock::default(),
            page: 0,
            seed,
            tick: 0,
        }
    }

    /// Reads every waiting byte and keeps the sessions of the last intact
    /// frame, and returns whether one arrived. Damaged frames are dropped;
    /// the Mac sends a fresh one soon.
    ///
    /// [`Scene::update`] calls this; firmware that redraws only on changes
    /// calls it directly.
    pub fn receive(&mut self) -> bool {
        let mut received = false;
        while let Some(byte) = (self.read)() {
            if let Some(frame) = self.decoder.push(byte) {
                received = true;
                self.clock = frame.clock();
                self.rows = frame
                    .entries()
                    .take(MAX_SESSIONS)
                    .enumerate()
                    .filter_map(|(i, entry)| {
                        let nth = frame
                            .entries()
                            .take(i)
                            .filter(|earlier| earlier.name == entry.name)
                            .count();
                        Some(Row {
                            agent: entry.agent,
                            status: entry.status,
                            color: entry.color,
                            name: entry.name.try_into().ok()?,
                            uptime: entry.uptime,
                            context: entry.context,
                            subagents: entry.subagents,
                            hash: ui::session_hash(entry.name, nth, self.seed),
                        })
                    })
                    .collect();
                // Busy sessions first, each group in the Mac's order. This
                // runs after hashing, so a session keeps its animation when
                // its status changes.
                let mut busy = 0;
                for i in 0..self.rows.len() {
                    if self.rows[i].status == Status::Busy {
                        self.rows[busy..=i].rotate_right(1);
                        busy += 1;
                    }
                }
                // Stay on the last page if sessions ended.
                self.page = self.page.min(self.pages() - 1);
            }
        }
        received
    }
}

impl<R: FnMut() -> Option<u8>> Scene for Deck<R> {
    type Color = Rgb565;

    fn update(&mut self) {
        self.receive();
        self.tick();
    }

    fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        ui::draw_page(target, &self.rows, self.clock, self.page, self.tick)
    }
}

impl<R> fmt::Debug for Deck<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deck")
            .field("rows", &self.rows)
            .field("clock", &self.clock)
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use embedded_graphics::{
        framebuffer::{Framebuffer, buffer_size},
        pixelcolor::{
            Rgb565,
            raw::{BigEndian, RawU16},
        },
        prelude::*,
    };
    use frames::Scene;
    use link_lib::{Agent, Clock, Context, Encoder, Entry, MAX_FRAME_LEN, SessionColor, Status};

    use super::{Deck, FPS, SIZE, ui::session_hash};

    const WIDTH: usize = SIZE.width as usize;
    const HEIGHT: usize = SIZE.height as usize;

    /// The panel's pixels, as the firmware keeps them.
    type Panel = Framebuffer<
        Rgb565,
        RawU16,
        BigEndian,
        WIDTH,
        HEIGHT,
        { buffer_size::<Rgb565>(WIDTH, HEIGHT) },
    >;

    /// [`Deck::draw_sprites`] over a full draw must change nothing, or the
    /// firmware's partial redraws would erase what lies in the animation
    /// areas.
    #[test]
    fn draw_sprites_matches_draw() {
        let context = Some(Context {
            used: 420_000,
            window: Some(1_000_000),
        });
        let entry = |agent, status, subagents| Entry {
            agent,
            status,
            color: (agent == Agent::Claude).then_some(SessionColor::Purple),
            name: "agent-deck",
            uptime: 4_000,
            context,
            subagents,
        };
        let mut deck = received(&[
            entry(Agent::Claude, Status::Busy, 5),
            entry(Agent::Pi, Status::Busy, 0),
            entry(Agent::Codex, Status::Idle, 0),
        ]);

        let (mut full, mut partial) = (Panel::new(), Panel::new());
        for tick in 0..3 * FPS {
            for panel in [&mut full, &mut partial] {
                let Ok(()) = panel.clear(Rgb565::BLACK);
                let Ok(()) = deck.draw(panel);
            }
            let Ok(()) = deck.draw_sprites(&mut partial);
            assert!(full.data() == partial.data(), "tick {tick}");
            deck.tick();
        }
    }

    /// Busy sessions must come first, in the order sent, and keep the
    /// animation their place in that order picks.
    #[test]
    fn busy_sessions_come_first() {
        let entry = |name, status| Entry {
            agent: Agent::Claude,
            status,
            color: None,
            name,
            uptime: 60,
            context: None,
            subagents: 0,
        };
        let deck = received(&[
            entry("a", Status::Idle),
            entry("deck", Status::Idle),
            entry("b", Status::Busy),
            entry("deck", Status::Busy),
        ]);
        let names = deck.rows.iter().map(|row| row.name.as_str());
        assert!(names.eq(["b", "deck", "a", "deck"]));
        assert_eq!(deck.rows[1].hash, session_hash("deck", 1, 7));
    }

    /// Returns a deck, seeded with 7, that has received `entries` in one
    /// frame.
    fn received(entries: &[Entry<'_>]) -> Deck<impl FnMut() -> Option<u8> + use<>> {
        let mut buf = [0; MAX_FRAME_LEN];
        let mut encoder = Encoder::new(
            &mut buf,
            Clock {
                hour: 18,
                minute: 25,
            },
        );
        entries.iter().for_each(|entry| encoder.push(entry));
        let len = encoder.finish().len();
        let mut bytes = buf.into_iter().take(len);
        let mut deck = Deck::new(move || bytes.next(), 7);
        assert!(deck.receive());
        deck
    }
}
