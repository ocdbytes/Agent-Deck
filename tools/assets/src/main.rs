//! Renders the fonts and logos in `deck/assets/` and OpenAI's pets into the
//! bitmaps and images the deck draws, and writes `deck/src/assets.rs` to
//! describe them. Also renders the README banner, `docs/banner.png`.
//!
//! The pets are OpenAI's artwork, so they are not in git: this tool
//! downloads them with `curl` from where Codex does and writes them to the
//! ignored `deck/assets/gen/pets.bin`. Run it once after cloning, and after
//! changing a font, size, character set, logo or pet:
//!
//! ```sh
//! cargo run --manifest-path tools/assets/Cargo.toml
//! ```
//!
//! A font glyph is a bitmap of one coverage value from 0 (background) to 15
//! (full color) per pixel, two pixels per byte with the first in the high
//! nibble, row by row; a font's glyphs are back to back, each starting on a
//! byte. A logo is an image of big-endian RGB565 pixels, row by row, already
//! blended over the deck's black background. A sprite sheet is frames of
//! the same size stored like logos, back to back: frame by frame, animation
//! by animation and sprite by sprite.

use std::{error::Error, fmt::Write as _, fs, io::Cursor, path::Path, process::Command};

use fontdue::{Font, FontSettings};
use resvg::{tiny_skia, usvg};

/// Ark Pixel's 12 px monospaced Latin font, for body text, cut down to the
/// characters of [`FONTS`]' `BODY`. Drawn on a 12 px grid, every stroke is a
/// whole pixel, fully lit.
const ARK_PIXEL: &str = "fonts/arkpixel/ark-pixel-12px-monospaced-latin.ttf";

/// DotGothic16, a pixel font drawn on a 16 px grid, for numbers and kanji.
const DOTGOTHIC: &str = "fonts/dotgothic16/DotGothic16-Regular.ttf";

/// A font to render.
struct FontSpec {
    /// Constant name in `assets.rs`.
    name: &'static str,
    /// Doc comment for the constant.
    doc: &'static str,
    /// Font file, relative to `deck/assets/`.
    file: &'static str,
    /// Size in pixels per em.
    px: f32,
    /// Characters to include; the deck draws `?` for any other.
    chars: &'static str,
    /// Whether this is a pixel font drawn at its pixel size `px`: its glyphs
    /// are then read one font pixel at a time, fully on or off, with
    /// [`pixel_glyph`].
    pixel: bool,
}

/// The fonts the deck draws with. DotGothic16's pixels land almost on whole
/// pixels at 32 px, and are read one by one at 16 px, as is Ark Pixel at
/// its 12 px.
const FONTS: &[FontSpec] = &[
    FontSpec {
        name: "BODY",
        doc: "Ark Pixel at its 12 px pixel size, pixel for pixel, for body text: \
              ASCII and `…`.",
        file: ARK_PIXEL,
        px: 12.0,
        chars: " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`\
                abcdefghijklmnopqrstuvwxyz{|}~…",
        pixel: true,
    },
    FontSpec {
        name: "NUMERAL",
        doc: "DotGothic16 at twice its pixel size, for big numbers.",
        file: DOTGOTHIC,
        px: 32.0,
        chars: "0123456789-",
        pixel: false,
    },
    FontSpec {
        name: "PERCENT",
        doc: "DotGothic16's `%` at its 16 px pixel size, pixel for pixel, after big \
              numbers.",
        file: DOTGOTHIC,
        px: 16.0,
        chars: "%",
        pixel: true,
    },
    FontSpec {
        name: "KANJI",
        doc: "DotGothic16's kanji at its 16 px pixel size, pixel for pixel.",
        file: DOTGOTHIC,
        px: 16.0,
        chars: "稼働待機今",
        pixel: true,
    },
];

/// A logo to render from SVG.
struct LogoSpec {
    /// Constant name in `assets.rs`.
    name: &'static str,
    /// Doc comment for the constant.
    doc: &'static str,
    /// SVG file, relative to `deck/assets/`.
    file: &'static str,
    /// `0xRRGGBB` color for a one-color logo, or `None` to keep the SVG's
    /// colors, scaled up to full brightness.
    tint: Option<u32>,
    /// Whether the deck draws the logo at full size, not only at subagent
    /// size. Claude Code, pi and Codex show animations there instead.
    full_size: bool,
}

/// The logos the deck draws, one per agent.
const LOGOS: &[LogoSpec] = &[
    LogoSpec {
        name: "CLAUDE",
        doc: "Claude's logo, for Claude Code.",
        file: "logos/claude.svg",
        // Claude's orange `0xD97757` at full brightness, as the deck's
        // mascot.
        tint: Some(0xFF8C66),
        full_size: false,
    },
    LogoSpec {
        name: "PI",
        doc: "pi's logo, from the press kit at <https://pi.dev/press-kit>.",
        file: "logos/pi.svg",
        tint: None,
        full_size: false,
    },
    LogoSpec {
        name: "OPENAI",
        doc: "OpenAI's logo, for Codex.",
        file: "logos/openai.svg",
        // The deck's text color.
        tint: Some(0xFFFFFF),
        full_size: false,
    },
];

/// Width and height of every logo, in pixels. At 32 px the strokes of
/// Claude's and OpenAI's logos keep their shape as pixel art, and pi's
/// four-block grid lands on whole 8 px blocks at subagent size.
const LOGO_SIZE: usize = 32;

/// Width and height of the small logos drawn for subagents, in pixels: a
/// quarter of [`LOGO_SIZE`].
const SMALL_LOGO_SIZE: usize = LOGO_SIZE / 4;

/// Logos are rendered this many times larger, so each pixel's coverage is
/// measured over `SUPERSAMPLE`² samples.
const SUPERSAMPLE: usize = 8;

/// A logo pixel is on when at least this percentage of it is covered; below
/// half, so thin strokes stay unbroken.
const LOGO_CUTOFF: usize = 35;

/// Codex's built-in pets, as its terminal UI lists them in
/// `codex-rs/tui/src/pets/catalog.rs`.
const PETS: [&str; 8] = [
    "codex",
    "dewey",
    "fireball",
    "rocky",
    "seedy",
    "stacky",
    "bsod",
    "null-signal",
];

/// Where Codex downloads a pet's sprite sheet: this, the pet's name, then
/// [`PET_URL_SUFFIX`].
const PET_URL_PREFIX: &str = "https://persistent.oaistatic.com/codex/pets/v1/";

/// End of a pet sprite sheet's URL.
const PET_URL_SUFFIX: &str = "-spritesheet-v4.webp";

/// Width and height of one frame of a pet sprite sheet, in pixels.
const PET_FRAME: (usize, usize) = (192, 208);

/// Frames per row and rows of a pet sprite sheet.
const PET_GRID: (usize, usize) = (8, 9);

/// The sprite sheet rows the deck shows: Codex's `idle` animation, then its
/// `running` one (typing on a laptop) for busy sessions.
const PET_ROWS: [usize; 2] = [0, 7];

/// Frames in each of [`PET_ROWS`].
const PET_FRAMES: usize = 6;

/// Height of a pet on the deck, in pixels: the room between a tile's top
/// padding and the top of its status line.
const PET_HEIGHT: usize = 36;

/// How much to brighten the pets, whose art is too dark on the LCD: each
/// channel `c` becomes `255 * (c / 255)^(1 / PET_GAMMA)`, which lifts dark
/// and mid tones and keeps black and white. 1 keeps the art's colors.
const PET_GAMMA: f32 = 2.0;

/// The README banner's text.
const BANNER_TEXT: &str = "Agent Deck";

/// Width and height of the README banner, in pixels.
const BANNER_SIZE: (usize, usize) = (1280, 320);

/// Banner pixels per DotGothic16 pixel.
const BANNER_SCALE: usize = 12;

/// [`pixel_glyph`] rasterizes glyphs this many times their pixel size, so
/// the center of each font pixel lands well inside one bitmap pixel.
const PIXEL_SUPERSAMPLE: f32 = 8.0;

/// `0xRRGGBB` color of the banner text: the deck's text color.
const BANNER_COLOR: u32 = 0xFFFFFF;

fn main() -> Result<(), Box<dyn Error>> {
    let deck = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deck");
    let assets = deck.join("assets");
    let generated = assets.join("gen");
    fs::create_dir_all(&generated)?;
    let mut rust = String::from(
        "//! Bitmaps of the fonts and logos in `deck/assets/` and of OpenAI's pets,\n\
         //! generated by `tools/assets`. Do not edit; rerun\n\
         //! `cargo run --manifest-path tools/assets/Cargo.toml` instead.\n\n\
         use crate::draw::{Font, Image, SpriteSheet};\n",
    );

    for spec in FONTS {
        let font = Font::from_bytes(fs::read(assets.join(spec.file))?, FontSettings::default())?;
        let glyphs = render_font(&font, spec);
        let file = format!("{}.bin", spec.name.to_lowercase());
        let bytes: Vec<u8> = glyphs
            .coverage
            .iter()
            .flat_map(|glyph| pack(glyph))
            .collect();
        fs::write(generated.join(&file), bytes)?;
        writeln!(
            rust,
            "\n/// {}\npub(crate) const {}: Font = Font {{\n    chars: {:?},\n    \
             width: {},\n    height: {},\n    baseline: {},\n    \
             data: include_bytes!(\"../assets/gen/{file}\"),\n}};",
            spec.doc, spec.name, spec.chars, glyphs.width, glyphs.height, glyphs.baseline,
        )?;
    }

    for spec in LOGOS {
        let svg = fs::read(assets.join(spec.file))?;
        let full = (spec.name.to_owned(), spec.doc.to_owned(), LOGO_SIZE);
        let small = (
            format!("{}_SMALL", spec.name),
            format!("{} Quarter size, for subagents.", spec.doc),
            SMALL_LOGO_SIZE,
        );
        let sizes = spec.full_size.then_some(full).into_iter().chain([small]);
        for (name, doc, size) in sizes {
            let file = format!("{}.bin", name.to_lowercase());
            fs::write(generated.join(&file), render_logo(&svg, spec.tint, size)?)?;
            writeln!(
                rust,
                "\n/// {doc}\npub(crate) const {name}: Image = Image {{\n    \
                 width: {size},\n    height: {size},\n    \
                 data: include_bytes!(\"../assets/gen/{file}\"),\n}};",
            )?;
        }
    }

    let (width, pets) = render_pets()?;
    fs::write(generated.join("pets.bin"), pets)?;
    writeln!(
        rust,
        "\n/// OpenAI's pets, downloaded by `tools/assets` and not in\n\
         /// git: {} pets, each with an idle and a busy animation.\n\
         pub(crate) const PETS: SpriteSheet = SpriteSheet {{\n    \
         sprites: {},\n    animations: {},\n    frames: {PET_FRAMES},\n    \
         width: {width},\n    height: {PET_HEIGHT},\n    \
         data: include_bytes!(\"../assets/gen/pets.bin\"),\n}};",
        PETS.len(),
        PETS.len(),
        PET_ROWS.len(),
    )?;

    fs::write(deck.join("src/assets.rs"), rust)?;

    let dotgothic = Font::from_bytes(fs::read(assets.join(DOTGOTHIC))?, FontSettings::default())?;
    let docs = deck.join("../docs");
    fs::create_dir_all(&docs)?;
    render_banner(&dotgothic)?.save_png(docs.join("banner.png"))?;
    Ok(())
}

/// Downloads OpenAI's pets and returns their width on the deck and their
/// frames, as a sprite sheet of [`PET_ROWS`] animations per pet.
///
/// Every frame is cut to the box that holds the pets in all the frames used,
/// so they keep their sizes relative to each other, and scaled down to
/// [`PET_HEIGHT`] by averaging the pixels each deck pixel covers, then
/// brightened by [`PET_GAMMA`].
fn render_pets() -> Result<(usize, Vec<u8>), Box<dyn Error>> {
    let (frame_w, frame_h) = PET_FRAME;
    let (columns, rows) = PET_GRID;
    let sheets = PETS
        .iter()
        .map(|pet| download_sheet(&format!("{PET_URL_PREFIX}{pet}{PET_URL_SUFFIX}")))
        .collect::<Result<Vec<_>, _>>()?;
    let sheet_w = frame_w * columns;
    if let Some(sheet) = sheets
        .iter()
        .find(|sheet| sheet.len() != sheet_w * frame_h * rows * 4)
    {
        return Err(format!(
            "a pet sprite sheet has {} bytes, not the expected size",
            sheet.len()
        )
        .into());
    }
    // Top-left corner of each frame used, in sheet pixels.
    let frames: Vec<(usize, usize)> = PET_ROWS
        .iter()
        .flat_map(|&row| (0..PET_FRAMES).map(move |column| (column * frame_w, row * frame_h)))
        .collect();

    // The box, in frame pixels, around every pixel that is not transparent.
    let (mut left, mut top, mut right, mut bottom) = (frame_w, frame_h, 0, 0);
    for sheet in &sheets {
        for &(x0, y0) in &frames {
            for y in 0..frame_h {
                for x in 0..frame_w {
                    if sheet[((y0 + y) * sheet_w + x0 + x) * 4 + 3] > 0 {
                        (left, top) = (left.min(x), top.min(y));
                        (right, bottom) = (right.max(x + 1), bottom.max(y + 1));
                    }
                }
            }
        }
    }
    if left >= right {
        return Err("the pet sprite sheets are empty".into());
    }
    let (box_w, box_h) = (right - left, bottom - top);
    let width = (box_w * PET_HEIGHT).div_ceil(box_h);

    let mut out = Vec::new();
    for sheet in &sheets {
        for &(x0, y0) in &frames {
            for y in 0..PET_HEIGHT {
                let (sy0, sy1) = (y * box_h / PET_HEIGHT, (y + 1) * box_h / PET_HEIGHT);
                for x in 0..width {
                    let (sx0, sx1) = (x * box_w / width, ((x + 1) * box_w / width).min(box_w));
                    // Premultiplied red, green and blue, which is the color
                    // over black.
                    let mut sum = [0u32; 3];
                    for sy in sy0..sy1 {
                        for sx in sx0..sx1 {
                            let i = ((y0 + top + sy) * sheet_w + x0 + left + sx) * 4;
                            let alpha = u32::from(sheet[i + 3]);
                            for (total, &channel) in sum.iter_mut().zip(&sheet[i..i + 3]) {
                                *total += u32::from(channel) * alpha / 255;
                            }
                        }
                    }
                    let count = ((sy1 - sy0) * (sx1 - sx0)).max(1) as u32;
                    let [r, g, b] = sum.map(|total| {
                        let channel = total as f32 / count as f32 / 255.0;
                        (255.0 * channel.powf(PET_GAMMA.recip())).round() as u8
                    });
                    out.extend(rgb565(r, g, b).to_be_bytes());
                }
            }
        }
    }
    Ok((width, out))
}

/// Downloads the WebP image at `url` and returns its RGBA pixels, row by row.
fn download_sheet(url: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "60",
            url,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!("curl {url}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(output.stdout))?;
    if !decoder.has_alpha() {
        return Err(format!("{url} has no alpha channel").into());
    }
    let mut pixels = vec![
        0;
        decoder
            .output_buffer_size()
            .ok_or("pet sprite sheet too large")?
    ];
    decoder.read_image(&mut pixels)?;
    Ok(pixels)
}

/// Renders [`BANNER_TEXT`] in DotGothic16's square pixels, scaled up by
/// [`BANNER_SCALE`] and centered on a black [`BANNER_SIZE`] banner.
fn render_banner(font: &Font) -> Result<tiny_skia::Pixmap, Box<dyn Error>> {
    // Lit font pixels as (column, row up from the baseline).
    let mut dots = Vec::new();
    let mut pen = 0;
    for c in BANNER_TEXT.chars() {
        let (glyph, advance) = pixel_glyph(font, c, 16.0);
        dots.extend(glyph.into_iter().map(|(x, y)| (pen + x, y)));
        pen += advance;
    }

    let min_x = dots
        .iter()
        .map(|&(x, _)| x)
        .min()
        .ok_or("empty banner text")?;
    let max_x = dots.iter().map(|&(x, _)| x).max().unwrap_or(min_x);
    let min_y = dots.iter().map(|&(_, y)| y).min().unwrap_or(0);
    let max_y = dots.iter().map(|&(_, y)| y).max().unwrap_or(min_y);
    let (banner_w, banner_h) = BANNER_SIZE;
    let scale = BANNER_SCALE as i32;
    let left = (banner_w as i32 - (max_x - min_x + 1) * scale) / 2;
    let top = (banner_h as i32 - (max_y - min_y + 1) * scale) / 2;
    if left < 0 || top < 0 {
        return Err("banner text does not fit BANNER_SIZE".into());
    }

    let mut pixmap =
        tiny_skia::Pixmap::new(banner_w as u32, banner_h as u32).ok_or("empty banner size")?;
    pixmap.fill(tiny_skia::Color::BLACK);
    let [r, g, b] = [16, 8, 0].map(|shift| (BANNER_COLOR >> shift & 0xFF) as u8);
    let color = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, 255).ok_or("bad color")?;
    let pixels = pixmap.pixels_mut();
    for (x, y) in dots {
        let x = (left + (x - min_x) * scale) as usize;
        for row in 0..BANNER_SCALE {
            let start = (top + (max_y - y) * scale) as usize * banner_w + row * banner_w + x;
            pixels[start..start + BANNER_SCALE].fill(color);
        }
    }
    Ok(pixmap)
}

/// A font's glyphs, each in a cell of the same size.
struct Glyphs {
    width: usize,
    height: usize,
    /// Rows from the top of a cell to the baseline.
    baseline: usize,
    /// Coverage (0 to 15) of each glyph's cell, row by row.
    coverage: Vec<Vec<u8>>,
}

/// Renders `spec`'s characters into equal cells: as wide as the widest
/// advance, and as tall as all glyphs together above and below the baseline.
fn render_font(font: &Font, spec: &FontSpec) -> Glyphs {
    if spec.pixel {
        return render_pixel_font(font, spec);
    }
    let rendered: Vec<_> = spec
        .chars
        .chars()
        .map(|c| font.rasterize(c, spec.px))
        .collect();
    let width = rendered
        .iter()
        .map(|(metrics, _)| metrics.advance_width.round() as usize)
        .max()
        .unwrap_or(0);
    // Glyph rows run from -(ymin + height) to -ymin, relative to the baseline.
    let top = rendered
        .iter()
        .map(|(m, _)| -(m.ymin + m.height as i32))
        .min()
        .unwrap_or(0);
    let bottom = rendered.iter().map(|(m, _)| -m.ymin).max().unwrap_or(0);
    let height = (bottom - top) as usize;
    let coverage = rendered
        .iter()
        .map(|(metrics, bitmap)| {
            let mut cell = vec![0; width * height];
            for (i, &value) in bitmap.iter().enumerate() {
                let x = metrics.xmin + (i % metrics.width) as i32;
                let y = -(metrics.ymin + metrics.height as i32) + (i / metrics.width) as i32 - top;
                // Parts that overhang the cell are cut off.
                if (0..width as i32).contains(&x) {
                    cell[y as usize * width + x as usize] = quantize(value);
                }
            }
            cell
        })
        .collect();
    Glyphs {
        width,
        height,
        baseline: (-top) as usize,
        coverage,
    }
}

/// Renders `spec`'s characters from a pixel font into equal cells, each font
/// pixel fully on: as wide as the widest advance, and as tall as all glyphs
/// together above and below the baseline.
fn render_pixel_font(font: &Font, spec: &FontSpec) -> Glyphs {
    let glyphs: Vec<_> = spec
        .chars
        .chars()
        .map(|c| pixel_glyph(font, c, spec.px))
        .collect();
    let width = glyphs
        .iter()
        .map(|&(_, advance)| advance)
        .max()
        .unwrap_or(0)
        .max(0) as usize;
    let rows = || {
        glyphs
            .iter()
            .flat_map(|(dots, _)| dots.iter().map(|&(_, y)| y))
    };
    let top = rows().max().map_or(0, |y| y + 1);
    let bottom = rows().min().unwrap_or(0);
    let height = (top - bottom) as usize;
    let coverage = glyphs
        .iter()
        .map(|(dots, _)| {
            let mut cell = vec![0; width * height];
            for &(x, y) in dots {
                // Parts that overhang the cell are cut off.
                if (0..width as i32).contains(&x) {
                    cell[(top - 1 - y) as usize * width + x as usize] = 15;
                }
            }
            cell
        })
        .collect();
    Glyphs {
        width,
        height,
        baseline: top as usize,
        coverage,
    }
}

/// Returns the glyph of `c` in pixel font `font` at its pixel size `px`: the
/// font pixels that are on, as (column right of the origin, row up from the
/// baseline, with 0 just above it), and the advance in pixels.
///
/// A pixel font's grid can sit a fraction of a pixel off the bitmap's, and
/// DotGothic16's sits about half a pixel off, which smears each font pixel
/// over two. So the glyph is rasterized [`PIXEL_SUPERSAMPLE`] times larger
/// and each font pixel is read at its center.
fn pixel_glyph(font: &Font, c: char, px: f32) -> (Vec<(i32, i32)>, i32) {
    let size = PIXEL_SUPERSAMPLE;
    let (metrics, bitmap) = font.rasterize(c, px * size);
    let bounds = metrics.bounds;
    let columns = (bounds.width / size).round() as i32;
    let rows = (bounds.height / size).round() as i32;
    let left = (bounds.xmin / size).round() as i32;
    let top = ((bounds.ymin + bounds.height) / size).round() as i32 - 1;
    let mut dots = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            // The center of this font pixel, in rasterized pixels.
            let x = bounds.xmin + (column as f32 + 0.5) * size;
            let y = bounds.ymin + bounds.height - (row as f32 + 0.5) * size;
            let bitmap_x = x.floor() as i32 - metrics.xmin;
            let bitmap_y = metrics.ymin + metrics.height as i32 - 1 - y.floor() as i32;
            let i = bitmap_y as usize * metrics.width + bitmap_x as usize;
            if bitmap.get(i).is_some_and(|&value| value >= 128) {
                dots.push((left + column, top - row));
            }
        }
    }
    (dots, (metrics.advance_width / size).round() as i32)
}

/// Renders an SVG logo into a `size` square of big-endian RGB565
/// pixels, as pixel art: each pixel is either off (black) or fully on, in
/// `tint` or in the SVG's own color there at full brightness.
///
/// Anti-aliased edges look soft at this size, so a pixel is on when at
/// least [`LOGO_CUTOFF`] percent of it is covered. The logo's shapes, not its
/// SVG canvas, are scaled to fill the square, so logos with different
/// margins come out the same size.
fn render_logo(svg: &[u8], tint: Option<u32>, size: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default())?;
    let bounds = tree.root().abs_bounding_box();
    let big = size * SUPERSAMPLE;
    let scale = big as f32 / bounds.width().max(bounds.height());
    let x = (big as f32 - bounds.width() * scale) / 2.0 - bounds.x() * scale;
    let y = (big as f32 - bounds.height() * scale) / 2.0 - bounds.y() * scale;
    let mut pixmap = tiny_skia::Pixmap::new(big as u32, big as u32).ok_or("empty logo size")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, x, y),
        &mut pixmap.as_mut(),
    );

    let samples = pixmap.pixels();
    let mut out = Vec::with_capacity(size * size * 2);
    for i in 0..size * size {
        let (left, top) = (i % size * SUPERSAMPLE, i / size * SUPERSAMPLE);
        // Sum of alpha and of premultiplied red, green and blue under the pixel.
        let mut sum = [0u32; 4];
        for row in top..top + SUPERSAMPLE {
            for sample in &samples[row * big + left..row * big + left + SUPERSAMPLE] {
                let channels = [sample.alpha(), sample.red(), sample.green(), sample.blue()];
                for (total, channel) in sum.iter_mut().zip(channels) {
                    *total += u32::from(channel);
                }
            }
        }
        let [alpha, red, green, blue] = sum;
        let full = (SUPERSAMPLE * SUPERSAMPLE * 255) as u32;
        let [r, g, b] = if alpha * 100 < LOGO_CUTOFF as u32 * full {
            [0, 0, 0]
        } else if let Some(tint) = tint {
            [16, 8, 0].map(|shift| (tint >> shift & 0xFF) as u8)
        } else {
            // Average color of the covered samples, scaled up so its
            // brightest channel is full, as the LCD shows darker colors
            // dim. The ratios take alpha out.
            let max = red.max(green).max(blue).max(1);
            [red, green, blue].map(|channel| (channel * 255 / max) as u8)
        };
        out.extend(rgb565(r, g, b).to_be_bytes());
    }
    Ok(out)
}

/// Converts an 8-bit-per-channel color to RGB565, rounding to nearest.
fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    let scale = |channel: u8, max: u16| (u16::from(channel) * max + 127) / 255;
    scale(r, 31) << 11 | scale(g, 63) << 5 | scale(b, 31)
}

/// Scales an 8-bit coverage value to 4 bits, rounding to nearest.
fn quantize(value: u8) -> u8 {
    ((u16::from(value) * 15 + 127) / 255) as u8
}

/// Packs coverage values two to a byte, the first in the high nibble.
fn pack(coverage: &[u8]) -> Vec<u8> {
    coverage
        .chunks(2)
        .map(|pair| pair[0] << 4 | pair.get(1).copied().unwrap_or(0))
        .collect()
}
