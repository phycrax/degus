//! Builds and reads the font files of DWIN T5L displays.
//!
//! For now, font 0: the ASCII font in every size from 4 × 8 to 64 × 128
//! that displays come with, and that data variables and text controls use
//! by default. [`Font0`] holds its glyphs as [`Bitmap`]s, 1 bit per pixel;
//! [`Font0::to_bytes`] writes the file and [`Font0::from_bytes`] reads one.
//! DWIN's tools name the file `0_DWIN_ASC.HZK`.
//!
//! With the `render` feature, on by default, [`Font0::render`] draws font 0
//! from a TrueType or OpenType font through [`glyph`], the `degus-glyph`
//! crate.
//!
//! ```no_run
//! # #[cfg(feature = "render")] {
//! use degus_font::glyph::Face;
//! use degus_font::{Font0, Font0Options};
//!
//! let face = Face::new(std::fs::read("fonts/mono.ttf")?)?;
//! let font = Font0::render(&face, &Font0Options::default())?;
//! std::fs::write("0_DWIN_ASC.HZK", font.to_bytes())?;
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod bitmap;
mod error;
mod font0;
#[cfg(feature = "render")]
mod render;

pub use bitmap::Bitmap;
pub use error::{Error, Result};
pub use font0::{FONT0_CODES, FONT0_LEN, FONT0_WIDTHS, Font0};
#[cfg(feature = "render")]
pub use render::Font0Options;

/// The `degus-glyph` crate, whose [`Face`](glyph::Face) [`Font0::render`]
/// draws from.
#[cfg(feature = "render")]
pub use degus_glyph as glyph;
