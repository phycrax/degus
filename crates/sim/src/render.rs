//! Drawing the current page from the state: background, press effect,
//! display controls, then the popup keyboard and the number being typed.

use degus_controls::{
    Align, BarDirection, CursorColor, DataVar, Display, IconVar, Keyboard, LayerMode, ProgressBar,
};
use image::{Rgb, RgbImage};

use crate::draw::{Blend, Rect, blit, fill, rgb565, stroke};
use crate::font::draw_str;
use crate::format::{field_chars, format_number, read_number};
use crate::input::InputSession;
use crate::sim::{Sim, bar_fraction};
use crate::text::draw_text;
use crate::touch::Target;

/// Width of the data input cursor.
const CURSOR_W: i32 = 2;

impl Sim {
    /// The screen as the panel would show it, at full brightness.
    pub fn render(&self) -> RgbImage {
        let mut fb = RgbImage::new(self.world.screen.w.into(), self.world.screen.h.into());
        self.render_into(&mut fb);
        fb
    }

    /// Like [`Sim::render`], reusing `fb` (resized if needed).
    pub fn render_into(&self, fb: &mut RgbImage) {
        let (w, h) = (
            u32::from(self.world.screen.w),
            u32::from(self.world.screen.h),
        );
        if fb.dimensions() != (w, h) {
            *fb = RgbImage::new(w, h);
        }
        let icl = self.background_icl();
        match self.world.icons.icon(icl, self.page) {
            Some(background) => {
                // Pages without a full-screen image leave the rest black.
                fill(fb, Rect::new(0, 0, w as i32, h as i32), Rgb([0, 0, 0]));
                blit(fb, &background, full(&background), 0, 0, Blend::Opaque);
            }
            None => fill(fb, Rect::new(0, 0, w as i32, h as i32), Rgb([0, 0, 0])),
        }

        // The press effect shows another page's pixels in the pressed area.
        if let Some(press) = &self.press
            && let Some(target @ Target::Page(_)) = press.target
            && let Some(touch) = self.target_touch(target)
            && let Some(pressed) = touch.pressed_page
            && let Some(image) = self.world.icons.icon(icl, pressed)
        {
            let area = Rect::from_area(touch.area);
            blit(fb, &image, area, area.x, area.y, Blend::Opaque);
        }

        if let Some(page) = self.world.controls.get(self.page) {
            for display in &page.display {
                match display {
                    Display::Icon(icon) => self.draw_icon(fb, icon),
                    Display::Data(data) => self.draw_data(fb, data),
                    Display::Text(text) => draw_text(fb, &self.vp, text),
                    Display::Progress(bar) => self.draw_progress(fb, bar),
                }
            }
        }

        if let Some(session) = &self.input {
            self.draw_input(fb, session);
        }
    }

    /// The image a variable icon shows, if any.
    fn icon_image(&self, icon: &IconVar) -> Option<std::sync::Arc<RgbImage>> {
        let value = self.vp.get(icon.vp);
        // The guide: values outside the range show no icon.
        if value < icon.val_min || value > icon.val_max {
            return None;
        }
        // ASSUME: values map linearly onto the icons, which is
        // `icon_min + (value - val_min)` when both ranges are the same size.
        let span = i64::from(icon.val_max) - i64::from(icon.val_min);
        let offset = i64::from(value) - i64::from(icon.val_min);
        let icons = i64::from(icon.icon_max) - i64::from(icon.icon_min);
        let id = if span == 0 || span == icons {
            i64::from(icon.icon_min) + offset
        } else {
            i64::from(icon.icon_min) + offset * icons / span
        };
        let lib = match icon.icon_lib {
            0 => self.background_icl(),
            lib => lib.into(),
        };
        self.world.icons.icon(lib, u16::try_from(id).ok()?)
    }

    fn draw_icon(&self, fb: &mut RgbImage, icon: &IconVar) {
        let Some(image) = self.icon_image(icon) else {
            return;
        };
        // ASSUME: transparency mode keys out pixels darker than half the
        // filter strength in every channel, and overlay mode 2 mixes icon and
        // background by their brightness bytes.
        let blend = match (icon.layer_mode, icon.show_background) {
            (LayerMode::OverlayMode2, _) => Blend::Gains {
                icon: icon.icon_brightness,
                background: icon.background_brightness,
            },
            (_, true) => Blend::Opaque,
            (_, false) => Blend::KeyBlack {
                threshold: (icon.filter.min(0x3F) / 2).max(1),
            },
        };
        let (x, y) = (i32::from(icon.position.x), i32::from(icon.position.y));
        blit(fb, &image, full(&image), x, y, blend);
    }

    fn draw_data(&self, fb: &mut RgbImage, data: &DataVar) {
        let number = format_number(
            read_number(&self.vp, data.vp, data.data_type),
            data.integer_digits,
            data.decimal_digits,
            data.leading_zeros,
        );
        let (cw, ch) = data_cell(data.font_size);
        let field = field_chars(data.integer_digits, data.decimal_digits) as i32 * cw as i32;
        let width = number.chars().count() as i32 * cw as i32;
        // ASSUME: the number aligns in a field of integer + decimal digits
        // starting at the position, and the unit follows the number.
        let x = i32::from(data.position.x)
            + match data.align {
                Align::Left => 0,
                Align::Right => field - width,
                Align::Center => (field - width) / 2,
            };
        let text = number + &data.unit;
        draw_str(
            fb,
            x,
            data.position.y.into(),
            (cw, ch),
            0,
            &text,
            rgb565(data.color),
        );
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
                Rect::new(area.x + area.w - w, area.y, w, area.h)
            }
            BarDirection::Down => Rect::new(area.x, area.y, area.w, along(area.h)),
            BarDirection::Up => {
                let h = along(area.h);
                Rect::new(area.x, area.y + area.h - h, area.w, h)
            }
        };
        fill(fb, filled, rgb565(bar.bar_color));
        if bar.show_border {
            stroke(fb, area, rgb565(bar.border_color));
        }
    }

    fn draw_input(&self, fb: &mut RgbImage, session: &InputSession) {
        let control = &session.control;
        let icl = self.background_icl();
        if let Keyboard::Page {
            page,
            area,
            position,
        } = control.keyboard
            && let Some(image) = self.world.icons.icon(icl, page)
        {
            let keyboard = Rect::from_area(area);
            let (x, y) = (i32::from(position.x), i32::from(position.y));
            let blend = match control.keyboard_transparency {
                0 => Blend::Opaque,
                t => Blend::Alpha(255 - t),
            };
            blit(fb, &image, keyboard, x, y, blend);
            // A pressed key shows its press effect where the keyboard is.
            if let Some(press) = &self.press
                && let Some(target @ Target::Keyboard(_)) = press.target
                && let Some(key) = self.target_touch(target)
                && let Some(pressed) = key.pressed_page
                && let Some(image) = self.world.icons.icon(icl, pressed)
            {
                let key_area = Rect::from_area(key.area);
                let kx = x + key_area.x - keyboard.x;
                let ky = y + key_area.y - keyboard.y;
                blit(fb, &image, key_area, kx, ky, Blend::Opaque);
            }
        }

        // The guide: the number is right-aligned, (x, y) being the top right
        // of the cursor.
        let (cw, ch) = data_cell(control.font_size);
        let (x, y) = (i32::from(control.position.x), i32::from(control.position.y));
        let shown = session.shown();
        let width = shown.chars().count() as i32 * cw as i32;
        draw_str(
            fb,
            x - CURSOR_W - width,
            y,
            (cw, ch),
            0,
            &shown,
            rgb565(control.color),
        );
        let cursor = match control.cursor_color {
            CursorColor::Black => Rgb([0, 0, 0]),
            CursorColor::White => Rgb([255, 255, 255]),
        };
        fill(fb, Rect::new(x - CURSOR_W, y, CURSOR_W, ch as i32), cursor);
    }

    /// The box a display control draws in, for overlays.
    pub(crate) fn display_box(&self, display: &Display) -> Rect {
        match display {
            Display::Icon(icon) => {
                let (w, h) = self
                    .icon_image(icon)
                    .or_else(|| {
                        let lib = match icon.icon_lib {
                            0 => self.background_icl(),
                            lib => lib.into(),
                        };
                        self.world.icons.icon(lib, icon.icon_min)
                    })
                    .map_or((16, 16), |image| {
                        (image.width() as i32, image.height() as i32)
                    });
                Rect::new(icon.position.x.into(), icon.position.y.into(), w, h)
            }
            Display::Data(data) => {
                let (cw, ch) = data_cell(data.font_size);
                let chars = field_chars(data.integer_digits, data.decimal_digits) + data.unit.len();
                Rect::new(
                    data.position.x.into(),
                    data.position.y.into(),
                    chars as i32 * cw as i32,
                    ch as i32,
                )
            }
            Display::Text(text) => Rect::from_area(text.area),
            Display::Progress(bar) => Rect::from_area(bar.area),
        }
    }
}

/// A data variable's character cell: `font_size` wide, twice as high, like
/// font 0.
fn data_cell(font_size: u8) -> (u32, u32) {
    let w = u32::from(font_size.max(1));
    (w, 2 * w)
}

fn full(image: &RgbImage) -> Rect {
    Rect::new(0, 0, image.width() as i32, image.height() as i32)
}
