//! Sample encodings of WAE clips.

/// How clip samples are stored (header byte 15). Both are mono.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleFormat {
    /// Signed 16-bit little-endian PCM ("32KHz 16bit WAV" in DWIN's tools).
    #[default]
    Pcm16,
    /// DWIN "12:8" companding ("32KHz 12:8 WAV" in DWIN's tools): the top 12
    /// bits of each sample become an 8-bit code, a sign bit and a 7-bit
    /// logarithmic magnitude. Half the size of [`SampleFormat::Pcm16`].
    Companded8,
}

impl SampleFormat {
    /// Value stored in the WAE header.
    pub const fn code(self) -> u8 {
        match self {
            SampleFormat::Pcm16 => 0,
            SampleFormat::Companded8 => 1,
        }
    }

    pub const fn bytes_per_sample(self) -> usize {
        match self {
            SampleFormat::Pcm16 => 2,
            SampleFormat::Companded8 => 1,
        }
    }

    pub(crate) fn encode(self, samples: &[i16]) -> Vec<u8> {
        match self {
            SampleFormat::Pcm16 => samples.iter().flat_map(|s| s.to_le_bytes()).collect(),
            SampleFormat::Companded8 => samples.iter().map(|&s| compand(s)).collect(),
        }
    }
}

/// Smallest `|sample| >> 4` for each 12:8 magnitude code.
const COMPAND: [u16; 128] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 35, 37, 39, 41, 43, 45, 47, 49, 51, 53, 55, 57, 60, 64, 67, 70,
    73, 76, 79, 82, 86, 90, 94, 98, 103, 107, 112, 117, 122, 128, 133, 139, 145, 152, 159, 166,
    173, 180, 188, 196, 205, 214, 224, 234, 244, 255, 267, 279, 291, 304, 317, 332, 346, 362, 378,
    394, 412, 430, 449, 469, 490, 511, 534, 558, 582, 608, 635, 663, 693, 723, 755, 789, 824, 860,
    898, 938, 980, 1023, 1069, 1116, 1165, 1217, 1271, 1327, 1386, 1447, 1511, 1578, 1648, 1721,
    1798, 1877, 1960, 2047,
];

fn compand(sample: i16) -> u8 {
    let magnitude = sample.unsigned_abs().min(i16::MAX as u16) >> 4;
    let code = COMPAND.partition_point(|&lower| lower <= magnitude) - 1;
    code as u8 | if sample < 0 { 0x80 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compand_maps_both_ends_of_each_range_to_its_code() {
        for (code, &lower) in COMPAND.iter().enumerate() {
            let low = (lower << 4) as i16;
            let high = COMPAND
                .get(code + 1)
                .map_or(i16::MAX, |&next| ((next << 4) - 1) as i16);
            for sample in [low, high] {
                assert_eq!(compand(sample), code as u8, "sample {sample}");
                if sample != 0 {
                    assert_eq!(compand(-sample), code as u8 | 0x80, "sample -{sample}");
                }
            }
        }
        assert_eq!(compand(i16::MIN), 0xFF);
    }

    #[test]
    fn pcm16_is_little_endian() {
        let samples = [0x1234, -2, i16::MIN];
        assert_eq!(
            SampleFormat::Pcm16.encode(&samples),
            [0x34, 0x12, 0xFE, 0xFF, 0x00, 0x80]
        );
    }
}
