//! Links to hosts over TCP, or a serial port with the `serial` feature,
//! bridged onto the display's UART: what the hosts send goes into it, and
//! what the display sends goes to all of them. Each link has threads of its
//! own, so that the display never waits for a host, nor a host for the
//! window.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use degus_sim_gui::Uart;

use crate::log::{LogKind, LogLine};

/// How many bytes a link keeps for a host that doesn't read them.
const OUTBOX_BYTES: usize = 1 << 20;
/// How often a listener looks for a host.
const ACCEPT_POLL: Duration = Duration::from_millis(10);
/// How often a TCP link looks whether to stop while the host is quiet.
const READ_POLL: Duration = Duration::from_millis(50);
/// How long a TCP host may leave the display's bytes unread once the
/// socket's buffers are full. Then it is disconnected.
const WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// How long a serial read waits for bytes. On Windows an answer can't be
/// written before the read returns.
#[cfg(feature = "serial")]
const SERIAL_POLL: Duration = Duration::from_millis(5);
/// How long a serial write may take: the longest frame takes 270 ms at 9600
/// baud.
#[cfg(feature = "serial")]
const SERIAL_WRITE: Duration = Duration::from_secs(1);

/// Called from a link's thread when it tells the log something, to wake the
/// window.
pub(crate) type Waker = Arc<dyn Fn() + Send + Sync>;

/// Where links tell the log what happens.
#[derive(Clone)]
pub(crate) struct Notices {
    lines: Sender<LogLine>,
    wake: Waker,
}

impl Notices {
    pub fn new(lines: Sender<LogLine>, wake: Waker) -> Self {
        Self { lines, wake }
    }

    fn tell(&self, kind: LogKind, text: impl Into<String>) {
        // Fails once the window is gone, when no one is left to tell.
        let _ = self.lines.send(LogLine::now(kind, text));
        (self.wake)();
    }
}

/// The hosts on the display's UART.
pub(crate) struct Bridge {
    uart: Uart,
    links: Vec<Link>,
    notices: Notices,
}

impl Bridge {
    pub fn new(uart: Uart, notices: Notices) -> Self {
        Self {
            uart,
            links: Vec::new(),
            notices,
        }
    }

    /// Accepts a host on `addr`, such as `127.0.0.1:5000`, one at a time.
    /// Returns the link's name.
    pub fn tcp(&mut self, addr: &str) -> io::Result<&str> {
        let (link, _) = Link::tcp(addr, self.uart.try_clone()?, self.notices.clone())?;
        self.links.push(link);
        Ok(self.links.last().unwrap().name())
    }

    /// Links a host on serial port `port`, such as `COM5` or
    /// `/dev/ttyUSB0`. Returns the link's name.
    #[cfg(feature = "serial")]
    pub fn serial(&mut self, port: &str, baud: u32) -> io::Result<&str> {
        let link = Link::serial(port, baud, self.uart.try_clone()?, self.notices.clone())?;
        self.links.push(link);
        Ok(self.links.last().unwrap().name())
    }

    /// Passes what the display sends on to the hosts, on a thread of its
    /// own, until the display is gone. Then the links close.
    pub fn start(self) -> io::Result<()> {
        let Self {
            mut uart,
            links,
            notices,
        } = self;
        thread::Builder::new()
            .name("degus sim hosts".into())
            .spawn(move || {
                let mut buf = [0u8; 1024];
                while let Ok(n @ 1..) = uart.read(&mut buf) {
                    for link in &links {
                        if let Err(error) = link.send(&buf[..n]) {
                            notices.tell(LogKind::Error, format!("{}: {error}", link.name()));
                        }
                    }
                }
            })?;
        Ok(())
    }
}

/// The display's bytes on their way to a host.
#[derive(Default)]
struct Outbox {
    state: Mutex<OutboxState>,
    changed: Condvar,
}

#[derive(Default)]
struct OutboxState {
    bytes: VecDeque<u8>,
    /// A host is connected.
    open: bool,
    /// Bytes were dropped, and the host has read none since.
    full: bool,
}

impl Outbox {
    fn lock(&self) -> MutexGuard<'_, OutboxState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A host connected.
    fn open(&self) {
        *self.lock() = OutboxState {
            open: true,
            ..OutboxState::default()
        };
    }

    /// No more bytes come: the host gets what is left, then the end.
    fn close(&self) {
        self.lock().open = false;
        self.changed.notify_all();
    }

    fn is_open(&self) -> bool {
        self.lock().open
    }

    /// Adds `bytes` if a host is connected, without blocking. When the host
    /// has left too much unread, `bytes` are dropped; the first time, that
    /// is an error.
    fn push(&self, bytes: &[u8]) -> Result<(), &'static str> {
        let mut state = self.lock();
        if !state.open {
            return Ok(());
        }
        if state.bytes.len() + bytes.len() > OUTBOX_BYTES {
            return match std::mem::replace(&mut state.full, true) {
                false => Err("the host doesn't read, so what the display sends is dropped"),
                true => Ok(()),
            };
        }
        state.bytes.extend(bytes);
        self.changed.notify_one();
        Ok(())
    }

    /// Waits for bytes; 0 is the end.
    fn read(&self, buf: &mut [u8]) -> usize {
        let mut state = self.lock();
        while state.bytes.is_empty() && state.open {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        state.take(buf)
    }

    /// The bytes there are within `timeout`.
    #[cfg(feature = "serial")]
    fn read_timeout(&self, buf: &mut [u8], timeout: Duration) -> usize {
        let mut state = self.lock();
        if state.bytes.is_empty() {
            state = self
                .changed
                .wait_timeout(state, timeout)
                .map_or_else(|poisoned| poisoned.into_inner().0, |(state, _)| state);
        }
        state.take(buf)
    }
}

impl OutboxState {
    fn take(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.bytes.len());
        for (to, from) in buf.iter_mut().zip(self.bytes.drain(..n)) {
            *to = from;
        }
        if n > 0 {
            self.full = false;
        }
        n
    }
}

/// A link to a host. Dropping it closes it and ends its thread.
struct Link {
    name: String,
    outbox: Arc<Outbox>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// What a link's thread works with.
struct Serving<'a> {
    /// The link's name.
    name: &'a str,
    outbox: &'a Outbox,
    stop: &'a AtomicBool,
    /// Where the host's bytes go.
    uart: Uart,
    notices: Notices,
}

impl Serving<'_> {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

impl Link {
    /// Listens on `addr`, serving one host at a time. Gives the address it
    /// got, too.
    fn tcp(
        addr: impl ToSocketAddrs,
        uart: Uart,
        notices: Notices,
    ) -> io::Result<(Self, SocketAddr)> {
        let listener = TcpListener::bind(addr)?;
        // So that the thread can end while it waits for a host.
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let link = Self::spawn(format!("tcp {addr}"), uart, notices, move |serving| {
            serve_tcp(&listener, serving);
        })?;
        Ok((link, addr))
    }

    /// Opens serial port `port`.
    #[cfg(feature = "serial")]
    fn serial(port: &str, baud: u32, uart: Uart, notices: Notices) -> io::Result<Self> {
        let serial = serialport::new(port, baud).timeout(SERIAL_POLL).open()?;
        let name = format!("serial {port} at {baud} baud");
        Self::spawn(name, uart, notices, move |mut serving| {
            serving.outbox.open();
            serving
                .notices
                .tell(LogKind::Event, format!("host connected: {}", serving.name));
            let served = serve_serial(serial, &mut serving);
            serving.outbox.close();
            if let Err(error) = served {
                let text = format!("host disconnected: {error}");
                serving.notices.tell(LogKind::Event, text);
            }
        })
    }

    fn spawn(
        name: String,
        uart: Uart,
        notices: Notices,
        serve: impl FnOnce(Serving<'_>) + Send + 'static,
    ) -> io::Result<Self> {
        let outbox = Arc::new(Outbox::default());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (name, outbox, stop) = (name.clone(), outbox.clone(), stop.clone());
            thread::Builder::new()
                .name(format!("degus sim {name}"))
                .spawn(move || {
                    serve(Serving {
                        name: &name,
                        outbox: &outbox,
                        stop: &stop,
                        uart,
                        notices,
                    });
                })?
        };
        Ok(Self {
            name,
            outbox,
            stop,
            thread: Some(thread),
        })
    }

    /// Such as `tcp 127.0.0.1:5000`.
    fn name(&self) -> &str {
        &self.name
    }

    /// Sends `bytes` to the host, if one is connected, without waiting for
    /// it to take them.
    fn send(&self, bytes: &[u8]) -> Result<(), &'static str> {
        self.outbox.push(bytes)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.outbox.close();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn serve_tcp(listener: &TcpListener, mut serving: Serving<'_>) {
    while !serving.stopped() {
        match listener.accept() {
            Ok((host, peer)) => {
                if let Err(error) = serve_host(host, &peer.to_string(), &mut serving) {
                    serving.notices.tell(LogKind::Error, error.to_string());
                }
            }
            Err(error) => {
                if error.kind() != io::ErrorKind::WouldBlock {
                    serving.notices.tell(LogKind::Error, error.to_string());
                }
                thread::park_timeout(ACCEPT_POLL);
            }
        }
    }
}

/// Serves the host on `stream`, called `peer`, until it disconnects.
fn serve_host(mut stream: TcpStream, peer: &str, serving: &mut Serving<'_>) -> io::Result<()> {
    // On Windows it takes after the listener.
    stream.set_nonblocking(false)?;
    let _ = stream.set_nodelay(true);
    // Neither shutting the socket down nor anything else ends a blocked
    // read or write on Windows, so both give up after a while.
    stream.set_read_timeout(Some(READ_POLL))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let (name, outbox, stop) = (serving.name, serving.outbox, serving.stop);
    let stopped = || stop.load(Ordering::SeqCst);
    let (uart, notices) = (&mut serving.uart, &serving.notices);
    outbox.open();
    notices.tell(LogKind::Event, format!("host connected: {peer}"));

    let reason = thread::scope(|scope| {
        // Writing blocks while the host doesn't read, so it has a thread of
        // its own.
        scope.spawn(|| {
            let mut buf = [0u8; 1024];
            loop {
                let n = outbox.read(&mut buf);
                if n == 0 {
                    break;
                }
                if let Err(error) = writer.write_all(&buf[..n]) {
                    if !stopped() {
                        notices.tell(LogKind::Error, format!("{name}: {error}"));
                    }
                    // Unless the host is gone, it doesn't read: let it go.
                    outbox.close();
                    break;
                }
            }
        });
        let mut buf = [0u8; 1024];
        let reason = loop {
            if stopped() {
                break "degus sim stopped".to_owned();
            }
            match stream.read(&mut buf) {
                Ok(0) => break "closed".to_owned(),
                Ok(n) => {
                    if uart.write_all(&buf[..n]).is_err() {
                        break "the display is gone".to_owned();
                    }
                }
                Err(error) if is_timeout(&error) => {
                    if !outbox.is_open() {
                        break "what the display sent could not be written".to_owned();
                    }
                }
                Err(error) => break error.to_string(),
            }
        };
        outbox.close();
        let _ = stream.shutdown(Shutdown::Both);
        reason
    });
    notices.tell(
        LogKind::Event,
        format!("host disconnected: {peer}: {reason}"),
    );
    Ok(())
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

/// Serves the host on `port` until stopped, or until reading fails.
#[cfg(feature = "serial")]
fn serve_serial(
    mut port: Box<dyn serialport::SerialPort>,
    serving: &mut Serving<'_>,
) -> io::Result<()> {
    let mut buf = [0u8; 1024];
    while !serving.stopped() {
        let wait = match port.read(&mut buf) {
            Ok(0) => Duration::ZERO,
            Ok(n) => {
                if serving.uart.write_all(&buf[..n]).is_err() {
                    return Ok(());
                }
                // The answer, if there is one, comes at once.
                SERIAL_POLL
            }
            Err(error) if is_timeout(&error) => Duration::ZERO,
            Err(error) => return Err(error),
        };
        let n = serving.outbox.read_timeout(&mut buf, wait);
        if n > 0 {
            port.set_timeout(SERIAL_WRITE)?;
            if let Err(error) = port.write_all(&buf[..n]) {
                let text = format!("{}: {error}", serving.name);
                serving.notices.tell(LogKind::Error, text);
            }
            port.set_timeout(SERIAL_POLL)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Instant;

    use degus_set::{Device, DwinSet, Size};
    use degus_sim::Sim;
    use degus_sim_gui::SimThread;

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);
    const WRITE: [u8; 8] = [0x5A, 0xA5, 0x05, 0x82, 0x10, 0x00, 0x00, 0x07];
    const ACK: [u8; 6] = [0x5A, 0xA5, 0x03, 0x82, b'O', b'K'];

    fn display() -> SimThread {
        SimThread::new(Sim::new(DwinSet::new(Device::new(Size::new(8, 8)))))
    }

    fn notices() -> (Notices, Receiver<LogLine>) {
        let (lines, received) = channel();
        (Notices::new(lines, Arc::new(|| {})), received)
    }

    /// Waits for a line starting with `start`.
    #[track_caller]
    fn told(lines: &Receiver<LogLine>, start: &str) -> LogLine {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match lines.recv_timeout(left) {
                Ok(line) if line.text.starts_with(start) => return line,
                Ok(_) => {}
                Err(_) => panic!("no line {start:?} within {WAIT:?}"),
            }
        }
    }

    #[test]
    fn bridges_one_tcp_host_after_the_other_to_the_display() {
        let sim = display();
        let (notices, lines) = notices();
        let mut bridge = Bridge::new(sim.uart(), notices);
        let name = bridge.tcp("127.0.0.1:0").unwrap().to_owned();
        let addr = name.strip_prefix("tcp ").unwrap().to_owned();
        bridge.start().unwrap();
        for _ in 0..2 {
            let mut host = TcpStream::connect(&addr).unwrap();
            host.set_read_timeout(Some(WAIT)).unwrap();
            told(&lines, "host connected: 127.0.0.1:");
            host.write_all(&WRITE).unwrap();
            let mut ack = [0u8; 6];
            host.read_exact(&mut ack).unwrap();
            assert_eq!(ack, ACK);
            drop(host);
            told(&lines, "host disconnected: 127.0.0.1:");
        }
        assert_eq!(sim.lock().vp().get(0x1000), 7);
    }

    #[test]
    fn the_links_close_once_the_display_is_gone() {
        let sim = display();
        let (notices, lines) = notices();
        let mut bridge = Bridge::new(sim.uart(), notices);
        let name = bridge.tcp("127.0.0.1:0").unwrap().to_owned();
        let addr = name.strip_prefix("tcp ").unwrap().to_owned();
        bridge.start().unwrap();
        let mut host = TcpStream::connect(&addr).unwrap();
        host.set_read_timeout(Some(WAIT)).unwrap();
        told(&lines, "host connected");
        drop(sim);
        told(&lines, "host disconnected");
        assert_eq!(host.read(&mut [0; 8]).unwrap(), 0);
        // The port is free again.
        TcpListener::bind(&addr).unwrap();
    }

    #[test]
    fn closing_a_tcp_link_disconnects_a_host_that_does_not_read() {
        let sim = display();
        let (notices, lines) = notices();
        let (link, addr) = Link::tcp("127.0.0.1:0", sim.uart(), notices).unwrap();
        let host = TcpStream::connect(addr).unwrap();
        told(&lines, "host connected");
        // More than the sockets hold, so that writing blocks.
        let fell_behind = (0..1000).any(|_| {
            thread::sleep(Duration::from_millis(1));
            link.send(&[0; 1 << 16]).is_err()
        });
        assert!(fell_behind);

        drop(link);
        told(&lines, "host disconnected");
        drop(host);
        TcpListener::bind(addr).unwrap();
    }
}
