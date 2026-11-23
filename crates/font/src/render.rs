use std::ops::RangeInclusive;

use degus_glyph::{Face, Hinting};

use crate::bitmap::Bitmap;
use crate::error::{Error, Result};
use crate::font0::{FONT0_WIDTHS, Font0};

/// How [`Font0::render`] draws font 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Font0Options {
    /// The em size, in pixels per pixel of glyph width. `None` fits the
    /// font to the glyphs: the widest advance of the printable characters
    /// fills the width, or the font's line fills the height, whichever
    /// comes first.
    pub scale: Option<f32>,
    /// Hints the outlines for 1-bit pixels. Without hinting, stems come out
    /// one or two pixels wide by chance at small sizes.
    pub hinting: bool,
    /// The coverage, 1 to 255, from which a pixel is set. 0 counts as 1.
    pub threshold: u8,
}

impl Default for Font0Options {
    fn default() -> Self {
        Self {
            scale: None,
            hinting: true,
            threshold: 128,
        }
    }
}

/// The characters font 0 draws: printable ASCII, `!` to `~`.
const PRINTABLE: RangeInclusive<u8> = 0x21..=0x7E;

impl Font0 {
    /// Draws font 0 from a TrueType or OpenType font.
    ///
    /// Each printable character, `!` to `~`, is drawn at every width. At a
    /// width, the characters share a baseline that puts the font's line in
    /// the middle of the glyph's height, and each is centred in the glyph's
    /// width by its advance. Ink outside the glyph is cut off. The space,
    /// the control codes and the characters the font lacks stay blank.
    ///
    /// The display gives every character of font 0 the same width, so a
    /// monospaced font suits it best: with a proportional one, narrow
    /// characters stand alone in wide cells.
    pub fn render(face: &Face, options: &Font0Options) -> Result<Self> {
        let scale = options.scale.unwrap_or_else(|| fit(face));
        let hinting = if options.hinting {
            Hinting::Mono
        } else {
            Hinting::None
        };
        let threshold = options.threshold.max(1);
        let mut font = Font0::new();
        for width in FONT0_WIDTHS {
            let error = |source| Error::Render { width, source };
            let px = scale * f32::from(width);
            let scaler = face.scaler(px, hinting).map_err(error)?;
            let (w, h) = (i32::from(width), 2 * i32::from(width));
            let line = face.metrics(px);
            let baseline = (h - line.height().round() as i32) / 2 + line.ascent.round() as i32;
            for code in PRINTABLE {
                let Some(glyph) = scaler.render(char::from(code)).map_err(error)? else {
                    continue;
                };
                // The glyph's coverage box, in the cell.
                let x0 = ((w as f32 - glyph.advance) / 2.0).round() as i32 + glyph.left;
                let y0 = baseline - glyph.top;
                let bitmap = Bitmap::from_fn(width.into(), 2 * u16::from(width), |x, y| {
                    glyph.at(i32::from(x) - x0, i32::from(y) - y0) >= threshold
                });
                font.set(width, code, bitmap)
                    .expect("the bitmap has the width's glyph size");
            }
        }
        Ok(font)
    }
}

/// The em size, per pixel of glyph width, that fits `face` to font 0's
/// glyphs; see [`Font0Options::scale`].
fn fit(face: &Face) -> f32 {
    let advance = PRINTABLE
        .filter_map(|code| face.advance(char::from(code), 1.0))
        .fold(0.0, f32::max);
    let line = face.metrics(1.0).height();
    let by_width = if advance > 0.0 {
        1.0 / advance
    } else {
        f32::INFINITY
    };
    let by_height = if line > 0.0 {
        2.0 / line
    } else {
        f32::INFINITY
    };
    let scale = by_width.min(by_height);
    if scale.is_finite() { scale } else { 1.0 }
}
