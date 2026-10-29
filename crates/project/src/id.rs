/// Extracts the file number from the leading decimal digits of a `DWIN_SET`
/// file name, the way the display does: `22_Config.bin` is file 22, stored
/// at 22 × [`SLOT_SIZE`](crate::SLOT_SIZE) in flash.
///
/// Returns `None` when the name does not start with a digit or the number
/// exceeds 65535.
pub(crate) fn id_from_file_name(name: &str) -> Option<u16> {
    let digits = name.bytes().take_while(u8::is_ascii_digit);
    let mut id: u32 = 0;
    let mut any = false;
    for d in digits {
        id = id * 10 + u32::from(d - b'0');
        if id > u32::from(u16::MAX) {
            return None;
        }
        any = true;
    }
    any.then_some(id as u16)
}

#[cfg(test)]
mod tests {
    use super::id_from_file_name;

    #[test]
    fn parses_leading_digits() {
        assert_eq!(id_from_file_name("0021_com_calib.png"), Some(21));
        assert_eq!(id_from_file_name("7.jpg"), Some(7));
        assert_eq!(id_from_file_name("0.bmp"), Some(0));
        assert_eq!(id_from_file_name("65535x.png"), Some(65535));
        assert_eq!(id_from_file_name("0000000000012.png"), Some(12));
    }

    #[test]
    fn rejects_missing_or_large_ids() {
        assert_eq!(id_from_file_name("logo.png"), None);
        assert_eq!(id_from_file_name("_1.png"), None);
        assert_eq!(id_from_file_name("65536.png"), None);
        assert_eq!(id_from_file_name(""), None);
    }
}
