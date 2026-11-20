use crate::bundle::SLOT_SIZE;

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
