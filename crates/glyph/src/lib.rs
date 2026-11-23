//! Renders the glyphs of TrueType and OpenType fonts into coverage masks,
//! for building bitmap fonts.
//!
//! A [`Face`] holds a font file. [`Face::scaler`] picks a pixel size and a
//! [`Hinting`] mode, and [`Scaler::render`] draws a character into a
//! [`Glyph`]: 8-bit coverage, placed against the baseline.
//!
//! The outlines come from [`skrifa`], which also hints them.
//! [`Hinting::Mono`] snaps them to the pixel grid for 1-bit output, where an
//! unhinted outline loses or doubles stems at small sizes.
//!
//! ```no_run
//! use degus_glyph::{Face, Hinting};
//!
//! let face = Face::new(std::fs::read("fonts/mono.ttf")?)?;
//! let scaler = face.scaler(16.0, Hinting::Mono)?;
//! if let Some(glyph) = scaler.render('A')? {
//!     println!("{}x{}, advance {}", glyph.width, glyph.height, glyph.advance);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod error;
mod face;
mod raster;

pub use error::{Error, Result};
pub use face::{Face, Hinting, LineMetrics, Scaler};
pub use raster::Glyph;
