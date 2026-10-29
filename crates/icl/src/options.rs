/// T5L chip variant. Only affects the per-icon JPEG size limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chip {
    /// T5L1 (256 KB icon budget). T5L0 shares this limit.
    T5L1,
    /// T5L2 (768 KB icon budget).
    #[default]
    T5L2,
    /// Another chip with the given icon budget in KB.
    Custom(u16),
}

impl Chip {
    /// Bytes DWIN's tools keep back from the icon budget.
    const RESERVED: usize = 8202;

    /// Icon budget in KB.
    pub const fn budget_kb(self) -> u16 {
        match self {
            Chip::T5L1 => 256,
            Chip::T5L2 => 768,
            Chip::Custom(kb) => kb,
        }
    }

    /// Largest JPEG (before alignment padding) the chip accepts for one icon:
    /// the icon budget minus 8202 bytes, as enforced by DWIN's tools.
    pub const fn max_jpeg_size(self) -> usize {
        (self.budget_kb() as usize * 1024).saturating_sub(Self::RESERVED)
    }
}

/// Chroma subsampling for encoded icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Subsampling {
    /// Full chroma resolution; sharper text and fine colour detail.
    S444,
    /// Half chroma resolution in both directions. DWIN's tools label this
    /// "4:1:1".
    #[default]
    S420,
}

/// What to do when an encoded icon exceeds [`Chip::max_jpeg_size`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OversizePolicy {
    /// Fail the build.
    #[default]
    Error,
    /// Re-encode with the quality lowered by `step` (0 counts as 1), retrying
    /// until the icon fits. Fails if it is still too large at quality 1.
    ReduceQuality { step: u8 },
}

impl OversizePolicy {
    /// [`OversizePolicy::ReduceQuality`] with the default step of 1.
    pub const fn reduce_quality() -> Self {
        Self::ReduceQuality { step: 1 }
    }
}

/// Where the ICL file will live in T5L flash: file `id` starts at
/// `id * size` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashSlot {
    /// File ID, the leading number of the ICL file name (32 for `32.icl`).
    pub id: u16,
    /// Bytes of flash per file ID ("Lib Size" in DWIN's tools: 256 KB or
    /// 512 KB).
    pub size: u32,
}

impl FlashSlot {
    pub const DEFAULT_SIZE: u32 = 256 * 1024;

    pub const fn new(id: u16) -> Self {
        Self {
            id,
            size: Self::DEFAULT_SIZE,
        }
    }
}

/// Settings for converting images into ICL icons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IclOptions {
    /// JPEG quality, 1-100.
    pub quality: u8,
    pub subsampling: Subsampling,
    /// Store JPEG inputs without re-encoding when the T5L can decode them
    /// ("Skip Original JPG" in DWIN's tools); only their header is rearranged
    /// into libjpeg's layout. Unsupported or oversized JPEGs are re-encoded.
    pub passthrough_jpeg: bool,
    /// Colour that transparent pixels are blended onto (DWIN's tools use
    /// white).
    pub background: [u8; 3],
    pub chip: Chip,
    pub oversize: OversizePolicy,
    /// Start the first icon record at a multiple of this many bytes
    /// ("Align Head" in DWIN's tools: 64 KB or 0). 0 places it right after the
    /// offset table.
    pub head_align: u32,
    /// Zero-pad every icon record to a multiple of this many bytes
    /// ("Align Body" in DWIN's tools: 100, 10, 1, 0.5 KB or 0). 0 disables
    /// padding.
    pub body_align: u32,
    /// When set, zero padding is inserted so that no icon record straddles a
    /// 16 MB flash boundary once the file is downloaded to this slot.
    pub flash_slot: Option<FlashSlot>,
}

impl Default for IclOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            subsampling: Subsampling::default(),
            passthrough_jpeg: false,
            background: [0xFF, 0xFF, 0xFF],
            chip: Chip::default(),
            oversize: OversizePolicy::default(),
            head_align: 0,
            body_align: 0,
            flash_slot: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Chip;

    #[test]
    fn chip_limits_keep_dwins_reserve() {
        // The values DWIN's tools enforce.
        assert_eq!(Chip::T5L1.max_jpeg_size(), 253_942);
        assert_eq!(Chip::T5L2.max_jpeg_size(), 778_230);
        assert_eq!(Chip::Custom(256).max_jpeg_size(), 253_942);
        assert_eq!(Chip::Custom(512).max_jpeg_size(), 516_086);
        assert_eq!(Chip::Custom(8).max_jpeg_size(), 0);
    }
}
