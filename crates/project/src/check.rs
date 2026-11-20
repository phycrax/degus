use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use degus_config::FILE_NAME as CONFIG_FILE_NAME;
use degus_controls::{
    Area, DISPLAY_FILE_NAME, Display, IconVar, KeyTarget, Keyboard, MAX_DISPLAYS_PER_PAGE,
    MAX_PAGES, Point, TOUCH_FILE_NAME, TouchAction,
};
use degus_icl::IclBuilder;
use degus_wae::WaeBuilder;

use crate::id::id_from_file_name;
use crate::layout::StructLayout;
use crate::model::{ConfigSpec, Control, PROJECT_FILE_NAME, Size, is_name};
use crate::project::{Project, SourceRef, group_control_location, group_items};
use crate::resolve::{Lookup, Resolved, Scope, resolve};
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
    /// The structs' layouts by name.
    layouts: BTreeMap<&'a str, StructLayout>,
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
        let layouts = self.layouts();
        let key_pages = self
            .pages()
            .filter(|(_, _, page)| {
                self.page_items(page, &layouts)
                    .iter()
                    .any(|item| matches!(item.control, Control::Key(_)))
            })
            .map(|(_, name, _)| name)
            .collect();
        let context = Context {
            screen: settings.screen,
            icl_ids,
            backgrounds,
            pages: self
                .pages()
                .map(|(_, name, page)| (name, page.id))
                .collect(),
            key_pages,
            layouts,
            icons: &icons,
        };
        self.check_structs(&mut a);
        self.check_styles(&mut a);
        self.check_controls(&mut a, &context);
        self.check_groups(&mut a, &context);
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

    /// Checks each struct's layout, and that no two structs share a VP.
    fn check_structs(&self, a: &mut Analysis) {
        let mut declared = BTreeMap::new();
        // The words of each element: start, end, its struct, its name, and
        // where the struct is declared.
        let mut ranges: Vec<(u32, u32, &str, String, String)> = Vec::new();
        for file in &self.files {
            for (name, spec) in &file.declarations.structs {
                let location = format!("{}: [struct.{name}]", file.path.display());
                if !declare(a, &location, &mut declared, name, &file.path) {
                    continue;
                }
                let (layout, problems) = spec.layout();
                for problem in problems {
                    a.error(&location, problem);
                }
                for index in 0..layout.count {
                    let range = layout.element_range(index);
                    if range.is_empty() {
                        continue;
                    }
                    let label = if layout.count > 1 {
                        format!("{name}[{index}]")
                    } else {
                        name.clone()
                    };
                    ranges.push((range.start, range.end, name, label, location.clone()));
                }
            }
        }
        ranges.sort();
        let mut reported = BTreeSet::new();
        let mut furthest: Option<usize> = None;
        for i in 0..ranges.len() {
            let (start, end, name, label, location) = &ranges[i];
            if let Some(j) = furthest {
                let (_, other_end, other_name, other_label, _) = &ranges[j];
                if start < other_end && name != other_name && reported.insert((*other_name, *name))
                {
                    a.error(
                        location,
                        format!("{label} overlaps {other_label} at VP 0x{start:04X}"),
                    );
                }
            }
            if furthest.is_none_or(|j| *end > ranges[j].1) {
                furthest = Some(i);
            }
        }
    }

    fn check_styles(&self, a: &mut Analysis) {
        let mut declared = BTreeMap::new();
        for file in &self.files {
            for name in file.declarations.style.keys() {
                let location = format!("{}: [style.{name}]", file.path.display());
                declare(a, &location, &mut declared, name, &file.path);
            }
        }
    }

    /// Checks each control once, however many pages list it.
    fn check_controls(&self, a: &mut Analysis, context: &Context) {
        let listed = self.listed();
        let page_id = |name: &str| context.pages.get(name).copied();
        let lookup = Lookup {
            layouts: &context.layouts,
            page_id: &page_id,
        };
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
                let resolution = resolve(control, Scope::default(), &lookup);
                for error in &resolution.errors {
                    a.error(&location, error);
                }
                check_values(a, &location, control, &resolution.control, context);
                for problem in &resolution.geometry {
                    a.error(&location, problem);
                }
                check_geometry(a, &location, &resolution.control, context.screen);
            }
        }
    }

    /// Checks each group: what its controls name once, and where they are in
    /// every copy.
    fn check_groups(&self, a: &mut Analysis, context: &Context) {
        let listed = self.listed();
        let page_id = |name: &str| context.pages.get(name).copied();
        let lookup = Lookup {
            layouts: &context.layouts,
            page_id: &page_id,
        };
        let mut declared = BTreeMap::new();
        for (index, file) in self.files.iter().enumerate() {
            for (name, group) in &file.declarations.group {
                let location = format!("{}: [group.{name}]", file.path.display());
                if !declare(a, &location, &mut declared, name, &file.path) {
                    continue;
                }
                if let Some((other, _)) = self.control(name) {
                    a.error(
                        &location,
                        format!(
                            "{name} is also a control, in {}; pages list both by name",
                            self.files[other].path.display()
                        ),
                    );
                }
                if !listed.contains(name.as_str()) {
                    a.warning(&location, "no page lists it");
                }
                if let Some(bound) = &group.bound
                    && !context.layouts.contains_key(bound.as_str())
                {
                    a.error(&location, format!("struct {bound} is not declared"));
                }
                if group.controls.is_empty() {
                    a.warning(&location, "has no controls");
                }
                for item in group_items(index, name, group, &context.layouts) {
                    let resolution = resolve(item.control, item.scope, &lookup);
                    let place = item.source.group.as_ref();
                    if place.is_none_or(|place| place.element.as_ref().is_none_or(|e| e.1 == 0)) {
                        let base = base_location(&item.source, &file.path);
                        for error in &resolution.errors {
                            a.error(&base, error);
                        }
                        check_values(a, &base, item.control, &resolution.control, context);
                    }
                    let location = item.source.location(&file.path);
                    for problem in &resolution.geometry {
                        a.error(&location, problem);
                    }
                    check_geometry(a, &location, &resolution.control, context.screen);
                }
            }
        }
    }

    /// The names pages list.
    fn listed(&self) -> BTreeSet<&str> {
        self.pages()
            .flat_map(|(_, _, page)| page.controls.iter().map(String::as_str))
            .collect()
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
                for name in &page.controls {
                    if !names.insert(name) {
                        a.warning(&location, format!("lists {name} twice"));
                    }
                    if self.control(name).is_none() && self.group(name).is_none() {
                        a.error(&location, format!("lists {name}, which no file declares"));
                    }
                }
                let items = self.page_items(page, &context.layouts);
                let displays = items.iter().filter(|item| !item.control.is_touch()).count();
                if displays > MAX_DISPLAYS_PER_PAGE {
                    a.error(
                        &location,
                        format!(
                            "{displays} display controls; a page holds at most \
                             {MAX_DISPLAYS_PER_PAGE}"
                        ),
                    );
                }
                // Without a keyboard page, the keys are on the input's own
                // page. A group's copies share the message.
                if !context.key_pages.contains(page_name.as_str()) {
                    let mut warned = BTreeSet::new();
                    for item in &items {
                        if let Control::Input(input) = item.control
                            && input.keyboard.is_none()
                        {
                            let path = &self.files[item.source.file].path;
                            let base = base_location(&item.source, path);
                            if warned.insert(base.clone()) {
                                a.warning(
                                    format!("{base} (page {page_name})"),
                                    format!("keyboard page {page_name} has no key controls"),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Where a control is declared, without the element of its group's copy.
fn base_location(source: &SourceRef, path: &Path) -> String {
    match &source.group {
        None => source.location(path),
        Some(place) => group_control_location(path, &source.name, place.control),
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

/// The display reads any `T5LCFG*.CFG` as its configuration.
pub(crate) fn is_config_file(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("T5LCFG") && name.ends_with(".CFG")
}

/// Checks what a control holds, apart from where it is.
fn check_values(
    a: &mut Analysis,
    location: &str,
    control: &Control,
    resolved: &Resolved,
    context: &Context,
) {
    match resolved {
        Resolved::Touch(touch) => {
            match &touch.action {
                TouchAction::KeyCode(code) => {
                    if let KeyTarget::Bit(bit) = code.target
                        && bit > 15
                    {
                        a.error(location, format!("target bit {bit} is not in 0-15"));
                    }
                }
                TouchAction::Basic(code) => {
                    if matches!(code.to_be_bytes()[0], 0xFE | 0xFD) {
                        a.error(
                            location,
                            format!("code 0x{code:04X} must not start with FE or FD"),
                        );
                    }
                }
                TouchAction::DataInput(input) => {
                    if let Some(limits) = &input.limits
                        && limits.start() > limits.end()
                    {
                        a.error(
                            location,
                            format!(
                                "limits min {} is above max {}",
                                limits.start(),
                                limits.end()
                            ),
                        );
                    }
                    if let Control::Input(model) = control
                        && let Some(keyboard) = &model.keyboard
                        && context.pages.contains_key(keyboard.page.as_str())
                        && !context.key_pages.contains(keyboard.page.as_str())
                    {
                        a.warning(
                            location,
                            format!("keyboard page {} has no key controls", keyboard.page),
                        );
                    }
                }
            }
            if let (Some(id), Some((icl, images))) = (touch.pressed_page, context.backgrounds)
                && !images.contains(&id)
            {
                let name = match control {
                    Control::Button(c) => c.pressed_page.as_deref(),
                    Control::Key(c) => c.pressed_page.as_deref(),
                    Control::Input(c) => c.pressed_page.as_deref(),
                    _ => None,
                };
                a.warning(
                    location,
                    format!(
                        "pressed_page {} has no background image in {icl}.icl",
                        name.unwrap_or_default()
                    ),
                );
            }
        }
        Resolved::Display(Display::Icon(icon)) => check_icon(a, location, icon, context),
        Resolved::Display(Display::Data(data)) => {
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
        Resolved::Display(Display::Text(_)) => {}
        Resolved::Display(Display::Progress(bar)) => {
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
    }
}

fn check_icon(a: &mut Analysis, location: &str, icon: &IconVar, context: &Context) {
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

/// Checks that a control is on the screen.
fn check_geometry(a: &mut Analysis, location: &str, resolved: &Resolved, screen: Size) {
    match resolved {
        Resolved::Touch(touch) => {
            check_area(a, location, "area", touch.area, screen);
            if let TouchAction::DataInput(input) = &touch.action {
                check_point(a, location, input.position, screen);
                if let Keyboard::Page { area, position, .. } = input.keyboard {
                    check_area(a, location, "keyboard area", area, screen);
                    let right = u32::from(position.x) + u32::from(area.right - area.left);
                    let bottom = u32::from(position.y) + u32::from(area.bottom - area.top);
                    if right > u32::from(screen.w) || bottom > u32::from(screen.h) {
                        a.error(
                            location,
                            format!(
                                "keyboard drawn at ({}, {}) extends past the {}x{} screen",
                                position.x, position.y, screen.w, screen.h
                            ),
                        );
                    }
                }
            }
        }
        Resolved::Display(Display::Icon(icon)) => check_point(a, location, icon.position, screen),
        Resolved::Display(Display::Data(data)) => check_point(a, location, data.position, screen),
        Resolved::Display(Display::Text(text)) => {
            check_area(a, location, "area", text.area, screen);
        }
        Resolved::Display(Display::Progress(bar)) => {
            check_area(a, location, "area", bar.area, screen);
        }
    }
}

fn check_point(a: &mut Analysis, location: &str, point: Point, screen: Size) {
    if point.x >= screen.w || point.y >= screen.h {
        a.error(
            location,
            format!(
                "position ({}, {}) is outside the {}x{} screen",
                point.x, point.y, screen.w, screen.h
            ),
        );
    }
}

fn check_area(a: &mut Analysis, location: &str, key: &str, area: Area, screen: Size) {
    let (w, h) = (area.right - area.left, area.bottom - area.top);
    if w == 0 || h == 0 {
        a.error(location, format!("{key} is empty"));
    }
    if area.right > screen.w || area.bottom > screen.h {
        a.error(
            location,
            format!(
                "{key} ({}, {}) {w}x{h} extends past the {}x{} screen",
                area.left, area.top, screen.w, screen.h
            ),
        );
    }
}
