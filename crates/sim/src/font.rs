//! Characters: font 0 from the set, the one font the simulator draws. A
//! character that needs another font, a width font 0 lacks, or a font 0
//! the set lacks draws as an outlined box of its cell.

use degus_set::font::{Bitmap, FONT0_WIDTHS, Font0};
use degus_set::icl::image::{Rgb, RgbImage};

use crate::draw::{Rect, plot, stroke};

/// What a character draws as.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Mark<'a> {
    /// A font 0 glyph, `width` × 2 `width`.
    Glyph(&'a Bitmap),
    /// A box standing in for the glyph.
    Box { w: u16, h: u16 },
    /// Nothing, such as a space the simulator has no glyph for.
    Blank { w: u16 },
}

impl<'a> Mark<'a> {
    /// The mark of character `code` drawn with font `font` in a cell of
    /// `w` × `h`.
    // ASSUME: font 0 glyphs draw at their own size, `w` × 2 `w`, whatever
    // height the control gives.
    pub fn new(font0: Option<&'a Font0>, font: u8, code: u32, w: u8, h: u16) -> Self {
        if font == 0
            && let Some(font0) = font0
            && let Ok(code) = u8::try_from(code)
            && FONT0_WIDTHS.contains(&w)
            && let Some(glyph) = font0.glyph(w, code)
        {
            return Mark::Glyph(glyph);
        }
        if code == u32::from(b' ') || char::from_u32(code).is_some_and(char::is_control) {
            Mark::Blank { w: w.into() }
        } else {
            Mark::Box { w: w.into(), h }
        }
    }

    /// The width of the whole cell.
    pub fn width(&self) -> u16 {
        match *self {
            Mark::Glyph(glyph) => glyph.width(),
            Mark::Box { w, .. } | Mark::Blank { w } => w,
        }
    }

    /// The columns that draw anything, as `start..end`; `None` when none
    /// does. Boxes fill their cell.
    pub fn ink(&self) -> Option<(u16, u16)> {
        match *self {
            Mark::Glyph(glyph) => {
                let lit = |x: u16| (0..glyph.height()).any(|y| glyph.get(x, y));
                let start = (0..glyph.width()).find(|&x| lit(x))?;
                let end = (0..glyph.width()).rev().find(|&x| lit(x))? + 1;
                Some((start, end))
            }
            Mark::Box { w, .. } => (w > 0).then_some((0, w)),
            Mark::Blank { .. } => None,
        }
    }

    /// Draws columns `from..` of the mark with its top left corner at
    /// `(x, y)`, inside `clip`.
    pub fn draw(&self, fb: &mut RgbImage, x: i32, y: i32, from: u16, clip: Rect, color: Rgb<u8>) {
        match *self {
            Mark::Glyph(glyph) => {
                for gy in 0..glyph.height() {
                    for gx in from..glyph.width() {
                        if glyph.get(gx, gy) {
                            let px = x + i32::from(gx - from);
                            plot(fb, clip, px, y + i32::from(gy), color);
                        }
                    }
                }
            }
            Mark::Box { w, h } => {
                // Inset a little, so neighbouring boxes stay apart.
                let inset = i32::from(w.min(h) / 8);
                let cell = Rect::new(
                    x + inset,
                    y + inset,
                    i32::from(w) - 2 * inset,
                    i32::from(h) - 2 * inset,
                );
                stroke(fb, cell, clip, color);
            }
            Mark::Blank { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font0() -> Font0 {
        let mut font = Font0::new();
        // 'I' at width 4: one column, x = 1.
        let glyph = Bitmap::from_fn(4, 8, |x, _| x == 1);
        font.set(4, b'I', glyph).unwrap();
        font
    }

    #[test]
    fn takes_glyphs_from_font_0_and_boxes_the_rest() {
        let font = font0();
        assert!(matches!(
            Mark::new(Some(&font), 0, 'I'.into(), 4, 8),
            Mark::Glyph(_)
        ));
        // Another font, a width font 0 lacks, a code past ASCII, no font 0.
        for (font0, font_id, code, w) in [
            (Some(&font), 1, 'I' as u32, 4),
            (Some(&font), 0, 'I' as u32, 3),
            (Some(&font), 0, '中' as u32, 4),
            (None, 0, 'I' as u32, 4),
        ] {
            let mark = Mark::new(font0, font_id, code, w, 8);
            assert!(matches!(mark, Mark::Box { .. }), "{font_id} {code} {w}");
        }
        assert!(matches!(
            Mark::new(None, 0, ' '.into(), 4, 8),
            Mark::Blank { w: 4 }
        ));
    }

    #[test]
    fn finds_the_inked_columns() {
        let font = font0();
        let glyph = Mark::new(Some(&font), 0, 'I'.into(), 4, 8);
        assert_eq!(glyph.ink(), Some((1, 2)));
        // A blank glyph of font 0 draws nothing.
        assert_eq!(Mark::new(Some(&font), 0, 'J'.into(), 4, 8).ink(), None);
        assert_eq!(Mark::Box { w: 6, h: 12 }.ink(), Some((0, 6)));
    }

    #[test]
    fn draws_glyphs_and_boxes_inside_the_clip() {
        let font = font0();
        let white = Rgb([255, 255, 255]);
        let mut fb = RgbImage::new(20, 20);
        let all = Rect::of(&fb);
        Mark::new(Some(&font), 0, 'I'.into(), 4, 8).draw(&mut fb, 2, 2, 0, all, white);
        assert_eq!(fb.get_pixel(3, 2), &white);
        assert_eq!(fb.get_pixel(2, 2), &Rgb([0, 0, 0]));
        // Drawn from its inked column on.
        let mut fb = RgbImage::new(20, 20);
        Mark::new(Some(&font), 0, 'I'.into(), 4, 8).draw(&mut fb, 2, 2, 1, all, white);
        assert_eq!(fb.get_pixel(2, 2), &white);

        let mut fb = RgbImage::new(20, 20);
        Mark::Box { w: 16, h: 16 }.draw(&mut fb, 0, 0, 0, Rect::new(0, 0, 8, 20), white);
        assert_eq!(fb.get_pixel(2, 2), &white);
        assert_eq!(fb.get_pixel(13, 2), &Rgb([0, 0, 0]));
        assert_eq!(fb.get_pixel(8, 8), &Rgb([0, 0, 0]));
    }
}
