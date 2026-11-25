//! Links to the host: a TCP listener, or a serial port with the `serial`
//! feature. A thread reads each; the window polls the link for the bytes,
//! feeds them to the simulator and sends its answers back.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// Called from a link's thread whenever something arrives, to wake the
/// window.
pub(crate) type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a link reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkEvent {
    Connected(String),
    Disconnected(String),
    /// Bytes from the host.
    Bytes(Vec<u8>),
    Error(String),
}

type Writer = Arc<Mutex<Option<Box<dyn Write + Send>>>>;

/// A link to the host.
pub(crate) struct Link {
    name: String,
    events: Receiver<LinkEvent>,
    writer: Writer,
}

impl Link {
    /// Listens on `addr`, such as `127.0.0.1:5000`, serving one host at a
    /// time.
    pub fn tcp(addr: &str, wake: Waker) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let name = format!("tcp {}", listener.local_addr()?);
        let (sender, events) = channel();
        let writer: Writer = Arc::default();
        let thread_writer = writer.clone();
        std::thread::Builder::new()
            .name("degus sim tcp".into())
            .spawn(move || serve_tcp(&listener, &thread_writer, &sender, &wake))?;
        Ok(Self {
            name,
            events,
            writer,
        })
    }

    /// Opens serial port `port`, such as `COM5` or `/dev/ttyUSB0`.
    #[cfg(feature = "serial")]
    pub fn serial(port: &str, baud: u32, wake: Waker) -> Result<Self, serialport::Error> {
        let mut reader = serialport::new(port, baud)
            .timeout(std::time::Duration::from_millis(50))
            .open()?;
        let name = format!("serial {port} at {baud} baud");
        let writer: Writer = Arc::new(Mutex::new(Some(Box::new(reader.try_clone()?))));
        let (sender, events) = channel();
        let _ = sender.send(LinkEvent::Connected(name.clone()));
        std::thread::Builder::new()
            .name("degus sim serial".into())
            .spawn(move || {
                let mut buf = [0u8; 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {}
                        Ok(n) => {
                            let _ = sender.send(LinkEvent::Bytes(buf[..n].to_vec()));
                            wake();
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) => {
                            let _ = sender.send(LinkEvent::Disconnected(e.to_string()));
                            wake();
                            return;
                        }
                    }
                }
            })
            .map_err(|e| {
                serialport::Error::new(serialport::ErrorKind::Io(e.kind()), e.to_string())
            })?;
        Ok(Self {
            name,
            events,
            writer,
        })
    }

    /// Such as `tcp 127.0.0.1:5000`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What arrived since the last call.
    pub fn poll(&self) -> Vec<LinkEvent> {
        self.events.try_iter().collect()
    }

    /// Sends `bytes` to the host, if one is connected.
    pub fn send(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        match writer.as_mut() {
            Some(writer) => writer.write_all(bytes).and_then(|()| writer.flush()),
            None => Ok(()),
        }
    }
}

fn serve_tcp(listener: &TcpListener, writer: &Writer, events: &Sender<LinkEvent>, wake: &Waker) {
    for stream in listener.incoming() {
        let mut stream: TcpStream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                let _ = events.send(LinkEvent::Error(e.to_string()));
                continue;
            }
        };
        let peer = stream
            .peer_addr()
            .map_or_else(|_| "?".to_owned(), |a| a.to_string());
        let Ok(clone) = stream.try_clone() else {
            continue;
        };
        let _ = stream.set_nodelay(true);
        *writer.lock().unwrap() = Some(Box::new(clone));
        let _ = events.send(LinkEvent::Connected(peer.clone()));
        wake();

        let mut buf = [0u8; 1024];
        let reason = loop {
            match stream.read(&mut buf) {
                Ok(0) => break "closed".to_owned(),
                Ok(n) => {
                    let _ = events.send(LinkEvent::Bytes(buf[..n].to_vec()));
                    wake();
                }
                Err(e) => break e.to_string(),
            }
        };
        *writer.lock().unwrap() = None;
        let _ = events.send(LinkEvent::Disconnected(format!("{peer}: {reason}")));
        wake();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// The first `count` events, waiting up to 5 s for them.
    fn events(link: &Link, count: usize) -> Vec<LinkEvent> {
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while events.len() < count && Instant::now() < deadline {
            events.extend(link.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        events
    }

    #[test]
    fn serves_a_tcp_host() {
        let link = Link::tcp("127.0.0.1:0", Arc::new(|| {})).unwrap();
        let addr = link.name().strip_prefix("tcp ").unwrap().to_owned();
        let mut host = TcpStream::connect(&addr).unwrap();
        host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        host.write_all(&[0x5A, 0xA5, 0x05]).unwrap();

        let events = events(&link, 2);
        assert!(matches!(events[0], LinkEvent::Connected(_)));
        assert_eq!(events[1], LinkEvent::Bytes(vec![0x5A, 0xA5, 0x05]));

        link.send(&[0x5A, 0xA5, 0x03, 0x82, b'O', b'K']).unwrap();
        let mut ack = [0u8; 6];
        host.read_exact(&mut ack).unwrap();
        assert_eq!(ack, [0x5A, 0xA5, 0x03, 0x82, b'O', b'K']);
    }
}
