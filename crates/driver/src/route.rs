//! What the display sends without being asked, and what the runner drops.

use degus_codec::{Address, DecodeError, Space};
use degus_serde::Deserializer;
use serde::Deserialize;

/// A data frame the display sent without being asked: with auto-upload on,
/// a VP a touch control changed.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Upload<'a> {
    addr: Address,
    len: u8,
    data: Deserializer<'a>,
}

impl<'a> Upload<'a> {
    /// An upload of `data` from `addr`, which the frame says is `len` units
    /// long, as for testing a [`Router`].
    #[must_use]
    pub const fn new(addr: Address, len: u8, data: &'a [u8]) -> Self {
        Self {
            addr,
            len,
            data: Deserializer::new(data),
        }
    }

    /// Where the data is from.
    #[must_use]
    pub const fn addr(&self) -> Address {
        self.addr
    }

    /// How many units the frame says it carries: words, for VPs.
    #[must_use]
    #[expect(clippy::len_without_is_empty, reason = "the frame's length byte")]
    pub const fn len(&self) -> u8 {
        self.len
    }

    /// Takes a `T` off the front of the data not taken yet.
    ///
    /// # Errors
    ///
    /// [`RouteError::Decode`], with the upload's address. The data left is
    /// as it was then.
    pub fn take<T: Deserialize<'a>>(&mut self) -> Result<T, RouteError> {
        self.data
            .take()
            .map_err(|error| RouteError::Decode(self.addr, error))
    }

    /// The data not taken yet.
    #[must_use]
    pub const fn rest(&self) -> &'a [u8] {
        self.data.rest()
    }
}

/// Why a [`Router`] did not route an upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum RouteError {
    /// No route takes an upload from this address of this length.
    #[error("no route for {1} units from {0:?}")]
    Unhandled(Address, u8),
    /// The upload from this address failed to decode.
    #[error("the upload from {0:?} failed to decode: {1}")]
    Decode(Address, degus_serde::DecodeError),
}

/// Takes the uploads off the [`Runner`](crate::Runner).
///
/// Closures taking an [`Upload`] are routers too.
pub trait Router {
    /// Routes one upload.
    ///
    /// # Errors
    ///
    /// The [`RouteError`] the runner passes on to [`fault`](Self::fault).
    fn route(&mut self, upload: Upload<'_>) -> Result<(), RouteError>;

    /// Hears of what the runner dropped and carried on after. Does nothing
    /// unless implemented.
    fn fault(&mut self, fault: Fault) {
        let _ = fault;
    }
}

impl<F: FnMut(Upload<'_>) -> Result<(), RouteError>> Router for F {
    fn route(&mut self, upload: Upload<'_>) -> Result<(), RouteError> {
        self(upload)
    }
}

/// Something the [`Runner`](crate::Runner) dropped before carrying on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum Fault {
    /// Bytes that are no frame, or a frame that broke off, failed its CRC
    /// or has a command no display sends.
    #[error("dropped a bad frame: {0}")]
    Frame(DecodeError),
    /// An acknowledgement from this space, which no write waits for.
    #[error("an ack from {0:?} that no write waits for")]
    UnmatchedAck(Space),
    /// The reply to a read of `len` units from this address, which was
    /// cancelled before the reply came.
    #[error("the late reply to a cancelled read of {1} units from {0:?}")]
    LateReply(Address, u8),
    /// An upload whose data is not as long as its length says.
    #[error("an upload from {0:?} not {1} units long")]
    BadLength(Address, u8),
    /// The router did not route an upload.
    #[error("{0}")]
    Route(RouteError),
}
