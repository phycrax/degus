//! The simulator running a small panel built in code: buttons, a held
//! button, the press effect, data input on a popup keyboard, the UART,
//! system variables, drawing with font 0, and sound.

use std::time::Duration;

use degus_codec::{Address, Crc, MAX_FRAME, Request, Response};
use degus_set::config::{Audio, Config};
use degus_set::controls::{
    Align, Area, DataInput, DataVar, Display, IconVar, KeyCode, Keyboard, Point, ProgressBar,
    TextVar, Touch, TouchAction, rgb565,
};
use degus_set::font::{Bitmap, Font0};
use degus_set::icl::image::{DynamicImage, Rgb, RgbImage};
use degus_set::icl::{Icl, IclBuilder, IclOptions, IconSource};
use degus_set::wae::{SampleFormat, Wae};
use degus_set::{Device, DwinSet, Size};
use degus_sim::{Event, Sim, Sound};
use hex_literal::hex;

const NAVY: Rgb<u8> = Rgb([0, 0, 128]);
const GREEN: Rgb<u8> = Rgb([0, 128, 0]);
const RED: Rgb<u8> = Rgb([200, 0, 0]);
const GRAY: Rgb<u8> = Rgb([128, 128, 128]);
const WHITE: Rgb<u8> = Rgb([255, 255, 255]);
const YELLOW: Rgb<u8> = Rgb([255, 255, 0]);

/// Font 0 at widths 4 and 8: every printable character is a line down its
/// first column, so each character drawn shows where its cell starts.
fn font0() -> Font0 {
    let mut font = Font0::new();
    for width in [4u8, 8] {
        for code in b'!'..=b'~' {
            let glyph = Bitmap::from_fn(width.into(), 2 * u16::from(width), |x, _| x == 0);
            font.set(width, code, glyph).unwrap();
        }
    }
    font
}

fn icl(icons: &[(u16, RgbImage)]) -> Icl {
    let mut builder = IclBuilder::new(IclOptions::default());
    for (id, image) in icons {
        builder.insert(
            *id,
            IconSource::Image(DynamicImage::ImageRgb8(image.clone())),
        );
    }
    builder.build().unwrap()
}

fn solid(w: u32, h: u32, color: Rgb<u8>) -> RgbImage {
    RgbImage::from_pixel(w, h, color)
}

/// A 200 × 120 panel. Page 0 has:
///
/// - display: a lamp icon at (10, 10) showing VP 0x1000, a number at (10,
///   40) showing VP 0x1001 as `000.0`, a text box at (60, 40) showing VP
///   0x2000, a progress bar at (10, 100) showing VP 0x1003;
/// - touch: "on" (130, 10) writing 1 to 0x1000 with page 2 as its press
///   effect, "off" (165, 10) writing 0 without upload, "next" (130, 40)
///   writing 5 to 0x1002 after 0.5 s and going to page 1, and "setpoint"
///   (165, 40), a data input for 0x1010 on the keyboard of page 3.
///
/// Page 3 holds the keyboard's keys, 25 × 20 each: `1 2 . enter` on the
/// first row, `cancel 5` on the second.
fn panel() -> DwinSet {
    let mut set = DwinSet::new(Device::new(Size::new(200, 120)));
    set.config = Some(Config {
        background_icl: 32,
        music_wae: 60,
        ..Config::default()
    });
    set.icls.insert(
        32,
        icl(&[
            (0, solid(200, 120, NAVY)),
            (1, solid(200, 120, GREEN)),
            (2, solid(200, 120, RED)),
            (3, solid(200, 120, GRAY)),
        ]),
    );
    let mut cut_out = solid(20, 20, WHITE);
    for (x, y, pixel) in cut_out.enumerate_pixels_mut() {
        if (5..15).contains(&x) && (5..15).contains(&y) {
            *pixel = RED;
        }
    }
    set.icls.insert(
        40,
        icl(&[
            (0, solid(20, 20, RED)),
            (1, solid(20, 20, WHITE)),
            (2, cut_out),
        ]),
    );
    set.font0 = Some(font0());

    let home = set.controls.page_mut(0);
    home.display.push(
        IconVar {
            vp: 0x1000,
            position: Point::new(10, 10),
            val_max: 1,
            icon_max: 1,
            icon_lib: 40,
            show_background: true,
            ..IconVar::default()
        }
        .into(),
    );
    home.display.push(
        DataVar {
            vp: 0x1001,
            position: Point::new(10, 40),
            color: rgb565(0xFFFFFF),
            font_size: 8,
            integer_digits: 3,
            decimal_digits: 1,
            ..DataVar::default()
        }
        .into(),
    );
    home.display.push(
        TextVar {
            vp: 0x2000,
            area: Area::sized(60, 40, 60, 16),
            color: rgb565(0xFFFFFF),
            length: 8,
            font_width: 8,
            font_height: 16,
            ..TextVar::default()
        }
        .into(),
    );
    home.display.push(
        ProgressBar {
            vp: 0x1003,
            area: Area::sized(10, 100, 100, 10),
            val_max: 100,
            bar_color: rgb565(0xFFFF00),
            rate_vp: Some(0x1004),
            ..ProgressBar::default()
        }
        .into(),
    );
    home.touch.push(Touch {
        pressed_page: Some(2),
        ..Touch::new(Area::sized(130, 10, 30, 20), KeyCode::new(0x1000, 1))
    });
    home.touch.push(Touch::new(
        Area::sized(165, 10, 30, 20),
        KeyCode {
            upload: false,
            ..KeyCode::new(0x1000, 0)
        },
    ));
    home.touch.push(Touch {
        next_page: Some(1),
        ..Touch::new(
            Area::sized(130, 40, 30, 20),
            KeyCode {
                hold_time: 5,
                ..KeyCode::new(0x1002, 5)
            },
        )
    });
    home.touch.push(Touch::new(
        Area::sized(165, 40, 30, 20),
        DataInput {
            vp: 0x1010,
            integer_digits: 3,
            decimal_digits: 1,
            position: Point::new(195, 64),
            color: rgb565(0xFFFFFF),
            font_size: 8,
            limits: Some(0..=1000),
            keyboard: Keyboard::Page {
                page: 3,
                area: Area::sized(0, 0, 100, 40),
                position: Point::new(0, 80),
            },
            ..DataInput::default()
        },
    ));

    let keyboard = set.controls.page_mut(3);
    let keys = [
        (0, 0, 0x31),
        (25, 0, 0x32),
        (50, 0, 0x2E),
        (75, 0, 0xF1),
        (0, 20, 0xF0),
        (25, 20, 0x35),
    ];
    for (x, y, code) in keys {
        keyboard.touch.push(Touch::new(
            Area::sized(x, y, 25, 20),
            TouchAction::Basic(code),
        ));
    }
    set
}

/// Whether `pixel` is `color`, give or take what JPEG does to it.
fn near(pixel: &Rgb<u8>, color: Rgb<u8>) -> bool {
    pixel
        .0
        .iter()
        .zip(color.0)
        .all(|(&a, b)| a.abs_diff(b) <= 20)
}

#[track_caller]
fn assert_near(fb: &RgbImage, x: u32, y: u32, color: Rgb<u8>) {
    let pixel = fb.get_pixel(x, y);
    assert!(near(pixel, color), "({x}, {y}) is {pixel:?}, not {color:?}");
}

/// The columns from `x` on, `len` of them, lit in `color` in row `y`.
fn lit_columns(fb: &RgbImage, x: u32, len: u32, y: u32, color: Rgb<u8>) -> Vec<u32> {
    (x..x + len)
        .filter(|&x| *fb.get_pixel(x, y) == color)
        .collect()
}

fn notes(sim: &mut Sim) -> Vec<String> {
    sim.take_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Note(note) => Some(note),
            _ => None,
        })
        .collect()
}

fn sounds(sim: &mut Sim) -> Vec<Sound> {
    sim.take_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Sound(sound) => Some(sound),
            _ => None,
        })
        .collect()
}

/// The frame the display sends for `words` from `vp`, without CRC.
fn data_frame(vp: u16, words: &[u16]) -> Vec<u8> {
    let data: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
    let mut buf = [0; MAX_FRAME];
    let response = Response::Data {
        addr: Address::Vp(vp),
        len: words.len() as u8,
        data: &data,
    };
    response.encode(&mut buf, Crc::off()).unwrap().to_vec()
}

fn tap(sim: &mut Sim, x: u16, y: u16) {
    sim.press(Point::new(x, y));
    sim.release();
}

#[test]
fn boots_on_page_0_and_draws_it() {
    let mut sim = Sim::new(panel());
    assert_eq!(sim.page(), 0);
    assert_eq!(sim.vp().read(0x007A, 2).unwrap(), [200, 120]);
    let fb = sim.render();
    assert_eq!(fb.dimensions(), (200, 120));
    assert_near(&fb, 195, 115, NAVY);
    // VP 0x1000 is 0: the red icon.
    assert_near(&fb, 15, 15, RED);
    assert_eq!(notes(&mut sim), Vec::<String>::new());
}

/// The examples of DWIN's T5L DGUSII Application Development Guide, §4.1.
#[test]
fn answers_frames_as_the_guide_shows() {
    let mut sim = Sim::new(panel());
    sim.feed(&hex!("5AA5 05 82 1000 3132"));
    assert_eq!(sim.take_tx(), hex!("5AA5 03 82 4F4B"));
    sim.feed(&hex!("5AA5 04 83 1000 01"));
    assert_eq!(sim.take_tx(), hex!("5AA5 06 83 1000 01 3132"));

    // With CRC on, split byte by byte.
    let mut set = panel();
    set.config.as_mut().unwrap().crc = true;
    let mut sim = Sim::new(set);
    let mut buf = [0; MAX_FRAME];
    let write = Request::Write {
        addr: Address::Vp(0x1001),
        data: &[0x00, 0x5A],
    };
    for byte in write.encode(&mut buf, Crc::on()).unwrap() {
        sim.feed(&[*byte]);
    }
    assert_eq!(sim.take_tx(), hex!("5AA5 05 82 4F4B A5EF"));
    // A read without CRC, then one with.
    sim.feed(&hex!("5AA5 04 83 1001 01"));
    let read = Request::Read {
        addr: Address::Vp(0x1001),
        len: 1,
    };
    sim.feed(read.encode(&mut buf, Crc::on()).unwrap());
    // Only the second is answered.
    assert_eq!(sim.take_tx(), hex!("5AA5 08 83 1001 01 005A 0E2C"));
    assert!(notes(&mut sim).iter().any(|n| n.contains("expects a CRC")));
}

#[test]
fn reports_what_it_does_not_answer() {
    let mut sim = Sim::new(panel());
    sim.feed(&hex!("00 11 22 5AA5 04 83 000F 01 33"));
    assert_eq!(sim.take_tx(), data_frame(0x000F, &[0x6322]));
    assert_eq!(
        notes(&mut sim),
        [
            "dropped 3 bytes outside a frame",
            "dropped 1 byte outside a frame"
        ]
    );
    // The guide's register read, and a read longer than a frame.
    sim.feed(&hex!("5AA5 04 81 00 0A 04"));
    sim.feed(&hex!("5AA5 04 83 1000 7E"));
    assert!(sim.take_tx().is_empty());
    let notes = notes(&mut sim);
    assert!(
        notes[0].starts_with("0x81 register read is not simulated"),
        "{notes:?}"
    );
    assert!(notes[1].contains("one frame holds 125"), "{notes:?}");
}

#[test]
fn buttons_write_upload_and_switch_pages() {
    let mut sim = Sim::new(panel());
    tap(&mut sim, 140, 20);
    assert_eq!(sim.vp().get(0x1000), 1);
    assert_eq!(sim.take_tx(), data_frame(0x1000, &[1]));
    tap(&mut sim, 170, 20); // upload off
    assert_eq!(sim.vp().get(0x1000), 0);
    assert!(sim.take_tx().is_empty());

    // Held for 0.5 s before it fires.
    sim.press(Point::new(140, 50));
    sim.advance(Duration::from_millis(400));
    assert_eq!(sim.vp().get(0x1002), 0);
    sim.advance(Duration::from_millis(100));
    assert_eq!(sim.vp().get(0x1002), 5);
    assert_eq!(sim.take_tx(), data_frame(0x1002, &[5]));
    assert_eq!(sim.page(), 0);
    sim.release();
    assert_eq!(sim.page(), 1);
    assert_eq!(sim.vp().get(0x0014), 1);
    assert!(sim.take_events().contains(&Event::Page(1)));
}

#[test]
fn shows_the_pressed_page_while_pressed() {
    let mut sim = Sim::new(panel());
    sim.press(Point::new(140, 20));
    let fb = sim.render();
    assert_near(&fb, 131, 11, RED);
    assert_near(&fb, 129, 11, NAVY);
    sim.release();
    let fb = sim.render();
    assert_near(&fb, 131, 11, NAVY);
    // The button wrote 1 on press: the white icon.
    assert_near(&fb, 15, 15, WHITE);
}

#[test]
fn the_first_touch_control_listed_wins() {
    let mut set = panel();
    set.controls.page_mut(0).touch.push(Touch::new(
        Area::sized(130, 10, 30, 20),
        KeyCode::new(0x1005, 9),
    ));
    let mut sim = Sim::new(set);
    tap(&mut sim, 140, 20);
    assert_eq!(sim.vp().get(0x1000), 1);
    assert_eq!(sim.vp().get(0x1005), 0);
}

#[test]
fn types_into_a_data_input_on_a_popup_keyboard() {
    let mut sim = Sim::new(panel());
    tap(&mut sim, 170, 50);
    assert_eq!(sim.input_text(), Some(""));
    // The keyboard's part of page 3 covers its place.
    assert_near(&sim.render(), 5, 85, GRAY);

    tap(&mut sim, 140, 20); // outside the keyboard: ignored while it is up
    assert_eq!(sim.vp().get(0x1000), 0);
    for (x, y) in [(10, 90), (35, 90), (60, 90), (35, 110)] {
        tap(&mut sim, x, y); // 1 2 . 5
    }
    assert_eq!(sim.input_text(), Some("12.5"));
    // The number is drawn right-aligned against the cursor at x = 195.
    let fb = sim.render();
    assert_eq!(lit_columns(&fb, 150, 50, 66, WHITE), [161, 169, 177, 185]);
    tap(&mut sim, 85, 90); // enter
    assert_eq!(sim.vp().get(0x1010), 125);
    assert_eq!(sim.take_tx(), data_frame(0x1010, &[125]));
    assert_eq!(sim.input_text(), None);
    assert_near(&sim.render(), 5, 85, NAVY);

    // Out of the limits: dropped like a cancel.
    tap(&mut sim, 170, 50);
    for (x, y) in [(35, 90), (35, 90), (35, 90), (35, 90), (60, 90), (35, 110)] {
        tap(&mut sim, x, y); // 2 2 2, a fourth digit does not fit, . 5
    }
    tap(&mut sim, 85, 90);
    assert_eq!(sim.vp().get(0x1010), 125);
    assert!(sim.take_tx().is_empty());
    assert!(notes(&mut sim).iter().any(|n| n.contains("\"222.5\"")));
    tap(&mut sim, 170, 50);
    tap(&mut sim, 10, 90);
    tap(&mut sim, 10, 110); // cancel
    assert_eq!(sim.vp().get(0x1010), 125);
    assert_eq!(sim.input_text(), None);
}

#[test]
fn runs_system_variables() {
    let mut sim = Sim::new(panel());
    // PIC_Set switches page, then clears its 5A.
    sim.write_vp(0x0084, &[0x5A01, 3]).unwrap();
    assert_eq!(sim.page(), 3);
    assert_eq!(sim.vp().get(0x0084), 0x0001);
    // A page without a background is ignored.
    sim.write_vp(0x0084, &[0x5A01, 9]).unwrap();
    assert_eq!(sim.page(), 3);
    assert!(notes(&mut sim).iter().any(|n| n.contains("no page 9")));

    sim.write_vp(0x0082, &[0x2000]).unwrap();
    assert_eq!(sim.brightness(), 0x20);
    sim.write_vp(0x0014, &[7]).unwrap();
    assert_eq!(sim.vp().get(0x0014), 3);
    assert!(
        notes(&mut sim)
            .iter()
            .any(|n| n.contains("0x0014 is read-only"))
    );

    // The RTC counts from where it was set.
    sim.set_rtc(1_569_928_333); // 2019-10-01 11:12:13
    sim.advance(Duration::from_secs(2));
    sim.feed(&hex!("5AA5 04 83 0010 04"));
    assert_eq!(
        sim.take_tx(),
        data_frame(0x0010, &[0x130A, 0x0102, 0x0B0C, 0x0F00])
    );

    // Reset: back to page 0 with VPs cleared.
    sim.write_vp(0x1000, &[1]).unwrap();
    sim.write_vp(0x0004, &[0x55AA, 0x5AA5]).unwrap();
    assert_eq!(sim.page(), 0);
    assert_eq!(sim.vp().get(0x1000), 0);
    assert_eq!(sim.brightness(), 100);
    assert!(sim.take_events().contains(&Event::Reset));
    assert!(sim.write_vp(0xFFFF, &[0, 1]).is_err());
}

#[test]
fn draws_numbers_text_and_bars() {
    let mut sim = Sim::new(panel());
    sim.write_vp(0x1001, &[235]).unwrap(); // 23.5
    sim.write_vp(0x1003, &[50]).unwrap(); // half
    sim.write_vp(0x2000, &[0x4142, 0x4344]).unwrap(); // ABCD
    let fb = sim.render();

    // Font 0 at width 8: one cell per character, from the position on.
    assert_eq!(lit_columns(&fb, 10, 50, 45, WHITE), [10, 18, 26, 34]);
    // Adjusted spacing: each character takes only its inked column.
    assert_eq!(lit_columns(&fb, 60, 60, 45, WHITE), [60, 61, 62, 63]);
    // The bar fills the left half, and its rate VP says so.
    assert_near(&fb, 30, 105, YELLOW);
    assert_near(&fb, 90, 105, NAVY);
    assert_eq!(sim.vp().get(0x1004), 50);

    // Right-aligned in the field of `000.0`, and fixed-width text.
    let mut set = panel();
    let page = set.controls.page_mut(0);
    if let Display::Data(data) = &mut page.display[1] {
        data.align = Align::Right;
    }
    if let Display::Text(text) = &mut page.display[2] {
        text.fixed_width = true;
        text.char_spacing = 3;
    }
    sim.reload(set);
    let fb = sim.render();
    assert_eq!(lit_columns(&fb, 10, 50, 45, WHITE), [18, 26, 34, 42]);
    assert_eq!(lit_columns(&fb, 60, 60, 45, WHITE), [60, 68, 76, 84]);
}

#[test]
fn draws_boxes_for_fonts_it_does_not_have() {
    let mut set = panel();
    if let Display::Text(text) = &mut set.controls.page_mut(0).display[2] {
        text.font = 5;
    }
    let mut sim = Sim::new(set);
    assert_eq!(
        notes(&mut sim),
        ["page 0, display control 2: font 5 is not simulated, so its characters draw as boxes"]
    );
    sim.write_vp(0x2000, &[0x4100]).unwrap(); // A
    let fb = sim.render();
    // An 8 × 16 box, inset by 1.
    assert_eq!(
        lit_columns(&fb, 60, 20, 41, WHITE),
        (61..67).collect::<Vec<_>>()
    );

    // Without font 0, font 0 text is not drawn, and that is said once.
    let mut set = panel();
    set.font0 = None;
    let mut sim = Sim::new(set);
    sim.write_vp(0x1001, &[235]).unwrap();
    sim.show_page(1);
    sim.show_page(0);
    let notes = notes(&mut sim);
    assert_eq!(notes.iter().filter(|n| n.contains("no font 0")).count(), 1);
    let fb = sim.render();
    assert_eq!(lit_columns(&fb, 10, 50, 45, WHITE), Vec::<u32>::new());
}

#[test]
fn keys_out_the_background_of_transparent_icons() {
    let mut set = panel();
    if let Display::Icon(icon) = &mut set.controls.page_mut(0).display[0] {
        icon.show_background = false;
        icon.icon_min = 2;
        icon.icon_max = 2;
        icon.val_max = 0;
    }
    let fb = Sim::new(set).render();
    assert_near(&fb, 12, 12, NAVY);
    assert_near(&fb, 20, 20, RED);
}

#[test]
fn reports_controls_it_does_not_run() {
    let mut set = panel();
    let page = set.controls.page_mut(0);
    let mut record = [0; 32];
    record[..2].copy_from_slice(&[0x5A, 0x20]);
    page.display.push(Display::Unknown(record));
    page.touch.insert(
        0,
        Touch {
            next_page: Some(1),
            ..Touch::new(
                Area::sized(0, 60, 40, 20),
                TouchAction::Unknown(vec![0xFE, 0x02]),
            )
        },
    );
    let mut sim = Sim::new(set);
    assert_eq!(
        notes(&mut sim),
        [
            "page 0, touch control 0: type 0x02 is not simulated; only its page switch and \
             press effect work",
            "page 0, display control 4: type 0x20 is not simulated, so it draws nothing",
        ]
    );
    // Its page switch still works.
    tap(&mut sim, 10, 70);
    assert_eq!(sim.page(), 1);
}

#[test]
fn loads_file_22_when_the_configuration_says_so() {
    let mut image = vec![0; 0x2001 * 2];
    image[0x2000 * 2..].copy_from_slice(&[0x41, 0x42]);
    let mut set = panel();
    set.files.insert("22_Config.bin".into(), image);
    assert_eq!(Sim::new(set.clone()).vp().get(0x2000), 0);

    set.config.as_mut().unwrap().load_22 = true;
    let mut sim = Sim::new(set);
    assert_eq!(sim.vp().get(0x2000), 0x4142);
    // A reset loads it again.
    sim.write_vp(0x2000, &[0]).unwrap();
    sim.write_vp(0x0004, &[0x55AA, 0x5AA5]).unwrap();
    assert_eq!(sim.vp().get(0x2000), 0x4142);
}

#[test]
fn reload_keeps_the_page_and_the_vps() {
    let mut sim = Sim::new(panel());
    sim.write_vp(0x1001, &[235]).unwrap();
    sim.show_page(1);
    let mut set = panel();
    set.icls.insert(32, icl(&[(1, solid(200, 120, YELLOW))]));
    sim.reload(set);
    assert_eq!(sim.page(), 1);
    assert_eq!(sim.vp().get(0x1001), 235);
    assert_near(&sim.render(), 100, 100, YELLOW);
}

#[test]
fn plays_clips_in_music_mode_and_reports_the_status() {
    let mut set = panel();
    let config = set.config.as_mut().unwrap();
    config.audio = Audio::Music;
    let mut wae = Wae::new(SampleFormat::Pcm16);
    wae.clips.insert(0, vec![100; 16_000]); // 500 ms
    wae.clips.insert(1, vec![100; 8_000]); // 250 ms
    set.waes.insert(60, wae);
    let mut other = Wae::new(SampleFormat::Pcm16);
    other.clips.insert(0, vec![7; 3_200]);
    set.waes.insert(61, other);
    let mut sim = Sim::new(set);

    assert_eq!(sim.vp().get(0x00A1), 0x4000); // full volume, stopped
    sim.write_vp(0x00A0, &[0x0101]).unwrap(); // clip 1, play once
    let played = sounds(&mut sim);
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].to_string(), "clip 1 of 60.wae at 100%");
    assert_eq!(sim.vp().get(0x00A0), 0x0100); // the display clears the count
    assert_eq!(sim.vp().get(0x00A1), 0x4002); // playing
    sim.advance(Duration::from_millis(200));
    assert_eq!(sim.vp().get(0x00A1), 0x4002);
    sim.advance(Duration::from_millis(100));
    assert_eq!(sim.vp().get(0x00A1), 0x4000); // stopped

    // 5A then WAE 61 in D2:D1; a volume over 0x40 plays at 100 %.
    sim.write_vp(0x00A0, &[0x0001, 0x7000, 0x5A00, 0x3D00])
        .unwrap();
    let Sound::Clip {
        wae,
        id,
        volume,
        samples,
    } = &sounds(&mut sim)[0]
    else {
        panic!("expected a clip");
    };
    assert_eq!((*wae, *id, *volume), (61, 0, 0x40));
    assert_eq!(samples.0[..], [7; 3_200]);

    // No clip 9 in 61.wae, which 0x00A2 still selects.
    sim.write_vp(0x00A0, &[0x0901]).unwrap();
    assert!(
        notes(&mut sim)
            .iter()
            .any(|n| n == "no clip 9 in 61.wae to play")
    );

    // Touch sounds play voice IDs, which are not simulated: said once.
    tap(&mut sim, 140, 20);
    tap(&mut sim, 140, 20);
    let notes = notes(&mut sim);
    assert_eq!(notes.iter().filter(|n| n.contains("voice IDs")).count(), 1);
}

#[test]
fn beeps_in_buzzer_mode() {
    let mut sim = Sim::new(panel());
    sim.write_vp(0x00A0, &[0x007D]).unwrap(); // 125 × 8 ms
    let beep = |sim: &mut Sim| match &sounds(sim)[..] {
        [Sound::Beep { length, .. }] => Some(*length),
        [] => None,
        other => panic!("{other:?}"),
    };
    assert_eq!(beep(&mut sim), Some(Duration::from_millis(1000)));
    assert_eq!(sim.vp().get(0x00A0), 0);
    tap(&mut sim, 140, 20); // a touch control: the factory 100 ms
    assert_eq!(beep(&mut sim), Some(Duration::from_millis(100)));
    tap(&mut sim, 100, 80); // bare screen: silent
    assert_eq!(beep(&mut sim), None);
    sim.write_vp(0x0080, &[0x5A00, 0x0010]).unwrap(); // touch sound off
    tap(&mut sim, 140, 20);
    assert_eq!(beep(&mut sim), None);
}
