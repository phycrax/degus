use std::fmt;
use std::ops::RangeInclusive;

use crate::bitmap::{Bitmap, row_len};
use crate::error::{Error, Result};

/// The glyph widths font 0 holds. Width *w* has glyphs of *w* × 2*w*.
pub const FONT0_WIDTHS: RangeInclusive<u8> = 4..=64;

/// The codes font 0 holds at every width: ASCII.
pub const FONT0_CODES: RangeInclusive<u8> = 0..=127;

/// The length of a font 0 file.
pub const FONT0_LEN: usize = file_len();

const WIDTHS: usize = (*FONT0_WIDTHS.end() - *FONT0_WIDTHS.start()) as usize + 1;
const CODES: usize = *FONT0_CODES.end() as usize + 1;

const fn file_len() -> usize {
    let mut len = 0;
    let mut w = *FONT0_WIDTHS.start() as usize;
    while w <= *FONT0_WIDTHS.end() as usize {
        len += CODES * 2 * w * w.div_ceil(8);
        w += 1;
    }
    len
}

/// Font 0: ASCII in every width from 4 to 64, each glyph *w* × 2*w*.
///
/// The file has no header. It holds the glyphs of width 4, codes 0 to 127,
/// then those of width 5, and so on to width 64: [`FONT0_LEN`] bytes, which
/// take file numbers 0 to 11. The display finds a glyph by its width and
/// code alone, so a font 0 holds every glyph, blank or not.
#[derive(Clone, PartialEq, Eq)]
pub struct Font0 {
    /// By width, then code.
    glyphs: Vec<Bitmap>,
}

impl Default for Font0 {
    fn default() -> Self {
        Self::new()
    }
}

/// `Font0 { glyphs: 4562 drawn }`: the full font is too big to print.
impl fmt::Debug for Font0 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let drawn = self.glyphs.iter().filter(|g| !g.is_blank()).count();
        write!(f, "Font0 {{ glyphs: {drawn} drawn }}")
    }
}

impl Font0 {
    /// A font 0 whose glyphs are all blank.
    pub fn new() -> Self {
        let glyphs = FONT0_WIDTHS
            .flat_map(|w| FONT0_CODES.map(move |_| Bitmap::new(w.into(), 2 * u16::from(w))))
            .collect();
        Self { glyphs }
    }

    /// The glyph of `code` at `width`. `None` when the font has no such
    /// width or code.
    pub fn glyph(&self, width: u8, code: u8) -> Option<&Bitmap> {
        let index = index(width, code).ok()?;
        Some(&self.glyphs[index])
    }

    /// Sets the glyph of `code` at `width`, returning the one it replaces.
    /// Fails on a width or code font 0 doesn't have, and on a glyph that
    /// isn't `width` × 2 `width`.
    pub fn set(&mut self, width: u8, code: u8, glyph: Bitmap) -> Result<Bitmap> {
        let index = index(width, code)?;
        if (glyph.width(), glyph.height()) != (width.into(), 2 * u16::from(width)) {
            return Err(Error::GlyphSize {
                width,
                got_width: glyph.width(),
                got_height: glyph.height(),
            });
        }
        Ok(std::mem::replace(&mut self.glyphs[index], glyph))
    }

    /// Every glyph with its width and code, by width, then code.
    pub fn glyphs(&self) -> impl Iterator<Item = (u8, u8, &Bitmap)> {
        FONT0_WIDTHS
            .flat_map(|w| FONT0_CODES.map(move |code| (w, code)))
            .zip(&self.glyphs)
            .map(|((w, code), glyph)| (w, code, glyph))
    }

    /// Reads a font 0 file. Bits past a glyph's width are ignored.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != FONT0_LEN {
            return Err(Error::Font0Length(bytes.len()));
        }
        let mut glyphs = Vec::with_capacity(WIDTHS * CODES);
        let mut offset = 0;
        for w in FONT0_WIDTHS {
            let (width, height) = (u16::from(w), 2 * u16::from(w));
            let len = row_len(width) * usize::from(height);
            for _ in FONT0_CODES {
                let rows = &bytes[offset..offset + len];
                glyphs.push(Bitmap::from_rows(width, height, rows)?);
                offset += len;
            }
        }
        Ok(Self { glyphs })
    }

    /// Writes the font 0 file.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(FONT0_LEN);
        for glyph in &self.glyphs {
            bytes.extend_from_slice(glyph.as_bytes());
        }
        bytes
    }
}

fn index(width: u8, code: u8) -> Result<usize> {
    if !FONT0_WIDTHS.contains(&width) {
        return Err(Error::Font0Width(width));
    }
    if !FONT0_CODES.contains(&code) {
        return Err(Error::Font0Code(code));
    }
    Ok(usize::from(width - FONT0_WIDTHS.start()) * CODES + usize::from(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_length_is_the_spec_s() {
        assert_eq!(FONT0_LEN, 3_082_752);
    }
}
