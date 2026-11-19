/// Frame encoding failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum EncodeError {
    /// The buffer is too short for the frame.
    #[error("the buffer is too short for the frame")]
    BufferFull,
    /// The command, data and CRC take more than the length byte counts,
    /// [`MAX_CONTENT`](crate::MAX_CONTENT) bytes.
    #[error("the frame is longer than its length byte counts")]
    FrameTooLong,
    /// Invalid op. Curve buffer is write only.
    #[error("the curve buffer is write only")]
    WriteOnly,
}

/// Frame decoding failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DecodeError {
    /// Different header, compared to `5A A5`.
    #[error("no frame header")]
    BadHeader,
    /// Length byte is too short for the command, address and CRC.
    #[error("the length is too short for a frame")]
    BadLength,
    /// Length byte is more than the [`Accumulator`](crate::Accumulator)'s buffer size.
    #[error("the frame is longer than the buffer")]
    TooLarge,
    /// The CRC trailer does not match the content.
    #[error("the CRC does not match")]
    BadCrc,
    /// A command that is not one of `0x80` to `0x87`, or `0x85`, a read of
    /// the write-only curve buffer.
    #[error("unknown command")]
    UnknownCommand,
    /// A write's answer that is not `OK`.
    #[error("the acknowledgement is not OK")]
    BadAck,
    /// The input ends before the frame does, or the frame before its
    /// address, length or acknowledgement.
    #[error("the frame ends early")]
    Truncated,
}
