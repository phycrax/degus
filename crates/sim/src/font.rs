//! A stand-in for the panel's fonts: one bitmap font, scaled into each
//! control's character cell. Font IDs are ignored.
//!
//! The glyphs are embedded-graphics' 10 × 20 Latin-1 font, which has the
//! 1 : 2 cell of DWIN's font 0. Characters past Latin-1 draw as an outlined
//! box.

use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::Infallible;

use embedded_graphics::Drawable;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::{DrawTarget, OriginDimensions, Point, Size};
use embedded_graphics::text::{Baseline, Text};
use image::{Rgb, RgbImage};

use crate::draw::{Rect, fill, stroke};

const GLYPH_W: u32 = 10;
const GLYPH_H: u32 = 20;

/// One glyph's pixels, row by row.
type Glyph = [bool; (GLYPH_W * GLYPH_H) as usize];

struct Canvas(Glyph);

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(GLYPH_W, GLYPH_H)
    }
}

impl DrawTarget for Canvas {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<BinaryColor>>,
    {
        for embedded_graphics::Pixel(point, color) in pixels {
            if (0..GLYPH_W as i32).contains(&point.x) && (0..GLYPH_H as i32).contains(&point.y) {
                self.0[(point.y as u32 * GLYPH_W + point.x as u32) as usize] = color.is_on();
            }
        }
        Ok(())
    }
}

thread_local! {
    static GLYPHS: RefCell<HashMap<char, Glyph>> = RefCell::default();
}

fn glyph(c: char) -> Glyph {
    GLYPHS.with(|glyphs| {
        *glyphs.borrow_mut().entry(c).or_insert_with(|| {
            let mut canvas = Canvas([false; (GLYPH_W * GLYPH_H) as usize]);
            let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
            let mut text = [0u8; 4];
            let _ = Text::with_baseline(
                c.encode_utf8(&mut text),
                Point::zero(),
                style,
                Baseline::Top,
            )
            .draw(&mut canvas);
            canvas.0
        })
    })
}

/// Draws `c` scaled into the `w` × `h` cell at `(x, y)`.
pub fn draw_char(fb: &mut RgbImage, x: i32, y: i32, w: u32, h: u32, c: char, color: Rgb<u8>) {
    if w == 0 || h == 0 || c.is_control() || c == ' ' {
        return;
    }
    if u32::from(c) > 0xFF {
        // Not in the font: an outlined box, inset a little.
        let inset = (w.min(h) / 8) as i32;
        let cell = Rect::new(
            x + inset,
            y + inset,
            w as i32 - 2 * inset,
            h as i32 - 2 * inset,
        );
        stroke(fb, cell, color);
        return;
    }
    let glyph = glyph(c);
    for ty in 0..h {
        let sy = ty * GLYPH_H / h;
        for tx in 0..w {
            let sx = tx * GLYPH_W / w;
            if glyph[(sy * GLYPH_W + sx) as usize] {
                fill(fb, Rect::new(x + tx as i32, y + ty as i32, 1, 1), color);
            }
        }
    }
}

/// Draws `text` left to right in `w` × `h` cells, `spacing` pixels apart.
pub fn draw_str(
    fb: &mut RgbImage,
    x: i32,
    y: i32,
    (w, h): (u32, u32),
    spacing: i32,
    text: &str,
    color: Rgb<u8>,
) {
    let mut at = x;
    for c in text.chars() {
        draw_char(fb, at, y, w, h, c, color);
        at += w as i32 + spacing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(fb: &RgbImage) -> usize {
        fb.pixels().filter(|p| p.0 != [0, 0, 0]).count()
    }

    #[test]
    fn draws_glyphs_scaled_into_the_cell() {
        let white = Rgb([255, 255, 255]);
        let mut small = RgbImage::new(40, 40);
        draw_char(&mut small, 0, 0, 10, 20, '8', white);
        let mut large = RgbImage::new(40, 40);
        draw_char(&mut large, 0, 0, 20, 40, '8', white);
        assert!(lit(&small) > 10);
        assert!(lit(&large) > 3 * lit(&small));
        // Nothing outside the cell.
        assert!(
            small
                .enumerate_pixels()
                .all(|(x, y, p)| (x < 10 && y < 20) || p.0 == [0, 0, 0])
        );
    }

    #[test]
    fn draws_a_box_for_characters_outside_the_font() {
        let mut fb = RgbImage::new(20, 20);
        draw_char(&mut fb, 0, 0, 16, 16, '中', Rgb([255, 0, 0]));
        assert_eq!(fb.get_pixel(2, 2), &Rgb([255, 0, 0]));
        assert_eq!(fb.get_pixel(8, 8), &Rgb([0, 0, 0]));
    }
}
