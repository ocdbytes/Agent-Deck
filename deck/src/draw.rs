//! Draws the glyphs, logos and sprites in [`crate::assets`] over the black
//! background the deck clears to.

use embedded_graphics::{
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::*,
};

/// A color image: big-endian RGB565 pixels, row by row, already blended over
/// black. Black pixels are left undrawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Image {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: &'static [u8],
}

/// A monospaced font: one glyph per character of `chars`, in order, each
/// starting on a byte. A glyph is a bitmap of coverage values from 0
/// (background) to 15 (full color), two pixels per byte with the first in the
/// high nibble, row by row.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Font {
    pub(crate) chars: &'static str,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Rows from the top of a glyph to the baseline.
    pub(crate) baseline: u32,
    pub(crate) data: &'static [u8],
}

/// Frames of sprite animations: `sprites` sprites, each with `animations`
/// animations of `frames` frames. A frame is an [`Image`] of `width` by
/// `height` pixels; frames are back to back, frame by frame, animation by
/// animation and sprite by sprite.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpriteSheet {
    pub(crate) sprites: usize,
    pub(crate) animations: usize,
    pub(crate) frames: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: &'static [u8],
}

impl SpriteSheet {
    /// Returns frame `frame` of animation `animation` of sprite `sprite`, or
    /// `None` if the sheet has no such frame.
    pub(crate) fn frame(&self, sprite: usize, animation: usize, frame: usize) -> Option<Image> {
        if sprite >= self.sprites || animation >= self.animations || frame >= self.frames {
            return None;
        }
        let len = (self.width * self.height * 2) as usize;
        let index = (sprite * self.animations + animation) * self.frames + frame;
        Some(Image {
            width: self.width,
            height: self.height,
            data: self.data.get(index * len..(index + 1) * len)?,
        })
    }
}

impl Font {
    /// Returns the width of `text` in pixels.
    pub(crate) fn text_width(&self, text: &str) -> i32 {
        text.chars().count() as i32 * self.width as i32
    }

    /// Returns the glyph bitmap of `c`, or of `?` if the font has no `c`.
    fn glyph(&self, c: char) -> Option<&'static [u8]> {
        let index = self
            .chars
            .chars()
            .position(|glyph| glyph == c)
            .or_else(|| self.chars.chars().position(|glyph| glyph == '?'))?;
        let len = (self.width * self.height).div_ceil(2) as usize;
        self.data.get(index * len..(index + 1) * len)
    }

    /// Draws `glyph`, one of this font's glyph bitmaps, in `color` with its
    /// top-left corner at `top_left`.
    fn draw_glyph<D>(
        &self,
        target: &mut D,
        glyph: &[u8],
        top_left: Point,
        color: Rgb565,
    ) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let pixels = glyph
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0xF])
            .zip(0..self.width * self.height)
            .filter(|&(coverage, _)| coverage > 0)
            .map(|(coverage, i)| {
                let offset = Point::new((i % self.width) as i32, (i / self.width) as i32);
                Pixel(top_left + offset, fade(color, coverage))
            });
        target.draw_iter(pixels)
    }
}

/// Draws `image` with its top-left corner at `top_left`.
pub(crate) fn image<D>(target: &mut D, image: Image, top_left: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let pixels = image
        .data
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .zip(0..image.width * image.height)
        .filter(|&(raw, _)| raw != 0)
        .map(|(raw, i)| {
            let offset = Point::new((i % image.width) as i32, (i / image.width) as i32);
            Pixel(top_left + offset, Rgb565::from(RawU16::new(raw)))
        });
    target.draw_iter(pixels)
}

/// Draws `image` at twice its size with nearest-neighbor pixels.
pub(crate) fn image_2x<D>(target: &mut D, image: Image, top_left: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let pixels = image
        .data
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .zip(0..image.width * image.height)
        .filter(|&(raw, _)| raw != 0)
        .flat_map(|(raw, i)| {
            let color = Rgb565::from(RawU16::new(raw));
            let origin =
                top_left + Point::new((i % image.width) as i32 * 2, (i / image.width) as i32 * 2);
            [
                Pixel(origin, color),
                Pixel(origin + Point::new(1, 0), color),
                Pixel(origin + Point::new(0, 1), color),
                Pixel(origin + Point::new(1, 1), color),
            ]
        });
    target.draw_iter(pixels)
}

/// Draws `text` in `font` and `color`, starting at `origin` on the baseline,
/// and returns the x just after it.
pub(crate) fn text<D>(
    target: &mut D,
    font: &Font,
    text: &str,
    origin: Point,
    color: Rgb565,
) -> Result<i32, D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let top = origin.y - font.baseline as i32;
    let mut x = origin.x;
    for c in text.chars() {
        if let Some(glyph) = font.glyph(c) {
            font.draw_glyph(target, glyph, Point::new(x, top), color)?;
        }
        x += font.width as i32;
    }
    Ok(x)
}

/// Returns `color` at `coverage` (0 to 15) over black.
fn fade(color: Rgb565, coverage: u8) -> Rgb565 {
    let scale = |channel: u8| (u16::from(channel) * u16::from(coverage) / 15) as u8;
    Rgb565::new(scale(color.r()), scale(color.g()), scale(color.b()))
}
