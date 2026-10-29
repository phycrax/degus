use std::path::PathBuf;

use crc::{CRC_16_MODBUS, Crc};
use degus_icl::{
    Chip, Error, FlashSlot, IclBuilder, IclOptions, IconSource, OversizePolicy, Subsampling,
};
use image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage};

fn gradient(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, 128])
    }))
}

/// Deterministic noise, which compresses badly and produces large JPEGs.
fn noise(width: u32, height: u32, seed: u32) -> DynamicImage {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [r, g, b, _] = state.to_le_bytes();
        Rgb([r, g, b])
    }))
}

fn mean_abs_diff(a: &RgbImage, b: &RgbImage) -> f64 {
    let total: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.len() as f64
}

fn be16(data: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([data[at], data[at + 1]])
}

fn be32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// An icon record of a built ICL.
struct Record<'a> {
    id: u16,
    width: u16,
    height: u16,
    offset: usize,
    header_len: usize,
    /// The JPEG in ICL layout, padding included.
    payload: &'a [u8],
}

impl Record<'_> {
    /// The JPEG with its padded SOS segment shrunk back, as decoders
    /// require.
    fn jpeg(&self) -> Vec<u8> {
        let (header, stream) = self.payload.split_at(self.header_len);
        let sos = header.windows(2).rposition(|w| w == [0xFF, 0xDA]).unwrap();
        let used = 6 + 2 * usize::from(header[sos + 4]);
        let end = stream.iter().rposition(|&b| b != 0).unwrap() + 1;
        let mut jpeg = header[..sos + 2].to_vec();
        jpeg.extend_from_slice(&(used as u16).to_be_bytes());
        jpeg.extend_from_slice(&header[sos + 4..sos + 2 + used]);
        jpeg.extend_from_slice(&stream[..end]);
        jpeg
    }

    fn decode(&self) -> RgbImage {
        image::load_from_memory(&self.jpeg()).unwrap().to_rgb8()
    }
}

/// The highest icon ID and the records of a built ICL, after checking its
/// header, length and CRC.
fn records(bytes: &[u8]) -> (u16, Vec<Record<'_>>) {
    assert_eq!(bytes[..6], *b"DGUS_3");
    assert_eq!(bytes[12], 4, "version");
    assert_eq!(bytes[15], 0, "crypto flag");
    assert_eq!(be32(bytes, 8) as usize, bytes.len() - 8, "length");
    let crc = Crc::<u16>::new(&CRC_16_MODBUS).checksum(&bytes[8..]);
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), crc, "CRC");

    let max_id = be16(bytes, 13);
    let records = (0..=max_id)
        .filter_map(|id| {
            let offset = be32(bytes, 16 + 4 * usize::from(id)) as usize;
            if offset == 0 {
                return None;
            }
            let r = &bytes[offset..];
            let header_len = usize::from(be16(r, 4));
            Some(Record {
                id,
                width: be16(r, 0),
                height: be16(r, 2),
                offset,
                header_len,
                payload: &r[10..10 + header_len + be32(r, 6) as usize],
            })
        })
        .collect();
    (max_id, records)
}

/// The record of icon `id`.
fn record(bytes: &[u8], id: u16) -> Record<'_> {
    records(bytes)
        .1
        .into_iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no icon {id}"))
}

#[test]
fn builds_icl_with_id_gaps() {
    for subsampling in [Subsampling::S444, Subsampling::S420] {
        let mut builder = IclBuilder::new(IclOptions {
            quality: 90,
            subsampling,
            ..IclOptions::default()
        });
        let sources = [
            (0, gradient(64, 48)),
            (3, gradient(17, 9)),
            (7, gradient(1, 1)),
        ];
        for (id, image) in &sources {
            builder.insert(*id, IconSource::Image(image.clone()));
        }
        let built = builder.build().unwrap();
        let (max_id, icons) = records(&built.bytes);

        assert_eq!(max_id, 7);
        assert_eq!(icons.iter().map(|i| i.id).collect::<Vec<_>>(), [0, 3, 7]);
        assert_eq!(built.icons.len(), 3);
        for ((id, image), (icon, report)) in sources.iter().zip(icons.iter().zip(&built.icons)) {
            assert_eq!(icon.id, *id);
            assert_eq!(
                (icon.width, icon.height),
                (image.width() as u16, image.height() as u16)
            );
            assert_eq!(icon.offset, report.offset as usize);
            assert_eq!(report.quality, Some(90));
            // Records and their scan data are 4-byte aligned.
            assert_eq!(icon.offset % 4, 0);
            assert_eq!((icon.offset + 10 + icon.header_len) % 4, 0);
            assert!(mean_abs_diff(&icon.decode(), &image.to_rgb8()) < 6.0);
        }
        // The first record follows the 16-byte header and 8 table entries.
        assert_eq!(icons[0].offset, 16 + 4 * 8);
    }
}

#[test]
fn blends_transparency_onto_background() {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0])));
    let mut builder = IclBuilder::new(IclOptions {
        background: [0, 0, 255],
        ..IclOptions::default()
    });
    builder.insert(1, IconSource::Image(image));
    let bytes = builder.build().unwrap().bytes;
    let pixel = *record(&bytes, 1).decode().get_pixel(8, 8);
    assert!(pixel[0] < 8 && pixel[1] < 8 && pixel[2] > 247, "{pixel:?}");
}

#[test]
fn passthrough_keeps_compatible_jpegs() {
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 60)
        .encode(
            &gradient(40, 30).to_rgb8(),
            40,
            30,
            jpeg_encoder::ColorType::Rgb,
        )
        .unwrap();

    let build = |jpeg: &[u8], passthrough_jpeg| {
        let mut builder = IclBuilder::new(IclOptions {
            passthrough_jpeg,
            ..IclOptions::default()
        });
        builder.insert(2, IconSource::Encoded(jpeg.to_vec()));
        let built = builder.build().unwrap();
        (record(&built.bytes, 2).jpeg(), built.icons[0].quality)
    };
    // Passthrough only rewrites the header into libjpeg's layout...
    let (stored, quality) = build(&jpeg, true);
    assert_eq!(quality, None);
    assert_eq!(
        image::load_from_memory(&stored).unwrap().to_rgb8(),
        image::load_from_memory(&jpeg).unwrap().to_rgb8()
    );
    // ...so a JPEG already in that layout is stored unchanged.
    assert_eq!(build(&stored, true).0, stored);
    let (encoded, quality) = build(&stored, false);
    assert_ne!(encoded, stored);
    assert_eq!(quality, Some(IclOptions::default().quality));
}

#[test]
fn oversized_icons_fail_or_reduce_quality() {
    let options = IclOptions {
        quality: 100,
        subsampling: Subsampling::S444,
        chip: Chip::T5L1,
        ..IclOptions::default()
    };
    let is_oversize = |result: degus_icl::Result<_>| match result {
        Err(Error::Icon { id: 0, source }) => matches!(*source, Error::IconTooLarge { .. }),
        _ => false,
    };
    let mut builder = IclBuilder::new(options.clone());
    builder.insert(0, IconSource::Image(noise(400, 300, 1)));
    assert!(is_oversize(builder.build()));

    // Retries in steps of 1 until the icon fits.
    builder.options.oversize = OversizePolicy::reduce_quality();
    let built = builder.build().unwrap();
    let quality = built.icons[0].quality.unwrap();
    assert!(
        quality < 99,
        "expected several retries, got quality {quality}"
    );
    assert!(built.icons[0].size <= 10 + Chip::T5L1.max_jpeg_size() + 11);

    // One step higher would not have fitted.
    builder.options.oversize = OversizePolicy::Error;
    builder.options.quality = quality + 1;
    assert!(is_oversize(builder.build()));
}

#[test]
fn head_and_body_alignment() {
    let mut builder = IclBuilder::new(IclOptions {
        head_align: 64 * 1024,
        body_align: 512,
        ..IclOptions::default()
    });
    builder.insert(0, IconSource::Image(gradient(30, 20)));
    builder.insert(5, IconSource::Image(gradient(50, 10)));
    let built = builder.build().unwrap();
    let (_, icons) = records(&built.bytes);
    assert_eq!(icons[0].offset, 64 * 1024);
    for report in &built.icons {
        assert_eq!(report.size % 512, 0);
    }
    assert_eq!(icons[1].offset, 64 * 1024 + built.icons[0].size);
    assert_eq!(
        built.bytes.len(),
        64 * 1024 + built.icons.iter().map(|r| r.size).sum::<usize>()
    );
    icons[1].decode();
}

#[test]
fn flash_slot_keeps_records_within_16mb_banks() {
    // Slot 63 sits 256 KB below the first 16 MB boundary.
    let slot = FlashSlot::new(63);
    let mut builder = IclBuilder::new(IclOptions {
        quality: 100,
        subsampling: Subsampling::S444,
        chip: Chip::T5L2,
        flash_slot: Some(slot),
        ..IclOptions::default()
    });
    for id in 0..4 {
        builder.insert(id, IconSource::Image(noise(160, 120, u32::from(id))));
    }
    let built = builder.build().unwrap();
    let base = u64::from(slot.id) * u64::from(slot.size);
    let bank = 16 << 20;
    let mut padded = false;
    for report in &built.icons {
        let start = base + u64::from(report.offset);
        let last = start + report.size as u64 - 1;
        assert_eq!(
            start / bank,
            last / bank,
            "icon {} straddles a bank",
            report.id
        );
        padded |= start % bank == 0;
    }
    assert!(
        padded,
        "test images should force one record onto the boundary"
    );
    for icon in records(&built.bytes).1 {
        icon.decode();
    }
}

#[test]
fn file_sources_are_read_from_disk() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("file_sources_are_read_from_disk");
    std::fs::create_dir_all(&dir).unwrap();
    gradient(12, 8).save(dir.join("logo.png")).unwrap();

    let mut builder = IclBuilder::new(IclOptions::default());
    builder.insert(5, IconSource::File(dir.join("logo.png")));
    let bytes = builder.build().unwrap().bytes;
    let icon = record(&bytes, 5);
    assert_eq!((icon.width, icon.height), (12, 8));

    builder.insert(6, IconSource::File(dir.join("missing.png")));
    assert!(matches!(
        builder.build(),
        Err(Error::Icon { id: 6, source }) if matches!(*source, Error::Io { .. })
    ));
}

#[test]
fn rejects_an_empty_builder() {
    assert!(matches!(IclBuilder::default().build(), Err(Error::Empty)));
}
