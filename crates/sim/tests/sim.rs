//! The simulator running a small project: buttons, a held button, the press
//! effect, data input on a popup keyboard, system variables and drawing.

use std::path::{Path, PathBuf};

use degus_sim::{DeviceMsg, Event, HostCmd, LoadError, Runner, Sim};
use image::{Rgb, RgbImage};

const NAVY: Rgb<u8> = Rgb([0, 0, 128]);
const GREEN: Rgb<u8> = Rgb([0, 128, 0]);
const RED: Rgb<u8> = Rgb([200, 0, 0]);
const GRAY: Rgb<u8> = Rgb([128, 128, 128]);
const WHITE: Rgb<u8> = Rgb([255, 255, 255]);
const YELLOW: Rgb<u8> = Rgb([255, 255, 0]);

const PROJECT: &str = r#"
screen = { w = 200, h = 120 }
include = ["controls/home.toml", "controls/keyboard.toml"]

[config]
background_icl = 32

[[icl]]
id = 32
images = [
    { id = 0, path = "pages/home.png" },
    { id = 1, path = "pages/next.png" },
    { id = 2, path = "pages/pressed.png" },
    { id = 3, path = "pages/keyboard.png" },
    { id = 4, path = "pages/next.png" },
]

[[icl]]
id = 40
images = [
    { id = 0, path = "icons/off.png" },
    { id = 1, path = "icons/on.png" },
]

# Pages with only their background: where "next" goes, and the pressed look.
[page.second]
id = 1

[page.pressed]
id = 2
"#;

const HOME: &str = r##"
[page.home]
id = 0
controls = ["lamp", "on", "off", "next", "reading", "level", "message", "setpoint"]

[control.lamp]
type = "icon"
vp = 0x1000
position = { x = 10, y = 10 }
icon_lib = 40
val_max = 1
icon_min = 0
icon_max = 1

[control.on]
type = "button"
area = { x = 100, y = 10, w = 40, h = 30 }
vp = 0x1000
value = 1
pressed_page = "pressed"

[control.off]
type = "button"
area = { x = 150, y = 10, w = 40, h = 30 }
vp = 0x1000
value = 0
upload = false

[control.next]
type = "button"
area = { x = 100, y = 50, w = 40, h = 20 }
vp = 0x1002
value = 5
hold_time = 5
next_page = "second"

[control.reading]
type = "data"
vp = 0x1001
position = { x = 10, y = 50 }
color = "#FFFFFF"
font_size = 8
integer_digits = 3
decimal_digits = 1

[control.level]
type = "progress"
vp = 0x1003
area = { x = 10, y = 100, w = 100, h = 10 }
val_max = 100
bar_color = "#FFFF00"
rate_vp = 0x1004

[control.message]
type = "text"
vp = 0x2000
area = { x = 10, y = 30, w = 80, h = 20 }
length = 8
font_width = 8
font_height = 16
color = "#FFFFFF"

[control.setpoint]
type = "input"
area = { x = 150, y = 50, w = 40, h = 20 }
vp = 0x1010
position = { x = 190, y = 50 }
integer_digits = 3
decimal_digits = 1
limits = { min = 0, max = 1000 }
keyboard = { page = "keyboard", area = { x = 0, y = 0, w = 100, h = 40 }, position = { x = 0, y = 80 } }
"##;

/// Keys at 25 × 20: `1 2 . enter` on the first row, `cancel 5` on the
/// second.
const KEYBOARD: &str = r#"
[page.keyboard]
id = 3
controls = ["key_1", "key_2", "key_point", "key_enter", "key_cancel", "key_5"]

[control.key_1]
type = "key"
area = { x = 0, y = 0, w = 25, h = 20 }
code = 0x31

[control.key_2]
type = "key"
area = { x = 25, y = 0, w = 25, h = 20 }
code = 0x32

[control.key_point]
type = "key"
area = { x = 50, y = 0, w = 25, h = 20 }
code = 0x2E

[control.key_enter]
type = "key"
area = { x = 75, y = 0, w = 25, h = 20 }
code = 0xF1

[control.key_cancel]
type = "key"
area = { x = 0, y = 20, w = 25, h = 20 }
code = 0xF0

[control.key_5]
type = "key"
area = { x = 25, y = 20, w = 25, h = 20 }
code = 0x35
"#;

fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn save(path: impl AsRef<Path>, w: u32, h: u32, color: Rgb<u8>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    RgbImage::from_pixel(w, h, color).save(path).unwrap();
}

fn project(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("degus-sim")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    write(dir.join("project.toml"), PROJECT);
    write(dir.join("controls/home.toml"), HOME);
    write(dir.join("controls/keyboard.toml"), KEYBOARD);
    save(dir.join("pages/home.png"), 200, 120, NAVY);
    save(dir.join("pages/next.png"), 200, 120, GREEN);
    save(dir.join("pages/pressed.png"), 200, 120, RED);
    save(dir.join("pages/keyboard.png"), 200, 120, GRAY);
    save(dir.join("icons/off.png"), 20, 20, RED);
    save(dir.join("icons/on.png"), 20, 20, WHITE);
    dir
}

/// Runs `script` and returns the transcript.
fn run(sim: &mut Sim, script: &str) -> Vec<String> {
    let mut log = Vec::new();
    let result =
        Runner::new(sim, std::env::temp_dir(), |line| log.push(line.to_owned())).run(script);
    if let Err(error) = result {
        panic!("{error}\n{}", log.join("\n"));
    }
    log
}

fn lit(fb: &RgbImage, x: u32, y: u32, w: u32, h: u32, color: Rgb<u8>) -> usize {
    (y..y + h)
        .flat_map(|y| (x..x + w).map(move |x| (x, y)))
        .filter(|&(x, y)| *fb.get_pixel(x, y) == color)
        .count()
}

#[test]
fn loads_the_pages_and_draws_the_start_page() {
    let sim = Sim::load(project("start")).unwrap();
    let pages: Vec<_> = sim
        .world()
        .pages
        .iter()
        .map(|p| (p.id, p.label.as_str()))
        .collect();
    assert_eq!(
        pages,
        [
            (0, "home"),
            (1, "second"),
            (2, "pressed"),
            (3, "keyboard"),
            (4, "background only"),
        ]
    );
    assert_eq!(sim.page(), 0);
    let fb = sim.render();
    assert_eq!(fb.dimensions(), (200, 120));
    assert_eq!(fb.get_pixel(195, 5), &NAVY);
    // VP 0x1000 is 0: the "off" icon.
    assert_eq!(fb.get_pixel(15, 15), &RED);
}

#[test]
fn refuses_a_project_the_build_would_refuse() {
    let dir = project("broken");
    // The "on" button runs past the screen's right edge.
    write(
        dir.join("controls/home.toml"),
        HOME.replace(
            "area = { x = 100, y = 10, w = 40, h = 30 }",
            "area = { x = 190, y = 0, w = 40, h = 10 }",
        ),
    );
    let Err(LoadError::Check(diagnostics)) = Sim::load(&dir) else {
        panic!("the check should refuse the project");
    };
    assert!(
        diagnostics.iter().any(|d| d
            .to_string()
            .contains("controls/home.toml: [control.on]: area")),
        "{diagnostics:#?}"
    );
}

#[test]
fn buttons_write_upload_and_switch_pages() {
    let mut sim = Sim::load(project("buttons")).unwrap();
    run(
        &mut sim,
        "
        touch 110 20
        expect 0x1000 1
        expect upload 0x1000 0x0001
        touch 160 20            # upload = false
        expect 0x1000 0
        expect upload none

        press 110 60            # held for 0.5 s before it fires
        wait 400ms
        expect 0x1002 0
        wait 100ms
        expect 0x1002 5
        expect upload 0x1002 5
        expect page 0
        release
        expect page 1
        read 0x0014 1
        ",
    );
}

#[test]
fn shows_the_pressed_page_while_pressed() {
    let mut sim = Sim::load(project("pressed")).unwrap();
    sim.touch_down(110, 20);
    let fb = sim.render();
    assert_eq!(fb.get_pixel(101, 11), &RED);
    assert_eq!(fb.get_pixel(99, 11), &NAVY);
    sim.touch_up();
    assert_eq!(sim.render().get_pixel(101, 11), &NAVY);
    // The button wrote 1 on press: the "on" icon.
    assert_eq!(sim.render().get_pixel(15, 15), &WHITE);
}

#[test]
fn types_into_a_data_input_on_a_popup_keyboard() {
    let mut sim = Sim::load(project("input")).unwrap();
    sim.touch_down(160, 60);
    sim.touch_up();
    assert_eq!(sim.input_text(), Some(""));
    // The keyboard image covers its area.
    assert_eq!(sim.render().get_pixel(5, 85), &GRAY);

    run(
        &mut sim,
        "
        touch 110 20     # outside the keyboard: ignored while it is up
        expect 0x1000 0
        touch 10 90      # 1
        touch 35 90      # 2
        touch 60 90      # .
        touch 35 110     # 5
        touch 85 90      # enter
        expect 0x1010 125
        expect upload 0x1010 125
        ",
    );
    assert_eq!(sim.input_text(), None);
    assert_eq!(sim.render().get_pixel(5, 85), &NAVY);

    // Out of the limits: dropped like a cancel.
    let log = run(
        &mut sim,
        "
        touch 160 60
        touch 35 90
        touch 35 90
        touch 35 90
        touch 35 90      # a fourth integer digit does not fit
        touch 60 90
        touch 35 110     # 222.5 is 2225 > 1000
        touch 85 90
        expect 0x1010 125
        expect upload none
        touch 160 60
        touch 10 90
        touch 10 110     # cancel
        expect 0x1010 125
        expect upload none
        ",
    );
    assert!(
        log.iter()
            .any(|line| line.contains("\"222.5\"") && line.contains("rejected"))
    );
}

#[test]
fn host_commands_reach_system_variables() {
    let mut sim = Sim::load(project("system")).unwrap();
    let write = |vp, data: &[u8]| HostCmd::WriteVp {
        vp,
        data: data.to_vec(),
    };
    // PIC_Set switches page, then clears its 5A.
    assert_eq!(
        sim.handle(&write(0x0084, &[0x5A, 0x01, 0x00, 0x03])),
        Some(DeviceMsg::Ack)
    );
    assert_eq!(sim.page(), 3);
    assert_eq!(sim.vp().get(0x0084), 0x0001);
    assert_eq!(
        sim.handle(&HostCmd::ReadVp {
            vp: 0x0014,
            words: 1
        }),
        Some(DeviceMsg::ReadReply {
            vp: 0x0014,
            data: vec![3]
        })
    );
    // A page without a background is ignored.
    sim.handle(&write(0x0084, &[0x5A, 0x01, 0x00, 0x09]));
    assert_eq!(sim.page(), 3);
    assert!(
        sim.take_events()
            .iter()
            .any(|e| matches!(e, Event::Note(n) if n.contains("no page 9")))
    );

    // Resolution and backlight.
    assert_eq!(sim.vp().read(0x007A, 2).unwrap(), [200, 120]);
    sim.handle(&write(0x0082, &[0x20, 0x00]));
    assert_eq!(sim.brightness(), 0x20);

    // Reset: back to page 0 with VPs cleared.
    sim.handle(&write(0x1000, &[0, 1]));
    sim.handle(&write(0x0004, &[0x55, 0xAA, 0x5A, 0xA5]));
    assert_eq!(sim.page(), 0);
    assert_eq!(sim.vp().get(0x1000), 0);
    assert_eq!(sim.brightness(), 100);

    // Writes past the end get no answer.
    assert_eq!(sim.handle(&write(0xFFFF, &[0, 1, 0, 2])), None);
}

#[test]
fn draws_display_controls_from_vps() {
    let mut sim = Sim::load(project("display")).unwrap();
    let before = sim.render();
    run(
        &mut sim,
        "
        write 0x1001 235    # data: 23.5
        write 0x1003 50     # progress: half
        write 0x2000 0x4142 # text: AB
        ",
    );
    let after = sim.render();

    // The progress bar fills the left half and reports 50 % at its rate VP.
    assert_eq!(after.get_pixel(30, 105), &YELLOW);
    assert_eq!(after.get_pixel(90, 105), &NAVY);
    assert_eq!(sim.vp().get(0x1004), 50);

    // "23.5" has more lit pixels than "0.0", inside the field.
    let data = |fb: &RgbImage| lit(fb, 10, 50, 40, 16, WHITE);
    assert!(data(&after) > data(&before));
    // The text draws inside its box.
    assert!(lit(&after, 10, 30, 80, 20, WHITE) > lit(&before, 10, 30, 80, 20, WHITE));
}

#[test]
fn labels_the_overlay_with_the_controls_source() {
    let sim = Sim::load(project("overlay")).unwrap();
    let overlay = sim.overlay();
    let labels: Vec<_> = overlay.iter().map(|item| item.label.as_str()).collect();
    assert!(labels.contains(&"controls/home.toml: [control.lamp]"));
    assert!(labels.contains(&"controls/home.toml: [control.setpoint]"));
    let lamp = overlay.iter().find(|i| i.label.contains("lamp")).unwrap();
    assert_eq!((lamp.rect.w, lamp.rect.h), (20, 20));
    assert!(!lamp.touch);
}

#[test]
fn saves_screenshots_relative_to_the_given_folder() {
    let dir = project("shot");
    let mut sim = Sim::load(&dir).unwrap();
    Runner::new(&mut sim, &dir, |_| {})
        .run("shot shots/home.png")
        .unwrap();
    let shot = image::open(dir.join("shots/home.png")).unwrap().to_rgb8();
    assert_eq!(shot.get_pixel(195, 5), &NAVY);
}
