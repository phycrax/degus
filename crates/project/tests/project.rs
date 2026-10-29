use std::path::{Path, PathBuf};

use degus_config::Config;
use degus_controls::{
    Align, Area, Controls, CursorColor, DataInput, DataType, DataVar, Display, Encoding, IconVar,
    InputDataType, KeyCode, KeyTarget, Keyboard, Point, ProgressBar, TextAlign, TextVar, Touch,
    TouchAction, VerticalAlign,
};
use degus_icl::{FlashSlot, IclBuilder, IclOptions, IconSource};
use degus_project::model::{Color, Size};
use degus_project::{Error, Project, Severity};
use degus_wae::{ClipSource, WaeBuilder, WaeOptions};
use image::{Rgb, RgbImage};

/// An empty folder for one test.
fn fresh_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("degus-project")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn image(w: u32, h: u32, shade: u8) -> RgbImage {
    RgbImage::from_pixel(w, h, Rgb([shade, 255 - shade, 128]))
}

fn save_image(path: impl AsRef<Path>, w: u32, h: u32, shade: u8) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image(w, h, shade).save(path).unwrap();
}

fn save_wav(path: impl AsRef<Path>) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 32_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..3200 {
        writer
            .write_sample(((i % 64) * 512 - 16384) as i16)
            .unwrap();
    }
    writer.finalize().unwrap();
}

const PROJECT: &str = r#"
screen = { w = 800, h = 480 }
copy = ["extra/readme.txt"]
controls = [
    { id = 0, path = "controls/main.toml" },
    { id = 1, path = "controls/keyboard.toml" },
]

[config]
background_icl = 32
music_wae = 60
brightness = 80

[[icl]]
id = 32
images = [
    { id = 0, path = "images/pages/main.png" },
    { id = 1, path = "images/pages/other.png" },
]

[[icl]]
id = 48
images = [
    { id = 10100, path = "images/icons/off.png" },
    { id = 10101, path = "images/icons/on.png" },
]

[[wae]]
id = 60
clips = [{ id = 1, path = "audio/beep.wav" }]
"#;

/// The last entry of `controls` in `PROJECT`, to add entries after.
const LAST_CONTROLS: &str = "    { id = 1, path = \"controls/keyboard.toml\" },\n";

const MAIN_PAGE: &str = r##"
[[control]]
type = "icon"
name = "power"
vp = 0x1000
position = { x = 10, y = 20 }
icon_lib = 48
val_min = 0
val_max = 1
icon_min = 10100
icon_max = 10101

[[control]]
type = "data"
vp = 0x1010
position = { x = 100, y = 20 }
color = "#FF0000"
font_size = 24
align = "center"
integer_digits = 3
decimal_digits = 1
data_type = "u16"
unit = "C"

[[control]]
type = "text"
vp = 0x2000
sp = 0x5000
area = { x = 10, y = 200, w = 300, h = 40 }
length = 32
encoding = "unicode"
font = 1
font_width = 16
font_height = 32
align = "right_to_left"
vertical_align = "center"

[[control]]
type = "progress"
name = "level"
vp = 0x1020
area = { x = 10, y = 300, w = 400, h = 20 }
val_max = 100
bar_color = "#00FF00"
show_border = true
rate_vp = 0x1021

[[control]]
type = "button"
name = "power"
area = { x = 10, y = 20, w = 64, h = 64 }
vp = 0x1000
target = { bit = 3 }
value = 1
next_page = 1
pressed_page = 1
upload = false

[[control]]
type = "input"
name = "setpoint"
area = { x = 100, y = 100, w = 200, h = 40 }
vp = 0x1030
position = { x = 290, y = 108 }
data_type = "i32"
integer_digits = 3
decimal_digits = 1
font_size = 24
cursor_color = "white"
keyboard = { page = 1, area = { x = 0, y = 0, w = 300, h = 200 }, position = { x = 400, y = 200 } }
limits = { min = 0, max = 1000 }
"##;

const KEYBOARD_PAGE: &str = r#"
[[control]]
type = "key"
area = { x = 0, y = 0, w = 100, h = 50 }
code = 0x0031
"#;

/// A project using every control and library kind.
fn sample_project(name: &str) -> PathBuf {
    let dir = fresh_dir(name);
    write(dir.join("project.toml"), PROJECT);
    write(dir.join("controls/main.toml"), MAIN_PAGE);
    write(dir.join("controls/keyboard.toml"), KEYBOARD_PAGE);
    save_image(dir.join("images/pages/main.png"), 800, 480, 10);
    save_image(dir.join("images/pages/other.png"), 800, 480, 200);
    save_image(dir.join("images/icons/off.png"), 64, 64, 50);
    save_image(dir.join("images/icons/on.png"), 64, 64, 150);
    std::fs::create_dir_all(dir.join("audio")).unwrap();
    save_wav(dir.join("audio/beep.wav"));
    write(dir.join("extra/readme.txt"), b"copied as it is");
    dir
}

#[test]
fn builds_every_library_and_the_controls() {
    let dir = sample_project("build");
    let project = Project::load(&dir).unwrap();
    assert_eq!(project.check(), []);

    let report = project.build().unwrap();
    let names: Vec<_> = report.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "32.icl",
            "48.icl",
            "60.wae",
            "13TouchFile.bin",
            "14ShowFile.bin",
            "T5LCFG.CFG",
            "readme.txt",
        ]
    );
    assert_eq!(report.files[0].number, Some(32));
    assert_eq!(report.files[5].number, None);
    let out = dir.join("target/DWIN_SET");
    assert_eq!(report.output, out);

    // Each build replaces DWIN_SET, so files of an earlier one are gone.
    write(out.join("33.icl"), b"left over");
    project.build().unwrap();
    assert!(!out.join("33.icl").exists());

    let read = |name: &str| std::fs::read(out.join(name)).unwrap();
    // The libraries are what their builders make of the same sources.
    let icl = |id: u16, images: &[(u16, &str)]| {
        let mut builder = IclBuilder::new(IclOptions {
            flash_slot: Some(FlashSlot::new(id)),
            ..IclOptions::default()
        });
        for &(icon, path) in images {
            builder.insert(icon, IconSource::File(dir.join(path)));
        }
        builder.build().unwrap().bytes
    };
    assert_eq!(
        read("32.icl"),
        icl(
            32,
            &[(0, "images/pages/main.png"), (1, "images/pages/other.png")]
        )
    );
    assert_eq!(
        read("48.icl"),
        icl(
            48,
            &[
                (10100, "images/icons/off.png"),
                (10101, "images/icons/on.png")
            ]
        )
    );
    let mut wae = WaeBuilder::new(WaeOptions::default());
    wae.insert(1, ClipSource::File(dir.join("audio/beep.wav")));
    assert_eq!(read("60.wae"), wae.build().unwrap().bytes);
    assert_eq!(read("readme.txt"), b"copied as it is");
    assert_eq!(
        read("T5LCFG.CFG"),
        Config {
            music_wae: 60,
            brightness: 80,
            ..Config::default()
        }
        .build()
        .unwrap()
    );

    let mut controls = Controls::new();
    let main = controls.page_mut(0);
    main.touch = vec![
        Touch {
            area: Area::new(10, 20, 74, 84),
            next_page: Some(1),
            pressed_page: Some(1),
            action: TouchAction::KeyCode(KeyCode {
                vp: 0x1000,
                target: KeyTarget::Bit(3),
                value: 1,
                hold_time: 0,
                upload: false,
            }),
        },
        Touch::new(
            Area::new(100, 100, 300, 140),
            DataInput {
                vp: 0x1030,
                data_type: InputDataType::I32,
                integer_digits: 3,
                decimal_digits: 1,
                position: Point::new(290, 108),
                font_size: 24,
                cursor_color: CursorColor::White,
                keyboard: Keyboard::Page {
                    page: 1,
                    area: Area::new(0, 0, 300, 200),
                    position: Point::new(400, 200),
                },
                limits: Some(0..=1000),
                ..DataInput::default()
            },
        ),
    ];
    main.display = vec![
        Display::Icon(IconVar {
            vp: 0x1000,
            position: Point::new(10, 20),
            val_max: 1,
            icon_min: 10100,
            icon_max: 10101,
            icon_lib: 48,
            ..IconVar::default()
        }),
        Display::Data(DataVar {
            vp: 0x1010,
            position: Point::new(100, 20),
            color: 0xF800,
            font_size: 24,
            align: Align::Center,
            integer_digits: 3,
            decimal_digits: 1,
            data_type: DataType::U16,
            unit: "C".into(),
            ..DataVar::default()
        }),
        Display::Text(TextVar {
            vp: 0x2000,
            sp: Some(0x5000),
            area: Area::new(10, 200, 310, 240),
            length: 32,
            encoding: Encoding::Unicode,
            font: 1,
            font_width: 16,
            font_height: 32,
            align: TextAlign::RightToLeft,
            vertical_align: VerticalAlign::Center,
            ..TextVar::default()
        }),
        Display::Progress(ProgressBar {
            vp: 0x1020,
            area: Area::new(10, 300, 410, 320),
            val_max: 100,
            bar_color: 0x07E0,
            show_border: true,
            rate_vp: Some(0x1021),
            ..ProgressBar::default()
        }),
    ];
    controls.page_mut(1).touch = vec![Touch::new(
        Area::new(0, 0, 100, 50),
        TouchAction::Basic(0x0031),
    )];
    assert_eq!(project.to_controls(), controls);
    let files = controls.build().unwrap();
    assert_eq!(read("13TouchFile.bin"), files.touch);
    assert_eq!(read("14ShowFile.bin"), files.display);

    // Clean removes DWIN_SET, and the output folder it leaves empty.
    assert!(project.clean().unwrap());
    assert!(!dir.join("target").exists());
    assert!(!project.clean().unwrap());
}

#[test]
fn loads_a_folder_with_paths_relative_to_it() {
    let dir = sample_project("folder");
    let project = Project::load(&dir).unwrap();
    assert_eq!(project.file(), dir.join("project.toml"));
    assert_eq!(project.root(), dir);
    assert_eq!(project.output_dir(), dir.join("target"));
    assert_eq!(project.check(), []);

    // A project is its folder, not its project file.
    assert!(matches!(
        Project::load(dir.join("project.toml")),
        Err(Error::NotAFolder(_))
    ));
}

#[test]
fn creates_a_project_that_builds() {
    let dir = fresh_dir("new").join("demo");
    let file = degus_project::new(&dir, Size::FULL_HD).unwrap();
    assert_eq!(file, dir.join("project.toml"));
    let project = Project::load(&dir).unwrap();
    assert_eq!(project.config.screen, Size::FULL_HD);
    assert_eq!(project.check(), []);
    project.build().unwrap();
    assert!(matches!(
        degus_project::new(&dir, Size::FULL_HD),
        Err(Error::Exists(_))
    ));
}

#[test]
fn reports_build_problems_by_file_and_control() {
    let dir = sample_project("check");
    write(
        dir.join("project.toml"),
        PROJECT
            .replace("id = 60", "id = 13")
            .replace("copy = [\"extra/readme.txt\"]", "copy = [\"missing.bin\"]")
            .replace(
                LAST_CONTROLS,
                &format!(
                    "{LAST_CONTROLS}\
                     {{ id = 2, path = \"controls/bad.toml\" }},\n\
                     {{ id = 6, path = \"controls/bad.toml\" }},\n\
                     {{ id = 1, path = \"controls/empty.toml\" }},\n\
                     {{ id = 4092, path = \"controls/empty.toml\" }},\n"
                ),
            ),
    );
    write(
        dir.join("controls/bad.toml"),
        r#"
[[control]]
type = "icon"
name = "gauge"
vp = 0x3000
position = { x = 900, y = 20 }
icon_lib = 48
val_max = 3
icon_min = 10100
icon_max = 10103

[[control]]
type = "icon"
vp = 0x3001
position = { x = 0, y = 0 }
icon_lib = 49
val_max = 0
icon_min = 1
icon_max = 1

[[control]]
type = "button"
area = { x = 700, y = 400, w = 200, h = 50 }
vp = 0x3002
value = 1
next_page = 7

[[control]]
type = "data"
vp = 0x3003
position = { x = 0, y = 0 }
unit = "°C"

[[control]]
type = "progress"
vp = 0x3004
area = { x = 0, y = 0, w = 100, h = 0 }
val_min = 10
val_max = 10

[[control]]
type = "input"
vp = 0x3005
area = { x = 0, y = 0, w = 100, h = 40 }
position = { x = 90, y = 10 }
limits = { min = 5, max = -5 }

[[control]]
type = "input"
vp = 0x3006
area = { x = 0, y = 50, w = 100, h = 40 }
position = { x = 90, y = 60 }
keyboard = { page = 0, area = { x = 0, y = 0, w = 300, h = 200 }, position = { x = 600, y = 100 } }

[[control]]
type = "key"
area = { x = 0, y = 100, w = 50, h = 50 }
code = 0xFE31

[[control]]
type = "icon"
vp = 0x3007
position = { x = 0, y = 0 }
val_max = 1
icon_min = 1
icon_max = 2
"#,
    );
    write(dir.join("controls/empty.toml"), "");

    let diagnostics = Project::load(&dir).unwrap().check();
    let messages: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    let expect = |severity: Severity, needle: &str| {
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == severity && d.to_string().contains(needle)),
            "no {severity:?} containing {needle:?} in {messages:#?}"
        );
    };
    expect(
        Severity::Error,
        "[[wae]] id = 13: ID 13 is already used by 13TouchFile.bin",
    );
    expect(Severity::Error, "copy: missing.bin not found");
    expect(
        Severity::Error,
        "control 1 (\"gauge\"): position (900, 20) is outside the 800x480 screen",
    );
    expect(
        Severity::Error,
        "control 1 (\"gauge\"): 48.icl has no icon 10102, 10103",
    );
    // bad.toml is listed for pages 2 and 6, so its messages name the page.
    expect(
        Severity::Error,
        "controls/bad.toml (page 6): control 1 (\"gauge\"): 48.icl has no icon",
    );
    expect(
        Severity::Error,
        "control 2: icon_lib 49 is not an ICL declared in project.toml",
    );
    expect(
        Severity::Error,
        "control 3: area (700, 400) 200x50 extends past the 800x480 screen",
    );
    expect(
        Severity::Warning,
        "control 3: next_page 7 is not a page of this project",
    );
    expect(Severity::Error, "control 4: unit \"°C\" must be ASCII");
    expect(Severity::Error, "control 5: area is empty");
    expect(
        Severity::Error,
        "control 5: val_min 10 is not below val_max 10",
    );
    expect(Severity::Error, "control 6: limits min 5 is above max -5");
    // Page 2's key is invalid, but it still counts as a keyboard.
    assert!(
        !messages.iter().any(|m| m.contains("control 6: keyboard")),
        "{messages:#?}"
    );
    expect(
        Severity::Error,
        "control 7: keyboard drawn at (600, 100) extends past the 800x480 screen",
    );
    expect(
        Severity::Warning,
        "control 7: keyboard page 0 has no key controls",
    );
    expect(
        Severity::Error,
        "control 8: code 0xFE31 must not start with FE or FD",
    );
    // Without icon_lib, the icons come from the background ICL.
    expect(
        Severity::Error,
        "control 9: 32.icl (the background ICL) has no icon 2",
    );
    expect(
        Severity::Error,
        "project.toml: controls: page 1 is listed twice",
    );
    expect(
        Severity::Error,
        "project.toml: controls: page ID 4092 is not below 4092",
    );
    expect(
        Severity::Warning,
        "project.toml: controls: page 2 has no background image in 32.icl",
    );

    // Errors stop the build before anything is written, and the build
    // reports what the check does, warnings included.
    let project = Project::load(&dir).unwrap();
    let error = project.build().unwrap_err();
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert_eq!(
        error.to_string(),
        format!("the project has {errors} error(s)")
    );
    assert!(matches!(error, Error::Check(all) if all == diagnostics));
    assert!(!dir.join("target").exists());
}

#[test]
fn rejects_unknown_fields_with_their_place() {
    let dir = sample_project("unknown-field");
    write(
        dir.join("controls/main.toml"),
        MAIN_PAGE.replace("icon_max = 10101", "icon_max = 10101\nicon_mx = 3"),
    );
    let error = Project::load(&dir).unwrap_err().to_string();
    assert!(error.contains("main.toml"), "{error}");
    assert!(error.contains("unknown field `icon_mx`"), "{error}");
    assert!(error.contains("line 2"), "{error}");

    // Page IDs live in the project file, so a controls file cannot set one.
    write(dir.join("controls/main.toml"), "id = 0\n");
    let error = Project::load(&dir).unwrap_err().to_string();
    assert!(error.contains("unknown field `id`"), "{error}");

    write(dir.join("controls/main.toml"), MAIN_PAGE);
    std::fs::remove_file(dir.join("controls/keyboard.toml")).unwrap();
    let error = Project::load(&dir).unwrap_err().to_string();
    assert!(error.contains("keyboard.toml"), "{error}");
}

#[test]
fn takes_colours_as_rrggbb() {
    let dir = sample_project("colour");
    for (color, message) in [
        ("0xF800", "expected a colour \"#RRGGBB\""),
        ("\"#F80\"", "\"#F80\" is not a colour such as \"#FF8000\""),
        ("\"FF0000\"", "\"FF0000\" is not a colour"),
        ("\"#FF00GG\"", "\"#FF00GG\" is not a colour"),
    ] {
        write(
            dir.join("controls/main.toml"),
            MAIN_PAGE.replace("color = \"#FF0000\"", &format!("color = {color}")),
        );
        let error = Project::load(&dir).unwrap_err().to_string();
        assert!(error.contains("main.toml"), "{error}");
        assert!(error.contains(message), "{error}");
    }

    // The files keep the high bits of each channel, as the DGUS editor does.
    let color: Color = "#ff8040".parse().unwrap();
    assert_eq!(
        color.to_rgb565(),
        (0xFF >> 3) << 11 | (0x80 >> 2) << 5 | 0x40 >> 3
    );
    assert_eq!(color.to_string(), "#FF8040");
}

#[test]
fn reports_library_problems() {
    let dir = sample_project("libraries");
    write(
        dir.join("project.toml"),
        r#"
screen = { w = 800, h = 480 }

[config]
background_icl = 33

[[icl]]
id = 48
images = [
    { id = 10100, path = "images/icons/off.png" },
    { id = 10100, path = "images/icons/on.png" },
]

[[icl]]
id = 51

[[wae]]
id = 60
clips = [{ id = 1, path = "audio/missing.wav" }]
"#,
    );

    let messages: Vec<String> = Project::load(&dir)
        .unwrap()
        .check()
        .iter()
        .map(ToString::to_string)
        .collect();
    for expected in [
        "error: project.toml: [config] background_icl: 33 is not an ICL declared in project.toml",
        "error: project.toml: [[icl]] id = 48: images: ID 10100 is listed twice",
        "error: project.toml: [[icl]] id = 51: needs `images`",
        "error: project.toml: [[wae]] id = 60: audio/missing.wav not found",
    ] {
        assert!(
            messages.iter().any(|m| m == expected),
            "no {expected:?} in {messages:#?}"
        );
    }
}

#[test]
fn rejects_files_that_overlap_in_flash() {
    let dir = fresh_dir("flash");
    write(
        dir.join("project.toml"),
        "screen = { w = 800, h = 480 }\ncopy = [\"40_big.bin\", \"41_next.bin\"]\n",
    );
    write(dir.join("40_big.bin"), vec![0; 300 * 1024]);
    write(dir.join("41_next.bin"), b"next");

    let error = Project::load(&dir).unwrap().build().unwrap_err();
    assert!(
        matches!(&error, Error::FlashOverlap { first, second }
            if first == "40_big.bin" && second == "41_next.bin"),
        "{error}"
    );
}

#[test]
fn checks_the_config() {
    let dir = sample_project("config");
    let check = |config: &str, copy: &str| {
        let text = PROJECT
            .replace("brightness = 80", config)
            .replace("copy = [\"extra/readme.txt\"]", copy);
        write(dir.join("project.toml"), text);
        Project::load(&dir)
            .unwrap()
            .check()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    let at = "project.toml: [config]";

    assert_eq!(
        check("brightness = 101", ""),
        [format!("error: {at}: brightness is 101, not in 0-100")]
    );
    assert_eq!(
        check("baud = 1000000", ""),
        [format!(
            "error: {at}: baud 1000000 is not reachable; the nearest rate is 1075200"
        )]
    );
    assert_eq!(
        check("load_22 = true\ninit_file = 21", ""),
        [format!(
            "warning: {at}: load_22 is on, but the build has no file 21; \
             the display loads the file 21 it holds"
        )]
    );
    assert_eq!(
        check(
            "rotation = 90\nlcd = { pclk_phs = 1, pclk_div = 6, h_w = 30, h_s = 16, \
             h_d = 800, h_e = 210, v_w = 3, v_s = 20, v_d = 480, v_e = 12 }",
            ""
        ),
        [format!(
            "warning: {at}: lcd is 800x480, so pages at rotation 90 are 480x800, \
             not the 800x480 screen"
        )]
    );
    assert_eq!(
        check("sd_folder = \"MY_SET\"", ""),
        [format!(
            "warning: {at}: sd_folder stores \"MY_SET\" in the display's flash: later SD \
             downloads must use a folder named MY_SET"
        )]
    );
    write(dir.join("extra/T5LCFG_800480.CFG"), b"T5LC1");
    assert_eq!(
        check("", "copy = [\"extra/T5LCFG_800480.CFG\"]"),
        [
            "error: project.toml: copy: T5LCFG_800480.CFG would sit beside the T5LCFG.CFG \
          built from [config]"
        ]
    );
    let error = |config: &str| {
        write(
            dir.join("project.toml"),
            PROJECT.replace("brightness = 80", config),
        );
        Project::load(&dir).unwrap_err().to_string()
    };
    assert!(error("rotation = 45").contains("rotation 45 is not 0, 90, 180 or 270"));
    assert!(error("lcd = { h_d = 800 }").contains("missing field"));
}

#[test]
fn builds_no_config_without_the_table_and_checks_no_backgrounds() {
    let dir = sample_project("no-config");
    let text = PROJECT
        .replace(
            "[config]\nbackground_icl = 32\nmusic_wae = 60\nbrightness = 80\n",
            "",
        )
        .replace(
            LAST_CONTROLS,
            &format!("{LAST_CONTROLS}{{ id = 5, path = \"controls/plain.toml\" }},\n"),
        );
    write(dir.join("project.toml"), text);
    write(dir.join("controls/plain.toml"), "");
    let project = Project::load(&dir).unwrap();
    assert!(project.config.config.is_none());
    assert_eq!(project.check(), []);
    let report = project.build().unwrap();
    assert!(!report.files.iter().any(|f| f.name == "T5LCFG.CFG"));
}
