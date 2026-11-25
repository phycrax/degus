//! The names of the files in a `DWIN_SET` folder: those a set writes,
//! and what each name is to the display.

use crate::set::SLOT_SIZE;

/// The configuration file a set writes. The display reads any
/// `T5LCFG*.CFG` as its configuration.
pub const CONFIG_FILE: &str = "T5LCFG.CFG";
/// The touch control file a set writes, named as DWIN's tools name it.
/// The display reads file 13 ending in `.bin` as its touch controls.
pub const TOUCH_FILE: &str = "13TouchFile.bin";
/// The display control file a set writes, named as DWIN's tools name
/// it. The display reads file 14 ending in `.bin` as its display controls.
pub const DISPLAY_FILE: &str = "14ShowFile.bin";
/// The font 0 file a set writes, named as DWIN's tools name it. The
/// display stores file 0 at the start of its flash, where font 0 lives.
pub const FONT0_FILE: &str = "0_DWIN_ASC.HZK";

/// The endings of font files, which the display takes for any font ID.
const FONT_ENDINGS: [&str; 3] = [".hzk", ".dzk", ".bin"];

const TOUCH_NUMBER: u16 = 13;
const DISPLAY_NUMBER: u16 = 14;

/// What a file of the folder is to the display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Role {
    Config,
    Touch,
    Display,
    /// File 0 ending as a font file does. Font 0 has no header, so whether
    /// it is one is up to its length.
    Font0,
    Icl(u8),
    Wae(u8),
    Other,
}

impl Role {
    /// The role of the file named `name`, ignoring case.
    pub(crate) fn of(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        let number = number(name);
        let library = number.and_then(|number| u8::try_from(number).ok());
        if is_config_file(name) {
            Role::Config
        } else if let (Some(number), true) = (library, lower.ends_with(".icl")) {
            Role::Icl(number)
        } else if let (Some(number), true) = (library, lower.ends_with(".wae")) {
            Role::Wae(number)
        } else if lower.ends_with(".bin") && number == Some(TOUCH_NUMBER) {
            Role::Touch
        } else if lower.ends_with(".bin") && number == Some(DISPLAY_NUMBER) {
            Role::Display
        } else if number == Some(0) && FONT_ENDINGS.iter().any(|end| lower.ends_with(end)) {
            Role::Font0
        } else {
            Role::Other
        }
    }
}

/// The display reads any `T5LCFG*.CFG` as its configuration.
pub(crate) fn is_config_file(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("T5LCFG") && name.ends_with(".CFG")
}

/// The name a set writes the ICL of file number `file` under.
pub(crate) fn icl_name(file: u8) -> String {
    format!("{file}.icl")
}

/// The name a set writes the WAE of file number `file` under.
pub(crate) fn wae_name(file: u8) -> String {
    format!("{file}.wae")
}

/// A file of a `DWIN_SET` folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl File {
    pub fn new(name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            bytes: bytes.into(),
        }
    }

    /// The file number: the leading decimal digits of the name, as the
    /// display reads them. `22_Config.bin` is file 22, which the display
    /// stores at 22 × 256 KB in flash.
    ///
    /// `None` when the name does not start with a digit, as `T5LCFG.CFG`
    /// does not, or the number is over 65535.
    pub fn number(&self) -> Option<u16> {
        number(&self.name)
    }

    /// How many file numbers the file takes in flash, from
    /// [`File::number`] on.
    pub fn slots(&self) -> usize {
        self.bytes.len().div_ceil(SLOT_SIZE as usize).max(1)
    }
}

/// The file number of the file named `name`; see [`File::number`].
pub(crate) fn number(name: &str) -> Option<u16> {
    let mut number: u32 = 0;
    let mut any = false;
    for digit in name.bytes().take_while(u8::is_ascii_digit) {
        number = number * 10 + u32::from(digit - b'0');
        if number > u32::from(u16::MAX) {
            return None;
        }
        any = true;
    }
    any.then_some(number as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_the_leading_digits() {
        assert_eq!(number("0021_com_calib.png"), Some(21));
        assert_eq!(number("7.jpg"), Some(7));
        assert_eq!(number("13TouchFile.bin"), Some(13));
        assert_eq!(number("65535x.png"), Some(65535));
        assert_eq!(number("0000000000012.png"), Some(12));
    }

    #[test]
    fn names_without_a_number_have_none() {
        assert_eq!(number("T5LCFG.CFG"), None);
        assert_eq!(number("_1.png"), None);
        assert_eq!(number("65536.png"), None);
        assert_eq!(number(""), None);
    }

    #[test]
    fn slots_round_up() {
        assert_eq!(File::new("1.bin", vec![]).slots(), 1);
        assert_eq!(File::new("1.bin", vec![0; 256 * 1024]).slots(), 1);
        assert_eq!(File::new("1.bin", vec![0; 256 * 1024 + 1]).slots(), 2);
    }
}
