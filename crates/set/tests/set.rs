use std::ops::RangeInclusive;
use std::path::PathBuf;

use degus_set::config::{Config, Lcd};
use degus_set::controls::{
    Area, DataInput, DataType, DataVar, Encoding, IconVar, KeyCode, KeyTarget, Keyboard, Point,
    ProgressBar, TextVar, Touch, TouchAction, key,
};
use degus_set::font::{Bitmap, FONT0_LEN, Font0};
use degus_set::icl::image::{DynamicImage, Rgb, RgbImage};
use degus_set::icl::{Chip, Icl, IclBuilder, IclOptions, IconSource};
use degus_set::wae::{SampleFormat, Wae};
use degus_set::{Device, DwinSet, Error, File, Size};

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

/// A font 0 with a pixel in every glyph, so that every character is drawn.
fn font0() -> Font0 {
    let mut font = Font0::new();
    for width in 4..=64 {
        for code in 0..=127 {
            let mut glyph = Bitmap::new(width.into(), 2 * u16::from(width));
            glyph.set(0, 0, true);
            font.set(width, code, glyph).unwrap();
        }
    }
    font
}

fn wae(ids: &[u16]) -> Wae {
    let mut wae = Wae::new(SampleFormat::Pcm16);
    for &id in ids {
        wae.clips.insert(id, vec![0, 100, -100, 0]);
    }
    wae
}

/// A set without problems: a home page with a button, a page-switch
/// button and an icon, and a settings page with a data input and its keys.
fn panel() -> DwinSet {
    let mut set = DwinSet::new(Device::new(SCREEN));
    set.config = Some(Config {
        music_wae: 48,
        ..Config::default()
    });
    set.icls.insert(32, icl(&[(0, 800, 480), (1, 800, 480)]));
    set.icls.insert(40, icl(&[(0, 64, 64), (1, 64, 64)]));
    set.waes.insert(48, wae(&[0]));
    set.font0 = Some(font0());

    let home = set.controls.page_mut(0);
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

    let settings = set.controls.page_mut(1);
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
    set
}

/// The check's diagnostics as lines.
fn check(set: &DwinSet) -> Vec<String> {
    set.check().iter().map(ToString::to_string).collect()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_sound_set_has_no_diagnostics() {
    assert_eq!(check(&panel()), Vec::<String>::new());
}

#[test]
fn writes_the_folder_in_flash_order() {
    let dir = temp_dir("writes_the_folder_in_flash_order");
    let stale = dir.join("DWIN_SET").join("99_stale.bin");
    std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
    std::fs::write(&stale, b"left over").unwrap();

    let set = panel();
    let written = set.write(&dir).unwrap();
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
    assert_eq!(written.files, set.to_files().unwrap());

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
    assert_eq!(icl.bytes, set.icls[&32].to_bytes(32).unwrap());
    assert_eq!(icl.number(), Some(32));
}

#[test]
fn write_refuses_a_set_with_errors() {
    let dir = temp_dir("write_refuses_a_set_with_errors");
    let mut set = panel();
    set.controls.page_mut(0).touch.push(Touch::new(
        Area::sized(750, 0, 100, 50),
        KeyCode::new(0x1001, 1),
    ));
    let error = set.write(&dir).unwrap_err();
    assert!(matches!(&error, Error::Check(diagnostics) if diagnostics.len() == 1));
    assert_eq!(
        error.to_string(),
        "the set has 1 error\n\
         error: page 0, touch control 2: area (750, 0) 100x50 extends past the 800x480 screen"
    );
    // So that `main` returning the error prints it readably.
    assert_eq!(format!("{error:?}"), error.to_string());
    assert!(!dir.join("DWIN_SET").exists());
}

#[test]
fn reports_controls_off_the_screen() {
    let mut set = panel();
    let home = set.controls.page_mut(0);
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
        check(&set),
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
fn reports_what_controls_name_that_the_set_lacks() {
    let mut set = panel();
    let home = set.controls.page_mut(0);
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
    set.controls.page_mut(2).touch.push(Touch::new(
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
        check(&set),
        [
            "warning: page 2: has no background image in 32.icl",
            "warning: page 0, touch control 2: next_page 7 has no background image in 32.icl",
            "error: page 0, touch control 2: pressed_page 5000 is not below 4092",
            "warning: page 0, touch control 3: its keyboard is on its own page, which has no \
             keyboard keys",
            "error: page 0, display control 1: icon_lib 41 is not in the set",
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
    let mut set = panel();
    set.controls.page_mut(1).touch[0] = Touch::new(
        Area::sized(0, 0, 10, 10),
        KeyCode {
            target: KeyTarget::Bit(16),
            ..KeyCode::new(0x1001, 1)
        },
    );
    set.config.as_mut().unwrap().brightness = 101;
    assert_eq!(
        check(&set),
        [
            "error: T5LCFG.CFG: brightness is 101, not in 0-100",
            "error: page 1, touch control 0: target bit 16 is not in 0-15",
        ]
    );
    assert!(matches!(set.to_files(), Err(Error::Config(_))));

    let mut set = panel();
    set.controls.page_mut(0).display.push(
        DataVar {
            unit: "°C".into(),
            ..DataVar::default()
        }
        .into(),
    );
    assert_eq!(
        check(&set),
        ["error: page 0, display control 1: unit \"°C\" must be ASCII and at most 11 bytes"]
    );
    assert!(matches!(set.to_files(), Err(Error::Controls(_))));
}

#[test]
fn reports_file_names_and_flash_overlaps() {
    let mut set = panel();
    set.files.insert("13touchfile.BIN".into(), vec![1]);
    set.files.insert("fonts/1.hzk".into(), vec![1]);
    set.files.insert("T5LCFG_800480.CFG".into(), vec![1]);
    assert_eq!(
        check(&set),
        [
            "error: T5LCFG_800480.CFG: the display would read it as well as the T5LCFG.CFG \
             the set writes",
            "error: 13touchfile.BIN: is the same file as 13TouchFile.bin to the display",
            "error: fonts/1.hzk: is not a file name for DWIN_SET: files go in DWIN_SET \
             itself, not in folders",
            // Both are file 13.
            "error: 13touchfile.BIN: overlaps 13TouchFile.bin in flash: a file takes 256 KB \
             per file number, so a large file runs into the next numbers",
        ]
    );

    let mut set = panel();
    set.files.insert("50_big.bin".into(), vec![0; 300 * 1024]);
    set.files.insert("51_next.bin".into(), vec![0; 10]);
    assert_eq!(
        check(&set),
        [
            "error: 51_next.bin: overlaps 50_big.bin in flash: a file takes 256 KB per file \
          number, so a large file runs into the next numbers"
        ]
    );
    assert!(matches!(
        set.to_files(),
        Err(Error::FlashOverlap { first, second }) if first == "50_big.bin" && second == "51_next.bin"
    ));

    let mut set = panel();
    set.files.insert("a:b".into(), vec![1]);
    assert!(matches!(set.to_files(), Err(Error::FileName { name, .. }) if name == "a:b"));
}

#[test]
fn checks_files_against_the_end_of_flash() {
    // Without a flash size, no file is past its end.
    let mut set = panel();
    set.files.insert("200_far.bin".into(), vec![1]);
    assert_eq!(check(&set), Vec::<String>::new());
    set.to_files().unwrap();

    // 16 MB holds file numbers 0 to 63, so the last may fill 63.
    let mut set = panel();
    set.device.flash_mb = Some(16);
    set.files.insert("63_last.bin".into(), vec![0; 256 * 1024]);
    assert_eq!(check(&set), Vec::<String>::new());
    set.to_files().unwrap();

    set.files
        .insert("63_last.bin".into(), vec![0; 256 * 1024 + 1]);
    set.files.insert("70_far.bin".into(), vec![1]);
    assert_eq!(
        check(&set),
        [
            "error: 63_last.bin: ends in file number 64, past the device's 16 MB flash, which \
             ends in file number 63",
            "error: 70_far.bin: ends in file number 70, past the device's 16 MB flash, which \
             ends in file number 63",
        ]
    );
    let error = set.to_files().unwrap_err();
    assert!(
        matches!(&error, Error::PastFlash { name, flash_mb: 16 } if name == "63_last.bin"),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "63_last.bin ends past the device's 16 MB flash: a file takes 256 KB per file number, \
         from its own number on"
    );
}

#[test]
fn reports_what_the_configuration_names_that_the_set_lacks() {
    let mut set = panel();
    set.config = Some(Config {
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
        check(&set),
        [
            "error: T5LCFG.CFG: background_icl 33 is not in the set",
            "warning: T5LCFG.CFG: music_wae 49 is not in the set; the display plays music \
             from the file 49 it holds",
            "warning: T5LCFG.CFG: picture_replace_icl 50 is not in the set; the display \
             uses the file 50 it holds",
            "warning: T5LCFG.CFG: load_22 is on, but the set has no file 22; the display \
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
    let mut set = panel();
    set.icls.insert(32, icl(&[(0, 800, 480), (1, 400, 240)]));
    assert_eq!(
        check(&set),
        [
            "warning: 32.icl, icon 1: is 400x240, but the background of page 1 fills the \
          800x480 screen"
        ]
    );

    let mut set = panel();
    set.device.chip = Chip::Custom(8);
    let diagnostics = check(&set);
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
    let mut set = panel();
    set.icls.insert(41, Icl::new());
    set.waes.insert(49, Wae::new(SampleFormat::Pcm16));
    assert_eq!(
        check(&set),
        ["error: 41.icl: has no icons", "error: 49.wae: has no clips"]
    );
    assert!(matches!(set.to_files(), Err(Error::Icl { file: 41, .. })));
}

#[test]
fn reads_what_it_writes() {
    let dir = temp_dir("reads_what_it_writes");
    let set = panel();
    set.write(&dir).unwrap();
    std::fs::create_dir(dir.join("DWIN_SET").join("backup")).unwrap();
    assert_eq!(DwinSet::read(&dir, set.device).unwrap(), set);
}

#[test]
fn reads_files_as_the_display_does() {
    let set = panel();
    let mut files = set.to_files().unwrap();
    for file in &mut files {
        file.name = match file.name.as_str() {
            "13TouchFile.bin" => "13触控配置文件.bin".into(),
            "32.icl" => "32_backgrounds.ICL".into(),
            "T5LCFG.CFG" => "T5LCFG_800480.CFG".into(),
            "0_DWIN_ASC.HZK" => "00_ascii.DZK".into(),
            _ => continue,
        };
    }
    files.push(File::new("22_Config.bin", vec![1, 2]));
    files.push(File::new("300.icl", vec![3]));
    let read = DwinSet::from_files(files, set.device).unwrap();
    assert_eq!(read.config, set.config);
    assert_eq!(read.controls, set.controls);
    assert_eq!(read.icls, set.icls);
    assert_eq!(read.font0, set.font0);
    // ICL numbers are bytes, so 300.icl stays a plain file.
    assert_eq!(
        read.files.keys().collect::<Vec<_>>(),
        ["22_Config.bin", "300.icl"]
    );

    // Without file 13, the pages have no touch controls.
    let files = set
        .to_files()
        .unwrap()
        .into_iter()
        .filter(|file| file.name != "13TouchFile.bin");
    let read = DwinSet::from_files(files, set.device).unwrap();
    assert!(read.controls.iter().all(|(_, page)| page.touch.is_empty()));
    assert_eq!(read.controls.get(0).unwrap().display.len(), 1);
}

#[test]
fn reads_font_0_by_its_number_and_length() {
    let device = Device::new(SCREEN);
    let font = font0();
    let read = DwinSet::from_files([File::new("0.bin", font.to_bytes())], device).unwrap();
    assert_eq!(read.font0.as_ref(), Some(&font));
    assert!(read.files.is_empty());
    // Written back under DWIN's name, first: it is file 0.
    assert_eq!(read.to_files().unwrap()[0].name, "0_DWIN_ASC.HZK");

    // Font 0 has no header, so a file 0 of another length stays as it is,
    // and the check says why.
    let short = DwinSet::from_files([File::new("0_DWIN_ASC.HZK", vec![0; 1000])], device).unwrap();
    assert_eq!(short.font0, None);
    assert_eq!(short.files["0_DWIN_ASC.HZK"], vec![0; 1000]);
    assert_eq!(
        check(&short),
        [format!(
            "warning: 0_DWIN_ASC.HZK: the display reads file 0 as font 0, but this is 1000 \
             bytes, not the {FONT0_LEN} of font 0"
        )]
    );

    // Other files numbered 0 are not fonts.
    let logo = DwinSet::from_files([File::new("0_logo.png", vec![1])], device).unwrap();
    assert_eq!(logo.font0, None);
    assert_eq!(check(&logo), Vec::<String>::new());

    let two = [
        File::new("0_DWIN_ASC.HZK", font.to_bytes()),
        File::new("0.bin", vec![1]),
    ];
    assert!(matches!(
        DwinSet::from_files(two, device),
        Err(Error::Duplicate { .. })
    ));
}

#[test]
fn checks_the_sizes_controls_take_from_font_0() {
    let mut set = panel();
    let text = |encoding, ascii_font, font, font_width| TextVar {
        area: Area::sized(10, 300, 200, 40),
        encoding,
        ascii_font,
        font,
        font_width,
        font_height: font_width,
        ..TextVar::default()
    };
    let home = set.controls.page_mut(0);
    home.display.push(
        DataVar {
            position: Point::new(10, 10),
            font_size: 3,
            ..DataVar::default()
        }
        .into(),
    );
    // Only font 0 is checked.
    home.display.push(
        DataVar {
            position: Point::new(10, 10),
            font: 2,
            font_size: 3,
            ..DataVar::default()
        }
        .into(),
    );
    home.display.push(text(Encoding::Ascii, 0, 0, 65).into());
    home.display.push(text(Encoding::Ascii, 0, 3, 65).into());
    // GB2312 draws ASCII at half the width: 65 here, 12 next.
    home.display.push(text(Encoding::Gb2312, 0, 5, 130).into());
    home.display.push(text(Encoding::Gb2312, 0, 5, 24).into());
    home.display.push(text(Encoding::Unicode, 0, 0, 16).into());
    home.display.push(text(Encoding::Gbk, 0, 0, 16).into());
    if let TouchAction::DataInput(input) = &mut set.controls.page_mut(1).touch[0].action {
        input.font_size = 70;
    }

    assert_eq!(
        check(&set),
        [
            "error: page 0, display control 1: font_size 3 is not a width of font 0, which \
             has 4 to 64",
            "error: page 0, display control 3: font_width 65 is not a width of font 0, which \
             has 4 to 64",
            "error: page 0, display control 5: font_width 130 draws ASCII from font 0 at half \
             of it, 65, which is not a width of font 0, which has 4 to 64",
            "warning: page 0, display control 7: font is 0, but font 0 holds only ASCII: it \
             has no Unicode characters",
            "warning: page 0, display control 8: font is 0, but font 0 holds only ASCII: it \
             has no GBK characters",
            "error: page 1, touch control 0: font_size 70 is not a width of font 0, which has \
             4 to 64",
        ]
    );
}

#[test]
fn checks_that_the_set_s_font_0_draws_what_numbers_need() {
    let mut set = panel();
    let font = set.font0.as_mut().unwrap();
    for code in *b"7-g" {
        font.set(12, code, Bitmap::new(12, 24)).unwrap();
    }
    font.set(16, b'*', Bitmap::new(16, 32)).unwrap();
    let number = |data_type, decimal_digits, unit: &str, font_size| DataVar {
        position: Point::new(10, 10),
        data_type,
        decimal_digits,
        unit: unit.into(),
        font_size,
        ..DataVar::default()
    };
    let home = set.controls.page_mut(0);
    home.display.push(number(DataType::U16, 0, "kg", 12).into());
    home.display.push(number(DataType::I16, 1, "", 12).into());
    home.display.push(number(DataType::I16, 1, "kg", 13).into());
    if let TouchAction::DataInput(input) = &mut set.controls.page_mut(1).touch[0].action {
        input.masked = true;
    }

    assert_eq!(
        check(&set),
        [
            "warning: page 0, display control 1: the set's font 0 draws nothing for '7', \
             'g' at width 12",
            "warning: page 0, display control 2: the set's font 0 draws nothing for '-', \
             '7' at width 12",
            "warning: page 1, touch control 0: the set's font 0 draws nothing for '*' at \
             width 16",
        ]
    );

    // The font 0 a display comes with has every character.
    set.font0 = None;
    assert_eq!(check(&set), Vec::<String>::new());
}

#[test]
fn font_0_takes_files_0_to_11() {
    let mut set = panel();
    set.files.insert("11_data.bin".into(), vec![1]);
    set.files.insert("12_data.bin".into(), vec![1]);
    let diagnostics = check(&set);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(
        diagnostics[0].starts_with("error: 11_data.bin: overlaps 0_DWIN_ASC.HZK in flash"),
        "{diagnostics:#?}"
    );
    assert!(matches!(
        set.to_files(),
        Err(Error::FlashOverlap { first, second })
            if first == "0_DWIN_ASC.HZK" && second == "11_data.bin"
    ));
}

#[test]
fn rejects_files_that_do_not_read() {
    let device = Device::new(SCREEN);
    let duplicate = DwinSet::from_files(
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
        DwinSet::from_files(configs, device),
        Err(Error::Duplicate { .. })
    ));
    assert!(matches!(
        DwinSet::from_files([File::new("40.icl", b"DGUS_2".to_vec())], device),
        Err(Error::Icl { file: 40, .. })
    ));
    assert!(matches!(
        DwinSet::from_files([File::new("T5LCFG.CFG", vec![0; 4])], device),
        Err(Error::Config(_))
    ));
    assert!(matches!(
        DwinSet::read(temp_dir("rejects_files_that_do_not_read"), device),
        Err(Error::Io { .. })
    ));
}
