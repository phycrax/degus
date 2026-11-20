//! What the simulator runs: the screen, the controls, the configuration and
//! the images. [`World::from_project`] loads it from a degus project; a
//! loader for built `DWIN_SET` folders can produce the same.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use degus_config::Config;
use degus_controls::Controls;
use degus_icl::IclOptions;
use degus_project::{Diagnostic, Project, Severity, SourceMap, SourceRef};

use crate::audio::{AudioSource, ProjectAudio};
use image::RgbImage;

/// Why a project could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Project(#[from] degus_project::Error),
    /// The check found errors; the build would refuse the project too.
    /// Holds every diagnostic, warnings included.
    #[error(
        "the project has {} error(s)",
        .0.iter().filter(|d| d.severity == Severity::Error).count()
    )]
    Check(Vec<Diagnostic>),
}

/// Screen size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub w: u16,
    pub h: u16,
}

/// A page the project has: a declared one or a background image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInfo {
    pub id: u16,
    /// Its name, or a note that it is only a background image.
    pub label: String,
}

/// Images by ICL and image ID, decoded on first use.
pub trait IconSource: Send {
    /// Image `id` of ICL `lib`, without transparency, as the display has
    /// it.
    fn icon(&self, lib: u16, id: u16) -> Option<Arc<RgbImage>>;

    /// Whether ICL `lib` has image `id`, without decoding it.
    fn has_icon(&self, lib: u16, id: u16) -> bool;

    /// The image IDs of ICL `lib`.
    fn ids(&self, lib: u16) -> Vec<u16>;

    /// Problems met while decoding since the last call.
    fn take_problems(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Everything the simulator needs from a project.
pub struct World {
    pub screen: Screen,
    /// The project's `[config]`, or the defaults without one.
    pub config: Config,
    pub controls: Controls,
    /// Where each control comes from; empty when not loaded from a project.
    pub sources: SourceMap,
    /// The project's files, by [`SourceRef::file`].
    pub files: Vec<PathBuf>,
    pub pages: Vec<PageInfo>,
    pub icons: Box<dyn IconSource>,
    pub audio: Box<dyn AudioSource>,
    /// The check's warnings.
    pub warnings: Vec<Diagnostic>,
    /// The project folder, when loaded from one.
    pub root: Option<PathBuf>,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("screen", &self.screen)
            .field("pages", &self.pages.len())
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl World {
    /// Loads and checks the project in the folder `dir`.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, LoadError> {
        Self::from_project(&Project::load(dir)?)
    }

    /// Checks `project` and takes what the simulator needs. Fails on the
    /// errors that would stop a build.
    pub fn from_project(project: &Project) -> Result<Self, LoadError> {
        let diagnostics = project.check();
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            return Err(LoadError::Check(diagnostics));
        }
        // The check has accepted `[config]`, so this cannot fail.
        let config = match &project.settings.config {
            Some(spec) => spec.to_config().unwrap_or_default(),
            None => Config::default(),
        };
        let (controls, sources) = project.to_controls_with_sources();
        let icons = ProjectIcons::new(project);

        let mut labels: BTreeMap<u16, Vec<&str>> = BTreeMap::new();
        for (_, name, page) in project.pages() {
            labels.entry(page.id).or_default().push(name);
        }
        for id in icons.ids(config.background_icl.into()) {
            labels.entry(id).or_default();
        }
        let pages = labels
            .into_iter()
            .map(|(id, names)| PageInfo {
                id,
                label: if names.is_empty() {
                    "background only".to_owned()
                } else {
                    names.join(", ")
                },
            })
            .collect();

        Ok(Self {
            screen: Screen {
                w: project.settings.screen.w,
                h: project.settings.screen.h,
            },
            config,
            controls,
            sources,
            files: project.files.iter().map(|file| file.path.clone()).collect(),
            pages,
            icons: Box::new(icons),
            audio: Box::new(ProjectAudio::new(project)),
            warnings: diagnostics,
            root: Some(project.root().to_path_buf()),
        })
    }

    /// Where a control comes from, such as `controls/home.toml:
    /// [control.fan_on]`, as the check's messages name it.
    pub fn describe(&self, source: &SourceRef) -> String {
        let file = self
            .files
            .get(source.file)
            .map_or_else(|| "?".to_owned(), |path| path.display().to_string());
        format!("{file}: [control.{}]", source.name)
    }
}

/// Decoded images by ICL and image ID; `None` for ones that failed.
type IconCache = HashMap<(u16, u16), Option<Arc<RgbImage>>>;

/// The images of a project's `[[icl]]` tables, read from their source
/// files and flattened onto white like the build does.
pub struct ProjectIcons {
    libs: BTreeMap<u16, BTreeMap<u16, PathBuf>>,
    background: [u8; 3],
    cache: RefCell<IconCache>,
    problems: RefCell<Vec<String>>,
}

impl ProjectIcons {
    pub fn new(project: &Project) -> Self {
        let libs = project
            .icls()
            .map(|spec| {
                let images = spec
                    .images
                    .iter()
                    .map(|source| (source.id, project.resolve(&source.path)))
                    .collect();
                (spec.id, images)
            })
            .collect();
        Self {
            libs,
            background: IclOptions::default().background,
            cache: RefCell::default(),
            problems: RefCell::default(),
        }
    }

    fn decode(&self, path: &Path) -> Option<Arc<RgbImage>> {
        let image = match image::open(path) {
            Ok(image) => image,
            Err(error) => {
                self.problems
                    .borrow_mut()
                    .push(format!("{}: {error}", path.display()));
                return None;
            }
        };
        let rgb = degus_icl::flatten(&image, self.background);
        RgbImage::from_raw(image.width(), image.height(), rgb).map(Arc::new)
    }
}

impl IconSource for ProjectIcons {
    fn icon(&self, lib: u16, id: u16) -> Option<Arc<RgbImage>> {
        if let Some(cached) = self.cache.borrow().get(&(lib, id)) {
            return cached.clone();
        }
        let icon = self
            .libs
            .get(&lib)
            .and_then(|images| images.get(&id))
            .and_then(|path| self.decode(path));
        self.cache.borrow_mut().insert((lib, id), icon.clone());
        icon
    }

    fn has_icon(&self, lib: u16, id: u16) -> bool {
        self.libs
            .get(&lib)
            .is_some_and(|images| images.contains_key(&id))
    }

    fn ids(&self, lib: u16) -> Vec<u16> {
        self.libs
            .get(&lib)
            .map(|images| images.keys().copied().collect())
            .unwrap_or_default()
    }

    fn take_problems(&self) -> Vec<String> {
        std::mem::take(&mut self.problems.borrow_mut())
    }
}
