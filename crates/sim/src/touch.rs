//! Touch: hit-testing the touch controls of the page shown (or the popup
//! keyboard's) and running what they do.

use std::time::Duration;

use degus_set::config::Audio;
use degus_set::controls::{KeyCode, KeyTarget, Keyboard, Point, Touch, TouchAction};

use crate::draw::Rect;
use crate::sim::{Once, Sim};
use crate::system::TP_STATUS;

/// A held touch.
#[derive(Debug, Clone)]
pub(crate) struct Press {
    pub target: Option<Target>,
    pub at: Point,
    pub since: Duration,
    /// A held key code already fired.
    pub fired: bool,
}

/// The touch control a press landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// Touch control `i` of the page shown.
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
    /// A finger lands at `at`, in page pixels. Moves the touch when one is
    /// down already.
    pub fn press(&mut self, at: Point) {
        if self.press.is_some() {
            return self.move_to(at);
        }
        self.touch_status(FIRST_PRESS, at);
        let target = self.hit(at);
        self.press = Some(Press {
            target,
            at,
            since: self.clock,
            fired: false,
        });
        if let Some(target) = target
            && let Some(touch) = self.target_touch(target).cloned()
        {
            self.touch_sound();
            match touch.action {
                // ASSUME: a return key code fires on press, as DWIN's editor
                // previews it, or after its hold time when it has one.
                TouchAction::KeyCode(key) if key.hold_time == 0 => {
                    self.fire_key(&key);
                    if let Some(press) = &mut self.press {
                        press.fired = true;
                    }
                }
                TouchAction::Basic(code) => self.input_key(code),
                TouchAction::DataInput(input) => {
                    if matches!(target, Target::Page(_)) {
                        self.start_input(input);
                    }
                }
                _ => {}
            }
        }
        self.changed();
        self.refresh();
    }

    /// The finger moves while down. The press stays on the control it
    /// started on.
    pub fn move_to(&mut self, at: Point) {
        let Some(press) = &mut self.press else {
            return;
        };
        press.at = at;
        self.touch_status(PRESSING, at);
    }

    /// The finger lifts. A touch control's page switch happens now.
    // ASSUME: it happens wherever the finger lifts.
    pub fn release(&mut self) {
        let Some(press) = self.press.take() else {
            return;
        };
        self.touch_status(LIFT, press.at);
        // ASSUME: keys of a popup keyboard switch no page.
        if let Some(target @ Target::Page(_)) = press.target
            && let Some(next) = self.target_touch(target).and_then(|t| t.next_page)
        {
            self.set_page(next);
        }
        self.changed();
        self.refresh();
    }

    /// Whether a finger is down.
    pub fn is_pressed(&self) -> bool {
        self.press.is_some()
    }

    /// The control a press is on, and the area it shows its press effect
    /// in, when it has one: on the screen, so moved for a popup keyboard.
    pub(crate) fn pressed(&self) -> Option<(&Touch, Rect)> {
        let target = self.press.as_ref()?.target?;
        let touch = self.target_touch(target)?;
        let area = Rect::from_area(touch.area);
        let shown = match target {
            Target::Page(_) => area,
            Target::Keyboard(_) => {
                let (keyboard, shown) = self.keyboard_rects()?;
                Rect::new(
                    area.x - keyboard.x + shown.x,
                    area.y - keyboard.y + shown.y,
                    area.w,
                    area.h,
                )
            }
        };
        Some((touch, shown))
    }

    /// Fires a held return key code once its hold time has passed.
    pub(crate) fn check_hold(&mut self) {
        let Some(press) = &self.press else {
            return;
        };
        if press.fired {
            return;
        }
        let (Some(target), since) = (press.target, press.since) else {
            return;
        };
        let Some(TouchAction::KeyCode(key)) = self.target_touch(target).map(|t| t.action.clone())
        else {
            return;
        };
        let hold = Duration::from_millis(100) * u32::from(key.hold_time);
        if self.clock.saturating_sub(since) >= hold {
            self.fire_key(&key);
            if let Some(press) = &mut self.press {
                press.fired = true;
            }
        }
    }

    pub(crate) fn target_touch(&self, target: Target) -> Option<&Touch> {
        match target {
            Target::Page(i) => self.controls()?.touch.get(i),
            Target::Keyboard(i) => {
                let Keyboard::Page { page, .. } = self.input.as_ref()?.control.keyboard else {
                    return None;
                };
                self.set.controls.get(page)?.touch.get(i)
            }
        }
    }

    /// The popup keyboard's area on its page, and where it is shown.
    pub(crate) fn keyboard_rects(&self) -> Option<(Rect, Rect)> {
        let Keyboard::Page { area, position, .. } = self.input.as_ref()?.control.keyboard else {
            return None;
        };
        let keyboard = Rect::from_area(area);
        let shown = Rect::new(position.x.into(), position.y.into(), keyboard.w, keyboard.h);
        Some((keyboard, shown))
    }

    /// The touch control at `at`. While a popup keyboard is up, only its
    /// keys respond.
    // ASSUME: the popup keyboard is modal, and where touch areas overlap the
    // one listed first wins, as DWIN's editor previews it.
    fn hit(&self, at: Point) -> Option<Target> {
        let (x, y) = (i32::from(at.x), i32::from(at.y));
        if let Some((keyboard, shown)) = self.keyboard_rects() {
            let Keyboard::Page { page, .. } = self.input.as_ref()?.control.keyboard else {
                return None;
            };
            if !shown.contains(x, y) {
                return None;
            }
            let (kx, ky) = (x - shown.x + keyboard.x, y - shown.y + keyboard.y);
            return self
                .set
                .controls
                .get(page)?
                .touch
                .iter()
                .position(|t| {
                    matches!(t.action, TouchAction::Basic(_))
                        && Rect::from_area(t.area).contains(kx, ky)
                })
                .map(Target::Keyboard);
        }
        self.controls()?
            .touch
            .iter()
            .position(|t| Rect::from_area(t.area).contains(x, y))
            .map(Target::Page)
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
    /// control's voice ID, which the controls do not hold yet, so nothing.
    // ASSUME: only touches on a touch control sound, not on bare screen.
    fn touch_sound(&mut self) {
        if !self.touch_sound {
            return;
        }
        match self.config.audio {
            Audio::Buzzer => {
                let time = self.config.buzzer.unwrap_or_default().time;
                self.beep(Duration::from_millis(10) * u32::from(time));
            }
            Audio::Music => self.note_once(
                Once::VoiceIds,
                "touch sounds in music mode play voice IDs, which are not simulated yet",
            ),
        }
    }

    /// Updates `0x0016`-`0x0019`: fresh-data mark and state, X, Y.
    fn touch_status(&mut self, state: u8, at: Point) {
        let words = [u16::from_be_bytes([UPDATED, state]), at.x, at.y, 0];
        let _ = self.vp.write(TP_STATUS, &words);
    }
}
