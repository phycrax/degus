pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while writing or reading a configuration file.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{field} is {value}, not in {min}-{max}")]
    Range {
        field: &'static str,
        value: i32,
        min: i32,
        max: i32,
    },
    #[error("baud is {0}, which the UART cannot run at within 2 %")]
    Baud(u32),
    #[error("sd_folder {0:?} must be 1-8 characters of 0-9, A-Z, a-z, _ and -")]
    SdFolder(String),
    #[error("not a configuration file: it does not start with T5LC1")]
    Magic,
    #[error("a configuration file holds at least 16 bytes, not {0}")]
    TooShort(usize),
}
