//! Simulates a DWIN T5L display running a degus project: the VP space and
//! its system variables, pages, touch controls, data input, and the screen
//! drawn from the display controls.
//!
//! The simulator works from the project, not a built `DWIN_SET`: images
//! come from their source files, flattened like the build does, so editing
//! and reloading stay fast. JPEG artifacts and anything the panel's
//! hardware gets wrong are not shown. Fonts are a stand-in: one bitmap font
//! scaled into each control's character cell.
//!
//! Where DWIN's guide leaves behaviour open, the code says `ASSUME:` and
//! `ASSUMPTIONS.md` lists it, to check on a device.
//!
//! ```no_run
//! use degus_sim::{HostCmd, Sim};
//!
//! let mut sim = Sim::load("my-hmi")?;
//! sim.handle(&HostCmd::WriteVp { vp: 0x1001, data: vec![0x00, 235] });
//! sim.touch_down(310, 210);
//! sim.touch_up();
//! for upload in sim.take_outbox() {
//!     println!("{upload}");
//! }
//! sim.render().save("home.png")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [`Runner`] runs scripts of such steps; [`Link`] connects a host over TCP
//! (or a serial port with the `serial` feature) through a [`Codec`]: DGUS
//! frames with [`FrameCodec`], or text lines with [`LineCodec`].

mod audio;
mod draw;
mod font;
mod format;
mod frames;
mod host;
mod input;
mod line;
mod render;
pub mod script;
mod sim;
pub mod system;
mod text;
mod touch;
mod transport;
mod vp;
mod world;

pub use audio::{AudioSource, CLIP_RATE, FULL_VOLUME, ProjectAudio, Samples, Sound, buzzer_tone};
/// The buzzer settings a [`Sound::Beep`] carries.
pub use degus_config::Buzzer;
pub use draw::{Rect, rgb565};
pub use format::{Number, format_number};
pub use frames::FrameCodec;
pub use host::{Codec, Decoded, DeviceMsg, HostCmd};
pub use input::{KEY_BACKSPACE, KEY_CANCEL, KEY_DOT, KEY_ENTER, KEY_SIGN};
pub use line::LineCodec;
pub use script::{Runner, ScriptError};
pub use sim::{Event, OverlayItem, Sim};
pub use transport::{Link, LinkEvent, Waker};
pub use vp::{OutOfRange, USER_START, VP_WORDS, VpSpace};
pub use world::{IconSource, LoadError, PageInfo, ProjectIcons, Screen, World};
