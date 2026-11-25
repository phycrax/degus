//! Drawing the page shown: background, press effect, display controls,
//! then the popup keyboard and the number being typed.

use std::sync::Arc;

use degus_set::controls::{
    Align, BarDirection, CursorColor, DataVar, Display, IconVar, LayerMode, ProgressBar,
};
use degus_set::icl::image::{Rgb, RgbImage};

use crate::draw::{Blend, Rect, blit, fill, rgb565, stroke};
use crate::font::Mark;
use crate::input::InputSession;
use crate::number::{field_chars, format_number, read_number};
use crate::sim::{Sim, bar_fraction};
use crate::text::draw_text;

/// Width of the data input cursor.
const CURSOR_W: i32 = 2;

/// How near a pixel must be to an icon's background colour, in every
/// channel, to be keyed out in transparency mode.
const KEY_TOLERANCE: u8 = 30;

impl Sim {
    /// The screen as the panel would show it, at full brightness.
    pub fn render(&self) -> RgbImage {
        let screen = self.screen();
        let mut fb = RgbImage::new(screen.w.into(), screen.h.into());
        self.render_into(&mut fb);
        fb
    }

    /// Like [`render`](Sim::render), into `fb`, which is resized if needed.
    pub fn render_into(&self, fb: &mut RgbImage) {
        let screen = self.screen();
        let (w, h) = (u32::from(screen.w), u32::from(screen.h));
        if fb.dimensions() != (w, h) {
            *fb = RgbImage::new(w, h);
        }
        // Pages without a full-screen background leave the rest black.
        fill(fb, Rect::of(fb), Rgb([0, 0, 0]));
        if let Some(background) = self.page_image(self.page) {
            blit(fb, &background, Rect::of(&background), 0, 0, Blend::Opaque);
        }

        // The press effect shows another page's pixels in the pressed area.
        if let Some((touch, shown)) = self.pressed()
            && let Some(pressed) = touch.pressed_page
            && let Some(image) = self.page_image(pressed)
        {
            let area = Rect::from_area(touch.area);
            blit(fb, &image, area, shown.x, shown.y, Blend::Opaque);
        }

        if let Some(page) = self.controls() {
            for display in &page.display {
                match display {
                    Display::Icon(icon) => self.draw_icon(fb, icon),
                    Display::Data(data) => self.draw_data(fb, data),
                    Display::Text(text) => draw_text(fb, &self.vp, text, self.set.font0.as_ref()),
                    Display::Progress(bar) => self.draw_progress(fb, bar),
                    _ => {}
                }
            }
        }

        if let Some(session) = &self.input {
            self.draw_input(fb, session);
        }
    }

    /// Page `page`'s background image.
    fn page_image(&self, page: u16) -> Option<Arc<RgbImage>> {
        let (lib, id) = self.background(page)?;
        self.icons.get(&self.set.icls, lib, id)
    }

    /// The icon a variable icon shows, if any, for values from `val_min` to
    /// `val_max` (§7.2).
    // ASSUME: value `v` shows icon `icon_min + (v - val_min)`, up to
    // `icon_max`, as DWIN's editor previews it. The guide says only that the
    // values map linearly onto the icons.
    fn icon_image(&self, icon: &IconVar) -> Option<Arc<RgbImage>> {
        let value = self.vp.get(icon.vp);
        if value < icon.val_min || value > icon.val_max {
            return None;
        }
        let id = u32::from(icon.icon_min) + u32::from(value - icon.val_min);
        if id > u32::from(icon.icon_max) {
            return None;
        }
        // Library 0 is the background ICL in use.
        let lib = match icon.icon_lib {
            0 => self.background_icl(),
            lib => lib,
        };
        self.icons.get(&self.set.icls, lib, id as u16)
    }

    fn draw_icon(&self, fb: &mut RgbImage, icon: &IconVar) {
        let Some(image) = self.icon_image(icon) else {
            return;
        };
        // ASSUME: transparency mode keys out the icon's background colour,
        // taken at pixel (1, 1), within 30 in every channel, as DWIN's
        // editor previews it; the filter strength is ignored. Overlay mode 2
        // mixes icon and page by their brightness bytes, and overlay mode 1
        // draws like overlay mode 0.
        let blend = match (icon.layer_mode, icon.show_background) {
            (LayerMode::OverlayMode2, _) => Blend::Gains {
                icon: icon.icon_brightness,
                background: icon.background_brightness,
            },
            (_, true) => Blend::Opaque,
            (_, false) => {
                let at = |n: u32| n.saturating_sub(1).min(1);
                Blend::Key {
                    color: *image.get_pixel(at(image.width()), at(image.height())),
                    tolerance: KEY_TOLERANCE,
                }
            }
        };
        let (x, y) = (i32::from(icon.position.x), i32::from(icon.position.y));
        blit(fb, &image, Rect::of(&image), x, y, blend);
    }

    fn draw_data(&self, fb: &mut RgbImage, data: &DataVar) {
        let number = format_number(
            read_number(&self.vp, data.vp, data.data_type),
            data.integer_digits,
            data.decimal_digits,
            data.leading_zeros,
        );
        let text = number + &data.unit;
        // Font 0 cells: `font_size` wide, twice as high.
        let w = i32::from(data.font_size);
        let field = field_chars(data.integer_digits, data.decimal_digits, &data.unit) as i32 * w;
        let width = text.chars().count() as i32 * w;
        // ASSUME: the number and its unit align in the field of the digits,
        // the point and the unit, from the position on, as DWIN's editor
        // previews it.
        let x = i32::from(data.position.x)
            + match data.align {
                Align::Left => 0,
                Align::Right => field - width,
                Align::Center => (field - width) / 2,
            };
        self.draw_str(
            fb,
            x,
            data.position.y.into(),
            data.font,
            data.font_size,
            &text,
            data.color,
        );
    }

    /// Draws `text` from font `font` in cells `width` wide, one after the
    /// other.
    #[allow(clippy::too_many_arguments)]
    fn draw_str(
        &self,
        fb: &mut RgbImage,
        x: i32,
        y: i32,
        font: u8,
        width: u8,
        text: &str,
        color: u16,
    ) {
        let h = 2 * u16::from(width);
        let clip = Rect::of(fb);
        for (i, c) in text.chars().enumerate() {
            let mark = Mark::new(self.set.font0.as_ref(), font, c.into(), width, h);
            let at = x + i as i32 * i32::from(width);
            mark.draw(fb, at, y, 0, clip, rgb565(color));
        }
    }

    fn draw_progress(&self, fb: &mut RgbImage, bar: &ProgressBar) {
        let area = Rect::from_area(bar.area);
        if bar.fill_background {
            fill(fb, area, rgb565(bar.background_color));
        }
        let fraction = bar_fraction(&self.vp, bar);
        let along = |len: i32| (f64::from(len) * fraction).round() as i32;
        let filled = match bar.direction {
            BarDirection::Right => Rect::new(area.x, area.y, along(area.w), area.h),
            BarDirection::Left => {
                let w = along(area.w);
                Rect::new(area.right() - w, area.y, w, area.h)
            }
            BarDirection::Down => Rect::new(area.x, area.y, area.w, along(area.h)),
            BarDirection::Up => {
                let h = along(area.h);
                Rect::new(area.x, area.bottom() - h, area.w, h)
            }
        };
        fill(fb, filled, rgb565(bar.bar_color));
        if bar.show_border {
            stroke(fb, area, area, rgb565(bar.border_color));
        }
    }

    fn draw_input(&self, fb: &mut RgbImage, session: &InputSession) {
        let control = &session.control;
        if let Some((keyboard, shown)) = self.keyboard_rects()
            && let degus_set::controls::Keyboard::Page { page, .. } = control.keyboard
            && let Some(image) = self.page_image(page)
        {
            let blend = match control.keyboard_transparency {
                0 => Blend::Opaque,
                t => Blend::Alpha(255 - t),
            };
            blit(fb, &image, keyboard, shown.x, shown.y, blend);
            // A pressed key shows its press effect where the keyboard is.
            if let Some((key, shown)) = self.pressed()
                && let Some(pressed) = key.pressed_page
                && let Some(image) = self.page_image(pressed)
            {
                blit(
                    fb,
                    &image,
                    Rect::from_area(key.area),
                    shown.x,
                    shown.y,
                    Blend::Opaque,
                );
            }
        }

        // The guide: the number is right-aligned, the position being the top
        // right of the cursor (§6.2).
        let (x, y) = (i32::from(control.position.x), i32::from(control.position.y));
        let shown = session.shown();
        let w = i32::from(control.font_size);
        let width = shown.chars().count() as i32 * w;
        let left = x - CURSOR_W - width;
        self.draw_str(
            fb,
            left,
            y,
            control.font,
            control.font_size,
            &shown,
            control.color,
        );
        let cursor = match control.cursor_color {
            CursorColor::Black => Rgb([0, 0, 0]),
            CursorColor::White => Rgb([255, 255, 255]),
        };
        fill(fb, Rect::new(x - CURSOR_W, y, CURSOR_W, 2 * w), cursor);
    }
}
