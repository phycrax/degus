//! Links to the host: a TCP listener, or a serial port with the `serial`
//! feature. A background thread reads and decodes; whoever owns the
//! [`Sim`](crate::Sim) polls the link, feeds it the commands and sends the
//! replies back.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::host::{Codec, Decoded, DeviceMsg, HostCmd};

/// Called from the link's thread whenever something arrives, such as to
/// wake a window up.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a link reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    Connected(String),
    Disconnected(String),
    Cmd(HostCmd),
    /// Input that could not be decoded, or an I/O problem.
    Error(String),
}

struct Shared {
    codec: Mutex<Box<dyn Codec>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
}

impl Shared {
    fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        match writer.as_mut() {
            Some(writer) => writer.write_all(bytes).and_then(|()| writer.flush()),
            None => Ok(()),
        }
    }

    /// Decodes `bytes` into events, answering decode errors when the codec
    /// has a way to.
    fn decode(&self, bytes: &[u8], events: &Sender<LinkEvent>) {
        let mut decoded = Vec::new();
        let mut answer = Vec::new();
        {
            let mut codec = self.codec.lock().unwrap();
            codec.decode(bytes, &mut decoded);
            for item in &decoded {
                if let Decoded::Error(error) = item {
                    codec.encode_error(error, &mut answer);
                }
            }
        }
        if !answer.is_empty() {
            let _ = self.write(&answer);
        }
        for item in decoded {
            let _ = events.send(match item {
                Decoded::Cmd(cmd) => LinkEvent::Cmd(cmd),
                Decoded::Error(error) => LinkEvent::Error(error),
            });
        }
    }
}

/// A link to the host.
pub struct Link {
    name: String,
    events: Receiver<LinkEvent>,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Link {
    /// Listens on `addr`, such as `127.0.0.1:5000`, serving one host at a
    /// time.
    pub fn tcp(addr: &str, codec: Box<dyn Codec>, wake: Waker) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let name = format!("tcp {}", listener.local_addr()?);
        let (sender, events) = channel();
        let shared = Arc::new(Shared {
            codec: Mutex::new(codec),
            writer: Mutex::new(None),
        });
        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("degus-sim tcp".into())
            .spawn(move || serve_tcp(&listener, &thread_shared, &sender, &wake))?;
        Ok(Self {
            name,
            events,
            shared,
        })
    }

    /// Opens serial port `port`, such as `COM5` or `/dev/ttyUSB0`.
    #[cfg(feature = "serial")]
    pub fn serial(
        port: &str,
        baud: u32,
        codec: Box<dyn Codec>,
        wake: Waker,
    ) -> Result<Self, serialport::Error> {
        let mut reader = serialport::new(port, baud)
            .timeout(std::time::Duration::from_millis(50))
            .open()?;
        let writer = reader.try_clone()?;
        let name = format!("serial {port} at {baud} baud");
        let (sender, events) = channel();
        let shared = Arc::new(Shared {
            codec: Mutex::new(codec),
            writer: Mutex::new(Some(Box::new(writer))),
        });
        let thread_shared = shared.clone();
        let _ = sender.send(LinkEvent::Connected(name.clone()));
        std::thread::Builder::new()
            .name("degus-sim serial".into())
            .spawn(move || {
                let mut buf = [0u8; 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {}
                        Ok(n) => {
                            thread_shared.decode(&buf[..n], &sender);
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
            shared,
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

    /// Sends `msg` to the host, if one is connected.
    pub fn send(&self, msg: &DeviceMsg) -> std::io::Result<()> {
        let mut bytes = Vec::new();
        self.shared.codec.lock().unwrap().encode(msg, &mut bytes);
        self.shared.write(&bytes)
    }
}

fn serve_tcp(listener: &TcpListener, shared: &Shared, events: &Sender<LinkEvent>, wake: &Waker) {
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
        let Ok(writer) = stream.try_clone() else {
            continue;
        };
        let _ = stream.set_nodelay(true);
        *shared.writer.lock().unwrap() = Some(Box::new(writer));
        shared.codec.lock().unwrap().reset();
        let _ = events.send(LinkEvent::Connected(peer.clone()));
        wake();

        let mut buf = [0u8; 1024];
        let reason = loop {
            match stream.read(&mut buf) {
                Ok(0) => break "closed".to_owned(),
                Ok(n) => {
                    shared.decode(&buf[..n], events);
                    wake();
                }
                Err(e) => break e.to_string(),
            }
        };
        *shared.writer.lock().unwrap() = None;
        let _ = events.send(LinkEvent::Disconnected(format!("{peer}: {reason}")));
        wake();
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::line::LineCodec;

    #[test]
    fn serves_a_tcp_host() {
        let link = Link::tcp("127.0.0.1:0", Box::new(LineCodec::new()), Arc::new(|| {})).unwrap();
        let addr = link.name().strip_prefix("tcp ").unwrap().to_owned();
        let mut host = TcpStream::connect(&addr).unwrap();
        host.write_all(b"write 0x1000 7\nbogus\n").unwrap();

        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while events.len() < 3 && Instant::now() < deadline {
            events.extend(link.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(events[0], LinkEvent::Connected(_)));
        assert_eq!(
            events[1],
            LinkEvent::Cmd(HostCmd::WriteVp {
                vp: 0x1000,
                data: vec![0, 7]
            })
        );
        assert!(matches!(&events[2], LinkEvent::Error(e) if e.contains("bogus")));

        link.send(&DeviceMsg::Ack).unwrap();
        let mut lines = BufReader::new(host).lines();
        assert!(lines.next().unwrap().unwrap().starts_with("error "));
        assert_eq!(lines.next().unwrap().unwrap(), "ack");
    }
}
