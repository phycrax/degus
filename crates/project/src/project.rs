use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use degus_controls::Controls;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result, io_error};
use crate::model::{Control, ControlsFile, PROJECT_FILE_NAME, ProjectFile};

/// A loaded project: the project file and the controls files it lists.
#[derive(Debug, Clone)]
pub struct Project {
    file: PathBuf,
    root: PathBuf,
    pub config: ProjectFile,
    /// The controls files, in the order the project file lists them.
    pub controls: Vec<ControlsEntry>,
}

/// A controls file and the page it belongs to.
#[derive(Debug, Clone)]
pub struct ControlsEntry {
    /// Page ID.
    pub id: u16,
    /// Path relative to the project root.
    pub path: PathBuf,
    pub file: ControlsFile,
}

impl Project {
    /// Loads the project in the folder `dir`: its `project.toml` and every
    /// controls file it lists. Paths in the project are relative to `dir`.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let root = dir.as_ref().to_path_buf();
        if root.is_file() {
            return Err(Error::NotAFolder(root));
        }
        let file = root.join(PROJECT_FILE_NAME);
        let config: ProjectFile = read_toml(&file)?;
        let controls = config
            .controls
            .iter()
            .map(|spec| {
                Ok(ControlsEntry {
                    id: spec.id,
                    path: spec.path.clone(),
                    file: read_toml(&root.join(&spec.path))?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            file,
            root,
            config,
            controls,
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
        self.resolve(&self.config.output)
    }
    /// The controls of every page, ready for `degus-controls` to build.
    /// Entries sharing a page ID are merged; the check reports them.
    pub fn to_controls(&self) -> Controls {
        self.to_controls_with_sources().0
    }

    /// Like [`Project::to_controls`], with where each control comes from.
    pub fn to_controls_with_sources(&self) -> (Controls, SourceMap) {
        let mut controls = Controls::new();
        let mut sources = SourceMap::default();
        for (entry_index, entry) in self.controls.iter().enumerate() {
            let target = controls.page_mut(entry.id);
            let page_sources = sources.pages.entry(entry.id).or_default();
            for (index, control) in entry.file.controls.iter().enumerate() {
                let source = SourceRef {
                    entry: entry_index,
                    index,
                    name: control.name().map(str::to_owned),
                };
                if control.is_touch() {
                    page_sources.touch.push(source);
                } else {
                    page_sources.display.push(source);
                }
                match control {
                    Control::Button(button) => target.touch.push(button.to_touch()),
                    Control::Key(key) => target.touch.push(key.to_touch()),
                    Control::Input(input) => target.touch.push(input.to_touch()),
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

/// A control in a controls file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    /// Index into [`Project::controls`].
    pub entry: usize,
    /// Position of the control in its file, from 0.
    pub index: usize,
    /// The control's `name`, if it has one.
    pub name: Option<String>,
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(io_error(path))?;
    toml::from_str(&text).map_err(|source| Error::Toml {
        path: path.to_path_buf(),
        source,
    })
}
