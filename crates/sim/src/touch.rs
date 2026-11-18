//! Touch: hit-testing the current page's touch controls (or the popup
//! keyboard) and running what they do.

use std::time::Duration;

use degus_config::Audio;
use degus_controls::{KeyCode, KeyTarget, Keyboard, Touch, TouchAction};

use crate::draw::Rect;
use crate::sim::Sim;
use crate::system::TP_STATUS;

/// A held touch.
#[derive(Debug, Clone)]
pub(crate) struct Press {
    pub target: Option<Target>,
    pub x: u16,
    pub y: u16,
    pub since: Duration,
    /// A held button already fired.
    pub fired: bool,
}

/// The touch control a press landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// Touch control `i` of the current page.
    Page(usize),
    /// Touch control `i` of the popup keyboard's page.
    Keyboard(usize),
}

/// `0x0016` touch states.
const FIRST_PRESS: u8 = 0x01;
const LIFT: u8 = 0x02;
const PRESSING: u8 = 0x03;
/// Marks fresh touch data in `0x0016`.
const UPDATED: u8 = 0x5A;

impl Sim {
    /// A finger lands at `(x, y)` in panel pixels.
    pub fn touch_down(&mut self, x: u16, y: u16) {
        if self.press.is_some() {
            return self.touch_move(x, y);
        }
        self.touch_status(FIRST_PRESS, x, y);
        let target = self.hit(x, y);
        self.press = Some(Press {
            target,
            x,
            y,
            since: self.clock,
            fired: false,
        });
        if let Some(target) = target
            && let Some(touch) = self.target_touch(target).cloned()
        {
            self.touch_sound();
            match touch.action {
                // ASSUME: a return key code fires on press, or after its hold
                // time when it has one.
                TouchAction::KeyCode(key) if key.hold_time == 0 => {
                    self.fire_key(&key);
                    if let Some(press) = &mut self.press {
                        press.fired = true;
                    }
                }
                TouchAction::KeyCode(_) => {}
                TouchAction::Basic(code) => self.input_key(code),
                TouchAction::DataInput(input) => {
                    if matches!(target, Target::Page(_)) {
                        self.start_input(input);
                    }
                }
            }
        }
        self.changed();
        self.refresh();
    }

    /// The finger moves while down. The press stays on the control it
    /// started on.
    pub fn touch_move(&mut self, x: u16, y: u16) {
        let Some(press) = &mut self.press else {
            return;
        };
        press.x = x;
        press.y = y;
        self.touch_status(PRESSING, x, y);
    }

    /// The finger lifts. A touch control's page switch happens now.
    // ASSUME: it happens wherever the finger lifts.
    pub fn touch_up(&mut self) {
        let Some(press) = self.press.take() else {
            return;
        };
        self.touch_status(LIFT, press.x, press.y);
        // ASSUME: keys of a popup keyboard switch no page.
        if let Some(target @ Target::Page(_)) = press.target
            && let Some(next) = self.target_touch(target).and_then(|t| t.next_page)
        {
            self.set_page(next);
        }
        self.changed();
        self.refresh();
    }

    /// Fires a held return key code once its hold time has passed.
    pub(crate) fn check_hold(&mut self) {
        let Some(press) = &self.press else {
            return;
        };
        if press.fired {
            return;
        }
        let Some(target) = press.target else {
            return;
        };
        let since = press.since;
        let Some(TouchAction::KeyCode(key)) = self.target_touch(target).map(|t| t.action.clone())
        else {
            return;
        };
        if self.clock.saturating_sub(since) >= Duration::from_millis(100) * u32::from(key.hold_time)
        {
            self.fire_key(&key);
            if let Some(press) = &mut self.press {
                press.fired = true;
            }
        }
    }

    pub(crate) fn target_touch(&self, target: Target) -> Option<&Touch> {
        match target {
            Target::Page(i) => self.world.controls.get(self.page)?.touch.get(i),
            Target::Keyboard(i) => {
                let Keyboard::Page { page, .. } = self.input.as_ref()?.control.keyboard else {
                    return None;
                };
                self.world.controls.get(page)?.touch.get(i)
            }
        }
    }

    /// The touch control at `(x, y)`. While a popup keyboard is up, only
    /// its keys respond.
    // ASSUME: the popup keyboard is modal, and where touch areas overlap the
    // one listed last (drawn on top) wins.
    fn hit(&self, x: u16, y: u16) -> Option<Target> {
        let (x, y) = (i32::from(x), i32::from(y));
        if let Some(session) = &self.input
            && let Keyboard::Page {
                page,
                area,
                position,
            } = session.control.keyboard
        {
            let keyboard = Rect::from_area(area);
            let shown = Rect::new(position.x.into(), position.y.into(), keyboard.w, keyboard.h);
            if !shown.contains(x, y) {
                return None;
            }
            let (kx, ky) = (x - shown.x + keyboard.x, y - shown.y + keyboard.y);
            return self
                .world
                .controls
                .get(page)?
                .touch
                .iter()
                .enumerate()
                .rev()
                .find(|(_, t)| {
                    matches!(t.action, TouchAction::Basic(_))
                        && Rect::from_area(t.area).contains(kx, ky)
                })
                .map(|(i, _)| Target::Keyboard(i));
        }
        self.world
            .controls
            .get(self.page)?
            .touch
            .iter()
            .enumerate()
            .rev()
            .find(|(_, t)| Rect::from_area(t.area).contains(x, y))
            .map(|(i, _)| Target::Page(i))
    }

    /// Writes a return key code's value and uploads the VP.
    fn fire_key(&mut self, key: &KeyCode) {
        let old = self.vp.get(key.vp);
        let new = match key.target {
            KeyTarget::Word => key.value,
            // The low byte of the value goes into the chosen byte.
            KeyTarget::HighByte => (key.value & 0xFF) << 8 | old & 0x00FF,
            KeyTarget::LowByte => old & 0xFF00 | key.value & 0xFF,
            KeyTarget::Bit(bit) if bit < 16 => {
                if key.value & 1 == 1 {
                    old | 1 << bit
                } else {
                    old & !(1 << bit)
                }
            }
            KeyTarget::Bit(_) => old,
        };
        let _ = self.store(key.vp, &[new]);
        if key.upload && self.auto_upload {
            self.upload(key.vp, 1);
        }
    }

    /// The sound of pressing a touch control, if touch sound is on: the
    /// buzzer for its configured time. In music mode a touch plays its
    /// control's voice ID, which degus does not build, so nothing.
    // ASSUME: only touches on a touch control sound, not on bare screen.
    fn touch_sound(&mut self) {
        if !self.touch_sound {
            return;
        }
        match self.world.config.audio {
            Audio::Buzzer => {
                let time = self.world.config.buzzer.unwrap_or_default().time;
                self.beep(Duration::from_millis(10) * u32::from(time));
            }
            Audio::Music => {
                if !self.voice_noted {
                    self.voice_noted = true;
                    self.note(
                        "touch sound in music mode plays voice IDs, which degus does not build yet",
                    );
                }
            }
        }
    }

    /// Updates `0x0016`-`0x0019`: fresh-data mark and state, X, Y.
    fn touch_status(&mut self, state: u8, x: u16, y: u16) {
        let _ = self
            .vp
            .write(TP_STATUS, &[u16::from_be_bytes([UPDATED, state]), x, y, 0]);
    }
}
