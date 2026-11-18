//! The simulated display: what was loaded ([`World`]) and the runtime state
//! (VPs, page, touch, input), kept apart so a new [`World`] can replace
//! the old one without losing the state.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use degus_controls::{Display, ProgressBar};

use crate::draw::Rect;
use crate::host::{DeviceMsg, HostCmd};
use crate::input::InputSession;
use crate::touch::Press;
use crate::vp::{OutOfRange, VpSpace};
use crate::world::{LoadError, World};

/// Something that happened in the display, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The page shown changed.
    Page(u16),
    /// The display reset, through `0x0004`.
    Reset,
    /// Worth telling the user, such as a system VP that is only stored.
    Note(String),
}

/// A control's box on the current page, for overlays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayItem {
    pub rect: Rect,
    /// A touch control rather than a display control.
    pub touch: bool,
    /// Where the control comes from.
    pub label: String,
}

/// A simulated T5L display running a [`World`].
#[derive(Debug)]
pub struct Sim {
    pub(crate) world: World,
    pub(crate) vp: VpSpace,
    pub(crate) page: u16,
    /// Backlight, 0-100 %.
    pub(crate) brightness: u8,
    pub(crate) auto_upload: bool,
    pub(crate) touch_sound: bool,
    pub(crate) clock: Duration,
    pub(crate) press: Option<Press>,
    pub(crate) input: Option<InputSession>,
    pub(crate) outbox: Vec<DeviceMsg>,
    pub(crate) events: Vec<Event>,
    pub(crate) generation: u64,
    /// System VPs already reported as not simulated.
    pub(crate) noted: BTreeSet<u16>,
}

impl Sim {
    /// The display at power-on, on page 0.
    pub fn new(world: World) -> Self {
        let mut sim = Self {
            brightness: world.config.brightness.min(100),
            auto_upload: world.config.auto_upload,
            touch_sound: world.config.touch_sound,
            world,
            vp: VpSpace::new(),
            page: 0,
            clock: Duration::ZERO,
            press: None,
            input: None,
            outbox: Vec::new(),
            events: Vec::new(),
            generation: 0,
            noted: BTreeSet::new(),
        };
        sim.init_system();
        sim.refresh();
        sim
    }

    /// Loads and checks the project in the folder `dir`.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, LoadError> {
        Ok(Self::new(World::load(dir)?))
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn vp(&self) -> &VpSpace {
        &self.vp
    }

    /// The page shown.
    pub fn page(&self) -> u16 {
        self.page
    }

    /// Backlight, 0-100 %.
    pub fn brightness(&self) -> u8 {
        self.brightness
    }

    /// Time since power-on, as last given to [`Sim::tick`].
    pub fn clock(&self) -> Duration {
        self.clock
    }

    /// Changes whenever what [`Sim::render`] draws may have changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether a touch is held, so [`Sim::tick`] should run often.
    pub fn is_pressed(&self) -> bool {
        self.press.is_some()
    }

    /// The text being typed into a data input, if any.
    pub fn input_text(&self) -> Option<&str> {
        self.input.as_ref().map(|session| session.text.as_str())
    }

    /// Answers a host command; `None` when the display would not answer.
    pub fn handle(&mut self, cmd: &HostCmd) -> Option<DeviceMsg> {
        let reply = match cmd {
            HostCmd::WriteVp { vp, data } => match self.store_bytes(*vp, data) {
                Ok(()) => Some(DeviceMsg::Ack),
                Err(error) => {
                    self.note(format!("ignored {cmd}: {error}"));
                    None
                }
            },
            HostCmd::ReadVp { vp, words } => {
                self.before_read(*vp, usize::from(*words));
                match self.vp.read(*vp, usize::from(*words)) {
                    Some(data) => Some(DeviceMsg::ReadReply {
                        vp: *vp,
                        data: data.to_vec(),
                    }),
                    None => {
                        self.note(format!("ignored {cmd}: it runs past 0xFFFF"));
                        None
                    }
                }
            }
        };
        self.refresh();
        reply
    }

    /// Writes `words` from `vp` on as the host would, so that system
    /// variables take effect. Gives no reply.
    pub fn write_vp(&mut self, vp: u16, words: &[u16]) -> Result<(), OutOfRange> {
        self.store(vp, words)?;
        self.refresh();
        Ok(())
    }

    /// Shows `page` directly, as when picking it in the window.
    pub fn show_page(&mut self, page: u16) {
        self.set_page(page);
        self.refresh();
    }

    /// Advances the clock to `now`, firing held buttons.
    pub fn tick(&mut self, now: Duration) {
        self.clock = self.clock.max(now);
        self.check_hold();
        self.refresh();
    }

    /// Messages the display sends unasked, such as uploads, since the last
    /// call.
    pub fn take_outbox(&mut self) -> Vec<DeviceMsg> {
        std::mem::take(&mut self.outbox)
    }

    /// What happened since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// The ICL that pages and `icon_lib = 0` icons come from: the
    /// configuration's, unless `0x00DE` switched it.
    pub fn background_icl(&self) -> u16 {
        self.switched_icl()
            .unwrap_or_else(|| self.world.config.background_icl.into())
    }

    /// The boxes of the current page's controls, touch controls last.
    pub fn overlay(&self) -> Vec<OverlayItem> {
        let Some(page) = self.world.controls.get(self.page) else {
            return Vec::new();
        };
        let sources = self.world.sources.page(self.page);
        let label = |touch: bool, i: usize| {
            sources
                .and_then(|s| {
                    if touch {
                        s.touch.get(i)
                    } else {
                        s.display.get(i)
                    }
                })
                .map_or_else(
                    || {
                        format!(
                            "{} control {}",
                            if touch { "touch" } else { "display" },
                            i + 1
                        )
                    },
                    |source| self.world.describe(source),
                )
        };
        let mut items: Vec<_> = page
            .display
            .iter()
            .enumerate()
            .map(|(i, display)| OverlayItem {
                rect: self.display_box(display),
                touch: false,
                label: label(false, i),
            })
            .collect();
        items.extend(page.touch.iter().enumerate().map(|(i, touch)| OverlayItem {
            rect: Rect::from_area(touch.area),
            touch: true,
            label: label(true, i),
        }));
        items
    }

    pub(crate) fn changed(&mut self) {
        self.generation += 1;
    }

    pub(crate) fn note(&mut self, text: impl Into<String>) {
        self.events.push(Event::Note(text.into()));
    }

    /// Writes VPs and runs the system variables they touch.
    pub(crate) fn store(&mut self, vp: u16, words: &[u16]) -> Result<(), OutOfRange> {
        self.vp.write(vp, words)?;
        self.after_write(vp, words.len());
        self.changed();
        Ok(())
    }

    pub(crate) fn store_bytes(&mut self, vp: u16, bytes: &[u8]) -> Result<(), OutOfRange> {
        self.vp.write_bytes(vp, bytes)?;
        self.after_write(vp, bytes.len().div_ceil(2));
        self.changed();
        Ok(())
    }

    pub(crate) fn set_page(&mut self, page: u16) {
        if !self.world.icons.has_icon(self.background_icl(), page) {
            self.note(format!(
                "page {page} has no background in ICL {}",
                self.background_icl()
            ));
        }
        self.page = page;
        self.sync_readonly();
        self.events.push(Event::Page(page));
        self.changed();
    }

    /// Queues an upload of `words` words from `vp`.
    pub(crate) fn upload(&mut self, vp: u16, words: usize) {
        if let Some(data) = self.vp.read(vp, words) {
            self.outbox.push(DeviceMsg::Upload {
                vp,
                data: data.to_vec(),
            });
        }
    }

    /// Updates what follows from the state: the progress bars' rate VPs,
    /// and decoding problems.
    pub(crate) fn refresh(&mut self) {
        let rates: Vec<(u16, u16)> = self
            .world
            .controls
            .get(self.page)
            .into_iter()
            .flat_map(|page| &page.display)
            .filter_map(|display| match display {
                Display::Progress(bar) => bar
                    .rate_vp
                    .map(|vp| (vp, (bar_fraction(&self.vp, bar) * 100.0).round() as u16)),
                _ => None,
            })
            .collect();
        for (vp, percent) in rates {
            if self.vp.get(vp) != percent {
                self.vp.set(vp, percent);
                self.changed();
            }
        }
        for problem in self.world.icons.take_problems() {
            self.note(problem);
        }
    }
}

/// How full a progress bar is, 0-1.
pub(crate) fn bar_fraction(vp: &VpSpace, bar: &ProgressBar) -> f64 {
    use degus_controls::BarDataType;
    let value = match bar.data_type {
        BarDataType::I16 => f64::from(vp.get(bar.vp) as i16),
        BarDataType::HighByte => f64::from(vp.high(bar.vp)),
        BarDataType::LowByte => f64::from(vp.low(bar.vp)),
    };
    let (min, max) = (f64::from(bar.val_min), f64::from(bar.val_max));
    if max == min {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}
