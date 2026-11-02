//! Renders the deck: the 2×2 tile layout, its colors, and the text it builds
//! from session values.

use core::fmt::Write as _;

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use heapless::String;
use link_lib::{Agent, Context, Status};

use crate::{
    Row, SIZE,
    assets::{BODY, CLAUDE_SMALL, KANJI, NUMERAL, OPENAI_SMALL, PERCENT, PI, PI_SMALL},
    draw::{self, Image},
};

mod sprites;

pub(crate) use sprites::session_hash;

/// Width of a tile in the 2×2 grid, in pixels.
const TILE_W: i32 = 120;

/// Height of a tile in the 2×2 grid, in pixels.
const TILE_H: i32 = 160;

/// Space between a tile's edge and its content, in pixels.
const PAD: i32 = 8;

/// Sessions per page: three tiles, with the fourth showing the summary.
pub(crate) const PER_PAGE: usize = 3;

/// Context use, in percent, from which a session shows in vermilion.
const WARN_PERCENT: u64 = 80;

/// Most subagents drawn as one small logo each; more show as one logo and a
/// count, such as `x5`.
const MAX_SUBAGENT_LOGOS: usize = 3;

/// Space between small subagent logos, and before the count, in pixels.
const SUBAGENT_GAP: i32 = 2;

/// How far kanji sit below the Latin baseline, in pixels, so that their
/// middle lines up with the Latin text's.
const KANJI_DROP: i32 = 2;

/// Values and names.
const TEXT: Rgb565 = rgb(0xFFFFFF);

/// Labels and idle sessions.
const DIM: Rgb565 = rgb(0xB8B5AE);

/// Busy sessions.
const GREEN: Rgb565 = rgb(0xB5F59A);

/// Kanji accents and context warnings.
const VERMILION: Rgb565 = rgb(0xFF7A52);

/// Lines between tiles.
const RULE: Rgb565 = rgb(0x4A4A4A);

/// The empty part of a context bar.
const TRACK: Rgb565 = rgb(0x5A5A5A);

/// Text the deck builds for one value. 40 bytes fit the longest: a context
/// of two 17-character token counts and a slash.
type Label = String<40>;

/// Draws page `page` of `pages` of `rows`, with animations `tick` ticks in:
/// the grid lines, up to [`PER_PAGE`] session tiles and the summary tile.
pub(crate) fn draw_page<D>(
    target: &mut D,
    rows: &[Row],
    page: usize,
    pages: usize,
    tick: u32,
) -> Result<(), D::Error>
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
    for (slot, row) in visible(rows, page) {
        draw_session(target, row, tile(slot), tick)?;
    }
    draw_summary(target, rows, page, pages, tile(PER_PAGE))
}

/// Redraws only the animations on page `page` of `rows`, `tick` ticks in:
/// clears each of [`sprite_areas`] and draws its frame.
pub(crate) fn draw_sprites<D>(
    target: &mut D,
    rows: &[Row],
    page: usize,
    tick: u32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    for (slot, row) in visible(rows, page).filter(|(_, row)| is_animated(row.agent)) {
        let area = sprite_area(slot);
        target.fill_solid(&area, Rgb565::BLACK)?;
        draw_logo(target, row, tick, area.top_left)?;
    }
    Ok(())
}

/// Returns the areas of the animations on page `page` of `rows`, which
/// [`draw_sprites`] redraws.
pub(crate) fn sprite_areas(rows: &[Row], page: usize) -> impl Iterator<Item = Rectangle> + '_ {
    visible(rows, page)
        .filter(|(_, row)| is_animated(row.agent))
        .map(|(slot, _)| sprite_area(slot))
}

/// Returns the sessions on page `page` of `rows`, each with the slot of the
/// tile it is drawn in.
fn visible(rows: &[Row], page: usize) -> impl Iterator<Item = (usize, &Row)> {
    rows.iter().skip(page * PER_PAGE).take(PER_PAGE).enumerate()
}

/// Returns whether `agent`'s sessions show an animation instead of a logo.
fn is_animated(agent: Agent) -> bool {
    matches!(agent, Agent::Claude | Agent::Codex)
}

/// Returns the area of the logo or animation in tile `slot`.
fn sprite_area(slot: usize) -> Rectangle {
    Rectangle::new(tile(slot) + Point::new(PAD, PAD), sprites::SIZE)
}

/// Returns the top-left corner of tile `slot`, counting left to right, then
/// top to bottom.
fn tile(slot: usize) -> Point {
    Point::new((slot % 2) as i32 * TILE_W, (slot / 2) as i32 * TILE_H)
}

/// Draws one session's tile with its top-left corner at `at`, with its
/// animation `tick` ticks in.
fn draw_session<D>(target: &mut D, row: &Row, at: Point, tick: u32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let (left, right) = (at.x + PAD, at.x + TILE_W - PAD);

    draw_logo(target, row, tick, Point::new(left, at.y + PAD))?;
    let small = match row.agent {
        Agent::Claude => Some(CLAUDE_SMALL),
        Agent::Pi => Some(PI_SMALL),
        Agent::Codex => Some(OPENAI_SMALL),
        _ => None,
    };
    if let Some(small) = small {
        draw_subagents(target, small, row.subagents, right, at.y + PAD)?;
    }

    // ● 稼働 busy, or ○ 待機 idle.
    let busy = row.status == Status::Busy;
    let (color, kanji) = if busy {
        (GREEN, "稼働")
    } else {
        (DIM, "待機")
    };
    // The kanji's 15 px stand just below the animation.
    let y = at.y + 56;
    let dot = Circle::new(Point::new(left, y - 6), 6);
    let dot_style = if busy {
        PrimitiveStyle::with_fill(color)
    } else {
        PrimitiveStyle::with_stroke(color, 1)
    };
    dot.into_styled(dot_style).draw(target)?;
    let x = draw::text(
        target,
        &KANJI,
        kanji,
        Point::new(left + 10, y + KANJI_DROP),
        color,
    )?;
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
        let window = context.window.filter(|&window| window > 0)?;
        Some((context.used.saturating_mul(100) + window / 2) / window)
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

/// Draws the logo of `row`'s agent with its top-left corner at `top_left`:
/// for Claude Code its mascot and for Codex its pet, `tick` ticks into their
/// animation.
fn draw_logo<D>(target: &mut D, row: &Row, tick: u32, top_left: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    match row.agent {
        Agent::Claude => sprites::draw_claude(target, row.hash, tick, top_left),
        Agent::Codex => sprites::draw_pet(target, row.hash, row.status, tick, top_left),
        Agent::Pi => draw::image(target, PI, top_left),
        _ => Ok(()),
    }
}

/// Draws a session's `count` running subagents as small `logo`s in a row
/// ending at `right`, with their tops at `top`: one logo per subagent up to
/// [`MAX_SUBAGENT_LOGOS`], or one logo and `x<count>` for more.
fn draw_subagents<D>(
    target: &mut D,
    logo: Image,
    count: usize,
    right: i32,
    top: i32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut x = right;
    let logos = if count > MAX_SUBAGENT_LOGOS {
        let mut text = Label::new();
        // invariant: `x` and a usize of at most 20 digits fit a Label.
        let _ = write!(text, "x{count}");
        x -= BODY.text_width(&text);
        // The count sits on the logos' bottom edge.
        draw::text(
            target,
            &BODY,
            &text,
            Point::new(x, top + logo.height as i32),
            TEXT,
        )?;
        x -= SUBAGENT_GAP;
        1
    } else {
        count
    };
    for _ in 0..logos {
        x -= logo.width as i32;
        draw::image(target, logo, Point::new(x, top))?;
        x -= SUBAGENT_GAP;
    }
    Ok(())
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
    let x = draw::text(
        target,
        &KANJI,
        "今",
        Point::new(left, y + KANJI_DROP),
        VERMILION,
    )?;
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
        .map(|context| context.used)
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
    let mut text = tokens_text(context.used);
    if let Some(window) = context.window {
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
