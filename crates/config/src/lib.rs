//! Builds the hardware configuration file of DWIN T5L displays,
//! `T5LCFG.CFG`: the system settings the display applies from every file
//! (UART, backlight, rotation, the ICL holding page backgrounds, …) and the
//! blocks after them, each with its own enable marker (LCD timing, touch
//! panel, buzzer, …).
//!
//! A block left out is written disabled, and the display keeps the setting
//! it has. The LCD and touch panel blocks are set at the factory for the
//! panel; take them from the display's own CFG file if they must be
//! rewritten.
//!
//! The `serde` feature derives `Serialize` and `Deserialize` for the
//! enums and blocks, using snake_case names and rotations in degrees.
//!
//! ```no_run
//! use degus_config::{Config, Rotation};
//!
//! let config = Config {
//!     rotation: Rotation::Deg90,
//!     baud_divisor: Config::divisor_for_baud(115_200).unwrap(),
//!     brightness: 80,
//!     ..Config::default()
//! };
//! std::fs::write(format!("DWIN_SET/{}", degus_config::FILE_NAME), config.build()?)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod config;
mod error;

pub use config::{
    Audio, BAUD_CLOCK, Buzzer, ClockOut, Config, FILE_LEN, FILE_NAME, Lcd, MAX_BAUD_DIVISOR,
    NandSize, Rotation, TouchPanel,
};
pub use error::{Error, Result};
