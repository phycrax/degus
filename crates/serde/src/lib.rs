//! Encodes and decodes the values DWIN T5L displays hold in their VPs, with
//! [`serde`].
//!
//! VP space is a run of 16-bit words, big-endian: VP `0x1000` is one word,
//! and a 32-bit value at `0x1000` takes `0x1000` and `0x1001`. A value
//! encodes as the bytes it takes from its VP on, and a struct as its fields
//! one after the other, so a struct written to one VP fills the VPs after
//! it.
//!
//! - [`to_slice`] encodes a value at the start of a buffer, and
//!   [`encoded_len`] says how long it is.
//! - [`from_bytes`] decodes one value that takes all the bytes given.
//! - [`Deserializer`] takes several values off the front of some bytes,
//!   one after the other.
//!
//! The bytes are those of a frame's data, without the frame. With
//! `degus-codec`, `FrameWriter::fill` encodes straight into a frame:
//!
//! ```
//! use degus_codec::{Address, Crc, MAX_FRAME, Request, Response};
//! use serde::Serialize;
//!
//! /// `PIC_Set` (`0x0084`): `5A 01`, then the page to show.
//! #[derive(Serialize)]
//! struct PicSet {
//!     enable: u8,
//!     mode: u8,
//!     page: u16,
//! }
//!
//! let switch = PicSet { enable: 0x5A, mode: 0x01, page: 1 };
//! let mut buf = [0u8; MAX_FRAME];
//! let mut writer = Request::begin_write(&mut buf, Address::Vp(0x0084), Crc::off())?;
//! writer.fill(|room| degus_serde::to_slice(&switch, room).map(|data| data.len()))?;
//! assert_eq!(
//!     writer.finish(),
//!     [0x5A, 0xA5, 0x07, 0x82, 0x00, 0x84, 0x5A, 0x01, 0x00, 0x01]
//! );
//!
//! // The display's answer to a read of `PIC_Now` (`0x0014`): page 7.
//! let answer = [0x5A, 0xA5, 0x06, 0x83, 0x00, 0x14, 0x01, 0x00, 0x07];
//! let (Response::Data { data, .. }, _) = Response::decode(&answer, Crc::off())? else {
//!     unreachable!();
//! };
//! let page: u16 = degus_serde::from_bytes(data)?;
//! assert_eq!(page, 7);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # How values encode
//!
//! - Integers and floats: their big-endian bytes. A `u8` or `i8` is half a
//!   word, so two make one VP, the first its high byte. A value that ends
//!   with half a word ends halfway through a VP.
//! - `bool`: a word, 0 or 1. Decoding rejects other values.
//! - Structs, tuples, tuple structs and arrays: their fields in order,
//!   with nothing between them. Newtypes: what they hold. `()` and unit
//!   structs: nothing.
//! - Enums: unit variants only, as their index in a word, the first variant
//!   0. For other values, such as key codes, encode through a `u16`, as
//!   `serde_repr` does.
//! - Strings and `char`s: their UTF-8 bytes. Byte slices: their bytes.
//!   Neither has a length or an end mark, so they only encode.
//! - Sequences, such as slices: their elements without their number, so
//!   they only encode too. Decode an array instead. serde's arrays go up to
//!   32 elements.
//!
//! `Option`s, maps, enum variants with data and fields skipped at run time
//! by `#[serde(skip_serializing_if)]` have no layout and fail with
//! [`EncodeError::Unsupported`] or [`DecodeError::Unsupported`].

#![no_std]

mod de;
mod error;
mod ser;

use serde::{Deserialize, Serialize};

pub use de::Deserializer;
pub use error::{DecodeError, EncodeError};

use ser::{Count, Serializer, Slice};

/// Encodes `value` at the start of `buf` and returns the bytes it takes.
///
/// # Errors
///
/// [`EncodeError::BufferFull`] if `buf` is too short, and the other
/// [`EncodeError`]s for a value without a layout in VP space.
pub fn to_slice<'b, T: Serialize + ?Sized>(
    value: &T,
    buf: &'b mut [u8],
) -> Result<&'b mut [u8], EncodeError> {
    let mut serializer = Serializer {
        output: Slice::new(buf),
    };
    value.serialize(&mut serializer)?;
    Ok(serializer.output.finish())
}

/// The number of bytes `value` encodes to.
///
/// # Errors
///
/// As [`to_slice`], except that there is no buffer to fill.
pub fn encoded_len<T: Serialize + ?Sized>(value: &T) -> Result<usize, EncodeError> {
    let mut serializer = Serializer { output: Count(0) };
    value.serialize(&mut serializer)?;
    Ok(serializer.output.0)
}

/// Decodes a `T` that takes all of `bytes`.
///
/// # Errors
///
/// [`DecodeError::TrailingBytes`] if the value ends before `bytes` do, and
/// the other [`DecodeError`]s as [`Deserializer::take`].
pub fn from_bytes<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, DecodeError> {
    let mut deserializer = Deserializer::new(bytes);
    let value = deserializer.take()?;
    if deserializer.rest().is_empty() {
        Ok(value)
    } else {
        Err(DecodeError::TrailingBytes)
    }
}
