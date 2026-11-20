use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use degus_config::Config;
use degus_controls::{Controls, DISPLAY_FILE_NAME, TOUCH_FILE_NAME};
use degus_icl::{Chip, Icl};
use degus_wae::Wae;

use crate::check::Diagnostic;
use crate::error::{Error, Result, io_error};
use crate::file::{File, number};

/// The folder the display downloads from the root of its SD card.
pub const FOLDER: &str = "DWIN_SET";

/// Flash bytes per file number: the display stores file N at N × 256 KB.
pub const SLOT_SIZE: u32 = 256 * 1024;

/// A size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    pub w: u16,
    pub h: u16,
}

impl Size {
    pub const fn new(w: u16, h: u16) -> Self {
        Self { w, h }
    }
}

/// `800x480`.
impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.w, self.h)
    }
}

/// The display a bundle is made for, which its files do not record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Device {
    /// The size pages are drawn at: the panel's size, with width and
    /// height swapped when the display is rotated by 90° or 270°.
    pub screen: Size,
    /// Sets the largest icon an ICL may hold.
    pub chip: Chip,
}

impl Device {
    /// A display with a T5L2 chip.
    pub const fn new(screen: Size) -> Self {
        Self {
            screen,
            chip: Chip::T5L2,
        }
    }
}

/// Everything in a `DWIN_SET` folder, and the display it is for.
#[derive(Debug, Clone, PartialEq)]
pub struct Bundle {
    pub device: Device,
    /// `T5LCFG.CFG`. Without it, the display keeps the configuration it
    /// has.
    pub config: Option<Config>,
    /// Files 13 and 14: the touch and display controls of every page.
    pub controls: Controls,
    /// Icon libraries by file number, written as `<n>.icl`.
    pub icls: BTreeMap<u8, Icl>,
    /// Audio libraries by file number, written as `<n>.wae`.
    pub waes: BTreeMap<u8, Wae>,
    /// Any other files, such as fonts, by name. They are written as they
    /// are.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// What [`Bundle::write`] wrote.
#[derive(Debug, Clone)]
pub struct Written {
    /// The files, in the order [`Bundle::to_files`] gives.
    pub files: Vec<File>,
    /// The check's warnings.
    pub warnings: Vec<Diagnostic>,
}

impl Bundle {
    /// An empty bundle for `device`.
    pub fn new(device: Device) -> Self {
        Self {
            device,
            config: None,
            controls: Controls::new(),
            icls: BTreeMap::new(),
            waes: BTreeMap::new(),
            files: BTreeMap::new(),
        }
    }

    /// Reads the `DWIN_SET` folder in `dir`; see [`Bundle::from_files`].
    /// Folders inside it are skipped, as the display skips them.
    pub fn read(dir: impl AsRef<Path>, device: Device) -> Result<Self> {
        let folder = dir.as_ref().join(FOLDER);
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&folder).map_err(io_error(&folder))? {
            let entry = entry.map_err(io_error(&folder))?;
            let path = entry.path();
            if !entry.file_type().map_err(io_error(&path))?.is_file() {
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|name| Error::FileName {
                    name: name.to_string_lossy().into_owned(),
                    reason: "it is not UTF-8",
                })?;
            let bytes = std::fs::read(&path).map_err(io_error(&path))?;
            files.push(File::new(name, bytes));
        }
        Self::from_files(files, device)
    }

    /// The bundle that `files` make, as the display reads them:
    ///
    /// - `T5LCFG*.CFG` is the configuration;
    /// - files 13 and 14 ending in `.bin` are the controls;
    /// - files ending in `.icl` and `.wae` are the libraries of their file
    ///   numbers, which are written back as `<n>.icl` and `<n>.wae`;
    /// - the other files go in [`Bundle::files`].
    ///
    /// Names are matched ignoring case. Fails on two files with the same
    /// role, and on a file that does not read.
    pub fn from_files(files: impl IntoIterator<Item = File>, device: Device) -> Result<Self> {
        let mut files: Vec<(Role, File)> = files
            .into_iter()
            .map(|file| (Role::of(&file.name), file))
            .collect();
        files.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
        // The name of the file with each role.
        let mut roles: BTreeMap<Role, &str> = BTreeMap::new();
        for (role, file) in &files {
            if *role != Role::Other
                && let Some(first) = roles.insert(*role, &file.name)
            {
                return Err(Error::Duplicate {
                    first: first.to_owned(),
                    second: file.name.clone(),
                });
            }
        }

        let mut bundle = Bundle::new(device);
        let (mut touch, mut display) = (None, None);
        for (role, file) in files {
            match role {
                Role::Config => bundle.config = Some(Config::from_bytes(&file.bytes)?),
                Role::Touch => touch = Some(file.bytes),
                Role::Display => display = Some(file.bytes),
                Role::Icl(number) => {
                    let icl = Icl::from_bytes(&file.bytes).map_err(|source| Error::Icl {
                        file: number,
                        source,
                    })?;
                    bundle.icls.insert(number, icl);
                }
                Role::Wae(number) => {
                    let wae = Wae::from_bytes(&file.bytes).map_err(|source| Error::Wae {
                        file: number,
                        source,
                    })?;
                    bundle.waes.insert(number, wae);
                }
                Role::Other => {
                    bundle.files.insert(file.name, file.bytes);
                }
            }
        }
        if touch.is_some() || display.is_some() {
            let empty = Controls::new().to_bytes()?;
            let touch = touch.unwrap_or(empty.touch);
            let display = display.unwrap_or(empty.display);
            bundle.controls = Controls::from_bytes(&touch, &display)?;
        }
        Ok(bundle)
    }

    /// Encodes every file: the numbered ones by number, then the others by
    /// name.
    ///
    /// Fails on a part that cannot be encoded, a bad or repeated file name,
    /// and files that overlap in flash. Unlike [`Bundle::write`], it does
    /// not run [`Bundle::check`].
    pub fn to_files(&self) -> Result<Vec<File>> {
        let mut files = Vec::new();
        if let Some(config) = &self.config {
            files.push(File::new(degus_config::FILE_NAME, config.to_bytes()?));
        }
        let controls = self.controls.to_bytes()?;
        files.push(File::new(TOUCH_FILE_NAME, controls.touch));
        files.push(File::new(DISPLAY_FILE_NAME, controls.display));
        for (&file, icl) in &self.icls {
            let bytes = icl
                .to_bytes(file)
                .map_err(|source| Error::Icl { file, source })?;
            files.push(File::new(icl_name(file), bytes));
        }
        for (&file, wae) in &self.waes {
            let bytes = wae
                .to_bytes()
                .map_err(|source| Error::Wae { file, source })?;
            files.push(File::new(wae_name(file), bytes));
        }
        for (name, bytes) in &self.files {
            files.push(File::new(name.clone(), bytes.clone()));
        }
        let entries = files.iter().map(|file| (&*file.name, file.bytes.len()));
        if let Some((name, problem)) = layout_problems(entries).into_iter().next() {
            return Err(match problem {
                LayoutProblem::Name(reason) => Error::FileName { name, reason },
                LayoutProblem::SameName(first) => Error::Duplicate {
                    first,
                    second: name,
                },
                LayoutProblem::Overlap(first) => Error::FlashOverlap {
                    first,
                    second: name,
                },
            });
        }
        sort(&mut files);
        Ok(files)
    }

    /// Checks the bundle and, if [`Bundle::check`] finds no errors, writes
    /// its files to `dir/DWIN_SET`, replacing that folder.
    ///
    /// Nothing of an earlier write is kept: a file left in the folder would
    /// still reach the display.
    pub fn write(&self, dir: impl AsRef<Path>) -> Result<Written> {
        let diagnostics = self.check();
        if diagnostics.iter().any(Diagnostic::is_error) {
            return Err(Error::Check(diagnostics));
        }
        let files = self.to_files()?;
        let folder = dir.as_ref().join(FOLDER);
        if folder.exists() {
            std::fs::remove_dir_all(&folder).map_err(io_error(&folder))?;
        }
        std::fs::create_dir_all(&folder).map_err(io_error(&folder))?;
        for file in &files {
            let path = folder.join(&file.name);
            std::fs::write(&path, &file.bytes).map_err(io_error(&path))?;
        }
        Ok(Written {
            files,
            warnings: diagnostics,
        })
    }
}

/// What a file of the folder is to the display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Config,
    Touch,
    Display,
    Icl(u8),
    Wae(u8),
    Other,
}

impl Role {
    fn of(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        let number = number(name);
        let library = number.and_then(|number| u8::try_from(number).ok());
        if is_config_file(name) {
            Role::Config
        } else if let (Some(number), true) = (library, lower.ends_with(".icl")) {
            Role::Icl(number)
        } else if let (Some(number), true) = (library, lower.ends_with(".wae")) {
            Role::Wae(number)
        } else if lower.ends_with(".bin") && number == Some(13) {
            Role::Touch
        } else if lower.ends_with(".bin") && number == Some(14) {
            Role::Display
        } else {
            Role::Other
        }
    }
}

/// The display reads any `T5LCFG*.CFG` as its configuration.
pub(crate) fn is_config_file(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("T5LCFG") && name.ends_with(".CFG")
}

pub(crate) fn icl_name(file: u8) -> String {
    format!("{file}.icl")
}

pub(crate) fn wae_name(file: u8) -> String {
    format!("{file}.wae")
}

/// Numbered files by number, then the others by name.
fn sort(files: &mut [File]) {
    files.sort_by(|a, b| {
        let key = |file: &File| (file.number().is_none(), file.number(), file.name.clone());
        key(a).cmp(&key(b))
    });
}

/// Why a file cannot go in the folder as it is.
pub(crate) enum LayoutProblem {
    /// The name is not one an SD card holds.
    Name(&'static str),
    /// An earlier file has the same name, ignoring case as the SD card
    /// does.
    SameName(String),
    /// The file starts in the flash this earlier file takes.
    Overlap(String),
}

/// The problems of each file, given by name and length, in the order the
/// files come.
pub(crate) fn layout_problems<'a>(
    files: impl IntoIterator<Item = (&'a str, usize)>,
) -> Vec<(String, LayoutProblem)> {
    let files: Vec<_> = files.into_iter().collect();
    let mut problems = Vec::new();
    let mut names = BTreeMap::new();
    for &(name, _) in &files {
        if let Some(reason) = bad_name(name) {
            problems.push((name.to_owned(), LayoutProblem::Name(reason)));
        }
        if let Some(first) = names.insert(name.to_ascii_lowercase(), name) {
            problems.push((name.to_owned(), LayoutProblem::SameName(first.to_owned())));
        }
    }

    // Each numbered file's flash, by start, files with the same start in
    // the order they came; the file reaching furthest so far is the one a
    // later start runs into.
    let mut ranges: Vec<(u64, u64, &str)> = files
        .iter()
        .filter_map(|&(name, len)| {
            let start = u64::from(number(name)?) * u64::from(SLOT_SIZE);
            Some((start, start + (len as u64).max(1), name))
        })
        .collect();
    ranges.sort_by_key(|&(start, _, _)| start);
    let mut furthest: Option<(u64, &str)> = None;
    for (start, end, name) in ranges {
        if let Some((reach, first)) = furthest
            && start < reach
        {
            problems.push((name.to_owned(), LayoutProblem::Overlap(first.to_owned())));
        }
        if furthest.is_none_or(|(reach, _)| end > reach) {
            furthest = Some((end, name));
        }
    }
    problems
}

/// Why `name` cannot be a file name on the SD card, if it cannot.
fn bad_name(name: &str) -> Option<&'static str> {
    if name.is_empty() || name == "." || name == ".." {
        Some("it names no file")
    } else if name.contains(['/', '\\']) {
        Some("files go in DWIN_SET itself, not in folders")
    } else if name
        .chars()
        .any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        Some("an SD card cannot hold it")
    } else {
        None
    }
}
