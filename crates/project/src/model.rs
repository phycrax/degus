//! The project description. `project.toml` holds the settings of the whole
//! project and includes other files; every file, `project.toml` too, holds
//! declarations: pages, controls, libraries and files to copy. Which file a
//! declaration is in means nothing to the build.
//!
//! Field names follow `degus-controls`. Geometry is written as `x, y, w, h`
//! instead of the files' start and end corners.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use degus_config::{
    Audio, BAUD_CLOCK, Buzzer, ClockOut, Config, Lcd, MAX_BAUD_DIVISOR, NandSize, Rotation,
    TouchPanel,
};
use degus_controls::{
    Align, Area, BarDataType, BarDirection, CursorColor, DataInput, DataType, DataVar, Display,
    Encoding, IconVar, InputDataType, KeyCode, KeyTarget, Keyboard, LayerMode, Point, ProgressBar,
    TextAlign, TextVar, Touch, TouchAction, VerticalAlign,
};
use degus_icl::{Chip, IclOptions, Subsampling};
use degus_wae::{Downmix, SampleFormat};
use serde::{Deserialize, Deserializer, de};

/// Name of the project file in a project folder.
pub const PROJECT_FILE_NAME: &str = "project.toml";

/// `project.toml`: the [`Settings`], then [`Declarations`] like any file.
/// [`RootFile::split`] separates them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootFile {
    pub screen: Size,
    #[serde(default)]
    pub chip: ChipKind,
    #[serde(default = "default_output")]
    pub output: PathBuf,
    #[serde(default)]
    pub include: Vec<PathBuf>,
    pub config: Option<ConfigSpec>,
    #[serde(default)]
    pub copy: Vec<PathBuf>,
    #[serde(default)]
    pub icl: Vec<IclSpec>,
    #[serde(default)]
    pub wae: Vec<WaeSpec>,
    #[serde(default)]
    pub page: BTreeMap<String, PageSpec>,
    #[serde(default)]
    pub control: BTreeMap<String, Control>,
}

impl RootFile {
    pub fn split(self) -> (Settings, Declarations) {
        let settings = Settings {
            screen: self.screen,
            chip: self.chip,
            output: self.output,
            include: self.include,
            config: self.config,
        };
        let declarations = Declarations {
            copy: self.copy,
            icl: self.icl,
            wae: self.wae,
            page: self.page,
            control: self.control,
        };
        (settings, declarations)
    }
}

/// What only `project.toml` sets: the device, the build and the other
/// files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Screen size in pixels.
    pub screen: Size,
    /// Chip variant; sets the largest icon an ICL can hold.
    pub chip: ChipKind,
    /// Folder the build writes `DWIN_SET` in, relative to the project root.
    pub output: PathBuf,
    /// The other files of the project, relative to the project root.
    pub include: Vec<PathBuf>,
    /// The hardware configuration file; none is written without it.
    pub config: Option<ConfigSpec>,
}

fn default_output() -> PathBuf {
    "target".into()
}

/// What any file of a project declares. Names are unique across the files,
/// one set for pages and one for controls.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    /// Files copied into the output unchanged, relative to the project root.
    #[serde(default)]
    pub copy: Vec<PathBuf>,
    /// Icon libraries.
    #[serde(default)]
    pub icl: Vec<IclSpec>,
    /// Audio libraries.
    #[serde(default)]
    pub wae: Vec<WaeSpec>,
    /// `[page.<name>]` tables.
    #[serde(default)]
    pub page: BTreeMap<String, PageSpec>,
    /// `[control.<name>]` tables. Their order means nothing; a page's list
    /// sets the drawing order.
    #[serde(default)]
    pub control: BTreeMap<String, Control>,
}

/// `[config]`: the hardware configuration file, `T5LCFG.CFG`. Keys follow
/// `degus-config`; each block left out is written disabled, so the display
/// keeps its own setting.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSpec {
    /// ID of the `[[icl]]` holding the page backgrounds: image N is the
    /// background of page N. When set, the check uses it; otherwise the
    /// file names ICL 32.
    pub background_icl: Option<u16>,
    /// ID of the `[[wae]]` music plays from; WAE 0 when not set.
    pub music_wae: Option<u16>,
    #[serde(default)]
    pub crc: bool,
    #[serde(default)]
    pub audio: Audio,
    #[serde(default)]
    pub load_22: bool,
    #[serde(default = "default_auto_upload")]
    pub auto_upload: bool,
    #[serde(default = "default_touch_sound")]
    pub touch_sound: bool,
    #[serde(default)]
    pub standby: bool,
    /// In degrees.
    #[serde(default)]
    pub rotation: Rotation,
    #[serde(default)]
    pub pwm_backlight: bool,
    pub nand_flash: Option<NandSize>,
    #[serde(default)]
    pub format_nand_flash: bool,
    #[serde(default)]
    pub erase_internal_flash: bool,
    #[serde(default)]
    pub adc_16bit: bool,
    #[serde(default)]
    pub encrypt_8051: bool,
    #[serde(default)]
    pub run_os_during_download: bool,
    #[serde(default = "default_touch_report_div")]
    pub touch_report_div: u8,
    /// UART speed in baud.
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    #[serde(default)]
    pub standby_brightness: u8,
    #[serde(default = "default_standby_delay")]
    pub standby_delay: u16,
    pub lcd: Option<Lcd>,
    pub touch_panel: Option<TouchPanel>,
    pub clock_out: Option<ClockOut>,
    pub buzzer: Option<Buzzer>,
    pub init_file: Option<u8>,
    pub sysclk_adjust: Option<i8>,
    pub picture_replace_icl: Option<u8>,
    pub sd_folder: Option<String>,
    pub backlight_delay: Option<u8>,
}

/// How far the UART may run from the requested `baud`.
const BAUD_TOLERANCE: f64 = 0.02;

impl ConfigSpec {
    /// The configuration to build. Range checks are left to
    /// [`Config::build`].
    pub fn to_config(&self) -> Result<Config, String> {
        let byte = |key: &str, value: Option<u16>, default: u8| match value {
            None => Ok(default),
            Some(value) => u8::try_from(value).map_err(|_| format!("{key} {value} is over 255")),
        };
        let defaults = Config::default();
        Ok(Config {
            crc: self.crc,
            audio: self.audio,
            load_22: self.load_22,
            auto_upload: self.auto_upload,
            touch_sound: self.touch_sound,
            standby: self.standby,
            rotation: self.rotation,
            pwm_backlight: self.pwm_backlight,
            nand_flash: self.nand_flash,
            format_nand_flash: self.format_nand_flash,
            erase_internal_flash: self.erase_internal_flash,
            adc_16bit: self.adc_16bit,
            encrypt_8051: self.encrypt_8051,
            run_os_during_download: self.run_os_during_download,
            music_wae: byte("music_wae", self.music_wae, defaults.music_wae)?,
            background_icl: byte(
                "background_icl",
                self.background_icl,
                defaults.background_icl,
            )?,
            touch_report_div: self.touch_report_div,
            baud_divisor: baud_divisor(self.baud)?,
            brightness: self.brightness,
            standby_brightness: self.standby_brightness,
            standby_delay: self.standby_delay,
            lcd: self.lcd,
            touch_panel: self.touch_panel,
            clock_out: self.clock_out,
            buzzer: self.buzzer,
            init_file: self.init_file,
            sysclk_adjust: self.sysclk_adjust,
            picture_replace_icl: self.picture_replace_icl,
            sd_folder: self.sd_folder.clone(),
            backlight_delay: self.backlight_delay,
        })
    }
}

fn baud_divisor(baud: u32) -> Result<u16, String> {
    let divisor = Config::divisor_for_baud(baud).ok_or_else(|| {
        format!("baud {baud} is out of range: {BAUD_CLOCK} / baud must be 1-{MAX_BAUD_DIVISOR}")
    })?;
    let actual = Config {
        baud_divisor: divisor,
        ..Config::default()
    }
    .baud();
    if (f64::from(actual) - f64::from(baud)).abs() > f64::from(baud) * BAUD_TOLERANCE {
        return Err(format!(
            "baud {baud} is not reachable; the nearest rate is {actual}"
        ));
    }
    Ok(divisor)
}

fn default_auto_upload() -> bool {
    Config::default().auto_upload
}

fn default_touch_sound() -> bool {
    Config::default().touch_sound
}

fn default_touch_report_div() -> u8 {
    Config::default().touch_report_div
}

fn default_baud() -> u32 {
    Config::default().baud()
}

fn default_brightness() -> u8 {
    Config::default().brightness
}

fn default_standby_delay() -> u16 {
    Config::default().standby_delay
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub w: u16,
    pub h: u16,
}

impl Size {
    pub const FULL_HD: Size = Size { w: 1920, h: 1080 };
}

/// `1920x1080`, with `x` in either case.
impl std::str::FromStr for Size {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let size = || {
            let (w, h) = text.trim().split_once(['x', 'X'])?;
            Some(Size {
                w: w.trim().parse().ok()?,
                h: h.trim().parse().ok()?,
            })
        };
        size().ok_or_else(|| format!("{text:?} is not a size such as 1920x1080"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum ChipKind {
    #[serde(rename = "t5l1")]
    T5l1,
    #[default]
    #[serde(rename = "t5l2")]
    T5l2,
}

impl From<ChipKind> for Chip {
    fn from(chip: ChipKind) -> Self {
        match chip {
            ChipKind::T5l1 => Chip::T5L1,
            ChipKind::T5l2 => Chip::T5L2,
        }
    }
}

/// An ICL to build from images.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IclSpec {
    /// The ICL is written as `<id>.icl` and stored at `id` × 256 KB in
    /// flash. Icon controls refer to it as `icon_lib`.
    pub id: u16,
    /// The images, each with its icon ID.
    #[serde(default)]
    pub images: Vec<Source>,
    /// JPEG quality, 1-100.
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default)]
    pub subsampling: SubsamplingKind,
    /// Store JPEG sources unchanged when the T5L can decode them.
    #[serde(default)]
    pub passthrough_jpeg: bool,
}

fn default_quality() -> u8 {
    IclOptions::default().quality
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum SubsamplingKind {
    #[default]
    #[serde(rename = "4:2:0")]
    S420,
    #[serde(rename = "4:4:4")]
    S444,
}

impl From<SubsamplingKind> for Subsampling {
    fn from(kind: SubsamplingKind) -> Self {
        match kind {
            SubsamplingKind::S420 => Subsampling::S420,
            SubsamplingKind::S444 => Subsampling::S444,
        }
    }
}

/// An image of an ICL or an audio file of a WAE.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Icon or clip ID in the library.
    pub id: u16,
    /// Relative to the project root.
    pub path: PathBuf,
}

/// A WAE to build from audio files.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaeSpec {
    /// The WAE is written as `<id>.wae` and stored at `id` × 256 KB in
    /// flash.
    pub id: u16,
    /// The audio files, each with its clip ID.
    #[serde(default)]
    pub clips: Vec<Source>,
    #[serde(default)]
    pub format: FormatKind,
    #[serde(default)]
    pub downmix: DownmixKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatKind {
    /// 16-bit PCM.
    #[default]
    Pcm16,
    /// DWIN's 8-bit "12:8" companding, half the size.
    Companded8,
}

impl From<FormatKind> for SampleFormat {
    fn from(kind: FormatKind) -> Self {
        match kind {
            FormatKind::Pcm16 => SampleFormat::Pcm16,
            FormatKind::Companded8 => SampleFormat::Companded8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownmixKind {
    /// Keep the left channel, as DWIN's tools do.
    #[default]
    Left,
    /// Average all channels.
    Average,
}

impl From<DownmixKind> for Downmix {
    fn from(kind: DownmixKind) -> Self {
        match kind {
            DownmixKind::Left => Downmix::Left,
            DownmixKind::Average => Downmix::Average,
        }
    }
}

/// A `[page.<name>]` table. Controls name pages; only this gives the ID.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageSpec {
    /// Page ID, below 4092: the page shows image `id` of the background ICL,
    /// and the firmware switches to it by this ID.
    pub id: u16,
    /// Names of its controls, in drawing order. Touch and display controls
    /// may be mixed; the build keeps the order of each kind.
    #[serde(default)]
    pub controls: Vec<String>,
}

/// A `[control.<name>]` table, selected by its `type` key.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    /// "Return key code" touch control: writes `value` to `vp` when pressed.
    Button(Button),
    /// "Basic touch" control: a keyboard key, sending `code` to the data
    /// input being edited.
    Key(Key),
    /// "Data input" touch control: types a number into `vp` on a keyboard.
    Input(Input),
    /// "Variable icon": shows icon `icon_min + (value − val_min)` of ICL
    /// `icon_lib`.
    Icon(Icon),
    /// "Data variable": shows a VP as a number.
    Data(Data),
    /// "Text display": shows text stored from `vp` on.
    Text(Text),
    /// "Progress bar": fills `area` in proportion to the VP value.
    Progress(Progress),
}

impl Control {
    /// Whether the control goes to the touch file.
    pub fn is_touch(&self) -> bool {
        matches!(
            self,
            Control::Button(_) | Control::Key(_) | Control::Input(_)
        )
    }
}

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    /// The rectangle as the files store it. Coordinates past `u16::MAX` are
    /// reported by the check, so they saturate here.
    pub fn to_area(self) -> Area {
        Area::new(
            self.x,
            self.y,
            self.x.saturating_add(self.w),
            self.y.saturating_add(self.h),
        )
    }

    /// Right and bottom edges, which may exceed `u16`.
    pub fn end(self) -> (u32, u32) {
        (
            u32::from(self.x) + u32::from(self.w),
            u32::from(self.y) + u32::from(self.h),
        )
    }
}

/// A position in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pos {
    pub x: u16,
    pub y: u16,
}

impl From<Pos> for Point {
    fn from(pos: Pos) -> Self {
        Point::new(pos.x, pos.y)
    }
}

/// A colour, written `"#RRGGBB"`. The files store RGB565.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Drops the low bits of each channel, as the DGUS editor does.
    pub fn to_rgb565(self) -> u16 {
        u16::from(self.r >> 3) << 11 | u16::from(self.g >> 2) << 5 | u16::from(self.b >> 3)
    }
}

/// `#RRGGBB`, in upper case.
impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// `#RRGGBB`, in either case.
impl std::str::FromStr for Color {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let color = || {
            let hex = text.strip_prefix('#')?;
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let rgb = u32::from_str_radix(hex, 16).ok()?;
            Some(Color {
                r: (rgb >> 16) as u8,
                g: (rgb >> 8) as u8,
                b: rgb as u8,
            })
        };
        color().ok_or_else(|| format!("{text:?} is not a colour such as \"#FF8000\""))
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColorVisitor;

        impl de::Visitor<'_> for ColorVisitor {
            type Value = Color;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a colour \"#RRGGBB\"")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<Color, E> {
                text.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(ColorVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Button {
    pub area: Rect,
    /// Name of the page to switch to on release.
    pub next_page: Option<String>,
    /// Name of the page whose pixels show over the area while it is
    /// pressed.
    pub pressed_page: Option<String>,
    pub vp: u16,
    /// Which part of the VP receives `value`: `"word"`, `"high_byte"`,
    /// `"low_byte"` or `{ bit = n }`.
    #[serde(default)]
    pub target: KeyTarget,
    pub value: u16,
    /// Hold time in 0.1 s units.
    #[serde(default)]
    pub hold_time: u8,
    /// Report the written VP over the UART.
    #[serde(default = "yes")]
    pub upload: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub area: Rect,
    /// Name of the page to switch to on release.
    pub next_page: Option<String>,
    /// Name of the page whose pixels show over the area while it is
    /// pressed.
    pub pressed_page: Option<String>,
    /// Key code: `0x30`-`0x39` digits, `0x2E` decimal point, `0x2D` sign,
    /// `0xF0` cancel, `0xF1` enter, `0xF2` backspace.
    pub code: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Pressing it starts input.
    pub area: Rect,
    /// Name of the page to switch to on release.
    pub next_page: Option<String>,
    /// Name of the page whose pixels show over the area while it is
    /// pressed.
    pub pressed_page: Option<String>,
    pub vp: u16,
    #[serde(default)]
    pub data_type: InputDataType,
    #[serde(default = "default_input_integer_digits")]
    pub integer_digits: u8,
    #[serde(default)]
    pub decimal_digits: u8,
    /// The cursor; the number being typed is drawn leftward from it.
    pub position: Pos,
    #[serde(default)]
    pub color: Color,
    /// Font library ID.
    #[serde(default)]
    pub font: u8,
    #[serde(default = "default_input_font_size")]
    pub font_size: u8,
    #[serde(default)]
    pub cursor_color: CursorColor,
    /// Show `*` instead of the typed characters.
    #[serde(default)]
    pub masked: bool,
    /// A keyboard on another page; without it the keys are on this page.
    pub keyboard: Option<KeyboardPage>,
    /// Allowed range, ignoring the decimal point.
    pub limits: Option<Limits>,
    /// The editor's "Loading data when input": a VP that receives
    /// `load_value` during input.
    pub load_vp: Option<u16>,
    #[serde(default)]
    pub load_value: u16,
    /// 0-255.
    #[serde(default)]
    pub keyboard_transparency: u8,
    /// Report the written VP over the UART.
    #[serde(default = "yes")]
    pub upload: bool,
}

fn default_input_integer_digits() -> u8 {
    DataInput::default().integer_digits
}

fn default_input_font_size() -> u8 {
    DataInput::default().font_size
}

/// A keyboard drawn from another page while input is active.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyboardPage {
    /// Name of the page holding the keyboard image and its `key` controls.
    pub page: String,
    /// The keyboard on that page.
    pub area: Rect,
    /// Where the keyboard is drawn on the input's page.
    pub position: Pos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Icon {
    pub vp: u16,
    /// Description pointer.
    pub sp: Option<u16>,
    #[serde(default)]
    pub popup_menu: bool,
    pub position: Pos,
    /// File number of the ICL holding the icons; 0 (the default) is the
    /// active background ICL.
    #[serde(default)]
    pub icon_lib: u8,
    /// VP value shown as `icon_min`.
    #[serde(default)]
    pub val_min: u16,
    /// VP value shown as `icon_max`.
    pub val_max: u16,
    pub icon_min: u16,
    pub icon_max: u16,
    #[serde(default)]
    pub show_background: bool,
    #[serde(default)]
    pub layer_mode: LayerMode,
    /// 0-255.
    #[serde(default)]
    pub icon_brightness: u8,
    /// 0-255.
    #[serde(default)]
    pub background_brightness: u8,
    #[serde(default = "default_filter")]
    pub filter: u8,
}

fn default_filter() -> u8 {
    IconVar::default().filter
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    pub vp: u16,
    /// Description pointer.
    pub sp: Option<u16>,
    #[serde(default)]
    pub popup_menu: bool,
    pub position: Pos,
    #[serde(default)]
    pub color: Color,
    /// Font library ID.
    #[serde(default)]
    pub font: u8,
    #[serde(default = "default_font_size")]
    pub font_size: u8,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub auto_spacing: bool,
    #[serde(default)]
    pub leading_zeros: bool,
    #[serde(default = "default_integer_digits")]
    pub integer_digits: u8,
    #[serde(default)]
    pub decimal_digits: u8,
    #[serde(default)]
    pub data_type: DataType,
    /// ASCII, at most 11 bytes.
    #[serde(default)]
    pub unit: String,
}

fn default_font_size() -> u8 {
    DataVar::default().font_size
}

fn default_integer_digits() -> u8 {
    DataVar::default().integer_digits
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub vp: u16,
    /// Description pointer.
    pub sp: Option<u16>,
    #[serde(default)]
    pub popup_menu: bool,
    pub area: Rect,
    #[serde(default)]
    pub color: Color,
    /// Maximum text length in bytes.
    #[serde(default = "default_text_length")]
    pub length: u16,
    #[serde(default)]
    pub encoding: Encoding,
    /// Font for ASCII characters in the multi-byte encodings.
    #[serde(default)]
    pub ascii_font: u8,
    /// Font for everything else.
    #[serde(default)]
    pub font: u8,
    #[serde(default = "default_font_width")]
    pub font_width: u8,
    #[serde(default = "default_font_height")]
    pub font_height: u8,
    #[serde(default)]
    pub align: TextAlign,
    #[serde(default)]
    pub vertical_align: VerticalAlign,
    #[serde(default)]
    pub auto_spacing: bool,
    #[serde(default)]
    pub char_spacing: u8,
    #[serde(default)]
    pub line_spacing: u8,
}

fn default_text_length() -> u16 {
    TextVar::default().length
}

fn default_font_width() -> u8 {
    TextVar::default().font_width
}

fn default_font_height() -> u8 {
    TextVar::default().font_height
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub vp: u16,
    /// Description pointer.
    pub sp: Option<u16>,
    pub area: Rect,
    /// Value shown as an empty bar.
    #[serde(default)]
    pub val_min: i16,
    /// Value shown as a full bar.
    pub val_max: i16,
    #[serde(default)]
    pub data_type: BarDataType,
    #[serde(default)]
    pub direction: BarDirection,
    /// The filled part.
    #[serde(default)]
    pub bar_color: Color,
    #[serde(default)]
    pub border_color: Color,
    /// The empty part.
    #[serde(default)]
    pub background_color: Color,
    #[serde(default)]
    pub show_border: bool,
    #[serde(default)]
    pub fill_background: bool,
    /// The editor's "rate addr".
    pub rate_vp: Option<u16>,
}

/// A touch control's area and pages. `page_id` gives the ID of a page by
/// name; names it does not know are left out.
fn touch(
    area: Rect,
    next_page: Option<&str>,
    pressed_page: Option<&str>,
    page_id: impl Fn(&str) -> Option<u16>,
    action: TouchAction,
) -> Touch {
    Touch {
        area: area.to_area(),
        next_page: next_page.and_then(&page_id),
        pressed_page: pressed_page.and_then(&page_id),
        action,
    }
}

impl Button {
    /// The control as the touch file holds it. `page_id` gives the ID of a
    /// page by name; names it does not know are left out, and the check
    /// reports them.
    pub fn to_touch(&self, page_id: impl Fn(&str) -> Option<u16>) -> Touch {
        touch(
            self.area,
            self.next_page.as_deref(),
            self.pressed_page.as_deref(),
            page_id,
            TouchAction::KeyCode(KeyCode {
                vp: self.vp,
                target: self.target,
                value: self.value,
                hold_time: self.hold_time,
                upload: self.upload,
            }),
        )
    }
}

impl Key {
    /// Like [`Button::to_touch`].
    pub fn to_touch(&self, page_id: impl Fn(&str) -> Option<u16>) -> Touch {
        touch(
            self.area,
            self.next_page.as_deref(),
            self.pressed_page.as_deref(),
            page_id,
            TouchAction::Basic(self.code),
        )
    }
}

impl Input {
    /// Like [`Button::to_touch`]. A keyboard on a page `page_id` does not
    /// know becomes the keys of the input's own page.
    pub fn to_touch(&self, page_id: impl Fn(&str) -> Option<u16>) -> Touch {
        let keyboard = self
            .keyboard
            .as_ref()
            .and_then(|keyboard| {
                Some(Keyboard::Page {
                    page: page_id(&keyboard.page)?,
                    area: keyboard.area.to_area(),
                    position: keyboard.position.into(),
                })
            })
            .unwrap_or(Keyboard::OnPage);
        touch(
            self.area,
            self.next_page.as_deref(),
            self.pressed_page.as_deref(),
            page_id,
            TouchAction::DataInput(DataInput {
                vp: self.vp,
                data_type: self.data_type,
                integer_digits: self.integer_digits,
                decimal_digits: self.decimal_digits,
                position: self.position.into(),
                color: self.color.to_rgb565(),
                font: self.font,
                font_size: self.font_size,
                cursor_color: self.cursor_color,
                masked: self.masked,
                keyboard,
                limits: self.limits.map(|limits| limits.min..=limits.max),
                load_vp: self.load_vp,
                load_value: self.load_value,
                keyboard_transparency: self.keyboard_transparency,
                upload: self.upload,
            }),
        )
    }
}

impl Icon {
    pub fn to_display(&self) -> Display {
        Display::Icon(IconVar {
            vp: self.vp,
            sp: self.sp,
            popup_menu: self.popup_menu,
            position: self.position.into(),
            val_min: self.val_min,
            val_max: self.val_max,
            icon_min: self.icon_min,
            icon_max: self.icon_max,
            icon_lib: self.icon_lib,
            show_background: self.show_background,
            layer_mode: self.layer_mode,
            icon_brightness: self.icon_brightness,
            background_brightness: self.background_brightness,
            filter: self.filter,
        })
    }
}

impl Data {
    pub fn to_display(&self) -> Display {
        Display::Data(DataVar {
            vp: self.vp,
            sp: self.sp,
            popup_menu: self.popup_menu,
            position: self.position.into(),
            color: self.color.to_rgb565(),
            font: self.font,
            font_size: self.font_size,
            align: self.align,
            auto_spacing: self.auto_spacing,
            leading_zeros: self.leading_zeros,
            integer_digits: self.integer_digits,
            decimal_digits: self.decimal_digits,
            data_type: self.data_type,
            unit: self.unit.clone(),
        })
    }
}

impl Text {
    pub fn to_display(&self) -> Display {
        Display::Text(TextVar {
            vp: self.vp,
            sp: self.sp,
            popup_menu: self.popup_menu,
            area: self.area.to_area(),
            color: self.color.to_rgb565(),
            length: self.length,
            encoding: self.encoding,
            ascii_font: self.ascii_font,
            font: self.font,
            font_width: self.font_width,
            font_height: self.font_height,
            align: self.align,
            vertical_align: self.vertical_align,
            auto_spacing: self.auto_spacing,
            char_spacing: self.char_spacing,
            line_spacing: self.line_spacing,
        })
    }
}

impl Progress {
    pub fn to_display(&self) -> Display {
        Display::Progress(ProgressBar {
            vp: self.vp,
            sp: self.sp,
            area: self.area.to_area(),
            val_min: self.val_min,
            val_max: self.val_max,
            data_type: self.data_type,
            direction: self.direction,
            bar_color: self.bar_color.to_rgb565(),
            border_color: self.border_color.to_rgb565(),
            background_color: self.background_color.to_rgb565(),
            show_border: self.show_border,
            fill_background: self.fill_background,
            rate_vp: self.rate_vp,
        })
    }
}
