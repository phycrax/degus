use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::audio;
use crate::error::{Error, Result};
use crate::format::{
    CRC_START, ENTRY_LEN, HEADER_LEN, MAGIC, SAMPLE_RATE, VERSION, checksum, round_up_block,
};
use crate::options::WaeOptions;

/// Where a clip's audio comes from.
#[derive(Debug, Clone)]
pub enum ClipSource {
    /// An audio file (WAV, MP3, FLAC or Ogg Vorbis).
    File(PathBuf),
    /// Encoded audio file bytes (WAV, MP3, FLAC or Ogg Vorbis).
    Encoded(Vec<u8>),
    /// Mono 16-bit samples at `rate` Hz.
    Samples { rate: u32, samples: Vec<i16> },
    /// Clip data already in the builder's sample format, stored unchanged.
    Raw(Vec<u8>),
}

/// Per-clip details of a built WAE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipReport {
    pub id: u16,
    /// Absolute offset of the clip data in the file.
    pub offset: u32,
    /// Clip data length in bytes, excluding block padding.
    pub len: u32,
    pub samples: usize,
}

/// A serialized WAE file.
#[derive(Debug, Clone)]
pub struct BuiltWae {
    pub bytes: Vec<u8>,
    /// One entry per clip, in ID order.
    pub clips: Vec<ClipReport>,
}

/// Collects audio clips by ID and serializes them into a WAE file.
#[derive(Debug, Clone, Default)]
pub struct WaeBuilder {
    pub options: WaeOptions,
    clips: BTreeMap<u16, ClipSource>,
}

impl WaeBuilder {
    pub fn new(options: WaeOptions) -> Self {
        Self {
            options,
            clips: BTreeMap::new(),
        }
    }

    /// Sets the clip for `id`, returning the source it replaced.
    pub fn insert(&mut self, id: u16, source: ClipSource) -> Option<ClipSource> {
        self.clips.insert(id, source)
    }

    pub fn remove(&mut self, id: u16) -> Option<ClipSource> {
        self.clips.remove(&id)
    }

    pub fn contains(&self, id: u16) -> bool {
        self.clips.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Clip IDs in ascending order.
    pub fn ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.clips.keys().copied()
    }

    /// Converts all clips and serializes the WAE file.
    pub fn build(&self) -> Result<BuiltWae> {
        let max_id = *self.clips.keys().next_back().ok_or(Error::Empty)?;
        let count = u16::try_from(self.clips.len()).map_err(|_| Error::TooManyClips)?;
        let data_start = round_up_block(HEADER_LEN + ENTRY_LEN * (usize::from(max_id) + 1));
        let mut bytes = vec![0; data_start];
        bytes[..6].copy_from_slice(&MAGIC);
        bytes[12] = VERSION;
        bytes[13..15].copy_from_slice(&count.to_be_bytes());
        bytes[15] = self.options.format.code();

        let mut clips = Vec::with_capacity(self.clips.len());
        for (&id, source) in &self.clips {
            let data = self.encode(source).map_err(|e| Error::Clip {
                id,
                source: Box::new(e),
            })?;
            let offset = u32::try_from(bytes.len()).map_err(|_| Error::FileTooLarge)?;
            let len = u32::try_from(data.len()).map_err(|_| Error::FileTooLarge)?;
            let entry = HEADER_LEN + ENTRY_LEN * usize::from(id);
            bytes[entry..entry + 4].copy_from_slice(&offset.to_be_bytes());
            bytes[entry + 4..entry + 8].copy_from_slice(&len.to_be_bytes());
            bytes.extend_from_slice(&data);
            bytes.resize(round_up_block(bytes.len()), 0);
            clips.push(ClipReport {
                id,
                offset,
                len,
                samples: data.len() / self.options.format.bytes_per_sample(),
            });
        }

        let length = u32::try_from(bytes.len() - CRC_START).map_err(|_| Error::FileTooLarge)?;
        bytes[8..12].copy_from_slice(&length.to_be_bytes());
        let crc = checksum(&bytes[CRC_START..]);
        bytes[6..8].copy_from_slice(&crc.to_le_bytes());
        Ok(BuiltWae { bytes, clips })
    }

    /// Converts one clip into the builder's sample format.
    fn encode(&self, source: &ClipSource) -> Result<Vec<u8>> {
        let downmix = self.options.downmix;
        let decoded = match source {
            ClipSource::Raw(data) => {
                if data.is_empty() {
                    return Err(Error::EmptyAudio);
                }
                return Ok(data.clone());
            }
            ClipSource::Samples { rate, samples } => audio::Decoded {
                samples: audio::to_f32(samples),
                rate: *rate,
            },
            ClipSource::File(path) => {
                let bytes = std::fs::read(path).map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
                let extension = path.extension().and_then(|ext| ext.to_str());
                audio::decode(bytes, extension, downmix)?
            }
            ClipSource::Encoded(bytes) => audio::decode(bytes.clone(), None, downmix)?,
        };
        if decoded.samples.is_empty() {
            return Err(Error::EmptyAudio);
        }
        let samples = audio::resample(decoded.samples, decoded.rate, SAMPLE_RATE)?;
        Ok(self
            .options
            .format
            .encode(&audio::mix_to_i16(&samples, SAMPLE_RATE)))
    }
}
