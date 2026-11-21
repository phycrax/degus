//! Builds and reads DWIN T5L WAE audio libraries (`DGUS_2` format).
//!
//! A WAE file is a 16-byte header, a table of `(offset, length)` entries
//! indexed by clip ID, and the clips themselves: mono 32 kHz audio, stored
//! as 16-bit PCM or DWIN's 8-bit "12:8" companding. The header
//! and every clip start on a 4 KB boundary.
//!
//! [`WaeBuilder`] converts audio files into a [`Wae`], [`Wae::to_bytes`]
//! writes the file and [`Wae::from_bytes`] reads one.
//!
//! ```no_run
//! use degus_wae::{ClipSource, SampleFormat, WaeBuilder, WaeOptions};
//!
//! let mut builder = WaeBuilder::new(WaeOptions {
//!     format: SampleFormat::Companded8,
//!     ..WaeOptions::default()
//! });
//! builder.insert(0, ClipSource::Encoded(std::fs::read("audio/beep.wav")?));
//! builder.insert(1, ClipSource::Encoded(std::fs::read("audio/alarm.mp3")?));
//! let wae = builder.build()?;
//! let file = wae.to_bytes()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod audio;
mod builder;
mod codec;
mod error;
mod format;
mod options;
mod wae;

pub use builder::{ClipSource, WaeBuilder};
pub use codec::SampleFormat;
pub use error::{Error, Result};
pub use format::{MAGIC, SAMPLE_RATE, VERSION};
pub use options::{Downmix, WaeOptions};
pub use wae::Wae;
