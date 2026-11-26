//! The command box: frames as the host would send them, page switches and
//! taps, typed in a line.

use degus_codec::{Address, Crc, MAX_FRAME, Request};
use degus_set::controls::Point;
use degus_sim_gui::SimThread;
use degus_sim_gui::egui::{self, Key};

use crate::log::{Log, LogKind, codec_crc, describe_request};

/// A line of the command box.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    /// A frame from the host, whose answer is only logged, and how to log
    /// it.
    Frame {
        frame: Vec<u8>,
        text: String,
    },
    Page(u16),
    /// A press and a release.
    Touch(Point),
}

impl Command {
    /// Reads `line`; `None` when it is empty. Frames carry `crc`.
    fn parse(line: &str, crc: Option<Crc>) -> Result<Option<Self>, String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let numbers: Option<Vec<i64>> = words.iter().skip(1).map(|w| int(w)).collect();
        let command = match (words.first().copied(), numbers.as_deref()) {
            (None, _) => return Ok(None),
            (Some("write"), Some([vp, values @ ..])) if !values.is_empty() => {
                let data: Option<Vec<u8>> = values
                    .iter()
                    .map(|&v| u16::try_from(v).ok().map(u16::to_be_bytes))
                    .collect::<Option<Vec<_>>>()
                    .map(|words| words.concat());
                match (u16::try_from(*vp), data) {
                    (Ok(vp), Some(data)) => frame(
                        &Request::Write {
                            addr: Address::Vp(vp),
                            data: &data,
                        },
                        crc,
                    )?,
                    _ => return Err("write takes a VP and 16-bit words".to_owned()),
                }
            }
            (Some("read"), Some([vp, rest @ ..])) if rest.len() <= 1 => {
                let len = rest.first().copied().unwrap_or(1);
                match (u16::try_from(*vp), u8::try_from(len)) {
                    (Ok(vp), Ok(len)) => frame(
                        &Request::Read {
                            addr: Address::Vp(vp),
                            len,
                        },
                        crc,
                    )?,
                    _ => return Err("read takes a VP and a word count".to_owned()),
                }
            }
            (Some("page"), Some([page])) => match u16::try_from(*page) {
                Ok(page) => Self::Page(page),
                Err(_) => return Err("no such page".to_owned()),
            },
            (Some("touch"), Some([x, y])) => match (u16::try_from(*x), u16::try_from(*y)) {
                (Ok(x), Ok(y)) => Self::Touch(Point::new(x, y)),
                _ => return Err("touch takes x and y".to_owned()),
            },
            _ => {
                return Err(format!(
                    "{line:?}: expected write VP WORD..., read VP [COUNT], page N or touch X Y"
                ));
            }
        };
        Ok(Some(command))
    }
}

/// `request` as the host sends it.
fn frame(request: &Request<'_>, crc: Option<Crc>) -> Result<Command, String> {
    let mut buf = [0u8; MAX_FRAME];
    let frame = request.encode(&mut buf, crc).map_err(|e| e.to_string())?;
    Ok(Command::Frame {
        frame: frame.to_vec(),
        text: format!("{} (command box)", describe_request(request)),
    })
}

/// `0x1000` or `4096`.
pub(crate) fn int(text: &str) -> Option<i64> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// A line to type frames from the host, page switches and taps into, run
/// with Enter. A line that fails stays, and the log says why.
#[derive(Debug, Default)]
pub(crate) struct CommandBox {
    line: String,
}

impl CommandBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, sim: &SimThread, log: &mut Log) {
        ui.heading("Command");
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut self.line)
                    .hint_text("write 0x1000 1")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text(
                "As the host, answers only logged: write 0x1000 1 2, read 0x1000 2
                 As a finger: touch x y
                 page n",
            );
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            match run(&self.line, sim, log) {
                Ok(()) => self.line.clear(),
                Err(error) => log.push(LogKind::Error, error),
            }
            response.request_focus();
        }
    }
}

/// Runs `line`. Frames go to the display as from the host, but their
/// answers go only to the log.
fn run(line: &str, sim: &SimThread, log: &mut Log) -> Result<(), String> {
    let crc = codec_crc(sim.lock().config().crc);
    match Command::parse(line, crc)? {
        None => {}
        Some(Command::Frame { frame, text }) => {
            log.push(LogKind::Host, text);
            let answer = sim.update(|sim| {
                sim.feed(&frame);
                sim.take_tx()
            });
            log.push_display_frames(&answer);
        }
        Some(Command::Page(page)) => sim.update(|sim| sim.show_page(page)),
        Some(Command::Touch(at)) => sim.update(|sim| {
            sim.press(at);
            sim.release();
        }),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::*;

    #[test]
    fn reads_numbers() {
        assert_eq!(int("0x1000"), Some(0x1000));
        assert_eq!(int("0X1f"), Some(0x1F));
        assert_eq!(int("12"), Some(12));
        assert_eq!(int("-2"), Some(-2));
        assert_eq!(int("x"), None);
    }

    #[test]
    fn reads_commands() {
        let parse = |line| Command::parse(line, Crc::off());
        assert_eq!(parse("  "), Ok(None));
        assert_eq!(
            parse("write 0x1000 1 0x0203"),
            Ok(Some(Command::Frame {
                frame: hex!("5AA5 07 82 1000 0001 0203").to_vec(),
                text: "write 0x1000 0001 0203 (command box)".to_owned(),
            }))
        );
        let Ok(Some(Command::Frame { frame, .. })) = Command::parse("read 0x14", Crc::on()) else {
            panic!("a read");
        };
        assert_eq!(frame[..7], hex!("5AA5 06 83 0014 01"));
        assert_eq!(parse("page 3"), Ok(Some(Command::Page(3))));
        assert_eq!(
            parse("touch 10 20"),
            Ok(Some(Command::Touch(Point::new(10, 20))))
        );
        assert!(parse("write 0x1000 0x10000").is_err());
        assert!(parse("read 0x1000 1 2").is_err());
        assert!(parse("page -1").is_err());
        assert!(parse("jump").is_err());
        // More words than a frame holds.
        let long = format!("write 0x1000{}", " 1".repeat(130));
        assert!(parse(&long).is_err());
    }
}
