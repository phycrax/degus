//! The screen: the display drawn, dimmed by its backlight, with the pointer
//! as a finger.

use degus_set::controls::Point;
use degus_sim::image::RgbImage;
use eframe::egui::{
    self, Color32, ColorImage, InnerResponse, Pos2, Rect, Response, Sense, TextureHandle,
    TextureOptions, pos2, vec2,
};

use crate::SimThread;
use crate::thread::TOUCH_LEASE;

/// How the screen is scaled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// To the space there is, keeping its shape.
    Fit,
    /// Points per pixel.
    Scale(f32),
}

/// The display's screen, as a widget. A press of the pointer on it is a
/// touch, which follows the pointer and ends when the pointer lifts or
/// leaves the screen, when the window loses focus, or soon after the screen
/// isn't shown anymore.
///
/// It never takes the keyboard focus, so keys go where they went before;
/// a touch takes the focus from text fields, so that they don't type the
/// keys held while touching.
#[derive(Default)]
pub struct Screen {
    fb: RgbImage,
    texture: Option<TextureHandle>,
    /// The generation the texture shows.
    shown: Option<u64>,
    touch: Touch,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Touch {
    #[default]
    Up,
    Down {
        id: u64,
        /// The window had focus during the touch.
        focused: bool,
    },
    /// Ended while the pointer is still down, which then touches nothing.
    Ended,
}

impl Screen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows the screen of `sim` at `zoom`. The inner response is the page
    /// pixel the pointer hovers, if any.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        sim: &SimThread,
        zoom: Zoom,
    ) -> InnerResponse<Option<Point>> {
        let brightness = self.update(ui, sim);
        let (w, h) = (self.fb.width() as f32, self.fb.height() as f32);
        let scale = match zoom {
            Zoom::Fit => {
                let available = ui.available_size();
                let fit = (available.x / w).min(available.y / h);
                if fit.is_finite() { fit.max(0.05) } else { 1.0 }
            }
            Zoom::Scale(scale) => scale,
        };
        // Clicks and drags, but no focus: Space and Enter don't click it.
        let (rect, response) =
            ui.allocate_exact_size(vec2(w, h) * scale, Sense::CLICK | Sense::DRAG);
        if let Some(texture) = &self.texture
            && ui.is_rect_visible(rect)
        {
            let painter = ui.painter_at(rect);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), rect, uv, Color32::WHITE);
            let dim = 255 - u32::from(brightness.min(100)) * 255 / 100;
            if dim > 0 {
                painter.rect_filled(rect, 0.0, Color32::from_black_alpha(dim as u8));
            }
        }

        let to_page = |pos: Pos2| -> Point {
            let p = (pos - rect.min) / scale;
            Point::new(
                p.x.clamp(0.0, w - 1.0) as u16,
                p.y.clamp(0.0, h - 1.0) as u16,
            )
        };
        self.follow(ui, &response, sim, to_page);
        let hover = response.hover_pos().map(to_page);
        InnerResponse::new(hover, response)
    }

    /// Renders the screen anew if it changed, with the simulator locked no
    /// longer than that, and uploads it. Returns the backlight.
    fn update(&mut self, ui: &egui::Ui, sim: &SimThread) -> u8 {
        let (brightness, rendered) = {
            let sim = sim.lock();
            let generation = sim.generation();
            let stale = self.shown != Some(generation) || self.texture.is_none();
            if stale {
                sim.render_into(&mut self.fb);
                self.shown = Some(generation);
            }
            (sim.brightness(), stale)
        };
        if rendered {
            let size = [self.fb.width() as usize, self.fb.height() as usize];
            let image = ColorImage::from_rgb(size, self.fb.as_raw());
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    let texture =
                        ui.ctx()
                            .load_texture("degus screen", image, TextureOptions::NEAREST);
                    self.texture = Some(texture);
                }
            }
        }
        brightness
    }

    /// Turns the pointer into a touch.
    fn follow(
        &mut self,
        ui: &egui::Ui,
        response: &Response,
        sim: &SimThread,
        to_page: impl Fn(Pos2) -> Point,
    ) {
        let (pos, latest, origin, down, released, focused) = ui.input(|i| {
            let pointer = &i.pointer;
            (
                pointer.interact_pos(),
                pointer.latest_pos(),
                pointer.press_origin(),
                pointer.any_down(),
                pointer.any_released(),
                i.focused,
            )
        });
        // Where the screen is, in the pointer's terms.
        let from_global = ui.ctx().layer_transform_from_global(ui.layer_id());
        let local = |pos: Pos2| from_global.map_or(pos, |t| t * pos);
        let on_screen = |pos: Pos2| response.interact_rect.contains(pos);
        let pos = pos.map(local);
        // `None` once the pointer left the window.
        let latest = latest.map(local);

        if self.touch == Touch::Up {
            if response.is_pointer_button_down_on()
                && let Some(start) = origin.map(local).or(pos)
                && on_screen(start)
            {
                let id = sim.start_touch(to_page(start));
                self.touch = Touch::Down { id, focused };
                ui.memory_mut(|memory| {
                    if let Some(field) = memory.focused() {
                        memory.surrender_focus(field);
                    }
                });
            } else if response.clicked_by(egui::PointerButton::Primary)
                && let Some(at) = pos.filter(|&p| on_screen(p))
            {
                // Pressed and released between two frames.
                let id = sim.start_touch(to_page(at));
                sim.end_touch(id);
            }
        }

        match self.touch {
            Touch::Up => {}
            Touch::Down { id, focused: had } => {
                let lost = had && !focused;
                let at = latest.filter(|&p| on_screen(p));
                match at {
                    Some(at) if down && !released && !lost && ui.is_enabled() => {
                        self.touch = Touch::Down {
                            id,
                            focused: had || focused,
                        };
                        if sim.move_touch(id, to_page(at)) {
                            // Keeps the touch from running out.
                            ui.ctx().request_repaint_after(TOUCH_LEASE / 5);
                        } else {
                            self.touch = Touch::Ended;
                        }
                    }
                    _ => {
                        sim.end_touch(id);
                        self.touch = if released || !down {
                            Touch::Up
                        } else {
                            Touch::Ended
                        };
                    }
                }
            }
            Touch::Ended => {
                if released || !down {
                    self.touch = Touch::Up;
                }
            }
        }
    }
}
