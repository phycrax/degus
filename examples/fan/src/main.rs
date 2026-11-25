//! A fan's display panel, driven from a PC: the panel sets the speed, steps
//! it up and down and turns oscillation on and off, and the PC keeps the
//! state and writes the speed back.
//!
//! Connect the display to a serial port at 115200 baud with CRC on, run
//! `cargo run -p fan` and pick the port.

mod uart;

use degus_driver::{Address, Config, Fault, Resources, RouteError, Router, Runner, Upload};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::signal::Signal;
use serde::{Deserialize, Serialize};
use static_cell::{ConstStaticCell, StaticCell};

use crate::uart::UartRx;

/// The longest frame sent to the display, in bytes.
const REQ_MTU: usize = 128;
/// The longest frame received from the display, in bytes.
const RESP_MTU: usize = 128;

/// The speed, set on the panel and written back at VP 0x1000 (one word).
const SPEED: Address = Address::Vp(0x1000);
/// Steps the speed up or down, at VP 0x1001 (one word).
const ADJUST: Address = Address::Vp(0x1001);
/// Oscillation on or off, at VP 0x1002 (one word).
const ROTATE: Address = Address::Vp(0x1002);

/// The application's state, changed by the event loop in [`main`].
#[derive(Default, Debug)]
struct FanApp {
    speed: Speed,
    /// Whether the fan head oscillates.
    rotating: bool,
}

/// The fan's speed: its index in a word, `Off = 0` to `High = 3`.
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy)]
enum Speed {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl Speed {
    /// The next speed up, `High` at most.
    fn increased(self) -> Self {
        match self {
            Speed::Off => Speed::Low,
            Speed::Low => Speed::Medium,
            Speed::Medium | Speed::High => Speed::High,
        }
    }

    /// The next speed down, `Off` at least.
    fn decreased(self) -> Self {
        match self {
            Speed::High => Speed::Medium,
            Speed::Medium => Speed::Low,
            Speed::Low | Speed::Off => Speed::Off,
        }
    }
}

/// A step of the speed: `Decrease = 0`, `Increase = 1`.
#[derive(Deserialize)]
enum Adjust {
    Decrease,
    Increase,
}

/// An upload, decoded by [`FanRouter`].
enum Event {
    Speed(Speed),
    Adjust(Adjust),
    Rotate(bool),
}

/// Decodes the display's uploads and signals them to [`main`].
struct FanRouter {
    events: &'static Signal<NoopRawMutex, Event>,
}

impl Router for FanRouter {
    fn route(&mut self, mut upload: Upload<'_>) -> Result<(), RouteError> {
        let event = match (upload.addr(), upload.len()) {
            (SPEED, 1) => Event::Speed(upload.take()?),
            (ADJUST, 1) => Event::Adjust(upload.take()?),
            // The `bool` decodes from a word, 0 off and 1 on.
            (ROTATE, 1) => Event::Rotate(upload.take()?),
            (addr, len) => return Err(RouteError::Unhandled(addr, len)),
        };
        self.events.signal(event);
        Ok(())
    }

    fn fault(&mut self, fault: Fault) {
        eprintln!("dropped: {fault}");
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (tx, rx) = uart::new();

    static RESOURCES: ConstStaticCell<Resources<NoopRawMutex, REQ_MTU, RESP_MTU>> =
        ConstStaticCell::new(Resources::new());
    let resources = RESOURCES.take();

    // Decoded uploads, from the runner's task to the event loop below.
    static EVENTS: StaticCell<Signal<NoopRawMutex, Event>> = StaticCell::new();
    let events: &'static Signal<NoopRawMutex, Event> = EVENTS.init(Signal::new());

    // The display takes the UART's transmit half, the runner its receive
    // half.
    let (mut display, runner) = degus_driver::new(resources, rx, tx, Config::new());

    spawner.spawn(runner_task(runner, FanRouter { events }).unwrap());

    // Switch to page 1 (PIC_Set, VP 0x0084), then read the page shown back
    // (PIC_Now, VP 0x0014) to see that it took.
    display
        .write(Address::Vp(0x0084), &(0x5A01_u16, 1_u16))
        .await
        .unwrap();
    let page: u16 = display.read(Address::Vp(0x0014), 1).await.unwrap();
    println!("page shown: {page}");

    let mut app = FanApp::default();
    loop {
        match events.wait().await {
            Event::Speed(speed) => {
                app.speed = speed;
                println!("speed set to {:?}", app.speed);
            }
            Event::Adjust(adjust) => {
                app.speed = match adjust {
                    Adjust::Increase => app.speed.increased(),
                    Adjust::Decrease => app.speed.decreased(),
                };
                println!("speed adjusted to {:?}", app.speed);
                display.write(SPEED, &app.speed).await.unwrap();
            }
            Event::Rotate(rotating) => {
                app.rotating = rotating;
                println!("rotating: {}", app.rotating);
            }
        }
        println!("{app:?}");
    }
}

#[embassy_executor::task]
async fn runner_task(mut runner: Runner<'static, NoopRawMutex, UartRx>, mut router: FanRouter) {
    let mut rx_buf = [0; 16];
    // Only the serial port failing stops the runner.
    let Err(e) = runner.run(&mut rx_buf, &mut router).await;
    panic!("receive failed: {e}");
}
