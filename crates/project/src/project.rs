use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use degus_controls::Controls;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result, io_error};
use crate::model::{
    Control, Declarations, IclSpec, PROJECT_FILE_NAME, PageSpec, RootFile, Settings, WaeSpec,
};

/// A loaded project: the settings of `project.toml`, and the declarations of
/// it and the files it includes.
#[derive(Debug, Clone)]
pub struct Project {
    file: PathBuf,
    root: PathBuf,
    pub settings: Settings,
    /// `project.toml`, then each file it includes, in the order `include`
    /// lists them. A file listed twice is here once; the check reports it.
    pub files: Vec<ProjectFile>,
}

/// A file of the project and what it declares.
#[derive(Debug, Clone)]
pub struct ProjectFile {
    /// Path relative to the project root.
    pub path: PathBuf,
    pub declarations: Declarations,
}

impl Project {
    /// Loads the project in the folder `dir`: its `project.toml` and every
    /// file it includes. Paths in the project are relative to `dir`.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let root = dir.as_ref().to_path_buf();
        if root.is_file() {
            return Err(Error::NotAFolder(root));
        }
        let file = root.join(PROJECT_FILE_NAME);
        let (settings, declarations) = read_toml::<RootFile>(&file)?.split();
        let mut files = vec![ProjectFile {
            path: PROJECT_FILE_NAME.into(),
            declarations,
        }];
        for path in &settings.include {
            if files.iter().any(|file| file.path == *path) {
                continue;
            }
            files.push(ProjectFile {
                path: path.clone(),
                declarations: read_toml(&root.join(path))?,
            });
        }
        Ok(Self {
            file,
            root,
            settings,
            files,
        })
    }

    /// The project file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The project folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `path` relative to the project root.
    pub fn resolve(&self, path: impl AsRef<Path>) -> PathBuf {
        self.root.join(path)
    }

    /// The folder [`Project::build`](crate::Project::build) writes `DWIN_SET`
    /// in.
    pub fn output_dir(&self) -> PathBuf {
        self.resolve(&self.settings.output)
    }

    /// The ICLs of every file.
    pub fn icls(&self) -> impl Iterator<Item = &IclSpec> {
        self.files.iter().flat_map(|file| &file.declarations.icl)
    }

    /// The WAEs of every file.
    pub fn waes(&self) -> impl Iterator<Item = &WaeSpec> {
        self.files.iter().flat_map(|file| &file.declarations.wae)
    }

    /// The files every file copies into the output.
    pub fn copies(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.iter().flat_map(|file| &file.declarations.copy)
    }

    /// The pages, each with its name and the index of its file in
    /// [`Project::files`]. A name declared in several files is the first
    /// one's; the check reports the others.
    pub fn pages(&self) -> impl Iterator<Item = (usize, &str, &PageSpec)> {
        self.files
            .iter()
            .enumerate()
            .flat_map(|(index, file)| {
                file.declarations
                    .page
                    .iter()
                    .map(move |(name, page)| (index, name.as_str(), page))
            })
            .filter(|&(index, name, _)| self.page(name).is_some_and(|(first, _)| first == index))
    }

    /// The page named `name`, with the index of its file in
    /// [`Project::files`].
    pub fn page(&self, name: &str) -> Option<(usize, &PageSpec)> {
        self.files
            .iter()
            .enumerate()
            .find_map(|(index, file)| file.declarations.page.get(name).map(|page| (index, page)))
    }

    /// The ID of the page named `name`.
    pub fn page_id(&self, name: &str) -> Option<u16> {
        self.page(name).map(|(_, page)| page.id)
    }

    /// The control named `name`, with the index of its file in
    /// [`Project::files`]. A name declared in several files is the first
    /// one's; the check reports the others.
    pub fn control(&self, name: &str) -> Option<(usize, &Control)> {
        self.files.iter().enumerate().find_map(|(index, file)| {
            file.declarations
                .control
                .get(name)
                .map(|control| (index, control))
        })
    }

    /// The controls of every page, ready for `degus-controls` to build.
    /// Pages sharing an ID are merged, and names nothing declares are left
    /// out; the check reports both.
    pub fn to_controls(&self) -> Controls {
        self.to_controls_with_sources().0
    }

    /// Like [`Project::to_controls`], with where each control comes from.
    pub fn to_controls_with_sources(&self) -> (Controls, SourceMap) {
        let page_id = |name: &str| self.page_id(name);
        let mut controls = Controls::new();
        let mut sources = SourceMap::default();
        for (_, _, page) in self.pages() {
            let target = controls.page_mut(page.id);
            let page_sources = sources.pages.entry(page.id).or_default();
            for name in &page.controls {
                let Some((file, control)) = self.control(name) else {
                    continue;
                };
                let source = SourceRef {
                    file,
                    name: name.clone(),
                };
                if control.is_touch() {
                    page_sources.touch.push(source);
                } else {
                    page_sources.display.push(source);
                }
                match control {
                    Control::Button(button) => target.touch.push(button.to_touch(page_id)),
                    Control::Key(key) => target.touch.push(key.to_touch(page_id)),
                    Control::Input(input) => target.touch.push(input.to_touch(page_id)),
                    Control::Icon(icon) => target.display.push(icon.to_display()),
                    Control::Data(data) => target.display.push(data.to_display()),
                    Control::Text(text) => target.display.push(text.to_display()),
                    Control::Progress(bar) => target.display.push(bar.to_display()),
                }
            }
        }
        (controls, sources)
    }
}

/// Where each control of [`Project::to_controls_with_sources`] comes from,
/// page by page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMap {
    pub pages: BTreeMap<u16, PageSources>,
}

impl SourceMap {
    /// The sources of a page's controls, if it has any.
    pub fn page(&self, id: u16) -> Option<&PageSources> {
        self.pages.get(&id)
    }
}

/// The sources of one page's controls, in the order of its
/// [`PageControls`](degus_controls::PageControls): `touch[i]` is where
/// touch control `i` comes from, and the same for `display`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageSources {
    pub touch: Vec<SourceRef>,
    pub display: Vec<SourceRef>,
}

/// A control as a file declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    /// Index into [`Project::files`] of the file declaring it.
    pub file: usize,
    /// The control's name.
    pub name: String,
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(io_error(path))?;
    toml::from_str(&text).map_err(|source| Error::Toml {
        path: path.to_path_buf(),
        source,
    })
}
