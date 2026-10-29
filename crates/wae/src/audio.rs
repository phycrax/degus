//! Decoding audio files into mono 16-bit clips at the target sample rate.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::error::{Error, Result};
use crate::options::Downmix;

/// Mono samples in `[-1, 1)` and their sample rate.
pub(crate) struct Decoded {
    pub samples: Vec<f32>,
    pub rate: u32,
}

/// Decodes the first audio track of an encoded file into mono.
/// `extension` is a hint for container detection.
pub(crate) fn decode(bytes: Vec<u8>, extension: Option<&str>, downmix: Downmix) -> Result<Decoded> {
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .first_track(TrackType::Audio)
        .ok_or(Error::NoAudioTrack)?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or(Error::NoAudioTrack)?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())?;

    let mut rate = params.sample_rate;
    let mut samples = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            // A corrupt packet only loses its own frames.
            Err(DecodeError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = buffer.spec();
        rate.get_or_insert(spec.rate());
        let channels = spec.channels().count().max(1);
        interleaved.clear();
        buffer.copy_to_vec_interleaved(&mut interleaved);
        let frames = interleaved.chunks_exact(channels);
        match downmix {
            Downmix::Left => samples.extend(frames.map(|frame| frame[0])),
            Downmix::Average => {
                samples.extend(frames.map(|frame| frame.iter().sum::<f32>() / channels as f32))
            }
        }
    }
    let rate = rate.ok_or(Error::UnknownSampleRate)?;
    Ok(Decoded { samples, rate })
}

/// Resamples mono audio from `from` Hz to `to` Hz, keeping
/// `len × to / from` samples rounded to the nearest, as DWIN's tools do.
pub(crate) fn resample(samples: Vec<f32>, from: u32, to: u32) -> Result<Vec<f32>> {
    if from == to || samples.is_empty() {
        return Ok(samples);
    }
    let resample_err = |e: &dyn std::fmt::Display| Error::Resample(e.to_string());
    let mut resampler = Fft::<f32>::new(from as usize, to as usize, 1024, 1, FixedSync::Both)
        .map_err(|e| resample_err(&e))?;
    let input = InterleavedSlice::new(&samples, 1, samples.len()).map_err(|e| resample_err(&e))?;
    let output = resampler
        .process_all(&input, samples.len(), None)
        .map_err(|e| resample_err(&e))?;
    // rubato rounds the length up.
    let (len, from, to) = (samples.len() as u64, u64::from(from), u64::from(to));
    let mut data = output.take_data();
    data.truncate(((len * to + from / 2) / from) as usize);
    Ok(data)
}

/// Converts mono samples at `rate` Hz to 16-bit.
/// - a linear fade-in over the first `rate / 200` samples (5 ms), with the
///   gain accumulated in `f32`;
/// - round-half-to-even conversion, the inverse of Symphonia's `s / 32768`;
/// - one trailing zero sample.
pub(crate) fn mix_to_i16(samples: &[f32], rate: u32) -> Vec<i16> {
    let ramp = (rate / 200) as usize;
    let step = 1.0 / ramp.max(1) as f32;
    let mut gain = 0.0_f32;
    let mut out = Vec::with_capacity(samples.len() + 1);
    for (i, &sample) in samples.iter().enumerate() {
        let sample = if i + 1 < ramp {
            gain += step;
            sample * gain
        } else {
            sample
        };
        out.push(
            (sample * 32768.0)
                .round_ties_even()
                .clamp(-32768.0, 32767.0) as i16,
        );
    }
    out.push(0);
    out
}

pub(crate) fn to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| f32::from(s) / 32768.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_fades_in_over_5ms_and_appends_a_zero() {
        let samples = vec![0.5_f32; 300];
        let mixed = mix_to_i16(&samples, 44_100);
        assert_eq!(mixed.len(), 301);
        // Gain (i + 1) / 220 for the first 219 samples, rounded half to even.
        assert_eq!(&mixed[..3], [74, 149, 223]);
        assert_eq!(mixed[218], 16310);
        assert!(mixed[219..300].iter().all(|&s| s == 16384));
        assert_eq!(mixed[300], 0);
    }

    #[test]
    fn resampled_length_rounds_to_nearest() {
        for (len, expected) in [(15_243, 11_061), (50_408, 36_577), (35_280, 25_600)] {
            let out = resample(vec![0.0; len], 44_100, 32_000).unwrap();
            assert_eq!(out.len(), expected, "{len} samples");
        }
    }
}
