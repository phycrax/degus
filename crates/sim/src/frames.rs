//! DGUS frames, as the panel speaks them on UART2: `5A A5 len cmd data [crc]`,
//! parsed and built by `degus-codec`.
//!
//! The simulator answers `0x82` writes and `0x83` reads of the VP space. The
//! other commands, `0x80`/`0x81` registers, the `0x84` curve buffer and
//! `0x86`/`0x87` double words, are reported and get no answer.

use std::fmt;

use degus_codec::{
    Accumulator, Address, Crc, DecodeError, Feed, MAX_CONTENT, MAX_FRAME, Request, Response, Space,
};

use crate::host::{Codec, Decoded, DeviceMsg, HostCmd};

/// DGUS frames. See the [module docs](self).
pub struct FrameCodec {
    crc: bool,
    acc: Accumulator<[u8; MAX_CONTENT]>,
    /// Bytes dropped while looking for a header, not reported yet.
    skipped: usize,
}

impl FrameCodec {
    /// Frames with a CRC trailer or without, as the project's `crc` sets
    /// CFG `0x05` bit 7.
    pub fn new(crc: bool) -> Self {
        Self {
            crc,
            acc: Accumulator::new([0; MAX_CONTENT], checksum(crc)),
            skipped: 0,
        }
    }

    /// The most words one frame carries: 125, or 124 with CRC.
    fn max_words(&self) -> usize {
        // The command, the address and the length take 4 bytes.
        (MAX_CONTENT - 4 - if self.crc { 2 } else { 0 }) / 2
    }
}

fn checksum(crc: bool) -> Option<Crc> {
    if crc { Crc::on() } else { Crc::off() }
}

impl fmt::Debug for FrameCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameCodec")
            .field("crc", &self.crc)
            .finish_non_exhaustive()
    }
}

impl Codec for FrameCodec {
    fn decode(&mut self, bytes: &[u8], out: &mut Vec<Decoded>) {
        let mut input = bytes;
        while !input.is_empty() {
            let (decoded, rest) = match self.acc.feed(input) {
                Feed::Consumed => break,
                // Noise between frames, reported once it ends.
                Feed::Error(DecodeError::BadHeader, rest) => {
                    self.skipped += input.len() - rest.len();
                    input = rest;
                    continue;
                }
                // ASSUME: the display drops a frame it cannot use without
                // an answer.
                Feed::Error(error, rest) => (Decoded::Error(dropped(error, self.crc)), rest),
                // ASSUME: bytes after a read's length are ignored, as
                // `degus-codec` reads it.
                Feed::Frame(content, rest) => match Request::from_content(content) {
                    Ok(request) => (command(request), rest),
                    Err(error) => (Decoded::Error(dropped(error, self.crc)), rest),
                },
            };
            report_skipped(&mut self.skipped, out);
            out.push(decoded);
            input = rest;
        }
        report_skipped(&mut self.skipped, out);
    }

    fn encode(&mut self, msg: &DeviceMsg, out: &mut Vec<u8>) -> Result<(), String> {
        let mut buf = [0u8; MAX_FRAME];
        let crc = checksum(self.crc);
        let frame = match msg {
            DeviceMsg::Ack => Response::Ack(Space::Vp).encode(&mut buf, crc),
            DeviceMsg::ReadReply { vp, data } | DeviceMsg::Upload { vp, data } => {
                // ASSUME: a read of more words than one frame holds gets no
                // answer. The guide allows reads of 1-125 words.
                let max = self.max_words();
                let Some(len) = u8::try_from(data.len()).ok().filter(|_| data.len() <= max) else {
                    let with = if self.crc { " with CRC" } else { "" };
                    return Err(format!(
                        "{} words do not fit in one frame; {max} do{with}",
                        data.len()
                    ));
                };
                Response::begin_data(&mut buf, Address::Vp(*vp), len, crc).and_then(|mut writer| {
                    for word in data {
                        writer.push(&word.to_be_bytes())?;
                    }
                    Ok(writer.finish())
                })
            }
        };
        out.extend_from_slice(frame.map_err(|e| e.to_string())?);
        Ok(())
    }

    fn reset(&mut self) {
        self.acc.reset();
        self.skipped = 0;
    }
}

/// What the simulator does with a request: VP writes and reads go to it,
/// the rest is reported.
fn command(request: Request<'_>) -> Decoded {
    let what = match request {
        Request::Write {
            addr: Address::Vp(vp),
            data,
        } => {
            return Decoded::Cmd(HostCmd::WriteVp {
                vp,
                data: data.to_vec(),
            });
        }
        Request::Read {
            addr: Address::Vp(vp),
            len,
        } => {
            return Decoded::Cmd(HostCmd::ReadVp { vp, words: len });
        }
        Request::Write {
            addr: Address::Register { .. },
            ..
        } => "0x80 register write",
        Request::Read {
            addr: Address::Register { .. },
            ..
        } => "0x81 register read",
        Request::Write {
            addr: Address::Curve(_),
            ..
        } => "0x84 curve buffer write",
        Request::Read {
            addr: Address::Curve(_),
            ..
        } => "0x85 curve buffer read",
        Request::Write {
            addr: Address::Dword(_),
            ..
        } => "0x86 double-word write",
        Request::Read {
            addr: Address::Dword(_),
            ..
        } => "0x87 double-word read",
    };
    Decoded::Error(format!("{what}: not simulated, so not answered"))
}

/// Why a frame was dropped. A CRC that is on at one end only shows up as a
/// bad CRC or a short frame, so those name the setting.
fn dropped(error: DecodeError, crc: bool) -> String {
    let setting = if crc { "crc = true" } else { "crc = false" };
    let why = match error {
        DecodeError::BadCrc => format!("its CRC does not match ({setting} in [config])"),
        DecodeError::BadLength => {
            format!("its length is too short for a frame ({setting} in [config])")
        }
        DecodeError::UnknownCommand => "its command is unknown".to_owned(),
        DecodeError::Truncated => "it ends before its address or length".to_owned(),
        other => other.to_string(),
    };
    format!("frame dropped: {why}")
}

fn report_skipped(skipped: &mut usize, out: &mut Vec<Decoded>) {
    match std::mem::take(skipped) {
        0 => {}
        1 => out.push(Decoded::Error("dropped 1 byte outside a frame".to_owned())),
        n => out.push(Decoded::Error(format!("dropped {n} bytes outside a frame"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes written as the guide writes them: `5AA5 05 82 1000 3132`.
    fn hex(text: &str) -> Vec<u8> {
        let digits: String = text.split_whitespace().collect();
        (0..digits.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
            .collect()
    }

    fn decode(codec: &mut FrameCodec, bytes: &[u8]) -> Vec<Decoded> {
        let mut out = Vec::new();
        codec.decode(bytes, &mut out);
        out
    }

    fn encode(codec: &mut FrameCodec, msg: &DeviceMsg) -> Vec<u8> {
        let mut out = Vec::new();
        codec.encode(msg, &mut out).unwrap();
        out
    }

    fn write(vp: u16, data: &[u8]) -> Decoded {
        Decoded::Cmd(HostCmd::WriteVp {
            vp,
            data: data.to_vec(),
        })
    }

    fn read(vp: u16, words: u8) -> Decoded {
        Decoded::Cmd(HostCmd::ReadVp { vp, words })
    }

    /// The examples in DWIN's T5L DGUSII Application Development Guide, §4.1.
    #[test]
    fn speaks_the_guides_examples() {
        let examples = [
            (
                false,
                "5AA5 05 82 1000 3132",
                "5AA5 04 83 000F 01",
                "5AA5 03 82 4F4B",
                "5AA5 06 83 000F 01 1410",
                "5AA5 06 83 1001 01 005A",
            ),
            (
                true,
                "5AA5 07 82 1000 3132 CC9B",
                "5AA5 06 83 000F 01 ED90",
                "5AA5 05 82 4F4B A5EF",
                "5AA5 08 83 000F 01 1410 43F0",
                "5AA5 08 83 1001 01 005A 0E2C",
            ),
        ];
        for (crc, write_frame, read_frame, ack, reply, upload) in examples {
            let mut codec = FrameCodec::new(crc);
            assert_eq!(
                decode(&mut codec, &hex(write_frame)),
                [write(0x1000, &[0x31, 0x32])]
            );
            assert_eq!(decode(&mut codec, &hex(read_frame)), [read(0x000F, 1)]);
            assert_eq!(encode(&mut codec, &DeviceMsg::Ack), hex(ack));
            let msg = DeviceMsg::ReadReply {
                vp: 0x000F,
                data: vec![0x1410],
            };
            assert_eq!(encode(&mut codec, &msg), hex(reply));
            let msg = DeviceMsg::Upload {
                vp: 0x1001,
                data: vec![0x005A],
            };
            assert_eq!(encode(&mut codec, &msg), hex(upload));
        }
    }

    #[test]
    fn decodes_frames_split_anywhere() {
        let mut codec = FrameCodec::new(true);
        let bytes = hex("5AA5 07 82 1000 3132 CC9B 5AA5 06 83 000F 01 ED90");
        let mut out = Vec::new();
        for byte in bytes {
            codec.decode(&[byte], &mut out);
        }
        assert_eq!(out, [write(0x1000, &[0x31, 0x32]), read(0x000F, 1)]);
    }

    #[test]
    fn reports_noise_once() {
        let mut codec = FrameCodec::new(false);
        let out = decode(&mut codec, &hex("00 11 22 5AA5 04 83 000F 01 33"));
        assert_eq!(
            out,
            [
                Decoded::Error("dropped 3 bytes outside a frame".to_owned()),
                read(0x000F, 1),
                Decoded::Error("dropped 1 byte outside a frame".to_owned()),
            ]
        );
    }

    #[test]
    fn names_the_crc_setting_when_frames_do_not_fit_it() {
        // The host sends no CRC; the display expects one.
        let mut codec = FrameCodec::new(true);
        let out = decode(&mut codec, &hex("5AA5 05 82 1000 3132 5AA5 04 83 000F 01"));
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(
            matches!(&out[0], Decoded::Error(e) if e.contains("CRC does not match (crc = true"))
        );
        assert!(
            matches!(&out[1], Decoded::Error(e) if e.contains("too short for a frame (crc = true"))
        );
        // What followed the short frame's length byte.
        assert_eq!(
            out[2],
            Decoded::Error("dropped 4 bytes outside a frame".to_owned())
        );
    }

    #[test]
    fn reports_what_it_does_not_simulate() {
        // The guide's register read: 4 bytes of R10-R13 on page 0.
        let mut codec = FrameCodec::new(false);
        let out = decode(&mut codec, &hex("5AA5 04 81 00 0A 04"));
        assert!(matches!(&out[..], [Decoded::Error(e)] if e.starts_with("0x81 register read")));
    }

    #[test]
    fn refuses_replies_longer_than_a_frame() {
        let reply = |words| DeviceMsg::ReadReply {
            vp: 0x1000,
            data: vec![0xABCD; words],
        };
        let mut out = Vec::new();
        let mut codec = FrameCodec::new(false);
        codec.encode(&reply(125), &mut out).unwrap();
        assert_eq!(out.len(), MAX_FRAME - 1);
        assert!(codec.encode(&reply(126), &mut Vec::new()).is_err());

        let mut codec = FrameCodec::new(true);
        codec.encode(&reply(124), &mut Vec::new()).unwrap();
        let error = codec.encode(&reply(125), &mut Vec::new()).unwrap_err();
        assert_eq!(error, "125 words do not fit in one frame; 124 do with CRC");
    }

    #[test]
    fn reset_drops_a_partial_frame() {
        let mut codec = FrameCodec::new(false);
        assert!(decode(&mut codec, &hex("5AA5 05 82 10")).is_empty());
        codec.reset();
        let out = decode(&mut codec, &hex("00 0001 5AA5 04 83 000F 01"));
        assert_eq!(
            out,
            [
                Decoded::Error("dropped 3 bytes outside a frame".to_owned()),
                read(0x000F, 1),
            ]
        );
    }
}
