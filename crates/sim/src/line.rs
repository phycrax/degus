//! A text protocol for links, until DGUS frames come from `dguscard`: the
//! host sends script lines (`write 0x1000 1`, `read 0x1000 2`), the display
//! answers `ack`, `data 0x1000 0x0001` and sends `upload 0x1000 0x0001`.
//! Easy to drive from `nc`, Python or a test.

use crate::host::{Codec, Decoded, DeviceMsg};
use crate::script::parse_line;

/// Longest line kept; anything longer is dropped as an error.
const MAX_LINE: usize = 4096;

/// The text protocol. See the [module docs](self).
#[derive(Debug, Default)]
pub struct LineCodec {
    pending: Vec<u8>,
}

impl LineCodec {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Codec for LineCodec {
    fn decode(&mut self, bytes: &[u8], out: &mut Vec<Decoded>) {
        for &byte in bytes {
            if byte != b'\n' {
                self.pending.push(byte);
                if self.pending.len() > MAX_LINE {
                    self.pending.clear();
                    out.push(Decoded::Error(format!("line longer than {MAX_LINE} bytes")));
                }
                continue;
            }
            let line = String::from_utf8_lossy(&self.pending).into_owned();
            self.pending.clear();
            match parse_line(&line) {
                Ok(None) => {}
                Ok(Some(command)) => match command.host_cmd() {
                    Some(cmd) => out.push(Decoded::Cmd(cmd)),
                    None => out.push(Decoded::Error(format!(
                        "{:?}: only write and read come from the host",
                        line.trim()
                    ))),
                },
                Err(message) => out.push(Decoded::Error(message)),
            }
        }
    }

    fn encode(&mut self, msg: &DeviceMsg, out: &mut Vec<u8>) {
        out.extend_from_slice(msg.to_string().as_bytes());
        out.push(b'\n');
    }

    fn encode_error(&mut self, error: &str, out: &mut Vec<u8>) {
        out.extend_from_slice(format!("error {error}\n").as_bytes());
    }

    fn reset(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HostCmd;

    #[test]
    fn decodes_lines_split_across_reads() {
        let mut codec = LineCodec::new();
        let mut out = Vec::new();
        codec.decode(b"write 0x1000 0x00", &mut out);
        assert!(out.is_empty());
        codec.decode(b"01\r\nread 0x1000 1\n\ntouch 1 2\n", &mut out);
        assert_eq!(
            out[..2],
            [
                Decoded::Cmd(HostCmd::WriteVp {
                    vp: 0x1000,
                    data: vec![0x00, 0x01]
                }),
                Decoded::Cmd(HostCmd::ReadVp {
                    vp: 0x1000,
                    words: 1
                }),
            ]
        );
        assert!(matches!(&out[2], Decoded::Error(e) if e.contains("only write and read")));
    }

    #[test]
    fn encodes_messages_as_lines() {
        let mut codec = LineCodec::new();
        let mut out = Vec::new();
        codec.encode(&DeviceMsg::Ack, &mut out);
        codec.encode(
            &DeviceMsg::Upload {
                vp: 0x1000,
                data: vec![1],
            },
            &mut out,
        );
        assert_eq!(out, b"ack\nupload 0x1000 0x0001\n");
    }
}
