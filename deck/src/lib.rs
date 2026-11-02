//! The agent deck: shows the agent sessions running on the Mac on a 2.8"
//! 240x320 color LCD.
//!
//! [`Deck`] is the display. It is a [`frames::Scene`] that reads the session
//! list the Mac sends over the serial cable as [`link_lib`] frames and draws
//! it, so it runs on the ESP32 and, with the `sim` feature, in a desktop
//! window.
//!
//! The panel is a 2×2 grid: three session tiles per page and a summary tile.
//! A session tile shows the agent's logo, its status, its name, how much of
//! its context window it fills and how long it has run. The summary shows the
//! page, how many sessions are busy and idle, and their total context tokens.
//!
//! With the `host` feature, [`host`] is the Mac side: its main loop fetches
//! the sessions with `deck_lib` and sends them to the deck.

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

use core::fmt::{self, Write as _};

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use frames::Scene;
use heapless::{String, Vec};
use link_lib::{Agent, Context, Decoder, MAX_SESSIONS, MAX_TEXT_LEN, Status};

use crate::assets::{BODY, CLAUDE, KANJI, NUMERAL, OPENAI, PERCENT, PI};

/// Panel size in pixels, in portrait orientation.
pub const SIZE: Size = Size::new(240, 320);

/// Width of a tile in the 2×2 grid, in pixels.
const TILE_W: i32 = 120;

/// Height of a tile in the 2×2 grid, in pixels.
const TILE_H: i32 = 160;

/// Space between a tile's edge and its content, in pixels.
const PAD: i32 = 8;

/// Sessions per page: three tiles, with the fourth showing the summary.
const PER_PAGE: usize = 3;

/// Context use, in percent, from which a session shows in vermilion.
const WARN_PERCENT: u64 = 80;

/// Values and names.
const TEXT: Rgb565 = rgb(0xE8E4DC);

/// Labels and idle sessions.
const DIM: Rgb565 = rgb(0x8A8780);

/// Busy sessions.
const GREEN: Rgb565 = rgb(0x9BD67F);

/// Kanji accents and context warnings.
const VERMILION: Rgb565 = rgb(0xE5603B);

/// Lines between tiles.
const RULE: Rgb565 = rgb(0x2E2E2E);

/// The empty part of a context bar.
const TRACK: Rgb565 = rgb(0x3A3A3A);

/// Text the deck builds for one value. 40 bytes fit the longest: a context
/// of two 17-character token counts and a slash.
type Label = String<40>;

/// One session as the deck keeps it between frames.
#[derive(Debug)]
struct Row {
    agent: Agent,
    status: Status,
    name: String<MAX_TEXT_LEN>,
    uptime: u64,
    context: Option<Context>,
}

/// The deck display: keeps the latest session list and draws it, a page at
/// a time.
pub struct Deck<R> {
    read: R,
    decoder: Decoder,
    rows: Vec<Row, MAX_SESSIONS>,
    page: usize,
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
    pub fn new(read: R) -> Self {
        Self {
            read,
            decoder: Decoder::new(),
            rows: Vec::new(),
            page: 0,
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
            if let Some(Ok(frame)) = self.decoder.push(byte) {
                received = true;
                self.rows = frame
                    .entries()
                    .take(MAX_SESSIONS)
                    .filter_map(|entry| {
                        Some(Row {
                            agent: entry.agent(),
                            status: entry.status(),
                            name: entry.name().try_into().ok()?,
                            uptime: entry.uptime(),
                            context: entry.context(),
                        })
                    })
                    .collect();
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
    }

    fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let rule = PrimitiveStyle::with_stroke(RULE, 1);
        let (width, height) = (SIZE.width as i32, SIZE.height as i32);
        Line::new(Point::new(TILE_W, 0), Point::new(TILE_W, height - 1))
            .into_styled(rule)
            .draw(target)?;
        Line::new(Point::new(0, TILE_H), Point::new(width - 1, TILE_H))
            .into_styled(rule)
            .draw(target)?;
        let sessions = self.rows.iter().skip(self.page * PER_PAGE).take(PER_PAGE);
        for (slot, row) in sessions.enumerate() {
            draw_session(target, row, tile(slot))?;
        }
        draw_summary(target, &self.rows, self.page, self.pages(), tile(PER_PAGE))
    }
}

impl<R> fmt::Debug for Deck<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deck")
            .field("rows", &self.rows)
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

/// Returns the top-left corner of tile `slot`, counting left to right, then
/// top to bottom.
fn tile(slot: usize) -> Point {
    Point::new((slot % 2) as i32 * TILE_W, (slot / 2) as i32 * TILE_H)
}

/// Draws one session's tile with its top-left corner at `at`.
fn draw_session<D>(target: &mut D, row: &Row, at: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let (left, right) = (at.x + PAD, at.x + TILE_W - PAD);

    let logo = match row.agent {
        Agent::Claude => Some(CLAUDE),
        Agent::Pi => Some(PI),
        Agent::Codex => Some(OPENAI),
        _ => None,
    };
    if let Some(logo) = logo {
        draw::image(target, logo, Point::new(left, at.y + PAD))?;
    }

    // ● 稼働 busy, or ○ 待機 idle.
    let busy = row.status == Status::Busy;
    let (color, kanji) = if busy {
        (GREEN, "稼働")
    } else {
        (DIM, "待機")
    };
    let y = at.y + 54;
    let dot = Circle::new(Point::new(left, y - 6), 6);
    let dot_style = if busy {
        PrimitiveStyle::with_fill(color)
    } else {
        PrimitiveStyle::with_stroke(color, 1)
    };
    dot.into_styled(dot_style).draw(target)?;
    // Kanji sit a pixel under the Latin baseline, centring them on the text.
    let x = draw::text(target, &KANJI, kanji, Point::new(left + 10, y + 1), color)?;
    draw::text(
        target,
        &BODY,
        row.status.as_str(),
        Point::new(x + 4, y),
        color,
    )?;

    draw_name(target, &row.name, Point::new(left, at.y + 70), right)?;

    // Big percentage of the context window, rounded, and a bar of it.
    let percent = row.context.and_then(|context| {
        let window = context.window().filter(|&window| window > 0)?;
        Some((context.used().saturating_mul(100) + window / 2) / window)
    });
    let color = if percent.is_some_and(|percent| percent >= WARN_PERCENT) {
        VERMILION
    } else {
        TEXT
    };
    let mut digits = Label::new();
    // invariant: a u64 has at most 20 digits, which fit a Label.
    let _ = match percent {
        Some(percent) => write!(digits, "{percent}"),
        None => digits.write_str("--"),
    };
    let y = at.y + 112;
    let x = draw::text(target, &NUMERAL, &digits, Point::new(left, y), color)?;
    draw::text(target, &PERCENT, "%", Point::new(x, y), color)?;
    let bar = at.y + 119;
    Line::new(Point::new(left, bar), Point::new(right - 1, bar))
        .into_styled(PrimitiveStyle::with_stroke(TRACK, 1))
        .draw(target)?;
    let filled = percent.unwrap_or(0).min(100) as i32 * (right - left) / 100;
    Rectangle::new(Point::new(left, bar - 1), Size::new(filled as u32, 2))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)?;

    draw_row(
        target,
        "ctx",
        &context_text(row.context),
        at.y + 136,
        left,
        right,
    )?;
    draw_row(
        target,
        "up",
        &uptime_text(row.uptime),
        at.y + 152,
        left,
        right,
    )
}

/// Draws the summary tile with its top-left corner at `at`: 今 now and the
/// page, how many sessions are busy and idle, and their total context tokens.
fn draw_summary<D>(
    target: &mut D,
    rows: &[Row],
    page: usize,
    pages: usize,
    at: Point,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let (left, right) = (at.x + PAD, at.x + TILE_W - PAD);

    let y = at.y + 22;
    let x = draw::text(target, &KANJI, "今", Point::new(left, y + 1), VERMILION)?;
    draw::text(target, &BODY, "now", Point::new(x + 4, y), DIM)?;
    let mut page_text = Label::new();
    // invariant: two usize values and a slash fit a Label.
    let _ = write!(page_text, "{}/{pages}", page + 1);
    let x = right - BODY.text_width(&page_text);
    draw::text(target, &BODY, &page_text, Point::new(x, y), DIM)?;

    let busy = rows.iter().filter(|row| row.status == Status::Busy).count();
    draw_count(target, busy, "busy", GREEN, Point::new(left, at.y + 74))?;
    draw_count(
        target,
        rows.len() - busy,
        "idle",
        TEXT,
        Point::new(left, at.y + 112),
    )?;

    Line::new(
        Point::new(left, at.y + 138),
        Point::new(right - 1, at.y + 138),
    )
    .into_styled(PrimitiveStyle::with_stroke(RULE, 1))
    .draw(target)?;
    let total = rows
        .iter()
        .filter_map(|row| row.context)
        .map(|context| context.used())
        .sum();
    draw_row(target, "tok", &tokens_text(total), at.y + 152, left, right)
}

/// Draws a big `count` in `color` and a dim `label` after it, on the
/// baseline at `origin`.
fn draw_count<D>(
    target: &mut D,
    count: usize,
    label: &str,
    color: Rgb565,
    origin: Point,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut digits = Label::new();
    // invariant: a usize has at most 20 digits, which fit a Label.
    let _ = write!(digits, "{count}");
    let x = draw::text(target, &NUMERAL, &digits, origin, color)?;
    draw::text(target, &BODY, label, Point::new(x + 4, origin.y), DIM)?;
    Ok(())
}

/// Draws `name` in corner brackets, 「name」, on the baseline at `origin`,
/// cut short with `…` so it ends before `right`.
fn draw_name<D>(target: &mut D, name: &str, origin: Point, right: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let style = PrimitiveStyle::with_stroke(DIM, 1);
    let top = origin.y - BODY.baseline as i32;
    let bottom = top + BODY.height as i32 - 1;
    // 「 is a corner at the top left.
    Line::new(Point::new(origin.x, top), Point::new(origin.x + 3, top))
        .into_styled(style)
        .draw(target)?;
    Line::new(Point::new(origin.x, top), Point::new(origin.x, top + 6))
        .into_styled(style)
        .draw(target)?;

    let start = Point::new(origin.x + 5, origin.y);
    let fits = ((right - start.x - 5) / BODY.width as i32) as usize;
    let end = if name.chars().count() > fits {
        let cut = name
            .char_indices()
            .nth(fits.saturating_sub(1))
            .map_or(name.len(), |(i, _)| i);
        let x = draw::text(target, &BODY, &name[..cut], start, TEXT)?;
        draw::text(target, &BODY, "…", Point::new(x, origin.y), TEXT)?
    } else {
        draw::text(target, &BODY, name, start, TEXT)?
    };

    // 」 is a corner at the bottom right, just after the name.
    let x = end + 2;
    Line::new(Point::new(x, bottom - 6), Point::new(x, bottom))
        .into_styled(style)
        .draw(target)?;
    Line::new(Point::new(x - 3, bottom), Point::new(x, bottom))
        .into_styled(style)
        .draw(target)?;
    Ok(())
}

/// Draws a row on the baseline `y`: a dim `label` at `left` and a bright
/// `value` ending at `right`.
fn draw_row<D>(
    target: &mut D,
    label: &str,
    value: &str,
    y: i32,
    left: i32,
    right: i32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::text(target, &BODY, label, Point::new(left, y), DIM)?;
    let x = right - BODY.text_width(value);
    draw::text(target, &BODY, value, Point::new(x, y), TEXT)?;
    Ok(())
}

/// Returns a session's context as `142k/200k`, as `142k` when the window is
/// unknown, or as `-` before its first reply.
fn context_text(context: Option<Context>) -> Label {
    let Some(context) = context else {
        return "-".try_into().unwrap_or_default();
    };
    let mut text = tokens_text(context.used());
    if let Some(window) = context.window() {
        // invariant: two token counts of at most 17 bytes and a slash fit a
        // Label.
        let _ = write!(text, "/{}", tokens_text(window));
    }
    text
}

/// Returns a token count as `850`, `142k` or `1.2M`.
fn tokens_text(tokens: u64) -> Label {
    let mut text = Label::new();
    // invariant: the longest result, `u64::MAX` as `18446744073709.5M`, is
    // 17 bytes and fits a Label.
    let _ = match tokens {
        0..1_000 => write!(text, "{tokens}"),
        1_000..1_000_000 => write!(text, "{}k", tokens / 1_000),
        _ => match tokens / 100_000 {
            tenths if tenths % 10 == 0 => write!(text, "{}M", tenths / 10),
            tenths => write!(text, "{}.{}M", tenths / 10, tenths % 10),
        },
    };
    text
}

/// Returns an uptime in seconds as `40m`, `2h 15m` or `3d 04h`.
fn uptime_text(secs: u64) -> Label {
    let (days, hours, minutes) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60);
    let mut text = Label::new();
    // invariant: the longest result, 15 digits of days and `d 23h`, is 20
    // bytes and fits a Label.
    let _ = if days > 0 {
        write!(text, "{days}d {hours:02}h")
    } else if hours > 0 {
        write!(text, "{hours}h {minutes:02}m")
    } else {
        write!(text, "{minutes}m")
    };
    text
}

/// Converts a `0xRRGGBB` color to RGB565.
const fn rgb(hex: u32) -> Rgb565 {
    Rgb565::new(
        (hex >> 19) as u8 & 0x1F,
        (hex >> 10) as u8 & 0x3F,
        (hex >> 3) as u8 & 0x1F,
    )
}
