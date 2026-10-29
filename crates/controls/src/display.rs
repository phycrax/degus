use num_enum::IntoPrimitive;

use crate::error::{Error, Result};
use crate::geom::{Area, Point, put_u16};

/// Every display record is 32 bytes.
pub(crate) const RECORD_LEN: usize = 32;
type Record = [u8; RECORD_LEN];

/// First byte of a record; 0x5F marks variables shown in a popup menu.
const MARK: u8 = 0x5A;
const POPUP_MARK: u8 = 0x5F;
const NO_SP: u16 = 0xFFFF;

/// Type byte and description length (in words) of each supported control.
const ICON: u8 = 0x00;
const ICON_LEN: u16 = 0x0A;
const DATA: u8 = 0x10;
const DATA_LEN: u16 = 0x0D;
const TEXT: u8 = 0x11;
const TEXT_LEN: u16 = 0x0D;
const PROGRESS: u8 = 0x23;
const PROGRESS_LEN: u16 = 0x0D;

/// Progress bar flags in byte 26.
const SHOW_RATE: u8 = 0x80;
const NO_FILL: u8 = 0x02;
const NO_BORDER: u8 = 0x01;

/// Bytes available for a data variable's unit string.
const UNIT_MAX: usize = 11;

/// A display control. Only the controls used so far are supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Display {
    /// "Variable icon" (`5A 00`): shows the icon matching a VP value.
    Icon(IconVar),
    /// "Data variable" (`5A 10`): shows a VP as a number.
    Data(DataVar),
    /// "Text display" (`5A 11`): shows a text stored in VPs.
    Text(TextVar),
    /// "Progress bar" (`5A 23`): fills a box in proportion to a VP.
    Progress(ProgressBar),
}

impl From<IconVar> for Display {
    fn from(icon: IconVar) -> Self {
        Self::Icon(icon)
    }
}

impl From<DataVar> for Display {
    fn from(data: DataVar) -> Self {
        Self::Data(data)
    }
}

impl From<TextVar> for Display {
    fn from(text: TextVar) -> Self {
        Self::Text(text)
    }
}

impl From<ProgressBar> for Display {
    fn from(bar: ProgressBar) -> Self {
        Self::Progress(bar)
    }
}

/// A variable icon: VP values `val_min..=val_max` select icons
/// `icon_min..=icon_max` from an ICL library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconVar {
    pub vp: u16,
    /// Description pointer: a VP the firmware mirrors this record to, so it
    /// can be changed at runtime.
    pub sp: Option<u16>,
    /// Show the variable in a popup menu.
    pub popup_menu: bool,
    pub position: Point,
    pub val_min: u16,
    pub val_max: u16,
    pub icon_min: u16,
    pub icon_max: u16,
    /// ICL file ID holding the icons.
    pub icon_lib: u8,
    /// Draw the icon's background instead of keying it out.
    pub show_background: bool,
    pub layer_mode: LayerMode,
    /// Icon brightness, 0-255 (the editor's percentage × 255 / 100).
    pub icon_brightness: u8,
    /// Background brightness, 0-255.
    pub background_brightness: u8,
    /// Background filter strength ("Filter Set").
    pub filter: u8,
}

impl Default for IconVar {
    fn default() -> Self {
        Self {
            vp: 0,
            sp: None,
            popup_menu: false,
            position: Point::default(),
            val_min: 0,
            val_max: 0,
            icon_min: 0,
            icon_max: 0,
            icon_lib: 0,
            show_background: false,
            layer_mode: LayerMode::default(),
            icon_brightness: 0,
            background_brightness: 0,
            filter: 0x3F,
        }
    }
}

/// How an icon is layered over the page ("Background transparent Mode").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum LayerMode {
    #[default]
    Overlay = 0,
    OverlayMode1 = 2,
    OverlayMode2 = 3,
}

/// A data variable: a VP shown as a decimal number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataVar {
    pub vp: u16,
    /// Description pointer; see [`IconVar::sp`].
    pub sp: Option<u16>,
    pub popup_menu: bool,
    pub position: Point,
    /// RGB565 text colour.
    pub color: u16,
    /// Font library ID.
    pub font: u8,
    /// Character width in pixels.
    pub font_size: u8,
    pub align: Align,
    /// Adjust character spacing automatically.
    pub auto_spacing: bool,
    /// Draw leading zeros ("Display Invalid Zero").
    pub leading_zeros: bool,
    pub integer_digits: u8,
    pub decimal_digits: u8,
    pub data_type: DataType,
    /// Unit drawn after the number, ASCII, at most 11 bytes.
    pub unit: String,
}

impl Default for DataVar {
    fn default() -> Self {
        Self {
            vp: 0,
            sp: None,
            popup_menu: false,
            position: Point::default(),
            color: 0,
            font: 0,
            font_size: 8,
            align: Align::default(),
            auto_spacing: false,
            leading_zeros: false,
            integer_digits: 2,
            decimal_digits: 0,
            data_type: DataType::default(),
            unit: String::new(),
        }
    }
}

/// Alignment of a data variable around its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum Align {
    #[default]
    Left = 0,
    Right = 1,
    Center = 2,
}

/// How a data variable reads its VP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum DataType {
    /// Signed 16-bit.
    #[default]
    I16 = 0,
    /// Signed 32-bit, two VPs.
    I32 = 1,
    /// The VP's high byte, unsigned.
    HighByte = 2,
    /// The VP's low byte, unsigned.
    LowByte = 3,
    /// Signed 64-bit, four VPs.
    I64 = 4,
    U16 = 5,
    U32 = 6,
    F32 = 7,
    F64 = 8,
}

/// A text display: text stored from `vp` on, drawn inside `area`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextVar {
    pub vp: u16,
    /// Description pointer; see [`IconVar::sp`].
    pub sp: Option<u16>,
    pub popup_menu: bool,
    /// The text box.
    pub area: Area,
    /// RGB565 text colour.
    pub color: u16,
    /// Maximum text length in bytes.
    pub length: u16,
    pub encoding: Encoding,
    /// Font for ASCII characters in the multi-byte encodings.
    pub ascii_font: u8,
    /// Font for everything else.
    pub font: u8,
    pub font_width: u8,
    pub font_height: u8,
    pub align: TextAlign,
    pub vertical_align: VerticalAlign,
    /// Adjust character spacing automatically.
    pub auto_spacing: bool,
    pub char_spacing: u8,
    pub line_spacing: u8,
}

impl Default for TextVar {
    fn default() -> Self {
        Self {
            vp: 0,
            sp: None,
            popup_menu: false,
            area: Area::default(),
            color: 0,
            length: 2,
            encoding: Encoding::default(),
            ascii_font: 0,
            font: 0,
            font_width: 8,
            font_height: 8,
            align: TextAlign::default(),
            vertical_align: VerticalAlign::default(),
            auto_spacing: false,
            char_spacing: 0,
            line_spacing: 0,
        }
    }
}

/// Character encoding of a text display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum Encoding {
    /// One byte per character.
    #[default]
    Ascii = 0,
    Gb2312 = 1,
    Gbk = 2,
    Big5 = 3,
    ShiftJis = 4,
    /// UTF-16, big-endian.
    Unicode = 5,
}

/// Horizontal alignment of a text display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum TextAlign {
    #[default]
    Left = 0,
    Right = 1,
    Center = 2,
    RightToLeft = 3,
}

/// Vertical alignment of a text display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum VerticalAlign {
    #[default]
    Top = 0,
    Center = 1,
}

/// A progress bar: fills `area` in proportion to the VP value between
/// `val_min` and `val_max`. The default is a new editor control, which has
/// neither border nor background.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgressBar {
    pub vp: u16,
    /// Description pointer; see [`IconVar::sp`].
    pub sp: Option<u16>,
    /// The bar's box.
    pub area: Area,
    /// Value shown as an empty bar.
    pub val_min: i16,
    /// Value shown as a full bar.
    pub val_max: i16,
    pub data_type: BarDataType,
    /// Which way the bar grows.
    pub direction: BarDirection,
    /// RGB565 colour of the filled part.
    pub bar_color: u16,
    /// RGB565 border colour.
    pub border_color: u16,
    /// RGB565 colour of the empty part.
    pub background_color: u16,
    /// Draw a border in `border_color`.
    pub show_border: bool,
    /// Fill the empty part with `background_color`.
    pub fill_background: bool,
    /// The editor's "rate addr": sets the "show rate" flag with this VP.
    pub rate_vp: Option<u16>,
}

/// How a progress bar reads its VP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum BarDataType {
    /// Signed 16-bit.
    #[default]
    I16 = 0,
    /// The VP's high byte, unsigned.
    HighByte = 1,
    /// The VP's low byte, unsigned.
    LowByte = 2,
}

/// The direction a progress bar grows in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IntoPrimitive)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[repr(u8)]
pub enum BarDirection {
    #[default]
    Right = 0,
    Left = 1,
    Up = 2,
    Down = 3,
}

impl Display {
    pub(crate) fn encode(&self, page: u16) -> Result<Record> {
        Ok(match self {
            Display::Icon(icon) => {
                let mut r = header(ICON, ICON_LEN, icon.sp, icon.popup_menu);
                put_u16(&mut r, 6, icon.vp);
                icon.position.write(&mut r, 8);
                put_u16(&mut r, 12, icon.val_min);
                put_u16(&mut r, 14, icon.val_max);
                put_u16(&mut r, 16, icon.icon_min);
                put_u16(&mut r, 18, icon.icon_max);
                r[20] = icon.icon_lib;
                r[21] = icon.show_background.into();
                r[22] = icon.layer_mode.into();
                r[23] = icon.icon_brightness;
                r[24] = icon.background_brightness;
                r[25] = icon.filter;
                r
            }
            Display::Data(data) => {
                let unit = data.unit.as_bytes();
                if unit.len() > UNIT_MAX || !data.unit.is_ascii() {
                    return Err(Error::Unit {
                        page,
                        unit: data.unit.clone(),
                    });
                }
                let mut r = header(DATA, DATA_LEN, data.sp, data.popup_menu);
                put_u16(&mut r, 6, data.vp);
                data.position.write(&mut r, 8);
                put_u16(&mut r, 12, data.color);
                r[14] = data.font;
                r[15] = data.font_size;
                r[16] = u8::from(data.auto_spacing) << 7
                    | u8::from(data.leading_zeros) << 6
                    | u8::from(data.align);
                r[17] = data.integer_digits;
                r[18] = data.decimal_digits;
                r[19] = data.data_type.into();
                r[20] = unit.len() as u8;
                r[21..21 + unit.len()].copy_from_slice(unit);
                r
            }
            Display::Text(text) => {
                let mut r = header(TEXT, TEXT_LEN, text.sp, text.popup_menu);
                put_u16(&mut r, 6, text.vp);
                text.area.top_left().write(&mut r, 8);
                put_u16(&mut r, 12, text.color);
                text.area.write(&mut r, 14);
                put_u16(&mut r, 22, text.length);
                r[24] = text.ascii_font;
                r[25] = text.font;
                r[26] = text.font_width;
                r[27] = text.font_height;
                r[28] = u8::from(text.auto_spacing) << 7
                    | u8::from(text.align) << 5
                    | u8::from(text.vertical_align) << 4
                    | u8::from(text.encoding);
                r[29] = text.char_spacing;
                r[30] = text.line_spacing;
                r
            }
            Display::Progress(bar) => {
                let mut r = header(PROGRESS, PROGRESS_LEN, bar.sp, false);
                put_u16(&mut r, 6, bar.vp);
                bar.area.write(&mut r, 8);
                put_u16(&mut r, 16, bar.border_color);
                put_u16(&mut r, 18, bar.bar_color);
                put_u16(&mut r, 20, bar.background_color);
                put_u16(&mut r, 22, bar.val_max.cast_unsigned());
                put_u16(&mut r, 24, bar.val_min.cast_unsigned());
                r[26] = if bar.rate_vp.is_some() { SHOW_RATE } else { 0 }
                    | if bar.fill_background { 0 } else { NO_FILL }
                    | if bar.show_border { 0 } else { NO_BORDER };
                r[27] = bar.direction.into();
                r[28] = bar.data_type.into();
                put_u16(&mut r, 30, bar.rate_vp.unwrap_or(0));
                r
            }
        })
    }
}

fn header(kind: u8, len: u16, sp: Option<u16>, popup_menu: bool) -> Record {
    let mut r = [0; RECORD_LEN];
    r[0] = if popup_menu { POPUP_MARK } else { MARK };
    r[1] = kind;
    put_u16(&mut r, 2, sp.unwrap_or(NO_SP));
    put_u16(&mut r, 4, len);
    r
}
