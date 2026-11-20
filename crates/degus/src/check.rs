use std::collections::BTreeSet;
use std::fmt;

use degus_controls::{
    Area, DISPLAY_FILE_NAME, Display, IconVar, Keyboard, MAX_DISPLAYS_PER_PAGE, MAX_PAGES, Point,
    TOUCH_FILE_NAME, Touch, TouchAction, key,
};
use degus_icl::Icl;

use crate::bundle::{Bundle, LayoutProblem, icl_name, layout_problems, wae_name};
use crate::file::number;

/// The background ICL of a display whose configuration names no other.
const DEFAULT_BACKGROUND_ICL: u8 = 32;
/// The file `load_22` loads unless `init_file` names another.
const INIT_FILE: u16 = 22;
/// Most icon IDs listed in one "missing icons" message.
const MISSING_SHOWN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Writes, but probably not what was meant.
    Warning,
    /// Stops [`Bundle::write`].
    Error,
}

/// Where a problem is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    /// Touch control `index` of page `page`, counting from 0.
    Touch {
        page: u16,
        index: usize,
    },
    /// Display control `index` of page `page`, counting from 0.
    Display {
        page: u16,
        index: usize,
    },
    Page(u16),
    /// The ICL of this file number.
    Icl(u8),
    Icon {
        icl: u8,
        id: u16,
    },
    /// The WAE of this file number.
    Wae(u8),
    /// The configuration, `T5LCFG.CFG`.
    Config,
    /// A file of [`Bundle::files`], or one the bundle writes.
    File(String),
}

/// `page 0, touch control 2`, `32.icl, icon 5`, `T5LCFG.CFG`.
impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Location::Touch { page, index } => write!(f, "page {page}, touch control {index}"),
            Location::Display { page, index } => {
                write!(f, "page {page}, display control {index}")
            }
            Location::Page(page) => write!(f, "page {page}"),
            Location::Icl(file) => write!(f, "{}", icl_name(*file)),
            Location::Icon { icl, id } => write!(f, "{}, icon {id}", icl_name(*icl)),
            Location::Wae(file) => write!(f, "{}", wae_name(*file)),
            Location::Config => f.write_str(degus_config::FILE_NAME),
            Location::File(name) => f.write_str(name),
        }
    }
}

/// A problem [`Bundle::check`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub location: Location,
    pub message: String,
}

impl Diagnostic {
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// `error: page 0, touch control 2: area is empty`.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{severity}: {}: {}", self.location, self.message)
    }
}

impl Bundle {
    /// Finds what would keep the display from showing the bundle as meant:
    /// errors, which make [`Bundle::write`] refuse the bundle, and warnings.
    pub fn check(&self) -> Vec<Diagnostic> {
        let mut checker = Checker::new(self);
        let numbers = checker.files();
        checker.config(&numbers);
        checker.icons();
        checker.pages();
        checker.controls();
        checker.diagnostics
    }
}

struct Checker<'a> {
    bundle: &'a Bundle,
    diagnostics: Vec<Diagnostic>,
    /// The background ICL the display uses, when the bundle has it: image
    /// N is the background of page N.
    backgrounds: Option<(u8, &'a Icl)>,
    /// Pages with keyboard keys.
    key_pages: BTreeSet<u16>,
}

impl<'a> Checker<'a> {
    fn new(bundle: &'a Bundle) -> Self {
        let background = bundle
            .config
            .as_ref()
            .map_or(DEFAULT_BACKGROUND_ICL, |config| config.background_icl);
        let key_pages = bundle
            .controls
            .iter()
            .filter(|(_, page)| page.touch.iter().any(is_key))
            .map(|(id, _)| id)
            .collect();
        Self {
            bundle,
            diagnostics: Vec::new(),
            backgrounds: bundle.icls.get(&background).map(|icl| (background, icl)),
            key_pages,
        }
    }

    fn error(&mut self, location: Location, message: impl Into<String>) {
        self.push(Severity::Error, location, message.into());
    }

    fn warning(&mut self, location: Location, message: impl Into<String>) {
        self.push(Severity::Warning, location, message.into());
    }

    fn push(&mut self, severity: Severity, location: Location, message: String) {
        self.diagnostics.push(Diagnostic {
            severity,
            location,
            message,
        });
    }

    /// Encodes each part, and checks the folder they make: names and
    /// flash. Returns the file numbers in use.
    fn files(&mut self) -> BTreeSet<u16> {
        let bundle = self.bundle;
        // Each file's name and length.
        let mut files: Vec<(String, usize)> = Vec::new();
        if let Some(config) = &bundle.config {
            match config.to_bytes() {
                Ok(bytes) => files.push((degus_config::FILE_NAME.to_owned(), bytes.len())),
                Err(error) => self.error(Location::Config, error.to_string()),
            }
        }
        match bundle.controls.to_bytes() {
            Ok(built) => {
                files.push((TOUCH_FILE_NAME.to_owned(), built.touch.len()));
                files.push((DISPLAY_FILE_NAME.to_owned(), built.display.len()));
            }
            Err(error) => {
                let (location, message) = controls_problem(error);
                self.error(location, message);
            }
        }
        for (&file, icl) in &bundle.icls {
            match icl.to_bytes(file) {
                Ok(bytes) => files.push((icl_name(file), bytes.len())),
                Err(degus_icl::Error::Empty) => self.error(Location::Icl(file), "has no icons"),
                Err(error) => self.error(Location::Icl(file), error.to_string()),
            }
        }
        for (&file, wae) in &bundle.waes {
            match wae.to_bytes() {
                Ok(bytes) => files.push((wae_name(file), bytes.len())),
                Err(degus_wae::Error::Empty) => self.error(Location::Wae(file), "has no clips"),
                Err(error) => self.error(Location::Wae(file), error.to_string()),
            }
        }
        for (name, bytes) in &bundle.files {
            if bundle.config.is_some() && is_config_file(name) {
                self.error(
                    Location::File(name.clone()),
                    format!(
                        "the display would read it as well as the {} the bundle writes",
                        degus_config::FILE_NAME
                    ),
                );
            }
            files.push((name.clone(), bytes.len()));
        }

        let entries = files.iter().map(|(name, len)| (name.as_str(), *len));
        for (name, problem) in layout_problems(entries) {
            let message = match problem {
                LayoutProblem::Name(reason) => {
                    format!("is not a file name for DWIN_SET: {reason}")
                }
                LayoutProblem::SameName(first) => {
                    format!("is the same file as {first} to the display")
                }
                LayoutProblem::Overlap(first) => format!(
                    "overlaps {first} in flash: a file takes 256 KB per file number, so a \
                     large file runs into the next numbers"
                ),
            };
            self.error(Location::File(name), message);
        }
        files.iter().filter_map(|(name, _)| number(name)).collect()
    }

    /// Checks what the configuration refers to. `numbers` are the file
    /// numbers in the bundle.
    fn config(&mut self, numbers: &BTreeSet<u16>) {
        let bundle = self.bundle;
        let Some(config) = &bundle.config else {
            return;
        };
        let background = config.background_icl;
        if !bundle.icls.contains_key(&background) {
            self.error(
                Location::Config,
                format!("background_icl {background} is not in the bundle"),
            );
        }
        let music = config.music_wae;
        if !bundle.waes.is_empty() && !bundle.waes.contains_key(&music) {
            self.warning(
                Location::Config,
                format!(
                    "music_wae {music} is not in the bundle; the display plays music from \
                     the file {music} it holds"
                ),
            );
        }
        if let Some(icl) = config.picture_replace_icl
            && !bundle.icls.contains_key(&icl)
        {
            self.warning(
                Location::Config,
                format!(
                    "picture_replace_icl {icl} is not in the bundle; the display uses the \
                     file {icl} it holds"
                ),
            );
        }
        if config.load_22 {
            let init = config.init_file.map_or(INIT_FILE, u16::from);
            if !numbers.contains(&init) {
                self.warning(
                    Location::Config,
                    format!(
                        "load_22 is on, but the bundle has no file {init}; the display loads \
                         the file {init} it holds"
                    ),
                );
            }
        }
        if let Some(lcd) = &config.lcd {
            let (w, h) = if config.rotation.swaps_axes() {
                (lcd.v_d, lcd.h_d)
            } else {
                (lcd.h_d, lcd.v_d)
            };
            let screen = bundle.device.screen;
            if (w, h) != (screen.w, screen.h) {
                self.warning(
                    Location::Config,
                    format!(
                        "the LCD is {}x{}, so pages at {}° are {w}x{h}, not the {screen} of \
                         the device",
                        lcd.h_d,
                        lcd.v_d,
                        config.rotation.degrees(),
                    ),
                );
            }
        }
        if let Some(name) = &config.sd_folder {
            self.warning(
                Location::Config,
                format!(
                    "sd_folder stores {name:?} in the display's flash: later SD downloads \
                     must come from a folder named {name}, not {}",
                    crate::FOLDER
                ),
            );
        }
    }

    /// Checks each icon against the chip's limit.
    fn icons(&mut self) {
        let limit = self.bundle.device.chip.max_jpeg_size();
        for (&file, icl) in &self.bundle.icls {
            for (&id, icon) in &icl.icons {
                let size = icon.jpeg().len();
                if size > limit {
                    self.error(
                        Location::Icon { icl: file, id },
                        format!(
                            "is a {size}-byte JPEG, over the {limit} bytes the device's chip \
                             takes per icon"
                        ),
                    );
                }
            }
        }
    }

    /// Checks that each page with controls has a background of the screen's
    /// size.
    fn pages(&mut self) {
        let Some((file, icl)) = self.backgrounds else {
            return;
        };
        let screen = self.bundle.device.screen;
        for (id, _) in self.bundle.controls.iter() {
            match icl.icons.get(&id) {
                None => self.warning(
                    Location::Page(id),
                    format!("has no background image in {}", icl_name(file)),
                ),
                Some(icon) if (icon.width(), icon.height()) != (screen.w, screen.h) => {
                    self.warning(
                        Location::Icon { icl: file, id },
                        format!(
                            "is {}x{}, but the background of page {id} fills the {screen} \
                             screen",
                            icon.width(),
                            icon.height()
                        ),
                    );
                }
                Some(_) => {}
            }
        }
    }

    fn controls(&mut self) {
        for (page, controls) in self.bundle.controls.iter() {
            for (index, touch) in controls.touch.iter().enumerate() {
                self.touch(&Location::Touch { page, index }, page, touch);
            }
            for (index, display) in controls.display.iter().enumerate() {
                self.display(&Location::Display { page, index }, display);
            }
        }
    }

    fn touch(&mut self, at: &Location, page: u16, touch: &Touch) {
        self.area(at, "area", touch.area);
        if let Some(target) = touch.next_page {
            self.page_reference(at, "next_page", target);
        }
        if let Some(target) = touch.pressed_page {
            self.page_reference(at, "pressed_page", target);
        }
        let TouchAction::DataInput(input) = &touch.action else {
            return;
        };
        if let Some(limits) = &input.limits
            && limits.start() > limits.end()
        {
            self.error(
                at.clone(),
                format!(
                    "limits {} to {} are the wrong way round",
                    limits.start(),
                    limits.end()
                ),
            );
        }
        self.point(at, "position", input.position);
        match input.keyboard {
            Keyboard::OnPage => {
                if !self.key_pages.contains(&page) {
                    self.warning(
                        at.clone(),
                        "its keyboard is on its own page, which has no keyboard keys",
                    );
                }
            }
            Keyboard::Page {
                page: keyboard,
                area,
                position,
            } => {
                self.page_reference(at, "keyboard page", keyboard);
                if keyboard < MAX_PAGES && !self.key_pages.contains(&keyboard) {
                    self.warning(
                        at.clone(),
                        format!("keyboard page {keyboard} has no keyboard keys"),
                    );
                }
                self.area(at, "keyboard area", area);
                let screen = self.bundle.device.screen;
                let right = u32::from(position.x) + u32::from(area.right.saturating_sub(area.left));
                let bottom =
                    u32::from(position.y) + u32::from(area.bottom.saturating_sub(area.top));
                if right > u32::from(screen.w) || bottom > u32::from(screen.h) {
                    self.error(
                        at.clone(),
                        format!(
                            "the keyboard drawn at ({}, {}) extends past the {screen} screen",
                            position.x, position.y
                        ),
                    );
                }
            }
        }
    }

    /// Checks a page a touch control names as `key`.
    fn page_reference(&mut self, at: &Location, key: &str, target: u16) {
        if target >= MAX_PAGES {
            self.error(
                at.clone(),
                format!("{key} {target} is not below {MAX_PAGES}"),
            );
        } else if let Some((file, icl)) = self.backgrounds
            && !icl.icons.contains_key(&target)
        {
            self.warning(
                at.clone(),
                format!(
                    "{key} {target} has no background image in {}",
                    icl_name(file)
                ),
            );
        }
    }

    fn display(&mut self, at: &Location, display: &Display) {
        match display {
            Display::Icon(icon) => {
                self.icon(at, icon);
                self.point(at, "position", icon.position);
            }
            Display::Data(data) => self.point(at, "position", data.position),
            Display::Text(text) => self.area(at, "area", text.area),
            Display::Progress(bar) => {
                if bar.val_min >= bar.val_max {
                    self.error(
                        at.clone(),
                        format!(
                            "val_min {} is not below val_max {}",
                            bar.val_min, bar.val_max
                        ),
                    );
                }
                self.area(at, "area", bar.area);
            }
            // Records read from files that the types do not model.
            _ => {}
        }
    }

    fn icon(&mut self, at: &Location, icon: &IconVar) {
        if icon.val_min > icon.val_max {
            self.error(
                at.clone(),
                format!("val_min {} is above val_max {}", icon.val_min, icon.val_max),
            );
        }
        if icon.icon_min > icon.icon_max {
            self.error(
                at.clone(),
                format!(
                    "icon_min {} is above icon_max {}",
                    icon.icon_min, icon.icon_max
                ),
            );
            return;
        }
        if icon.val_min <= icon.val_max
            && icon.val_max - icon.val_min != icon.icon_max - icon.icon_min
        {
            self.warning(
                at.clone(),
                format!(
                    "values {}..={} and icons {}..={} differ in length; the display shows \
                     icon_min + (value - val_min)",
                    icon.val_min, icon.val_max, icon.icon_min, icon.icon_max
                ),
            );
        }
        let (name, icl) = if icon.icon_lib == 0 {
            // Library 0 is whichever background ICL is active, which system
            // variable 0xDE can switch; only the configured one is checked.
            let Some((file, icl)) = self.backgrounds else {
                return;
            };
            (format!("{} (the background ICL)", icl_name(file)), icl)
        } else {
            let Some(icl) = self.bundle.icls.get(&icon.icon_lib) else {
                self.error(
                    at.clone(),
                    format!("icon_lib {} is not in the bundle", icon.icon_lib),
                );
                return;
            };
            (icl_name(icon.icon_lib), icl)
        };
        let missing: Vec<u16> = (icon.icon_min..=icon.icon_max)
            .filter(|id| !icl.icons.contains_key(id))
            .collect();
        if !missing.is_empty() {
            let mut list = missing
                .iter()
                .take(MISSING_SHOWN)
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            if missing.len() > MISSING_SHOWN {
                list.push_str(&format!(" and {} more", missing.len() - MISSING_SHOWN));
            }
            self.error(at.clone(), format!("{name} has no icon {list}"));
        }
    }

    /// Checks that `area`, named `key`, holds pixels and is on the screen.
    fn area(&mut self, at: &Location, key: &str, area: Area) {
        let screen = self.bundle.device.screen;
        if area.right <= area.left || area.bottom <= area.top {
            self.error(at.clone(), format!("{key} is empty"));
        } else if area.right > screen.w || area.bottom > screen.h {
            self.error(
                at.clone(),
                format!(
                    "{key} ({}, {}) {}x{} extends past the {screen} screen",
                    area.left,
                    area.top,
                    area.right - area.left,
                    area.bottom - area.top
                ),
            );
        }
    }

    /// Checks that `point`, named `key`, is on the screen.
    fn point(&mut self, at: &Location, key: &str, point: Point) {
        let screen = self.bundle.device.screen;
        if point.x >= screen.w || point.y >= screen.h {
            self.error(
                at.clone(),
                format!(
                    "{key} ({}, {}) is outside the {screen} screen",
                    point.x, point.y
                ),
            );
        }
    }
}

/// Whether `touch` is a keyboard key: a basic touch that sends a key.
fn is_key(touch: &Touch) -> bool {
    matches!(touch.action, TouchAction::Basic(code) if code != key::NONE)
}

/// The display reads any `T5LCFG*.CFG` as its configuration.
fn is_config_file(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("T5LCFG") && name.ends_with(".CFG")
}

/// Where a control file error is, and what it is.
fn controls_problem(error: degus_controls::Error) -> (Location, String) {
    use degus_controls::Error as E;
    match error {
        E::BitIndex { page, index, bit } => (
            Location::Touch { page, index },
            format!("target bit {bit} is not in 0-15"),
        ),
        E::BasicKey { page, index, code } => (
            Location::Touch { page, index },
            format!("key code {code:#06X} must not start with FE or FD"),
        ),
        E::Unit { page, index, unit } => (
            Location::Display { page, index },
            format!("unit {unit:?} must be ASCII and at most 11 bytes"),
        ),
        E::PageId { page } => (
            Location::Page(page),
            format!("page IDs must be below {MAX_PAGES}"),
        ),
        E::TooManyDisplays { page, count } => (
            Location::Page(page),
            format!("has {count} display controls; a page holds at most {MAX_DISPLAYS_PER_PAGE}"),
        ),
        other => (
            Location::File(DISPLAY_FILE_NAME.to_owned()),
            other.to_string(),
        ),
    }
}
