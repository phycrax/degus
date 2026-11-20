//! Builds the `DWIN_SET` folder that DWIN T5L displays download from an SD
//! card.
//!
//! A [`Bundle`] holds everything in the folder: the configuration, the
//! controls of every page, the icon and audio libraries and any other files.
//! [`Bundle::check`] reports problems, and [`Bundle::write`] checks the
//! bundle and writes the folder.
//!
//! The parts come from the format crates, re-exported here: [`config`],
//! [`controls`], [`icl`] and [`wae`].
//!
//! ```no_run
//! use degus::controls::{Area, KeyCode, Touch};
//! use degus::icl::{IclBuilder, IclOptions, IconSource};
//! use degus::{Bundle, Device, Size};
//!
//! let mut bundle = Bundle::new(Device::new(Size::new(800, 480)));
//!
//! let mut backgrounds = IclBuilder::new(IclOptions::default());
//! backgrounds.insert(0, IconSource::File("home.png".into()));
//! bundle.icls.insert(32, backgrounds.build()?);
//!
//! bundle.controls.page_mut(0).touch.push(Touch::new(
//!     Area::sized(300, 200, 120, 60),
//!     KeyCode::new(0x1000, 1),
//! ));
//!
//! let written = bundle.write("target")?; // target/DWIN_SET
//! for warning in &written.warnings {
//!     eprintln!("{warning}");
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod bundle;
mod check;
mod error;
mod file;

pub use degus_config as config;
pub use degus_controls as controls;
pub use degus_icl as icl;
pub use degus_wae as wae;

pub use bundle::{Bundle, Device, FOLDER, SLOT_SIZE, Size, Written};
pub use check::{Diagnostic, Location, Severity};
pub use error::{Error, Result};
pub use file::File;
