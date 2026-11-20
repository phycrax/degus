use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use degus_config::FILE_NAME as CONFIG_FILE_NAME;
use degus_controls::{
    DISPLAY_FILE_NAME, KeyTarget, MAX_DISPLAYS_PER_PAGE, MAX_PAGES, TOUCH_FILE_NAME,
};
use degus_icl::IclBuilder;
use degus_wae::WaeBuilder;

use crate::id::id_from_file_name;
use crate::model::{
    Button, ConfigSpec, Control, Data, Icon, Input, Key, PROJECT_FILE_NAME, Pos, Progress, Rect,
    Size, Text,
};
use crate::project::Project;
use crate::sources::{prepare_icl, prepare_wae};

/// Most icon IDs listed in one "missing icons" message.
const MISSING_SHOWN: usize = 8;
/// Longest unit string a data variable holds.
const UNIT_MAX: usize = 11;
/// File `load_22` loads unless `init_file` names another.
const INIT_FILE: u16 = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Builds, but probably not what was meant.
    Warning,
    /// Stops the build.
    Error,
}

/// A problem found by [`Project::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Where: a file, and the declaration in it, such as
    /// `controls/home.toml: [control.fan_on]`.
    pub location: String,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{severity}: {}: {}", self.location, self.message)
    }
}

/// Everything the check learns, kept for the build.
#[derive(Default)]
pub(crate) struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    /// Libraries that could be prepared, by ID.
    pub icls: Vec<(u16, IclBuilder)>,
    pub waes: Vec<(u16, WaeBuilder)>,
    /// The built `T5LCFG.CFG`, if `[config]` is set and valid.
    pub config: Option<Vec<u8>>,
}

impl Analysis {
    fn error(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.push(Severity::Error, location, message);
    }

    fn warning(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.push(Severity::Warning, location, message);
    }

    fn push(
        &mut self,
        severity: Severity,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity,
            location: location.into(),
            message: message.into(),
        });
    }
}

/// What controls are checked against.
struct Context<'a> {
    screen: Size,
    /// Icon IDs of each ICL, by ICL ID; libraries that failed to prepare
    /// are missing.
    icons: &'a BTreeMap<u16, BTreeSet<u16>>,
    /// IDs of every declared ICL.
    icl_ids: BTreeSet<u16>,
    /// Image IDs of the background ICL, if it is declared and prepared.
    backgrounds: Option<(u16, &'a BTreeSet<u16>)>,
    /// Page IDs by name.
    pages: BTreeMap<&'a str, u16>,
    /// Names of the pages with keyboard keys.
    key_pages: BTreeSet<&'a str>,
}

impl Project {
    /// Validates the project without building anything.
    pub fn check(&self) -> Vec<Diagnostic> {
        self.analyze().diagnostics
    }

    pub(crate) fn analyze(&self) -> Analysis {
        let mut a = Analysis::default();
        let settings = &self.settings;
        if settings.screen.w == 0 || settings.screen.h == 0 {
            a.error(PROJECT_FILE_NAME, "screen size must not be zero");
        }
        self.check_include(&mut a);

        // File numbers in use, so two libraries cannot share one.
        let mut numbers = BTreeMap::from([
            (13, TOUCH_FILE_NAME.to_owned()),
            (14, DISPLAY_FILE_NAME.to_owned()),
        ]);
        let mut claim = |a: &mut Analysis, id: u16, location: &str, name: String| {
            if let Some(other) = numbers.insert(id, name) {
                a.error(location, format!("ID {id} is already used by {other}"));
            }
        };

        let mut icons = BTreeMap::new();
        let mut icl_ids = BTreeSet::new();
        let mut wae_ids = BTreeSet::new();
        for file in &self.files {
            let path = file.path.display();
            for spec in &file.declarations.icl {
                let location = format!("{path}: [[icl]] id = {}", spec.id);
                claim(&mut a, spec.id, &location, format!("{}.icl", spec.id));
                icl_ids.insert(spec.id);
                match prepare_icl(self.root(), spec, settings.chip.into()) {
                    Ok(builder) => {
                        icons.insert(spec.id, builder.ids().collect());
                        a.icls.push((spec.id, builder));
                    }
                    Err(message) => a.error(&location, message),
                }
            }
            for spec in &file.declarations.wae {
                let location = format!("{path}: [[wae]] id = {}", spec.id);
                claim(&mut a, spec.id, &location, format!("{}.wae", spec.id));
                wae_ids.insert(spec.id);
                match prepare_wae(self.root(), spec) {
                    Ok(builder) => a.waes.push((spec.id, builder)),
                    Err(message) => a.error(&location, message),
                }
            }
        }
        let background_icl = settings
            .config
            .as_ref()
            .and_then(|spec| spec.background_icl);
        if let Some(id) = background_icl
            && !icl_ids.contains(&id)
        {
            a.error(
                format!("{PROJECT_FILE_NAME}: [config] background_icl"),
                format!("{id} is not a declared [[icl]]"),
            );
        }

        let generated: BTreeSet<_> = numbers.values().cloned().collect();
        for file in &self.files {
            let location = format!("{}: copy", file.path.display());
            for path in &file.declarations.copy {
                match path.file_name().map(|name| name.to_string_lossy()) {
                    _ if !self.resolve(path).is_file() => {
                        a.error(&location, format!("{} not found", path.display()));
                    }
                    Some(name) if generated.contains(name.as_ref()) => {
                        a.error(&location, format!("{name} is also generated by the build"));
                    }
                    Some(name) if settings.config.is_some() && is_config_file(&name) => {
                        a.error(
                            &location,
                            format!(
                                "{name} would sit beside the {CONFIG_FILE_NAME} built from \
                                 [config]"
                            ),
                        );
                    }
                    _ => {}
                }
            }
        }
        if let Some(spec) = &settings.config {
            self.check_config(&mut a, spec, &wae_ids);
        }

        let backgrounds = background_icl.and_then(|id| icons.get(&id).map(|ids| (id, ids)));
        let context = Context {
            screen: settings.screen,
            icl_ids,
            backgrounds,
            pages: self
                .pages()
                .map(|(_, name, page)| (name, page.id))
                .collect(),
            key_pages: self
                .pages()
                .filter(|(_, _, page)| {
                    page.controls
                        .iter()
                        .any(|name| matches!(self.control(name), Some((_, Control::Key(_)))))
                })
                .map(|(_, name, _)| name)
                .collect(),
            icons: &icons,
        };
        self.check_controls(&mut a, &context);
        self.check_pages(&mut a, &context);
        a
    }

    fn check_include(&self, a: &mut Analysis) {
        let location = format!("{PROJECT_FILE_NAME}: include");
        let mut seen = BTreeSet::from([Path::new(PROJECT_FILE_NAME)]);
        for path in &self.settings.include {
            if !seen.insert(path) {
                a.error(
                    &location,
                    format!("{} is already part of the project", path.display()),
                );
            }
        }
    }

    fn check_config(&self, a: &mut Analysis, spec: &ConfigSpec, wae_ids: &BTreeSet<u16>) {
        let location = format!("{PROJECT_FILE_NAME}: [config]");
        let config = match spec.to_config() {
            Ok(config) => config,
            Err(message) => return a.error(&location, message),
        };
        match config.build() {
            Ok(bytes) => a.config = Some(bytes),
            Err(error) => a.error(&location, error.to_string()),
        }

        if let Some(music) = spec.music_wae
            && !wae_ids.contains(&music)
        {
            a.warning(
                &location,
                format!(
                    "music_wae {music} is not a declared [[wae]]; \
                     the display plays the file {music} it holds"
                ),
            );
        }
        if config.load_22 {
            let init = config.init_file.map_or(INIT_FILE, u16::from);
            let built = self.copies().any(|path| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                id_from_file_name(&name) == Some(init)
            });
            if !built {
                a.warning(
                    &location,
                    format!(
                        "load_22 is on, but the build has no file {init}; \
                         the display loads the file {init} it holds"
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
            let screen = self.settings.screen;
            if (w, h) != (screen.w, screen.h) {
                a.warning(
                    &location,
                    format!(
                        "lcd is {}x{}, so pages at rotation {} are {w}x{h}, not the {}x{} screen",
                        lcd.h_d,
                        lcd.v_d,
                        config.rotation.degrees(),
                        screen.w,
                        screen.h
                    ),
                );
            }
        }
        if let Some(name) = &config.sd_folder {
            a.warning(
                &location,
                format!(
                    "sd_folder stores {name:?} in the display's flash: later SD downloads \
                     must use a folder named {name}"
                ),
            );
        }
    }

    /// Checks each control once, however many pages list it.
    fn check_controls(&self, a: &mut Analysis, context: &Context) {
        let listed: BTreeSet<&str> = self
            .pages()
            .flat_map(|(_, _, page)| page.controls.iter().map(String::as_str))
            .collect();
        let mut declared = BTreeMap::new();
        for file in &self.files {
            for (name, control) in &file.declarations.control {
                let location = format!("{}: [control.{name}]", file.path.display());
                if !declare(a, &location, &mut declared, name, &file.path) {
                    continue;
                }
                if !listed.contains(name.as_str()) {
                    a.warning(&location, "no page lists it");
                }
                match control {
                    Control::Button(button) => check_button(a, &location, button, context),
                    Control::Key(key) => check_key(a, &location, key, context),
                    Control::Input(input) => check_input(a, &location, input, context),
                    Control::Icon(icon) => check_icon(a, &location, icon, context),
                    Control::Data(data) => check_data(a, &location, data, context),
                    Control::Text(text) => check_text(a, &location, text, context),
                    Control::Progress(bar) => check_progress(a, &location, bar, context),
                }
            }
        }
    }

    /// Checks each page, and what depends on the page its controls are on.
    fn check_pages(&self, a: &mut Analysis, context: &Context) {
        let mut declared = BTreeMap::new();
        let mut ids = BTreeMap::new();
        for file in &self.files {
            for (page_name, page) in &file.declarations.page {
                let location = format!("{}: [page.{page_name}]", file.path.display());
                if !declare(a, &location, &mut declared, page_name, &file.path) {
                    continue;
                }
                let id = page.id;
                if id >= MAX_PAGES {
                    a.error(&location, format!("ID {id} is not below {MAX_PAGES}"));
                }
                if let Some(other) = ids.insert(id, page_name) {
                    a.error(&location, format!("ID {id} is also page {other}"));
                }
                if let Some((icl, images)) = context.backgrounds
                    && !images.contains(&id)
                {
                    a.warning(&location, format!("has no background image in {icl}.icl"));
                }

                let mut names = BTreeSet::new();
                let mut displays = 0;
                for name in &page.controls {
                    if !names.insert(name) {
                        a.warning(&location, format!("lists {name} twice"));
                    }
                    let Some((index, control)) = self.control(name) else {
                        a.error(&location, format!("lists {name}, which no file declares"));
                        continue;
                    };
                    if !control.is_touch() {
                        displays += 1;
                    }
                    // Without a keyboard page, the keys are on the input's
                    // own page.
                    if let Control::Input(input) = control
                        && input.keyboard.is_none()
                        && !context.key_pages.contains(page_name.as_str())
                    {
                        a.warning(
                            format!(
                                "{}: [control.{name}] (page {page_name})",
                                self.files[index].path.display()
                            ),
                            format!("keyboard page {page_name} has no key controls"),
                        );
                    }
                }
                if displays > MAX_DISPLAYS_PER_PAGE {
                    a.error(
                        &location,
                        format!(
                            "{displays} display controls; a page holds at most \
                             {MAX_DISPLAYS_PER_PAGE}"
                        ),
                    );
                }
            }
        }
    }
}

/// Records that `file` declares `name`, reporting a name that is not one
/// and one an earlier file declares. Returns whether this is the
/// declaration in use.
fn declare<'a>(
    a: &mut Analysis,
    location: &str,
    declared: &mut BTreeMap<&'a str, &'a Path>,
    name: &'a str,
    file: &'a Path,
) -> bool {
    if !is_name(name) {
        a.error(
            location,
            format!(
                "{name:?} is not a name: use lowercase letters, digits and _, starting with a \
                 letter"
            ),
        );
    }
    match declared.entry(name) {
        Entry::Occupied(first) => {
            a.error(
                location,
                format!("{name} is also declared in {}", first.get().display()),
            );
            false
        }
        Entry::Vacant(slot) => {
            slot.insert(file);
            true
        }
    }
}

/// Whether `name` is lowercase letters, digits and `_`, starting with a
/// letter, so code can use it as an identifier.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The display reads any `T5LCFG*.CFG` as its configuration.
pub(crate) fn is_config_file(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("T5LCFG") && name.ends_with(".CFG")
}

fn check_button(a: &mut Analysis, location: &str, button: &Button, context: &Context) {
    check_rect(a, location, "area", button.area, context.screen);
    if let KeyTarget::Bit(bit) = button.target
        && bit > 15
    {
        a.error(location, format!("target bit {bit} is not in 0-15"));
    }
    check_touch_pages(
        a,
        location,
        button.next_page.as_deref(),
        button.pressed_page.as_deref(),
        context,
    );
}

fn check_key(a: &mut Analysis, location: &str, key: &Key, context: &Context) {
    check_rect(a, location, "area", key.area, context.screen);
    if matches!(key.code.to_be_bytes()[0], 0xFE | 0xFD) {
        a.error(
            location,
            format!("code 0x{:04X} must not start with FE or FD", key.code),
        );
    }
    check_touch_pages(
        a,
        location,
        key.next_page.as_deref(),
        key.pressed_page.as_deref(),
        context,
    );
}

/// The keys of an input without a keyboard page are checked with its pages.
fn check_input(a: &mut Analysis, location: &str, input: &Input, context: &Context) {
    let screen = context.screen;
    check_rect(a, location, "area", input.area, screen);
    check_pos(a, location, input.position, screen);
    if let Some(limits) = input.limits
        && limits.min > limits.max
    {
        a.error(
            location,
            format!("limits min {} is above max {}", limits.min, limits.max),
        );
    }
    if let Some(keyboard) = &input.keyboard {
        check_rect(a, location, "keyboard area", keyboard.area, screen);
        let (x, y) = (keyboard.position.x, keyboard.position.y);
        let right = u32::from(x) + u32::from(keyboard.area.w);
        let bottom = u32::from(y) + u32::from(keyboard.area.h);
        if right > u32::from(screen.w) || bottom > u32::from(screen.h) {
            a.error(
                location,
                format!(
                    "keyboard drawn at ({x}, {y}) extends past the {}x{} screen",
                    screen.w, screen.h
                ),
            );
        }
        let page = keyboard.page.as_str();
        if !context.pages.contains_key(page) {
            a.error(
                location,
                format!("keyboard page {page} is not a declared page"),
            );
        } else if !context.key_pages.contains(page) {
            a.warning(
                location,
                format!("keyboard page {page} has no key controls"),
            );
        }
    }
    check_touch_pages(
        a,
        location,
        input.next_page.as_deref(),
        input.pressed_page.as_deref(),
        context,
    );
}

fn check_touch_pages(
    a: &mut Analysis,
    location: &str,
    next_page: Option<&str>,
    pressed_page: Option<&str>,
    context: &Context,
) {
    if let Some(page) = next_page
        && !context.pages.contains_key(page)
    {
        a.error(location, format!("next_page {page} is not a declared page"));
    }
    if let Some(page) = pressed_page {
        match context.pages.get(page) {
            None => a.error(
                location,
                format!("pressed_page {page} is not a declared page"),
            ),
            Some(id) => {
                if let Some((icl, images)) = context.backgrounds
                    && !images.contains(id)
                {
                    a.warning(
                        location,
                        format!("pressed_page {page} has no background image in {icl}.icl"),
                    );
                }
            }
        }
    }
}

fn check_icon(a: &mut Analysis, location: &str, icon: &Icon, context: &Context) {
    check_pos(a, location, icon.position, context.screen);
    if icon.val_min > icon.val_max {
        a.error(
            location,
            format!("val_min {} is above val_max {}", icon.val_min, icon.val_max),
        );
    }
    if icon.icon_min > icon.icon_max {
        a.error(
            location,
            format!(
                "icon_min {} is above icon_max {}",
                icon.icon_min, icon.icon_max
            ),
        );
        return;
    }
    if icon.val_min <= icon.val_max && icon.val_max - icon.val_min != icon.icon_max - icon.icon_min
    {
        a.warning(
            location,
            format!(
                "values {}..={} and icons {}..={} differ in length; the display shows icon_min + (value - val_min)",
                icon.val_min, icon.val_max, icon.icon_min, icon.icon_max
            ),
        );
    }
    let lib = u16::from(icon.icon_lib);
    let (name, ids) = if lib == 0 {
        // Library 0 is whichever background ICL is active, which system
        // variable 0xDE can switch; only `background_icl` is checked.
        let Some((file, ids)) = context.backgrounds else {
            return;
        };
        (format!("{file}.icl (the background ICL)"), ids)
    } else {
        if !context.icl_ids.contains(&lib) {
            a.error(
                location,
                format!("icon_lib {lib} is not a declared [[icl]]"),
            );
            return;
        }
        let Some(ids) = context.icons.get(&lib) else {
            return; // The library's own error is already reported.
        };
        (format!("{lib}.icl"), ids)
    };
    let missing: Vec<u16> = (icon.icon_min..=icon.icon_max)
        .filter(|id| !ids.contains(id))
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
        a.error(location, format!("{name} has no icon {list}"));
    }
}

fn check_data(a: &mut Analysis, location: &str, data: &Data, context: &Context) {
    check_pos(a, location, data.position, context.screen);
    if !data.unit.is_ascii() || data.unit.len() > UNIT_MAX {
        a.error(
            location,
            format!(
                "unit {:?} must be ASCII and at most {UNIT_MAX} bytes",
                data.unit
            ),
        );
    }
}

fn check_text(a: &mut Analysis, location: &str, text: &Text, context: &Context) {
    check_rect(a, location, "area", text.area, context.screen);
}

fn check_progress(a: &mut Analysis, location: &str, bar: &Progress, context: &Context) {
    check_rect(a, location, "area", bar.area, context.screen);
    if bar.val_min >= bar.val_max {
        a.error(
            location,
            format!(
                "val_min {} is not below val_max {}",
                bar.val_min, bar.val_max
            ),
        );
    }
}

fn check_pos(a: &mut Analysis, location: &str, pos: Pos, screen: Size) {
    if pos.x >= screen.w || pos.y >= screen.h {
        a.error(
            location,
            format!(
                "position ({}, {}) is outside the {}x{} screen",
                pos.x, pos.y, screen.w, screen.h
            ),
        );
    }
}

fn check_rect(a: &mut Analysis, location: &str, key: &str, rect: Rect, screen: Size) {
    if rect.w == 0 || rect.h == 0 {
        a.error(location, format!("{key} is empty"));
    }
    let (right, bottom) = rect.end();
    if right > u32::from(screen.w) || bottom > u32::from(screen.h) {
        a.error(
            location,
            format!(
                "{key} ({}, {}) {}x{} extends past the {}x{} screen",
                rect.x, rect.y, rect.w, rect.h, screen.w, screen.h
            ),
        );
    }
}
