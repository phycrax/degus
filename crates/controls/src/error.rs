use crate::controls::MAX_PAGES;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while building the configuration files.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("page {page}: page IDs must be below {MAX_PAGES}")]
    PageId { page: u16 },
    #[error("page {page} has {count} display controls; at most 255 fit")]
    TooManyDisplays { page: u16, count: usize },
    #[error("display records exceed the 16 MB the page table can address")]
    FileTooLarge,
    #[error("page {page}: bit index {bit} is out of range 0-15")]
    BitIndex { page: u16, bit: u8 },
    #[error("page {page}: basic touch key code {code:#06X} must not start with FE or FD")]
    BasicKey { page: u16, code: u16 },
    #[error("page {page}: unit {unit:?} must be ASCII and at most 11 bytes")]
    Unit { page: u16, unit: String },
}
