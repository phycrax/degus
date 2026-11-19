//! The frame around the content, the same in both directions: the header,
//! the length byte and the CRC trailer.

use crate::{Checksum, Crc, DecodeError, EncodeError, HEADER, MAX_CONTENT, OVERHEAD};

/// The CRC trailer, little-endian.
pub(crate) const CRC_LEN: usize = 2;

/// The shortest content: a command and two bytes, such as `82 4F 4B`.
const MIN_CONTENT: usize = 3;

/// The smallest length byte a frame can have.
pub(crate) const fn min_len(crc: bool) -> usize {
    MIN_CONTENT + if crc { CRC_LEN } else { 0 }
}

/// Checks and strips the CRC trailer of `content`, if there is one.
pub(crate) fn check_crc<'a, K: Checksum>(
    content: &'a [u8],
    crc: Option<&mut K>,
) -> Result<&'a [u8], DecodeError> {
    let Some(crc) = crc else {
        return Ok(content);
    };
    let (content, trailer) = content.split_last_chunk().ok_or(DecodeError::Truncated)?;
    if u16::from_le_bytes(*trailer) == crc.compute(content) {
        Ok(content)
    } else {
        Err(DecodeError::BadCrc)
    }
}

/// Takes the whole frame at the start of `input` off it. Returns the frame's
/// content, its command and data with the CRC checked and stripped, and the
/// input after the frame.
///
/// The frame must start at `input[0]`. To find frames in a stream, use an
/// [`Accumulator`](crate::Accumulator).
///
/// # Errors
///
/// [`DecodeError::BadHeader`] if `input` does not start with
/// [`HEADER`], [`DecodeError::BadLength`] if the length byte is too small,
/// [`DecodeError::Truncated`] if `input` ends before the frame does, and
/// [`DecodeError::BadCrc`].
pub fn split<K: Checksum>(input: &[u8], mut crc: Option<K>) -> Result<(&[u8], &[u8]), DecodeError> {
    let input = input.strip_prefix(&HEADER).ok_or(DecodeError::BadHeader)?;
    let (&len, input) = input.split_first().ok_or(DecodeError::Truncated)?;
    let len = usize::from(len);
    if len < min_len(crc.is_some()) {
        return Err(DecodeError::BadLength);
    }
    let (content, rest) = input.split_at_checked(len).ok_or(DecodeError::Truncated)?;
    Ok((check_crc(content, crc.as_mut())?, rest))
}

/// Builds one frame in a buffer: the header and length byte first, then the
/// content pushed piece by piece, then the CRC trailer on
/// [`finish`](Self::finish).
///
/// [`Request::begin_write`](crate::Request::begin_write) and
/// [`Response::begin_data`](crate::Response::begin_data) start one with the
/// command and address pushed; push the data after them. A frame needs at
/// least 3 bytes of content for the other end to take it.
///
/// ```
/// use degus_codec::{Address, Crc, MAX_FRAME, Request};
///
/// let mut buf = [0u8; MAX_FRAME];
/// let mut writer = Request::begin_write(&mut buf, Address::Vp(0x1000), Crc::off())?;
/// writer.push(&0x3132_u16.to_be_bytes())?;
/// writer.reserve(2)?.copy_from_slice(&[0x33, 0x34]);
/// assert_eq!(writer.finish(), [0x5A, 0xA5, 0x07, 0x82, 0x10, 0x00, 0x31, 0x32, 0x33, 0x34]);
/// # Ok::<(), degus_codec::EncodeError>(())
/// ```
#[derive(Debug)]
pub struct FrameWriter<'b, K: Checksum = Crc> {
    buf: &'b mut [u8],
    len: usize,
    crc: Option<K>,
}

impl<'b, K: Checksum> FrameWriter<'b, K> {
    /// Starts a frame at the start of `buf`.
    ///
    /// # Errors
    ///
    /// [`EncodeError::BufferFull`] if `buf` cannot hold the header, the
    /// length byte and the CRC trailer.
    pub fn new(buf: &'b mut [u8], crc: Option<K>) -> Result<Self, EncodeError> {
        let mut writer = Self { buf, len: 0, crc };
        if writer.buf.len() < OVERHEAD + writer.trailer() {
            return Err(EncodeError::BufferFull);
        }
        writer.buf[..2].copy_from_slice(&HEADER);
        // The length byte, filled in by `finish`.
        writer.buf[2] = 0;
        writer.len = OVERHEAD;
        Ok(writer)
    }

    const fn trailer(&self) -> usize {
        if self.crc.is_some() { CRC_LEN } else { 0 }
    }

    /// Appends `n` zeroed bytes to the content and returns them, to be filled
    /// in place.
    ///
    /// # Errors
    ///
    /// [`EncodeError::FrameTooLong`] if the content and CRC would take more
    /// than [`MAX_CONTENT`] bytes, and [`EncodeError::BufferFull`] if the
    /// buffer cannot hold them. Nothing is appended then.
    pub fn reserve(&mut self, n: usize) -> Result<&mut [u8], EncodeError> {
        let end = self.len.checked_add(n).ok_or(EncodeError::FrameTooLong)?;
        if end - OVERHEAD + self.trailer() > MAX_CONTENT {
            return Err(EncodeError::FrameTooLong);
        }
        if end + self.trailer() > self.buf.len() {
            return Err(EncodeError::BufferFull);
        }
        let slot = &mut self.buf[self.len..end];
        slot.fill(0);
        self.len = end;
        Ok(slot)
    }

    /// Appends `bytes` to the content.
    ///
    /// # Errors
    ///
    /// As [`reserve`](Self::reserve).
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        self.reserve(bytes.len())?.copy_from_slice(bytes);
        Ok(())
    }

    /// The content pushed so far: the command and data.
    #[must_use]
    pub fn content(&self) -> &[u8] {
        &self.buf[OVERHEAD..self.len]
    }

    /// Appends the CRC trailer, if the frame has one, fills in the length byte
    /// and returns the frame.
    #[must_use]
    pub fn finish(self) -> &'b [u8] {
        let Self {
            buf,
            mut len,
            mut crc,
        } = self;
        if let Some(crc) = crc.as_mut() {
            // `reserve` kept room for it.
            let sum = crc.compute(&buf[OVERHEAD..len]);
            buf[len..len + CRC_LEN].copy_from_slice(&sum.to_le_bytes());
            len += CRC_LEN;
        }
        debug_assert!(len - OVERHEAD <= MAX_CONTENT);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`reserve` keeps the content within the length byte"
        )]
        {
            buf[2] = (len - OVERHEAD) as u8;
        }
        &buf[..len]
    }
}
