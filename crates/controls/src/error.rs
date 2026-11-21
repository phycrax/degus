use crate::controls::MAX_PAGES;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while writing or reading the control files.
///
/// `index` counts a page's touch or display controls from 0.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the control files name page {page}; page IDs must be below {MAX_PAGES}")]
    PageId { page: u16 },
    #[error("page {page} has {count} display controls; at most 255 fit")]
    TooManyDisplays { page: u16, count: usize },
    #[error("display records exceed the 16 MB the page table can address")]
    FileTooLarge,
    #[error("page {page}, touch control {index}: bit index {bit} is out of range 0-15")]
    BitIndex { page: u16, index: usize, bit: u8 },
    #[error(
        "page {page}, touch control {index}: basic touch key code {code:#06X} must not start with FE or FD"
    )]
    BasicKey { page: u16, index: usize, code: u16 },
    #[error(
        "page {page}, touch control {index}: an unknown control's record must be whole 16-byte units of at most 64 bytes, starting with FE or FD"
    )]
    UnknownTouch { page: u16, index: usize },
    #[error("not a display file: it does not start with the DGUS II header")]
    NotDisplayFile,
    #[error("the display file puts the controls of page {page} past its end")]
    PageTable { page: u16 },
    #[error("the touch file ends without its FF FF end marker")]
    TouchFileEnd,
    #[error("the touch file has a record at offset {offset} that is not a touch control")]
    TouchRecord { offset: usize },
    #[error(
        "page {page}, display control {index}: unit {unit:?} must be ASCII and at most 11 bytes"
    )]
    Unit {
        page: u16,
        index: usize,
        unit: String,
    },
}
