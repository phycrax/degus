use crate::font0::FONT0_LEN;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while building, writing or reading font files.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a {width}x{height} bitmap takes {expected} bytes of rows, not {len}")]
    Rows {
        width: u16,
        height: u16,
        expected: usize,
        len: usize,
    },
    #[error("font 0 is {FONT0_LEN} bytes long, not {0}")]
    Font0Length(usize),
    #[error("font 0 has widths 4 to 64, not {0}")]
    Font0Width(u8),
    #[error("font 0 has codes 0 to 127, not {0}")]
    Font0Code(u8),
    #[error("font 0's glyphs of width {width} are {width}x{}, not {got_width}x{got_height}", 2 * u16::from(*width))]
    GlyphSize {
        width: u8,
        got_width: u16,
        got_height: u16,
    },
    #[cfg(feature = "render")]
    #[error("font 0, width {width}: {source}")]
    Render {
        width: u8,
        #[source]
        source: degus_glyph::Error,
    },
}
