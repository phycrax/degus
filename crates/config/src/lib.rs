//! Writes and reads the hardware configuration file of DWIN T5L displays,
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
//! ```
//! use degus_config::{Config, Rotation};
//!
//! let config = Config {
//!     rotation: Rotation::Deg90,
//!     baud: 115_200,
//!     brightness: 80,
//!     ..Config::default()
//! };
//! let file = config.to_bytes()?;
//! assert_eq!(Config::from_bytes(&file)?, config);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod config;
mod error;

pub use config::{
    Audio, BAUD_CLOCK, Buzzer, ClockOut, Config, FILE_LEN, Lcd, NandSize, Rotation, TouchPanel,
};
pub use error::{Error, Result};
