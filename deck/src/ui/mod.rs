//! Renders the deck: the 2×2 tile layout, its colors, and the text it builds
//! from session values.

use core::fmt::Write as _;

use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
};
use heapless::String;
use link_lib::{Agent, Clock, Context, Status};

use crate::{
    Row,
    assets::{BODY, CLAUDE_SMALL, KANJI, NUMERAL, OPENAI_SMALL, PERCENT, PI_SMALL},
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

/// Panel outlines and agent IDs.
const CYAN: Rgb565 = rgb(0x35E6F2);

/// Faint panel borders and scanlines.
const GRID: Rgb565 = rgb(0x12343A);

/// The summary panel.
const PINK: Rgb565 = rgb(0xFF2D7D);

/// Dark scanlines over the summary panel.
const PINK_GRID: Rgb565 = rgb(0xD51F67);

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
    clock: Clock,
    page: usize,
    tick: u32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    for slot in 0..PER_PAGE {
        draw_panel(target, tile(slot))?;
    }
    for (slot, row) in visible(rows, page) {
        draw_session(target, row, page * PER_PAGE + slot + 1, tile(slot), tick)?;
    }
    draw_summary(target, rows, clock, page, tile(PER_PAGE))
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
        draw_scanlines(target, area, GRID)?;
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
    matches!(agent, Agent::Claude | Agent::Pi | Agent::Codex)
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

/// Draws a dark scanlined panel with bright corner marks.
fn draw_panel<D>(target: &mut D, at: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let area = Rectangle::new(at + Point::new(4, 4), Size::new(112, 152));
    draw_scanlines(target, area, GRID)?;
    area.into_styled(PrimitiveStyle::with_stroke(GRID, 1))
        .draw(target)?;
    let (left, right, top, bottom) = (
        area.top_left.x,
        area.top_left.x + area.size.width as i32 - 1,
        area.top_left.y,
        area.top_left.y + area.size.height as i32 - 1,
    );
    let corner = PrimitiveStyle::with_stroke(CYAN, 1);
    for (a, b) in [
        (Point::new(left, top), Point::new(left + 7, top)),
        (Point::new(left, top), Point::new(left, top + 7)),
        (Point::new(right - 7, top), Point::new(right, top)),
        (Point::new(right, top), Point::new(right, top + 7)),
        (Point::new(left, bottom - 7), Point::new(left, bottom)),
        (Point::new(left, bottom), Point::new(left + 7, bottom)),
        (Point::new(right, bottom - 7), Point::new(right, bottom)),
        (Point::new(right - 7, bottom), Point::new(right, bottom)),
    ] {
        Line::new(a, b).into_styled(corner).draw(target)?;
    }
    Ok(())
}

/// Draws horizontal scanlines inside `area`.
fn draw_scanlines<D>(target: &mut D, area: Rectangle, color: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let right = area.top_left.x + area.size.width as i32 - 1;
    let bottom = area.top_left.y + area.size.height as i32;
    let start = area.top_left.y + (1 - area.top_left.y).rem_euclid(3);
    for y in (start..bottom).step_by(3) {
        Line::new(Point::new(area.top_left.x, y), Point::new(right, y))
            .into_styled(PrimitiveStyle::with_stroke(color, 1))
            .draw(target)?;
    }
    Ok(())
}

/// Draws one session's tile with its top-left corner at `at`, with its
/// animation `tick` ticks in.
fn draw_session<D>(
    target: &mut D,
    row: &Row,
    number: usize,
    at: Point,
    tick: u32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let (left, right) = (at.x + PAD, at.x + TILE_W - PAD);

    draw_logo(target, row, tick, Point::new(left, at.y + PAD))?;
    let mut id = Label::new();
    let _ = write!(id, "A{number:02}");
    draw::text(
        target,
        &BODY,
        &id,
        Point::new(right - BODY.text_width(&id), at.y + 14),
        CYAN,
    )?;
    let small = match row.agent {
        Agent::Claude => Some(CLAUDE_SMALL),
        Agent::Pi => Some(PI_SMALL),
        Agent::Codex => Some(OPENAI_SMALL),
        _ => None,
    };
    if let Some(small) = small {
        draw_subagents(target, small, row.subagents, right, at.y + 24)?;
    }

    // ■ 稼働 busy, or □ 待機 idle.
    let busy = row.status == Status::Busy;
    let (color, kanji) = if busy {
        (GREEN, "稼働")
    } else {
        (DIM, "待機")
    };
    // The kanji's 15 px stand just below the animation.
    let y = at.y + 56;
    let marker = Rectangle::new(Point::new(left, y - 6), Size::new(6, 6));
    let marker_style = if busy {
        PrimitiveStyle::with_fill(color)
    } else {
        PrimitiveStyle::with_stroke(color, 1)
    };
    marker.into_styled(marker_style).draw(target)?;
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
    let y = at.y + 120;
    let x = draw::text(target, &NUMERAL, &digits, Point::new(left, y), color)?;
    draw::text(target, &PERCENT, "%", Point::new(x, y), color)?;
    let bar = at.y + 127;
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
        at.y + 145,
        left,
        right,
    )?;
    draw_row(
        target,
        "up",
        &uptime_text(row.uptime),
        at.y + 153,
        left,
        right,
    )
}

/// Draws the logo of `row`'s agent with its top-left corner at `top_left`:
/// for Claude Code its mascot and for pi and Codex an OpenAI pet, `tick`
/// ticks into their animation.
fn draw_logo<D>(target: &mut D, row: &Row, tick: u32, top_left: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    match row.agent {
        Agent::Claude => sprites::draw_claude(target, row.hash, row.color, tick, top_left),
        Agent::Pi | Agent::Codex => sprites::draw_pet(target, row.hash, row.status, tick, top_left),
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

/// Draws the summary tile with its top-left corner at `at`: the local time,
/// busy sessions and total context tokens.
fn draw_summary<D>(
    target: &mut D,
    rows: &[Row],
    clock: Clock,
    page: usize,
    at: Point,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let panel = Rectangle::new(at + Point::new(4, 4), Size::new(112, 152));
    draw_scanlines(target, panel, GRID)?;
    panel
        .into_styled(PrimitiveStyle::with_stroke(PINK, 1))
        .draw(target)?;
    let (left, right) = (at.x + PAD, at.x + TILE_W - PAD);

    let header = Rectangle::new(at + Point::new(4, 4), Size::new(112, 20));
    header
        .into_styled(PrimitiveStyle::with_fill(PINK))
        .draw(target)?;
    draw_scanlines(target, header, PINK_GRID)?;
    let y = at.y + 18;
    let x = draw::text(
        target,
        &KANJI,
        "今",
        Point::new(left, y + KANJI_DROP),
        Rgb565::BLACK,
    )?;
    draw::text(target, &BODY, "NOW", Point::new(x + 4, y), Rgb565::BLACK)?;
    let mut time = Label::new();
    let _ = write!(time, "{:02}:{:02}", clock.hour, clock.minute);
    draw::text(
        target,
        &BODY,
        &time,
        Point::new(right - BODY.text_width(&time), y),
        Rgb565::BLACK,
    )?;

    let busy = rows.iter().filter(|row| row.status == Status::Busy).count();
    let mut count = Label::new();
    let _ = write!(count, "{busy}");
    let x = draw::text(target, &NUMERAL, &count, Point::new(left, at.y + 57), GREEN)?;
    let mut total = Label::new();
    let _ = write!(total, "/{}", rows.len());
    draw::text(target, &BODY, &total, Point::new(x, at.y + 51), DIM)?;
    draw::text(
        target,
        &BODY,
        "BUSY",
        Point::new(right - BODY.text_width("BUSY"), at.y + 51),
        GREEN,
    )?;

    for (slot, row) in visible(rows, page) {
        let top_left = Point::new(left + slot as i32 * 32, at.y + 63);
        let card = Rectangle::new(top_left, Size::new(26, 28));
        let color = if row.status == Status::Busy {
            GREEN
        } else {
            TRACK
        };
        card.into_styled(PrimitiveStyle::with_stroke(color, 1))
            .draw(target)?;
        let logo = match row.agent {
            Agent::Claude => CLAUDE_SMALL,
            Agent::Pi => PI_SMALL,
            Agent::Codex => OPENAI_SMALL,
            _ => continue,
        };
        draw::image_2x(target, logo, top_left + Point::new(5, 6))?;
    }

    let divider = at.y + 109;
    for x in (left..right).step_by(4) {
        Line::new(
            Point::new(x, divider),
            Point::new((x + 1).min(right - 1), divider),
        )
        .into_styled(PrimitiveStyle::with_stroke(TRACK, 1))
        .draw(target)?;
    }
    draw::text(target, &BODY, "TOK", Point::new(left, at.y + 126), CYAN)?;

    let tokens: u64 = rows
        .iter()
        .filter_map(|row| row.context)
        .map(|context| context.used)
        .sum();
    let (digits, suffix) = summary_tokens(tokens);
    let origin = Point::new(left, at.y + 153);
    draw::text(target, &NUMERAL, &digits, origin + Point::new(1, 0), PINK)?;
    let x = draw::text(target, &NUMERAL, &digits, origin, TEXT)?;
    draw::text(target, &BODY, &suffix, Point::new(x, origin.y), CYAN)?;
    Ok(())
}

/// Draws `name` after a command-prompt marker, cut short with `…` so it
/// ends before `right`.
fn draw_name<D>(target: &mut D, name: &str, origin: Point, right: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = draw::text(target, &BODY, ">_", origin, DIM)?;
    let fits = ((right - x) / BODY.width as i32) as usize;
    if name.chars().count() > fits {
        let cut = name
            .char_indices()
            .nth(fits.saturating_sub(1))
            .map_or(name.len(), |(i, _)| i);
        let x = draw::text(target, &BODY, &name[..cut], Point::new(x, origin.y), DIM)?;
        draw::text(target, &BODY, "…", Point::new(x, origin.y), DIM)?;
    } else {
        draw::text(target, &BODY, name, Point::new(x, origin.y), DIM)?;
    }
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

/// Splits the summary token count into large digits and a small suffix.
fn summary_tokens(tokens: u64) -> (Label, Label) {
    let (mut digits, mut suffix) = (Label::new(), Label::new());
    match tokens {
        0..1_000 => {
            let _ = write!(digits, "{tokens}");
        }
        1_000..1_000_000 => {
            let _ = write!(digits, "{}", tokens / 1_000);
            let _ = suffix.write_str("k");
        }
        _ => {
            let tenths = tokens / 100_000;
            let _ = write!(digits, "{}", tenths / 10);
            let _ = if tenths % 10 == 0 {
                suffix.write_str("M")
            } else {
                write!(suffix, ".{}M", tenths % 10)
            };
        }
    }
    (digits, suffix)
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

#[cfg(test)]
mod tests {
    use super::summary_tokens;

    #[test]
    fn summary_tokens_uses_the_right_unit() {
        let text = |tokens| {
            let (digits, suffix) = summary_tokens(tokens);
            (digits.as_str().to_owned(), suffix.as_str().to_owned())
        };
        assert_eq!(text(850), ("850".to_owned(), "".to_owned()));
        assert_eq!(text(366_000), ("366".to_owned(), "k".to_owned()));
        assert_eq!(text(2_890_000), ("2".to_owned(), ".8M".to_owned()));
    }
}
