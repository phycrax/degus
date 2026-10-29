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
        let mut controls = Controls::new();
        for entry in &self.controls {
            let target = controls.page_mut(entry.id);
            for control in &entry.file.controls {
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
        controls
    }
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(io_error(path))?;
    toml::from_str(&text).map_err(|source| Error::Toml {
        path: path.to_path_buf(),
        source,
    })
}
