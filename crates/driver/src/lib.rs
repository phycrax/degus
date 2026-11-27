//! Talks to a DWIN T5L display over its UART from async firmware: writes
//! and reads VPs, and hands on what the display sends without being asked.
//!
//! [`new`] splits the link in two, over the buffers in [`Resources`]:
//!
//! - a [`Display`], which writes values and reads them back, encoding and
//!   decoding them with [`degus_serde`], and waits for the display's
//!   answer;
//! - a [`Runner`], which reads the UART, matches each answer to the request
//!   waiting for it and passes the rest to a [`Router`]: uploads, such as a
//!   VP a touch control changed with auto-upload on, and faults it dropped.
//!
//! The runner must be running for the display's requests to be answered:
//! give it a task of its own, or poll it alongside them. A request made
//! before it starts waits for it. Only the transport failing stops it, and
//! requests then fail with [`Error::NotRunning`].
//!
//! One request is in flight at a time. A request can be dropped while it
//! waits, as when a timeout such as `embassy_time::with_timeout` cancels
//! it: the next request does not take its answer, and the runner drops the
//! late reply to a cancelled read with [`Fault::LateReply`].
//!
//! ```
//! use degus_driver::{Address, Config, Resources, RouteError, Upload};
//! use embassy_futures::select::select;
//! use embassy_sync::blocking_mutex::raw::NoopRawMutex;
//! use embedded_io_async::{Read, Write};
//!
//! async fn talk(rx: impl Read, tx: impl Write) {
//!     let mut resources = Resources::<NoopRawMutex, 128, 128>::new();
//!     let (mut display, mut runner) = degus_driver::new(&mut resources, rx, tx, Config::new());
//!
//!     // The uploads: here, a speed at VP 0x1000.
//!     let mut router = |mut upload: Upload<'_>| match (upload.addr(), upload.len()) {
//!         (Address::Vp(0x1000), 1) => {
//!             let speed: u16 = upload.take()?;
//!             Ok(())
//!         }
//!         (addr, len) => Err(RouteError::Unhandled(addr, len)),
//!     };
//!
//!     let app = async {
//!         // Switch to page 1 (PIC_Set, 0x0084), then read the page shown
//!         // back (PIC_Now, 0x0014).
//!         display.write(Address::Vp(0x0084), &(0x5A01_u16, 1_u16)).await?;
//!         display.read::<u16>(Address::Vp(0x0014), 1).await
//!     };
//!     let mut rx_buf = [0; 16];
//!     select(runner.run(&mut rx_buf, &mut router), app).await;
//! }
//! ```

#![no_std]

mod route;

use core::cell::Cell;

use degus_codec::{Accumulator, Feed, Request, Response};
pub use degus_codec::{Address, Crc, Space};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::mutex::Mutex as AsyncMutex;
use embassy_sync::signal::Signal;
use embedded_io_async::{Error as _, ErrorKind, Read as IoRead, Write as IoWrite};
pub use route::{Fault, RouteError, Router, Upload};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Why a request failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum Error {
    /// The [`Runner`] has stopped, so nothing would take the answer. A
    /// request made before it first runs waits for it instead.
    #[error("the runner has stopped")]
    NotRunning,
    /// The request does not fit its frame or the request buffer, or reads
    /// the write-only curve buffer.
    #[error("the request does not fit: {0}")]
    Frame(degus_codec::EncodeError),
    /// The value has no layout in VP space.
    #[error("the value failed to encode: {0}")]
    Encode(degus_serde::EncodeError),
    /// The transport failed to write.
    #[error("the transport failed to write: {0:?}")]
    Io(ErrorKind),
    /// The answer from this address failed to decode.
    #[error("the answer from {0:?} failed to decode: {1}")]
    Decode(Address, degus_serde::DecodeError),
}

/// Why the [`Runner`] stopped: the transport failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum RunError {
    /// The transport's input ended.
    #[error("the transport's input ended")]
    UnexpectedEof,
    /// The transport failed to read.
    #[error("the transport failed to read: {0:?}")]
    Io(ErrorKind),
}

/// How the display is set up, passed to [`new`].
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// Frames carry a CRC trailer, as CFG `0x05` bit 7 sets.
    pub crc: bool,
}

impl Config {
    /// Frames carry a CRC trailer.
    #[must_use]
    pub const fn new() -> Self {
        Self { crc: true }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// The answer a request waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// An acknowledgement from this space.
    Ack(Space),
    /// `len` units of data from `addr`.
    Read { addr: Address, len: u8 },
}

#[derive(Clone, Copy)]
struct State {
    /// The request in flight and its sequence number.
    pending: Option<(Expect, u8)>,
    /// The last read cancelled while it waited, whose reply may still come.
    abandoned_read: Option<(Address, u8)>,
    /// The runner ran and returned, so nothing takes the answers until it
    /// runs again.
    stopped: bool,
}

struct Shared<M: RawMutex> {
    state: Mutex<M, Cell<State>>,
    /// `(seq, len)` of the answer the runner put in the reply buffer, `len`
    /// 0 for an acknowledgement.
    replied: Signal<M, (u8, usize)>,
}

impl<M: RawMutex> Shared<M> {
    const fn new() -> Self {
        Self {
            state: Mutex::new(Cell::new(State {
                pending: None,
                abandoned_read: None,
                stopped: false,
            })),
            replied: Signal::new(),
        }
    }

    /// Runs `f` on the state under the lock.
    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        self.state.lock(|cell| {
            let mut state = cell.get();
            let out = f(&mut state);
            cell.set(state);
            out
        })
    }
}

/// Marks the runner stopped once it returns or is dropped.
struct RunGuard<'d, M: RawMutex> {
    shared: &'d Shared<M>,
}

impl<'d, M: RawMutex> RunGuard<'d, M> {
    fn new(shared: &'d Shared<M>) -> Self {
        shared.with(|s| s.stopped = false);
        Self { shared }
    }
}

impl<M: RawMutex> Drop for RunGuard<'_, M> {
    fn drop(&mut self) {
        self.shared.with(|s| s.stopped = true);
    }
}

/// Marks a request in flight while it lives. A read dropped before its
/// answer came is remembered, so the runner can drop the late reply.
struct PendingGuard<'d, M: RawMutex> {
    shared: &'d Shared<M>,
}

impl<'d, M: RawMutex> PendingGuard<'d, M> {
    fn new(shared: &'d Shared<M>, expect: Expect, seq: u8) -> Self {
        shared.with(|s| s.pending = Some((expect, seq)));
        Self { shared }
    }
}

impl<M: RawMutex> Drop for PendingGuard<'_, M> {
    fn drop(&mut self) {
        self.shared.with(|s| {
            if let Some((Expect::Read { addr, len }, _)) = s.pending.take() {
                s.abandoned_read = Some((addr, len));
            }
        });
    }
}

/// The driver's buffers, passed to [`new`].
///
/// `REQ` bounds the frames sent: the longest write, frame and CRC
/// included. A longer one fails with [`Error::Frame`] or [`Error::Encode`].
/// [`MAX_FRAME`](degus_codec::MAX_FRAME) bytes hold any frame.
///
/// `RESP` bounds the frames received, answers and uploads alike, from the
/// command byte through the CRC. A longer one is dropped with
/// [`Fault::Frame`]. [`MAX_CONTENT`](degus_codec::MAX_CONTENT) bytes hold
/// any frame.
pub struct Resources<M: RawMutex, const REQ: usize, const RESP: usize> {
    req_buf: [u8; REQ],
    acc_buf: [u8; RESP],
    /// The data of a read's answer, part of a frame received, so it always
    /// fits.
    reply_buf: AsyncMutex<M, [u8; RESP]>,
    shared: Shared<M>,
}

impl<M: RawMutex, const REQ: usize, const RESP: usize> Resources<M, REQ, RESP> {
    /// Buffers for a new driver.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            req_buf: [0; REQ],
            acc_buf: [0; RESP],
            reply_buf: AsyncMutex::new([0; RESP]),
            shared: Shared::new(),
        }
    }
}

impl<M: RawMutex, const REQ: usize, const RESP: usize> Default for Resources<M, REQ, RESP> {
    fn default() -> Self {
        Self::new()
    }
}

/// Sends requests to the display and waits for its answers.
pub struct Display<'d, M: RawMutex, TX: IoWrite> {
    tx: TX,
    buf: &'d mut [u8],
    reply_buf: &'d AsyncMutex<M, [u8]>,
    shared: &'d Shared<M>,
    seq: u8,
    crc: Option<Crc>,
}

/// Reads the display's frames: answers for the [`Display`], the rest for
/// a [`Router`].
pub struct Runner<'d, M: RawMutex, RX: IoRead> {
    rx: RX,
    acc: Accumulator<&'d mut [u8]>,
    reply_buf: &'d AsyncMutex<M, [u8]>,
    shared: &'d Shared<M>,
}

/// Splits a link to the display, `rx` from it and `tx` to it, into a
/// [`Display`] and a [`Runner`] over `resources`.
pub fn new<M, RX, TX, const REQ: usize, const RESP: usize>(
    resources: &mut Resources<M, REQ, RESP>,
    rx: RX,
    tx: TX,
    config: Config,
) -> (Display<'_, M, TX>, Runner<'_, M, RX>)
where
    M: RawMutex,
    RX: IoRead,
    TX: IoWrite,
{
    let crc = if config.crc { Crc::on() } else { Crc::off() };

    let Resources {
        req_buf,
        acc_buf,
        reply_buf,
        shared,
    } = resources;
    // Nothing from a previous driver over these resources carries over.
    *shared = Shared::new();
    let (reply_buf, shared) = (&*reply_buf, &*shared);

    let display = Display {
        tx,
        buf: &mut req_buf[..],
        reply_buf,
        shared,
        seq: 0,
        crc,
    };
    let runner = Runner {
        rx,
        acc: Accumulator::new(&mut acc_buf[..], crc),
        reply_buf,
        shared,
    };
    (display, runner)
}

impl<'d, M: RawMutex, TX: IoWrite> Display<'d, M, TX> {
    fn begin_request(&mut self) -> u8 {
        // The answer to a request cancelled after the runner took it.
        self.shared.replied.reset();
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Sends the first `len` bytes of the request buffer and waits for the
    /// answer to them. Returns the length of the answer's data in the reply
    /// buffer, 0 for an acknowledgement.
    ///
    /// Not generic, so every request shares one copy of it. Only encoding
    /// the request and decoding the answer depend on the value's type.
    async fn transfer(&mut self, expect: Expect, len: usize) -> Result<usize, Error> {
        if self.shared.with(|s| s.stopped) {
            return Err(Error::NotRunning);
        }

        let seq = self.begin_request();
        let _pending = PendingGuard::new(self.shared, expect, seq);
        if let Err(e) = self.tx.write_all(&self.buf[..len]).await {
            return Err(Error::Io(e.kind()));
        }

        loop {
            let (replied, len) = self.shared.replied.wait().await;
            // Or the answer to a request cancelled once the runner had taken
            // it? Ours is still to come.
            if replied == seq {
                return Ok(len);
            }
        }
    }

    /// Encodes a write of `value` to `addr` into the request buffer.
    /// Nothing is sent until [`Staged::send`].
    ///
    /// # Errors
    ///
    /// [`Error::Frame`] if the write does not fit, and [`Error::Encode`] if
    /// `value` has no layout in VP space or does not fit.
    pub fn stage_write<T: Serialize + ?Sized>(
        &mut self,
        addr: Address,
        value: &T,
    ) -> Result<Staged<'_, 'd, M, TX>, Error> {
        self.stage(addr, &mut |room| {
            degus_serde::to_slice(value, room).map(|data| data.len())
        })
    }

    /// [`stage_write`](Self::stage_write) with `encode` writing the data into
    /// the room it is lent. Not generic, so every value shares one copy of
    /// the framing, and only serializing it depends on its type.
    fn stage(
        &mut self,
        addr: Address,
        encode: &mut dyn FnMut(&mut [u8]) -> Result<usize, degus_serde::EncodeError>,
    ) -> Result<Staged<'_, 'd, M, TX>, Error> {
        let mut writer = Request::begin_write(self.buf, addr, self.crc).map_err(Error::Frame)?;
        writer.fill(encode).map_err(Error::Encode)?;
        let len = writer.finish().len();
        Ok(Staged {
            display: self,
            len,
            expect: Expect::Ack(addr.space()),
        })
    }

    /// Writes `value` from `addr` on and waits for the acknowledgement.
    ///
    /// # Errors
    ///
    /// As [`stage_write`](Self::stage_write) and [`Staged::send`].
    pub async fn write<T: Serialize + ?Sized>(
        &mut self,
        addr: Address,
        value: &T,
    ) -> Result<(), Error> {
        self.stage_write(addr, value)?.send().await
    }

    /// Reads `len` units from `addr` on, words for VPs, and decodes them as
    /// a `T`.
    ///
    /// # Errors
    ///
    /// [`Error::NotRunning`], [`Error::Io`], [`Error::Frame`] for a read of
    /// the curve buffer, and [`Error::Decode`] if the answer is not a `T`,
    /// such as when `T` is shorter or longer than `len` units.
    pub async fn read<T: DeserializeOwned>(&mut self, addr: Address, len: u8) -> Result<T, Error> {
        let frame = Request::Read { addr, len }
            .encode(self.buf, self.crc)
            .map_err(Error::Frame)?
            .len();
        let data = self.transfer(Expect::Read { addr, len }, frame).await?;
        // The runner fills the reply buffer only for a request in flight, and
        // only this display makes them.
        let reply = self.reply_buf.lock().await;
        degus_serde::from_bytes(&reply[..data]).map_err(|e| Error::Decode(addr, e))
    }
}

/// A write encoded by [`Display::stage_write`], ready to send.
///
/// It holds the display, so nothing can be staged over it while it lives.
pub struct Staged<'a, 'd, M: RawMutex, TX: IoWrite> {
    display: &'a mut Display<'d, M, TX>,
    len: usize,
    expect: Expect,
}

impl<'a, 'd, M: RawMutex, TX: IoWrite> Staged<'a, 'd, M, TX> {
    /// Sends the write and waits for its acknowledgement.
    ///
    /// Each call sends it again, so a send that was cancelled, as by a
    /// timeout, can be retried without encoding anything.
    ///
    /// # Errors
    ///
    /// [`Error::NotRunning`] and [`Error::Io`].
    pub async fn send(&mut self) -> Result<(), Error> {
        self.display
            .transfer(self.expect, self.len)
            .await
            .map(|_| ())
    }

    /// The display back, to stage the next request once this one is sent.
    /// Staging over a write not sent yet replaces it.
    pub fn into_display(self) -> &'a mut Display<'d, M, TX> {
        self.display
    }
}

impl<M: RawMutex, RX: IoRead> Runner<'_, M, RX> {
    /// Reads the display's frames into `buf`, chunk by chunk, until the
    /// transport fails. Answers go to the [`Display`] request waiting for
    /// them, uploads to `router`, and faults, which the runner drops and
    /// carries on after, to [`Router::fault`].
    ///
    /// # Errors
    ///
    /// [`RunError`] once the transport fails; it never returns otherwise.
    pub async fn run<R: Router + ?Sized>(
        &mut self,
        buf: &mut [u8],
        router: &mut R,
    ) -> Result<!, RunError> {
        let _guard = RunGuard::new(self.shared);
        loop {
            let n = self
                .rx
                .read(buf)
                .await
                .map_err(|e| RunError::Io(e.kind()))?;
            if n == 0 {
                return Err(RunError::UnexpectedEof);
            }

            let mut input = &buf[..n];
            loop {
                input = match self.acc.feed(input) {
                    Feed::Consumed => break,
                    Feed::Error(error, rest) => {
                        router.fault(Fault::Frame(error));
                        rest
                    }
                    Feed::Frame(content, rest) => {
                        receive(self.shared, self.reply_buf, content, router).await;
                        rest
                    }
                };
            }
        }
    }
}

/// Hands one frame's content on: an answer to the request waiting for it,
/// the rest to `router`.
async fn receive<M: RawMutex, R: Router + ?Sized>(
    shared: &Shared<M>,
    reply_buf: &AsyncMutex<M, [u8]>,
    content: &[u8],
    router: &mut R,
) {
    let (addr, len, data) = match Response::from_content(content) {
        Err(error) => return router.fault(Fault::Frame(error)),
        Ok(Response::Ack(space)) => {
            let expect = Expect::Ack(space);
            return match shared.with(|s| s.pending.take_if(|(e, _)| *e == expect)) {
                Some((_, seq)) => shared.replied.signal((seq, 0)),
                None => router.fault(Fault::UnmatchedAck(space)),
            };
        }
        Ok(Response::Data { addr, len, data }) => (addr, len, data),
    };

    let expect = Expect::Read { addr, len };
    // Are we waiting for it? Then the read decodes it, and says if it is
    // not as long as asked.
    if let Some((_, seq)) = shared.with(|s| s.pending.take_if(|(e, _)| *e == expect)) {
        reply_buf.lock().await[..data.len()].copy_from_slice(data);
        shared.replied.signal((seq, data.len()));
    }
    // Or the late reply to a read nobody waits for any more?
    else if shared
        .with(|s| s.abandoned_read.take_if(|r| *r == (addr, len)))
        .is_some()
    {
        router.fault(Fault::LateReply(addr, len));
    }
    // Or an upload.
    else if data.len() != usize::from(len) * unit(addr) {
        router.fault(Fault::BadLength(addr, len));
    } else if let Err(error) = router.route(Upload::new(addr, len, data)) {
        router.fault(Fault::Route(error));
    }
}

/// Bytes per unit of a frame's length from `addr`.
fn unit(addr: Address) -> usize {
    match addr {
        Address::Register { .. } | Address::Curve(_) => 1,
        Address::Vp(_) => 2,
        Address::Dword(_) => 4,
    }
}
