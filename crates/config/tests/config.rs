use std::assert_matches;

use degus_config::{
    Audio, Buzzer, ClockOut, Config, Error, FILE_LEN, Lcd, NandSize, Rotation, TouchPanel,
};
use hex_literal::hex;

/// `bytes`, zero-padded to a built file's length.
fn padded(bytes: &[u8]) -> Vec<u8> {
    let mut file = bytes.to_vec();
    file.resize(FILE_LEN, 0);
    file
}

#[test]
fn default_config_writes_only_the_system_settings() {
    assert_eq!(
        Config::default().to_bytes().unwrap(),
        padded(&hex!("54354c4331 18 00 00 20 28 001c 64 00 03e8"))
    );
}

#[test]
fn system_settings_match_the_template_shipped_with_the_dgus_editor() {
    // DGUS V7.651 `Config/T5LCFG.CFG`: 36 bytes, with stale 800x480 LCD
    // timing behind a disabled marker after the system settings.
    let template = hex!(
        "5435 4c43 3138 0000 2028 001c 4000 0000"
        "0000 0106 1e10 0320 d203 1401 e00c 0000"
        "000f 1000"
    );
    let file = Config {
        load_22: true,
        brightness: 0x40,
        standby_delay: 0,
        ..Config::default()
    }
    .to_bytes()
    .unwrap();
    assert_eq!(file[..0x10], template[..0x10]);
}

/// A configuration with every block enabled.
fn every_block() -> Config {
    Config {
        crc: true,
        audio: Audio::Music,
        load_22: true,
        auto_upload: false,
        touch_sound: false,
        standby: true,
        rotation: Rotation::Deg270,
        pwm_backlight: true,
        nand_flash: Some(NandSize::Gbit4),
        format_nand_flash: true,
        erase_internal_flash: true,
        adc_16bit: true,
        encrypt_8051: true,
        run_os_during_download: true,
        music_wae: 56,
        background_icl: 40,
        touch_report_div: 4,
        baud: 9600,
        brightness: 80,
        standby_brightness: 10,
        standby_delay: 0x1234,
        lcd: Some(Lcd {
            pclk_phs: 1,
            pclk_div: 6,
            h_w: 0x1E,
            h_s: 0x10,
            h_d: 800,
            h_e: 0xD2,
            v_w: 3,
            v_s: 0x14,
            v_d: 480,
            v_e: 0x0C,
            tcon_sel: 0,
        }),
        touch_panel: Some(TouchPanel {
            mode: 0x15,
            sense: 0x14,
            freq: 2,
        }),
        clock_out: Some(ClockOut {
            enabled: false,
            div: 100,
        }),
        buzzer: Some(Buzzer::default()),
        init_file: Some(15),
        sysclk_adjust: Some(-2),
        picture_replace_icl: Some(33),
        sd_folder: Some("RGZN_SET".into()),
        backlight_delay: Some(0x0A),
    }
}

#[test]
fn writes_every_block() {
    let file = every_block().to_bytes().unwrap();
    assert_eq!(file.len(), FILE_LEN);
    assert_eq!(
        file[..0x50],
        hex!(
            "54354c4331 e7 ff 38 28 04 0150 50 0a 1234"
            "5aa5 01 06 1e 10 0320 d2 03 14 01e0 0c 00 00"
            "5a 15 14 02 5a 00 64 5a 6e 0bb8 00f0 0a 5a 0f"
            "5a fe 5a 21 000000000000000000000000"
            "5aa5 08 52475a4e5f534554 0000000000"
        )
    );
    assert_eq!(file[0x70..0x72], [0x5A, 0x0A]);
    assert!(file[0x72..].iter().all(|&b| b == 0));
}

#[test]
fn writes_the_nearest_baud_divisor() {
    let divisor = |baud: u32| {
        Config {
            baud,
            ..Config::default()
        }
        .to_bytes()
        .map(|file| u16::from_be_bytes([file[0x0A], file[0x0B]]))
    };
    assert_eq!(divisor(115_200).unwrap(), 0x1C);
    assert_eq!(divisor(9600).unwrap(), 0x0150);
    assert_eq!(divisor(460_800).unwrap(), 7);
    // 248123 baud, 0.75 % off.
    assert_eq!(divisor(250_000).unwrap(), 13);
    assert_matches!(divisor(0), Err(Error::Baud(0)));
    // Below the slowest rate, divisor 0x3FF.
    assert_matches!(divisor(3000), Err(Error::Baud(3000)));
    // Divisor 3.5: 4 is 12.5 % off.
    assert_matches!(divisor(921_600), Err(Error::Baud(921_600)));
}

#[test]
fn reads_the_baud_a_divisor_gives() {
    let mut file = Config::default().to_bytes().unwrap();
    for (divisor, baud) in [(0x1C, 115_200), (13, 248_123), (0x03FF, 3153), (0, 0)] {
        file[0x0A..0x0C].copy_from_slice(&u16::to_be_bytes(divisor));
        assert_eq!(Config::from_bytes(&file).unwrap().baud, baud);
    }
}

#[test]
fn every_divisor_survives_a_read_and_write() {
    let mut file = Config::default().to_bytes().unwrap();
    for divisor in 1..=0x03FFu16 {
        file[0x0A..0x0C].copy_from_slice(&divisor.to_be_bytes());
        let config = Config::from_bytes(&file).unwrap();
        assert_eq!(config.to_bytes().unwrap(), file, "divisor {divisor}");
    }
}

#[test]
fn rejects_values_out_of_range() {
    let error = |config: Config| config.to_bytes().unwrap_err().to_string();
    assert_eq!(
        error(Config {
            brightness: 101,
            ..Config::default()
        }),
        "brightness is 101, not in 0-100"
    );
    assert_eq!(
        error(Config {
            background_icl: 8,
            ..Config::default()
        }),
        "background_icl is 8, not in 16-63"
    );
    assert_eq!(
        error(Config {
            standby: true,
            standby_delay: 0,
            ..Config::default()
        }),
        "standby_delay is 0, not in 1-65535"
    );
    assert_matches!(
        Config {
            sd_folder: Some("TOO_LONG_NAME".into()),
            ..Config::default()
        }
        .to_bytes(),
        Err(Error::SdFolder(_))
    );
}

#[test]
fn reads_what_it_writes() {
    let configs = [
        Config::default(),
        every_block(),
        Config {
            clock_out: Some(ClockOut {
                enabled: true,
                div: 1,
            }),
            nand_flash: Some(NandSize::Gbit1),
            rotation: Rotation::Deg90,
            sysclk_adjust: Some(2),
            ..Config::default()
        },
    ];
    for config in configs {
        let file = config.to_bytes().unwrap();
        assert_eq!(Config::from_bytes(&file).unwrap(), config);
    }
}

#[test]
fn reads_short_files_from_dwins_tools() {
    // The DGUS editor's 36-byte template: its LCD timing sits behind a
    // disabled marker.
    let template = hex!(
        "5435 4c43 3138 0000 2028 001c 4000 0000"
        "0000 0106 1e10 0320 d203 1401 e00c 0000"
        "000f 1000"
    );
    assert_eq!(
        Config::from_bytes(&template).unwrap(),
        Config {
            load_22: true,
            brightness: 0x40,
            standby_delay: 0,
            ..Config::default()
        }
    );

    // DWIN's tools mark the SD folder block `5A 5A`.
    let mut file = Config::default().to_bytes().unwrap();
    file[0x40..0x46].copy_from_slice(&hex!("5a5a 03 414243"));
    assert_eq!(
        Config::from_bytes(&file).unwrap().sd_folder.as_deref(),
        Some("ABC")
    );
}

#[test]
fn rejects_what_is_not_a_configuration() {
    assert_matches!(Config::from_bytes(b"T5LC1"), Err(Error::TooShort(5)));
    assert_matches!(Config::from_bytes(&[0; 0x80]), Err(Error::Magic));
}
