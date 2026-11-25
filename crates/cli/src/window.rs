//! The window of `degus sim`: the screen, which takes clicks as touches,
//! the page list, a command box, VP watches and memory, and a log of what
//! passes between host and display. With the `audio` feature, on by
//! default, it plays the display's clips and beeps.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use degus_codec::{Accumulator, Address, Crc, Feed, MAX_CONTENT, MAX_FRAME, Request, Response};
use degus_set::controls::Point;
use degus_sim::image::RgbImage;
use degus_sim::system::SYSTEM_RESET;
use degus_sim::{Event, Sim};
use eframe::egui::{
    self, Color32, ColorImage, Key, Pos2, Rect, RichText, Sense, TextureHandle, TextureOptions,
    pos2, vec2,
};

use crate::SimArgs;
use crate::link::{Link, LinkEvent, Waker};
use crate::load::Source;
use crate::speaker::Speaker;
use crate::watch::Watcher;

/// Lines the log keeps.
const LOG_LINES: usize = 5000;
/// How often the window ticks while a touch or an input is active, and
/// otherwise.
const ACTIVE_TICK: Duration = Duration::from_millis(50);
const IDLE_TICK: Duration = Duration::from_millis(250);
/// Master volume at first start.
const DEFAULT_VOLUME: f32 = 0.8;

/// Reads the set, opens the links and runs the window until it closes.
pub(crate) fn run(args: SimArgs) -> Result<(), String> {
    let source = Source::new(&args);
    let mut sim = Sim::new(source.read()?);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    sim.set_rtc(now);

    let ctx: Arc<OnceLock<egui::Context>> = Arc::default();
    let wake_ctx = ctx.clone();
    let wake: Waker = Arc::new(move || {
        if let Some(ctx) = wake_ctx.get() {
            ctx.request_repaint();
        }
    });
    let mut links = Vec::new();
    if let Some(addr) = &args.tcp {
        let link = Link::tcp(addr, wake.clone()).map_err(|e| format!("--tcp {addr}: {e}"))?;
        links.push(link);
    }
    #[cfg(feature = "serial")]
    if let Some(port) = &args.serial {
        let baud = args.baud.unwrap_or(sim.config().baud);
        let link =
            Link::serial(port, baud, wake.clone()).map_err(|e| format!("--serial {port}: {e}"))?;
        links.push(link);
    }
    let watcher = Watcher::new(source.folder(), wake)
        .map_err(|e| format!("{}: {e}", source.folder().display()))?;

    let title = format!("{} - degus sim", source.root.display());
    let screen = sim.screen();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1280.0, 820.0]),
        ..Default::default()
    };
    eframe::run_native(
        "degus-sim",
        options,
        Box::new(move |cc| {
            let _ = ctx.set(cc.egui_ctx.clone());
            Ok(Box::new(App::new(cc, sim, source, watcher, links)))
        }),
    )
    .map_err(|e| format!("the window failed ({screen} screen): {e}"))
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
    Display,
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
        let two = |v: u32| vec![(v >> 16) as u16, v as u16];
        let text = text.trim();
        Some(match self {
            Self::Hex | Self::U16 => vec![u16::try_from(int(text)?).ok()?],
            Self::I16 => vec![i16::try_from(int(text)?).ok()? as u16],
            Self::U32 => two(u32::try_from(int(text)?).ok()?),
            Self::I32 => two(i32::try_from(int(text)?).ok()? as u32),
            Self::F32 => two(text.parse::<f32>().ok()?.to_bits()),
        })
    }
}

/// `0x1000` or `4096`.
fn int(text: &str) -> Option<i64> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

struct Watch {
    vp: u16,
    kind: WatchKind,
    edit: String,
}

struct App {
    sim: Sim,
    source: Source,
    watcher: Watcher,
    links: Vec<Link>,
    start: Instant,
    last_tick: Instant,
    /// Frames from the host, for the log; the simulator reads its own copy.
    host_frames: Accumulator<[u8; MAX_CONTENT]>,
    /// Pages to pick from, with what is on them.
    pages: Vec<(u16, String)>,
    fb: RgbImage,
    texture: Option<TextureHandle>,
    /// The generation the texture shows.
    shown: Option<u64>,
    zoom: Zoom,
    log: VecDeque<LogLine>,
    watches: Vec<Watch>,
    new_watch: String,
    new_kind: WatchKind,
    memory_start: String,
    command: String,
    /// The pointer is down on the screen.
    touching: bool,
    speaker: Speaker,
    sound: bool,
    /// Master volume, 0-1.
    volume: f32,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        sim: Sim,
        source: Source,
        watcher: Watcher,
        links: Vec<Link>,
    ) -> Self {
        let mut app = Self {
            host_frames: Accumulator::new([0; MAX_CONTENT], crc(&sim)),
            pages: pages(&sim),
            sim,
            source,
            watcher,
            links,
            start: Instant::now(),
            last_tick: Instant::now(),
            fb: RgbImage::new(1, 1),
            texture: None,
            shown: None,
            zoom: Zoom::Fit,
            log: VecDeque::new(),
            watches: Vec::new(),
            new_watch: String::new(),
            new_kind: WatchKind::Hex,
            memory_start: "0x1000".to_owned(),
            command: String::new(),
            touching: false,
            speaker: Speaker::new(DEFAULT_VOLUME),
            sound: true,
            volume: DEFAULT_VOLUME,
        };
        if let Some(storage) = cc.storage {
            app.restore(storage);
        }
        app.speaker.set_volume(app.master_volume());
        let screen = app.sim.screen();
        let folder = app.source.folder();
        app.push(
            LogKind::Event,
            format!("running {} at {screen}", folder.display()),
        );
        let names: Vec<String> = app
            .links
            .iter()
            .map(|l| format!("link: {}", l.name()))
            .collect();
        for name in names {
            app.push(LogKind::Event, name);
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
                        vp: u16::try_from(int(vp)?).ok()?,
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
        if let Some(start) = storage.get_string("memory_start") {
            self.memory_start = start;
        }
        self.sound = storage.get_string("sound").as_deref() != Some("off");
        if let Some(volume) = storage.get_string("volume").and_then(|v| v.parse().ok()) {
            self.volume = f32::clamp(volume, 0.0, 1.0);
        }
    }

    /// The speaker's volume: 0 while the sound is off.
    fn master_volume(&self) -> f32 {
        if self.sound { self.volume } else { 0.0 }
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

    /// Sends and logs what the display sent, and logs its events.
    fn drain(&mut self) {
        let tx = self.sim.take_tx();
        if !tx.is_empty() {
            self.log_display_frames(&tx);
            let mut failures = Vec::new();
            for link in &self.links {
                if let Err(error) = link.send(&tx) {
                    failures.push(format!("{}: {error}", link.name()));
                }
            }
            for failure in failures {
                self.push(LogKind::Error, failure);
            }
        }
        for event in self.sim.take_events() {
            let text = match event {
                Event::Page(page) => format!("page {page}"),
                Event::Reset => "reset".to_owned(),
                Event::Note(note) => note,
                Event::Sound(sound) => {
                    let text = format!("sound: {sound}");
                    self.speaker.play(sound);
                    text
                }
            };
            self.push(LogKind::Event, text);
        }
        for problem in self.speaker.problems() {
            self.push(LogKind::Error, problem);
        }
    }

    /// Logs the frames of `bytes`, which the display sent.
    fn log_display_frames(&mut self, bytes: &[u8]) {
        let crc = crc(&self.sim);
        let mut rest = bytes;
        while !rest.is_empty() {
            match degus_codec::split(rest, crc) {
                Ok((content, after)) => {
                    let text = match Response::from_content(content) {
                        Ok(response) => describe_response(&response),
                        Err(error) => format!("unreadable frame: {error}"),
                    };
                    self.push(LogKind::Display, text);
                    rest = after;
                }
                Err(error) => {
                    self.push(LogKind::Error, format!("unreadable frame: {error}"));
                    break;
                }
            }
        }
    }

    /// Logs the frames among `bytes`, which the host sent.
    fn log_host_bytes(&mut self, bytes: &[u8]) {
        let mut lines = Vec::new();
        let mut input = bytes;
        while !input.is_empty() {
            input = match self.host_frames.feed(input) {
                Feed::Consumed => break,
                Feed::Frame(content, rest) => {
                    if let Ok(request) = Request::from_content(content) {
                        lines.push(describe_request(&request));
                    }
                    rest
                }
                // The simulator reports what it drops.
                Feed::Error(_, rest) => rest,
            };
        }
        for line in lines {
            self.push(LogKind::Host, line);
        }
    }

    /// Handles what the links received.
    fn poll_links(&mut self) {
        let events: Vec<LinkEvent> = self.links.iter().flat_map(Link::poll).collect();
        for event in events {
            match event {
                LinkEvent::Connected(peer) => {
                    self.push(LogKind::Event, format!("host connected: {peer}"));
                }
                LinkEvent::Disconnected(why) => {
                    self.push(LogKind::Event, format!("host disconnected: {why}"));
                }
                LinkEvent::Error(error) => self.push(LogKind::Error, error),
                LinkEvent::Bytes(bytes) => {
                    self.log_host_bytes(&bytes);
                    self.sim.feed(&bytes);
                }
            }
            self.drain();
        }
    }

    /// Reloads the set, keeping the page and the VPs.
    fn reload(&mut self) {
        match self.source.read() {
            Ok(set) => {
                self.sim.reload(set);
                self.host_frames = Accumulator::new([0; MAX_CONTENT], crc(&self.sim));
                self.pages = pages(&self.sim);
                self.touching = false;
                self.push(LogKind::Event, "reloaded");
            }
            Err(error) => self.push(LogKind::Error, format!("not reloaded: {error}")),
        }
    }

    /// Runs a line typed into the command box. Writes and reads go to the
    /// display as frames, but their answers are only logged, not sent to
    /// the host.
    fn run_command(&mut self) {
        let line = std::mem::take(&mut self.command);
        let words: Vec<&str> = line.split_whitespace().collect();
        let numbers: Option<Vec<i64>> = words.iter().skip(1).map(|w| int(w)).collect();
        let result = match (words.first().copied(), numbers.as_deref()) {
            (None, _) => return,
            (Some("write"), Some([vp, values @ ..])) if !values.is_empty() => {
                let data: Option<Vec<u8>> = values
                    .iter()
                    .map(|&v| u16::try_from(v).ok().map(u16::to_be_bytes))
                    .collect::<Option<Vec<_>>>()
                    .map(|words| words.concat());
                match (u16::try_from(*vp), data) {
                    (Ok(vp), Some(data)) => self.send_frame(Request::Write {
                        addr: Address::Vp(vp),
                        data: &data,
                    }),
                    _ => Err("write takes a VP and 16-bit words".to_owned()),
                }
            }
            (Some("read"), Some([vp, rest @ ..])) if rest.len() <= 1 => {
                let len = rest.first().copied().unwrap_or(1);
                match (u16::try_from(*vp), u8::try_from(len)) {
                    (Ok(vp), Ok(len)) => self.send_frame(Request::Read {
                        addr: Address::Vp(vp),
                        len,
                    }),
                    _ => Err("read takes a VP and a word count".to_owned()),
                }
            }
            (Some("page"), Some([page])) => match u16::try_from(*page) {
                Ok(page) => {
                    self.sim.show_page(page);
                    Ok(())
                }
                Err(_) => Err("no such page".to_owned()),
            },
            (Some("touch"), Some([x, y])) => match (u16::try_from(*x), u16::try_from(*y)) {
                (Ok(x), Ok(y)) => {
                    self.sim.press(Point::new(x, y));
                    self.drain();
                    self.sim.release();
                    Ok(())
                }
                _ => Err("touch takes x and y".to_owned()),
            },
            _ => Err(format!(
                "{line:?}: expected write VP WORD..., read VP [COUNT], page N or touch X Y"
            )),
        };
        if let Err(error) = result {
            self.command = line;
            self.push(LogKind::Error, error);
        }
    }

    /// Feeds `request` to the display as the host would, logging the answer
    /// without sending it to the host.
    fn send_frame(&mut self, request: Request<'_>) -> Result<(), String> {
        self.drain();
        let mut buf = [0u8; MAX_FRAME];
        let frame = request
            .encode(&mut buf, crc(&self.sim))
            .map_err(|e| e.to_string())?;
        self.push(
            LogKind::Host,
            format!("{} (command box)", describe_request(&request)),
        );
        self.sim.feed(frame);
        let answer = self.sim.take_tx();
        self.log_display_frames(&answer);
        Ok(())
    }

    /// Saves the screen next to the `DWIN_SET` folder, in `sim`.
    fn save_png(&mut self) {
        let dir = self.source.root.join("sim");
        let path = dir.join(format!("page-{}.png", self.sim.page()));
        let saved = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| self.sim.render().save(&path).map_err(|e| e.to_string()));
        match saved {
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
            for scale in [0.5, 1.0, 2.0] {
                ui.selectable_value(&mut self.zoom, Zoom::Scale(scale), format!("{scale}:1"));
            }
            ui.separator();
            let sound = ui.checkbox(&mut self.sound, "sound");
            let volume = ui.add_enabled(
                self.sound,
                egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false),
            );
            if sound.changed() || volume.changed() {
                self.speaker.set_volume(self.master_volume());
            }
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
                .button("reload")
                .on_hover_text("Reads the DWIN_SET folder again, keeping the page and the VPs")
                .clicked()
            {
                self.reload();
            }
            if ui
                .button("save PNG")
                .on_hover_text("Saves the screen in the sim folder next to DWIN_SET")
                .clicked()
            {
                self.save_png();
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
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let mut show = None;
        egui::ScrollArea::vertical()
            .id_salt("pages")
            .show(ui, |ui| {
                for (id, label) in &self.pages {
                    let current = *id == self.sim.page();
                    let text = format!("{id:>5}  {label}");
                    if ui
                        .selectable_label(current, RichText::new(text).monospace())
                        .clicked()
                    {
                        show = Some(*id);
                    }
                }
            });
        if let Some(page) = show {
            self.sim.show_page(page);
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Command");
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut self.command)
                    .hint_text("write 0x1000 1")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text(
                "As the host, answers only logged: write 0x1000 1 2, read 0x1000 2\n\
                 As a finger: touch x y\n\
                 page n",
            );
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.run_command();
            response.request_focus();
        }

        ui.separator();
        ui.heading("Watches");
        ui.horizontal(|ui| {
            let field = ui.add(
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
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.button("watch").clicked() || entered)
                && let Some(vp) = int(self.new_watch.trim()).and_then(|v| u16::try_from(v).ok())
            {
                self.watches.push(Watch {
                    vp,
                    kind: self.new_kind,
                    edit: String::new(),
                });
                self.new_watch.clear();
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
            let text: Vec<String> = words.iter().map(|w| format!("{w:04X}")).collect();
            self.push(
                LogKind::Host,
                format!("write {vp:#06X} {} (watch)", text.join(" ")),
            );
            if let Err(error) = self.sim.write_vp(vp, &words) {
                self.push(LogKind::Error, error.to_string());
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Memory");
            ui.add(egui::TextEdit::singleline(&mut self.memory_start).desired_width(70.0));
        });
        let start = int(self.memory_start.trim())
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(0x1000)
            & !7;
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        egui::ScrollArea::both()
            .id_salt("memory")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in 0..64u16 {
                    let at = start.wrapping_add(row * 8);
                    let words: Vec<String> = (0..8)
                        .map(|i| format!("{:04X}", self.sim.vp().get(at.wrapping_add(i))))
                        .collect();
                    ui.monospace(format!("{at:04X}  {}", words.join(" ")));
                }
            });
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
                        LogKind::Display => ("<", Color32::from_rgb(90, 200, 120)),
                        LogKind::Event => ("#", ui.visuals().weak_text_color()),
                        LogKind::Error => ("!", Color32::from_rgb(230, 90, 90)),
                    };
                    let text = format!("{:>8.3}  {prefix} {}", line.at.as_secs_f32(), line.text);
                    ui.label(RichText::new(text).monospace().color(color));
                }
            });
    }

    fn screen(&mut self, ui: &mut egui::Ui) {
        let screen = self.sim.screen();
        let (w, h) = (f32::from(screen.w), f32::from(screen.h));
        if self.shown != Some(self.sim.generation()) || self.texture.is_none() {
            self.sim.render_into(&mut self.fb);
            let image = ColorImage::from_rgb(
                [self.fb.width() as usize, self.fb.height() as usize],
                self.fb.as_raw(),
            );
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    let texture = ui
                        .ctx()
                        .load_texture("screen", image, TextureOptions::NEAREST);
                    self.texture = Some(texture);
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

            let to_page = |pos: Pos2| -> Point {
                let p = (pos - rect.min) / scale;
                Point::new(
                    p.x.clamp(0.0, w - 1.0) as u16,
                    p.y.clamp(0.0, h - 1.0) as u16,
                )
            };
            if response.is_pointer_button_down_on() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let at = to_page(pos);
                    if self.touching {
                        self.sim.move_to(at);
                    } else {
                        self.sim.press(at);
                        self.touching = true;
                    }
                }
            } else if self.touching {
                self.sim.release();
                self.touching = false;
            }
            if let Some(pos) = response.hover_pos() {
                let at = to_page(pos);
                response.on_hover_text_at_pointer(format!("({}, {})", at.x, at.y));
            }
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        self.sim.advance(now - self.last_tick);
        self.last_tick = now;
        if self.watcher.changed() {
            self.reload();
        }
        self.poll_links();
        self.drain();
        let busy = self.sim.is_pressed() || self.sim.input_text().is_some();
        ctx.request_repaint_after(if busy { ACTIVE_TICK } else { IDLE_TICK });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| self.log_panel(ui));
        egui::Panel::left("pages")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| self.pages(ui));
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(420.0)
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::default().show(ui, |ui| self.screen(ui));
        // Touches and writes made this frame.
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
        storage.set_string("memory_start", self.memory_start.clone());
        storage.set_string("sound", if self.sound { "on" } else { "off" }.to_owned());
        storage.set_string("volume", self.volume.to_string());
    }
}

fn crc(sim: &Sim) -> Option<Crc> {
    if sim.config().crc {
        Crc::on()
    } else {
        Crc::off()
    }
}

/// The pages to pick from: those with controls, and the background
/// images of the whole screen, which may be pages without controls. The
/// background ICL can hold icons as well, so smaller images are left out.
fn pages(sim: &Sim) -> Vec<(u16, String)> {
    let set = sim.set();
    let screen = set.device.screen;
    let mut pages: BTreeMap<u16, String> = set
        .controls
        .iter()
        .map(|(id, controls)| {
            let label = format!(
                "{} touch, {} display",
                controls.touch.len(),
                controls.display.len()
            );
            (id, label)
        })
        .collect();
    if let Some(icl) = set.icls.get(&sim.background_icl()) {
        for (&id, icon) in &icl.icons {
            if (icon.width(), icon.height()) == (screen.w, screen.h) {
                pages
                    .entry(id)
                    .or_insert_with(|| "background only".to_owned());
            }
        }
    }
    pages.into_iter().collect()
}

/// `write 0x1000 0001 0002`, `read 0x1000 2`.
fn describe_request(request: &Request<'_>) -> String {
    match *request {
        Request::Write { addr, data } => {
            format!("write {} {}", describe_address(addr), words(data))
        }
        Request::Read { addr, len } => format!("read {} {len}", describe_address(addr)),
    }
}

/// `ack`, `data 0x1000 0001`.
fn describe_response(response: &Response<'_>) -> String {
    match *response {
        Response::Ack(_) => "ack".to_owned(),
        Response::Data { addr, data, .. } => {
            format!("data {} {}", describe_address(addr), words(data))
        }
    }
}

fn describe_address(addr: Address) -> String {
    match addr {
        Address::Vp(vp) => format!("{vp:#06X}"),
        Address::Register { page, reg } => format!("register {page}:{reg:#04X}"),
        Address::Curve(channels) => format!("curve {channels:#04X}"),
        Address::Dword(at) => format!("dword {at:#010X}"),
    }
}

/// Bytes as big-endian words: `0001 0203`, an odd last byte on its own.
fn words(data: &[u8]) -> String {
    data.chunks(2)
        .map(|pair| pair.iter().map(|b| format!("{b:02X}")).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_frames() {
        let write = Request::Write {
            addr: Address::Vp(0x1000),
            data: &[0x00, 0x01, 0x02],
        };
        assert_eq!(describe_request(&write), "write 0x1000 0001 02");
        let read = Request::Read {
            addr: Address::Vp(0x0014),
            len: 1,
        };
        assert_eq!(describe_request(&read), "read 0x0014 1");
        let data = Response::Data {
            addr: Address::Vp(0x0014),
            len: 1,
            data: &[0x00, 0x03],
        };
        assert_eq!(describe_response(&data), "data 0x0014 0003");
    }

    #[test]
    fn parses_watch_values() {
        assert_eq!(int("0x1000"), Some(0x1000));
        assert_eq!(int("12"), Some(12));
        assert_eq!(WatchKind::I16.parse("-2"), Some(vec![0xFFFE]));
        assert_eq!(WatchKind::U32.parse("0x10002"), Some(vec![0x0001, 0x0002]));
        assert_eq!(WatchKind::Hex.parse("0x10000"), None);
    }
}
