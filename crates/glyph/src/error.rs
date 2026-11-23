pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while reading fonts and rendering glyphs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a font: {0}")]
    NotFont(String),
    #[error("the font has no outlines, only bitmaps or nothing")]
    NoOutlines,
    #[error("{0} pixels is not a size to draw at")]
    Size(f32),
    #[error("failed to set up hinting: {0}")]
    Hinting(String),
    #[error("failed to draw {ch:?}: {message}")]
    Draw { ch: char, message: String },
}
