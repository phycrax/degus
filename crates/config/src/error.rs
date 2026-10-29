pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while building a configuration file.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{field} is {value}, not in {min}-{max}")]
    Range {
        field: &'static str,
        value: i32,
        min: i32,
        max: i32,
    },
    #[error("sd_folder {0:?} must be 1-8 characters of 0-9, A-Z, a-z, _ and -")]
    SdFolder(String),
}
