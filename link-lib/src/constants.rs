//! Sizes and separator bytes of the frame format.

/// Most sessions one frame carries.
pub const MAX_SESSIONS: usize = 16;

/// Longest session name in a frame, in bytes.
pub const MAX_TEXT_LEN: usize = 32;

/// Longest frame, in bytes, including the closing newline.
pub const MAX_FRAME_LEN: usize = BODY_START + CLOCK_LEN + MAX_SESSIONS * MAX_ENTRY_LEN + 1;

/// Clock digits and their separator at the start of a frame body.
const CLOCK_LEN: usize = 5;

/// Longest entry, in bytes: the three codes, four numbers and the name with
/// their separators, and the closing `RS`.
pub(crate) const MAX_ENTRY_LEN: usize = 3 + 4 * (1 + MAX_DIGITS) + 1 + MAX_TEXT_LEN + 1;

/// Most digits of a number in a frame: `u64::MAX` has 20.
pub(crate) const MAX_DIGITS: usize = 20;

/// Where the body starts: after the 4 checksum digits and a space.
pub(crate) const BODY_START: usize = 5;

/// Ends each entry in a frame body (ASCII record separator).
pub(crate) const RS: u8 = 0x1E;

/// Separates the fields of an entry (ASCII unit separator).
pub(crate) const US: u8 = 0x1F;
