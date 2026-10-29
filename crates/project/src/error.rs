use std::path::PathBuf;

use crate::check::{Diagnostic, Severity};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while loading or building a project.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{}: {source}", path.display())]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    /// The check found errors. Holds every diagnostic, warnings included.
    #[error(
        "the project has {} error(s)",
        .0.iter().filter(|d| d.severity == Severity::Error).count()
    )]
    Check(Vec<Diagnostic>),
    #[error("{id}.icl: {source}")]
    Icl {
        id: u16,
        #[source]
        source: degus_icl::Error,
    },
    #[error("{id}.wae: {source}")]
    Wae {
        id: u16,
        #[source]
        source: degus_wae::Error,
    },
    #[error(transparent)]
    Controls(#[from] degus_controls::Error),
    #[error(
        "{first} and {second} overlap in flash: a file takes {} KB per file number, \
         so a large file runs into the next numbers",
        crate::build::SLOT_SIZE / 1024
    )]
    FlashOverlap { first: String, second: String },
    #[error(
        "{} is a file; give the project folder, which holds {}",
        .0.display(),
        crate::model::PROJECT_FILE_NAME
    )]
    NotAFolder(PathBuf),
    #[error("{} already exists", .0.display())]
    Exists(PathBuf),
}

pub(crate) fn io_error(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.into();
    move |source| Error::Io { path, source }
}
