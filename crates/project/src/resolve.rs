//! Turns a control as written into what the display files hold: its field
//! becomes a VP and gives the data type, length or values; its positions
//! move by its group's origin; page and variant names become numbers.

use std::collections::BTreeMap;

use degus_controls::{
    Area, BarDataType, DataInput, DataType, DataVar, Display, IconVar, InputDataType, KeyCode,
    Keyboard, Point, ProgressBar, TextVar, Touch, TouchAction,
};

use crate::layout::{FieldLayout, StructLayout};
use crate::model::{
    Button, Control, Data, FieldKind, Icon, Input, Key, NumberOrName, Pos, Progress, Rect, Text,
};

/// What controls name: structs by name, and pages by name.
pub(crate) struct Lookup<'a> {
    pub layouts: &'a BTreeMap<&'a str, StructLayout>,
    pub page_id: &'a dyn Fn(&str) -> Option<u16>,
}

/// Where a control is drawn. Outside a group, nothing moves it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Scope<'a> {
    /// Added to the control's positions.
    pub origin: (i64, i64),
    /// The struct the control's group binds, and the element its copy
    /// draws.
    pub element: Option<(&'a str, u16)>,
}

/// A control as the display files hold it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Resolved {
    Touch(Touch),
    Display(Display),
}

/// A resolved control and what did not resolve.
#[derive(Debug, Clone)]
pub(crate) struct Resolution {
    pub control: Resolved,
    /// Names the control gives that do not fit: fields, pages, variants.
    /// They do not depend on the group's copy.
    pub errors: Vec<String>,
    /// Positions the copy's origin moves outside the display's
    /// coordinates.
    pub geometry: Vec<String>,
}

pub(crate) fn resolve(control: &Control, scope: Scope, lookup: &Lookup) -> Resolution {
    let mut r = Resolver {
        scope,
        lookup,
        errors: Vec::new(),
        geometry: Vec::new(),
    };
    let control = match control {
        Control::Button(button) => Resolved::Touch(r.button(button)),
        Control::Key(key) => Resolved::Touch(r.key(key)),
        Control::Input(input) => Resolved::Touch(r.input(input)),
        Control::Icon(icon) => Resolved::Display(r.icon(icon)),
        Control::Data(data) => Resolved::Display(r.data(data)),
        Control::Text(text) => Resolved::Display(r.text(text)),
        Control::Progress(bar) => Resolved::Display(r.progress(bar)),
    };
    Resolution {
        control,
        errors: r.errors,
        geometry: r.geometry,
    }
}

/// A struct field as a control names it: `field` in a group bound to a
/// struct, `name.field`, or `name[n].field` in an array.
pub(crate) fn find_field<'a>(
    reference: &str,
    scope: Scope,
    layouts: &'a BTreeMap<&str, StructLayout>,
) -> Result<(u16, &'a FieldLayout), String> {
    let (name, index, field) = match reference.split_once('.') {
        None => match scope.element {
            Some((name, index)) => (name, Some(index), reference),
            None => {
                return Err(format!(
                    "field {reference} names no struct: write <struct>.{reference}, or bind \
                     the control's group to a struct"
                ));
            }
        },
        Some((head, field)) => match head.split_once('[') {
            None => (head, None, field),
            Some((name, rest)) => {
                let index = rest
                    .strip_suffix(']')
                    .and_then(|index| index.parse::<u16>().ok())
                    .ok_or_else(|| {
                        format!(
                            "field {reference:?} is not <struct>.<field> or <struct>[n].<field>"
                        )
                    })?;
                (name, Some(index), field)
            }
        },
    };
    let layout = layouts
        .get(name)
        .ok_or_else(|| format!("field {reference}: {name} is not a declared struct"))?;
    let index = match index {
        Some(_) if layout.count == 1 && scope.element.map(|(n, _)| n) != Some(name) => {
            return Err(format!(
                "field {reference}: {name} is not an array, so it takes no index"
            ));
        }
        Some(index) if index >= layout.count => {
            return Err(format!(
                "field {reference}: {name} has {} elements, from 0",
                layout.count
            ));
        }
        Some(index) => index,
        None if layout.count > 1 => {
            return Err(format!(
                "field {reference}: {name} is an array: write {name}[n].{field}, or bind the \
                 control's group to {name}"
            ));
        }
        None => 0,
    };
    let found = layout
        .field(field)
        .ok_or_else(|| format!("field {reference}: {name} has no field {field}"))?;
    let vp = layout.element_vp(index).wrapping_add(found.offset);
    Ok((vp, found))
}

struct Resolver<'a, 'b> {
    scope: Scope<'a>,
    lookup: &'b Lookup<'b>,
    errors: Vec<String>,
    geometry: Vec<String>,
}

impl<'b> Resolver<'_, 'b> {
    /// The VP a control names, and the field if it names one.
    fn binding(&mut self, vp: Option<u16>, field: Option<&str>) -> (u16, Option<&'b FieldLayout>) {
        match (vp, field) {
            (Some(vp), Some(_)) => {
                self.errors.push("takes vp or field, not both".to_owned());
                (vp, None)
            }
            (Some(vp), None) => (vp, None),
            (None, None) => {
                self.errors.push("needs vp or field".to_owned());
                (0, None)
            }
            (None, Some(reference)) => {
                match find_field(reference, self.scope, self.lookup.layouts) {
                    Ok((vp, field)) => (vp, Some(field)),
                    Err(message) => {
                        self.errors.push(message);
                        (0, None)
                    }
                }
            }
        }
    }

    /// Reports a field of a kind the control cannot use.
    fn wrong_field(&mut self, control: &str, field: &FieldLayout, wants: &str) {
        self.errors.push(format!(
            "{control} needs {wants}; field {} is {}",
            field.name,
            field.type_name()
        ));
    }

    /// Reports a key the field already gives.
    fn given(&mut self, key: &str, field: &FieldLayout) {
        self.errors.push(format!(
            "{key} comes from field {}; leave it out",
            field.name
        ));
    }

    fn one_word(&mut self, control: &str, field: Option<&FieldLayout>) {
        if let Some(field) = field
            && (field.words != 1 || field.kind == FieldKind::Text)
        {
            self.wrong_field(control, field, "a one-word field");
        }
    }

    fn page(&mut self, key: &str, name: Option<&str>) -> Option<u16> {
        let name = name?;
        let id = (self.lookup.page_id)(name);
        if id.is_none() {
            self.errors
                .push(format!("{key} {name} is not a declared page"));
        }
        id
    }

    /// A number, or a name: a variant of `field`, or `index` in a group's
    /// copy.
    fn value(&mut self, key: &str, value: &NumberOrName, field: Option<&FieldLayout>) -> u16 {
        let name = match value {
            NumberOrName::Number(number) => return *number,
            NumberOrName::Name(name) => name,
        };
        if let Some(field) = field
            && let Some(index) = field.variants.iter().position(|v| v == name)
        {
            return index as u16;
        }
        if name == "index" {
            if let Some((_, index)) = self.scope.element {
                return index;
            }
            self.errors.push(format!(
                "{key} index is the element a group's copy draws, and this control is in no \
                 group bound to a struct"
            ));
            return 0;
        }
        let message = match field {
            Some(field) if !field.variants.is_empty() => format!(
                "{key} {name} is not a variant of field {}: {}",
                field.name,
                field.variants.join(", ")
            ),
            Some(field) => format!(
                "{key} {name:?} names a variant, but field {} is {}",
                field.name,
                field.type_name()
            ),
            None => format!("{key} {name:?} names a variant, which needs an enum field"),
        };
        self.errors.push(message);
        0
    }

    /// `pos` moved by the origin.
    fn pos(&mut self, key: &str, pos: Pos) -> Point {
        let (x, y) = self.moved(key, pos.x, pos.y, 0, 0);
        Point::new(x, y)
    }

    /// `rect` moved by the origin.
    fn rect(&mut self, key: &str, rect: Rect) -> Area {
        let (x, y) = self.moved(key, rect.x, rect.y, rect.w, rect.h);
        Area::new(x, y, x.saturating_add(rect.w), y.saturating_add(rect.h))
    }

    fn moved(&mut self, key: &str, x: u16, y: u16, w: u16, h: u16) -> (u16, u16) {
        let (dx, dy) = self.scope.origin;
        let (x, y) = (i64::from(x) + dx, i64::from(y) + dy);
        let max = i64::from(u16::MAX);
        if x < 0 || y < 0 || x + i64::from(w) > max || y + i64::from(h) > max {
            self.geometry.push(format!(
                "{key} at ({x}, {y}) is outside the display's coordinates"
            ));
        }
        (x.clamp(0, max) as u16, y.clamp(0, max) as u16)
    }

    fn touch(
        &mut self,
        area: Rect,
        next: Option<&str>,
        pressed: Option<&str>,
        action: TouchAction,
    ) -> Touch {
        Touch {
            area: self.rect("area", area),
            next_page: self.page("next_page", next),
            pressed_page: self.page("pressed_page", pressed),
            action,
        }
    }

    fn button(&mut self, button: &Button) -> Touch {
        let (vp, field) = self.binding(button.vp, button.field.as_deref());
        self.one_word("a button", field);
        let value = self.value("value", &button.value, field);
        let action = TouchAction::KeyCode(KeyCode {
            vp,
            target: button.target,
            value,
            hold_time: button.hold_time,
            upload: button.upload,
        });
        self.touch(
            button.area,
            button.next_page.as_deref(),
            button.pressed_page.as_deref(),
            action,
        )
    }

    fn key(&mut self, key: &Key) -> Touch {
        self.touch(
            key.area,
            key.next_page.as_deref(),
            key.pressed_page.as_deref(),
            TouchAction::Basic(key.code),
        )
    }

    fn input(&mut self, input: &Input) -> Touch {
        let (vp, field) = self.binding(input.vp, input.field.as_deref());
        let data_type = match field {
            None => input.data_type.unwrap_or_default(),
            Some(field) => {
                if input.data_type.is_some() {
                    self.given("data_type", field);
                }
                match field.kind {
                    FieldKind::U16 | FieldKind::I16 => InputDataType::I16,
                    FieldKind::U32 | FieldKind::I32 => InputDataType::I32,
                    FieldKind::I64 => InputDataType::I64,
                    FieldKind::F32 => InputDataType::Float,
                    _ => {
                        self.wrong_field("an input", field, "a number field other than f64");
                        InputDataType::default()
                    }
                }
            }
        };
        let keyboard = match &input.keyboard {
            None => Keyboard::OnPage,
            Some(keyboard) => match self.page("keyboard page", Some(&keyboard.page)) {
                None => Keyboard::OnPage,
                Some(page) => Keyboard::Page {
                    page,
                    // The keyboard's area is on its own page, which the
                    // group's origin does not move.
                    area: Area::new(
                        keyboard.area.x,
                        keyboard.area.y,
                        keyboard.area.x.saturating_add(keyboard.area.w),
                        keyboard.area.y.saturating_add(keyboard.area.h),
                    ),
                    position: self.pos("keyboard position", keyboard.position),
                },
            },
        };
        let action = TouchAction::DataInput(DataInput {
            vp,
            data_type,
            integer_digits: input.integer_digits,
            decimal_digits: input.decimal_digits,
            position: self.pos("position", input.position),
            color: input.color.to_rgb565(),
            font: input.font,
            font_size: input.font_size,
            cursor_color: input.cursor_color,
            masked: input.masked,
            keyboard,
            limits: input.limits.map(|limits| limits.min..=limits.max),
            load_vp: input.load_vp,
            load_value: input.load_value,
            keyboard_transparency: input.keyboard_transparency,
            upload: input.upload,
        });
        self.touch(
            input.area,
            input.next_page.as_deref(),
            input.pressed_page.as_deref(),
            action,
        )
    }

    fn icon(&mut self, icon: &Icon) -> Display {
        let (vp, field) = self.binding(icon.vp, icon.field.as_deref());
        self.one_word("an icon", field);
        let (val_min, val_max) = match &icon.when {
            Some(when) => {
                if icon.val_min.is_some() || icon.val_max.is_some() {
                    self.errors
                        .push("when sets val_min and val_max; leave them out".to_owned());
                }
                let value = self.value("when", when, field);
                (value, value)
            }
            None => {
                let val_min = match &icon.val_min {
                    Some(value) => self.value("val_min", value, field),
                    None => 0,
                };
                let last = field
                    .filter(|field| !field.variants.is_empty())
                    .map(|field| field.variants.len() as u16 - 1);
                let val_max = match (&icon.val_max, last) {
                    (Some(value), _) => self.value("val_max", value, field),
                    (None, Some(last)) => last,
                    (None, None) => {
                        self.errors
                            .push("needs val_max, unless its field is an enum".to_owned());
                        val_min
                    }
                };
                (val_min, val_max)
            }
        };
        let icon_max = icon.icon_max.unwrap_or_else(|| {
            icon.icon_min
                .saturating_add(val_max.saturating_sub(val_min))
        });
        Display::Icon(IconVar {
            vp,
            sp: icon.sp,
            popup_menu: icon.popup_menu,
            position: self.pos("position", icon.position),
            val_min,
            val_max,
            icon_min: icon.icon_min,
            icon_max,
            icon_lib: icon.icon_lib,
            show_background: icon.show_background,
            layer_mode: icon.layer_mode,
            icon_brightness: icon.icon_brightness,
            background_brightness: icon.background_brightness,
            filter: icon.filter,
        })
    }

    fn data(&mut self, data: &Data) -> Display {
        let (vp, field) = self.binding(data.vp, data.field.as_deref());
        let data_type = match field {
            None => data.data_type.unwrap_or_default(),
            Some(field) => {
                if data.data_type.is_some() {
                    self.given("data_type", field);
                }
                match field.kind {
                    FieldKind::U16 | FieldKind::Bool | FieldKind::Enum => DataType::U16,
                    FieldKind::I16 => DataType::I16,
                    FieldKind::U32 => DataType::U32,
                    FieldKind::I32 => DataType::I32,
                    FieldKind::I64 => DataType::I64,
                    FieldKind::F32 => DataType::F32,
                    FieldKind::F64 => DataType::F64,
                    FieldKind::Text => {
                        self.wrong_field("a data control", field, "a number field");
                        DataType::default()
                    }
                }
            }
        };
        Display::Data(DataVar {
            vp,
            sp: data.sp,
            popup_menu: data.popup_menu,
            position: self.pos("position", data.position),
            color: data.color.to_rgb565(),
            font: data.font,
            font_size: data.font_size,
            align: data.align,
            auto_spacing: data.auto_spacing,
            leading_zeros: data.leading_zeros,
            integer_digits: data.integer_digits,
            decimal_digits: data.decimal_digits,
            data_type,
            unit: data.unit.clone(),
        })
    }

    fn text(&mut self, text: &Text) -> Display {
        let (vp, field) = self.binding(text.vp, text.field.as_deref());
        let length = match field {
            None => text.length.unwrap_or(TextVar::default().length),
            Some(field) => {
                if text.length.is_some() {
                    self.given("length", field);
                }
                if field.kind != FieldKind::Text {
                    self.wrong_field("a text control", field, "a text field");
                }
                field.length
            }
        };
        Display::Text(TextVar {
            vp,
            sp: text.sp,
            popup_menu: text.popup_menu,
            area: self.rect("area", text.area),
            color: text.color.to_rgb565(),
            length,
            encoding: text.encoding,
            ascii_font: text.ascii_font,
            font: text.font,
            font_width: text.font_width,
            font_height: text.font_height,
            align: text.align,
            vertical_align: text.vertical_align,
            auto_spacing: text.auto_spacing,
            char_spacing: text.char_spacing,
            line_spacing: text.line_spacing,
        })
    }

    fn progress(&mut self, bar: &Progress) -> Display {
        let (vp, field) = self.binding(bar.vp, bar.field.as_deref());
        let data_type = match field {
            None => bar.data_type.unwrap_or_default(),
            Some(field) => {
                if bar.data_type.is_some() {
                    self.given("data_type", field);
                }
                if !matches!(field.kind, FieldKind::U16 | FieldKind::I16) {
                    self.wrong_field("a progress bar", field, "a u16 or i16 field");
                }
                BarDataType::I16
            }
        };
        Display::Progress(ProgressBar {
            vp,
            sp: bar.sp,
            area: self.rect("area", bar.area),
            val_min: bar.val_min,
            val_max: bar.val_max,
            data_type,
            direction: bar.direction,
            bar_color: bar.bar_color.to_rgb565(),
            border_color: bar.border_color.to_rgb565(),
            background_color: bar.background_color.to_rgb565(),
            show_border: bar.show_border,
            fill_background: bar.fill_background,
            rate_vp: bar.rate_vp,
        })
    }
}
