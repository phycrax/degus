//! Creates project folders.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result, io_error};
use crate::model::{PROJECT_FILE_NAME, Size};

/// Creates the project folder `dir` with a `project.toml` for a screen of
/// `screen`, and returns the project file. Fails if `dir` exists.
///
/// The file starts with a comment pointing to `degus guide`, so whoever
/// opens it, person or agent, finds the format.
pub fn new(dir: impl AsRef<Path>, screen: Size) -> Result<PathBuf> {
    let mut folder = NewFolder::default();
    folder.write(
        PROJECT_FILE_NAME,
        format!(
            "# degus project: run \"degus guide\" for the format\n\
             screen = {{ w = {}, h = {} }}\n",
            screen.w, screen.h
        ),
    );
    let dir = dir.as_ref();
    folder.create(dir)?;
    Ok(dir.join(PROJECT_FILE_NAME))
}

/// The files of a project folder, gathered before anything is written.
#[derive(Default)]
struct NewFolder {
    /// Each file's path in the folder, and its contents.
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl NewFolder {
    fn write(&mut self, path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) {
        self.files.push((path.into(), bytes.into()));
    }

    /// Creates `dir` with the files. Fails if `dir` exists, and removes it
    /// again when a file cannot be written.
    fn create(&self, dir: &Path) -> Result<()> {
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(io_error(parent))?;
        }
        std::fs::create_dir(dir).map_err(|source| match source.kind() {
            std::io::ErrorKind::AlreadyExists => Error::Exists(dir.to_path_buf()),
            _ => Error::Io {
                path: dir.to_path_buf(),
                source,
            },
        })?;
        let result = self.files.iter().try_for_each(|(path, bytes)| {
            let path = dir.join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(io_error(parent))?;
            }
            std::fs::write(&path, bytes).map_err(io_error(&path))
        });
        if result.is_err() {
            let _ = std::fs::remove_dir_all(dir);
        }
        result
    }
}
