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
        Config::default().build().unwrap(),
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
    .build()
    .unwrap();
    assert_eq!(file[..0x10], template[..0x10]);
}

#[test]
fn writes_every_block() {
    let config = Config {
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
        baud_divisor: 0x0150,
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
    };
    let file = config.build().unwrap();
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
fn converts_baud_rates() {
    assert_eq!(Config::divisor_for_baud(115_200), Some(0x1C));
    assert_eq!(Config::divisor_for_baud(9600), Some(0x0150));
    assert_eq!(Config::divisor_for_baud(0), None);
    assert_eq!(Config::divisor_for_baud(3000), None);
    let config = Config {
        baud_divisor: 13,
        ..Config::default()
    };
    assert_eq!(config.baud(), 248_123);
    assert_eq!(Config::divisor_for_baud(config.baud()), Some(13));
}

#[test]
fn rejects_values_out_of_range() {
    let error = |config: Config| config.build().unwrap_err().to_string();
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
    assert!(matches!(
        Config {
            sd_folder: Some("TOO_LONG_NAME".into()),
            ..Config::default()
        }
        .build(),
        Err(Error::SdFolder(_))
    ));
}
