//! A simulated DWIN T5L display in an eframe app, which looks and acts
//! like a panel on the desk: a UART, a touch screen with a backlight, and a
//! speaker.
//!
//! - [`SimThread`] runs a [`degus_sim::Sim`] on a thread of its own, so
//!   that its answers don't wait for the window.
//! - The host talks to it over its [`Uart`], which reads and writes like a
//!   serial port.
//! - [`Screen`] shows it in any [`egui::Ui`], with the pointer as a finger.
//!   It never takes the keyboard focus, and handles no keys, so the app's
//!   keys stay the app's.
//! - [`Speaker`] plays its sounds, with the `audio` feature, on by default.
//!
//! Tools that look inside, such as `degus sim`'s, use
//! [`lock`](SimThread::lock) to read the simulator,
//! [`update`](SimThread::update) to change it, and
//! [`monitor`](SimThread::monitor) to watch the UART and the display's
//! events.
//!
//! ```no_run
//! use std::io::{Read, Write};
//!
//! use degus_set::{Device, DwinSet, Size};
//! use degus_sim::Sim;
//! use degus_sim_gui::{Screen, SimThread, Speaker, Zoom, eframe, egui};
//!
//! struct App {
//!     sim: SimThread,
//!     screen: Screen,
//! }
//!
//! impl eframe::App for App {
//!     fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
//!         egui::CentralPanel::default().show(ui, |ui| {
//!             self.screen.show(ui, &self.sim, Zoom::Fit);
//!         });
//!     }
//! }
//!
//! let set = DwinSet::read("panel/target", Device::new(Size::new(800, 480)))?;
//! let sim = SimThread::new(Sim::new(set));
//! sim.set_speaker(Speaker::new(0.8));
//!
//! // The firmware's end of the UART, on threads of its own.
//! let mut rx = sim.uart();
//! let mut tx = rx.try_clone()?;
//! std::thread::spawn(move || tx.write_all(&[0x5A, 0xA5, 0x05, 0x82, 0x10, 0x00, 0x00, 0x01]));
//! std::thread::spawn(move || {
//!     let mut buf = [0; 256];
//!     while let Ok(n @ 1..) = rx.read(&mut buf) {
//!         println!("from the display: {:02X?}", &buf[..n]);
//!     }
//! });
//!
//! let options = eframe::NativeOptions::default();
//! eframe::run_native(
//!     "panel",
//!     options,
//!     Box::new(|cc| {
//!         let ctx = cc.egui_ctx.clone();
//!         sim.on_change(move || ctx.request_repaint());
//!         let screen = Screen::new();
//!         Ok(Box::new(App { sim, screen }))
//!     }),
//! )?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! `examples/embed.rs` shows a set built in code beside a panel of the
//! app's own.

mod monitor;
mod notify;
mod screen;
mod speaker;
mod thread;
mod uart;

/// The eframe this crate is built with, and its egui.
pub use eframe::{self, egui};
pub use monitor::{Activity, ActivityKind, Monitor};
pub use screen::{Screen, Zoom};
pub use speaker::Speaker;
pub use thread::{SimGuard, SimThread};
pub use uart::Uart;
