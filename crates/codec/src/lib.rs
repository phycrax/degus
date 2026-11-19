//! Builds and parses the frames DWIN T5L displays speak on their UART in
//! DGUS II mode: `5A A5 len command data [crc]`.
//!
//! Both ends of the link use the same types. The host, usually the
//! firmware, encodes [`Request`]s and decodes [`Response`]s. The display, or
//! whatever stands in for it such as a simulator, decodes [`Request`]s and
//! encodes [`Response`]s.
//!
//! - [`Request::encode`] and [`Response::encode`] build a frame in a buffer;
//!   [`Request::begin_write`] and [`Response::begin_data`] hand out a
//!   [`FrameWriter`] to write the data in place instead.
//! - [`Accumulator`] collects frames from chunks of input, as a UART delivers
//!   them; [`split`] takes one whole frame off a slice.
//! - [`Request::from_content`] and [`Response::from_content`] read what a
//!   frame carries.
//!
//! Frames carry a CRC-16/MODBUS trailer when CFG `0x05` bit 7 is set. Pass
//! [`Crc::on()`] or [`Crc::off()`] to match, or `Some(..)` with your own
//! [`Checksum`], such as one that drives a CRC peripheral.
//!
//! # Example
//!
//! The host writes `0x3132` to VP `0x1000`, and the display answers.
//!
//! ```
//! use degus_codec::{Address, Crc, MAX_FRAME, Request, Response, Space};
//!
//! // The host.
//! let mut buf = [0u8; MAX_FRAME];
//! let write = Request::Write { addr: Address::Vp(0x1000), data: &[0x31, 0x32] };
//! let frame = write.encode(&mut buf, Crc::on())?;
//! assert_eq!(frame, [0x5A, 0xA5, 0x07, 0x82, 0x10, 0x00, 0x31, 0x32, 0xCC, 0x9B]);
//!
//! // The display.
//! let (request, _rest) = Request::decode(frame, Crc::on())?;
//! assert_eq!(request, write);
//! let mut buf = [0u8; MAX_FRAME];
//! let ack = Response::Ack(Space::Vp).encode(&mut buf, Crc::on())?;
//!
//! // The host again.
//! let (response, _rest) = Response::decode(ack, Crc::on())?;
//! assert_eq!(response, Response::Ack(Space::Vp));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![no_std]

mod accumulator;
mod checksum;
mod error;
mod frame;
mod message;

pub use accumulator::{Accumulator, Feed};
pub use checksum::{Checksum, Crc};
pub use error::{DecodeError, EncodeError};
pub use frame::{FrameWriter, split};
pub use message::{Address, Request, Response, Space};

/// Every frame starts with these two bytes.
pub const HEADER: [u8; 2] = [0x5A, 0xA5];

/// The header and the length byte.
pub const OVERHEAD: usize = 3;

/// The most bytes a frame's length byte counts: the command, the data and
/// the CRC.
pub const MAX_CONTENT: usize = u8::MAX as usize;

/// The longest frame, header and length byte included. A buffer this long
/// holds any frame.
pub const MAX_FRAME: usize = OVERHEAD + MAX_CONTENT;
