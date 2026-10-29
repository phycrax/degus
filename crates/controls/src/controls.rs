use std::collections::BTreeMap;

use crate::display::{Display, RECORD_LEN};
use crate::error::{Error, Result};
use crate::touch::Touch;

/// Name the DGUS editor gives the touch configuration file.
pub const TOUCH_FILE_NAME: &str = "13TouchFile.bin";
/// Name the DGUS editor gives the display variable configuration file.
pub const DISPLAY_FILE_NAME: &str = "14ShowFile.bin";

/// Page IDs must be below this: the display file's page table has 4092
/// entries.
pub const MAX_PAGES: u16 = 4092;
/// Display controls one page can hold.
pub const MAX_DISPLAYS_PER_PAGE: usize = 255;

const TOUCH_END: [u8; 2] = [0xFF, 0xFF];
const DISPLAY_MAGIC: [u8; 8] = *b"\x14DGUS_2\x10";
const HEADER_LEN: usize = 16;
const ENTRY_LEN: usize = 4;
/// Records follow the header and the page table, at 0x4000.
const RECORDS_START: usize = HEADER_LEN + ENTRY_LEN * MAX_PAGES as usize;
const DISPLAY_END: [u8; RECORD_LEN] = [0xFF; RECORD_LEN];
/// Page table offsets are 24-bit.
const MAX_OFFSET: usize = 0xFF_FFFF;

/// The controls of one page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageControls {
    /// Touch controls, written in this order.
    pub touch: Vec<Touch>,
    /// Display controls in drawing order: later ones draw over earlier ones.
    pub display: Vec<Display>,
}

/// The touch (file 13) and display (file 14) controls of every page.
///
/// Pages without controls still count: the display file records the
/// highest page ID, as the DGUS editor does for every page in the project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Controls {
    pages: BTreeMap<u16, PageControls>,
}

/// The two generated files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltControls {
    /// File 13, [`TOUCH_FILE_NAME`].
    pub touch: Vec<u8>,
    /// File 14, [`DISPLAY_FILE_NAME`].
    pub display: Vec<u8>,
}

impl Controls {
    pub fn new() -> Self {
        Self::default()
    }

    /// The page with this ID, created empty if missing.
    pub fn page_mut(&mut self, id: u16) -> &mut PageControls {
        self.pages.entry(id).or_default()
    }

    /// Adds or replaces a page, returning the previous one.
    pub fn insert(&mut self, id: u16, page: PageControls) -> Option<PageControls> {
        self.pages.insert(id, page)
    }

    pub fn remove(&mut self, id: u16) -> Option<PageControls> {
        self.pages.remove(&id)
    }

    pub fn get(&self, id: u16) -> Option<&PageControls> {
        self.pages.get(&id)
    }

    /// Pages in ascending ID order.
    pub fn iter(&self) -> impl Iterator<Item = (u16, &PageControls)> {
        self.pages.iter().map(|(&id, page)| (id, page))
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Generates files 13 and 14.
    pub fn build(&self) -> Result<BuiltControls> {
        if let Some(&page) = self.pages.keys().find(|&&id| id >= MAX_PAGES) {
            return Err(Error::PageId { page });
        }
        Ok(BuiltControls {
            touch: self.touch_file()?,
            display: self.display_file()?,
        })
    }

    /// Records of every page in ascending page order, then `FF FF`.
    fn touch_file(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for (&id, page) in &self.pages {
            for touch in &page.touch {
                touch.encode(id, &mut out)?;
            }
        }
        out.extend_from_slice(&TOUCH_END);
        Ok(out)
    }

    /// Header, page table, the records of each page, then 32 × `FF`.
    fn display_file(&self) -> Result<Vec<u8>> {
        let max_page = self.pages.keys().next_back().copied().unwrap_or(0);
        let mut out = Vec::with_capacity(RECORDS_START);
        out.extend_from_slice(&DISPLAY_MAGIC);
        out.extend_from_slice(&max_page.to_be_bytes());
        out.resize(HEADER_LEN, 0);

        let mut records = Vec::new();
        for id in 0..MAX_PAGES {
            let display = self.get(id).map_or(&[][..], |page| &page.display);
            if display.len() > MAX_DISPLAYS_PER_PAGE {
                return Err(Error::TooManyDisplays {
                    page: id,
                    count: display.len(),
                });
            }
            let offset = RECORDS_START + records.len();
            if offset > MAX_OFFSET {
                return Err(Error::FileTooLarge);
            }
            out.push(display.len() as u8);
            out.extend_from_slice(&(offset as u32).to_be_bytes()[1..]);
            for control in display {
                records.extend_from_slice(&control.encode(id)?);
            }
        }
        out.extend_from_slice(&records);
        out.extend_from_slice(&DISPLAY_END);
        Ok(out)
    }
}
