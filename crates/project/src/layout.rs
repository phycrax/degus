//! Struct layouts: where each field of a `[struct.<name>]` is, as the
//! firmware sees it. Fields follow each other word by word unless `at`
//! places one; an array repeats the element every `stride` words.

use std::collections::BTreeSet;

use crate::model::{FieldKind, StructSpec, is_name};

/// A struct with every field placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructLayout {
    /// VP of the first element.
    pub vp: u16,
    /// Elements, 1 unless an array.
    pub count: u16,
    /// Words from one element to the next.
    pub stride: u16,
    /// Words an element takes, to the end of its last field.
    pub size: u16,
    pub fields: Vec<FieldLayout>,
}

/// A field with its place in the element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLayout {
    pub name: String,
    pub kind: FieldKind,
    /// Words from the start of the element.
    pub offset: u16,
    pub words: u16,
    /// Bytes of a text field; 0 for the others.
    pub length: u16,
    /// An enum field's values, from 0; empty for the others.
    pub variants: Vec<String>,
}

impl FieldLayout {
    /// What the field holds, as written in `type`, such as `text` or `u16`.
    pub fn type_name(&self) -> &'static str {
        self.kind.name()
    }
}

impl StructLayout {
    pub fn field(&self, name: &str) -> Option<&FieldLayout> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// The VP of element `index`, from which its fields are placed.
    pub fn element_vp(&self, index: u16) -> u16 {
        self.vp.wrapping_add(index.wrapping_mul(self.stride))
    }

    /// The words element `index` takes, as `start..end`.
    pub fn element_range(&self, index: u16) -> std::ops::Range<u32> {
        let start = u32::from(self.vp) + u32::from(index) * u32::from(self.stride);
        start..start + u32::from(self.size)
    }
}

impl StructSpec {
    /// Places the fields. A field that cannot be placed as written, such as
    /// one `at` an offset an earlier field takes, is placed anyway, and the
    /// problem returned with the layout.
    pub fn layout(&self) -> (StructLayout, Vec<String>) {
        let mut problems = Vec::new();
        let mut fields = Vec::new();
        let mut names = BTreeSet::new();
        let mut next = 0u32;
        // The furthest any field reaches, which `at` can leave behind `next`.
        let mut end = 0u32;
        for spec in &self.fields {
            let name = &spec.name;
            if !is_name(name) {
                problems.push(format!(
                    "field {name:?} is not a name: use lowercase letters, digits and _, \
                     starting with a letter"
                ));
            }
            if !names.insert(name.as_str()) {
                problems.push(format!("field {name} is declared twice"));
            }
            let length = match (spec.kind, spec.length) {
                (FieldKind::Text, Some(length)) if length > 0 => length,
                (FieldKind::Text, _) => {
                    problems.push(format!("text field {name} needs a length in bytes"));
                    2
                }
                (_, Some(_)) => {
                    problems.push(format!("field {name}: only a text field has a length"));
                    0
                }
                (_, None) => 0,
            };
            let variants = match (spec.kind, &spec.variants) {
                (FieldKind::Enum, Some(variants)) if !variants.is_empty() => {
                    let mut seen = BTreeSet::new();
                    for variant in variants {
                        if !is_name(variant) {
                            problems.push(format!(
                                "field {name}: variant {variant:?} is not a name: use \
                                 lowercase letters, digits and _, starting with a letter"
                            ));
                        }
                        if !seen.insert(variant) {
                            problems
                                .push(format!("field {name}: variant {variant} is listed twice"));
                        }
                    }
                    variants.clone()
                }
                (FieldKind::Enum, _) => {
                    problems.push(format!("enum field {name} needs variants"));
                    Vec::new()
                }
                (_, Some(_)) => {
                    problems.push(format!("field {name}: only an enum field has variants"));
                    Vec::new()
                }
                (_, None) => Vec::new(),
            };
            if let Some(at) = spec.at {
                if u32::from(at) < next {
                    problems.push(format!(
                        "field {name} at 0x{at:02X} overlaps the fields before it, which end \
                         at 0x{next:02X}"
                    ));
                }
                next = u32::from(at);
            }
            let words = spec.kind.words(length);
            fields.push(FieldLayout {
                name: name.clone(),
                kind: spec.kind,
                offset: next.min(u32::from(u16::MAX)) as u16,
                words,
                length,
                variants,
            });
            next += u32::from(words);
            end = end.max(next);
        }
        if self.fields.is_empty() {
            problems.push("has no fields".to_owned());
        }
        let size = end.min(u32::from(u16::MAX)) as u16;
        let count = self.count;
        if count == 0 {
            problems.push("count must be at least 1".to_owned());
        }
        let stride = self.stride.unwrap_or(size);
        if count > 1 && stride < size {
            problems.push(format!(
                "stride 0x{stride:02X} is less than the 0x{size:02X} words of an element"
            ));
        }
        let end = u32::from(self.vp)
            + u32::from(count.saturating_sub(1)) * u32::from(stride)
            + u32::from(size);
        if end > 0x1_0000 {
            problems.push(format!(
                "runs from VP 0x{:04X} past 0xFFFF, to 0x{end:X}",
                self.vp
            ));
        }
        let layout = StructLayout {
            vp: self.vp,
            count: count.max(1),
            stride,
            size,
            fields,
        };
        (layout, problems)
    }
}
