use crate::error::{Error, Result};

/// Name the DGUS tools give the file. The display reads any file named
/// `T5LCFG*.CFG` in its download folder.
pub const FILE_NAME: &str = "T5LCFG.CFG";
/// Length of a built file: every documented field and the reserved bytes
/// after them.
pub const FILE_LEN: usize = 0x80;
/// The UART divisor is `BAUD_CLOCK / baud`.
pub const BAUD_CLOCK: u32 = 3_225_600;
/// Largest UART divisor, giving about 3150 baud.
pub const MAX_BAUD_DIVISOR: u16 = 0x03FF;

const MAGIC: &[u8; 5] = b"T5LC1";
/// Enable marker of the one-byte-marked blocks.
const ON: u8 = 0x5A;
/// Enable marker of the LCD and SD folder blocks, as DWIN's guide gives it.
/// DWIN's tools write `5A 5A` for the SD folder block.
const ON2: [u8; 2] = [0x5A, 0xA5];
const MAX_SD_FOLDER: usize = 8;

const FLAGS: usize = 0x05;
const FLAGS2: usize = 0x06;
const MUSIC_WAE: usize = 0x07;
const BACKGROUND_ICL: usize = 0x08;
const TOUCH_REPORT_DIV: usize = 0x09;
const BAUD: usize = 0x0A;
const BRIGHTNESS: usize = 0x0C;
const STANDBY_BRIGHTNESS: usize = 0x0D;
const STANDBY_DELAY: usize = 0x0E;
const LCD: usize = 0x10;
const TOUCH_PANEL: usize = 0x20;
const CLOCK_OUT: usize = 0x24;
const BUZZER: usize = 0x27;
const INIT_FILE: usize = 0x2E;
const SYSCLK: usize = 0x30;
const PICTURE_REPLACE: usize = 0x32;
const SD_FOLDER: usize = 0x40;
const BACKLIGHT_DELAY: usize = 0x70;

/// The contents of `T5LCFG.CFG`.
///
/// The fields up to `standby_delay` are the system settings at 0x05-0x0F,
/// which the display applies from every file. Each `Option` after them is a
/// block with its own enable marker: `None` writes the block disabled, and
/// the display keeps the setting it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 0x05.7: frames on the UART carry a CRC-16.
    pub crc: bool,
    /// 0x05.6: what plays touch sounds.
    pub audio: Audio,
    /// 0x05.5: load the VP initial values from file 22 (or `init_file`) at
    /// power-on.
    pub load_22: bool,
    /// 0x05.4: report VPs that touch controls change over the UART.
    pub auto_upload: bool,
    /// 0x05.3: sound on touch.
    pub touch_sound: bool,
    /// 0x05.2: dim the backlight to `standby_brightness` after
    /// `standby_delay` without a touch.
    pub standby: bool,
    /// 0x05.1-0: display direction.
    pub rotation: Rotation,
    /// 0x06.7: the PWM output drives the backlight at 1 kHz, instead of
    /// being left to the user.
    pub pwm_backlight: bool,
    /// 0x06.6 and 0x06.4: an added SPI NAND flash and its size.
    pub nand_flash: Option<NandSize>,
    /// 0x06.5: format the SPI NAND flash each time the file is loaded.
    pub format_nand_flash: bool,
    /// 0x06.3: erase the internal flash each time the file is loaded.
    pub erase_internal_flash: bool,
    /// 0x06.2: AD readings are 16-bit instead of 12-bit.
    pub adc_16bit: bool,
    /// 0x06.1: the OS CPU's 8051 program is downloaded encrypted.
    pub encrypt_8051: bool,
    /// 0x06.0: the OS CPU keeps running during an SD download.
    pub run_os_during_download: bool,
    /// 0x07: file number of the WAE that music plays from, 0-63.
    pub music_wae: u8,
    /// 0x08: file number of the ICL holding the page backgrounds, 16-63.
    pub background_icl: u8,
    /// 0x09: touch report rate is 400 Hz / `touch_report_div`.
    pub touch_report_div: u8,
    /// 0x0A-0x0B: UART speed is [`BAUD_CLOCK`] / `baud_divisor`, 1-0x3FF.
    pub baud_divisor: u16,
    /// 0x0C: backlight brightness at power-on, 0-100 %.
    pub brightness: u8,
    /// 0x0D: backlight brightness in standby, 0-100 %.
    pub standby_brightness: u8,
    /// 0x0E-0x0F: time without a touch before standby, in 10 ms units.
    pub standby_delay: u16,
    /// 0x10-0x1F: LCD timing.
    pub lcd: Option<Lcd>,
    /// 0x20-0x23: touch panel type and tuning.
    pub touch_panel: Option<TouchPanel>,
    /// 0x24-0x26: clock output on P3.0.
    pub clock_out: Option<ClockOut>,
    /// 0x27-0x2D: buzzer tone and length.
    pub buzzer: Option<Buzzer>,
    /// 0x2E-0x2F: file `load_22` loads, instead of 22.
    pub init_file: Option<u8>,
    /// 0x30-0x31: system clock adjustment, -2 to +2 in 1/224 steps.
    pub sysclk_adjust: Option<i8>,
    /// 0x32-0x33: ICL holding replacements for background pictures; a
    /// picture found there is shown instead of the one in `background_icl`.
    pub picture_replace_icl: Option<u8>,
    /// 0x40-0x4A: renames the SD download folder from `DWIN_SET`. The
    /// display stores the name in its flash, so it outlasts this file.
    pub sd_folder: Option<String>,
    /// 0x70-0x71: backlight delay at power-on, in 10 ms units.
    pub backlight_delay: Option<u8>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            crc: false,
            audio: Audio::Buzzer,
            load_22: false,
            auto_upload: true,
            touch_sound: true,
            standby: false,
            rotation: Rotation::Deg0,
            pwm_backlight: false,
            nand_flash: None,
            format_nand_flash: false,
            erase_internal_flash: false,
            adc_16bit: false,
            encrypt_8051: false,
            run_os_during_download: false,
            music_wae: 0,
            background_icl: 32,
            touch_report_div: 0x28,
            baud_divisor: 0x1C,
            brightness: 100,
            standby_brightness: 0,
            standby_delay: 1000,
            lcd: None,
            touch_panel: None,
            clock_out: None,
            buzzer: None,
            init_file: None,
            sysclk_adjust: None,
            picture_replace_icl: None,
            sd_folder: None,
            backlight_delay: None,
        }
    }
}

/// What plays touch sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum Audio {
    #[default]
    Buzzer,
    /// The WAE at `music_wae`.
    Music,
}

/// Display direction. Pages are drawn for the rotated screen: at 90° and
/// 270°, an 800x480 panel shows 480x800 pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(try_from = "u16", into = "u16")
)]
#[repr(u8)]
pub enum Rotation {
    #[default]
    Deg0 = 0,
    Deg90 = 1,
    Deg180 = 2,
    Deg270 = 3,
}

impl Rotation {
    pub fn degrees(self) -> u16 {
        u16::from(self as u8) * 90
    }

    pub fn from_degrees(degrees: u16) -> Option<Self> {
        Some(match degrees {
            0 => Self::Deg0,
            90 => Self::Deg90,
            180 => Self::Deg180,
            270 => Self::Deg270,
            _ => return None,
        })
    }

    /// Whether pages are the panel turned on its side.
    pub fn swaps_axes(self) -> bool {
        matches!(self, Self::Deg90 | Self::Deg270)
    }
}

impl TryFrom<u16> for Rotation {
    type Error = String;

    fn try_from(degrees: u16) -> Result<Self, String> {
        Self::from_degrees(degrees)
            .ok_or_else(|| format!("rotation {degrees} is not 0, 90, 180 or 270"))
    }
}

impl From<Rotation> for u16 {
    fn from(rotation: Rotation) -> Self {
        rotation.degrees()
    }
}

/// Size of an added SPI NAND flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum NandSize {
    #[cfg_attr(feature = "serde", serde(rename = "1gbit"))]
    Gbit1,
    #[cfg_attr(feature = "serde", serde(rename = "4gbit"))]
    Gbit4,
}

/// LCD timing, set at the factory for the panel. Wrong values leave the
/// screen white or flickering. Fields are named as in DWIN's guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct Lcd {
    /// 0x12: data latch phase, 0 = PCLK rising, 1 = PCLK falling.
    pub pclk_phs: u8,
    /// 0x13: pixel clock is 206.4 MHz / `pclk_div`.
    pub pclk_div: u8,
    /// 0x14
    pub h_w: u8,
    /// 0x15
    pub h_s: u8,
    /// 0x16-0x17: horizontal resolution.
    pub h_d: u16,
    /// 0x18
    pub h_e: u8,
    /// 0x19
    pub v_w: u8,
    /// 0x1A
    pub v_s: u8,
    /// 0x1B-0x1C: vertical resolution.
    pub v_d: u16,
    /// 0x1D
    pub v_e: u8,
    /// 0x1E: 0 = no TCON to configure.
    #[cfg_attr(feature = "serde", serde(default))]
    pub tcon_sel: u8,
}

/// Touch panel type and tuning, set at the factory for the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct TouchPanel {
    /// 0x21: panel type in the high 4 bits (0x0 4-wire resistive, 0x1
    /// GT911/GT9271/GT9110, …, 0xF 5-wire resistive); bit 3 calibrates a
    /// resistive panel on the next SD download; bits 2-0 reverse X, reverse
    /// Y and swap X and Y on capacitive panels.
    pub mode: u8,
    /// 0x22: sensitivity, 0x00-0x1F on capacitive panels (factory 0x14),
    /// 0x10-0xFF on resistive ones (factory 0x40).
    pub sense: u8,
    /// 0x23: scan frequency of ILI9881H panels, 0x01-0x14; 0 hops.
    #[cfg_attr(feature = "serde", serde(default))]
    pub freq: u8,
}

/// Clock output on P3.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct ClockOut {
    /// 0x25: drive the output; `false` turns it off.
    #[cfg_attr(feature = "serde", serde(default = "yes"))]
    pub enabled: bool,
    /// 0x26: frequency is 825.7536 MHz / `div`.
    pub div: u8,
}

#[cfg(feature = "serde")]
fn yes() -> bool {
    true
}

/// Buzzer tone and length. The defaults are the factory values: a 2.5 kHz
/// tone at 8 % duty for 100 ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct Buzzer {
    /// 0x28: frequency is 825.7536 MHz / (`div1` × `div2`).
    pub div1: u8,
    /// 0x29-0x2A
    pub div2: u16,
    /// 0x2B-0x2C: high time is `duty` / `div2`; more is louder.
    pub duty: u16,
    /// 0x2D: length of the touch beep, in 10 ms units.
    pub time: u8,
}

impl Default for Buzzer {
    fn default() -> Self {
        Self {
            div1: 0x6E,
            div2: 0x0BB8,
            duty: 0x00F0,
            time: 0x0A,
        }
    }
}

impl Config {
    /// The divisor nearest to `baud`, if one is in range. The UART then
    /// runs at [`BAUD_CLOCK`] / divisor, which can differ from `baud`.
    pub fn divisor_for_baud(baud: u32) -> Option<u16> {
        if baud == 0 {
            return None;
        }
        let divisor = (BAUD_CLOCK + baud / 2) / baud;
        u16::try_from(divisor)
            .ok()
            .filter(|d| (1..=MAX_BAUD_DIVISOR).contains(d))
    }

    /// The UART speed `baud_divisor` gives, rounded to a whole baud.
    pub fn baud(&self) -> u32 {
        let divisor = u32::from(self.baud_divisor.max(1));
        (BAUD_CLOCK + divisor / 2) / divisor
    }

    /// Writes the file: [`FILE_LEN`] bytes, with disabled blocks zeroed.
    pub fn build(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut b = vec![0; FILE_LEN];
        b[..MAGIC.len()].copy_from_slice(MAGIC);
        b[FLAGS] = bit(self.crc, 7)
            | bit(self.audio == Audio::Music, 6)
            | bit(self.load_22, 5)
            | bit(self.auto_upload, 4)
            | bit(self.touch_sound, 3)
            | bit(self.standby, 2)
            | self.rotation as u8;
        b[FLAGS2] = bit(self.pwm_backlight, 7)
            | bit(self.nand_flash.is_some(), 6)
            | bit(self.format_nand_flash, 5)
            | bit(self.nand_flash == Some(NandSize::Gbit4), 4)
            | bit(self.erase_internal_flash, 3)
            | bit(self.adc_16bit, 2)
            | bit(self.encrypt_8051, 1)
            | bit(self.run_os_during_download, 0);
        b[MUSIC_WAE] = self.music_wae;
        b[BACKGROUND_ICL] = self.background_icl;
        b[TOUCH_REPORT_DIV] = self.touch_report_div;
        put_u16(&mut b, BAUD, self.baud_divisor);
        b[BRIGHTNESS] = self.brightness;
        b[STANDBY_BRIGHTNESS] = self.standby_brightness;
        put_u16(&mut b, STANDBY_DELAY, self.standby_delay);

        if let Some(lcd) = &self.lcd {
            b[LCD..LCD + 2].copy_from_slice(&ON2);
            b[LCD + 2] = lcd.pclk_phs;
            b[LCD + 3] = lcd.pclk_div;
            b[LCD + 4] = lcd.h_w;
            b[LCD + 5] = lcd.h_s;
            put_u16(&mut b, LCD + 6, lcd.h_d);
            b[LCD + 8] = lcd.h_e;
            b[LCD + 9] = lcd.v_w;
            b[LCD + 10] = lcd.v_s;
            put_u16(&mut b, LCD + 11, lcd.v_d);
            b[LCD + 13] = lcd.v_e;
            b[LCD + 14] = lcd.tcon_sel;
        }
        if let Some(panel) = &self.touch_panel {
            b[TOUCH_PANEL..TOUCH_PANEL + 4].copy_from_slice(&[
                ON,
                panel.mode,
                panel.sense,
                panel.freq,
            ]);
        }
        if let Some(clock) = &self.clock_out {
            let enabled = if clock.enabled { ON } else { 0 };
            b[CLOCK_OUT..CLOCK_OUT + 3].copy_from_slice(&[ON, enabled, clock.div]);
        }
        if let Some(buzzer) = &self.buzzer {
            b[BUZZER] = ON;
            b[BUZZER + 1] = buzzer.div1;
            put_u16(&mut b, BUZZER + 2, buzzer.div2);
            put_u16(&mut b, BUZZER + 4, buzzer.duty);
            b[BUZZER + 6] = buzzer.time;
        }
        let pairs = [
            (INIT_FILE, self.init_file),
            (SYSCLK, self.sysclk_adjust.map(|adjust| adjust as u8)),
            (PICTURE_REPLACE, self.picture_replace_icl),
            (BACKLIGHT_DELAY, self.backlight_delay),
        ];
        for (at, value) in pairs {
            if let Some(value) = value {
                b[at..at + 2].copy_from_slice(&[ON, value]);
            }
        }
        if let Some(name) = &self.sd_folder {
            b[SD_FOLDER..SD_FOLDER + 2].copy_from_slice(&ON2);
            b[SD_FOLDER + 2] = name.len() as u8;
            b[SD_FOLDER + 3..][..name.len()].copy_from_slice(name.as_bytes());
        }
        Ok(b)
    }

    fn validate(&self) -> Result<()> {
        range("music_wae", self.music_wae, 0, 63)?;
        range("background_icl", self.background_icl, 16, 63)?;
        range("touch_report_div", self.touch_report_div, 1, 255)?;
        range(
            "baud_divisor",
            self.baud_divisor,
            1,
            MAX_BAUD_DIVISOR.into(),
        )?;
        range("brightness", self.brightness, 0, 100)?;
        range("standby_brightness", self.standby_brightness, 0, 100)?;
        if self.standby {
            range("standby_delay", self.standby_delay, 1, 0xFFFF)?;
        }
        if let Some(lcd) = &self.lcd {
            range("lcd.pclk_div", lcd.pclk_div, 1, 255)?;
        }
        if let Some(clock) = &self.clock_out {
            range("clock_out.div", clock.div, 1, 255)?;
        }
        if let Some(buzzer) = &self.buzzer {
            range("buzzer.div1", buzzer.div1, 1, 255)?;
            range("buzzer.div2", buzzer.div2, 1, 0xFFFF)?;
        }
        if let Some(adjust) = self.sysclk_adjust {
            range("sysclk_adjust", adjust, -2, 2)?;
        }
        if let Some(name) = &self.sd_folder
            && !valid_sd_folder(name.as_bytes())
        {
            return Err(Error::SdFolder(name.clone()));
        }
        Ok(())
    }
}

fn valid_sd_folder(name: &[u8]) -> bool {
    (1..=MAX_SD_FOLDER).contains(&name.len())
        && name
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn range(field: &'static str, value: impl Into<i32>, min: i32, max: i32) -> Result<()> {
    let value = value.into();
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(Error::Range {
            field,
            value,
            min,
            max,
        })
    }
}

fn bit(on: bool, index: u8) -> u8 {
    u8::from(on) << index
}

fn put_u16(b: &mut [u8], at: usize, value: u16) {
    b[at..at + 2].copy_from_slice(&value.to_be_bytes());
}
