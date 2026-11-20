//! The project description. `project.toml` holds the settings of the whole
//! project and includes other files; every file, `project.toml` too, holds
//! declarations: pages, structs, groups, controls, styles, libraries and
//! files to copy. Which file a declaration is in means nothing to the build.
//!
//! A struct lays out VPs the way the firmware sees them; controls bind its
//! fields by name, and groups place controls relative to an origin, once for
//! each element of the struct they bind.
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
    Align, BarDataType, BarDirection, CursorColor, DataInput, DataType, DataVar, Encoding, IconVar,
    InputDataType, KeyTarget, LayerMode, TextAlign, TextVar, VerticalAlign,
};
use degus_icl::{Chip, IclOptions, Subsampling};
use degus_wae::{Downmix, SampleFormat};
use serde::{Deserialize, Deserializer, de};

/// Name of the project file in a project folder.
pub const PROJECT_FILE_NAME: &str = "project.toml";

/// `project.toml` as written: the [`Settings`], then the declarations of any
/// file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootFile {
    screen: Size,
    #[serde(default)]
    chip: ChipKind,
    #[serde(default = "default_output")]
    output: PathBuf,
    #[serde(default)]
    include: Vec<PathBuf>,
    config: Option<ConfigSpec>,
    #[serde(default)]
    copy: Vec<PathBuf>,
    #[serde(default)]
    icl: Vec<IclSpec>,
    #[serde(default)]
    wae: Vec<WaeSpec>,
    #[serde(default)]
    page: BTreeMap<String, PageSpec>,
    #[serde(default, rename = "struct")]
    structs: BTreeMap<String, StructSpec>,
    #[serde(default)]
    group: BTreeMap<String, RawGroup>,
    #[serde(default)]
    control: BTreeMap<String, toml::Table>,
    #[serde(default)]
    style: BTreeMap<String, toml::Table>,
}

impl RootFile {
    pub(crate) fn split(self) -> (Settings, RawDeclarations) {
        let settings = Settings {
            screen: self.screen,
            chip: self.chip,
            output: self.output,
            include: self.include,
            config: self.config,
        };
        let declarations = RawDeclarations {
            copy: self.copy,
            icl: self.icl,
            wae: self.wae,
            page: self.page,
            structs: self.structs,
            group: self.group,
            control: self.control,
            style: self.style,
        };
        (settings, declarations)
    }
}

/// What a file declares as written. Controls stay tables until the project
/// applies their styles.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDeclarations {
    #[serde(default)]
    pub copy: Vec<PathBuf>,
    #[serde(default)]
    pub icl: Vec<IclSpec>,
    #[serde(default)]
    pub wae: Vec<WaeSpec>,
    #[serde(default)]
    pub page: BTreeMap<String, PageSpec>,
    #[serde(default, rename = "struct")]
    pub structs: BTreeMap<String, StructSpec>,
    #[serde(default)]
    pub group: BTreeMap<String, RawGroup>,
    #[serde(default)]
    pub control: BTreeMap<String, toml::Table>,
    #[serde(default)]
    pub style: BTreeMap<String, toml::Table>,
}

/// A group as written, its controls still tables.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGroup {
    #[serde(rename = "struct")]
    pub bound: Option<String>,
    #[serde(default)]
    pub origin: Pos,
    #[serde(default)]
    pub step: Step,
    #[serde(default)]
    pub control: Vec<toml::Table>,
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

/// What any file of a project declares. Names are unique across the files:
/// one set for pages, one for structs, one for styles, and one for the
/// controls and groups that pages list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Declarations {
    /// Files copied into the output unchanged, relative to the project root.
    pub copy: Vec<PathBuf>,
    /// Icon libraries.
    pub icl: Vec<IclSpec>,
    /// Audio libraries.
    pub wae: Vec<WaeSpec>,
    /// `[page.<name>]` tables.
    pub page: BTreeMap<String, PageSpec>,
    /// `[struct.<name>]` tables.
    pub structs: BTreeMap<String, StructSpec>,
    /// `[group.<name>]` tables.
    pub group: BTreeMap<String, GroupSpec>,
    /// `[control.<name>]` tables, with their styles applied. Their order
    /// means nothing; a page's list sets the drawing order.
    pub control: BTreeMap<String, Control>,
    /// `[style.<name>]` tables: keys a control naming the style takes unless
    /// it sets them.
    pub style: BTreeMap<String, toml::Table>,
}

/// A `[struct.<name>]` table: VPs laid out from `vp` the way the firmware
/// sees them. With a `count`, it is an array of `count` elements, `stride`
/// words apart.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructSpec {
    /// VP of the first field of the first element.
    pub vp: u16,
    /// Elements, 1 unless an array.
    #[serde(default = "one")]
    pub count: u16,
    /// Words from one element to the next; the element's size when not set.
    pub stride: Option<u16>,
    /// The fields in order, each after the one before unless `at` places it.
    pub fields: Vec<FieldSpec>,
}

fn one() -> u16 {
    1
}

/// A field of a struct.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    /// Bytes of a `text` field.
    pub length: Option<u16>,
    /// Names of an `enum` field's values, from 0.
    pub variants: Option<Vec<String>>,
    /// Offset in words from the start of the element.
    pub at: Option<u16>,
}

/// What a field holds, and so how many words it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    U16,
    I16,
    U32,
    I32,
    I64,
    F32,
    F64,
    /// A word, 0 or 1.
    Bool,
    /// A word holding the index of one of `variants`.
    Enum,
    /// `length` bytes, two to a word.
    Text,
}

impl FieldKind {
    /// Words the field takes; `length` is the bytes of a text field.
    pub fn words(self, length: u16) -> u16 {
        match self {
            FieldKind::U16 | FieldKind::I16 | FieldKind::Bool | FieldKind::Enum => 1,
            FieldKind::U32 | FieldKind::I32 | FieldKind::F32 => 2,
            FieldKind::I64 | FieldKind::F64 => 4,
            FieldKind::Text => length.div_ceil(2),
        }
    }

    /// The name written in `type`.
    pub fn name(self) -> &'static str {
        match self {
            FieldKind::U16 => "u16",
            FieldKind::I16 => "i16",
            FieldKind::U32 => "u32",
            FieldKind::I32 => "i32",
            FieldKind::I64 => "i64",
            FieldKind::F32 => "f32",
            FieldKind::F64 => "f64",
            FieldKind::Bool => "bool",
            FieldKind::Enum => "enum",
            FieldKind::Text => "text",
        }
    }
}

/// A `[group.<name>]` table: controls placed from `origin`, in drawing
/// order. Bound to a struct, the group is drawn once for each element,
/// moved by `step` each time, and its controls name the element's fields
/// without the struct.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSpec {
    /// The struct whose elements the group draws.
    pub bound: Option<String>,
    /// Where `(0, 0)` of its controls is on the screen.
    pub origin: Pos,
    /// How far each element's copy moves from the one before.
    pub step: Step,
    /// `[[group.<name>.control]]` tables, with their styles applied.
    pub controls: Vec<Control>,
}

/// A move in pixels, which may be negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
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

/// A control, selected by its `type` key. It names the VP it shows or
/// writes as `vp`, or as a struct's `field`, which then also gives what the
/// control is otherwise told: a data control's type, a text's length, the
/// values an icon shows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    /// "Return key code" touch control: writes `value` to its VP when
    /// pressed.
    Button(Button),
    /// "Basic touch" control: a keyboard key, sending `code` to the data
    /// input being edited.
    Key(Key),
    /// "Data input" touch control: types a number into its VP on a keyboard.
    Input(Input),
    /// "Variable icon": shows icon `icon_min + (value − val_min)` of ICL
    /// `icon_lib`.
    Icon(Icon),
    /// "Data variable": shows a VP as a number.
    Data(Data),
    /// "Text display": shows text stored from its VP on.
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

    /// The `type` it is written with.
    pub fn kind(&self) -> &'static str {
        match self {
            Control::Button(_) => "button",
            Control::Key(_) => "key",
            Control::Input(_) => "input",
            Control::Icon(_) => "icon",
            Control::Data(_) => "data",
            Control::Text(_) => "text",
            Control::Progress(_) => "progress",
        }
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

/// A position in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pos {
    pub x: u16,
    pub y: u16,
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

/// A word, written as a number or as a name: a variant of an enum field, or
/// `"index"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumberOrName {
    Number(u16),
    Name(String),
}

impl<'de> Deserialize<'de> for NumberOrName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl de::Visitor<'_> for Visitor {
            type Value = NumberOrName;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number 0-65535 or a variant's name")
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<NumberOrName, E> {
                u16::try_from(value)
                    .map(NumberOrName::Number)
                    .map_err(|_| E::custom(format!("{value} is not in 0-65535")))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<NumberOrName, E> {
                u16::try_from(value)
                    .map(NumberOrName::Number)
                    .map_err(|_| E::custom(format!("{value} is not in 0-65535")))
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<NumberOrName, E> {
                Ok(NumberOrName::Name(text.to_owned()))
            }
        }

        deserializer.deserialize_any(Visitor)
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
    pub vp: Option<u16>,
    /// The field written instead of `vp`.
    pub field: Option<String>,
    /// Which part of the VP receives `value`: `"word"`, `"high_byte"`,
    /// `"low_byte"` or `{ bit = n }`.
    #[serde(default)]
    pub target: KeyTarget,
    /// A number, a variant of the field, or `"index"`: the element the
    /// group's copy draws.
    pub value: NumberOrName,
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
    pub vp: Option<u16>,
    /// The field typed into instead of `vp`; it gives the data type.
    pub field: Option<String>,
    pub data_type: Option<InputDataType>,
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
    /// The keyboard on that page; a group's origin does not move it.
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
    pub vp: Option<u16>,
    /// The field shown instead of `vp`.
    pub field: Option<String>,
    /// Description pointer.
    pub sp: Option<u16>,
    #[serde(default)]
    pub popup_menu: bool,
    pub position: Pos,
    /// File number of the ICL holding the icons; 0 (the default) is the
    /// active background ICL.
    #[serde(default)]
    pub icon_lib: u8,
    /// Value shown as `icon_min`: 0, or the first variant of an enum field,
    /// unless set.
    pub val_min: Option<NumberOrName>,
    /// Value shown as `icon_max`: an enum field's last variant unless set.
    pub val_max: Option<NumberOrName>,
    /// One value, shown as `icon_min` and nothing else: sets `val_min` and
    /// `val_max` both.
    pub when: Option<NumberOrName>,
    pub icon_min: u16,
    /// `icon_min` plus as many icons as values unless set.
    pub icon_max: Option<u16>,
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
    pub vp: Option<u16>,
    /// The field shown instead of `vp`; it gives the data type.
    pub field: Option<String>,
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
    pub data_type: Option<DataType>,
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
    pub vp: Option<u16>,
    /// The field shown instead of `vp`; it gives the length.
    pub field: Option<String>,
    /// Description pointer.
    pub sp: Option<u16>,
    #[serde(default)]
    pub popup_menu: bool,
    pub area: Rect,
    #[serde(default)]
    pub color: Color,
    /// Maximum text length in bytes.
    pub length: Option<u16>,
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

fn default_font_width() -> u8 {
    TextVar::default().font_width
}

fn default_font_height() -> u8 {
    TextVar::default().font_height
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub vp: Option<u16>,
    /// The field shown instead of `vp`.
    pub field: Option<String>,
    /// Description pointer.
    pub sp: Option<u16>,
    pub area: Rect,
    /// Value shown as an empty bar.
    #[serde(default)]
    pub val_min: i16,
    /// Value shown as a full bar.
    pub val_max: i16,
    pub data_type: Option<BarDataType>,
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

/// Whether `name` is lowercase letters, digits and `_`, starting with a
/// letter, so code can use it as an identifier.
pub(crate) fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
