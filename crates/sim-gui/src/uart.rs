//! The display's UART, as the host sees it.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use parking_lot::{Condvar, Mutex};

use crate::thread::Message;

/// How many bytes the UART keeps for a host that doesn't read them.
const OUTBOX_BYTES: usize = 1 << 20;

/// The display's bytes on their way to the host.
pub(crate) struct Outbox {
    state: Mutex<OutboxState>,
    changed: Condvar,
}

struct OutboxState {
    bytes: VecDeque<u8>,
    /// A [`Uart`] was handed out: before that, no host listens.
    attached: bool,
    /// The display is running.
    open: bool,
    /// Bytes were dropped, and the host has read none since.
    full: bool,
}

impl Outbox {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(OutboxState {
                bytes: VecDeque::new(),
                attached: false,
                open: true,
                full: false,
            }),
            changed: Condvar::new(),
        }
    }

    pub fn attach(&self) {
        self.state.lock().attached = true;
    }

    /// The display is gone: the host reads what is left, then the end.
    pub fn close(&self) {
        self.state.lock().open = false;
        self.changed.notify_all();
    }

    /// Adds `bytes` for the host, without blocking. When the host has left
    /// too much unread, `bytes` are dropped; returns `true` when that
    /// starts.
    pub fn push(&self, bytes: &[u8]) -> bool {
        let mut state = self.state.lock();
        if !state.attached {
            return false;
        }
        if state.bytes.len() + bytes.len() > OUTBOX_BYTES {
            return !std::mem::replace(&mut state.full, true);
        }
        state.bytes.extend(bytes);
        self.changed.notify_one();
        false
    }

    /// Waits for bytes; 0 is the end, as for [`Read::read`].
    fn read(&self, buf: &mut [u8]) -> usize {
        let mut state = self.state.lock();
        while state.bytes.is_empty() && state.open && !buf.is_empty() {
            self.changed.wait(&mut state);
        }
        let n = buf.len().min(state.bytes.len());
        for (to, from) in buf.iter_mut().zip(state.bytes.drain(..n)) {
            *to = from;
        }
        if n > 0 {
            state.full = false;
        }
        n
    }
}

/// The display's UART, from [`SimThread::uart`](crate::SimThread::uart):
/// the host reads what the display sends and writes what the display
/// receives, as on a serial port.
///
/// [`Read`] blocks until bytes come, and returns `Ok(0)` once the display is
/// gone. [`Write`] never blocks; once the display is gone, it fails with
/// [`BrokenPipe`](io::ErrorKind::BrokenPipe). Up to 1 MiB that the host
/// leaves unread is kept, and what comes after is dropped.
pub struct Uart(Arc<Ends>);

struct Ends {
    outbox: Arc<Outbox>,
    inbox: Sender<Message>,
}

impl Uart {
    pub(crate) fn new(outbox: Arc<Outbox>, inbox: Sender<Message>) -> Self {
        Self(Arc::new(Ends { outbox, inbox }))
    }

    /// Another handle to the UART, so that one thread can read and another
    /// write. It never fails.
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self(self.0.clone()))
    }
}

impl fmt::Debug for Uart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Uart").finish_non_exhaustive()
    }
}

impl Read for Uart {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(self.0.outbox.read(buf))
    }
}

impl Write for Uart {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match self.0.inbox.send(Message::Bytes(buf.to_vec())) {
            Ok(()) => Ok(buf.len()),
            Err(_) => Err(io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use super::*;

    fn uart() -> (Arc<Outbox>, Uart) {
        let outbox = Arc::new(Outbox::new());
        outbox.attach();
        let (inbox, _messages) = channel();
        (outbox.clone(), Uart::new(outbox, inbox))
    }

    #[test]
    fn drops_what_the_host_leaves_unread_and_says_so_once() {
        let (outbox, mut host) = uart();
        for _ in 0..OUTBOX_BYTES / 256 {
            assert!(!outbox.push(&[0; 256]));
        }
        assert!(outbox.push(&[0; 256]));
        assert!(!outbox.push(&[0; 256]));
        // Reading makes room again.
        host.read_exact(&mut [0; 512]).unwrap();
        assert!(!outbox.push(&[1; 256]));
        outbox.close();
        let mut rest = Vec::new();
        host.read_to_end(&mut rest).unwrap();
        assert_eq!(rest.len(), OUTBOX_BYTES - 256);
    }

    #[test]
    fn the_host_reads_what_is_left_and_then_the_end() {
        let (outbox, mut host) = uart();
        outbox.push(&[1, 2, 3]);
        outbox.close();
        let mut bytes = Vec::new();
        host.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3]);
        assert_eq!(host.read(&mut [0; 8]).unwrap(), 0);
    }

    #[test]
    fn nothing_is_kept_before_a_host_is_attached() {
        let outbox = Outbox::new();
        outbox.push(&[1, 2, 3]);
        outbox.attach();
        outbox.push(&[4]);
        outbox.close();
        assert_eq!(outbox.read(&mut [0; 8]), 1);
    }
}
