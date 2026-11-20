//! Structs, the fields controls bind, groups that repeat over an array, and
//! styles.

use std::path::{Path, PathBuf};

use degus_controls::{
    Area, DataType, DataVar, Display, IconVar, KeyCode, Point, TextVar, Touch, TouchAction,
};
use degus_project::{Error, GroupPlace, Project, Severity, SourceRef};
use image::{Rgb, RgbImage};

fn fresh_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("degus-structs")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

const PROJECT: &str = r##"
screen = { w = 800, h = 480 }
include = ["rows.toml"]

[config]
background_icl = 32

[[icl]]
id = 32
images = [{ id = 0, path = "page.png" }]

[[icl]]
id = 40
images = [
    { id = 10, path = "icon.png" },
    { id = 11, path = "icon.png" },
    { id = 12, path = "icon.png" },
    { id = 20, path = "icon.png" },
]

[struct.status]
vp = 0x1000
fields = [
    { name = "mode", type = "enum", variants = ["off", "heat", "cool"] },
    { name = "temp", type = "i16" },
    { name = "power", type = "u32" },
    { name = "label", type = "text", length = 10 },
    { name = "event", type = "enum", variants = ["start", "stop"], at = 0x10 },
]

[style.small]
type = "data"
color = "#FF0000"
font_size = 16

[page.home]
id = 0
controls = ["mode", "heating", "temp", "power", "label", "start", "stop", "rows"]

# Icons 10, 11, 12 for off, heat and cool.
[control.mode]
type = "icon"
field = "status.mode"
position = { x = 0, y = 0 }
icon_lib = 40
icon_min = 10

# Icon 20 while heating, nothing otherwise.
[control.heating]
type = "icon"
field = "status.mode"
when = "heat"
position = { x = 50, y = 0 }
icon_lib = 40
icon_min = 20

[control.temp]
style = "small"
field = "status.temp"
position = { x = 100, y = 0 }

[control.power]
style = "small"
field = "status.power"
position = { x = 200, y = 0 }
font_size = 24

[control.label]
type = "text"
field = "status.label"
area = { x = 300, y = 0, w = 100, h = 20 }

[control.start]
type = "button"
field = "status.event"
value = "start"
area = { x = 0, y = 400, w = 100, h = 50 }

[control.stop]
type = "button"
field = "status.event"
value = "stop"
area = { x = 100, y = 400, w = 100, h = 50 }
"##;

/// Three rows, each its name, its value and a tap that writes its index.
const ROWS: &str = r#"
[struct.row]
vp = 0x2000
count = 3
stride = 0x10
fields = [
    { name = "name", type = "text", length = 8 },
    { name = "value", type = "u16" },
]

[struct.rows_input]
vp = 0x2100
fields = [{ name = "open", type = "u16" }]

[group.rows]
struct = "row"
origin = { x = 10, y = 100 }
step = { y = 40 }

[[group.rows.control]]
type = "text"
field = "name"
area = { x = 0, y = 0, w = 80, h = 20 }

[[group.rows.control]]
type = "data"
field = "value"
position = { x = 100, y = 0 }

[[group.rows.control]]
type = "button"
field = "rows_input.open"
value = "index"
area = { x = 0, y = 0, w = 200, h = 30 }
"#;

fn sample(name: &str) -> PathBuf {
    let dir = fresh_dir(name);
    write(dir.join("project.toml"), PROJECT);
    write(dir.join("rows.toml"), ROWS);
    RgbImage::from_pixel(800, 480, Rgb([0, 0, 64]))
        .save(dir.join("page.png"))
        .unwrap();
    RgbImage::from_pixel(20, 20, Rgb([255, 255, 255]))
        .save(dir.join("icon.png"))
        .unwrap();
    dir
}

fn messages(project: &Project) -> Vec<String> {
    project.check().iter().map(ToString::to_string).collect()
}

#[test]
fn lays_out_structs_word_by_word() {
    let project = Project::load(sample("layout")).unwrap();
    let layouts = project.layouts();
    let status = &layouts["status"];
    let offsets: Vec<_> = status
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.offset, f.words))
        .collect();
    assert_eq!(
        offsets,
        [
            ("mode", 0, 1),
            ("temp", 1, 1),
            ("power", 2, 2),
            ("label", 4, 5),
            ("event", 0x10, 1),
        ]
    );
    assert_eq!((status.vp, status.count, status.size), (0x1000, 1, 0x11));
    let row = &layouts["row"];
    assert_eq!((row.count, row.stride, row.size), (3, 0x10, 5));
    assert_eq!(row.element_vp(2), 0x2020);
}

#[test]
fn binds_fields_and_repeats_groups_over_arrays() {
    let project = Project::load(sample("bind")).unwrap();
    assert_eq!(messages(&project), Vec::<String>::new());
    let (controls, sources) = project.to_controls_with_sources();
    let page = controls.get(0).unwrap();

    let icon = |vp, val: (u16, u16), icons: (u16, u16), x| {
        Display::Icon(IconVar {
            vp,
            position: Point::new(x, 0),
            val_min: val.0,
            val_max: val.1,
            icon_min: icons.0,
            icon_max: icons.1,
            icon_lib: 40,
            ..IconVar::default()
        })
    };
    let data = |vp, data_type, position, color, font_size| {
        Display::Data(DataVar {
            vp,
            position,
            color,
            font_size,
            data_type,
            ..DataVar::default()
        })
    };
    let text = |vp, length, area| {
        Display::Text(TextVar {
            vp,
            length,
            area,
            ..TextVar::default()
        })
    };
    let mut display = vec![
        // The enum's three values show icons 10 to 12; `when` picks one.
        icon(0x1000, (0, 2), (10, 12), 0),
        icon(0x1000, (1, 1), (20, 20), 50),
        // The style gives the type, the colour and the size, which power
        // overrides; the field gives the data type.
        data(0x1001, DataType::I16, Point::new(100, 0), 0xF800, 16),
        data(0x1002, DataType::U32, Point::new(200, 0), 0xF800, 24),
        text(0x1004, 10, Area::new(300, 0, 400, 20)),
    ];
    // Each row 0x10 words and 40 pixels below the one before.
    for (row, vp) in [(0u16, 0x2000), (1, 0x2010), (2, 0x2020)] {
        let y = 100 + 40 * row;
        display.push(text(vp, 8, Area::new(10, y, 90, y + 20)));
        display.push(data(vp + 4, DataType::U16, Point::new(110, y), 0, 8));
    }
    assert_eq!(page.display, display);

    let button = |vp, value, area| Touch {
        area,
        next_page: None,
        pressed_page: None,
        action: TouchAction::KeyCode(KeyCode {
            vp,
            target: Default::default(),
            value,
            hold_time: 0,
            upload: true,
        }),
    };
    let mut touch = vec![
        button(0x1010, 0, Area::new(0, 400, 100, 450)),
        button(0x1010, 1, Area::new(100, 400, 200, 450)),
    ];
    for row in 0..3u16 {
        let y = 100 + 40 * row;
        touch.push(button(0x2100, row, Area::new(10, y, 210, y + 30)));
    }
    assert_eq!(page.touch, touch);

    // A group's controls are known by the group, their place in it and the
    // element their copy draws.
    let source = &sources.page(0).unwrap().display[8];
    assert_eq!(
        source,
        &SourceRef {
            file: 1,
            name: "rows".into(),
            group: Some(GroupPlace {
                control: 1,
                element: Some(("row".into(), 1)),
            }),
        }
    );
    assert_eq!(
        source.location(Path::new("rows.toml")),
        "rows.toml: [group.rows] control 2 (row[1])"
    );
}

#[test]
fn reports_what_fields_and_groups_get_wrong() {
    let dir = sample("problems");
    write(
        dir.join("project.toml"),
        format!(
            "{PROJECT}{}",
            r##"
[struct.wide]
vp = 0x3000
fields = [{ name = "big", type = "u32" }]

[struct.inside]
vp = 0x3001
fields = [{ name = "small", type = "u16" }]

[struct.broken]
vp = 0x4000
count = 2
stride = 1
fields = [
    { name = "words", type = "text" },
    { name = "kind", type = "enum" },
    { name = "number", type = "u16", length = 4 },
    { name = "late", type = "u16", at = 0 },
]

[page.bad]
id = 1
controls = ["unknown_field", "wrong_type", "both", "neither", "no_struct", "no_index",
    "bad_variant", "loose_index", "given_type", "no_val_max", "lost_rows", "far_rows"]

[control.unknown_field]
type = "data"
field = "status.humidity"
position = { x = 0, y = 0 }

[control.wrong_type]
type = "text"
field = "status.temp"
area = { x = 0, y = 0, w = 10, h = 10 }

[control.both]
type = "data"
vp = 0x5000
field = "status.temp"
position = { x = 0, y = 0 }

[control.neither]
type = "data"
position = { x = 0, y = 0 }

[control.no_struct]
type = "data"
field = "temp"
position = { x = 0, y = 0 }

[control.no_index]
type = "data"
field = "row.value"
position = { x = 0, y = 0 }

[control.bad_variant]
type = "button"
field = "status.event"
value = "pause"
area = { x = 0, y = 0, w = 10, h = 10 }

[control.loose_index]
type = "button"
field = "status.event"
value = "index"
area = { x = 0, y = 0, w = 10, h = 10 }

[control.given_type]
type = "data"
field = "status.temp"
data_type = "u16"
position = { x = 0, y = 0 }

[control.no_val_max]
type = "icon"
vp = 0x5001
position = { x = 0, y = 0 }
icon_min = 1

[control.rows]
type = "data"
vp = 0x5002
position = { x = 0, y = 0 }

[group.lost_rows]
struct = "lost"

[[group.lost_rows.control]]
type = "data"
field = "value"
position = { x = 0, y = 0 }

# Its last copy runs past the bottom, and its control names no field it
# has, once for all three copies.
[group.far_rows]
struct = "row"
origin = { x = 0, y = 300 }
step = { y = 90 }

[[group.far_rows.control]]
type = "text"
field = "missing"
area = { x = 0, y = 0, w = 50, h = 20 }
"##
        ),
    );
    let project = Project::load(&dir).unwrap();
    let messages = messages(&project);
    let expect = |severity: Severity, needle: &str| {
        let prefix = match severity {
            Severity::Error => "error: ",
            Severity::Warning => "warning: ",
        };
        assert!(
            messages
                .iter()
                .any(|m| m.starts_with(prefix) && m.contains(needle)),
            "no {severity:?} containing {needle:?} in {messages:#?}"
        );
    };
    let at = "project.toml";

    // Structs.
    expect(
        Severity::Error,
        &format!("{at}: [struct.inside]: inside overlaps wide at VP 0x3001"),
    );
    expect(
        Severity::Error,
        &format!("{at}: [struct.broken]: text field words needs a length in bytes"),
    );
    expect(
        Severity::Error,
        &format!("{at}: [struct.broken]: enum field kind needs variants"),
    );
    expect(
        Severity::Error,
        &format!("{at}: [struct.broken]: field number: only a text field has a length"),
    );
    expect(
        Severity::Error,
        &format!(
            "{at}: [struct.broken]: field late at 0x00 overlaps the fields before it, which \
             end at 0x03"
        ),
    );
    expect(
        Severity::Error,
        &format!("{at}: [struct.broken]: stride 0x01 is less than the 0x03 words of an element"),
    );

    // Controls.
    expect(
        Severity::Error,
        "[control.unknown_field]: field status.humidity: status has no field humidity",
    );
    expect(
        Severity::Error,
        "[control.wrong_type]: a text control needs a text field; field temp is i16",
    );
    expect(
        Severity::Error,
        "[control.both]: takes vp or field, not both",
    );
    expect(Severity::Error, "[control.neither]: needs vp or field");
    expect(
        Severity::Error,
        "[control.no_struct]: field temp names no struct: write <struct>.temp",
    );
    expect(
        Severity::Error,
        "[control.no_index]: field row.value: row is an array: write row[n].value",
    );
    expect(
        Severity::Error,
        "[control.bad_variant]: value pause is not a variant of field event: start, stop",
    );
    expect(
        Severity::Error,
        "[control.loose_index]: value index is the element a group's copy draws",
    );
    expect(
        Severity::Error,
        "[control.given_type]: data_type comes from field temp; leave it out",
    );
    expect(
        Severity::Error,
        "[control.no_val_max]: needs val_max, unless its field is an enum",
    );

    // Groups.
    expect(
        Severity::Error,
        &format!("{at}: [group.lost_rows]: struct lost is not declared"),
    );
    expect(
        Severity::Error,
        "rows.toml: [group.rows]: rows is also a control, in project.toml",
    );
    expect(
        Severity::Error,
        &format!(
            "{at}: [group.far_rows] control 1 (row[2]): area (0, 480) 50x20 extends past the \
             800x480 screen"
        ),
    );
    let missing: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("row has no field missing"))
        .collect();
    assert_eq!(
        missing,
        [&format!(
            "error: {at}: [group.far_rows] control 1: field missing: row has no field missing"
        )]
    );
}

#[test]
fn styles_fill_in_what_controls_leave_out() {
    let dir = sample("styles");
    let project = Project::load(&dir).unwrap();
    let declared = &project.files[0].declarations;
    assert_eq!(declared.style["small"]["font_size"].as_integer(), Some(16));

    // A style nothing declares, and one using another, stop the load.
    let load = |text: String| {
        write(dir.join("project.toml"), text);
        Project::load(&dir).unwrap_err()
    };
    let error = load(PROJECT.replace(
        "style = \"small\"\nfield = \"status.temp\"",
        "style = \"tiny\"\nfield = \"status.temp\"",
    ));
    assert!(matches!(error, Error::Declaration { .. }), "{error}");
    assert_eq!(
        error.to_string(),
        "project.toml: [control.temp]: style tiny is not declared"
    );
    let error = load(PROJECT.replace("font_size = 16\n", "font_size = 16\nstyle = \"small\"\n"));
    assert_eq!(
        error.to_string(),
        "project.toml: [control.power]: style small names a style itself; a style cannot use \
         another"
    );

    // A group's controls take styles too, and their errors name their place.
    write(dir.join("project.toml"), PROJECT);
    write(
        dir.join("rows.toml"),
        ROWS.replacen(
            "type = \"data\"\nfield = \"value\"",
            "style = \"small\"\nfield = \"value\"\nfont = \"big\"",
            1,
        ),
    );
    let error = Project::load(&dir).unwrap_err().to_string();
    assert!(
        error.starts_with("rows.toml: [group.rows] control 2: invalid type"),
        "{error}"
    );
}
