use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use crc::{CRC_16_MODBUS, Crc};
use degus_wae::{ClipSource, Downmix, Error, SampleFormat, WaeBuilder, WaeOptions};

/// Fade-in length at 32 kHz (5 ms).
const RAMP: usize = 160;

fn sine(rate: u32, freq: f32, seconds: f32, amplitude: f32) -> Vec<i16> {
    (0..(rate as f32 * seconds) as usize)
        .map(|i| (amplitude * (TAU * freq * i as f32 / rate as f32).sin()) as i16)
        .collect()
}

fn samples(rate: u32, samples: &[i16]) -> ClipSource {
    ClipSource::Samples {
        rate,
        samples: samples.to_vec(),
    }
}

/// Writes a 16-bit WAV; `channels` holds one sample vector per channel.
fn write_wav(path: &Path, rate: u32, channels: &[Vec<i16>]) {
    let spec = hound::WavSpec {
        channels: channels.len() as u16,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for frame in 0..channels[0].len() {
        for channel in channels {
            writer.write_sample(channel[frame]).unwrap();
        }
    }
    writer.finalize().unwrap();
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn be32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// Checks the header, length and CRC of a built WAE.
fn check_header(bytes: &[u8], clips: u16, format: u8) {
    assert_eq!(bytes[..6], *b"DGUS_2");
    assert_eq!(bytes[12], 3, "version");
    assert_eq!(
        u16::from_be_bytes([bytes[13], bytes[14]]),
        clips,
        "clip count"
    );
    assert_eq!(bytes[15], format, "sample format");
    assert_eq!(be32(bytes, 8) as usize, bytes.len() - 8, "length");
    let crc = Crc::<u16>::new(&CRC_16_MODBUS).checksum(&bytes[8..]);
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), crc, "CRC");
}

/// Clip table entry of `id`: offset and length, both 0 without a clip.
fn entry(bytes: &[u8], id: u16) -> (usize, usize) {
    let at = 16 + 8 * usize::from(id);
    (be32(bytes, at) as usize, be32(bytes, at + 4) as usize)
}

/// The data of clip `id`.
fn clip(bytes: &[u8], id: u16) -> &[u8] {
    let (offset, len) = entry(bytes, id);
    assert_ne!(offset, 0, "no clip {id}");
    &bytes[offset..offset + len]
}

/// The samples of a 16-bit PCM clip.
fn samples_of(data: &[u8]) -> Vec<i16> {
    data.as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect()
}

/// The single clip of a one-clip 16-bit PCM build.
fn built_samples(builder: &WaeBuilder, id: u16) -> Vec<i16> {
    let bytes = builder.build().unwrap().bytes;
    check_header(&bytes, 1, 0);
    samples_of(clip(&bytes, id))
}

fn zero_crossings(samples: &[i16]) -> usize {
    samples
        .windows(2)
        .filter(|w| (w[0] < 0) != (w[1] < 0))
        .count()
}

fn assert_converted(clip: &[i16], input: &[i16]) {
    assert_eq!(clip.len(), input.len() + 1);
    assert_eq!(clip.last(), Some(&0));
    for (out, original) in clip.iter().zip(input).take(RAMP) {
        assert!(out.unsigned_abs() <= original.unsigned_abs());
    }
    if input.len() > RAMP {
        assert_eq!(clip[RAMP..input.len()], input[RAMP..]);
    }
}

#[test]
fn builds_block_aligned_wae_with_id_gaps() {
    let mut builder = WaeBuilder::new(WaeOptions::default());
    let clips = [
        (0, sine(32_000, 440.0, 0.1, 8000.0)),
        (2, vec![1, -1, 300]),
        (9, sine(32_000, 1000.0, 0.3, 20000.0)),
    ];
    for (id, input) in &clips {
        builder.insert(*id, samples(32_000, input));
    }
    let built = builder.build().unwrap();
    let bytes = &built.bytes;
    check_header(bytes, 3, 0);
    assert_eq!(bytes.len() % 4096, 0);

    assert_eq!(
        (0..10)
            .filter(|&id| entry(bytes, id).0 != 0)
            .collect::<Vec<_>>(),
        [0, 2, 9]
    );
    // The table fits in the first block, so the first clip starts at 4 KB.
    assert_eq!(entry(bytes, 0).0, 4096);
    for ((id, input), report) in clips.iter().zip(&built.clips) {
        let (offset, len) = entry(bytes, *id);
        assert_eq!(report.id, *id);
        assert_eq!(offset % 4096, 0);
        assert_eq!(offset, report.offset as usize);
        assert_eq!(len, report.len as usize);
        assert_eq!(len, (input.len() + 1) * 2);
        assert_converted(&samples_of(clip(bytes, *id)), input);
    }
}

#[test]
fn large_ids_push_data_to_the_next_block() {
    let mut builder = WaeBuilder::new(WaeOptions::default());
    builder.insert(600, samples(32_000, &[5; 10]));
    let bytes = builder.build().unwrap().bytes;
    // 16 + 8 * 601 bytes of table round up to 8 KB.
    assert_eq!(entry(&bytes, 600).0, 8192);
}

#[test]
fn companded_clips_use_one_byte_per_sample() {
    let input = sine(32_000, 300.0, 0.2, 30000.0);
    let mut builder = WaeBuilder::new(WaeOptions {
        format: SampleFormat::Companded8,
        ..WaeOptions::default()
    });
    builder.insert(1, samples(32_000, &input));
    let bytes = builder.build().unwrap().bytes;
    check_header(&bytes, 1, 1);
    let codes = clip(&bytes, 1);
    assert_eq!(codes.len(), input.len() + 1);

    // Each code is a sign bit and a magnitude that grows with the sample's.
    builder.options.format = SampleFormat::Pcm16;
    let pcm = built_samples(&builder, 1);
    let mut pairs: Vec<_> = pcm.iter().zip(codes).collect();
    for (sample, code) in &pairs {
        assert_eq!(*code & 0x80 != 0, **sample < 0, "sample {sample}");
    }
    pairs.sort_by_key(|(sample, _)| sample.unsigned_abs());
    assert!(pairs.windows(2).all(|w| w[0].1 & 0x7F <= w[1].1 & 0x7F));
    assert!(
        pairs.last().unwrap().1 & 0x7F > 0x70,
        "loud samples use high codes"
    );
}

#[test]
fn wav_at_target_rate_keeps_its_samples() {
    let dir = temp_dir("wav_at_target_rate_keeps_its_samples");
    let input = sine(32_000, 523.0, 0.25, 12345.0);
    let path = dir.join("tone.wav");
    write_wav(&path, 32_000, std::slice::from_ref(&input));

    let mut builder = WaeBuilder::new(WaeOptions::default());
    builder.insert(4, ClipSource::File(path));
    assert_converted(&built_samples(&builder, 4), &input);
}

#[test]
fn resamples_to_the_target_rate() {
    let dir = temp_dir("resamples_to_the_target_rate");
    let tone = sine(44_100, 1000.0, 1.0, 16000.0);
    let path = dir.join("tone.wav");
    write_wav(&path, 44_100, std::slice::from_ref(&tone));

    let mut builder = WaeBuilder::new(WaeOptions::default());
    builder.insert(1, ClipSource::File(path));
    let resampled = built_samples(&builder, 1);
    assert!(
        resampled.len().abs_diff(32_001) <= 320,
        "{} samples",
        resampled.len()
    );
    // A 1 kHz tone still crosses zero 2000 times per second.
    assert!(zero_crossings(&resampled).abs_diff(2000) <= 4);
    let peak = resampled.iter().map(|s| s.unsigned_abs()).max().unwrap();
    assert!((15_000..=17_000).contains(&peak), "peak {peak}");
}

#[test]
fn downmix_keeps_left_or_averages() {
    let dir = temp_dir("downmix_keeps_left_or_averages");
    let tone = sine(32_000, 1000.0, 0.1, 16000.0);
    let inverted: Vec<i16> = tone.iter().map(|s| -s).collect();
    let path = dir.join("opposed.wav");
    write_wav(&path, 32_000, &[tone.clone(), inverted]);

    let build = |downmix| {
        let mut builder = WaeBuilder::new(WaeOptions {
            downmix,
            ..WaeOptions::default()
        });
        builder.insert(1, ClipSource::File(path.clone()));
        built_samples(&builder, 1)
    };
    // DWIN's tools keep the left channel.
    assert_converted(&build(Downmix::Left), &tone);
    // Opposite channels average to silence.
    assert!(build(Downmix::Average).iter().all(|&s| s == 0));
}

#[test]
fn rejects_bad_input() {
    let mut builder = WaeBuilder::new(WaeOptions::default());
    assert!(matches!(builder.build(), Err(Error::Empty)));
    builder.insert(0, samples(32_000, &[]));
    assert!(matches!(builder.build(), Err(Error::Clip { id: 0, .. })));
    builder.insert(0, ClipSource::Encoded(b"not audio".to_vec()));
    assert!(matches!(builder.build(), Err(Error::Clip { id: 0, .. })));
}
