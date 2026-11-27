//! VP watches and the VP memory.

use std::fmt;
use std::time::Duration;

use degus_sim::VpSpace;
use degus_sim_gui::SimThread;
use degus_sim_gui::egui::{self, Button, Key, Sense};

use crate::command::int;
use crate::log::{Log, LogKind};

/// How often the VPs are shown anew: the display changes some of them
/// without a notice, such as the RTC and the touch state.
const REFRESH: Duration = Duration::from_millis(250);

/// How a watched VP is shown and edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchKind {
    Hex,
    U16,
    I16,
    /// Two words, high word first, as are the others below.
    U32,
    I32,
    F32,
}

impl WatchKind {
    pub const ALL: [Self; 6] = [
        Self::Hex,
        Self::U16,
        Self::I16,
        Self::U32,
        Self::I32,
        Self::F32,
    ];

    /// `hex`, `u16`, and so on.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::U16 => "u16",
            Self::I16 => "i16",
            Self::U32 => "u32",
            Self::I32 => "i32",
            Self::F32 => "f32",
        }
    }

    /// The kind [`name`](WatchKind::name) gives.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    fn show(self, v: &VpSpace, vp: u16) -> String {
        match self {
            Self::Hex => format!("{:#06X}", v.get(vp)),
            Self::U16 => v.get(vp).to_string(),
            Self::I16 => v.get(vp).cast_signed().to_string(),
            Self::U32 => v.u32(vp).to_string(),
            Self::I32 => v.u32(vp).cast_signed().to_string(),
            Self::F32 => f32::from_bits(v.u32(vp)).to_string(),
        }
    }

    /// The words that `text` stands for.
    fn parse(self, text: &str) -> Option<Vec<u16>> {
        let two = |v: u32| vec![(v >> 16) as u16, v as u16];
        let text = text.trim();
        Some(match self {
            Self::Hex | Self::U16 => vec![u16::try_from(int(text)?).ok()?],
            Self::I16 => vec![i16::try_from(int(text)?).ok()?.cast_unsigned()],
            Self::U32 => two(u32::try_from(int(text)?).ok()?),
            Self::I32 => two(i32::try_from(int(text)?).ok()?.cast_unsigned()),
            Self::F32 => two(text.parse::<f32>().ok()?.to_bits()),
        })
    }
}

impl fmt::Display for WatchKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug)]
struct Watch {
    vp: u16,
    kind: WatchKind,
    edit: String,
}

/// VPs to watch, each shown as a [`WatchKind`] and written by typing a new
/// value and Enter.
#[derive(Debug)]
pub(crate) struct Watches {
    watches: Vec<Watch>,
    new_watch: String,
    new_kind: WatchKind,
}

impl Default for Watches {
    fn default() -> Self {
        Self {
            watches: Vec::new(),
            new_watch: String::new(),
            new_kind: WatchKind::Hex,
        }
    }
}

impl Watches {
    pub fn new() -> Self {
        Self::default()
    }

    /// Watches `vp` as `kind`, after those watched.
    pub fn add(&mut self, vp: u16, kind: WatchKind) {
        self.watches.push(Watch {
            vp,
            kind,
            edit: String::new(),
        });
    }

    /// The VPs watched, in order.
    pub fn iter(&self) -> impl Iterator<Item = (u16, WatchKind)> + '_ {
        self.watches.iter().map(|watch| (watch.vp, watch.kind))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, sim: &SimThread, log: &mut Log) {
        ui.ctx().request_repaint_after(REFRESH);
        ui.heading("Watches");
        ui.horizontal_wrapped(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.new_watch)
                    .hint_text("0x1000")
                    .desired_width(70.0),
            );
            // Buttons without focus, so that Space and Enter don't press
            // them.
            for kind in WatchKind::ALL {
                let button = Button::selectable(self.new_kind == kind, kind.name()).small();
                if ui.add(button.sense(Sense::CLICK)).clicked() {
                    self.new_kind = kind;
                }
            }
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let add = ui.add(Button::new("watch").sense(Sense::CLICK)).clicked();
            if (add || entered)
                && let Some(vp) = int(self.new_watch.trim()).and_then(|v| u16::try_from(v).ok())
            {
                self.add(vp, self.new_kind);
                self.new_watch.clear();
            }
        });

        let values: Vec<String> = {
            let sim = sim.lock();
            let show = |watch: &Watch| watch.kind.show(sim.vp(), watch.vp);
            self.watches.iter().map(show).collect()
        };
        let mut remove = None;
        let mut writes = Vec::new();
        egui::Grid::new("watches").striped(true).show(ui, |ui| {
            for (i, (watch, value)) in self.watches.iter_mut().zip(values).enumerate() {
                ui.monospace(format!("{:#06X}", watch.vp));
                ui.label(watch.kind.name());
                let response =
                    ui.add(egui::TextEdit::singleline(&mut watch.edit).desired_width(90.0));
                if response.lost_focus()
                    && ui.input(|i| i.key_pressed(Key::Enter))
                    && let Some(words) = watch.kind.parse(&watch.edit)
                {
                    writes.push((watch.vp, words));
                }
                if !response.has_focus() {
                    watch.edit = value;
                }
                let x = Button::new("x").small().sense(Sense::CLICK);
                if ui.add(x).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.watches.remove(i);
        }
        for (vp, words) in writes {
            let text: Vec<String> = words.iter().map(|w| format!("{w:04X}")).collect();
            log.push(
                LogKind::Host,
                format!("write {vp:#06X} {} (watch)", text.join(" ")),
            );
            if let Err(error) = sim.update(|sim| sim.write_vp(vp, &words)) {
                log.push(LogKind::Error, error.to_string());
            }
        }
    }
}

/// The VP memory: 64 rows of 8 words from a VP typed in.
#[derive(Debug)]
pub(crate) struct Memory {
    start: String,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    /// Starts at `0x1000`, the first user VP.
    pub fn new() -> Self {
        Self {
            start: "0x1000".to_owned(),
        }
    }

    /// What was typed as the first VP.
    pub fn start(&self) -> &str {
        &self.start
    }

    pub fn set_start(&mut self, start: impl Into<String>) {
        self.start = start.into();
    }

    /// Shows the rows, in the space left.
    pub fn show(&mut self, ui: &mut egui::Ui, sim: &SimThread) {
        const ROWS: u16 = 64;
        ui.ctx().request_repaint_after(REFRESH);
        ui.horizontal(|ui| {
            ui.heading("Memory");
            ui.add(egui::TextEdit::singleline(&mut self.start).desired_width(70.0));
        });
        let start = int(self.start.trim())
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(0x1000)
            & !7;
        let words: Vec<u16> = {
            let sim = sim.lock();
            (0..ROWS * 8)
                .map(|i| sim.vp().get(start.wrapping_add(i)))
                .collect()
        };
        ui.scope(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            egui::ScrollArea::both()
                .id_salt("memory")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (row, words) in (0..ROWS).zip(words.chunks(8)) {
                        let at = start.wrapping_add(row * 8);
                        let words: Vec<String> = words.iter().map(|w| format!("{w:04X}")).collect();
                        ui.monospace(format!("{at:04X}  {}", words.join(" ")));
                    }
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_watch_values() {
        assert_eq!(WatchKind::I16.parse("-2"), Some(vec![0xFFFE]));
        assert_eq!(WatchKind::U32.parse("0x10002"), Some(vec![0x0001, 0x0002]));
        assert_eq!(WatchKind::F32.parse("1"), Some(vec![0x3F80, 0x0000]));
        assert_eq!(WatchKind::Hex.parse("0x10000"), None);
    }

    #[test]
    fn names_kinds() {
        for kind in WatchKind::ALL {
            assert_eq!(WatchKind::from_name(&kind.to_string()), Some(kind));
        }
        assert_eq!(WatchKind::from_name("u8"), None);
    }
}
