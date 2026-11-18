//! Scripts: drive the simulator from a text file, one command per line,
//! for tests, agents and screenshots without a window.
//!
//! ```text
//! # Comments start with '#'. Numbers are decimal or 0x hex.
//! write 0x1000 0x0001 0x0002   # host writes VPs (0x82)
//! read 0x1000 2                # host reads VPs (0x83)
//! touch 300 200                # press and release at (x, y)
//! press 300 200                # press and hold
//! move 310 200
//! release
//! wait 200ms                   # 200ms, 1s, 1.5s, or plain milliseconds
//! page 3                       # show page 3 directly
//! shot out/home.png            # save the screen, relative to the script
//! expect 0x1000 0x0001         # VPs hold these words
//! expect page 3
//! expect upload 0x1000 0x0001  # the oldest upload not yet expected
//! expect upload none           # no uploads left unexpected
//! expect sound clip 3          # the oldest sound not yet expected
//! expect sound beep 100ms
//! expect sound none
//! ```

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::audio::Sound;
use crate::host::{DeviceMsg, HostCmd};
use crate::sim::{Event, Sim};

/// One script line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Write { vp: u16, words: Vec<u16> },
    Read { vp: u16, words: u8 },
    Touch { x: u16, y: u16 },
    Press { x: u16, y: u16 },
    Move { x: u16, y: u16 },
    Release,
    Wait(Duration),
    Page(u16),
    Shot(PathBuf),
    Expect(Expectation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation {
    Vp {
        vp: u16,
        words: Vec<u16>,
    },
    Page(u16),
    Upload {
        vp: u16,
        words: Vec<u16>,
    },
    NoUpload,
    /// The oldest sound not yet expected is this clip.
    Clip(u16),
    /// The oldest sound not yet expected is a beep this long.
    Beep(Duration),
    NoSound,
}

impl Command {
    /// The host command this is, for `write` and `read`.
    pub fn host_cmd(&self) -> Option<HostCmd> {
        match self {
            Command::Write { vp, words } => Some(HostCmd::WriteVp {
                vp: *vp,
                data: words.iter().flat_map(|w| w.to_be_bytes()).collect(),
            }),
            Command::Read { vp, words } => Some(HostCmd::ReadVp {
                vp: *vp,
                words: *words,
            }),
            _ => None,
        }
    }
}

/// A script that failed, at a line numbered from 1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ScriptError {
    pub line: usize,
    pub message: String,
}

/// A number: decimal or `0x` hex.
fn number(token: &str) -> Result<u64, String> {
    let parsed = match token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        Some(hex) => u64::from_str_radix(&hex.replace('_', ""), 16),
        None => token.replace('_', "").parse(),
    };
    parsed.map_err(|_| format!("{token:?} is not a number"))
}

fn u16_arg(token: &str) -> Result<u16, String> {
    u16::try_from(number(token)?).map_err(|_| format!("{token} is over 0xFFFF"))
}

/// A word: like [`u16_arg`], and also a negative decimal as two's
/// complement.
fn word(token: &str) -> Result<u16, String> {
    if let Some(magnitude) = token.strip_prefix('-') {
        let value = number(magnitude)?;
        return i16::try_from(-(value as i64))
            .map(|v| v as u16)
            .map_err(|_| format!("{token} is below -32768"));
    }
    u16_arg(token)
}

fn duration(token: &str) -> Result<Duration, String> {
    let (value, scale) = if let Some(ms) = token.strip_suffix("ms") {
        (ms, 0.001)
    } else if let Some(s) = token.strip_suffix('s') {
        (s, 1.0)
    } else {
        (token, 0.001)
    };
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| Duration::from_secs_f64(v * scale))
        .ok_or_else(|| format!("{token:?} is not a duration such as 200ms or 1.5s"))
}

/// Parses a line; `None` for a blank or comment line.
pub fn parse_line(line: &str) -> Result<Option<Command>, String> {
    let line = line.split('#').next().unwrap_or("").trim();
    let mut tokens = line.split_whitespace();
    let Some(name) = tokens.next() else {
        return Ok(None);
    };
    let args: Vec<&str> = tokens.collect();
    let count = |n: usize| {
        if args.len() == n {
            Ok(())
        } else {
            Err(format!("{name} takes {n} argument(s)"))
        }
    };
    let at = |i: usize| u16_arg(args[i]);
    let words = |from: usize| -> Result<Vec<u16>, String> {
        if args.len() <= from {
            return Err(format!("{name} needs at least one word"));
        }
        args[from..].iter().map(|t| word(t)).collect()
    };
    Ok(Some(match name {
        "write" => Command::Write {
            vp: args
                .first()
                .map_or_else(|| Err("write needs a VP".to_owned()), |t| u16_arg(t))?,
            words: words(1)?,
        },
        "read" => {
            count(2)?;
            let words = u8::try_from(number(args[1])?)
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| format!("{} is not a word count of 1-255", args[1]))?;
            Command::Read { vp: at(0)?, words }
        }
        "touch" => {
            count(2)?;
            Command::Touch {
                x: at(0)?,
                y: at(1)?,
            }
        }
        "press" => {
            count(2)?;
            Command::Press {
                x: at(0)?,
                y: at(1)?,
            }
        }
        "move" => {
            count(2)?;
            Command::Move {
                x: at(0)?,
                y: at(1)?,
            }
        }
        "release" => {
            count(0)?;
            Command::Release
        }
        "wait" => {
            count(1)?;
            Command::Wait(duration(args[0])?)
        }
        "page" => {
            count(1)?;
            Command::Page(at(0)?)
        }
        "shot" => {
            // The rest of the line, so paths may hold spaces.
            let path = line[name.len()..].trim();
            if path.is_empty() {
                return Err("shot needs a file name".to_owned());
            }
            Command::Shot(path.into())
        }
        "expect" => Command::Expect(match args.as_slice() {
            ["page", page] => Expectation::Page(u16_arg(page)?),
            ["upload", "none"] => Expectation::NoUpload,
            ["sound", "none"] => Expectation::NoSound,
            ["sound", "clip", id] => Expectation::Clip(u16_arg(id)?),
            ["sound", "beep", length] => Expectation::Beep(duration(length)?),
            ["sound", ..] => {
                return Err("expect sound takes clip <id>, beep <length> or none".to_owned());
            }
            ["upload", vp, ..] => Expectation::Upload {
                vp: u16_arg(vp)?,
                words: words(2)?,
            },
            [vp, ..] => Expectation::Vp {
                vp: u16_arg(vp)?,
                words: words(1)?,
            },
            [] => return Err("expect needs what to check".to_owned()),
        }),
        _ => return Err(format!("unknown command {name:?}")),
    }))
}

/// Parses a whole script, reporting the first bad line.
pub fn parse(text: &str) -> Result<Vec<(usize, Command)>, ScriptError> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| match parse_line(line) {
            Ok(None) => None,
            Ok(Some(command)) => Some(Ok((i + 1, command))),
            Err(message) => Some(Err(ScriptError {
                line: i + 1,
                message,
            })),
        })
        .collect()
}

/// Runs scripts against a simulator, keeping uploads for `expect upload`.
pub struct Runner<'a> {
    sim: &'a mut Sim,
    /// Folder `shot` paths are relative to.
    base: PathBuf,
    uploads: VecDeque<DeviceMsg>,
    sounds: VecDeque<Sound>,
    log: Box<dyn FnMut(&str) + 'a>,
}

impl<'a> Runner<'a> {
    /// `log` receives a transcript: `> ` what the host sent, `< ` what the
    /// display sent, `# ` what happened.
    pub fn new(sim: &'a mut Sim, base: impl AsRef<Path>, log: impl FnMut(&str) + 'a) -> Self {
        Self {
            sim,
            base: base.as_ref().to_path_buf(),
            uploads: VecDeque::new(),
            sounds: VecDeque::new(),
            log: Box::new(log),
        }
    }

    /// Parses and runs `text`, stopping at the first failure.
    pub fn run(&mut self, text: &str) -> Result<(), ScriptError> {
        for (line, command) in parse(text)? {
            self.execute(&command)
                .map_err(|message| ScriptError { line, message })?;
        }
        Ok(())
    }

    /// Runs one command.
    pub fn execute(&mut self, command: &Command) -> Result<(), String> {
        let result = self.step(command);
        self.collect();
        result
    }

    fn step(&mut self, command: &Command) -> Result<(), String> {
        match command {
            Command::Write { .. } | Command::Read { .. } => {
                let cmd = command.host_cmd().unwrap();
                (self.log)(&format!("> {cmd}"));
                match self.sim.handle(&cmd) {
                    Some(reply) => (self.log)(&format!("< {reply}")),
                    None => (self.log)("< (no reply)"),
                }
            }
            &Command::Touch { x, y } => {
                self.sim.touch_down(x, y);
                self.collect();
                self.sim.touch_up();
            }
            &Command::Press { x, y } => self.sim.touch_down(x, y),
            &Command::Move { x, y } => self.sim.touch_move(x, y),
            Command::Release => self.sim.touch_up(),
            &Command::Wait(duration) => {
                let now = self.sim.clock() + duration;
                self.sim.tick(now);
            }
            &Command::Page(page) => self.sim.show_page(page),
            Command::Shot(path) => {
                let path = self.base.join(path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                self.sim
                    .render()
                    .save(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                (self.log)(&format!("# saved {}", path.display()));
            }
            Command::Expect(expectation) => self.check(expectation)?,
        }
        Ok(())
    }

    fn check(&mut self, expectation: &Expectation) -> Result<(), String> {
        let hex = |words: &[u16]| {
            words
                .iter()
                .map(|w| format!("{w:#06X}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        match expectation {
            Expectation::Vp { vp, words } => {
                let actual = self.sim.vp().read(*vp, words.len()).unwrap_or_default();
                if actual != words.as_slice() {
                    return Err(format!(
                        "VP {vp:#06X} holds {}, expected {}",
                        hex(actual),
                        hex(words)
                    ));
                }
            }
            Expectation::Page(page) => {
                if self.sim.page() != *page {
                    return Err(format!(
                        "page {} is shown, expected {page}",
                        self.sim.page()
                    ));
                }
            }
            Expectation::Upload { vp, words } => {
                let expected = DeviceMsg::Upload {
                    vp: *vp,
                    data: words.clone(),
                };
                match self.uploads.pop_front() {
                    Some(upload) if upload == expected => {}
                    Some(upload) => return Err(format!("got {upload}, expected {expected}")),
                    None => return Err(format!("no upload left, expected {expected}")),
                }
            }
            Expectation::NoUpload => {
                if let Some(upload) = self.uploads.front() {
                    return Err(format!("{upload} was not expected"));
                }
            }
            Expectation::Clip(id) => match self.sounds.pop_front() {
                Some(Sound::Clip { id: played, .. }) if played == *id => {}
                Some(sound) => return Err(format!("got {sound}, expected clip {id}")),
                None => return Err(format!("no sound left, expected clip {id}")),
            },
            Expectation::Beep(length) => match self.sounds.pop_front() {
                Some(Sound::Beep { length: played, .. }) if played == *length => {}
                Some(sound) => {
                    return Err(format!(
                        "got {sound}, expected a {}ms beep",
                        length.as_millis()
                    ));
                }
                None => {
                    return Err(format!(
                        "no sound left, expected a {}ms beep",
                        length.as_millis()
                    ));
                }
            },
            Expectation::NoSound => {
                if let Some(sound) = self.sounds.front() {
                    return Err(format!("{sound} was not expected"));
                }
            }
        }
        Ok(())
    }

    /// Logs and keeps what the display sent unasked, and what happened.
    fn collect(&mut self) {
        for msg in self.sim.take_outbox() {
            (self.log)(&format!("< {msg}"));
            self.uploads.push_back(msg);
        }
        for event in self.sim.take_events() {
            match event {
                Event::Page(page) => (self.log)(&format!("# page {page}")),
                Event::Reset => (self.log)("# reset"),
                Event::Note(note) => (self.log)(&format!("# {note}")),
                Event::Sound(sound) => {
                    (self.log)(&format!("# sound {sound}"));
                    self.sounds.push_back(sound);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_command() {
        let script = "
            # a comment
            write 0x1000 1 -1 0xABCD   # trailing comment
            read 0x0014 1
            touch 10 20
            press 1 2
            move 3 4
            release
            wait 1.5s
            wait 200ms
            wait 30
            page 3
            shot out/home page.png
            expect 0x1000 1
            expect page 3
            expect upload 0x1000 0x0001
            expect upload none
            expect sound clip 3
            expect sound beep 1s
            expect sound none
        ";
        let commands: Vec<Command> = parse(script).unwrap().into_iter().map(|(_, c)| c).collect();
        assert_eq!(
            commands,
            [
                Command::Write {
                    vp: 0x1000,
                    words: vec![1, 0xFFFF, 0xABCD]
                },
                Command::Read {
                    vp: 0x0014,
                    words: 1
                },
                Command::Touch { x: 10, y: 20 },
                Command::Press { x: 1, y: 2 },
                Command::Move { x: 3, y: 4 },
                Command::Release,
                Command::Wait(Duration::from_millis(1500)),
                Command::Wait(Duration::from_millis(200)),
                Command::Wait(Duration::from_millis(30)),
                Command::Page(3),
                Command::Shot("out/home page.png".into()),
                Command::Expect(Expectation::Vp {
                    vp: 0x1000,
                    words: vec![1]
                }),
                Command::Expect(Expectation::Page(3)),
                Command::Expect(Expectation::Upload {
                    vp: 0x1000,
                    words: vec![1]
                }),
                Command::Expect(Expectation::NoUpload),
                Command::Expect(Expectation::Clip(3)),
                Command::Expect(Expectation::Beep(Duration::from_secs(1))),
                Command::Expect(Expectation::NoSound),
            ]
        );
        assert!(parse_line("expect sound loud").is_err());
    }

    #[test]
    fn reports_bad_lines_by_number() {
        let error = parse("write 0x1000 1\n\nread 0x1000\n").unwrap_err();
        assert_eq!(error.line, 3);
        assert_eq!(error.message, "read takes 2 argument(s)");
        assert_eq!(
            parse_line("write 0x10000 1").unwrap_err(),
            "0x10000 is over 0xFFFF"
        );
        assert_eq!(
            parse_line("jump 1").unwrap_err(),
            "unknown command \"jump\""
        );
        assert!(parse_line("write 0x1000 -40000").is_err());
        assert!(parse_line("wait soon").is_err());
    }

    #[test]
    fn parses_the_example_in_the_project_format_guide() {
        let guide = include_str!("../../../docs/project-format.md");
        let section = guide.split("## Simulating").nth(1).unwrap();
        let example = section
            .split("```text")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .unwrap();
        assert_eq!(parse(example).unwrap().len(), 16);
        // The same for the module docs.
        let docs = include_str!("script.rs")
            .split("//! ```text")
            .nth(1)
            .unwrap();
        let docs: String = docs
            .lines()
            .take_while(|line| !line.starts_with("//! ```"))
            .map(|line| line.trim_start_matches("//!").trim_start())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse(&docs).unwrap().len(), 16);
    }
}
