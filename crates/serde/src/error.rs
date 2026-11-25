use core::fmt::Display;

/// Encoding failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum EncodeError {
    /// The buffer is too short for the value.
    #[error("the buffer is too short for the value")]
    BufferFull,
    /// An enum variant's index is more than a word holds.
    #[error("the variant index is more than a word holds")]
    BadEnum,
    /// The value has a part VP space has no layout for, such as an
    /// `Option`, a map or an enum variant with data.
    #[error("the value has no layout in VP space")]
    Unsupported,
    /// The value's `Serialize` reported an error.
    #[error("the value failed to serialize")]
    Custom,
}

impl serde::ser::Error for EncodeError {
    fn custom<T: Display>(_msg: T) -> Self {
        Self::Custom
    }
}

/// Decoding failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DecodeError {
    /// The bytes end before the value does.
    #[error("the bytes end before the value does")]
    UnexpectedEnd,
    /// Bytes are left after the value.
    #[error("bytes are left after the value")]
    TrailingBytes,
    /// A `bool` word that is neither 0 nor 1.
    #[error("a bool that is neither 0 nor 1")]
    BadBool,
    /// The type has a part VP space has no layout for, such as an
    /// `Option`, a string or a sequence without a length.
    #[error("the type has no layout in VP space")]
    Unsupported,
    /// The type's `Deserialize` rejected the bytes, such as an enum given
    /// an index it has no variant for.
    #[error("the bytes are no valid value of the type")]
    Invalid,
}

impl serde::de::Error for DecodeError {
    fn custom<T: Display>(_msg: T) -> Self {
        Self::Invalid
    }
}
