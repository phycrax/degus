use std::collections::BTreeMap;

use crate::audio;
use crate::error::{Error, Result};
use crate::format::SAMPLE_RATE;
use crate::options::{Downmix, WaeOptions};
use crate::wae::Wae;

/// Where a clip's audio comes from.
#[derive(Debug, Clone)]
pub enum ClipSource {
    /// Encoded audio file bytes (WAV, MP3, FLAC or Ogg Vorbis).
    Encoded(Vec<u8>),
    /// Mono 16-bit samples at `rate` Hz.
    Samples { rate: u32, samples: Vec<i16> },
    /// Clip data already in the builder's sample format.
    Raw(Vec<u8>),
}

/// Collects audio clips by ID and converts them into a [`Wae`].
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

    /// Decodes, downmixes and resamples every clip: the slow part of making
    /// a WAE. Each clip holds the samples the display plays, so a
    /// [`SampleFormat::Companded8`] clip already carries its loss.
    ///
    /// [`SampleFormat::Companded8`]: crate::SampleFormat::Companded8
    pub fn build(&self) -> Result<Wae> {
        let mut wae = Wae::new(self.options.format);
        for (&id, source) in &self.clips {
            let samples = self.stored_samples(source).map_err(|e| Error::Clip {
                id,
                source: Box::new(e),
            })?;
            wae.clips.insert(id, samples);
        }
        Ok(wae)
    }

    /// The samples of `source` after a trip through the sample format.
    fn stored_samples(&self, source: &ClipSource) -> Result<Vec<i16>> {
        let format = self.options.format;
        let stored = match source {
            ClipSource::Raw(data) if data.is_empty() => return Err(Error::EmptyAudio),
            ClipSource::Raw(data) => data.clone(),
            _ => format.encode(&convert(source, self.options.downmix)?),
        };
        Ok(format.decode(&stored))
    }
}

/// Decodes, downmixes and resamples a clip that is not [`ClipSource::Raw`].
fn convert(source: &ClipSource, downmix: Downmix) -> Result<Vec<i16>> {
    let decoded = match source {
        ClipSource::Raw(_) => unreachable!("raw clips are not converted"),
        ClipSource::Samples { rate, samples } => audio::Decoded {
            samples: audio::to_f32(samples),
            rate: *rate,
        },
        ClipSource::Encoded(bytes) => audio::decode(bytes.clone(), downmix)?,
    };
    if decoded.samples.is_empty() {
        return Err(Error::EmptyAudio);
    }
    let samples = audio::resample(decoded.samples, decoded.rate, SAMPLE_RATE)?;
    Ok(audio::mix_to_i16(&samples, SAMPLE_RATE))
}
