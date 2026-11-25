//! The VP space: 64K 16-bit words, addressed by VP and big-endian on the
//! wire. `0x0000`–`0x0FFF` are system variables, the rest is the user's.

use std::fmt;

/// Number of words in the VP space.
pub const VP_WORDS: usize = 0x1_0000;

/// First user VP; everything below is a system variable.
pub const USER_START: u16 = 0x1000;

/// A write that runs past `0xFFFF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("VP {start:#06X} + {words} word(s) runs past 0xFFFF")]
pub struct OutOfRange {
    pub start: u16,
    pub words: usize,
}

/// The display's variable memory.
#[derive(Clone, PartialEq, Eq)]
pub struct VpSpace {
    words: Box<[u16]>,
}

impl Default for VpSpace {
    fn default() -> Self {
        Self::new()
    }
}

/// `VpSpace { 3 non-zero words }`: the whole space is 64K words.
impl fmt::Debug for VpSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let set = self.words.iter().filter(|&&w| w != 0).count();
        write!(f, "VpSpace {{ {set} non-zero words }}")
    }
}

impl VpSpace {
    /// All zero, as at power-on without file 22.
    pub fn new() -> Self {
        Self {
            words: vec![0; VP_WORDS].into_boxed_slice(),
        }
    }

    pub fn get(&self, vp: u16) -> u16 {
        self.words[usize::from(vp)]
    }

    /// `count` words from `vp` on, or `None` past `0xFFFF`.
    pub fn read(&self, vp: u16, count: usize) -> Option<&[u16]> {
        let start = usize::from(vp);
        self.words.get(start..start.checked_add(count)?)
    }

    pub fn high(&self, vp: u16) -> u8 {
        self.get(vp).to_be_bytes()[0]
    }

    pub fn low(&self, vp: u16) -> u8 {
        self.get(vp).to_be_bytes()[1]
    }

    /// Two words, high word first.
    pub fn u32(&self, vp: u16) -> u32 {
        self.wide(vp, 2) as u32
    }

    /// Four words, highest word first.
    pub fn u64(&self, vp: u16) -> u64 {
        self.wide(vp, 4)
    }

    /// `len` bytes from `vp` on, high byte of each word first, cut short at
    /// `0xFFFF`.
    pub fn bytes(&self, vp: u16, len: usize) -> Vec<u8> {
        let start = usize::from(vp);
        let end = (start + len.div_ceil(2)).min(VP_WORDS);
        let mut bytes: Vec<u8> = self.words[start..end]
            .iter()
            .flat_map(|w| w.to_be_bytes())
            .collect();
        bytes.truncate(len);
        bytes
    }

    /// `n` words from `vp` on as one big-endian number, wrapping past
    /// `0xFFFF`.
    fn wide(&self, vp: u16, n: u16) -> u64 {
        (0..n).fold(0, |acc, i| {
            acc << 16 | u64::from(self.get(vp.wrapping_add(i)))
        })
    }

    pub(crate) fn clear(&mut self) {
        self.words.fill(0);
    }

    pub(crate) fn set(&mut self, vp: u16, value: u16) {
        self.words[usize::from(vp)] = value;
    }

    pub(crate) fn set_high(&mut self, vp: u16, value: u8) {
        self.set(vp, u16::from_be_bytes([value, self.low(vp)]));
    }

    pub(crate) fn set_low(&mut self, vp: u16, value: u8) {
        self.set(vp, u16::from_be_bytes([self.high(vp), value]));
    }

    /// Writes `words` from `vp` on.
    pub(crate) fn write(&mut self, vp: u16, words: &[u16]) -> Result<(), OutOfRange> {
        let start = usize::from(vp);
        let target = self
            .words
            .get_mut(start..start + words.len())
            .ok_or(OutOfRange {
                start: vp,
                words: words.len(),
            })?;
        target.copy_from_slice(words);
        Ok(())
    }

    /// Writes big-endian `bytes` from `vp` on. An odd last byte goes into
    /// the high byte of its word, leaving the low byte as it was.
    pub(crate) fn write_bytes(&mut self, vp: u16, bytes: &[u8]) -> Result<(), OutOfRange> {
        let words = bytes.len().div_ceil(2);
        let start = usize::from(vp);
        if start + words > VP_WORDS {
            return Err(OutOfRange { start: vp, words });
        }
        for (i, pair) in bytes.chunks(2).enumerate() {
            let at = start + i;
            self.words[at] = match *pair {
                [high, low] => u16::from_be_bytes([high, low]),
                [high] => u16::from_be_bytes([high, self.words[at].to_be_bytes()[1]]),
                _ => unreachable!(),
            };
        }
        Ok(())
    }

    /// Loads an image of the space from VP 0 on, as file 22 holds it: VP
    /// *n* at byte 2*n*. Bytes past `0xFFFF` are ignored.
    pub(crate) fn load(&mut self, image: &[u8]) {
        let image = &image[..image.len().min(2 * VP_WORDS)];
        // Fits: the image is cut to the space.
        let _ = self.write_bytes(0, image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_bytes_big_endian_keeping_the_low_byte_of_an_odd_end() {
        let mut vp = VpSpace::new();
        vp.set(0x1001, 0x00AA);
        vp.write_bytes(0x1000, &[0x12, 0x34, 0x56]).unwrap();
        assert_eq!(vp.read(0x1000, 2).unwrap(), [0x1234, 0x56AA]);
        assert_eq!(vp.bytes(0x1000, 3), [0x12, 0x34, 0x56]);
    }

    #[test]
    fn rejects_writes_past_the_end() {
        let mut vp = VpSpace::new();
        assert!(vp.write_bytes(0xFFFF, &[1, 2]).is_ok());
        assert_eq!(
            vp.write_bytes(0xFFFF, &[1, 2, 3]),
            Err(OutOfRange {
                start: 0xFFFF,
                words: 2
            })
        );
        assert!(vp.write(0xFFFE, &[1, 2, 3]).is_err());
        assert_eq!(vp.read(0xFFFF, 2), None);
    }

    #[test]
    fn reads_wide_values_high_word_first() {
        let mut vp = VpSpace::new();
        vp.write(0x2000, &[0x0001, 0x0002, 0x0003, 0x0004]).unwrap();
        assert_eq!(vp.u32(0x2000), 0x0001_0002);
        assert_eq!(vp.u64(0x2000), 0x0001_0002_0003_0004);
        vp.set_high(0x2000, 0xAB);
        vp.set_low(0x2001, 0xCD);
        assert_eq!(vp.read(0x2000, 2).unwrap(), [0xAB01, 0x00CD]);
    }

    #[test]
    fn loads_an_image_from_vp_0() {
        let mut vp = VpSpace::new();
        let mut image = vec![0; 0x2002 * 2];
        image[0x2000 * 2..].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        vp.load(&image);
        assert_eq!(vp.read(0x2000, 2).unwrap(), [0x1234, 0x5678]);
        // An image longer than the space is cut.
        vp.load(&vec![0xFF; 2 * VP_WORDS + 10]);
        assert_eq!(vp.get(0xFFFF), 0xFFFF);
    }
}
