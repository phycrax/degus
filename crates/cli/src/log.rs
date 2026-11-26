//! The log: the frames between host and display, what happens in the
//! display, and what the links and the window do.

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use degus_codec::{Accumulator, Address, Crc, Feed, MAX_CONTENT, Request, Response};
use degus_sim::Event;
use degus_sim_gui::egui::{self, Button, Color32, RichText, Sense};
use degus_sim_gui::{Activity, ActivityKind, Monitor};

/// Lines the log keeps.
const LOG_LINES: usize = 5000;

/// What a [`LogLine`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogKind {
    /// From the host.
    Host,
    /// From the display.
    Display,
    /// Something that happened.
    Event,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogLine {
    pub at: Instant,
    pub kind: LogKind,
    pub text: String,
}

impl LogLine {
    pub fn now(kind: LogKind, text: impl Into<String>) -> Self {
        Self {
            at: Instant::now(),
            kind,
            text: text.into(),
        }
    }

    /// `   1.250  > write 0x1000 0001`, timed from `start`.
    fn show(&self, start: Instant) -> String {
        let prefix = match self.kind {
            LogKind::Host => ">",
            LogKind::Display => "<",
            LogKind::Event => "#",
            LogKind::Error => "!",
        };
        let at = self.at.saturating_duration_since(start);
        format!("{:>8.3}  {prefix} {}", at.as_secs_f32(), self.text)
    }
}

/// The log, from what the display's monitor sees and what the links and
/// the window tell it.
pub(crate) struct Log {
    start: Instant,
    lines: VecDeque<LogLine>,
    monitor: Monitor,
    /// From the links.
    notices: Receiver<LogLine>,
    host_frames: HostFrames,
    crc: Option<Crc>,
}

impl Log {
    /// Times lines from `start`, and reads frames with `crc`.
    pub fn new(
        start: Instant,
        monitor: Monitor,
        notices: Receiver<LogLine>,
        crc: Option<Crc>,
    ) -> Self {
        Self {
            start,
            lines: VecDeque::new(),
            monitor,
            notices,
            host_frames: HostFrames::new(crc),
            crc,
        }
    }

    /// Adds a line from the window.
    pub fn push(&mut self, kind: LogKind, text: impl Into<String>) {
        self.pull();
        self.add(LogLine::now(kind, text));
    }

    /// Adds the frames of `bytes`, which the display sent but not to the
    /// host.
    pub fn push_display_frames(&mut self, bytes: &[u8]) {
        for (kind, text) in describe_display_frames(bytes, self.crc) {
            self.push(kind, text);
        }
    }

    /// Adds what the display reported before it ran on its thread.
    pub fn push_events(&mut self, events: Vec<Event>) {
        for event in events {
            self.push(LogKind::Event, describe_event(event));
        }
    }

    /// Frames carry `crc` from now on, as after a reload.
    pub fn reloaded(&mut self, crc: Option<Crc>) {
        self.pull();
        self.crc = crc;
        self.host_frames = HostFrames::new(crc);
    }

    /// Shows the lines, the latest at the bottom, in the space left.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.pull();
        ui.horizontal(|ui| {
            ui.heading("Log");
            // Without focus, so that Space and Enter don't press it.
            let clear = Button::new("clear").small().sense(Sense::CLICK);
            if ui.add(clear).clicked() {
                self.lines.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("log")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &self.lines {
                    let color = match line.kind {
                        LogKind::Host => Color32::from_rgb(90, 160, 255),
                        LogKind::Display => Color32::from_rgb(90, 200, 120),
                        LogKind::Event => ui.visuals().weak_text_color(),
                        LogKind::Error => Color32::from_rgb(230, 90, 90),
                    };
                    let text = RichText::new(line.show(self.start)).monospace();
                    ui.label(text.color(color));
                }
            });
    }

    /// Adds what the monitor and the links reported, in the order it
    /// happened.
    fn pull(&mut self) {
        let mut lines = Vec::new();
        for activity in self.monitor.take() {
            self.describe(activity, &mut lines);
        }
        lines.extend(self.notices.try_iter());
        lines.sort_by_key(|line| line.at);
        for line in lines {
            self.add(line);
        }
    }

    fn add(&mut self, line: LogLine) {
        if self.lines.len() == LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    fn describe(&mut self, activity: Activity, lines: &mut Vec<LogLine>) {
        let at = activity.at;
        let mut line = |kind, text| lines.push(LogLine { at, kind, text });
        match activity.kind {
            ActivityKind::FromHost(bytes) => {
                for text in self.host_frames.describe(&bytes) {
                    line(LogKind::Host, text);
                }
            }
            ActivityKind::ToHost(bytes) => {
                for (kind, text) in describe_display_frames(&bytes, self.crc) {
                    line(kind, text);
                }
            }
            ActivityKind::Event(event) => line(LogKind::Event, describe_event(event)),
            ActivityKind::Overflow => line(
                LogKind::Error,
                "the hosts don't read, so what the display sends is dropped".to_owned(),
            ),
        }
    }
}

/// The CRC frames carry when the configuration's `crc` is `on`.
pub(crate) fn codec_crc(on: bool) -> Option<Crc> {
    if on { Crc::on() } else { Crc::off() }
}

fn describe_event(event: Event) -> String {
    match event {
        Event::Page(page) => format!("page {page}"),
        Event::Reset => "reset".to_owned(),
        Event::Note(note) => note,
        Event::Sound(sound) => format!("sound: {sound}"),
    }
}

/// Finds the frames among the host's bytes, for the log; the simulator
/// reads its own copy.
struct HostFrames(Accumulator<[u8; MAX_CONTENT]>);

impl HostFrames {
    fn new(crc: Option<Crc>) -> Self {
        Self(Accumulator::new([0; MAX_CONTENT], crc))
    }

    /// Takes `bytes`, which the host sent, and describes the frames they
    /// complete.
    fn describe(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        let mut input = bytes;
        while !input.is_empty() {
            input = match self.0.feed(input) {
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
        lines
    }
}

/// Describes the frames of `bytes`, which the display sent.
fn describe_display_frames(bytes: &[u8], crc: Option<Crc>) -> Vec<(LogKind, String)> {
    let mut lines = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        match degus_codec::split(rest, crc) {
            Ok((content, after)) => {
                let text = match Response::from_content(content) {
                    Ok(response) => describe_response(&response),
                    Err(error) => format!("unreadable frame: {error}"),
                };
                lines.push((LogKind::Display, text));
                rest = after;
            }
            Err(error) => {
                lines.push((LogKind::Error, format!("unreadable frame: {error}")));
                break;
            }
        }
    }
    lines
}

/// `write 0x1000 0001 0002`, `read 0x1000 2`.
pub(crate) fn describe_request(request: &Request<'_>) -> String {
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
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use degus_set::{Device, DwinSet, Size};
    use degus_sim::Sim;
    use degus_sim_gui::SimThread;

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
    fn describes_the_frames_among_the_hosts_bytes() {
        let mut frames = HostFrames::new(Crc::off());
        let none = Vec::<String>::new();
        assert_eq!(frames.describe(&[0x00, 0x5A, 0xA5, 0x04, 0x83]), none);
        assert_eq!(frames.describe(&[0x10, 0x00, 0x01]), ["read 0x1000 1"]);
    }

    #[test]
    fn formats_lines() {
        let start = Instant::now();
        let line = LogLine {
            at: start + Duration::from_millis(1250),
            kind: LogKind::Host,
            text: "read 0x1000 1".to_owned(),
        };
        assert_eq!(line.show(start), "   1.250  > read 0x1000 1");
    }

    #[test]
    fn puts_what_the_display_and_the_links_report_in_order() {
        let sim = SimThread::new(Sim::new(DwinSet::new(Device::new(Size::new(8, 8)))));
        let (notices, from_links) = channel();
        let start = Instant::now();
        let mut log = Log::new(start, sim.monitor(), from_links, Crc::off());
        notices
            .send(LogLine::now(LogKind::Event, "host connected: test"))
            .unwrap();
        let mut uart = sim.uart();
        std::io::Write::write_all(&mut uart, &[0x5A, 0xA5, 0x05, 0x82, 0x10, 0x00, 0x00, 0x07])
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while log.lines.len() < 3 && Instant::now() < deadline {
            log.pull();
            std::thread::sleep(Duration::from_millis(5));
        }
        log.push(LogKind::Event, "from the window");
        let texts: Vec<&str> = log.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "host connected: test",
                "write 0x1000 0007",
                "ack",
                "from the window"
            ]
        );
    }
}
