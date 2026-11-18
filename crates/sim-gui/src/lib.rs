//! The window of the degus display simulator: the screen, which takes
//! clicks as touches, the page list, a VP inspector, a host command box and
//! a log of everything that passes between host and display.
//!
//! ```no_run
//! use degus_sim::{LineCodec, Link, Sim};
//! use degus_sim_gui::Gui;
//!
//! let sim = Sim::load("my-hmi")?;
//! let gui = Gui::new();
//! let link = Link::tcp("127.0.0.1:5000", Box::new(LineCodec::new()), gui.waker())?;
//! gui.run(sim, vec![link], "my-hmi")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use degus_sim::script::{Command, parse_line};
use degus_sim::system::SYSTEM_RESET;
use degus_sim::{DeviceMsg, Event, Link, LinkEvent, Sim, Waker};
use eframe::egui::{
    self, Color32, ColorImage, Key, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Vec2, pos2, vec2,
};
use image::RgbImage;

/// Lines the log keeps.
const LOG_LINES: usize = 5000;
/// How often the simulator ticks while a touch or an input is active.
const ACTIVE_TICK: Duration = Duration::from_millis(50);

/// Starts the window. Create it first, so links can wake the window with
/// [`Gui::waker`].
#[derive(Default)]
pub struct Gui {
    ctx: Arc<OnceLock<egui::Context>>,
}

impl Gui {
    pub fn new() -> Self {
        Self::default()
    }

    /// Repaints the window, once it is open; for [`Link`]s to call when
    /// something arrives.
    pub fn waker(&self) -> Waker {
        let ctx = self.ctx.clone();
        Arc::new(move || {
            if let Some(ctx) = ctx.get() {
                ctx.request_repaint();
            }
        })
    }

    /// Opens the window and runs until it closes.
    pub fn run(self, sim: Sim, links: Vec<Link>, title: &str) -> eframe::Result {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(format!("{title} - degus sim"))
                .with_inner_size([1280.0, 820.0]),
            ..Default::default()
        };
        eframe::run_native(
            "degus-sim",
            options,
            Box::new(move |cc| {
                let _ = self.ctx.set(cc.egui_ctx.clone());
                Ok(Box::new(App::new(cc, sim, links)))
            }),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Zoom {
    Fit,
    Scale(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogKind {
    /// From the host.
    Host,
    /// From the display.
    Device,
    /// Something that happened.
    Event,
    Error,
}

struct LogLine {
    at: Duration,
    kind: LogKind,
    text: String,
}

/// How a watched VP is shown and edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchKind {
    Hex,
    U16,
    I16,
    U32,
    I32,
    F32,
}

impl WatchKind {
    const ALL: [Self; 6] = [
        Self::Hex,
        Self::U16,
        Self::I16,
        Self::U32,
        Self::I32,
        Self::F32,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::U16 => "u16",
            Self::I16 => "i16",
            Self::U32 => "u32",
            Self::I32 => "i32",
            Self::F32 => "f32",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    fn show(self, sim: &Sim, vp: u16) -> String {
        let v = sim.vp();
        match self {
            Self::Hex => format!("{:#06X}", v.get(vp)),
            Self::U16 => v.get(vp).to_string(),
            Self::I16 => (v.get(vp) as i16).to_string(),
            Self::U32 => v.u32(vp).to_string(),
            Self::I32 => (v.u32(vp) as i32).to_string(),
            Self::F32 => f32::from_bits(v.u32(vp)).to_string(),
        }
    }

    /// The words that `text` stands for.
    fn parse(self, text: &str) -> Option<Vec<u16>> {
        let text = text.trim();
        let hex = |t: &str| {
            t.strip_prefix("0x")
                .or_else(|| t.strip_prefix("0X"))
                .map(str::to_owned)
        };
        let int = |t: &str| -> Option<i64> {
            match hex(t) {
                Some(h) => i64::from_str_radix(&h, 16).ok(),
                None => t.parse().ok(),
            }
        };
        let two = |v: u32| vec![(v >> 16) as u16, v as u16];
        Some(match self {
            Self::Hex | Self::U16 => vec![u16::try_from(int(text)?).ok()?],
            Self::I16 => vec![i16::try_from(int(text)?).ok()? as u16],
            Self::U32 => two(u32::try_from(int(text)?).ok()?),
            Self::I32 => two(i32::try_from(int(text)?).ok()? as u32),
            Self::F32 => two(text.parse::<f32>().ok()?.to_bits()),
        })
    }
}

struct Watch {
    vp: u16,
    kind: WatchKind,
    edit: String,
}

struct App {
    sim: Sim,
    links: Vec<Link>,
    start: Instant,
    fb: RgbImage,
    texture: Option<TextureHandle>,
    /// The generation the texture shows.
    shown: Option<u64>,
    zoom: Zoom,
    overlay: bool,
    log: VecDeque<LogLine>,
    watches: Vec<Watch>,
    new_watch: String,
    new_kind: WatchKind,
    hex_start: String,
    command: String,
    /// The pointer is down on the screen.
    touching: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, sim: Sim, links: Vec<Link>) -> Self {
        let mut app = Self {
            sim,
            links,
            start: Instant::now(),
            fb: RgbImage::new(1, 1),
            texture: None,
            shown: None,
            zoom: Zoom::Fit,
            overlay: false,
            log: VecDeque::new(),
            watches: Vec::new(),
            new_watch: String::new(),
            new_kind: WatchKind::Hex,
            hex_start: "0x1000".to_owned(),
            command: String::new(),
            touching: false,
        };
        if let Some(storage) = cc.storage {
            app.restore(storage);
        }
        let names: Vec<String> = app
            .links
            .iter()
            .map(|l| format!("link: {}", l.name()))
            .collect();
        for name in names {
            app.push(LogKind::Event, name);
        }
        for warning in app.sim.world().warnings.clone() {
            app.push(LogKind::Error, warning.to_string());
        }
        app.drain();
        app
    }

    fn restore(&mut self, storage: &dyn eframe::Storage) {
        if let Some(watches) = storage.get_string("watches") {
            self.watches = watches
                .split(',')
                .filter_map(|item| {
                    let (vp, kind) = item.split_once(':')?;
                    Some(Watch {
                        vp: u16::from_str_radix(vp.trim_start_matches("0x"), 16).ok()?,
                        kind: WatchKind::from_name(kind)?,
                        edit: String::new(),
                    })
                })
                .collect();
        }
        match storage.get_string("zoom").as_deref() {
            Some("fit") | None => self.zoom = Zoom::Fit,
            Some(scale) => {
                if let Ok(scale) = scale.parse() {
                    self.zoom = Zoom::Scale(scale);
                }
            }
        }
        self.overlay = storage.get_string("overlay").as_deref() == Some("on");
        if let Some(start) = storage.get_string("hex_start") {
            self.hex_start = start;
        }
    }

    fn push(&mut self, kind: LogKind, text: impl Into<String>) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(LogLine {
            at: self.start.elapsed(),
            kind,
            text: text.into(),
        });
    }

    /// Sends and logs what the display sent unasked, and logs its events.
    fn drain(&mut self) {
        for msg in self.sim.take_outbox() {
            self.send(&msg);
        }
        for event in self.sim.take_events() {
            let text = match event {
                Event::Page(page) => format!("page {page}"),
                Event::Reset => "reset".to_owned(),
                Event::Note(note) => note,
            };
            self.push(LogKind::Event, text);
        }
    }

    fn send(&mut self, msg: &DeviceMsg) {
        self.push(LogKind::Device, msg.to_string());
        let mut failures = Vec::new();
        for link in &self.links {
            if let Err(error) = link.send(msg) {
                failures.push(format!("{}: {error}", link.name()));
            }
        }
        for failure in failures {
            self.push(LogKind::Error, failure);
        }
    }

    /// Handles what the links received.
    fn poll_links(&mut self) {
        let events: Vec<LinkEvent> = self.links.iter().flat_map(Link::poll).collect();
        for event in events {
            match event {
                LinkEvent::Connected(peer) => {
                    self.push(LogKind::Event, format!("host connected: {peer}"))
                }
                LinkEvent::Disconnected(why) => {
                    self.push(LogKind::Event, format!("host disconnected: {why}"));
                }
                LinkEvent::Error(error) => self.push(LogKind::Error, error),
                LinkEvent::Cmd(cmd) => {
                    self.push(LogKind::Host, cmd.to_string());
                    if let Some(reply) = self.sim.handle(&cmd) {
                        self.send(&reply);
                    }
                }
            }
        }
    }

    /// Runs a line typed into the command box.
    fn run_command(&mut self) {
        let line = std::mem::take(&mut self.command);
        let command = match parse_line(&line) {
            Ok(Some(command)) => command,
            Ok(None) => return,
            Err(error) => {
                self.command = line;
                return self.push(LogKind::Error, error);
            }
        };
        if let Some(cmd) = command.host_cmd() {
            self.push(LogKind::Host, cmd.to_string());
            if let Some(reply) = self.sim.handle(&cmd) {
                self.send(&reply);
            }
            return;
        }
        match command {
            Command::Touch { x, y } => {
                self.sim.touch_down(x, y);
                self.drain();
                self.sim.touch_up();
            }
            Command::Press { x, y } => self.sim.touch_down(x, y),
            Command::Move { x, y } => self.sim.touch_move(x, y),
            Command::Release => self.sim.touch_up(),
            Command::Page(page) => self.sim.show_page(page),
            Command::Shot(path) => self.save_png(path),
            _ => self.push(LogKind::Error, "wait and expect only work in scripts"),
        }
    }

    /// Saves the screen; relative paths are in the project folder.
    fn save_png(&mut self, path: PathBuf) {
        let path = match &self.sim.world().root {
            Some(root) => root.join(path),
            None => path,
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match self.sim.render().save(&path) {
            Ok(()) => self.push(LogKind::Event, format!("saved {}", path.display())),
            Err(error) => self.push(LogKind::Error, format!("{}: {error}", path.display())),
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("page {}", self.sim.page())).strong());
            ui.separator();
            ui.label("zoom");
            ui.selectable_value(&mut self.zoom, Zoom::Fit, "fit");
            for scale in [1.0, 2.0] {
                ui.selectable_value(&mut self.zoom, Zoom::Scale(scale), format!("{scale}:1"));
            }
            ui.separator();
            ui.checkbox(&mut self.overlay, "outlines");
            ui.separator();
            ui.label(format!("backlight {}%", self.sim.brightness()));
            ui.separator();
            if ui
                .button("reset")
                .on_hover_text("Writes 55AA 5AA5 to 0x0004")
                .clicked()
            {
                let _ = self.sim.write_vp(SYSTEM_RESET, &[0x55AA, 0x5AA5]);
            }
            if ui
                .button("save PNG")
                .on_hover_text("Saves the screen in the project's target/sim folder")
                .clicked()
            {
                let path = PathBuf::from(format!("target/sim/page-{}.png", self.sim.page()));
                self.save_png(path);
            }
            ui.separator();
            if self.links.is_empty() {
                ui.label("no host link");
            }
            for link in &self.links {
                ui.label(link.name());
            }
        });
    }

    fn pages(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pages");
        let pages = self.sim.world().pages.clone();
        egui::ScrollArea::vertical()
            .id_salt("pages")
            .show(ui, |ui| {
                for page in pages {
                    let current = page.id == self.sim.page();
                    let label = format!("{:>4}  {}", page.id, page.label);
                    if ui
                        .selectable_label(current, RichText::new(label).monospace())
                        .clicked()
                    {
                        self.sim.show_page(page.id);
                    }
                }
            });
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("VPs");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_watch)
                    .hint_text("0x1000")
                    .desired_width(70.0),
            );
            egui::ComboBox::from_id_salt("watch kind")
                .selected_text(self.new_kind.name())
                .width(55.0)
                .show_ui(ui, |ui| {
                    for kind in WatchKind::ALL {
                        ui.selectable_value(&mut self.new_kind, kind, kind.name());
                    }
                });
            let entered = ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button("watch").clicked() || entered && !self.new_watch.is_empty() {
                let text = self.new_watch.trim();
                let vp = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                    Some(hex) => u16::from_str_radix(hex, 16).ok(),
                    None => text.parse().ok(),
                };
                if let Some(vp) = vp {
                    self.watches.push(Watch {
                        vp,
                        kind: self.new_kind,
                        edit: String::new(),
                    });
                    self.new_watch.clear();
                }
            }
        });

        let mut remove = None;
        let mut writes = Vec::new();
        egui::Grid::new("watches").striped(true).show(ui, |ui| {
            for (i, watch) in self.watches.iter_mut().enumerate() {
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
                    watch.edit = watch.kind.show(&self.sim, watch.vp);
                }
                if ui.small_button("x").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.watches.remove(i);
        }
        for (vp, words) in writes {
            let words_text: Vec<String> = words.iter().map(|w| format!("{w:#06X}")).collect();
            self.push(
                LogKind::Host,
                format!("write {vp:#06X} {} (inspector)", words_text.join(" ")),
            );
            if let Err(error) = self.sim.write_vp(vp, &words) {
                self.push(LogKind::Error, error.to_string());
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("memory from");
            ui.add(egui::TextEdit::singleline(&mut self.hex_start).desired_width(70.0));
        });
        let start = self.hex_start.trim();
        let start = start
            .strip_prefix("0x")
            .or_else(|| start.strip_prefix("0X"))
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
            .unwrap_or(0x1000);
        for row in 0..16u16 {
            let at = start.wrapping_add(row * 8);
            let words: Vec<String> = (0..8)
                .map(|i| format!("{:04X}", self.sim.vp().get(at.wrapping_add(i))))
                .collect();
            ui.monospace(format!("{at:04X}  {}", words.join(" ")));
        }

        ui.separator();
        ui.label("command")
            .on_hover_text("write 0x1000 1, read 0x1000 2, touch x y, page n, shot file.png");
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.command)
                .hint_text("write 0x1000 1")
                .desired_width(f32::INFINITY),
        );
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.run_command();
            response.request_focus();
        }
    }

    fn log_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Log");
            if ui.small_button("clear").clicked() {
                self.log.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("log")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &self.log {
                    let (prefix, color) = match line.kind {
                        LogKind::Host => (">", Color32::from_rgb(90, 160, 255)),
                        LogKind::Device => ("<", Color32::from_rgb(90, 200, 120)),
                        LogKind::Event => ("#", ui.visuals().weak_text_color()),
                        LogKind::Error => ("!", Color32::from_rgb(230, 90, 90)),
                    };
                    let text = format!("{:>8.3}  {prefix} {}", line.at.as_secs_f32(), line.text);
                    ui.label(RichText::new(text).monospace().color(color));
                }
            });
    }

    fn screen(&mut self, ui: &mut egui::Ui) {
        let (w, h) = (
            self.sim.world().screen.w as f32,
            self.sim.world().screen.h as f32,
        );
        if self.shown != Some(self.sim.generation()) || self.texture.is_none() {
            self.sim.render_into(&mut self.fb);
            let image = ColorImage::from_rgb(
                [self.fb.width() as usize, self.fb.height() as usize],
                self.fb.as_raw(),
            );
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    self.texture = Some(ui.ctx().load_texture(
                        "screen",
                        image,
                        TextureOptions::NEAREST,
                    ));
                }
            }
            self.shown = Some(self.sim.generation());
        }
        let Some(texture) = self.texture.clone() else {
            return;
        };

        let scale = match self.zoom {
            Zoom::Fit => {
                let available = ui.available_size();
                (available.x / w).min(available.y / h).max(0.05)
            }
            Zoom::Scale(scale) => scale,
        };
        egui::ScrollArea::both().id_salt("screen").show(ui, |ui| {
            let (rect, response) =
                ui.allocate_exact_size(vec2(w * scale, h * scale), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let dim = 255 - u32::from(self.sim.brightness()) * 255 / 100;
            if dim > 0 {
                painter.rect_filled(rect, 0.0, Color32::from_black_alpha(dim as u8));
            }

            let to_panel = |pos: Pos2| -> (u16, u16) {
                let p = (pos - rect.min) / scale;
                (
                    p.x.clamp(0.0, w - 1.0) as u16,
                    p.y.clamp(0.0, h - 1.0) as u16,
                )
            };
            let to_screen = |r: degus_sim::Rect| {
                Rect::from_min_size(
                    rect.min + Vec2::new(r.x as f32, r.y as f32) * scale,
                    Vec2::new(r.w as f32, r.h as f32) * scale,
                )
            };

            if response.is_pointer_button_down_on() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = to_panel(pos);
                    if self.touching {
                        self.sim.touch_move(x, y);
                    } else {
                        self.sim.touch_down(x, y);
                        self.touching = true;
                    }
                }
            } else if self.touching {
                self.sim.touch_up();
                self.touching = false;
            }

            if self.overlay {
                let items = self.sim.overlay();
                for item in &items {
                    let color = if item.touch {
                        Color32::from_rgb(255, 160, 0)
                    } else {
                        Color32::from_rgb(0, 200, 255)
                    };
                    painter.rect_stroke(
                        to_screen(item.rect),
                        0.0,
                        Stroke::new(1.0, color),
                        StrokeKind::Inside,
                    );
                }
                if let Some(pos) = response.hover_pos() {
                    let (x, y) = to_panel(pos);
                    let under: Vec<&str> = items
                        .iter()
                        .filter(|item| item.rect.contains(x.into(), y.into()))
                        .map(|item| item.label.as_str())
                        .collect();
                    let text = format!("({x}, {y})\n{}", under.join("\n"));
                    response.on_hover_text_at_pointer(text);
                }
            } else if let Some(pos) = response.hover_pos() {
                let (x, y) = to_panel(pos);
                response.on_hover_text_at_pointer(format!("({x}, {y})"));
            }
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sim.tick(self.start.elapsed());
        self.poll_links();
        self.drain();
        if self.sim.is_pressed() || self.sim.input_text().is_some() {
            ctx.request_repaint_after(ACTIVE_TICK);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| self.log_panel(ui));
        egui::Panel::left("pages")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| self.pages(ui));
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(330.0)
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::default().show(ui, |ui| self.screen(ui));
        // Touches and inspector writes made this frame.
        self.drain();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let watches: Vec<String> = self
            .watches
            .iter()
            .map(|w| format!("{:#06X}:{}", w.vp, w.kind.name()))
            .collect();
        storage.set_string("watches", watches.join(","));
        let zoom = match self.zoom {
            Zoom::Fit => "fit".to_owned(),
            Zoom::Scale(scale) => scale.to_string(),
        };
        storage.set_string("zoom", zoom);
        storage.set_string(
            "overlay",
            if self.overlay { "on" } else { "off" }.to_owned(),
        );
        storage.set_string("hex_start", self.hex_start.clone());
    }
}
