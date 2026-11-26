//! A simulated display in an app's own window, beside the app's panel.
//!
//! The set is built in code: a counter, and a button that uploads VP 0x1100
//! when touched. A host thread, standing in for the firmware, writes the
//! counter to VP 0x1000 through the display's UART and prints what the
//! display uploads. The app's panel shows whether Space is held, also while
//! the pointer touches the screen.
//!
//! ```sh
//! cargo run -p degus-sim-gui --example embed
//! ```

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use degus_codec::{Accumulator, Address, Crc, Feed, MAX_CONTENT, MAX_FRAME, Request, Response};
use degus_set::config::Config;
use degus_set::controls::{Area, DataVar, KeyCode, Point, Touch, rgb565};
use degus_set::font::{Bitmap, Font0};
use degus_set::icl::image::{DynamicImage, Rgb, RgbImage};
use degus_set::icl::{Icl, IclBuilder, IclOptions, IconSource};
use degus_set::{Device, DwinSet, Size};
use degus_sim::Sim;
use degus_sim_gui::egui::{self, Color32, Key, RichText};
use degus_sim_gui::{Screen, SimThread, Speaker, Uart, Zoom, eframe};

const COUNTER_VP: u16 = 0x1000;
const BUTTON_VP: u16 = 0x1100;
const BUTTON: Area = Area::sized(300, 180, 150, 70);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sim = SimThread::new(Sim::new(panel()));
    sim.set_speaker(Speaker::new(0.5));
    let counter = Arc::new(AtomicU16::new(0));
    host(sim.uart(), counter.clone())?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("degus embed")
            .with_inner_size([900.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "degus-embed",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            sim.on_change(move || ctx.request_repaint());
            Ok(Box::new(App {
                sim,
                screen: Screen::new(),
                counter,
            }))
        }),
    )?;
    Ok(())
}

struct App {
    sim: SimThread,
    screen: Screen,
    counter: Arc<AtomicU16>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::right("app")
            .default_size(240.0)
            .show(ui, |ui| {
                ui.heading("The app's panel");
                let space = ui.input(|i| i.key_down(Key::Space));
                let (text, color) = match space {
                    true => ("Space held", Color32::from_rgb(90, 200, 120)),
                    false => ("Space up", ui.visuals().weak_text_color()),
                };
                ui.label(RichText::new(text).size(24.0).color(color));
                ui.label(format!(
                    "The host wrote {}.",
                    self.counter.load(Ordering::Relaxed)
                ));
                let pressed = self.sim.lock().is_pressed();
                ui.label(format!(
                    "The screen is {}touched.",
                    if pressed { "" } else { "not " }
                ));
                ui.separator();
                ui.label("Hold Space and touch the button on the screen.");
            });
        egui::CentralPanel::default().show(ui, |ui| {
            self.screen.show(ui, &self.sim, Zoom::Fit);
        });
    }
}

/// The firmware's stand-in: writes a counter to the display and prints what
/// it uploads, until the display is gone.
fn host(uart: Uart, counter: Arc<AtomicU16>) -> std::io::Result<()> {
    let mut writer = uart.try_clone()?;
    std::thread::spawn(move || {
        let mut buf = [0u8; MAX_FRAME];
        for value in 1u16.. {
            let write = Request::Write {
                addr: Address::Vp(COUNTER_VP),
                data: &value.to_be_bytes(),
            };
            let frame = write.encode(&mut buf, Crc::off()).unwrap();
            if writer.write_all(frame).is_err() {
                return;
            }
            counter.store(value, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    let mut reader = uart;
    std::thread::spawn(move || {
        let mut frames = Accumulator::new([0u8; MAX_CONTENT], Crc::off());
        let mut buf = [0u8; 256];
        while let Ok(n @ 1..) = reader.read(&mut buf) {
            let mut input = &buf[..n];
            loop {
                input = match frames.feed(input) {
                    Feed::Consumed => break,
                    Feed::Frame(content, rest) => {
                        // Acks to the counter's writes aren't worth printing.
                        if let Ok(Response::Data {
                            addr: Address::Vp(vp),
                            data,
                            ..
                        }) = Response::from_content(content)
                        {
                            println!("the display uploads VP {vp:#06X}: {data:02X?}");
                        }
                        rest
                    }
                    Feed::Error(_, rest) => rest,
                };
            }
        }
    });
    Ok(())
}

/// A 480 × 272 panel: the counter on page 0, and a button whose press
/// effect is page 1.
fn panel() -> DwinSet {
    let (w, h) = (480, 272);
    let mut set = DwinSet::new(Device::new(Size::new(w, h)));
    set.config = Some(Config {
        background_icl: 32,
        ..Config::default()
    });
    let page = |button: Rgb<u8>| {
        RgbImage::from_fn(w.into(), h.into(), |x, y| {
            let on_button = (300..450).contains(&x) && (180..250).contains(&y);
            if on_button { button } else { Rgb([20, 30, 70]) }
        })
    };
    set.icls.insert(
        32,
        icl(&[
            (0, page(Rgb([90, 100, 130]))),
            (1, page(Rgb([240, 180, 40]))),
        ]),
    );
    set.font0 = Some(digits(32));

    let home = set.controls.page_mut(0);
    home.display.push(
        DataVar {
            vp: COUNTER_VP,
            position: Point::new(40, 40),
            color: rgb565(0xFFFFFF),
            font_size: 32,
            integer_digits: 5,
            ..DataVar::default()
        }
        .into(),
    );
    home.touch.push(Touch {
        pressed_page: Some(1),
        ..Touch::new(BUTTON, KeyCode::new(BUTTON_VP, 1))
    });
    set
}

fn icl(icons: &[(u16, RgbImage)]) -> Icl {
    let mut builder = IclBuilder::new(IclOptions::default());
    for (id, image) in icons {
        builder.insert(
            *id,
            IconSource::Image(DynamicImage::ImageRgb8(image.clone())),
        );
    }
    builder.build().expect("the icons fit")
}

/// Font 0 with the digits alone, as seven segments, at `width`.
fn digits(width: u8) -> Font0 {
    // Segments a to g, bit 0 to bit 6: top, top right, bottom right,
    // bottom, bottom left, top left, middle.
    const DIGITS: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
    let (w, h) = (u16::from(width), 2 * u16::from(width));
    let (margin, thick) = (w / 8, w / 6);
    let mut font = Font0::new();
    for (digit, segments) in (b'0'..).zip(DIGITS) {
        let glyph = Bitmap::from_fn(w, h, |x, y| {
            let lit = |segment: u8| segments & 1 << segment != 0;
            let row =
                |top: u16| (top..top + thick).contains(&y) && (margin..w - margin).contains(&x);
            let left = (margin..margin + thick).contains(&x);
            let right = (w - margin - thick..w - margin).contains(&x);
            let upper = (margin..=h / 2).contains(&y);
            let lower = (h / 2..h - margin).contains(&y);
            (lit(0) && row(margin))
                || (lit(1) && right && upper)
                || (lit(2) && right && lower)
                || (lit(3) && row(h - margin - thick))
                || (lit(4) && left && lower)
                || (lit(5) && left && upper)
                || (lit(6) && row(h / 2 - thick / 2))
        });
        font.set(width, digit, glyph)
            .expect("the glyph has the width's size");
    }
    font
}
