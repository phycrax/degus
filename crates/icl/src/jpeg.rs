//! Converting baseline JPEGs into the ICL record payload layout.
//!
//! The T5L decoder expects the entropy-coded scan data to start on a 4-byte
//! boundary in the file. Records are 4-byte aligned and begin with a 10-byte
//! header, so the SOS segment is zero-padded (and its length field grown)
//! until `10 + header_len` is a multiple of 4. The payload is then
//! zero-padded so the record length stays a multiple of 4, followed by
//! 8 more zero bytes.
//!
//! The header is also brought into the layout libjpeg writes, which is what
//! ICLs from DWIN's tools use: quantization tables before the frame header,
//! and components numbered 1, 2, 3. Other encoders differ (jpeg-encoder writes
//! the frame header first and numbers components from 0); PC decoders accept
//! either, but the T5L is only known to display libjpeg's layout.

use crate::error::{Error, JpegError};
use crate::format::RECORD_HEADER_LEN;

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const SOF0: u8 = 0xC0;
const DHT: u8 = 0xC4;
const JPG: u8 = 0xC8;
const DAC: u8 = 0xCC;
const DQT: u8 = 0xDB;
const APP0: u8 = 0xE0;
const APP14: u8 = 0xEE;
const APP15: u8 = 0xEF;
const COM: u8 = 0xFE;
const TRAILING_ZEROS: usize = 8;

/// A JPEG in ICL record layout.
#[derive(Debug, Clone)]
pub(crate) struct PreparedJpeg {
    pub width: u16,
    pub height: u16,
    /// Length of everything up to and including the padded SOS segment.
    pub header_len: u16,
    pub data: Vec<u8>,
}

/// A marker segment preceding the scan data.
struct Segment {
    marker: u8,
    /// Offset of the 0xFF that introduces the marker.
    start: usize,
    /// Offset just past the segment.
    end: usize,
}

impl Segment {
    fn body<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        &data[(self.start + 4).min(self.end)..self.end]
    }
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, JpegError> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or(JpegError::Truncated)
}

fn is_frame(marker: u8) -> bool {
    (0xC0..=0xCF).contains(&marker) && !matches!(marker, DHT | JPG | DAC)
}

/// Splits the bytes before the scan data into segments, the last being SOS.
fn header_segments(data: &[u8]) -> Result<Vec<Segment>, JpegError> {
    if !data.starts_with(&[0xFF, SOI]) {
        return Err(JpegError::NotJpeg);
    }
    let mut segments = Vec::new();
    let mut pos = 2;
    loop {
        match data.get(pos) {
            None => return Err(JpegError::MissingScan),
            Some(0xFF) => {}
            Some(_) => return Err(JpegError::BadMarker(pos)),
        }
        // A marker may be preceded by any number of 0xFF fill bytes.
        while data.get(pos + 1) == Some(&0xFF) {
            pos += 1;
        }
        let marker = *data.get(pos + 1).ok_or(JpegError::Truncated)?;
        let end = match marker {
            EOI => return Err(JpegError::MissingScan),
            0x00 | SOI => return Err(JpegError::BadMarker(pos)),
            0x01 | 0xD0..=0xD7 => pos + 2,
            _ => {
                let len = usize::from(read_u16(data, pos + 2)?);
                if len < 2 || pos + 2 + len > data.len() {
                    return Err(JpegError::Truncated);
                }
                pos + 2 + len
            }
        };
        segments.push(Segment {
            marker,
            start: pos,
            end,
        });
        if marker == SOS {
            return Ok(segments);
        }
        pos = end;
    }
}

/// Validates an SOF0 segment and returns `(width, height)`.
fn frame_size(frame: &Segment, data: &[u8]) -> Result<(u16, u16), JpegError> {
    let body = frame.body(data);
    let [precision, h0, h1, w0, w1, count, components @ ..] = body else {
        return Err(JpegError::Truncated);
    };
    if *precision != 8 {
        return Err(JpegError::Precision(*precision));
    }
    if *count != 3 {
        return Err(JpegError::Components(*count));
    }
    let [_, luma, _, _, cb, _, _, cr, _, ..] = components else {
        return Err(JpegError::Truncated);
    };
    if !matches!(luma, 0x11 | 0x22) || *cb != 0x11 || *cr != 0x11 {
        return Err(JpegError::Sampling);
    }
    let (width, height) = (
        u16::from_be_bytes([*w0, *w1]),
        u16::from_be_bytes([*h0, *h1]),
    );
    if width == 0 || height == 0 {
        return Err(JpegError::ZeroSize);
    }
    Ok((width, height))
}

/// Checks that `sos` starts the only scan, interleaving all three components,
/// and returns the offset just past EOI.
fn scan_end(data: &[u8], sos: &Segment) -> Result<usize, JpegError> {
    if sos.body(data).first() != Some(&3) {
        return Err(JpegError::MultiScan);
    }
    let mut pos = sos.end;
    loop {
        let ff = data[pos..]
            .iter()
            .position(|&b| b == 0xFF)
            .map(|i| pos + i)
            .ok_or(JpegError::Truncated)?;
        match data.get(ff + 1) {
            None => return Err(JpegError::Truncated),
            // Stuffed byte, restart marker or fill byte: still inside the scan.
            Some(0x00 | 0xD0..=0xD7 | 0xFF) => pos = ff + 1,
            Some(&EOI) => return Ok(ff + 2),
            Some(_) => return Err(JpegError::MultiScan),
        }
    }
}

/// Converts a baseline JPEG into ICL record layout. Comments and metadata
/// segments are dropped; `limit` applies to the result before padding.
pub(crate) fn prepare(jpeg: &[u8], limit: usize) -> Result<PreparedJpeg, Error> {
    let segments = header_segments(jpeg)?;
    let (sos, tables) = segments
        .split_last()
        .expect("header_segments ends with SOS");
    let mut size = None;
    for segment in tables {
        if is_frame(segment.marker) {
            if segment.marker != SOF0 {
                return Err(JpegError::UnsupportedFrame(segment.marker - SOF0).into());
            }
            size = Some(frame_size(segment, jpeg)?);
        } else if segment.marker == APP14 && segment.body(jpeg).starts_with(b"Adobe") {
            return Err(JpegError::Adobe.into());
        }
    }
    let (width, height) = size.ok_or(JpegError::MissingFrame)?;
    let end = scan_end(jpeg, sos)?;

    let mut kept: Vec<&Segment> = tables
        .iter()
        .filter(|s| s.marker != COM && !(APP0 < s.marker && s.marker <= APP15))
        .collect();
    // libjpeg's order: APP0, DQT, SOF0, then DHT and DRI as they come.
    kept.sort_by_key(|s| match s.marker {
        APP0 => 0,
        DQT => 1,
        SOF0 => 2,
        _ => 3,
    });
    let mut data = Vec::with_capacity(end + 16);
    data.extend_from_slice(&[0xFF, SOI]);
    let mut sof_at = 0;
    for segment in kept {
        if segment.marker == SOF0 {
            sof_at = data.len();
        }
        data.extend_from_slice(&jpeg[segment.start..segment.end]);
    }
    let sos_at = data.len();
    data.extend_from_slice(&jpeg[sos.start..end]);
    renumber_components(&mut data, sof_at, sos_at)?;
    if data.len() > limit {
        return Err(Error::IconTooLarge {
            size: data.len(),
            limit,
        });
    }

    let sos_len = usize::from(read_u16(&data, sos_at + 2)?);
    let sos_end = sos_at + 2 + sos_len;
    let pad = (4 - (sos_end + RECORD_HEADER_LEN) % 4) % 4;
    let padded_len = u16::try_from(sos_len + pad).map_err(|_| JpegError::HeaderTooLarge)?;
    data.splice(sos_end..sos_end, std::iter::repeat_n(0, pad));
    data[sos_at + 2..sos_at + 4].copy_from_slice(&padded_len.to_be_bytes());
    let header_len = u16::try_from(sos_end + pad).map_err(|_| JpegError::HeaderTooLarge)?;

    let tail = (4 - (data.len() + RECORD_HEADER_LEN) % 4) % 4;
    data.resize(data.len() + tail + TRAILING_ZEROS, 0);
    Ok(PreparedJpeg {
        width,
        height,
        header_len,
        data,
    })
}

/// Numbers the three components 1, 2, 3 in frame order, as libjpeg does, in
/// both the SOF0 segment at `sof` and the SOS segment at `sos`.
fn renumber_components(data: &mut [u8], sof: usize, sos: usize) -> Result<(), JpegError> {
    // Component IDs: SOF0 body bytes 6, 9, 12; SOS body bytes 1, 3, 5.
    let frame_id = |i: usize| sof + 10 + 3 * i;
    let scan_id = |i: usize| sos + 5 + 2 * i;
    if read_u16(data, sos + 2)? < 6 + 2 * 3 {
        return Err(JpegError::Truncated);
    }
    let ids = [0, 1, 2].map(|i| data[frame_id(i)]);
    for i in 0..3 {
        let id = data[scan_id(i)];
        let index = ids
            .iter()
            .position(|&x| x == id)
            .ok_or(JpegError::UnknownComponent(id))?;
        data[scan_id(i)] = index as u8 + 1;
        data[frame_id(i)] = i as u8 + 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

    fn encode(
        width: u16,
        height: u16,
        sampling: SamplingFactor,
        setup: impl FnOnce(&mut Encoder<&mut Vec<u8>>),
    ) -> Vec<u8> {
        let pixels: Vec<u8> = (0..usize::from(width) * usize::from(height) * 3)
            .map(|i| (i * 7 % 251) as u8)
            .collect();
        let mut out = Vec::new();
        let mut encoder = Encoder::new(&mut out, 80);
        encoder.set_sampling_factor(sampling);
        setup(&mut encoder);
        encoder
            .encode(&pixels, width, height, ColorType::Rgb)
            .unwrap();
        out
    }

    fn baseline(width: u16, height: u16, sampling: SamplingFactor) -> Vec<u8> {
        encode(width, height, sampling, |_| {})
    }

    #[test]
    fn prepare_aligns_scan_data_and_record() {
        for (w, h) in [(1, 1), (17, 9), (64, 33), (127, 128)] {
            for sampling in [SamplingFactor::F_1_1, SamplingFactor::F_2_2] {
                let jpeg = baseline(w, h, sampling);
                let p = prepare(&jpeg, usize::MAX).unwrap();
                assert_eq!((p.width, p.height), (w, h));
                let header_len = usize::from(p.header_len);
                assert_eq!((RECORD_HEADER_LEN + header_len) % 4, 0);
                assert_eq!((RECORD_HEADER_LEN + p.data.len()) % 4, 0);
                assert!(p.data.ends_with(&[0; TRAILING_ZEROS]));
                // The SOS length field covers the padding.
                let sos = p.data[..header_len]
                    .windows(2)
                    .rposition(|w| w == [0xFF, SOS])
                    .unwrap();
                assert_eq!(
                    sos + 2 + usize::from(read_u16(&p.data, sos + 2).unwrap()),
                    header_len
                );
            }
        }
    }

    /// The prepared payload, for comparing results.
    fn prepared(jpeg: &[u8]) -> Vec<u8> {
        prepare(jpeg, usize::MAX).unwrap().data
    }

    #[test]
    fn writes_libjpeg_header_layout() {
        // jpeg-encoder writes SOF0 before DQT and numbers components 0, 1, 2.
        let jpeg = baseline(16, 16, SamplingFactor::F_2_2);
        let p = prepare(&jpeg, usize::MAX).unwrap();
        let header = &p.data[..usize::from(p.header_len)];
        let segments = header_segments(header).unwrap();
        let markers: Vec<u8> = segments.iter().map(|s| s.marker).collect();
        assert_eq!(markers, [APP0, DQT, DQT, SOF0, DHT, DHT, DHT, DHT, SOS]);
        let frame = segments[3].body(header);
        assert_eq!([frame[6], frame[9], frame[12]], [1, 2, 3]);
        assert_eq!([frame[7], frame[10], frame[13]], [0x22, 0x11, 0x11]);
        let scan = segments[8].body(header);
        assert_eq!(scan[..7], [3, 1, 0x00, 2, 0x11, 3, 0x11]);
    }

    #[test]
    fn rejects_scan_with_unknown_component() {
        let mut jpeg = baseline(8, 8, SamplingFactor::F_1_1);
        let sos = jpeg.windows(2).position(|w| w == [0xFF, SOS]).unwrap();
        jpeg[sos + 5] = 9;
        assert!(matches!(
            prepare(&jpeg, usize::MAX),
            Err(Error::Jpeg(JpegError::UnknownComponent(9)))
        ));
    }

    #[test]
    fn strips_comments_and_metadata() {
        let jpeg = baseline(8, 8, SamplingFactor::F_2_2);
        let mut noisy = jpeg[..2].to_vec();
        noisy.extend_from_slice(&[0xFF, COM, 0, 7, b'h', b'e', b'l', b'l', b'o']);
        noisy.extend_from_slice(&[0xFF, 0xE1, 0, 6, b'E', b'x', b'i', b'f']);
        noisy.extend_from_slice(&jpeg[2..]);
        assert_eq!(prepared(&noisy), prepared(&jpeg));
    }

    #[test]
    fn drops_bytes_after_eoi() {
        let jpeg = baseline(8, 8, SamplingFactor::F_1_1);
        let mut trailing = jpeg.clone();
        trailing.extend_from_slice(b"junk");
        assert_eq!(prepared(&trailing), prepared(&jpeg));
    }

    #[test]
    fn rejects_unsupported_layouts() {
        let err = |jpeg: Vec<u8>| match prepare(&jpeg, usize::MAX) {
            Err(Error::Jpeg(e)) => e,
            other => panic!("expected JPEG error, got {other:?}"),
        };
        assert_eq!(
            err(encode(16, 16, SamplingFactor::F_2_2, |e| e.set_progressive(true))),
            JpegError::UnsupportedFrame(2)
        );
        assert_eq!(
            err(baseline(16, 16, SamplingFactor::F_2_1)),
            JpegError::Sampling
        );
        assert_eq!(
            err(encode(16, 16, SamplingFactor::F_2_2, |e| e
                .set_optimized_huffman_tables(true))),
            JpegError::MultiScan
        );
        assert_eq!(err(b"\x89PNG".to_vec()), JpegError::NotJpeg);
        let jpeg = baseline(16, 16, SamplingFactor::F_2_2);
        assert_eq!(err(jpeg[..jpeg.len() - 2].to_vec()), JpegError::Truncated);

        let mut gray = Vec::new();
        Encoder::new(&mut gray, 80)
            .encode(&[0; 64], 8, 8, ColorType::Luma)
            .unwrap();
        assert_eq!(err(gray), JpegError::Components(1));
    }

    #[test]
    fn enforces_size_limit_before_padding() {
        let jpeg = baseline(32, 32, SamplingFactor::F_2_2);
        assert!(prepare(&jpeg, jpeg.len()).is_ok());
        assert!(matches!(
            prepare(&jpeg, jpeg.len() - 1),
            Err(Error::IconTooLarge { size, .. }) if size == jpeg.len()
        ));
    }
}
