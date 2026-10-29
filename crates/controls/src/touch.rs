use std::ops::RangeInclusive;

use num_enum::IntoPrimitive;

use crate::error::{Error, Result};
use crate::geom::{Area, Point, put_i32, put_u16};

/// Page fields use `FF 00` for "no page".
const NO_PAGE: u16 = 0xFF00;
/// Control code prefixes: with and without auto-upload of the changed VP.
const UPLOAD: u8 = 0xFE;
const NO_UPLOAD: u8 = 0xFD;
/// Records are runs of 16-byte units; every unit after the first starts
/// with `FE`.
const UNIT_LEN: usize = 16;
const CONTINUATION: u8 = 0xFE;

/// Control types and record lengths. Basic touch has no type byte.
const BASIC_LEN: usize = 16;
const DATA_INPUT: u8 = 0x00;
const DATA_INPUT_LEN: usize = 64;
const KEY_CODE: u8 = 0x05;
const KEY_CODE_LEN: usize = 32;
const MAX_LEN: usize = 64;

/// Data input display modes in byte 31.
const MASKED: u8 = 0x00;
const UNMASKED: u8 = 0x01;
/// Data input keyboard sources in byte 33.
const KEYBOARD_ON_PAGE: u8 = 0x00;
const KEYBOARD_PAGE: u8 = 0x01;
/// Keyboard page of a keyboard on the input's own page (the editor's -1).
const NO_KEYBOARD_PAGE: u16 = 0xFFFF;
/// Data input option flags in bytes 49 and 58.
const LIMITS_ON: u8 = 0xFF;
const LOAD_ON: u8 = 0x5A;

/// A touch control: the area that reacts and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touch {
    pub area: Area,
    /// Page to switch to when the area is released.
    pub next_page: Option<u16>,
    /// Page whose pixels are shown over the area while it is pressed
    /// (the editor's "button effect").
    pub pressed_page: Option<u16>,
    pub action: TouchAction,
}

impl Touch {
    /// A touch control without page switch or press effect.
    pub fn new(area: Area, action: impl Into<TouchAction>) -> Self {
        Self {
            area,
            next_page: None,
            pressed_page: None,
            action: action.into(),
        }
    }
}

/// What a touch control does. Only the controls used so far are supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TouchAction {
    /// "Basic touch": a 16-byte record without VP. Pressing it sends this
    /// key code to the data input being edited, so keyboards are made of
    /// these: `0x30`-`0x39` digits, `0x2E` decimal point, `0x2D` sign,
    /// `0xF0` cancel, `0xF1` enter, `0xF2` backspace. The high byte must
    /// not be `FE` or `FD`.
    Basic(u16),
    /// "Return key code" (`FE 05`): writes a value to a VP when pressed.
    KeyCode(KeyCode),
    /// "Data input" (`FE 00`): types a number into a VP on a keyboard.
    DataInput(DataInput),
}

impl From<KeyCode> for TouchAction {
    fn from(key: KeyCode) -> Self {
        Self::KeyCode(key)
    }
}

impl From<DataInput> for TouchAction {
    fn from(input: DataInput) -> Self {
        Self::DataInput(input)
    }
}

impl TouchAction {
    fn record_len(&self) -> usize {
        match self {
            Self::Basic(_) => BASIC_LEN,
            Self::KeyCode(_) => KEY_CODE_LEN,
            Self::DataInput(_) => DATA_INPUT_LEN,
        }
    }
}

/// Settings of a "Return key code" touch control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCode {
    pub vp: u16,
    /// Which part of the VP receives `value`.
    pub target: KeyTarget,
    pub value: u16,
    /// Hold time in 0.1 s units.
    pub hold_time: u8,
    /// Report the written VP over the UART ("Data auto-uploading").
    pub upload: bool,
}

impl KeyCode {
    /// Writes `value` to the whole VP word and uploads it, like a new editor
    /// control.
    pub const fn new(vp: u16, value: u16) -> Self {
        Self {
            vp,
            target: KeyTarget::Word,
            value,
            hold_time: 0,
            upload: true,
        }
    }
}

/// Where a key code is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum KeyTarget {
    /// The whole VP word.
    #[default]
    Word,
    /// The VP's high byte.
    HighByte,
    /// The VP's low byte.
    LowByte,
    /// A single bit of the VP, 0-15.
    Bit(u8),
}

impl KeyTarget {
    fn code(self, page: u16) -> Result<u8> {
        match self {
            Self::Word => Ok(0),
            Self::HighByte => Ok(1),
            Self::LowByte => Ok(2),
            Self::Bit(bit @ 0..=15) => Ok(0x10 + bit),
            Self::Bit(bit) => Err(Error::BitIndex { page, bit }),
        }
    }
}

/// Settings of a "Data input" touch control (the editor's "Variable data
/// input"). Pressing the area starts input: the keyboard's basic touch keys
/// edit a number drawn at `position`, and enter writes it to `vp`.
///
/// The default is a new editor control, except that it shows the typed
/// digits instead of `*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataInput {
    pub vp: u16,
    pub data_type: InputDataType,
    pub integer_digits: u8,
    pub decimal_digits: u8,
    /// Where the number being typed is drawn.
    pub position: Point,
    /// RGB565 colour of the number being typed.
    pub color: u16,
    /// Font library ID.
    pub font: u8,
    /// Character width in pixels.
    pub font_size: u8,
    pub cursor_color: CursorColor,
    /// Show `*` instead of the typed characters.
    pub masked: bool,
    pub keyboard: Keyboard,
    /// Rejects numbers outside this range ("Enabling limits"). The bounds
    /// ignore the decimal point: with 2 decimal digits, `0..=10000` allows
    /// 0.00 to 100.00.
    pub limits: Option<RangeInclusive<i32>>,
    /// The editor's "Loading data when input": a VP that receives
    /// `load_value` during input.
    pub load_vp: Option<u16>,
    pub load_value: u16,
    /// Keyboard layer transparency, 0-255 (the editor's percentage × 255 /
    /// 100). The editor shows it only for custom projects.
    pub keyboard_transparency: u8,
    /// Report the written VP over the UART ("Data auto-uploading").
    pub upload: bool,
}

impl Default for DataInput {
    fn default() -> Self {
        Self {
            vp: 0,
            data_type: InputDataType::default(),
            integer_digits: 8,
            decimal_digits: 0,
            position: Point::default(),
            color: 0,
            font: 0,
            font_size: 16,
            cursor_color: CursorColor::default(),
            masked: false,
            keyboard: Keyboard::default(),
            limits: None,
            load_vp: None,
            load_value: 0,
            keyboard_transparency: 0,
            upload: true,
        }
    }
}

/// How a data input writes its VP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum InputDataType {
    /// Signed 16-bit.
    #[default]
    I16 = 0,
    /// Signed 32-bit, two VPs.
    I32 = 1,
    /// The VP's high byte.
    HighByte = 2,
    /// The VP's low byte.
    LowByte = 3,
    /// Signed 64-bit, four VPs.
    I64 = 4,
    /// Floating point (the editor's "float").
    Float = 5,
}

/// Colour of a data input's cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum CursorColor {
    #[default]
    Black = 0,
    White = 1,
}

/// Where a data input's keyboard is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Keyboard {
    /// The keys are basic touch controls on the input's own page.
    #[default]
    OnPage,
    /// While input is active, `area` of `page` is drawn at `position`, and
    /// the basic touch keys of `page` inside `area` work there.
    Page {
        page: u16,
        area: Area,
        position: Point,
    },
}

impl Touch {
    /// Appends this control's record for `page` to `out`.
    pub(crate) fn encode(&self, page: u16, out: &mut Vec<u8>) -> Result<()> {
        let mut r = [0u8; MAX_LEN];
        put_u16(&mut r, 0, page);
        self.area.write(&mut r, 2);
        put_u16(&mut r, 10, self.next_page.unwrap_or(NO_PAGE));
        put_u16(&mut r, 12, self.pressed_page.unwrap_or(NO_PAGE));
        match &self.action {
            &TouchAction::Basic(code) => {
                if matches!(code.to_be_bytes()[0], UPLOAD | NO_UPLOAD) {
                    return Err(Error::BasicKey { page, code });
                }
                put_u16(&mut r, 14, code);
            }
            TouchAction::KeyCode(key) => {
                r[14] = prefix(key.upload);
                r[15] = KEY_CODE;
                put_u16(&mut r, 17, key.vp);
                r[19] = key.target.code(page)?;
                put_u16(&mut r, 20, key.value);
                r[22] = key.hold_time;
            }
            TouchAction::DataInput(input) => input.write(&mut r),
        }
        let len = self.action.record_len();
        for unit in (UNIT_LEN..len).step_by(UNIT_LEN) {
            r[unit] = CONTINUATION;
        }
        out.extend_from_slice(&r[..len]);
        Ok(())
    }
}

impl DataInput {
    /// Writes bytes 14 and up of the record, except the unit markers.
    fn write(&self, r: &mut [u8]) {
        r[14] = prefix(self.upload);
        r[15] = DATA_INPUT;
        put_u16(r, 17, self.vp);
        r[19] = self.data_type.into();
        r[20] = self.integer_digits;
        r[21] = self.decimal_digits;
        self.position.write(r, 22);
        put_u16(r, 26, self.color);
        r[28] = self.font;
        r[29] = self.font_size;
        r[30] = self.cursor_color.into();
        r[31] = if self.masked { MASKED } else { UNMASKED };
        match self.keyboard {
            Keyboard::OnPage => {
                r[33] = KEYBOARD_ON_PAGE;
                put_u16(r, 34, NO_KEYBOARD_PAGE);
            }
            Keyboard::Page {
                page,
                area,
                position,
            } => {
                r[33] = KEYBOARD_PAGE;
                put_u16(r, 34, page);
                area.write(r, 36);
                position.write(r, 44);
            }
        }
        if let Some(limits) = &self.limits {
            r[49] = LIMITS_ON;
            put_i32(r, 50, *limits.start());
            put_i32(r, 54, *limits.end());
        }
        if let Some(vp) = self.load_vp {
            r[58] = LOAD_ON;
            put_u16(r, 59, vp);
        }
        put_u16(r, 61, self.load_value);
        r[63] = self.keyboard_transparency;
    }
}

fn prefix(upload: bool) -> u8 {
    if upload { UPLOAD } else { NO_UPLOAD }
}
