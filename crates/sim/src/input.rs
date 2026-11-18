//! Data input: typing a number on a keyboard of basic touch keys.

use degus_controls::{DataInput, InputDataType};

use crate::format::{InputWrite, input_write, parse_input};
use crate::sim::Sim;

/// Key codes of basic touch keys.
pub const KEY_DOT: u16 = 0x2E;
pub const KEY_SIGN: u16 = 0x2D;
pub const KEY_CANCEL: u16 = 0xF0;
pub const KEY_ENTER: u16 = 0xF1;
pub const KEY_BACKSPACE: u16 = 0xF2;

/// A data input being typed into.
#[derive(Debug, Clone)]
pub struct InputSession {
    pub control: DataInput,
    /// What was typed: digits, at most one `.`, and a leading `-`.
    pub text: String,
    /// `load_vp` and its value before input, put back when input ends.
    restore: Option<(u16, u16)>,
}

enum KeyOutcome {
    Edited,
    Enter,
    Cancel,
}

impl InputSession {
    /// The text as shown: `*` for each character when masked.
    pub fn shown(&self) -> String {
        if self.control.masked {
            "*".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        }
    }

    fn signed(&self) -> bool {
        !matches!(
            self.control.data_type,
            InputDataType::HighByte | InputDataType::LowByte
        )
    }

    fn key(&mut self, code: u16) -> KeyOutcome {
        let digits = self.text.trim_start_matches('-');
        let (integer, fraction) = match digits.split_once('.') {
            Some((integer, fraction)) => (integer.len(), Some(fraction.len())),
            None => (digits.len(), None),
        };
        match code {
            0x30..=0x39 => {
                let room = match fraction {
                    Some(len) => len < usize::from(self.control.decimal_digits),
                    None => integer < usize::from(self.control.integer_digits),
                };
                if room {
                    self.text.push(char::from(code as u8));
                }
            }
            KEY_DOT if self.control.decimal_digits > 0 && fraction.is_none() => {
                self.text.push('.');
            }
            KEY_SIGN if self.signed() => match self.text.strip_prefix('-') {
                Some(rest) => self.text = rest.to_owned(),
                None => self.text.insert(0, '-'),
            },
            KEY_BACKSPACE => {
                self.text.pop();
                if self.text == "-" {
                    self.text.clear();
                }
            }
            KEY_CANCEL => return KeyOutcome::Cancel,
            KEY_ENTER => return KeyOutcome::Enter,
            _ => {}
        }
        KeyOutcome::Edited
    }
}

impl Sim {
    /// Starts typing into `control`, ending any other input.
    pub(crate) fn start_input(&mut self, control: DataInput) {
        self.end_input();
        // ASSUME: "load data during input" writes `load_value` at the start and
        // puts the old value back when input ends.
        let restore = control.load_vp.map(|vp| (vp, self.vp.get(vp)));
        if let Some(vp) = control.load_vp {
            let _ = self.store(vp, &[control.load_value]);
        }
        self.input = Some(InputSession {
            control,
            text: String::new(),
            restore,
        });
        self.changed();
    }

    /// A basic touch key was pressed.
    pub(crate) fn input_key(&mut self, code: u16) {
        let Some(session) = &mut self.input else {
            return;
        };
        match session.key(code) {
            KeyOutcome::Edited => {}
            KeyOutcome::Enter => self.enter_input(),
            KeyOutcome::Cancel => self.end_input(),
        }
        self.changed();
    }

    /// Writes the typed number and ends input. Input without digits, outside
    /// the limits or too large for the type is dropped, like a cancel.
    fn enter_input(&mut self) {
        let Some(session) = &self.input else {
            return;
        };
        let control = session.control.clone();
        let typed = session.text.clone();
        let write = parse_input(&typed, control.decimal_digits).and_then(|value| {
            let allowed = control.limits.as_ref().is_none_or(|limits| {
                (i128::from(*limits.start())..=i128::from(*limits.end())).contains(&value)
            });
            if allowed {
                input_write(value, control.data_type, control.decimal_digits)
            } else {
                None
            }
        });
        self.end_input();
        let Some(write) = write else {
            if !typed.is_empty() {
                self.note(format!(
                    "input {typed:?} for VP {:#06X} rejected",
                    control.vp
                ));
            }
            return;
        };
        let words = match write {
            InputWrite::Words(words) => {
                let _ = self.store(control.vp, &words);
                words.len()
            }
            InputWrite::HighByte(byte) => {
                let word = u16::from(byte) << 8 | self.vp.get(control.vp) & 0x00FF;
                let _ = self.store(control.vp, &[word]);
                1
            }
            InputWrite::LowByte(byte) => {
                let word = self.vp.get(control.vp) & 0xFF00 | u16::from(byte);
                let _ = self.store(control.vp, &[word]);
                1
            }
        };
        if control.upload && self.auto_upload {
            self.upload(control.vp, words);
        }
    }

    /// Ends input without writing.
    pub(crate) fn end_input(&mut self) {
        let Some(session) = self.input.take() else {
            return;
        };
        if let Some((vp, value)) = session.restore {
            let _ = self.store(vp, &[value]);
        }
        self.changed();
    }
}
