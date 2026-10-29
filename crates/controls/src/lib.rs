//! Builds the controls of DWIN T5L pages: the touch controls of
//! file 13 (`13TouchFile.bin`) and the display controls of file 14
//! (`14ShowFile.bin`), as written by the DGUS editor in "DGUS II" mode.
//!
//! Only the basic controls are supported so far:
//!
//! - touch: "Return key code" ([`KeyCode`]), "Data input" ([`DataInput`])
//!   and "Basic touch" ([`TouchAction::Basic`], the keys of a keyboard), each
//!   with optional page switch and press effect;
//! - display: "Variable icon" ([`IconVar`]), "Data variable" ([`DataVar`]),
//!   "Text display" ([`TextVar`]) and "Progress bar" ([`ProgressBar`]).
//!
//! The `serde` feature derives `Serialize` and `Deserialize` for the option
//! enums, using snake_case names such as `"high_byte"`.
//!
//! ```no_run
//! use degus_controls::{Area, Controls, DataVar, IconVar, KeyCode, Point, Touch};
//!
//! let mut controls = Controls::new();
//! let start = controls.page_mut(0);
//! start
//!     .touch
//!     .push(Touch::new(Area::new(1648, 848, 1798, 998), KeyCode::new(0xBA00, 2)));
//! start.display.push(
//!     IconVar {
//!         vp: 0x1000,
//!         position: Point::new(740, 439),
//!         val_max: 2,
//!         icon_min: 10100,
//!         icon_max: 10102,
//!         icon_lib: 48,
//!         ..IconVar::default()
//!     }
//!     .into(),
//! );
//! controls.page_mut(12).display.push(
//!     DataVar {
//!         vp: 0x120A,
//!         position: Point::new(632, 87),
//!         font_size: 24,
//!         integer_digits: 3,
//!         ..DataVar::default()
//!     }
//!     .into(),
//! );
//!
//! let files = controls.build()?;
//! std::fs::write(format!("DWIN_SET/{}", degus_controls::TOUCH_FILE_NAME), &files.touch)?;
//! std::fs::write(format!("DWIN_SET/{}", degus_controls::DISPLAY_FILE_NAME), &files.display)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod controls;
mod display;
mod error;
mod geom;
mod touch;

pub use controls::{
    BuiltControls, Controls, DISPLAY_FILE_NAME, MAX_DISPLAYS_PER_PAGE, MAX_PAGES, PageControls,
    TOUCH_FILE_NAME,
};
pub use display::{
    Align, BarDataType, BarDirection, DataType, DataVar, Display, Encoding, IconVar, LayerMode,
    ProgressBar, TextAlign, TextVar, VerticalAlign,
};
pub use error::{Error, Result};
pub use geom::{Area, Point};
pub use touch::{
    CursorColor, DataInput, InputDataType, KeyCode, KeyTarget, Keyboard, Touch, TouchAction,
};
