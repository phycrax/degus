//! A serial port, picked at the prompt, as the async UART halves the driver
//! takes.

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use serialport::SerialPort;
use static_cell::StaticCell;

type Port = Mutex<NoopRawMutex, Box<dyn SerialPort>>;

pub fn new() -> (UartTx, UartRx) {
    static PORT: StaticCell<Port> = StaticCell::new();

    let port = prompt_serial_port().expect("no port picked");
    let port = serialport::new(&port, 115_200)
        .open()
        .unwrap_or_else(|e| panic!("failed to open port {port}: {e}"));
    let port = PORT.init(Mutex::new(port));

    (UartTx { port }, UartRx { port })
}

fn prompt_serial_port() -> Option<String> {
    let ports = serialport::available_ports().unwrap();

    println!("Choose port:");
    for (idx, port) in ports.iter().enumerate() {
        println!("{idx}: {port:?}");
    }

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .expect("failed to read input");

    let idx: usize = input.trim().parse().ok()?;
    ports.get(idx).map(|port| port.port_name.clone())
}

pub struct UartTx {
    port: &'static Port,
}

pub struct UartRx {
    port: &'static Port,
}

impl embedded_io_async::ErrorType for UartTx {
    type Error = std::io::Error;
}

impl embedded_io_async::Write for UartTx {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.port.lock().await.write_all(buf)?;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.port.lock().await.flush()
    }
}

impl embedded_io_async::ErrorType for UartRx {
    type Error = std::io::Error;
}

impl embedded_io_async::Read for UartRx {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        loop {
            // Nothing yet, or the port's read timed out: let the other
            // tasks run.
            let result = self.port.lock().await.read(buf);
            match result {
                Ok(0) | Err(_) => embassy_futures::yield_now().await,
                Ok(n) => break Ok(n),
            }
        }
    }
}
