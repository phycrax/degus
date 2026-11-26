//! What the tests share: small sets built in code, and a host on the
//! display's UART.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use degus_codec::{Accumulator, Address, Crc, Feed, MAX_CONTENT, MAX_FRAME, Request};
use degus_set::config::Config;
use degus_set::controls::{Area, IconVar, KeyCode, Point, Touch};
use degus_set::icl::image::{DynamicImage, Rgb, RgbImage};
use degus_set::icl::{Icl, IclBuilder, IclOptions, IconSource};
use degus_set::{Device, DwinSet, Size};
use degus_sim_gui::{SimThread, Uart};

/// What the tests allow for an answer: the host waits 50 ms.
pub const ANSWER: Duration = Duration::from_millis(50);
/// What they allow for things that only have to happen.
pub const WAIT: Duration = Duration::from_secs(5);

/// The key code's VP, and what it writes there.
pub const KEY_VP: u16 = 0x1000;
pub const KEY_VALUE: u16 = 1;
/// Where the key code is, on page 0.
pub const KEY: Area = Area::sized(10, 10, 50, 30);

pub fn icl(icons: &[(u16, RgbImage)]) -> Icl {
    let mut builder = IclBuilder::new(IclOptions::default());
    for (id, image) in icons {
        let source = IconSource::Image(DynamicImage::ImageRgb8(image.clone()));
        builder.insert(*id, source);
    }
    builder.build().unwrap()
}

/// A 200 × 120 panel without CRC: page 0, navy, has a key code writing 1 to
/// VP 0x1000 and uploading it; page 1 is green.
pub fn panel() -> DwinSet {
    let mut set = DwinSet::new(Device::new(Size::new(200, 120)));
    set.config = Some(Config {
        background_icl: 32,
        crc: false,
        ..Config::default()
    });
    set.icls.insert(
        32,
        icl(&[
            (0, RgbImage::from_pixel(200, 120, Rgb([0, 0, 128]))),
            (1, RgbImage::from_pixel(200, 120, Rgb([0, 128, 0]))),
        ]),
    );
    set.controls
        .page_mut(0)
        .touch
        .push(Touch::new(KEY, KeyCode::new(KEY_VP, KEY_VALUE)));
    set
}

/// A 1920 × 1080 panel on a noisy background, with icons over it keyed out
/// pixel by pixel: a render that takes some time.
pub fn full_hd() -> DwinSet {
    let (w, h) = (1920, 1080);
    let mut set = DwinSet::new(Device::new(Size::new(w, h)));
    set.config = Some(Config {
        background_icl: 32,
        ..Config::default()
    });
    let background = RgbImage::from_fn(w.into(), h.into(), |x, y| {
        let squares = if (x / 16 + y / 16) % 2 == 0 { 64 } else { 0 };
        Rgb([(x / 8) as u8, (y / 5) as u8, squares])
    });
    set.icls.insert(32, icl(&[(0, background)]));
    let mut icon = RgbImage::from_pixel(300, 300, Rgb([255, 255, 255]));
    for (x, y, pixel) in icon.enumerate_pixels_mut() {
        if (x / 30 + y / 30) % 2 == 0 {
            *pixel = Rgb([200, 0, 0]);
        }
    }
    set.icls.insert(40, icl(&[(0, icon)]));
    let page = set.controls.page_mut(0);
    for i in 0..6 {
        page.display.push(
            IconVar {
                vp: 0x2000,
                position: Point::new(100 + 300 * i, 300),
                icon_lib: 40,
                ..IconVar::default()
            }
            .into(),
        );
    }
    set
}

/// A frame from the host writing `words` from `vp` on.
pub fn write_frame(vp: u16, words: &[u16]) -> Vec<u8> {
    let data: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
    let request = Request::Write {
        addr: Address::Vp(vp),
        data: &data,
    };
    let mut buf = [0; MAX_FRAME];
    request.encode(&mut buf, Crc::off()).unwrap().to_vec()
}

/// What an ack frame carries.
pub const ACK: [u8; 3] = [0x82, b'O', b'K'];

/// The content of an upload of `words` from `vp`.
pub fn upload(vp: u16, words: &[u16]) -> Vec<u8> {
    let mut content = vec![0x83];
    content.extend(vp.to_be_bytes());
    content.push(words.len() as u8);
    content.extend(words.iter().flat_map(|w| w.to_be_bytes()));
    content
}

/// A host on the display's UART, reading frames on a thread.
pub struct Host {
    pub uart: Uart,
    /// Frames' content, and `None` once reading returned 0.
    frames: Receiver<Option<Vec<u8>>>,
}

impl Host {
    pub fn new(sim: &SimThread) -> Self {
        let uart = sim.uart();
        let mut reader = uart.try_clone().unwrap();
        let (sender, frames) = channel();
        std::thread::spawn(move || {
            let mut frames = Accumulator::new([0u8; MAX_CONTENT], Crc::off());
            let mut buf = [0u8; 256];
            loop {
                let n = reader.read(&mut buf).unwrap();
                if n == 0 {
                    let _ = sender.send(None);
                    return;
                }
                let mut input = &buf[..n];
                loop {
                    input = match frames.feed(input) {
                        Feed::Consumed => break,
                        Feed::Frame(content, rest) => {
                            let _ = sender.send(Some(content.to_vec()));
                            rest
                        }
                        Feed::Error(error, _) => panic!("the display sent no frame: {error}"),
                    };
                }
            }
        });
        Self { uart, frames }
    }

    pub fn send(&mut self, bytes: &[u8]) {
        self.uart.write_all(bytes).unwrap();
    }

    /// The content of the next frame within `within`.
    #[track_caller]
    pub fn frame(&self, within: Duration) -> Vec<u8> {
        match self.frames.recv_timeout(within) {
            Ok(Some(frame)) => frame,
            Ok(None) => panic!("the link closed"),
            Err(_) => panic!("no frame within {within:?}"),
        }
    }

    /// Whether a frame comes within `within`.
    pub fn quiet(&self, within: Duration) -> bool {
        matches!(
            self.frames.recv_timeout(within),
            Err(RecvTimeoutError::Timeout)
        )
    }

    /// Whether reading returns 0 within `within`, after any frames.
    pub fn closes(&self, within: Duration) -> bool {
        loop {
            match self.frames.recv_timeout(within) {
                Ok(Some(_)) => {}
                Ok(None) => return true,
                Err(_) => return false,
            }
        }
    }
}

/// Waits up to [`WAIT`] for `done`.
#[track_caller]
pub fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + WAIT;
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: not within {WAIT:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
