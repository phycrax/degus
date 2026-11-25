//! The UART, as the display speaks it in DGUS II mode: `5A A5 len cmd data
//! [crc]`, parsed and built by `degus-codec`.
//!
//! The simulator answers `0x82` writes and `0x83` reads of the VP space. The
//! other commands, `0x80`/`0x81` registers, the `0x84` curve buffer and
//! `0x86`/`0x87` double words, are reported and get no answer.

use degus_codec::{Address, DecodeError, Feed, MAX_CONTENT, MAX_FRAME, Request, Response, Space};

use crate::sim::Sim;

/// What feeding the accumulator came to, without borrowing it.
enum Got<'a> {
    Consumed,
    Noise(&'a [u8]),
    Dropped(DecodeError, &'a [u8]),
    Frame(Vec<u8>, &'a [u8]),
}

impl Sim {
    /// Takes bytes the host sent, in chunks of any size, as a UART delivers
    /// them. Answers go to [`take_tx`](Sim::take_tx); frames the display
    /// cannot use are reported as [`Event::Note`](crate::Event::Note)s.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut input = bytes;
        while !input.is_empty() {
            let got = match self.rx.feed(input) {
                Feed::Consumed => Got::Consumed,
                Feed::Error(DecodeError::BadHeader, rest) => Got::Noise(rest),
                Feed::Error(error, rest) => Got::Dropped(error, rest),
                Feed::Frame(content, rest) => Got::Frame(content.to_vec(), rest),
            };
            input = match got {
                Got::Consumed => break,
                // Noise between frames, reported once it ends.
                Got::Noise(rest) => {
                    self.skipped += input.len() - rest.len();
                    rest
                }
                // ASSUME: the display drops a frame it cannot use without an
                // answer.
                Got::Dropped(error, rest) => {
                    self.report_skipped();
                    self.note(self.dropped(error));
                    rest
                }
                // ASSUME: bytes after a read's length are ignored, as
                // `degus-codec` reads it.
                Got::Frame(content, rest) => {
                    self.report_skipped();
                    match Request::from_content(&content) {
                        Ok(request) => self.request(request),
                        Err(error) => self.note(self.dropped(error)),
                    }
                    rest
                }
            };
        }
        self.report_skipped();
        self.refresh();
    }

    /// The bytes the display sent since the last call: answers to the
    /// host's frames, and uploads from touches.
    pub fn take_tx(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tx)
    }

    /// Sends an upload of `words` words from `vp`, as the display does for
    /// a VP a touch control changed.
    pub(crate) fn upload(&mut self, vp: u16, words: usize) {
        let Some(data) = self.vp.read(vp, words).map(be_bytes) else {
            return;
        };
        self.respond(Response::Data {
            addr: Address::Vp(vp),
            len: words as u8,
            data: &data,
        });
    }

    fn request(&mut self, request: Request<'_>) {
        match request {
            Request::Write {
                addr: Address::Vp(vp),
                data,
            } => match self.store_bytes(vp, data) {
                Ok(()) => self.respond(Response::Ack(Space::Vp)),
                Err(error) => self.note(format!("write ignored, so not answered: {error}")),
            },
            Request::Read {
                addr: Address::Vp(vp),
                len,
            } => {
                // ASSUME: a read of more words than one frame holds gets no
                // answer. The guide allows reads of 1-125 words.
                let max = self.max_words();
                if usize::from(len) > max {
                    let with = if self.crc().is_some() {
                        " with CRC"
                    } else {
                        ""
                    };
                    return self.note(format!(
                        "read of {len} words from {vp:#06X} not answered: one frame holds \
                         {max}{with}"
                    ));
                }
                self.before_read(vp, len.into());
                match self.vp.read(vp, len.into()).map(be_bytes) {
                    Some(data) => self.respond(Response::Data {
                        addr: Address::Vp(vp),
                        len,
                        data: &data,
                    }),
                    None => self.note(format!(
                        "read of {len} words from {vp:#06X} not answered: it runs past 0xFFFF"
                    )),
                }
            }
            Request::Write { addr, .. } | Request::Read { addr, .. } => {
                let read = matches!(request, Request::Read { .. });
                let what = match (addr, read) {
                    (Address::Register { .. }, false) => "0x80 register write",
                    (Address::Register { .. }, true) => "0x81 register read",
                    (Address::Curve(_), _) => "0x84 curve buffer write",
                    (Address::Dword(_), false) => "0x86 double-word write",
                    (Address::Dword(_), true) => "0x87 double-word read",
                    (Address::Vp(_), _) => unreachable!("VP requests are answered above"),
                };
                self.note(format!("{what} is not simulated, so not answered"));
            }
        }
    }

    fn respond(&mut self, response: Response<'_>) {
        let mut buf = [0u8; MAX_FRAME];
        match response.encode(&mut buf, self.crc()) {
            Ok(frame) => self.tx.extend_from_slice(frame),
            Err(error) => self.note(format!("answer not sent: {error}")),
        }
    }

    /// The most words one frame carries: 125, or 124 with CRC.
    fn max_words(&self) -> usize {
        // The command, the address and the length take 4 bytes.
        let crc = if self.crc().is_some() { 2 } else { 0 };
        (MAX_CONTENT - 4 - crc) / 2
    }

    /// Why a frame was dropped. A CRC that is on at one end only shows up as
    /// a bad CRC or a short frame, so those name the setting.
    fn dropped(&self, error: DecodeError) -> String {
        let setting = if self.config.crc {
            "the display expects a CRC"
        } else {
            "the display expects no CRC"
        };
        let why = match error {
            DecodeError::BadCrc => format!("its CRC does not match ({setting})"),
            DecodeError::BadLength => format!("its length is too short for a frame ({setting})"),
            DecodeError::UnknownCommand => "its command is unknown".to_owned(),
            DecodeError::Truncated => "it ends before its address or length".to_owned(),
            other => other.to_string(),
        };
        format!("frame dropped: {why}")
    }

    fn report_skipped(&mut self) {
        match std::mem::take(&mut self.skipped) {
            0 => {}
            1 => self.note("dropped 1 byte outside a frame"),
            n => self.note(format!("dropped {n} bytes outside a frame")),
        }
    }
}

fn be_bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_be_bytes()).collect()
}
