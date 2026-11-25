//! Simulates a DWIN T5L display running a [`DwinSet`]: the VP space and
//! its system variables, pages, touch controls, data input, the UART, sound,
//! and the screen drawn from the display controls.
//!
//! [`Sim`] runs any set: one built in Rust, or one read with
//! [`DwinSet::read`] from a `DWIN_SET` folder, including one made by DWIN's
//! tools. It draws what the set holds: the icons as the JPEGs the display
//! stores, and text with the set's font 0. Characters that need another
//! font draw as boxes, and without a font 0 in the set, font 0 text is not
//! drawn. That, and controls the simulator does not run, are reported once,
//! when their page is shown.
//!
//! The simulator does no I/O and keeps no clock: whoever runs it feeds it
//! the host's bytes, touches and time, and takes the bytes, events and
//! pixels it gives back.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use degus_set::controls::Point;
//! use degus_set::{Device, DwinSet, Size};
//! use degus_sim::{Event, Sim};
//!
//! let set = DwinSet::read("target", Device::new(Size::new(800, 480)))?;
//! let mut sim = Sim::new(set);
//!
//! // The display's end of the UART: frames in, answers and uploads out.
//! sim.feed(&[0x5A, 0xA5, 0x05, 0x82, 0x10, 0x00, 0x00, 0x01]);
//! let to_host = sim.take_tx();
//!
//! // A finger, and time passing.
//! sim.press(Point::new(310, 210));
//! sim.advance(Duration::from_millis(100));
//! sim.release();
//!
//! for event in sim.take_events() {
//!     match event {
//!         Event::Sound(sound) => println!("play {sound}"),
//!         Event::Note(note) => println!("{note}"),
//!         _ => {}
//!     }
//! }
//! sim.render().save("home.png")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Where DWIN's guide leaves the display's behaviour open, the code says
//! `ASSUME:` and `ASSUMPTIONS.md` lists it, to check on a device.
//!
//! [`DwinSet`]: degus_set::DwinSet
//! [`DwinSet::read`]: degus_set::DwinSet::read

mod audio;
mod draw;
mod font;
mod icons;
mod input;
mod number;
mod render;
mod sim;
pub mod system;
mod text;
mod touch;
mod uart;
mod vp;

pub use audio::{CLIP_RATE, FULL_VOLUME, Samples, Sound, buzzer_tone};
/// The `image` crate, whose [`RgbImage`](image::RgbImage) [`Sim::render`]
/// draws into.
pub use degus_set::icl::image;
pub use sim::{Event, Sim};
pub use vp::{OutOfRange, USER_START, VP_WORDS, VpSpace};
