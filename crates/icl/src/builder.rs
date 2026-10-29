use std::collections::BTreeMap;
use std::path::PathBuf;

use image::DynamicImage;

use crate::encode::encode_source;
use crate::error::{Error, Result};
use crate::format::{
    CRC_START, FLASH_BANK_SIZE, HEADER_LEN, MAGIC, RECORD_HEADER_LEN, VERSION, checksum,
};
use crate::options::{FlashSlot, IclOptions};

/// Where an icon's pixels come from.
#[derive(Debug, Clone)]
pub enum IconSource {
    /// An image file (PNG, BMP, GIF or JPEG).
    File(PathBuf),
    /// Encoded image bytes (PNG, BMP, GIF or JPEG).
    Encoded(Vec<u8>),
    /// Decoded pixels.
    Image(DynamicImage),
    /// A baseline JPEG to store unchanged, regardless of
    /// [`IclOptions::passthrough_jpeg`]. Fails if the T5L cannot decode it.
    Jpeg(Vec<u8>),
}

/// Per-icon details of a built ICL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconReport {
    pub id: u16,
    pub width: u16,
    pub height: u16,
    /// Absolute offset of the icon record in the file.
    pub offset: u32,
    /// Record size in bytes, including its 10-byte header and alignment padding.
    pub size: usize,
    /// JPEG quality used, or `None` when a source JPEG was stored unchanged.
    pub quality: Option<u8>,
}

/// A serialized ICL file.
#[derive(Debug, Clone)]
pub struct BuiltIcl {
    pub bytes: Vec<u8>,
    /// One entry per icon, in ID order.
    pub icons: Vec<IconReport>,
}

/// Collects icons by ID and serializes them into an ICL file.
#[derive(Debug, Clone, Default)]
pub struct IclBuilder {
    pub options: IclOptions,
    icons: BTreeMap<u16, IconSource>,
}

impl IclBuilder {
    pub fn new(options: IclOptions) -> Self {
        Self {
            options,
            icons: BTreeMap::new(),
        }
    }

    /// Sets the icon for `id`, returning the source it replaced.
    pub fn insert(&mut self, id: u16, source: IconSource) -> Option<IconSource> {
        self.icons.insert(id, source)
    }

    pub fn remove(&mut self, id: u16) -> Option<IconSource> {
        self.icons.remove(&id)
    }

    pub fn contains(&self, id: u16) -> bool {
        self.icons.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }

    /// Icon IDs in ascending order.
    pub fn ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.icons.keys().copied()
    }

    /// Encodes all icons and serializes the ICL file.
    pub fn build(&self) -> Result<BuiltIcl> {
        let max_id = *self.icons.keys().next_back().ok_or(Error::Empty)?;
        let table_end = HEADER_LEN + 4 * (usize::from(max_id) + 1);
        let mut bytes = vec![0; table_end];
        bytes[..6].copy_from_slice(&MAGIC);
        bytes[12] = VERSION;
        bytes[13..15].copy_from_slice(&max_id.to_be_bytes());

        bytes.resize(round_up(table_end, self.options.head_align), 0);

        let mut icons = Vec::with_capacity(self.icons.len());
        for (&id, source) in &self.icons {
            let encoded = encode_source(source, &self.options).map_err(|e| Error::Icon {
                id,
                source: Box::new(e),
            })?;
            let jpeg = encoded.jpeg;
            let size = round_up(RECORD_HEADER_LEN + jpeg.data.len(), self.options.body_align);
            if let Some(slot) = self.options.flash_slot {
                let padding = bank_padding(slot, bytes.len(), size);
                bytes.resize(bytes.len() + padding, 0);
            }
            let start = bytes.len();
            let offset = u32::try_from(start).map_err(|_| Error::FileTooLarge)?;
            let entry = HEADER_LEN + 4 * usize::from(id);
            bytes[entry..entry + 4].copy_from_slice(&offset.to_be_bytes());

            let stream_len = jpeg.data.len() - usize::from(jpeg.header_len);
            bytes.extend_from_slice(&jpeg.width.to_be_bytes());
            bytes.extend_from_slice(&jpeg.height.to_be_bytes());
            bytes.extend_from_slice(&jpeg.header_len.to_be_bytes());
            bytes.extend_from_slice(&(stream_len as u32).to_be_bytes());
            bytes.extend_from_slice(&jpeg.data);
            bytes.resize(start + size, 0);
            icons.push(IconReport {
                id,
                width: jpeg.width,
                height: jpeg.height,
                offset,
                size,
                quality: encoded.quality,
            });
        }

        let length = u32::try_from(bytes.len() - CRC_START).map_err(|_| Error::FileTooLarge)?;
        bytes[8..12].copy_from_slice(&length.to_be_bytes());
        let crc = checksum(&bytes[CRC_START..]);
        bytes[6..8].copy_from_slice(&crc.to_le_bytes());
        Ok(BuiltIcl { bytes, icons })
    }
}

/// Rounds `n` up to a multiple of `align`; `align == 0` leaves it unchanged.
fn round_up(n: usize, align: u32) -> usize {
    match align as usize {
        0 => n,
        align => n.div_ceil(align) * align,
    }
}

/// Zero bytes to insert before a record of `size` bytes at file offset `pos`
/// so that it does not straddle a 16 MB boundary in flash.
fn bank_padding(slot: FlashSlot, pos: usize, size: usize) -> usize {
    let start = u64::from(slot.id) * u64::from(slot.size) + pos as u64;
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
        // Slot 63 starts 256 KB below the first 16 MB boundary.
        let slot = FlashSlot::new(63);
        assert_eq!(bank_padding(slot, 0, 1000), 0);
        assert_eq!(bank_padding(slot, 262_144 - 1000, 1000), 0);
        assert_eq!(bank_padding(slot, 262_144 - 999, 1000), 999);
        assert_eq!(bank_padding(FlashSlot::new(64), 0, 1 << 20), 0);
        assert_eq!(
            bank_padding(
                FlashSlot {
                    id: 31,
                    size: 512 * 1024
                },
                1,
                524_288
            ),
            524_287
        );
    }

    #[test]
    fn round_up_handles_zero_alignment() {
        assert_eq!(round_up(1, 0), 1);
        assert_eq!(round_up(0, 512), 0);
        assert_eq!(round_up(1, 512), 512);
        assert_eq!(round_up(1024, 512), 1024);
    }
}
