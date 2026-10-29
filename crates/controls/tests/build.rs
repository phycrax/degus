use degus_controls::{
    Align, Area, BarDataType, BarDirection, Controls, CursorColor, DataInput, DataType, DataVar,
    Display, Encoding, Error, IconVar, InputDataType, KeyCode, KeyTarget, Keyboard, LayerMode,
    PageControls, Point, ProgressBar, TextAlign, TextVar, Touch, TouchAction, VerticalAlign,
};
use hex_literal::hex;

/// Records start after the 16-byte header and the 4092-entry page table.
const RECORDS: usize = 0x4000;

/// Page table entry of `page`: record count and offset.
fn entry(file: &[u8], page: usize) -> (u8, usize) {
    let e = &file[16 + 4 * page..][..4];
    (e[0], u32::from_be_bytes([0, e[1], e[2], e[3]]) as usize)
}

fn record(file: &[u8], offset: usize) -> &[u8] {
    &file[offset..offset + 32]
}

// The expected records below are taken from a DGUS V7.651 project's output.

#[test]
fn key_code_touch_matches_editor_output() {
    let mut controls = Controls::new();
    controls.page_mut(0).touch.push(Touch::new(
        Area::new(1648, 848, 1798, 998),
        KeyCode::new(0xBA00, 2),
    ));
    let touch = controls.build().unwrap().touch;
    assert_eq!(
        touch,
        hex!(
            "0000 0670 0350 0706 03e6 ff00 ff00 fe05"
            "feba 0000 0002 0000 0000 0000 0000 0000"
            "ffff"
        )
    );
}

// No editor output with a data input or a basic touch is at hand to compare
// against.
#[test]
fn data_input_and_key_records_follow_the_editor_layout() {
    let mut controls = Controls::new();
    let page = controls.page_mut(1);
    page.touch.push(Touch::new(
        Area::new(100, 50, 300, 90),
        DataInput {
            vp: 0x1200,
            data_type: InputDataType::I32,
            integer_digits: 3,
            decimal_digits: 2,
            position: Point::new(290, 60),
            color: 0xF800,
            font_size: 24,
            cursor_color: CursorColor::White,
            keyboard: Keyboard::Page {
                page: 30,
                area: Area::new(0, 0, 400, 300),
                position: Point::new(200, 150),
            },
            limits: Some(-500..=10000),
            load_vp: Some(0x1210),
            load_value: 1,
            ..DataInput::default()
        },
    ));
    page.touch.push(Touch::new(
        Area::new(10, 400, 60, 450),
        TouchAction::Basic(0x0031),
    ));
    // A new editor control, except that it is not masked.
    page.touch
        .push(Touch::new(Area::default(), DataInput::default()));
    let touch = controls.build().unwrap().touch;

    assert_eq!(
        touch,
        hex!(
            "0001 0064 0032 012c 005a ff00 ff00 fe00"
            "fe12 0001 0302 0122 003c f800 0018 0101"
            "fe01 001e 0000 0000 0190 012c 00c8 0096"
            "feff ffff fe0c 0000 2710 5a12 1000 0100"

            "0001 000a 0190 003c 01c2 ff00 ff00 0031"

            "0001 0000 0000 0000 0000 ff00 ff00 fe00"
            "fe00 0000 0800 0000 0000 0000 0010 0001"
            "fe00 ffff 0000 0000 0000 0000 0000 0000"
            "fe00 0000 0000 0000 0000 0000 0000 0000"
            "ffff"
        )
    );
}

#[test]
fn display_records_match_editor_output() {
    let mut controls = Controls::new();
    controls.page_mut(0).display.push(
        IconVar {
            vp: 0x1000,
            position: Point::new(740, 439),
            val_max: 2,
            icon_min: 10100,
            icon_max: 10102,
            icon_lib: 48,
            ..IconVar::default()
        }
        .into(),
    );
    controls.page_mut(2).display.push(
        TextVar {
            vp: 0xF000,
            area: Area::new(726, 467, 1194, 613),
            length: 2048,
            font_width: 16,
            font_height: 16,
            align: TextAlign::Center,
            ..TextVar::default()
        }
        .into(),
    );
    controls.page_mut(12).display.push(
        DataVar {
            vp: 0x120A,
            position: Point::new(632, 87),
            color: 0xF800,
            font_size: 24,
            integer_digits: 3,
            data_type: DataType::U16,
            ..DataVar::default()
        }
        .into(),
    );
    let file = controls.build().unwrap().display;

    assert_eq!(entry(&file, 0), (1, RECORDS));
    assert_eq!(entry(&file, 2), (1, RECORDS + 32));
    assert_eq!(entry(&file, 12), (1, RECORDS + 64));
    assert_eq!(
        record(&file, RECORDS),
        hex!(
            "5a00 ffff 000a 1000 02e4 01b7 0000 0002"
            "2774 2776 3000 0000 003f 0000 0000 0000"
        )
    );
    assert_eq!(
        record(&file, RECORDS + 32),
        hex!(
            "5a11 ffff 000d f000 02d6 01d3 0000 02d6"
            "01d3 04aa 0265 0800 0000 1010 4000 0000"
        )
    );
    assert_eq!(
        record(&file, RECORDS + 64),
        hex!(
            "5a10 ffff 000d 120a 0278 0057 f800 0018"
            "0003 0005 0000 0000 0000 0000 0000 0000"
        )
    );
}

// No editor output with a progress bar is at hand to compare against.
#[test]
fn progress_bar_records_follow_the_editor_layout() {
    let mut controls = Controls::new();
    let page = controls.page_mut(0);
    page.display.push(
        ProgressBar {
            vp: 0x1100,
            area: Area::new(100, 200, 500, 240),
            val_min: -50,
            val_max: 150,
            direction: BarDirection::Up,
            bar_color: 0x07E0,
            border_color: 0xFFFF,
            background_color: 0x2104,
            show_border: true,
            fill_background: true,
            rate_vp: Some(0x1101),
            ..ProgressBar::default()
        }
        .into(),
    );
    // A new editor control: no border, no background, no rate.
    page.display.push(ProgressBar::default().into());
    let file = controls.build().unwrap().display;

    assert_eq!(
        record(&file, RECORDS),
        hex!(
            "5a23 ffff 000d 1100 0064 00c8 01f4 00f0"
            "ffff 07e0 2104 0096 ffce 8002 0000 1101"
        )
    );
    assert_eq!(
        record(&file, RECORDS + 32),
        hex!(
            "5a23 ffff 000d 0000 0000 0000 0000 0000"
            "0000 0000 0000 0000 0000 0300 0000 0000"
        )
    );
}

fn icon(vp: u16) -> Display {
    IconVar {
        vp,
        ..IconVar::default()
    }
    .into()
}

#[test]
fn display_file_has_header_page_table_and_end_marker() {
    let mut controls = Controls::new();
    controls.page_mut(3).display.extend([icon(1), icon(2)]);
    controls.page_mut(7).display.push(icon(3));
    // Pages without controls still raise the highest page ID.
    controls.page_mut(40);
    let file = controls.build().unwrap().display;

    assert_eq!(file[..16], hex!("1444 4755 535f 3210 0028 0000 0000 0000"));
    assert_eq!(file.len(), RECORDS + 3 * 32 + 32);
    // Pages without records carry the running offset.
    assert_eq!(entry(&file, 0), (0, RECORDS));
    assert_eq!(entry(&file, 3), (2, RECORDS));
    assert_eq!(entry(&file, 4), (0, RECORDS + 64));
    assert_eq!(entry(&file, 7), (1, RECORDS + 64));
    assert_eq!(entry(&file, 4091), (0, RECORDS + 96));
    // Records keep their order within a page.
    assert_eq!(record(&file, RECORDS + 32)[6..8], [0, 2]);
    assert!(file[RECORDS + 96..].iter().all(|&b| b == 0xFF));
}

#[test]
fn empty_project_builds_valid_files() {
    let built = Controls::new().build().unwrap();
    assert_eq!(built.touch, [0xFF, 0xFF]);
    assert_eq!(built.display.len(), RECORDS + 32);
}

#[test]
fn writes_every_field() {
    let mut controls = Controls::new();
    let page = controls.page_mut(5);
    page.touch.push(Touch {
        area: Area::new(1, 2, 300, 400),
        next_page: Some(7),
        pressed_page: Some(9),
        action: KeyCode {
            vp: 0x2000,
            target: KeyTarget::Bit(13),
            value: 0x00F1,
            hold_time: 12,
            upload: false,
        }
        .into(),
    });
    page.touch.push(Touch::new(
        Area::new(5, 6, 7, 8),
        KeyCode {
            target: KeyTarget::LowByte,
            ..KeyCode::new(0x2001, 3)
        },
    ));
    page.touch.push(Touch {
        area: Area::new(10, 20, 110, 60),
        next_page: Some(2),
        pressed_page: Some(4),
        action: DataInput {
            vp: 0x2002,
            data_type: InputDataType::Float,
            integer_digits: 5,
            decimal_digits: 3,
            position: Point::new(100, 30),
            color: 0x07E0,
            font: 4,
            font_size: 32,
            cursor_color: CursorColor::White,
            masked: true,
            keyboard: Keyboard::OnPage,
            limits: None,
            load_vp: None,
            load_value: 0x1234,
            keyboard_transparency: 127,
            upload: false,
        }
        .into(),
    });
    page.touch.push(Touch::new(
        Area::new(10, 70, 110, 110),
        DataInput {
            vp: 0x2006,
            data_type: InputDataType::LowByte,
            keyboard: Keyboard::Page {
                page: 11,
                area: Area::new(1, 2, 3, 4),
                position: Point::new(5, 6),
            },
            limits: Some(-7..=-3),
            load_vp: Some(0x2007),
            ..DataInput::default()
        },
    ));
    page.touch.push(Touch {
        pressed_page: Some(3),
        ..Touch::new(Area::new(0, 0, 50, 50), TouchAction::Basic(0x00F2))
    });
    page.display.push(
        IconVar {
            vp: 0x3000,
            sp: Some(0x5000),
            popup_menu: true,
            position: Point::new(10, 20),
            val_min: 1,
            val_max: 4,
            icon_min: 100,
            icon_max: 103,
            icon_lib: 40,
            show_background: true,
            layer_mode: LayerMode::OverlayMode2,
            icon_brightness: 200,
            background_brightness: 100,
            filter: 7,
        }
        .into(),
    );
    page.display.push(
        DataVar {
            vp: 0x3100,
            sp: Some(0x5010),
            popup_menu: true,
            position: Point::new(30, 40),
            color: 0x07E0,
            font: 2,
            font_size: 32,
            align: Align::Center,
            auto_spacing: true,
            leading_zeros: true,
            integer_digits: 4,
            decimal_digits: 2,
            data_type: DataType::F32,
            unit: "mJ/cm2 Hz".into(),
        }
        .into(),
    );
    controls.page_mut(9).display.push(
        TextVar {
            vp: 0x3200,
            sp: Some(0x5020),
            popup_menu: false,
            area: Area::new(50, 60, 500, 120),
            color: 0x001F,
            length: 40,
            encoding: Encoding::Unicode,
            ascii_font: 1,
            font: 3,
            font_width: 24,
            font_height: 48,
            align: TextAlign::RightToLeft,
            vertical_align: VerticalAlign::Center,
            auto_spacing: true,
            char_spacing: 2,
            line_spacing: 4,
        }
        .into(),
    );
    controls.page_mut(9).display.push(
        ProgressBar {
            vp: 0x3300,
            sp: Some(0x5030),
            area: Area::new(20, 30, 40, 330),
            val_min: -1000,
            val_max: 1000,
            data_type: BarDataType::HighByte,
            direction: BarDirection::Down,
            bar_color: 0xF800,
            border_color: 0x001F,
            background_color: 0x8410,
            show_border: false,
            fill_background: true,
            rate_vp: Some(0x3301),
        }
        .into(),
    );

    let built = controls.build().unwrap();
    assert_eq!(
        built.touch,
        hex!(
            // Key code without auto-upload (FD 05) on bit 13 (1D).
            "0005 0001 0002 012c 0190 0007 0009 fd05"
            "fe20 001d 00f1 0c00 0000 0000 0000 0000"
            "0005 0005 0006 0007 0008 ff00 ff00 fe05"
            "fe20 0102 0003 0000 0000 0000 0000 0000"

            "0005 000a 0014 006e 003c 0002 0004 fd00"
            "fe20 0205 0503 0064 001e 07e0 0420 0100"
            "fe00 ffff 0000 0000 0000 0000 0000 0000"
            "fe00 0000 0000 0000 0000 0000 0012 347f"

            "0005 000a 0046 006e 006e ff00 ff00 fe00"
            "fe20 0603 0800 0000 0000 0000 0010 0001"
            "fe01 000b 0001 0002 0003 0004 0005 0006"
            "feff ffff fff9 ffff fffd 5a20 0700 0000"

            "0005 0000 0000 0032 0032 ff00 0003 00f2"
            "ffff"
        )
    );

    let file = &built.display;
    assert_eq!(entry(file, 5), (2, RECORDS));
    assert_eq!(entry(file, 9), (2, RECORDS + 64));
    assert_eq!(
        file[RECORDS..RECORDS + 128],
        hex!(
            "5f00 5000 000a 3000 000a 0014 0001 0004"
            "0064 0067 2801 03c8 6407 0000 0000 0000"

            "5f10 5010 000d 3100 001e 0028 07e0 0220"
            "c204 0207 096d 4a2f 636d 3220 487a 0000"

            "5a11 5020 000d 3200 0032 003c 001f 0032"
            "003c 01f4 0078 0028 0103 1830 f502 0400"

            "5a23 5030 000d 3300 0014 001e 0028 014a"
            "001f f800 8410 03e8 fc18 8103 0100 3301"
        )
    );
}

#[test]
fn rejects_what_the_files_cannot_hold() {
    let mut controls = Controls::new();
    controls.page_mut(4092);
    assert!(matches!(
        controls.build(),
        Err(Error::PageId { page: 4092 })
    ));

    let mut controls = Controls::new();
    controls.insert(
        1,
        PageControls {
            touch: Vec::new(),
            display: (0..256).map(icon).collect(),
        },
    );
    assert!(matches!(
        controls.build(),
        Err(Error::TooManyDisplays {
            page: 1,
            count: 256
        })
    ));

    for unit in ["twelve bytes", "°C"] {
        let mut controls = Controls::new();
        controls.page_mut(2).display.push(
            DataVar {
                unit: unit.into(),
                ..DataVar::default()
            }
            .into(),
        );
        assert!(matches!(controls.build(), Err(Error::Unit { page: 2, .. })));
    }

    let mut controls = Controls::new();
    controls.page_mut(3).touch.push(Touch::new(
        Area::default(),
        KeyCode {
            target: KeyTarget::Bit(16),
            ..KeyCode::new(0, 0)
        },
    ));
    assert!(matches!(
        controls.build(),
        Err(Error::BitIndex { page: 3, bit: 16 })
    ));

    // Basic touch codes starting with FE or FD would read as other controls.
    for code in [0xFE31, 0xFD00] {
        let mut controls = Controls::new();
        controls
            .page_mut(6)
            .touch
            .push(Touch::new(Area::default(), TouchAction::Basic(code)));
        assert!(matches!(
            controls.build(),
            Err(Error::BasicKey { page: 6, code: c }) if c == code
        ));
    }
}
