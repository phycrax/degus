use std::ops::RangeInclusive;
use std::path::PathBuf;

use degus::config::{Config, Lcd};
use degus::controls::{
    Area, DataInput, DataVar, IconVar, KeyCode, KeyTarget, Keyboard, Point, ProgressBar, TextVar,
    Touch, TouchAction, key,
};
use degus::icl::image::{DynamicImage, Rgb, RgbImage};
use degus::icl::{Chip, Icl, IclBuilder, IclOptions, IconSource};
use degus::wae::{SampleFormat, Wae};
use degus::{Bundle, Device, Error, File, Size};

const SCREEN: Size = Size::new(800, 480);

/// An ICL with a `w` × `h` icon for each `(id, w, h)`.
fn icl(icons: &[(u16, u32, u32)]) -> Icl {
    let mut builder = IclBuilder::new(IclOptions::default());
    for &(id, w, h) in icons {
        let image = RgbImage::from_pixel(w, h, Rgb([40, 80, 120]));
        builder.insert(id, IconSource::Image(DynamicImage::ImageRgb8(image)));
    }
    builder.build().unwrap()
}

fn wae(ids: &[u16]) -> Wae {
    let mut wae = Wae::new(SampleFormat::Pcm16);
    for &id in ids {
        wae.clips.insert(id, vec![0, 100, -100, 0]);
    }
    wae
}

/// A bundle without problems: a home page with a button, a page-switch
/// button and an icon, and a settings page with a data input and its keys.
fn panel() -> Bundle {
    let mut bundle = Bundle::new(Device::new(SCREEN));
    bundle.config = Some(Config {
        music_wae: 48,
        ..Config::default()
    });
    bundle.icls.insert(32, icl(&[(0, 800, 480), (1, 800, 480)]));
    bundle.icls.insert(40, icl(&[(0, 64, 64), (1, 64, 64)]));
    bundle.waes.insert(48, wae(&[0]));
    bundle
        .files
        .insert("0_DWIN_ASC.HZK".into(), vec![0xAA; 1000]);

    let home = bundle.controls.page_mut(0);
    home.touch.push(Touch::new(
        Area::sized(300, 200, 120, 60),
        KeyCode::new(0x1000, 1),
    ));
    home.touch.push(Touch {
        next_page: Some(1),
        ..Touch::new(Area::sized(680, 20, 100, 60), TouchAction::Basic(key::NONE))
    });
    home.display.push(
        IconVar {
            vp: 0x1000,
            position: Point::new(100, 200),
            icon_lib: 40,
            val_max: 1,
            icon_max: 1,
            ..IconVar::default()
        }
        .into(),
    );

    let settings = bundle.controls.page_mut(1);
    settings.touch.push(Touch::new(
        Area::sized(300, 120, 200, 60),
        DataInput {
            vp: 0x1002,
            position: Point::new(480, 130),
            ..DataInput::default()
        },
    ));
    for (i, code) in (0u16..).zip([key::digit(1), key::digit(2), key::ENTER]) {
        settings.touch.push(Touch::new(
            Area::sized(i * 100, 400, 100, 80),
            TouchAction::Basic(code),
        ));
    }
    bundle
}

/// The check's diagnostics as lines.
fn check(bundle: &Bundle) -> Vec<String> {
    bundle.check().iter().map(ToString::to_string).collect()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_sound_bundle_has_no_diagnostics() {
    assert_eq!(check(&panel()), Vec::<String>::new());
}

#[test]
fn writes_the_folder_in_flash_order() {
    let dir = temp_dir("writes_the_folder_in_flash_order");
    let stale = dir.join("DWIN_SET").join("99_stale.bin");
    std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
    std::fs::write(&stale, b"left over").unwrap();

    let bundle = panel();
    let written = bundle.write(&dir).unwrap();
    assert!(written.warnings.is_empty());
    let names: Vec<_> = written.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "0_DWIN_ASC.HZK",
            "13TouchFile.bin",
            "14ShowFile.bin",
            "32.icl",
            "40.icl",
            "48.wae",
            "T5LCFG.CFG"
        ]
    );
    assert_eq!(written.files, bundle.to_files().unwrap());

    let mut on_disk: Vec<_> = std::fs::read_dir(dir.join("DWIN_SET"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    let mut expected: Vec<_> = names.iter().map(|name| name.to_string()).collect();
    expected.sort();
    assert_eq!(on_disk, expected, "the stale file is gone");
    for file in &written.files {
        let bytes = std::fs::read(dir.join("DWIN_SET").join(&file.name)).unwrap();
        assert_eq!(bytes, file.bytes, "{}", file.name);
    }
    let icl = &written.files[3];
    assert_eq!(icl.bytes, bundle.icls[&32].to_bytes(32).unwrap());
    assert_eq!(icl.number(), Some(32));
}

#[test]
fn write_refuses_a_bundle_with_errors() {
    let dir = temp_dir("write_refuses_a_bundle_with_errors");
    let mut bundle = panel();
    bundle.controls.page_mut(0).touch.push(Touch::new(
        Area::sized(750, 0, 100, 50),
        KeyCode::new(0x1001, 1),
    ));
    let error = bundle.write(&dir).unwrap_err();
    assert!(matches!(&error, Error::Check(diagnostics) if diagnostics.len() == 1));
    assert_eq!(
        error.to_string(),
        "the bundle has 1 error\n\
         error: page 0, touch control 2: area (750, 0) 100x50 extends past the 800x480 screen"
    );
    // So that `main` returning the error prints it readably.
    assert_eq!(format!("{error:?}"), error.to_string());
    assert!(!dir.join("DWIN_SET").exists());
}

#[test]
fn reports_controls_off_the_screen() {
    let mut bundle = panel();
    let home = bundle.controls.page_mut(0);
    home.touch.push(Touch::new(
        Area::new(10, 10, 10, 50),
        KeyCode::new(0x1001, 1),
    ));
    home.touch.push(Touch::new(
        Area::sized(0, 0, 100, 100),
        DataInput {
            position: Point::new(800, 10),
            limits: Some(RangeInclusive::new(10, -10)),
            keyboard: Keyboard::Page {
                page: 1,
                area: Area::sized(0, 380, 300, 100),
                position: Point::new(600, 200),
            },
            ..DataInput::default()
        },
    ));
    home.display.push(
        TextVar {
            area: Area::sized(700, 400, 200, 40),
            ..TextVar::default()
        }
        .into(),
    );
    home.display.push(
        ProgressBar {
            area: Area::sized(0, 0, 100, 10),
            val_max: 0,
            ..ProgressBar::default()
        }
        .into(),
    );
    home.display.push(
        DataVar {
            position: Point::new(0, 480),
            ..DataVar::default()
        }
        .into(),
    );
    assert_eq!(
        check(&bundle),
        [
            "error: page 0, touch control 2: area is empty",
            "error: page 0, touch control 3: limits 10 to -10 are the wrong way round",
            "error: page 0, touch control 3: position (800, 10) is outside the 800x480 screen",
            "error: page 0, touch control 3: the keyboard drawn at (600, 200) extends past \
             the 800x480 screen",
            "error: page 0, display control 1: area (700, 400) 200x40 extends past the \
             800x480 screen",
            "error: page 0, display control 2: val_min 0 is not below val_max 0",
            "error: page 0, display control 3: position (0, 480) is outside the 800x480 screen",
        ]
    );
}

#[test]
fn reports_what_controls_name_that_the_bundle_lacks() {
    let mut bundle = panel();
    let home = bundle.controls.page_mut(0);
    home.touch.push(Touch {
        next_page: Some(7),
        pressed_page: Some(5000),
        ..Touch::new(Area::sized(0, 0, 10, 10), KeyCode::new(0x1001, 1))
    });
    home.touch
        .push(Touch::new(Area::sized(0, 0, 10, 10), DataInput::default()));
    home.display.push(
        IconVar {
            icon_lib: 41,
            ..IconVar::default()
        }
        .into(),
    );
    home.display.push(
        IconVar {
            icon_lib: 40,
            val_max: 5,
            icon_max: 5,
            ..IconVar::default()
        }
        .into(),
    );
    home.display.push(
        IconVar {
            val_max: 3,
            icon_max: 2,
            ..IconVar::default()
        }
        .into(),
    );
    bundle.controls.page_mut(2).touch.push(Touch::new(
        Area::sized(0, 0, 10, 10),
        DataInput {
            keyboard: Keyboard::Page {
                page: 0,
                area: Area::sized(0, 0, 10, 10),
                position: Point::new(0, 0),
            },
            ..DataInput::default()
        },
    ));
    assert_eq!(
        check(&bundle),
        [
            "warning: page 2: has no background image in 32.icl",
            "warning: page 0, touch control 2: next_page 7 has no background image in 32.icl",
            "error: page 0, touch control 2: pressed_page 5000 is not below 4092",
            "warning: page 0, touch control 3: its keyboard is on its own page, which has no \
             keyboard keys",
            "error: page 0, display control 1: icon_lib 41 is not in the bundle",
            "error: page 0, display control 2: 40.icl has no icon 2, 3, 4, 5",
            "warning: page 0, display control 3: values 0..=3 and icons 0..=2 differ in \
             length; the display shows icon_min + (value - val_min)",
            "error: page 0, display control 3: 32.icl (the background ICL) has no icon 2",
            "warning: page 2, touch control 0: keyboard page 0 has no keyboard keys",
        ]
    );
}

#[test]
fn reports_values_the_files_cannot_hold() {
    let mut bundle = panel();
    bundle.controls.page_mut(1).touch[0] = Touch::new(
        Area::sized(0, 0, 10, 10),
        KeyCode {
            target: KeyTarget::Bit(16),
            ..KeyCode::new(0x1001, 1)
        },
    );
    bundle.config.as_mut().unwrap().brightness = 101;
    assert_eq!(
        check(&bundle),
        [
            "error: T5LCFG.CFG: brightness is 101, not in 0-100",
            "error: page 1, touch control 0: target bit 16 is not in 0-15",
        ]
    );
    assert!(matches!(bundle.to_files(), Err(Error::Config(_))));

    let mut bundle = panel();
    bundle.controls.page_mut(0).display.push(
        DataVar {
            unit: "°C".into(),
            ..DataVar::default()
        }
        .into(),
    );
    assert_eq!(
        check(&bundle),
        ["error: page 0, display control 1: unit \"°C\" must be ASCII and at most 11 bytes"]
    );
    assert!(matches!(bundle.to_files(), Err(Error::Controls(_))));
}

#[test]
fn reports_file_names_and_flash_overlaps() {
    let mut bundle = panel();
    bundle.files.insert("13touchfile.BIN".into(), vec![1]);
    bundle.files.insert("fonts/1.hzk".into(), vec![1]);
    bundle.files.insert("T5LCFG_800480.CFG".into(), vec![1]);
    assert_eq!(
        check(&bundle),
        [
            "error: T5LCFG_800480.CFG: the display would read it as well as the T5LCFG.CFG \
             the bundle writes",
            "error: 13touchfile.BIN: is the same file as 13TouchFile.bin to the display",
            "error: fonts/1.hzk: is not a file name for DWIN_SET: files go in DWIN_SET \
             itself, not in folders",
            // Both are file 13.
            "error: 13touchfile.BIN: overlaps 13TouchFile.bin in flash: a file takes 256 KB \
             per file number, so a large file runs into the next numbers",
        ]
    );

    let mut bundle = panel();
    bundle
        .files
        .insert("50_big.bin".into(), vec![0; 300 * 1024]);
    bundle.files.insert("51_next.bin".into(), vec![0; 10]);
    assert_eq!(
        check(&bundle),
        [
            "error: 51_next.bin: overlaps 50_big.bin in flash: a file takes 256 KB per file \
          number, so a large file runs into the next numbers"
        ]
    );
    assert!(matches!(
        bundle.to_files(),
        Err(Error::FlashOverlap { first, second }) if first == "50_big.bin" && second == "51_next.bin"
    ));

    let mut bundle = panel();
    bundle.files.insert("a:b".into(), vec![1]);
    assert!(matches!(bundle.to_files(), Err(Error::FileName { name, .. }) if name == "a:b"));
}

#[test]
fn reports_what_the_configuration_names_that_the_bundle_lacks() {
    let mut bundle = panel();
    bundle.config = Some(Config {
        background_icl: 33,
        music_wae: 49,
        load_22: true,
        picture_replace_icl: Some(50),
        sd_folder: Some("PANEL".into()),
        lcd: Some(Lcd {
            pclk_phs: 0,
            pclk_div: 6,
            h_w: 0,
            h_s: 0,
            h_d: 1024,
            h_e: 0,
            v_w: 0,
            v_s: 0,
            v_d: 600,
            v_e: 0,
            tcon_sel: 0,
        }),
        ..Config::default()
    });
    assert_eq!(
        check(&bundle),
        [
            "error: T5LCFG.CFG: background_icl 33 is not in the bundle",
            "warning: T5LCFG.CFG: music_wae 49 is not in the bundle; the display plays music \
             from the file 49 it holds",
            "warning: T5LCFG.CFG: picture_replace_icl 50 is not in the bundle; the display \
             uses the file 50 it holds",
            "warning: T5LCFG.CFG: load_22 is on, but the bundle has no file 22; the display \
             loads the file 22 it holds",
            "warning: T5LCFG.CFG: the LCD is 1024x600, so pages at 0° are 1024x600, not the \
             800x480 of the device",
            "warning: T5LCFG.CFG: sd_folder stores \"PANEL\" in the display's flash: later SD \
             downloads must come from a folder named PANEL, not DWIN_SET",
        ]
    );
}

#[test]
fn checks_icons_against_the_chip_and_the_screen() {
    let mut bundle = panel();
    bundle.icls.insert(32, icl(&[(0, 800, 480), (1, 400, 240)]));
    assert_eq!(
        check(&bundle),
        [
            "warning: 32.icl, icon 1: is 400x240, but the background of page 1 fills the \
          800x480 screen"
        ]
    );

    let mut bundle = panel();
    bundle.device.chip = Chip::Custom(8);
    let diagnostics = check(&bundle);
    assert_eq!(diagnostics.len(), 4, "{diagnostics:#?}");
    assert!(
        diagnostics[0].starts_with("error: 32.icl, icon 0: is a ")
            && diagnostics[0]
                .ends_with("-byte JPEG, over the 0 bytes the device's chip takes per icon"),
        "{}",
        diagnostics[0]
    );
}

#[test]
fn empty_libraries_cannot_be_written() {
    let mut bundle = panel();
    bundle.icls.insert(41, Icl::new());
    bundle.waes.insert(49, Wae::new(SampleFormat::Pcm16));
    assert_eq!(
        check(&bundle),
        ["error: 41.icl: has no icons", "error: 49.wae: has no clips"]
    );
    assert!(matches!(
        bundle.to_files(),
        Err(Error::Icl { file: 41, .. })
    ));
}

#[test]
fn reads_what_it_writes() {
    let dir = temp_dir("reads_what_it_writes");
    let bundle = panel();
    bundle.write(&dir).unwrap();
    std::fs::create_dir(dir.join("DWIN_SET").join("backup")).unwrap();
    assert_eq!(Bundle::read(&dir, bundle.device).unwrap(), bundle);
}

#[test]
fn reads_files_as_the_display_does() {
    let bundle = panel();
    let mut files = bundle.to_files().unwrap();
    for file in &mut files {
        file.name = match file.name.as_str() {
            "13TouchFile.bin" => "13触控配置文件.bin".into(),
            "32.icl" => "32_backgrounds.ICL".into(),
            "T5LCFG.CFG" => "T5LCFG_800480.CFG".into(),
            _ => continue,
        };
    }
    files.push(File::new("22_Config.bin", vec![1, 2]));
    files.push(File::new("300.icl", vec![3]));
    let read = Bundle::from_files(files, bundle.device).unwrap();
    assert_eq!(read.config, bundle.config);
    assert_eq!(read.controls, bundle.controls);
    assert_eq!(read.icls, bundle.icls);
    // ICL numbers are bytes, so 300.icl stays a plain file.
    assert_eq!(
        read.files.keys().collect::<Vec<_>>(),
        ["0_DWIN_ASC.HZK", "22_Config.bin", "300.icl"]
    );

    // Without file 13, the pages have no touch controls.
    let files = bundle
        .to_files()
        .unwrap()
        .into_iter()
        .filter(|file| file.name != "13TouchFile.bin");
    let read = Bundle::from_files(files, bundle.device).unwrap();
    assert!(read.controls.iter().all(|(_, page)| page.touch.is_empty()));
    assert_eq!(read.controls.get(0).unwrap().display.len(), 1);
}

#[test]
fn rejects_files_that_do_not_read() {
    let device = Device::new(SCREEN);
    let duplicate = Bundle::from_files(
        [File::new("32.icl", vec![]), File::new("32_x.ICL", vec![])],
        device,
    );
    assert!(matches!(
        duplicate,
        Err(Error::Duplicate { first, second }) if first == "32.icl" && second == "32_x.ICL"
    ));
    let configs = [
        File::new("T5LCFG.CFG", vec![]),
        File::new("t5lcfg2.cfg", vec![]),
    ];
    assert!(matches!(
        Bundle::from_files(configs, device),
        Err(Error::Duplicate { .. })
    ));
    assert!(matches!(
        Bundle::from_files([File::new("40.icl", b"DGUS_2".to_vec())], device),
        Err(Error::Icl { file: 40, .. })
    ));
    assert!(matches!(
        Bundle::from_files([File::new("T5LCFG.CFG", vec![0; 4])], device),
        Err(Error::Config(_))
    ));
    assert!(matches!(
        Bundle::read(temp_dir("rejects_files_that_do_not_read"), device),
        Err(Error::Io { .. })
    ));
}
