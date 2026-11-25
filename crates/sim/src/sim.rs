//! The simulated display: the set it runs and the state it keeps.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::time::Duration;

use degus_codec::{Accumulator, Crc, MAX_CONTENT};
use degus_set::config::Config;
use degus_set::controls::{
    BarDataType, Display, Encoding, Keyboard, PageControls, ProgressBar, TouchAction,
};
use degus_set::{DwinSet, File, Size};

use crate::audio::{Samples, Sound};
use crate::icons::Icons;
use crate::input::InputSession;
use crate::touch::Press;
use crate::vp::{OutOfRange, VpSpace};

/// The RTC at power-on: 2000-01-01 00:00:00, in seconds since 1970.
const RTC_START: u64 = 946_684_800;

/// Something that happened in the display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The page shown changed.
    Page(u16),
    /// The display reset, through `0x0004`.
    Reset,
    /// Something to play.
    Sound(Sound),
    /// Worth telling the user: a control or a system variable the simulator
    /// does not run, a frame it dropped, a page without a background.
    Note(String),
}

/// What is reported once per set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Once {
    /// The page's controls were looked over.
    Page(u16),
    /// A system VP the simulator only stores.
    SystemVp(u16),
    /// The set has no font 0.
    Font0,
    /// Touch sounds in music mode play voice IDs.
    VoiceIds,
}

/// A simulated T5L display running a [`DwinSet`].
///
/// It does no I/O and keeps no clock of its own: bytes, touches and time go
/// in through [`feed`](Sim::feed), [`press`](Sim::press) and
/// [`advance`](Sim::advance), and bytes, [`Event`]s and pixels come out
/// through [`take_tx`](Sim::take_tx), [`take_events`](Sim::take_events)
/// and [`render`](Sim::render).
pub struct Sim {
    pub(crate) set: DwinSet,
    /// The set's configuration, or the defaults without one.
    pub(crate) config: Config,
    pub(crate) icons: Icons,
    pub(crate) clips: HashMap<(u8, u16), Samples>,
    pub(crate) vp: VpSpace,
    pub(crate) page: u16,
    /// Backlight, 0-100 %.
    pub(crate) brightness: u8,
    pub(crate) auto_upload: bool,
    pub(crate) touch_sound: bool,
    pub(crate) clock: Duration,
    /// The RTC at clock 0, in seconds since 1970.
    pub(crate) rtc_start: u64,
    pub(crate) press: Option<Press>,
    pub(crate) input: Option<InputSession>,
    pub(crate) rx: Accumulator<[u8; MAX_CONTENT]>,
    /// Bytes dropped while looking for a frame, not reported yet.
    pub(crate) skipped: usize,
    pub(crate) tx: Vec<u8>,
    pub(crate) events: Vec<Event>,
    pub(crate) generation: u64,
    pub(crate) once: BTreeSet<Once>,
    /// When the clip playing ends.
    pub(crate) music_until: Option<Duration>,
}

impl fmt::Debug for Sim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sim")
            .field("screen", &self.screen())
            .field("page", &self.page)
            .field("clock", &self.clock)
            .finish_non_exhaustive()
    }
}

impl Sim {
    /// The display at power-on, running `set`: on page 0, with the VPs at
    /// zero, or as file 22 holds them when the configuration loads it.
    pub fn new(set: DwinSet) -> Self {
        let config = set.config.clone().unwrap_or_default();
        let mut sim = Self {
            rx: Accumulator::new([0; MAX_CONTENT], crc(&config)),
            brightness: config.brightness.min(100),
            auto_upload: config.auto_upload,
            touch_sound: config.touch_sound,
            set,
            config,
            icons: Icons::default(),
            clips: HashMap::new(),
            vp: VpSpace::new(),
            page: 0,
            clock: Duration::ZERO,
            rtc_start: RTC_START,
            press: None,
            input: None,
            skipped: 0,
            tx: Vec::new(),
            events: Vec::new(),
            generation: 0,
            once: BTreeSet::new(),
            music_until: None,
        };
        sim.power_on();
        sim
    }

    /// Runs `set` instead, as after rebuilding it. The VPs, the page, the
    /// clock and the settings the host changed are kept. A touch or an
    /// input in progress ends, and what was reported once is reported again.
    pub fn reload(&mut self, set: DwinSet) {
        self.config = set.config.clone().unwrap_or_default();
        self.set = set;
        self.rx = Accumulator::new([0; MAX_CONTENT], crc(&self.config));
        self.skipped = 0;
        self.icons = Icons::default();
        self.clips.clear();
        self.once.clear();
        self.press = None;
        self.input = None;
        self.sync_readonly();
        self.enter_page();
        self.changed();
        self.refresh();
    }

    /// The set the display runs.
    pub fn set(&self) -> &DwinSet {
        &self.set
    }

    /// The configuration the display runs with: the set's, or the defaults
    /// when it has none.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The size pages are drawn at, and [`render`](Sim::render)'s size.
    pub fn screen(&self) -> Size {
        self.set.device.screen
    }

    pub fn vp(&self) -> &VpSpace {
        &self.vp
    }

    /// The page shown.
    pub fn page(&self) -> u16 {
        self.page
    }

    /// Backlight, 0-100 %. [`render`](Sim::render) draws at full
    /// brightness.
    pub fn brightness(&self) -> u8 {
        self.brightness
    }

    /// Time since power-on.
    pub fn clock(&self) -> Duration {
        self.clock
    }

    /// Changes whenever what [`render`](Sim::render) draws may have
    /// changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The ICL that page backgrounds come from: the configuration's, unless
    /// `0x00DE` switched it.
    pub fn background_icl(&self) -> u8 {
        self.switched_icl().unwrap_or(self.config.background_icl)
    }

    /// The text being typed into a data input, if any.
    pub fn input_text(&self) -> Option<&str> {
        self.input.as_ref().map(|session| session.text.as_str())
    }

    /// Sets the RTC (`0x0010`) to `seconds` since 1970, in UTC. At power-on
    /// it reads 2000-01-01 00:00:00.
    pub fn set_rtc(&mut self, seconds: u64) {
        self.rtc_start = seconds.saturating_sub(self.clock.as_secs());
    }

    /// Writes `words` from `vp` on as the host would, so that system
    /// variables take effect, but without answering.
    pub fn write_vp(&mut self, vp: u16, words: &[u16]) -> Result<(), OutOfRange> {
        self.store(vp, words)?;
        self.refresh();
        Ok(())
    }

    /// Shows `page`, as when picking it from a list.
    pub fn show_page(&mut self, page: u16) {
        self.set_page(page);
        self.refresh();
    }

    /// Lets `dt` pass: held buttons fire and clips end.
    pub fn advance(&mut self, dt: Duration) {
        self.clock += dt;
        self.check_hold();
        self.check_music();
        self.refresh();
    }

    /// What happened since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn crc(&self) -> Option<Crc> {
        crc(&self.config)
    }

    pub(crate) fn changed(&mut self) {
        self.generation += 1;
    }

    pub(crate) fn note(&mut self, text: impl Into<String>) {
        self.events.push(Event::Note(text.into()));
    }

    /// Notes `text` unless `what` was noted before.
    pub(crate) fn note_once(&mut self, what: Once, text: impl Into<String>) {
        if self.once.insert(what) {
            self.note(text);
        }
    }

    /// The controls of the page shown.
    pub(crate) fn controls(&self) -> Option<&PageControls> {
        self.set.controls.get(self.page)
    }

    /// Power-on, and `0x0004`'s reset: everything starts over from the set.
    pub(crate) fn power_on(&mut self) {
        self.vp.clear();
        self.page = 0;
        self.press = None;
        self.input = None;
        self.music_until = None;
        self.brightness = self.config.brightness.min(100);
        self.auto_upload = self.config.auto_upload;
        self.touch_sound = self.config.touch_sound;
        if self.config.load_22 {
            self.load_22();
        }
        self.init_system();
        self.enter_page();
        self.changed();
        self.refresh();
    }

    /// Loads the VPs from file 22, or the file the configuration names.
    fn load_22(&mut self) {
        let number = self.config.init_file.map_or(22, u16::from);
        let image = self
            .set
            .files
            .iter()
            .find(|(name, _)| File::new(name.as_str(), Vec::new()).number() == Some(number));
        match image {
            Some((_, bytes)) => self.vp.load(bytes),
            None => self.note(format!(
                "load_22 is on, but the set has no file {number}: the VPs start at 0"
            )),
        }
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

    /// Whether the background ICL has a background for `page`.
    pub(crate) fn has_background(&self, page: u16) -> bool {
        self.background(page).is_some()
    }

    /// The ICL and icon ID page `page`'s background comes from: the
    /// picture replacement ICL when the configuration names one that has
    /// it, else the background ICL.
    pub(crate) fn background(&self, page: u16) -> Option<(u8, u16)> {
        let has = |lib: u8| {
            self.set
                .icls
                .get(&lib)
                .is_some_and(|icl| icl.icons.contains_key(&page))
        };
        let replacement = self.config.picture_replace_icl.filter(|&lib| has(lib));
        let lib = replacement.unwrap_or_else(|| self.background_icl());
        has(lib).then_some((lib, page))
    }

    // ASSUME: an input in progress survives a page switch, so keys on a page
    // reached through the input's `next_page` still type into it.
    pub(crate) fn set_page(&mut self, page: u16) {
        if !self.has_background(page) {
            self.note(format!(
                "page {page} has no background in {}.icl",
                self.background_icl()
            ));
        }
        self.page = page;
        self.sync_readonly();
        self.events.push(Event::Page(page));
        self.enter_page();
        self.changed();
    }

    /// Reports, once per page, what of its controls the simulator does not
    /// run or draw.
    pub(crate) fn enter_page(&mut self) {
        let page = self.page;
        if !self.once.insert(Once::Page(page)) {
            return;
        }
        let Some(controls) = self.set.controls.get(page) else {
            return;
        };
        let mut notes = Vec::new();
        // Font IDs that controls use, besides font 0.
        let mut fonts = Vec::new();
        let mut uses_font0 = false;
        for (i, touch) in controls.touch.iter().enumerate() {
            let at = format!("page {page}, touch control {i}");
            match &touch.action {
                TouchAction::DataInput(input) => {
                    fonts.push((at, input.font));
                    if let Keyboard::Page { page: keys, .. } = input.keyboard
                        && !self.has_background(keys)
                    {
                        notes.push(format!(
                            "page {page}, touch control {i}: keyboard page {keys} has no \
                             background in {}.icl",
                            self.background_icl()
                        ));
                    }
                }
                TouchAction::Unknown(rest) => notes.push(format!(
                    "{at}: type {:#04X} is not simulated; only its page switch and press \
                     effect work",
                    rest.get(1).copied().unwrap_or(0)
                )),
                _ => {}
            }
        }
        for (i, display) in controls.display.iter().enumerate() {
            let at = format!("page {page}, display control {i}");
            match display {
                Display::Data(data) => fonts.push((at, data.font)),
                Display::Text(text) => {
                    if !matches!(text.encoding, Encoding::Ascii | Encoding::Unicode) {
                        fonts.push((at.clone(), text.ascii_font));
                    }
                    fonts.push((at, text.font));
                }
                Display::Unknown(record) => notes.push(format!(
                    "{at}: type {:#04X} is not simulated, so it draws nothing",
                    record[1]
                )),
                _ => {}
            }
        }
        for (at, font) in fonts {
            if font == 0 {
                uses_font0 = true;
            } else {
                notes.push(format!(
                    "{at}: font {font} is not simulated, so its characters draw as boxes"
                ));
            }
        }
        notes.dedup();
        for note in notes {
            self.note(note);
        }
        if uses_font0 && self.set.font0.is_none() {
            self.note_once(
                Once::Font0,
                "the set has no font 0, so font 0 text is not drawn; the display would draw \
                 it with the font 0 it holds",
            );
        }
    }

    /// Updates what follows from the state: the progress bars' rate VPs,
    /// and decoding problems.
    pub(crate) fn refresh(&mut self) {
        let rates: Vec<(u16, u16)> = self
            .controls()
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
        for problem in self.icons.take_problems() {
            self.note(problem);
        }
    }
}

fn crc(config: &Config) -> Option<Crc> {
    if config.crc { Crc::on() } else { Crc::off() }
}

/// How full a progress bar is, 0-1.
pub(crate) fn bar_fraction(vp: &VpSpace, bar: &ProgressBar) -> f64 {
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
