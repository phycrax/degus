use std::path::PathBuf;

use crc::{CRC_16_MODBUS, Crc};
use degus_icl::{
    Chip, Error, Icl, IclBuilder, IclOptions, IconSource, OversizePolicy, Subsampling,
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
        let icl = builder.build().unwrap();
        let bytes = icl.to_bytes(32).unwrap();
        let (max_id, records) = records(&bytes);

        assert_eq!(max_id, 7);
        assert_eq!(icl.icons.keys().copied().collect::<Vec<_>>(), [0, 3, 7]);
        assert_eq!(records.iter().map(|r| r.id).collect::<Vec<_>>(), [0, 3, 7]);
        for ((id, image), record) in sources.iter().zip(&records) {
            let icon = &icl.icons[id];
            let size = (image.width() as u16, image.height() as u16);
            assert_eq!((icon.width(), icon.height()), size);
            assert_eq!((record.width, record.height), size);
            assert_eq!(icon.jpeg(), record.jpeg());
            // Records and their scan data are 4-byte aligned.
            assert_eq!(record.offset % 4, 0);
            assert_eq!((record.offset + 10 + record.header_len) % 4, 0);
            assert!(mean_abs_diff(&record.decode(), &image.to_rgb8()) < 6.0);
        }
        // The first record follows the 16-byte header and 8 table entries.
        assert_eq!(records[0].offset, 16 + 4 * 8);
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
    let bytes = builder.build().unwrap().to_bytes(32).unwrap();
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
        builder.build().unwrap().icons[&2].jpeg()
    };
    // Passthrough only rewrites the header into libjpeg's layout...
    let stored = build(&jpeg, true);
    assert_eq!(
        image::load_from_memory(&stored).unwrap().to_rgb8(),
        image::load_from_memory(&jpeg).unwrap().to_rgb8()
    );
    // ...so a JPEG already in that layout is stored unchanged.
    assert_eq!(build(&stored, true), stored);
    assert_ne!(build(&stored, false), stored);
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

    builder.options.oversize = OversizePolicy::reduce_quality();
    let icl = builder.build().unwrap();
    assert!(icl.icons[&0].jpeg().len() <= Chip::T5L1.max_jpeg_size());
}

#[test]
fn records_stay_within_16mb_flash_banks() {
    let mut builder = IclBuilder::new(IclOptions {
        quality: 100,
        subsampling: Subsampling::S444,
        chip: Chip::T5L2,
        ..IclOptions::default()
    });
    for id in 0..4 {
        builder.insert(id, IconSource::Image(noise(160, 120, u32::from(id))));
    }
    let icl = builder.build().unwrap();
    // File 63 sits 256 KB below the first 16 MB boundary.
    let bytes = icl.to_bytes(63).unwrap();
    let base = 63 * 256 * 1024;
    let bank = 16 << 20;
    let mut padded = false;
    for record in records(&bytes).1 {
        let start = base + record.offset;
        let last = start + 10 + record.payload.len() - 1;
        assert_eq!(
            start / bank,
            last / bank,
            "icon {} straddles a bank",
            record.id
        );
        padded |= start % bank == 0;
        record.decode();
    }
    assert!(
        padded,
        "test images should force one record onto the boundary"
    );
    // Elsewhere, the same icons need no padding.
    let unpadded = icl.to_bytes(32).unwrap();
    assert!(unpadded.len() < bytes.len());
}

#[test]
fn file_sources_are_read_from_disk() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("file_sources_are_read_from_disk");
    std::fs::create_dir_all(&dir).unwrap();
    gradient(12, 8).save(dir.join("logo.png")).unwrap();

    let mut builder = IclBuilder::new(IclOptions::default());
    builder.insert(5, IconSource::File(dir.join("logo.png")));
    let icl = builder.build().unwrap();
    assert_eq!((icl.icons[&5].width(), icl.icons[&5].height()), (12, 8));

    builder.insert(6, IconSource::File(dir.join("missing.png")));
    assert!(matches!(
        builder.build(),
        Err(Error::Icon { id: 6, source }) if matches!(*source, Error::Io { .. })
    ));
}

#[test]
fn an_empty_icl_cannot_be_written() {
    let icl = IclBuilder::default().build().unwrap();
    assert_eq!(icl, Icl::new());
    assert!(matches!(icl.to_bytes(32), Err(Error::Empty)));
}

#[test]
fn reads_what_it_writes() {
    let mut builder = IclBuilder::new(IclOptions {
        quality: 100,
        subsampling: Subsampling::S444,
        ..IclOptions::default()
    });
    builder.insert(0, IconSource::Image(gradient(64, 48)));
    builder.insert(9, IconSource::Image(gradient(3, 200)));
    for id in 20..24 {
        builder.insert(id, IconSource::Image(noise(160, 120, u32::from(id))));
    }
    let icl = builder.build().unwrap();
    // File 63's records are moved to the 16 MB boundary; reading skips the
    // gap.
    for file in [32, 63] {
        let bytes = icl.to_bytes(file).unwrap();
        assert_eq!(Icl::from_bytes(&bytes).unwrap(), icl);
    }
}

#[test]
fn decodes_icons() {
    let image = gradient(64, 48);
    let mut builder = IclBuilder::new(IclOptions::default());
    builder.insert(4, IconSource::Image(image.clone()));
    let bytes = builder.build().unwrap().to_bytes(32).unwrap();
    let icon = &Icl::from_bytes(&bytes).unwrap().icons[&4];
    let decoded = icon.decode().unwrap();
    assert_eq!(decoded.dimensions(), (64, 48));
    assert!(mean_abs_diff(&decoded, &image.to_rgb8()) < 6.0);
    assert_eq!(decoded, record(&bytes, 4).decode());
}

#[test]
fn rejects_what_it_cannot_read() {
    let mut builder = IclBuilder::new(IclOptions::default());
    builder.insert(1, IconSource::Image(gradient(8, 8)));
    let bytes = builder.build().unwrap().to_bytes(32).unwrap();
    let changed = |at: usize, value: u8| {
        let mut bytes = bytes.clone();
        bytes[at] = value;
        Icl::from_bytes(&bytes)
    };

    assert!(matches!(Icl::from_bytes(b"DGUS_2"), Err(Error::NotIcl)));
    assert!(matches!(changed(5, b'2'), Err(Error::NotIcl)));
    assert!(matches!(changed(12, 3), Err(Error::Version(3))));
    assert!(matches!(changed(15, 1), Err(Error::Encrypted(1))));
    // A changed byte no longer matches the CRC.
    assert!(matches!(
        changed(bytes.len() - 20, 0x55),
        Err(Error::Damaged(_))
    ));
    assert!(matches!(
        Icl::from_bytes(&bytes[..bytes.len() - 4]),
        Err(Error::Damaged(_))
    ));
}
