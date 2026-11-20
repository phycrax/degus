//! ICL libraries as data, and their file layout.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::format::{
    CRC_START, FLASH_BANK_SIZE, HEADER_LEN, MAGIC, RECORD_HEADER_LEN, SLOT_SIZE, VERSION, checksum,
};
use crate::jpeg::PreparedJpeg;

/// An icon library: icons by ID.
///
/// [`IclBuilder`](crate::IclBuilder) makes one from images, and
/// [`Icl::to_bytes`] writes it as a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Icl {
    pub icons: BTreeMap<u16, Icon>,
}

/// An icon: a baseline JPEG laid out the way the T5L decodes it.
///
/// [`IclBuilder`](crate::IclBuilder) makes icons from images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    width: u16,
    height: u16,
    /// Bytes of `data` up to and including the SOS segment.
    header_len: u16,
    /// The JPEG as an ICL record stores it: the SOS segment padded so that
    /// the scan data starts on a 4-byte boundary in the file, and zeros
    /// after EOI.
    data: Vec<u8>,
}

impl Icon {
    pub(crate) fn from_prepared(jpeg: PreparedJpeg) -> Self {
        Self {
            width: jpeg.width,
            height: jpeg.height,
            header_len: jpeg.header_len,
            data: jpeg.data,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// The icon as a standard JPEG, without the padding of its record.
    /// The chip's per-icon limit ([`Chip::max_jpeg_size`]) applies to its
    /// length.
    ///
    /// [`Chip::max_jpeg_size`]: crate::Chip::max_jpeg_size
    pub fn jpeg(&self) -> Vec<u8> {
        let (header, stream) = self.data.split_at(usize::from(self.header_len));
        // The header ends with the SOS segment, whose body holds no 0xFF.
        let sos = header
            .windows(2)
            .rposition(|w| w == [0xFF, 0xDA])
            .expect("an icon's header ends with SOS");
        // The SOS length without padding: 6 bytes and 2 per component.
        let used = 6 + 2 * usize::from(header[sos + 4]);
        // The scan ends with EOI (FF D9); only zeros follow.
        let end = stream.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        let mut jpeg = Vec::with_capacity(sos + used + end);
        jpeg.extend_from_slice(&header[..sos + 2]);
        jpeg.extend_from_slice(&(used as u16).to_be_bytes());
        jpeg.extend_from_slice(&header[sos + 4..sos + 2 + used]);
        jpeg.extend_from_slice(&stream[..end]);
        jpeg
    }

    /// Bytes of the icon's record: its header and the stored JPEG.
    fn record_len(&self) -> usize {
        RECORD_HEADER_LEN + self.data.len()
    }
}

impl Icl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes the ICL file for file number `file`.
    ///
    /// The display stores file `file` at `file` × 256 KB in flash. An icon
    /// record must not cross a multiple of 16 MB there, so a record that
    /// would is moved forward to start on it.
    pub fn to_bytes(&self, file: u8) -> Result<Vec<u8>> {
        let max_id = *self.icons.keys().next_back().ok_or(Error::Empty)?;
        let table_end = HEADER_LEN + 4 * (usize::from(max_id) + 1);
        let mut bytes = vec![0; table_end];
        bytes[..6].copy_from_slice(&MAGIC);
        bytes[12] = VERSION;
        bytes[13..15].copy_from_slice(&max_id.to_be_bytes());

        let base = u64::from(file) * SLOT_SIZE;
        for (&id, icon) in &self.icons {
            let size = icon.record_len().next_multiple_of(4);
            let padding = bank_padding(base + bytes.len() as u64, size);
            bytes.resize(bytes.len() + padding, 0);
            let start = bytes.len();
            let offset = u32::try_from(start).map_err(|_| Error::FileTooLarge)?;
            let entry = HEADER_LEN + 4 * usize::from(id);
            bytes[entry..entry + 4].copy_from_slice(&offset.to_be_bytes());

            let stream_len = icon.data.len() - usize::from(icon.header_len);
            let stream_len = u32::try_from(stream_len).map_err(|_| Error::FileTooLarge)?;
            bytes.extend_from_slice(&icon.width.to_be_bytes());
            bytes.extend_from_slice(&icon.height.to_be_bytes());
            bytes.extend_from_slice(&icon.header_len.to_be_bytes());
            bytes.extend_from_slice(&stream_len.to_be_bytes());
            bytes.extend_from_slice(&icon.data);
            bytes.resize(start + size, 0);
        }

        let length = u32::try_from(bytes.len() - CRC_START).map_err(|_| Error::FileTooLarge)?;
        bytes[8..12].copy_from_slice(&length.to_be_bytes());
        let crc = checksum(&bytes[CRC_START..]);
        bytes[6..8].copy_from_slice(&crc.to_le_bytes());
        Ok(bytes)
    }
}

/// Zero bytes to insert before a record of `size` bytes that would start at
/// flash address `start`, so that it does not cross a 16 MB boundary.
fn bank_padding(start: u64, size: usize) -> usize {
    let last = start + size as u64 - 1;
    if start / FLASH_BANK_SIZE == last / FLASH_BANK_SIZE {
        0
    } else {
        (last / FLASH_BANK_SIZE * FLASH_BANK_SIZE - start) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_padding_moves_straddling_records_to_the_boundary() {
        // File 63 starts 256 KB below the first 16 MB boundary.
        let file = 63 * SLOT_SIZE;
        assert_eq!(bank_padding(file, 1000), 0);
        assert_eq!(bank_padding(file + 262_144 - 1000, 1000), 0);
        assert_eq!(bank_padding(file + 262_144 - 999, 1000), 999);
        assert_eq!(bank_padding(64 * SLOT_SIZE, 1 << 20), 0);
    }
}
