use std::fmt;
use std::path::PathBuf;

use crate::check::Diagnostic;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while writing or reading a bundle.
///
/// `Debug` prints the same as `Display`, so that a `main` returning this
/// error shows a readable message.
#[derive(thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// [`Bundle::check`](crate::Bundle::check) found errors. Holds every
    /// diagnostic, warnings included, and prints them all.
    #[error("{}", check_message(.0))]
    Check(Vec<Diagnostic>),
    #[error("{name}: {0}", name = crate::CONFIG_FILE)]
    Config(#[from] degus_config::Error),
    #[error(transparent)]
    Controls(#[from] degus_controls::Error),
    #[error("{file}.icl: {source}")]
    Icl {
        file: u8,
        #[source]
        source: degus_icl::Error,
    },
    #[error("{file}.wae: {source}")]
    Wae {
        file: u8,
        #[source]
        source: degus_wae::Error,
    },
    #[error("{name:?} is not a file name for DWIN_SET: {reason}")]
    FileName { name: String, reason: &'static str },
    #[error("{first} and {second} are the same file to the display")]
    Duplicate { first: String, second: String },
    #[error(
        "{first} and {second} overlap in flash: a file takes 256 KB per file number, so a \
         large file runs into the next numbers"
    )]
    FlashOverlap { first: String, second: String },
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// `the bundle has 2 errors`, then each diagnostic on a line of its own.
fn check_message(diagnostics: &[Diagnostic]) -> String {
    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    let plural = if errors == 1 { "" } else { "s" };
    let mut message = format!("the bundle has {errors} error{plural}");
    for diagnostic in diagnostics {
        message.push('\n');
        message.push_str(&diagnostic.to_string());
    }
    message
}

pub(crate) fn io_error(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.into();
    move |source| Error::Io { path, source }
}
