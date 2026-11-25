//! Drawing into the framebuffer, clipped to it.

use degus_set::controls::Area;
use degus_set::icl::image::{Rgb, RgbImage};

/// An RGB565 colour as the panel shows it, the low bits filled from the
/// high ones.
pub fn rgb565(color: u16) -> Rgb<u8> {
    let r = (color >> 11 & 0x1F) as u8;
    let g = (color >> 5 & 0x3F) as u8;
    let b = (color & 0x1F) as u8;
    Rgb([r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2])
}

/// A rectangle that may lie partly off screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// A control area; its bottom right corner lies just outside it.
    pub fn from_area(area: Area) -> Self {
        Self::new(
            area.top_left.x.into(),
            area.top_left.y.into(),
            area.width().into(),
            area.height().into(),
        )
    }

    pub fn of(image: &RgbImage) -> Self {
        Self::new(0, 0, image.width() as i32, image.height() as i32)
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn right(self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(self) -> i32 {
        self.y + self.h
    }

    /// The part inside both rectangles, possibly empty.
    pub fn intersect(self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let w = (self.right().min(other.right()) - x).max(0);
        let h = (self.bottom().min(other.bottom()) - y).max(0);
        Rect::new(x, y, w, h)
    }

    /// The part inside a `w` × `h` image, as pixel ranges.
    fn clip(self, w: u32, h: u32) -> Option<(std::ops::Range<u32>, std::ops::Range<u32>)> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = (self.x + self.w).min(w as i32);
        let y1 = (self.y + self.h).min(h as i32);
        (x0 < x1 && y0 < y1).then_some((x0 as u32..x1 as u32, y0 as u32..y1 as u32))
    }
}

pub(crate) fn fill(fb: &mut RgbImage, rect: Rect, color: Rgb<u8>) {
    let Some((xs, ys)) = rect.clip(fb.width(), fb.height()) else {
        return;
    };
    // One row, then copies of it: fast even unoptimized.
    let row = color.0.repeat(xs.len());
    let width = fb.width() as usize;
    let raw: &mut [u8] = fb.as_mut();
    for y in ys {
        let start = (y as usize * width + xs.start as usize) * 3;
        raw[start..start + row.len()].copy_from_slice(&row);
    }
}

/// Sets the pixel at `(x, y)` if it is inside `clip` and the framebuffer.
pub(crate) fn plot(fb: &mut RgbImage, clip: Rect, x: i32, y: i32, color: Rgb<u8>) {
    if clip.contains(x, y) && Rect::of(fb).contains(x, y) {
        fb.put_pixel(x as u32, y as u32, color);
    }
}

/// A one-pixel outline just inside `rect`, cut to `clip`.
pub(crate) fn stroke(fb: &mut RgbImage, rect: Rect, clip: Rect, color: Rgb<u8>) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let edges = [
        Rect::new(rect.x, rect.y, rect.w, 1),
        Rect::new(rect.x, rect.bottom() - 1, rect.w, 1),
        Rect::new(rect.x, rect.y, 1, rect.h),
        Rect::new(rect.right() - 1, rect.y, 1, rect.h),
    ];
    for edge in edges {
        fill(fb, edge.intersect(clip), color);
    }
}

/// How source pixels combine with the framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blend {
    /// Copy every pixel.
    Opaque,
    /// Skip pixels within `tolerance` of `color` in every channel.
    Key { color: Rgb<u8>, tolerance: u8 },
    /// Mix in the source with weight `alpha` / 255.
    Alpha(u8),
    /// `icon × icon_gain + background × background_gain`, each gain in
    /// 1/256 units.
    Gains { icon: u8, background: u8 },
}

/// Draws `src_rect` of `src` with its top left corner at `(x, y)`.
pub(crate) fn blit(
    fb: &mut RgbImage,
    src: &RgbImage,
    src_rect: Rect,
    x: i32,
    y: i32,
    blend: Blend,
) {
    // Clip to the source first, then to the framebuffer.
    let Some((sxs, sys)) = src_rect.clip(src.width(), src.height()) else {
        return;
    };
    let dx = x - src_rect.x;
    let dy = y - src_rect.y;
    let dst = Rect::new(
        sxs.start as i32 + dx,
        sys.start as i32 + dy,
        sxs.len() as i32,
        sys.len() as i32,
    );
    let Some((xs, ys)) = dst.clip(fb.width(), fb.height()) else {
        return;
    };
    if blend == Blend::Opaque {
        // Whole rows at once: page backgrounds are full-screen copies.
        let (fb_w, src_w) = (fb.width() as usize, src.width() as usize);
        let len = xs.len() * 3;
        let sx = (xs.start as i32 - dx) as usize;
        for ty in ys {
            let sy = (ty as i32 - dy) as usize;
            let from = (sy * src_w + sx) * 3;
            let to = (ty as usize * fb_w + xs.start as usize) * 3;
            fb.as_mut()[to..to + len].copy_from_slice(&src.as_raw()[from..from + len]);
        }
        return;
    }
    for ty in ys {
        for tx in xs.clone() {
            let s = *src.get_pixel((tx as i32 - dx) as u32, (ty as i32 - dy) as u32);
            let d = fb.get_pixel_mut(tx, ty);
            match blend {
                Blend::Opaque => *d = s,
                Blend::Key { color, tolerance } => {
                    let near =
                        s.0.iter()
                            .zip(color.0)
                            .all(|(&c, k)| c.abs_diff(k) <= tolerance);
                    if !near {
                        *d = s;
                    }
                }
                Blend::Alpha(alpha) => {
                    let a = u16::from(alpha);
                    for (dc, sc) in d.0.iter_mut().zip(s.0) {
                        *dc = ((u16::from(sc) * a + u16::from(*dc) * (255 - a) + 127) / 255) as u8;
                    }
                }
                Blend::Gains { icon, background } => {
                    for (dc, sc) in d.0.iter_mut().zip(s.0) {
                        let mixed = u32::from(sc) * u32::from(icon)
                            + u32::from(*dc) * u32::from(background);
                        *dc = (mixed / 256).min(255) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_rgb565_to_full_range() {
        assert_eq!(rgb565(0xFFFF), Rgb([255, 255, 255]));
        assert_eq!(rgb565(0x0000), Rgb([0, 0, 0]));
        assert_eq!(rgb565(0xF800), Rgb([255, 0, 0]));
        assert_eq!(rgb565(0x07E0), Rgb([0, 255, 0]));
    }

    #[test]
    fn clips_fills_and_blits_at_the_edges() {
        let mut fb = RgbImage::new(4, 4);
        fill(&mut fb, Rect::new(-2, 2, 4, 10), Rgb([9, 9, 9]));
        assert_eq!(fb.get_pixel(1, 3), &Rgb([9, 9, 9]));
        assert_eq!(fb.get_pixel(2, 3), &Rgb([0, 0, 0]));

        let src = RgbImage::from_fn(3, 3, |x, y| Rgb([x as u8 * 10, y as u8 * 10, 100]));
        let mut fb = RgbImage::new(4, 4);
        blit(&mut fb, &src, Rect::new(1, 1, 5, 5), 2, 2, Blend::Opaque);
        assert_eq!(fb.get_pixel(2, 2), &Rgb([10, 10, 100]));
        assert_eq!(fb.get_pixel(3, 3), &Rgb([20, 20, 100]));
        assert_eq!(fb.get_pixel(1, 1), &Rgb([0, 0, 0]));
    }

    #[test]
    fn keys_out_pixels_near_the_key_colour() {
        let src = RgbImage::from_fn(3, 1, |x, _| match x {
            0 => Rgb([250, 250, 240]),
            1 => Rgb([200, 0, 0]),
            _ => Rgb([255, 255, 255]),
        });
        let mut fb = RgbImage::from_pixel(3, 1, Rgb([1, 2, 3]));
        let key = Blend::Key {
            color: Rgb([255, 255, 255]),
            tolerance: 30,
        };
        blit(&mut fb, &src, Rect::of(&src), 0, 0, key);
        assert_eq!(fb.get_pixel(0, 0), &Rgb([1, 2, 3]));
        assert_eq!(fb.get_pixel(1, 0), &Rgb([200, 0, 0]));
        assert_eq!(fb.get_pixel(2, 0), &Rgb([1, 2, 3]));
    }

    #[test]
    fn intersects_and_strokes_inside_a_clip() {
        let a = Rect::new(0, 0, 10, 10);
        assert_eq!(a.intersect(Rect::new(5, 5, 10, 10)), Rect::new(5, 5, 5, 5));
        assert_eq!(a.intersect(Rect::new(20, 0, 5, 5)).w, 0);

        let mut fb = RgbImage::new(6, 6);
        let red = Rgb([255, 0, 0]);
        stroke(&mut fb, Rect::new(0, 0, 6, 6), Rect::new(0, 0, 3, 6), red);
        assert_eq!(fb.get_pixel(2, 0), &red);
        assert_eq!(fb.get_pixel(5, 0), &Rgb([0, 0, 0]));
        assert_eq!(fb.get_pixel(2, 2), &Rgb([0, 0, 0]));
    }
}
