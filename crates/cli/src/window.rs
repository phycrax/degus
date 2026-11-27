//! The window of `degus sim`: the display from `degus-sim-gui`, whose
//! screen takes clicks as touches and which plays its sounds with the
//! `audio` feature, on by default; and the tools to look inside it: the
//! page list, a command box, VP watches and memory, and a log of what
//! passes between host and display.

use std::sync::mpsc::channel;
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use degus_sim::Sim;
use degus_sim::system::SYSTEM_RESET;
use degus_sim_gui::egui::{self, RichText};
use degus_sim_gui::{Screen, SimThread, Speaker, Zoom, eframe};

use crate::SimArgs;
use crate::command::CommandBox;
use crate::link::{Bridge, Notices, Waker};
use crate::load::Source;
use crate::log::{Log, LogKind, codec_crc};
use crate::pages::PageList;
use crate::watch::{Memory, WatchKind, Watches};

/// Master volume at first start.
const DEFAULT_VOLUME: f32 = 0.8;

/// Reads the set, opens the links and runs the window until it closes.
pub(crate) fn run(args: SimArgs) -> Result<(), String> {
    let start = Instant::now();
    let source = Source::new(&args);
    let mut sim = Sim::new(source.read()?);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or_default(|d| d.as_secs());
    sim.set_rtc(now);
    let screen = sim.screen();
    let crc = codec_crc(sim.config().crc);
    #[cfg(feature = "serial")]
    let baud = args.baud.unwrap_or(sim.config().baud);
    // Notes on the page shown at power-on.
    let power_on = sim.take_events();

    let sim = SimThread::new(sim);
    let (notices, from_links) = channel();
    let mut log = Log::new(start, sim.monitor(), from_links, crc);
    let folder = source.folder();
    log.push(
        LogKind::Event,
        format!("running {} at {screen}", folder.display()),
    );
    log.push_events(power_on);

    // The window, once there is one, shows what the links tell the log.
    let ctx: Arc<OnceLock<egui::Context>> = Arc::default();
    let wake: Waker = {
        let ctx = ctx.clone();
        Arc::new(move || {
            if let Some(ctx) = ctx.get() {
                ctx.request_repaint();
            }
        })
    };
    let mut bridge = Bridge::new(sim.uart(), Notices::new(notices, wake));
    let mut links = Vec::new();
    if let Some(addr) = &args.tcp {
        let name = bridge.tcp(addr).map_err(|e| format!("--tcp {addr}: {e}"))?;
        links.push(name.to_owned());
    }
    #[cfg(feature = "serial")]
    if let Some(port) = &args.serial {
        let name = bridge
            .serial(port, baud)
            .map_err(|e| format!("--serial {port}: {e}"))?;
        links.push(name.to_owned());
    }
    for name in &links {
        log.push(LogKind::Event, format!("link: {name}"));
    }
    bridge
        .start()
        .map_err(|e| format!("no thread for the links: {e}"))?;

    let title = format!("{} - degus sim", source.root.display());
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
            Ok(Box::new(App::new(cc, sim, source, log, links)))
        }),
    )
    .map_err(|e| format!("the window failed ({screen} screen): {e}"))
}

struct App {
    sim: SimThread,
    source: Source,
    /// The links' names.
    links: Vec<String>,
    screen: Screen,
    zoom: Zoom,
    pages: PageList,
    command: CommandBox,
    watches: Watches,
    memory: Memory,
    log: Log,
    speaker: Speaker,
    sound: bool,
    /// Master volume, 0-1.
    volume: f32,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        sim: SimThread,
        source: Source,
        log: Log,
        links: Vec<String>,
    ) -> Self {
        let mut app = Self {
            sim,
            source,
            links,
            screen: Screen::new(),
            zoom: Zoom::Fit,
            pages: PageList::new(),
            command: CommandBox::new(),
            watches: Watches::new(),
            memory: Memory::new(),
            log,
            speaker: Speaker::new(DEFAULT_VOLUME),
            sound: true,
            volume: DEFAULT_VOLUME,
        };
        if let Some(storage) = cc.storage {
            app.restore(storage);
        }
        app.speaker.set_volume(app.master_volume());
        app.sim.set_speaker(app.speaker.clone());
        let ctx = cc.egui_ctx.clone();
        app.sim.on_change(move || ctx.request_repaint());
        app
    }

    fn restore(&mut self, storage: &dyn eframe::Storage) {
        if let Some(watches) = storage.get_string("watches") {
            for item in watches.split(',') {
                let watch = item.split_once(':').and_then(|(vp, kind)| {
                    let vp = vp.strip_prefix("0x")?;
                    Some((
                        u16::from_str_radix(vp, 16).ok()?,
                        WatchKind::from_name(kind)?,
                    ))
                });
                if let Some((vp, kind)) = watch {
                    self.watches.add(vp, kind);
                }
            }
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
            self.memory.set_start(start);
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

    /// Reloads the set, keeping the page and the VPs.
    fn reload(&mut self) {
        match self.source.read() {
            Ok(set) => {
                let crc = codec_crc(set.config.as_ref().is_some_and(|config| config.crc));
                self.sim.update(|sim| sim.reload(set));
                self.log.reloaded(crc);
                self.pages.reloaded();
                self.log.push(LogKind::Event, "reloaded");
            }
            Err(error) => self
                .log
                .push(LogKind::Error, format!("not reloaded: {error}")),
        }
    }

    /// Saves the screen next to the `DWIN_SET` folder, in `sim`.
    fn save_png(&mut self) {
        let (page, image) = {
            let sim = self.sim.lock();
            (sim.page(), sim.render())
        };
        let dir = self.source.root.join("sim");
        let path = dir.join(format!("page-{page}.png"));
        let saved = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| image.save(&path).map_err(|e| e.to_string()));
        match saved {
            Ok(()) => self
                .log
                .push(LogKind::Event, format!("saved {}", path.display())),
            Err(error) => self
                .log
                .push(LogKind::Error, format!("{}: {error}", path.display())),
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let (page, brightness) = {
            let sim = self.sim.lock();
            (sim.page(), sim.brightness())
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("page {page}")).strong());
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
            ui.label(format!("backlight {brightness}%"));
            ui.separator();
            if ui
                .button("reset")
                .on_hover_text("Writes 55AA 5AA5 to 0x0004")
                .clicked()
            {
                let _ = self
                    .sim
                    .update(|sim| sim.write_vp(SYSTEM_RESET, &[0x55AA, 0x5AA5]));
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
                ui.label(link);
            }
        });
    }

    fn screen(&mut self, ui: &mut egui::Ui) {
        let shown = match self.zoom {
            Zoom::Fit => self.screen.show(ui, &self.sim, Zoom::Fit),
            zoom => {
                egui::ScrollArea::both()
                    .id_salt("screen")
                    .show(ui, |ui| self.screen.show(ui, &self.sim, zoom))
                    .inner
            }
        };
        if let Some(at) = shown.inner {
            shown
                .response
                .on_hover_text_at_pointer(format!("({}, {})", at.x, at.y));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        for problem in self.speaker.problems() {
            self.log.push(LogKind::Error, problem);
        }
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| self.log.show(ui));
        egui::Panel::left("pages")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| self.pages.show(ui, &self.sim));
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(420.0)
            .show(ui, |ui| {
                self.command.show(ui, &self.sim, &mut self.log);
                ui.separator();
                self.watches.show(ui, &self.sim, &mut self.log);
                ui.separator();
                self.memory.show(ui, &self.sim);
            });
        egui::CentralPanel::default().show(ui, |ui| self.screen(ui));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let watches: Vec<String> = self
            .watches
            .iter()
            .map(|(vp, kind)| format!("{vp:#06X}:{kind}"))
            .collect();
        storage.set_string("watches", watches.join(","));
        let zoom = match self.zoom {
            Zoom::Fit => "fit".to_owned(),
            Zoom::Scale(scale) => scale.to_string(),
        };
        storage.set_string("zoom", zoom);
        storage.set_string("memory_start", self.memory.start().to_owned());
        storage.set_string("sound", if self.sound { "on" } else { "off" }.to_owned());
        storage.set_string("volume", self.volume.to_string());
    }
}
