//! WAE libraries as data, and their file layout.

use std::collections::BTreeMap;

use crate::codec::SampleFormat;
use crate::error::{Error, Result};
use crate::format::{CRC_START, ENTRY_LEN, HEADER_LEN, MAGIC, VERSION, checksum, round_up_block};

/// An audio library: mono clips at [`SAMPLE_RATE`](crate::SAMPLE_RATE), by
/// ID.
///
/// [`WaeBuilder`](crate::WaeBuilder) makes one from audio files,
/// [`Wae::to_bytes`] writes it as a file and [`Wae::from_bytes`] reads one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wae {
    /// How the file stores the samples. [`SampleFormat::Companded8`] rounds
    /// each sample to the middle of its code's range.
    pub format: SampleFormat,
    /// Clips by ID: mono 16-bit samples at 32 kHz.
    pub clips: BTreeMap<u16, Vec<i16>>,
}

impl Wae {
    pub fn new(format: SampleFormat) -> Self {
        Self {
            format,
            clips: BTreeMap::new(),
        }
    }

    /// Reads a WAE file.
    pub fn from_bytes(bytes: &[u8]) -> Result<Wae> {
        if bytes.len() < HEADER_LEN || bytes[..6] != MAGIC {
            return Err(Error::NotWae);
        }
        if bytes[12] != VERSION {
            return Err(Error::Version(bytes[12]));
        }
        if be32(bytes, 8) as usize != bytes.len() - CRC_START {
            return Err(Error::Damaged("its length field does not match its length"));
        }
        if u16::from_le_bytes([bytes[6], bytes[7]]) != checksum(&bytes[CRC_START..]) {
            return Err(Error::Damaged("its CRC does not match"));
        }
        let count = usize::from(u16::from_be_bytes([bytes[13], bytes[14]]));
        let format = SampleFormat::from_code(bytes[15]).ok_or(Error::Format(bytes[15]))?;

        // The table runs from the header to the first clip, which no entry
        // seen so far may start inside.
        let mut wae = Wae::new(format);
        let mut table_end = bytes.len();
        let mut id = 0;
        while HEADER_LEN + ENTRY_LEN * (id + 1) <= table_end {
            let at = HEADER_LEN + ENTRY_LEN * id;
            let (offset, len) = (be32(bytes, at) as usize, be32(bytes, at + 4) as usize);
            if offset != 0 {
                if offset < at + ENTRY_LEN {
                    return Err(Error::Damaged("a clip starts inside the table"));
                }
                table_end = table_end.min(offset);
                let data = bytes
                    .get(offset..offset + len)
                    .ok_or(Error::Damaged("a clip runs past its end"))?;
                let clip = u16::try_from(id).map_err(|_| Error::TooManyClips)?;
                wae.clips.insert(clip, format.decode(data));
            }
            id += 1;
        }
        if wae.clips.len() != count {
            return Err(Error::Damaged("its clip count does not match its table"));
        }
        Ok(wae)
    }

    /// Writes the WAE file.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let max_id = *self.clips.keys().next_back().ok_or(Error::Empty)?;
        let count = u16::try_from(self.clips.len()).map_err(|_| Error::TooManyClips)?;
        let data_start = round_up_block(HEADER_LEN + ENTRY_LEN * (usize::from(max_id) + 1));
        let mut bytes = vec![0; data_start];
        bytes[..6].copy_from_slice(&MAGIC);
        bytes[12] = VERSION;
        bytes[13..15].copy_from_slice(&count.to_be_bytes());
        bytes[15] = self.format.code();

        for (&id, samples) in &self.clips {
            if samples.is_empty() {
                return Err(Error::Clip {
                    id,
                    source: Box::new(Error::EmptyAudio),
                });
            }
            let data = self.format.encode(samples);
            let offset = u32::try_from(bytes.len()).map_err(|_| Error::FileTooLarge)?;
            let len = u32::try_from(data.len()).map_err(|_| Error::FileTooLarge)?;
            let entry = HEADER_LEN + ENTRY_LEN * usize::from(id);
            bytes[entry..entry + 4].copy_from_slice(&offset.to_be_bytes());
            bytes[entry + 4..entry + 8].copy_from_slice(&len.to_be_bytes());
            bytes.extend_from_slice(&data);
            bytes.resize(round_up_block(bytes.len()), 0);
        }

        let length = u32::try_from(bytes.len() - CRC_START).map_err(|_| Error::FileTooLarge)?;
        bytes[8..12].copy_from_slice(&length.to_be_bytes());
        let crc = checksum(&bytes[CRC_START..]);
        bytes[6..8].copy_from_slice(&crc.to_le_bytes());
        Ok(bytes)
    }
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
