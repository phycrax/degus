use std::fmt;

use crate::error::{Error, Result};

/// A 1-bit image: the glyph cell of a bitmap font.
///
/// It is stored as the fonts store it: rows from the top, each
/// ⌈width / 8⌉ bytes, the most significant bit being the leftmost pixel,
/// and the bits past the width 0. A set bit is a pixel in the text colour.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Bitmap {
    width: u16,
    height: u16,
    data: Vec<u8>,
}

impl Bitmap {
    /// A bitmap with no pixel set.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            data: vec![0; row_len(width) * usize::from(height)],
        }
    }

    /// A bitmap whose pixel `(x, y)` is set when `f(x, y)` is true.
    pub fn from_fn(width: u16, height: u16, mut f: impl FnMut(u16, u16) -> bool) -> Self {
        let mut bitmap = Self::new(width, height);
        for y in 0..height {
            for x in 0..width {
                if f(x, y) {
                    bitmap.set(x, y, true);
                }
            }
        }
        bitmap
    }

    /// A bitmap from rows laid out as fonts store them. Bits past the width
    /// are cleared. Fails unless `rows` holds exactly `height` rows.
    pub fn from_rows(width: u16, height: u16, rows: &[u8]) -> Result<Self> {
        let expected = row_len(width) * usize::from(height);
        if rows.len() != expected {
            return Err(Error::Rows {
                width,
                height,
                expected,
                len: rows.len(),
            });
        }
        let mut data = rows.to_vec();
        let unused = (row_len(width) * 8 - usize::from(width)) as u32;
        if unused > 0 {
            let keep = 0xFFu8 << unused;
            for row in data.chunks_mut(row_len(width)) {
                *row.last_mut().expect("rows of a non-zero width") &= keep;
            }
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Whether pixel `(x, y)` is set. Pixels outside the bitmap are not.
    pub fn get(&self, x: u16, y: u16) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let (index, bit) = self.bit(x, y);
        self.data[index] & bit != 0
    }

    /// Sets or clears pixel `(x, y)`.
    ///
    /// # Panics
    ///
    /// If `(x, y)` is outside the bitmap.
    pub fn set(&mut self, x: u16, y: u16, on: bool) {
        assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) is outside a {}x{} bitmap",
            self.width,
            self.height
        );
        let (index, bit) = self.bit(x, y);
        if on {
            self.data[index] |= bit;
        } else {
            self.data[index] &= !bit;
        }
    }

    /// Whether no pixel is set.
    pub fn is_blank(&self) -> bool {
        self.data.iter().all(|&b| b == 0)
    }

    /// The rows, as fonts store them.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    fn bit(&self, x: u16, y: u16) -> (usize, u8) {
        let index = usize::from(y) * row_len(self.width) + usize::from(x / 8);
        (index, 0x80 >> (x % 8))
    }
}

/// Bytes in a row of a bitmap `width` pixels wide.
pub(crate) fn row_len(width: u16) -> usize {
    usize::from(width).div_ceil(8)
}

/// `Bitmap(8x16)`; with `{:#?}`, also the pixels, `#` for set.
impl fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bitmap({}x{})", self.width, self.height)?;
        if f.alternate() {
            for y in 0..self.height {
                f.write_str("\n")?;
                for x in 0..self.width {
                    f.write_str(if self.get(x, y) { "#" } else { "." })?;
                }
            }
        }
        Ok(())
    }
}
