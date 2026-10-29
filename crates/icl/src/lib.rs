//! Builds DWIN T5L ICL icon libraries (`DGUS_3` format).
//!
//! An ICL file is a 16-byte header, a table of big-endian `u32` record
//! offsets indexed by icon ID, and one record per icon: a 10-byte record
//! header followed by a baseline JPEG whose scan data is 4-byte aligned.
//!
//! ```no_run
//! use degus_icl::{IclBuilder, IclOptions, IconSource};
//!
//! let mut builder = IclBuilder::new(IclOptions::default());
//! builder.insert(0, IconSource::File("images/home.png".into()));
//! builder.insert(1, IconSource::File("images/settings.png".into()));
//! let icl = builder.build()?;
//! std::fs::write("32.icl", &icl.bytes)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod builder;
mod encode;
mod error;
mod format;
mod jpeg;
mod options;

pub use builder::{BuiltIcl, IclBuilder, IconReport, IconSource};
pub use error::{Error, JpegError, Result};
pub use format::{MAGIC, VERSION};
pub use options::{Chip, FlashSlot, IclOptions, OversizePolicy, Subsampling};
