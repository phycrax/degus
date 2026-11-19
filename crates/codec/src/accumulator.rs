use crate::frame::{check_crc, min_len};
use crate::{Checksum, Crc, DecodeError, HEADER};

/// Collects frames from chunks of input, checking the header, length and CRC as bytes arrive.
///
/// The buffer holds one frame's content and CRC: a buffer of
/// [`MAX_CONTENT`](crate::MAX_CONTENT) bytes takes any frame. It can be
/// owned `[u8; N]` or borrowed `&mut [u8]`.
///
/// Bytes outside a frame are reported one by one as
/// [`DecodeError::BadHeader`], so noise on the line is dropped and the next
/// frame still found.
///
/// ```
/// use degus_codec::{Accumulator, Address, Crc, Feed, MAX_CONTENT, Request};
///
/// let mut acc = Accumulator::new([0u8; MAX_CONTENT], Crc::off());
/// // A write and a read, split where a UART might split them.
/// let chunks: [&[u8]; 3] = [
///     &[0x5A, 0xA5, 0x05, 0x82, 0x10],
///     &[0x00, 0x31, 0x32, 0x5A, 0xA5, 0x04],
///     &[0x83, 0x00, 0x0F, 0x01],
/// ];
/// let mut requests = Vec::new();
/// for chunk in chunks {
///     let mut input = chunk;
///     loop {
///         input = match acc.feed(input) {
///             Feed::Consumed => break,
///             Feed::Frame(content, rest) => {
///                 requests.push(format!("{:?}", Request::from_content(content)?));
///                 rest
///             }
///             Feed::Error(_noise, rest) => rest,
///         };
///     }
/// }
/// assert_eq!(requests, [
///     "Write { addr: Vp(4096), data: [49, 50] }",
///     "Read { addr: Vp(15), len: 1 }",
/// ]);
/// # Ok::<(), degus_codec::DecodeError>(())
/// ```
#[derive(Debug)]
pub struct Accumulator<B, K: Checksum = Crc> {
    buf: B,
    /// Bytes of content in `buf`.
    idx: usize,
    crc: Option<K>,
    state: State,
}

/// How far an [`Accumulator`] is into a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Waiting for `5A`.
    Idle,
    /// Has `5A`, waiting for `A5`.
    Header,
    /// Has the header, waiting for the length byte.
    Length,
    /// Has this many bytes of content and CRC still to come.
    Content(u8),
}

/// What feeding an [`Accumulator`] came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Feed<'f, 'a> {
    /// All input taken; the frame is not complete yet.
    Consumed,
    /// A frame is complete. Holds its content, the command and data with the
    /// CRC checked and stripped, then the input after the frame.
    Frame(&'f [u8], &'a [u8]),
    /// The input is not a frame, or broke the one coming in, which is
    /// dropped. Holds the input after the offending byte.
    Error(DecodeError, &'a [u8]),
}

impl<B: AsRef<[u8]> + AsMut<[u8]>, K: Checksum> Accumulator<B, K> {
    /// Collects frames into `buf`.
    pub const fn new(buf: B, crc: Option<K>) -> Self {
        Self {
            buf,
            idx: 0,
            crc,
            state: State::Idle,
        }
    }

    /// Takes `input` until a frame is complete, the input breaks the frame,
    /// or the input runs out.
    pub fn feed<'a>(&mut self, mut input: &'a [u8]) -> Feed<'_, 'a> {
        while let Some((&byte, rest)) = input.split_first() {
            input = rest;
            match self.feed_byte(byte) {
                Ok(None) => {}
                Ok(Some(len)) => return Feed::Frame(&self.buf.as_ref()[..len], rest),
                Err(error) => return Feed::Error(error, rest),
            }
        }
        Feed::Consumed
    }

    /// Drops a partly received frame, as when the other end restarts.
    pub fn reset(&mut self) {
        self.idx = 0;
        self.state = State::Idle;
    }

    /// Takes one byte. Returns the content's length once a frame is complete.
    fn feed_byte(&mut self, byte: u8) -> Result<Option<usize>, DecodeError> {
        match self.state {
            State::Idle if byte == HEADER[0] => self.state = State::Header,
            State::Idle => return Err(DecodeError::BadHeader),
            State::Header if byte == HEADER[1] => self.state = State::Length,
            // The `5A` before was noise, this one may start the header.
            State::Header if byte == HEADER[0] => return Err(DecodeError::BadHeader),
            State::Header => {
                self.state = State::Idle;
                return Err(DecodeError::BadHeader);
            }
            State::Length => {
                self.state = State::Idle;
                if usize::from(byte) < min_len(self.crc.is_some()) {
                    return Err(DecodeError::BadLength);
                }
                if usize::from(byte) > self.buf.as_ref().len() {
                    return Err(DecodeError::TooLarge);
                }
                self.idx = 0;
                self.state = State::Content(byte);
            }
            State::Content(left) => {
                // The length byte was checked against the buffer.
                self.buf.as_mut()[self.idx] = byte;
                self.idx += 1;
                if left > 1 {
                    self.state = State::Content(left - 1);
                } else {
                    self.state = State::Idle;
                    let content = &self.buf.as_ref()[..self.idx];
                    return check_crc(content, self.crc.as_mut())
                        .map(|content| Some(content.len()));
                }
            }
        }
        Ok(None)
    }
}
