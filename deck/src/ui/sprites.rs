//! The animations drawn in place of a session's logo: Claude Code's mascot,
//! and for Codex one of the pets from its terminal UI.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use link_lib::Status;

use super::rgb;
use crate::{FPS, assets::PETS, draw};

/// Claude's orange: its logo's `0xD97757` scaled up to full brightness, as
/// the LCD shows darker colors dim.
const CLAUDE_ORANGE: Rgb565 = rgb(0xFF8C66);

/// One pose of Claude Code's mascot: three rows of nine quadrant block
/// characters, the way Claude Code draws it in the terminal.
type Pose = [&'static str; 3];

/// The mascot as Claude Code's welcome screen shows it.
const IDLE: Pose = [" ▐▛███▜▌ ", "▝▜█████▛▘", "  ▘▘ ▝▝  "];

/// Eyes closed.
const BLINK: Pose = [" ▐█████▌ ", "▝▜█████▛▘", "  ▘▘ ▝▝  "];

/// Eyes moved one quadrant left.
const LOOK_LEFT: Pose = [" ▐▜██▛█▌ ", "▝▜█████▛▘", "  ▘▘ ▝▝  "];

/// Eyes moved one quadrant right.
const LOOK_RIGHT: Pose = [" ▐█▜██▛▌ ", "▝▜█████▛▘", "  ▘▘ ▝▝  "];

/// Legs moved one quadrant towards the middle, for stepping in place.
const STEP: Pose = [" ▐▛███▜▌ ", "▝▜█████▛▘", "  ▝▝ ▘▘  "];

/// How long each of the mascot's moves lasts, in milliseconds. A move that
/// ends sooner shows [`IDLE`] for the rest.
const MOVE_MS: u64 = 2500;

/// What the mascot can do: each move is poses with how long each shows, in
/// milliseconds. Every [`MOVE_MS`] each mascot picks its next move at
/// random.
const MOVES: [&[(Pose, u64)]; 7] = [
    // Rest.
    &[],
    &[(IDLE, 1200), (BLINK, 150)],
    &[(IDLE, 800), (BLINK, 120), (IDLE, 150), (BLINK, 120)],
    &[(IDLE, 500), (LOOK_LEFT, 1000)],
    &[(IDLE, 500), (LOOK_RIGHT, 1000)],
    &[(LOOK_LEFT, 700), (IDLE, 200), (LOOK_RIGHT, 700)],
    &[
        (STEP, 250),
        (IDLE, 250),
        (STEP, 250),
        (IDLE, 250),
        (STEP, 250),
    ],
];

/// Quadrant block characters, indexed by the quadrants they fill: top left
/// 8, top right 4, bottom left 2 and bottom right 1.
const QUADRANTS: &str = " ▗▖▄▝▐▞▟▘▚▌▙▀▜▛█";

/// Size of one quadrant of a mascot character, in pixels: a quarter of a
/// cell twice as tall as wide, like a terminal's.
const QUADRANT: Size = Size::new(2, 4);

/// Size of the mascot, in pixels: nine by three characters of two by two
/// quadrants.
const MASCOT: Size = Size::new(9 * 2 * QUADRANT.width, 3 * 2 * QUADRANT.height);

/// A pet's idle animation, as Codex plays it: each frame of the sheet's idle
/// animation and how long it shows, in milliseconds.
const PET_IDLE: [(usize, u64); 6] = [(0, 1680), (1, 660), (2, 660), (3, 840), (4, 840), (5, 1920)];

/// A pet's animation while its session is busy: Codex's `running` animation,
/// typing on a laptop, with its frame times.
const PET_BUSY: [(usize, u64); 6] = [(0, 120), (1, 120), (2, 120), (3, 120), (4, 120), (5, 220)];

/// Size of the area an animation is drawn in, in pixels: the larger of a pet
/// and the mascot in each direction.
pub(super) const SIZE: Size = Size::new(
    if PETS.width > MASCOT.width {
        PETS.width
    } else {
        MASCOT.width
    },
    if PETS.height > MASCOT.height {
        PETS.height
    } else {
        MASCOT.height
    },
);

/// Draws the Claude Code mascot of the session with [`session_hash`] `hash`
/// as it is `tick` ticks into its animation, centered in the [`SIZE`] area
/// whose top-left corner is `top_left`.
pub(super) fn draw_claude<D>(
    target: &mut D,
    hash: u32,
    tick: u32,
    top_left: Point,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let pose = claude_pose(hash, tick);
    let origin = top_left + (SIZE - MASCOT) / 2;
    for (row, line) in (0..).zip(pose) {
        for (column, c) in (0..).zip(line.chars()) {
            let filled = QUADRANTS.chars().position(|q| q == c).unwrap_or(0);
            for quadrant in (0..4).filter(|quadrant| filled & 8 >> quadrant != 0) {
                let offset = Point::new(
                    ((column * 2 + quadrant % 2) * QUADRANT.width) as i32,
                    ((row * 2 + quadrant / 2) * QUADRANT.height) as i32,
                );
                target.fill_solid(&Rectangle::new(origin + offset, QUADRANT), CLAUDE_ORANGE)?;
            }
        }
    }
    Ok(())
}

/// Draws the Codex pet of the session with [`session_hash`] `hash` as it is
/// `tick` ticks into the animation for `status`, centered in the [`SIZE`]
/// area whose top-left corner is `top_left`.
pub(super) fn draw_pet<D>(
    target: &mut D,
    hash: u32,
    status: Status,
    tick: u32,
    top_left: Point,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let (animation, frames) = if status == Status::Busy {
        (1, &PET_BUSY)
    } else {
        (0, &PET_IDLE)
    };
    let total: u64 = frames.iter().map(|&(_, ms)| ms).sum();
    // Starting each pet at its own point keeps two of the same apart.
    let ms = (millis(tick) + u64::from(hash)) % total;
    let pet = hash as usize % PETS.sprites;
    let Some(image) = frame_at(frames, ms).and_then(|frame| PETS.frame(pet, animation, frame))
    else {
        return Ok(());
    };
    let size = Size::new(image.width, image.height);
    draw::image(target, image, top_left + (SIZE - size) / 2)
}

/// Returns the hash that picks the random animation of the `nth` session
/// named `name`, counting from 0 in the order the Mac sends them: the
/// mascot's moves for Claude Code, and the pet for Codex.
///
/// Sessions can share a name, such as two in one directory, so `nth` keeps
/// theirs apart. The hash stays the same while the session runs, and a random
/// `seed` gives different picks on each run.
pub(crate) fn session_hash(name: &str, nth: usize, seed: u32) -> u32 {
    fnv(fnv(0x811C_9DC5 ^ seed, name.as_bytes()), &nth.to_le_bytes())
}

/// Returns the pose of the mascot of the session with [`session_hash`]
/// `hash`, `tick` ticks in.
///
/// Each mascot starts at its own point in time and picks its moves from its
/// own random sequence, so no two move together.
fn claude_pose(hash: u32, tick: u32) -> Pose {
    let ms = millis(tick) + u64::from(hash);
    let slot = ms / MOVE_MS;
    let pick = fnv(hash, &slot.to_le_bytes()) as usize % MOVES.len();
    frame_at(MOVES[pick], ms % MOVE_MS).unwrap_or(IDLE)
}

/// Returns the item of `frames` that shows `ms` milliseconds in, where each
/// item shows for its number of milliseconds, or `None` once all have shown.
fn frame_at<T: Copy>(frames: &[(T, u64)], mut ms: u64) -> Option<T> {
    for &(item, length) in frames {
        if ms < length {
            return Some(item);
        }
        ms -= length;
    }
    None
}

/// Returns how long `tick` ticks last, in milliseconds.
fn millis(tick: u32) -> u64 {
    u64::from(tick) * 1000 / u64::from(FPS)
}

/// Returns the FNV-1a hash of `bytes`, continuing from `hash`.
fn fnv(hash: u32, bytes: &[u8]) -> u32 {
    bytes.iter().fold(hash, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_at_follows_frame_times() {
        let frames = [('a', 100), ('b', 50)];
        assert_eq!(frame_at(&frames, 0), Some('a'));
        assert_eq!(frame_at(&frames, 99), Some('a'));
        assert_eq!(frame_at(&frames, 100), Some('b'));
        assert_eq!(frame_at(&frames, 149), Some('b'));
        assert_eq!(frame_at(&frames, 150), None);
    }

    #[test]
    fn mascots_move_apart() {
        let minute = |hash| (0..60 * FPS).map(move |tick| claude_pose(hash, tick));
        let (first, second) = (session_hash("deck", 0, 7), session_hash("deck", 1, 7));
        assert_ne!(first, second, "sessions sharing a name share a hash");
        assert!(
            minute(first).ne(minute(second)),
            "two mascots move together"
        );
        let mut poses: [Pose; 5] = [IDLE; 5];
        let mut seen = 0;
        for pose in minute(first) {
            if !poses[..seen].contains(&pose) {
                poses[seen] = pose;
                seen += 1;
            }
        }
        assert_eq!(
            seen, 5,
            "a mascot shows only {seen} of its 5 poses in a minute"
        );
    }
}
