//! What frames carry: the commands and answers.

use crate::{Checksum, DecodeError, EncodeError, FrameWriter, split};

const REGISTER: u8 = 0x80;
const VP: u8 = 0x82;
const CURVE: u8 = 0x84;
const DWORD: u8 = 0x86;

/// An address space. Each has a write command, and a read command one above
/// it, except the curve buffer, which is write only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Space {
    /// The registers: `0x80`, `0x81`.
    Register,
    /// The variable (VP) space, by word: `0x82`, `0x83`.
    Vp,
    /// The curve buffer: `0x84`.
    Curve,
    /// The variable space, by double word: `0x86`, `0x87`.
    Dword,
}

impl Space {
    /// The command that writes this space.
    #[must_use]
    pub const fn write_command(self) -> u8 {
        match self {
            Space::Register => REGISTER,
            Space::Vp => VP,
            Space::Curve => CURVE,
            Space::Dword => DWORD,
        }
    }

    /// The command that reads this space, `None` for the curve buffer.
    #[must_use]
    pub const fn read_command(self) -> Option<u8> {
        match self {
            Space::Curve => None,
            space => Some(space.write_command() + 1),
        }
    }

    const fn from_write_command(command: u8) -> Option<Self> {
        match command {
            REGISTER => Some(Space::Register),
            VP => Some(Space::Vp),
            CURVE => Some(Space::Curve),
            DWORD => Some(Space::Dword),
            _ => None,
        }
    }
}

/// Where a command writes or reads, as it follows the command in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Address {
    /// A register on a register page: `page reg`.
    Register {
        /// The register page.
        page: u8,
        /// The register on the page.
        reg: u8,
    },
    /// A VP, a word of the variable space: 2 bytes.
    Vp(u16),
    /// The curve buffer channels to write to, one byte.
    Curve(u8),
    /// A double word of the variable space: 4 bytes.
    Dword(u32),
}

impl Address {
    /// The address space.
    #[must_use]
    pub const fn space(self) -> Space {
        match self {
            Address::Register { .. } => Space::Register,
            Address::Vp(_) => Space::Vp,
            Address::Curve(_) => Space::Curve,
            Address::Dword(_) => Space::Dword,
        }
    }

    /// Takes the address that follows `command` off `input`.
    fn take(command: u8, input: &[u8]) -> Result<(Self, &[u8]), DecodeError> {
        let space = Space::from_write_command(command & !1).ok_or(DecodeError::UnknownCommand)?;
        if command & 1 == 1 && space.read_command().is_none() {
            return Err(DecodeError::UnknownCommand);
        }
        let truncated = DecodeError::Truncated;
        Ok(match space {
            Space::Register => {
                let (&[page, reg], rest) = input.split_first_chunk().ok_or(truncated)?;
                (Address::Register { page, reg }, rest)
            }
            Space::Vp => {
                let (vp, rest) = input.split_first_chunk().ok_or(truncated)?;
                (Address::Vp(u16::from_be_bytes(*vp)), rest)
            }
            Space::Curve => {
                let (&channels, rest) = input.split_first().ok_or(truncated)?;
                (Address::Curve(channels), rest)
            }
            Space::Dword => {
                let (addr, rest) = input.split_first_chunk().ok_or(truncated)?;
                (Address::Dword(u32::from_be_bytes(*addr)), rest)
            }
        })
    }

    /// Starts a frame with `command` and this address.
    fn begin<K: Checksum>(
        self,
        command: u8,
        buf: &mut [u8],
        crc: Option<K>,
    ) -> Result<FrameWriter<'_, K>, EncodeError> {
        let mut writer = FrameWriter::new(buf, crc)?;
        writer.push(&[command])?;
        match self {
            Address::Register { page, reg } => writer.push(&[page, reg])?,
            Address::Vp(vp) => writer.push(&vp.to_be_bytes())?,
            Address::Curve(channels) => writer.push(&[channels])?,
            Address::Dword(addr) => writer.push(&addr.to_be_bytes())?,
        }
        Ok(writer)
    }
}

/// A command from the host to the display.
///
/// The display answers a [`Write`](Request::Write) with
/// [`Response::Ack`] and a [`Read`](Request::Read) with
/// [`Response::Data`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Request<'a> {
    /// Writes `data` from `addr` on: `0x80`, `0x82`, `0x84` or `0x86`.
    Write {
        /// Where the data goes.
        addr: Address,
        /// The bytes, big-endian.
        data: &'a [u8],
    },
    /// Reads `len` units from `addr` on: `0x81`, `0x83` or `0x87`.
    Read {
        /// Where to read from.
        addr: Address,
        /// How many units: bytes for registers, words for VPs, double words
        /// for [`Address::Dword`].
        len: u8,
    },
}

impl<'a> Request<'a> {
    /// Takes the request framed at the start of `input` off it. Returns the
    /// request and the input after its frame.
    ///
    /// # Errors
    ///
    /// As [`split`] and [`from_content`](Self::from_content).
    pub fn decode<K: Checksum>(
        input: &'a [u8],
        crc: Option<K>,
    ) -> Result<(Self, &'a [u8]), DecodeError> {
        let (content, rest) = split(input, crc)?;
        Ok((Self::from_content(content)?, rest))
    }

    /// Reads the request a frame's content carries, as [`split`] or an
    /// [`Accumulator`](crate::Accumulator) returns it.
    ///
    /// Bytes after a read's length are ignored.
    ///
    /// # Errors
    ///
    /// [`DecodeError::UnknownCommand`], and [`DecodeError::Truncated`] if the
    /// content ends before the address or a read's length.
    pub fn from_content(content: &'a [u8]) -> Result<Self, DecodeError> {
        let (&command, rest) = content.split_first().ok_or(DecodeError::Truncated)?;
        let (addr, rest) = Address::take(command, rest)?;
        if command & 1 == 1 {
            let (&len, _) = rest.split_first().ok_or(DecodeError::Truncated)?;
            Ok(Request::Read { addr, len })
        } else {
            Ok(Request::Write { addr, data: rest })
        }
    }

    /// Builds the request's frame at the start of `buf` and returns it.
    ///
    /// # Errors
    ///
    /// [`EncodeError::WriteOnly`] for a read of the curve buffer, and as
    /// [`FrameWriter::push`].
    pub fn encode<'b, K: Checksum>(
        &self,
        buf: &'b mut [u8],
        crc: Option<K>,
    ) -> Result<&'b [u8], EncodeError> {
        match *self {
            Request::Write { addr, data } => {
                let mut writer = Self::begin_write(buf, addr, crc)?;
                writer.push(data)?;
                Ok(writer.finish())
            }
            Request::Read { addr, len } => {
                let command = addr.space().read_command().ok_or(EncodeError::WriteOnly)?;
                let mut writer = addr.begin(command, buf, crc)?;
                writer.push(&[len])?;
                Ok(writer.finish())
            }
        }
    }

    /// Starts a write to `addr` at the start of `buf`, for the data to be
    /// pushed in place.
    ///
    /// # Errors
    ///
    /// [`EncodeError::BufferFull`] if `buf` cannot hold the command and
    /// address.
    pub fn begin_write<K: Checksum>(
        buf: &mut [u8],
        addr: Address,
        crc: Option<K>,
    ) -> Result<FrameWriter<'_, K>, EncodeError> {
        addr.begin(addr.space().write_command(), buf, crc)
    }
}

/// A frame from the display to the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Response<'a> {
    /// The answer to a write: its command, then `OK`, such as `82 4F 4B`.
    Ack(Space),
    /// The answer to a read, such as `83 vp len data`. With auto-upload on,
    /// the display also sends one unasked for a VP a touch control changed.
    Data {
        /// Where the data is from.
        addr: Address,
        /// How many units the frame says it carries: bytes for registers,
        /// words for VPs, double words for [`Address::Dword`].
        len: u8,
        /// The bytes, big-endian, as many as the frame holds.
        data: &'a [u8],
    },
}

impl<'a> Response<'a> {
    /// Takes the response framed at the start of `input` off it. Returns the
    /// response and the input after its frame.
    ///
    /// # Errors
    ///
    /// As [`split`] and [`from_content`](Self::from_content).
    pub fn decode<K: Checksum>(
        input: &'a [u8],
        crc: Option<K>,
    ) -> Result<(Self, &'a [u8]), DecodeError> {
        let (content, rest) = split(input, crc)?;
        Ok((Self::from_content(content)?, rest))
    }

    /// Reads the response a frame's content carries, as [`split`] or an
    /// [`Accumulator`](crate::Accumulator) returns it.
    ///
    /// The data is taken as it is: whether it is `len` units long is up to
    /// the caller.
    ///
    /// # Errors
    ///
    /// [`DecodeError::UnknownCommand`], [`DecodeError::BadAck`] for a write
    /// command followed by something other than `OK`, and
    /// [`DecodeError::Truncated`] if the content ends before the
    /// acknowledgement, the address or the length.
    pub fn from_content(content: &'a [u8]) -> Result<Self, DecodeError> {
        let (&command, rest) = content.split_first().ok_or(DecodeError::Truncated)?;
        if let Some(space) = Space::from_write_command(command) {
            let (ack, _) = rest
                .split_first_chunk::<2>()
                .ok_or(DecodeError::Truncated)?;
            return if ack == b"OK" {
                Ok(Response::Ack(space))
            } else {
                Err(DecodeError::BadAck)
            };
        }
        let (addr, rest) = Address::take(command, rest)?;
        let (&len, data) = rest.split_first().ok_or(DecodeError::Truncated)?;
        Ok(Response::Data { addr, len, data })
    }

    /// Builds the response's frame at the start of `buf` and returns it.
    ///
    /// # Errors
    ///
    /// [`EncodeError::WriteOnly`] for data from the curve buffer, and as
    /// [`FrameWriter::push`].
    pub fn encode<'b, K: Checksum>(
        &self,
        buf: &'b mut [u8],
        crc: Option<K>,
    ) -> Result<&'b [u8], EncodeError> {
        match *self {
            Response::Ack(space) => {
                let mut writer = FrameWriter::new(buf, crc)?;
                writer.push(&[space.write_command()])?;
                writer.push(b"OK")?;
                Ok(writer.finish())
            }
            Response::Data { addr, len, data } => {
                let mut writer = Self::begin_data(buf, addr, len, crc)?;
                writer.push(data)?;
                Ok(writer.finish())
            }
        }
    }

    /// Starts data from `addr`, `len` units of it, at the start of `buf`, for
    /// the data to be pushed in place.
    ///
    /// # Errors
    ///
    /// [`EncodeError::WriteOnly`] for the curve buffer, and
    /// [`EncodeError::BufferFull`] if `buf` cannot hold the command, address
    /// and length.
    pub fn begin_data<K: Checksum>(
        buf: &mut [u8],
        addr: Address,
        len: u8,
        crc: Option<K>,
    ) -> Result<FrameWriter<'_, K>, EncodeError> {
        let command = addr.space().read_command().ok_or(EncodeError::WriteOnly)?;
        let mut writer = addr.begin(command, buf, crc)?;
        writer.push(&[len])?;
        Ok(writer)
    }
}
