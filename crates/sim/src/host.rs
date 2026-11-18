//! What passes between the host (the firmware) and the display, whatever
//! carries it.
//!
//! A [`Codec`] turns bytes into [`HostCmd`]s and [`DeviceMsg`]s into
//! bytes. [`LineCodec`](crate::LineCodec) speaks a text protocol; the DGUS
//! frame codec (`5A A5 len cmd data [crc]`) is to come from the `dguscard`
//! crate's device side.

use std::fmt;

/// A command from the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCmd {
    /// `0x82`: write `data`, big-endian, from `vp` on.
    WriteVp { vp: u16, data: Vec<u8> },
    /// `0x83`: read `words` words from `vp` on.
    ReadVp { vp: u16, words: u8 },
}

/// A message from the display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceMsg {
    /// The answer to a write: `82 4F 4B`.
    Ack,
    /// The answer to a read: `83 vp len data`.
    ReadReply { vp: u16, data: Vec<u16> },
    /// A VP that a touch control changed, sent unasked when auto-upload is
    /// on. On the wire it is the same `83` frame as a read reply.
    Upload { vp: u16, data: Vec<u16> },
}

/// A decoded piece of the host's byte stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    Cmd(HostCmd),
    /// Bytes that could not be decoded; the display ignores them.
    Error(String),
}

/// Converts between a link's bytes and commands.
pub trait Codec: Send {
    /// Decodes `bytes`, which may end in the middle of a command: the codec
    /// keeps the rest for the next call.
    fn decode(&mut self, bytes: &[u8], out: &mut Vec<Decoded>);

    /// Appends the bytes of `msg` to `out`.
    fn encode(&mut self, msg: &DeviceMsg, out: &mut Vec<u8>);

    /// Appends what tells the host that its input could not be decoded.
    /// The display itself stays silent, so by default nothing.
    fn encode_error(&mut self, _error: &str, _out: &mut Vec<u8>) {}

    /// Drops partly received input, as when the host disconnects.
    fn reset(&mut self) {}
}

fn words(f: &mut fmt::Formatter<'_>, words: impl IntoIterator<Item = u16>) -> fmt::Result {
    for word in words {
        write!(f, " {word:#06X}")?;
    }
    Ok(())
}

/// The command in script syntax: `write 0x1000 0x0001`, `read 0x1000 2`.
impl fmt::Display for HostCmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostCmd::WriteVp { vp, data } => {
                write!(f, "write {vp:#06X}")?;
                let (pairs, rest) = data.as_chunks::<2>();
                words(f, pairs.iter().map(|&pair| u16::from_be_bytes(pair)))?;
                for byte in rest {
                    write!(f, " {byte:#04X}")?;
                }
                Ok(())
            }
            HostCmd::ReadVp { vp, words } => write!(f, "read {vp:#06X} {words}"),
        }
    }
}

/// `ack`, `data 0x1000 0x0001` or `upload 0x1000 0x0001`.
impl fmt::Display for DeviceMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceMsg::Ack => f.write_str("ack"),
            DeviceMsg::ReadReply { vp, data } => {
                write!(f, "data {vp:#06X}")?;
                words(f, data.iter().copied())
            }
            DeviceMsg::Upload { vp, data } => {
                write!(f, "upload {vp:#06X}")?;
                words(f, data.iter().copied())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_commands_in_script_syntax() {
        let write = HostCmd::WriteVp {
            vp: 0x1000,
            data: vec![0x00, 0x01, 0xAB],
        };
        assert_eq!(write.to_string(), "write 0x1000 0x0001 0xAB");
        let read = HostCmd::ReadVp {
            vp: 0x0014,
            words: 1,
        };
        assert_eq!(read.to_string(), "read 0x0014 1");
        let upload = DeviceMsg::Upload {
            vp: 0x1000,
            data: vec![1, 0xFFFF],
        };
        assert_eq!(upload.to_string(), "upload 0x1000 0x0001 0xFFFF");
    }
}
