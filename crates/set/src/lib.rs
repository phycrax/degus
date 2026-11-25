//! Builds the `DWIN_SET` folder that DWIN T5L displays download from an SD
//! card.
//!
//! A [`DwinSet`] holds everything in the folder: the configuration, the
//! controls of every page, the icon and audio libraries and any other files.
//! [`DwinSet::check`] reports problems, and [`DwinSet::write`] checks the
//! set and writes the folder. [`DwinSet::read`] reads a folder back,
//! including one made by DWIN's tools.
//!
//! The parts come from the format crates, re-exported here: [`config`],
//! [`controls`], [`font`], [`icl`] and [`wae`]. Drawing fonts takes the
//! `render` feature, on by default.
//!
//! ```no_run
//! use degus_set::controls::{Area, KeyCode, Touch};
//! use degus_set::icl::{IclBuilder, IclOptions, IconSource};
//! use degus_set::{Device, DwinSet, Size};
//!
//! let mut set = DwinSet::new(Device::new(Size::new(800, 480)));
//!
//! let mut backgrounds = IclBuilder::new(IclOptions::default());
//! backgrounds.insert(0, IconSource::Encoded(std::fs::read("home.png")?));
//! set.icls.insert(32, backgrounds.build()?);
//!
//! set.controls.page_mut(0).touch.push(Touch::new(
//!     Area::sized(300, 200, 120, 60),
//!     KeyCode::new(0x1000, 1),
//! ));
//!
//! let written = set.write("target")?; // target/DWIN_SET
//! for warning in &written.warnings {
//!     eprintln!("{warning}");
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod check;
mod error;
mod file;
mod set;

pub use degus_config as config;
pub use degus_controls as controls;
pub use degus_font as font;
pub use degus_icl as icl;
pub use degus_wae as wae;

pub use check::{Diagnostic, Location, Severity};
pub use error::{Error, Result};
pub use file::{CONFIG_FILE, DISPLAY_FILE, FONT0_FILE, File, TOUCH_FILE};
pub use set::{Device, DwinSet, FOLDER, SLOT_SIZE, Size, Written};
