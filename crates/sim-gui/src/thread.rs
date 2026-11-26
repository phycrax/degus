//! The display on a thread of its own, which keeps its time and passes
//! bytes between it and the host, however slow the window is.

use std::fmt;
use std::ops::Deref;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Weak};
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};

use degus_set::controls::Point;
use degus_sim::{Event, Sim};
use parking_lot::{FairMutex, FairMutexGuard, Mutex};

use crate::monitor::{Activity, ActivityKind, Monitor, Queue};
use crate::notify::Notifier;
use crate::speaker::Speaker;
use crate::uart::{Outbox, Uart};

/// How often time passes in the display while nothing else happens.
const TICK: Duration = Duration::from_millis(20);
/// How many messages are handled in one go, with the simulator locked.
const BATCH: usize = 256;
/// How long a touch on a screen lasts once the screen isn't shown anymore.
pub(crate) const TOUCH_LEASE: Duration = Duration::from_millis(500);

/// What the display's thread is told.
pub(crate) enum Message {
    /// Bytes from the host.
    Bytes(Vec<u8>),
    /// The simulator was changed: pass on and draw what came of it.
    Changed,
    Stop,
}

/// A simulated display, running on a thread of its own as a panel runs on
/// its own chip.
///
/// The thread feeds what the host writes to the [`Uart`] to the [`Sim`],
/// sends back what it answers, lets time pass and plays its sounds, so that
/// a slow or stalled window doesn't delay the display's answers. The window
/// [`lock`](SimThread::lock)s the `Sim` only to draw it.
///
/// Dropping it ends the thread: the host's reads return the end.
pub struct SimThread {
    shared: Arc<Shared>,
    inbox: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

/// What the thread and its handle share.
struct Shared {
    /// Fair, so that a window drawing without pause doesn't keep the thread
    /// from answering.
    display: FairMutex<Display>,
    outbox: Arc<Outbox>,
    monitors: Mutex<Monitors>,
    speaker: Mutex<Option<Speaker>>,
    notifier: Arc<Notifier>,
}

struct Display {
    sim: Sim,
    /// The touch of a screen.
    touch: Option<ScreenTouch>,
    /// Touches of screens so far.
    touches: u64,
    /// What changes from outside the thread made the display send and
    /// report, for the thread to pass on.
    pending: Vec<Step>,
}

struct ScreenTouch {
    id: u64,
    /// When the touch ends, unless the screen is shown again before.
    until: Instant,
}

#[derive(Default)]
struct Monitors {
    queues: Vec<Weak<Queue>>,
    /// The thread ended: no more activity comes.
    closed: bool,
}

impl SimThread {
    /// Runs `sim` on a thread of its own.
    ///
    /// What `sim` sent and reported before is dropped, such as notes on
    /// the page shown at power-on: take it with [`Sim::take_events`] first
    /// to keep it.
    ///
    /// # Panics
    ///
    /// When the thread can't be started.
    pub fn new(mut sim: Sim) -> Self {
        sim.take_tx();
        sim.take_events();
        let display = Display {
            sim,
            touch: None,
            touches: 0,
            pending: Vec::new(),
        };
        let notifier = Arc::new(Notifier::default());
        let shared = Arc::new(Shared {
            display: FairMutex::new(display),
            outbox: Arc::new(Outbox::new()),
            monitors: Mutex::default(),
            speaker: Mutex::default(),
            notifier: notifier.clone(),
        });
        let (inbox, messages) = channel();
        let pump = Pump {
            shared: shared.clone(),
            messages,
        };
        Builder::new()
            .name("degus sim notify".into())
            .spawn(move || notifier.run())
            .expect("the notifying thread starts");
        let thread = Builder::new()
            .name("degus sim".into())
            .spawn(move || pump.run())
            .expect("the display's thread starts");
        Self {
            shared,
            inbox,
            thread: Some(thread),
        }
    }

    /// The display's UART, for the host. Each call gives a handle to the
    /// same UART. What the display sends before the first call is lost, as
    /// on a UART nothing is connected to.
    pub fn uart(&self) -> Uart {
        self.shared.outbox.attach();
        Uart::new(self.shared.outbox.clone(), self.inbox.clone())
    }

    /// Calls `callback` whenever there may be something new to draw or to
    /// [`monitor`](SimThread::monitor), from a thread that does nothing
    /// else. An eframe app asks for a repaint:
    ///
    /// ```no_run
    /// # fn new(cc: &degus_sim_gui::eframe::CreationContext<'_>, sim: degus_sim_gui::SimThread) {
    /// let ctx = cc.egui_ctx.clone();
    /// sim.on_change(move || ctx.request_repaint());
    /// # }
    /// ```
    ///
    /// It replaces the callback set before, and is called once right away.
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.shared.notifier.set_callback(Arc::new(callback));
    }

    /// Plays the display's sounds on `speaker`.
    pub fn set_speaker(&self, speaker: Speaker) {
        *self.shared.speaker.lock() = Some(speaker);
    }

    /// Locks the simulator, to draw it or to look into it.
    ///
    /// The display answers no one while it is locked, so hold it no longer
    /// than it takes to render, and don't call
    /// [`update`](SimThread::update) meanwhile: it locks it too.
    pub fn lock(&self) -> SimGuard<'_> {
        SimGuard(self.shared.display.lock())
    }

    /// Changes the simulator, for tools such as a page list or a command
    /// box. What the change makes the display send and report goes on as
    /// usual, unless `change` takes it with [`Sim::take_tx`] or
    /// [`Sim::take_events`]: those return only what `change` caused.
    ///
    /// Like [`lock`](SimThread::lock), it holds up the display while
    /// `change` runs. Leave [`Sim::feed`] of the host's bytes and
    /// [`Sim::advance`] to the thread.
    pub fn update<T>(&self, change: impl FnOnce(&mut Sim) -> T) -> T {
        self.change(|display| change(&mut display.sim))
    }

    /// Watches what passes over the UART and what happens in the display,
    /// from now on.
    pub fn monitor(&self) -> Monitor {
        let (monitor, queue) = Monitor::new();
        let mut monitors = self.shared.monitors.lock();
        if monitors.closed {
            queue.close();
        }
        monitors.queues.push(Arc::downgrade(&queue));
        monitor
    }

    /// A finger lands on a screen. The touch ends after [`TOUCH_LEASE`]
    /// unless [`move_touch`](SimThread::move_touch) keeps it up, so that it
    /// also ends when the screen isn't shown anymore.
    pub(crate) fn start_touch(&self, at: Point) -> u64 {
        self.change(|display| {
            display.sim.press(at);
            display.touches += 1;
            display.touch = Some(ScreenTouch {
                id: display.touches,
                until: Instant::now() + TOUCH_LEASE,
            });
            display.touches
        })
    }

    /// Moves touch `id`, unless it ended.
    pub(crate) fn move_touch(&self, id: u64, at: Point) -> bool {
        self.change(|display| {
            let Some(touch) = display.touch.as_mut().filter(|touch| touch.id == id) else {
                return false;
            };
            touch.until = Instant::now() + TOUCH_LEASE;
            display.sim.move_to(at);
            true
        })
    }

    /// Ends touch `id`, unless it ended.
    pub(crate) fn end_touch(&self, id: u64) {
        self.change(|display| {
            if display.touch.take_if(|touch| touch.id == id).is_some() {
                display.sim.release();
            }
        });
    }

    /// Changes the display, and has the thread pass on what came of it.
    fn change<T>(&self, change: impl FnOnce(&mut Display) -> T) -> T {
        let result = {
            let mut display = self.shared.display.lock();
            let result = change(&mut display);
            display.settle();
            result
        };
        // Fails only once the thread panicked.
        let _ = self.inbox.send(Message::Changed);
        result
    }
}

impl Drop for SimThread {
    fn drop(&mut self) {
        let _ = self.inbox.send(Message::Stop);
        // Not waited for: the callback may wait for whoever drops this.
        self.shared.notifier.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl fmt::Debug for SimThread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimThread").finish_non_exhaustive()
    }
}

/// The locked simulator, from [`SimThread::lock`].
pub struct SimGuard<'a>(FairMutexGuard<'a, Display>);

impl Deref for SimGuard<'_> {
    type Target = Sim;

    fn deref(&self) -> &Sim {
        &self.0.sim
    }
}

/// What a message, a change or the time that passed made the display send
/// and report.
struct Step {
    at: Instant,
    /// What the host sent.
    rx: Vec<u8>,
    /// What the display sent.
    tx: Vec<u8>,
    events: Vec<Event>,
}

impl Step {
    fn is_empty(&self) -> bool {
        self.rx.is_empty() && self.tx.is_empty() && self.events.is_empty()
    }
}

impl Display {
    /// What the display sent and reported since the last step, after `rx`.
    fn step(&mut self, rx: Vec<u8>) -> Step {
        Step {
            at: Instant::now(),
            rx,
            tx: self.sim.take_tx(),
            events: self.sim.take_events(),
        }
    }

    /// Keeps what a change made the display send and report for the
    /// thread, and ends the touch of a screen the change ended, as a reset
    /// or a reload does.
    fn settle(&mut self) {
        if !self.sim.is_pressed() {
            self.touch = None;
        }
        let step = self.step(Vec::new());
        if !step.is_empty() {
            self.pending.push(step);
        }
    }

    /// Ends the touch of a screen that isn't shown anymore.
    fn end_stale_touch(&mut self, now: Instant) {
        if self.touch.take_if(|touch| touch.until <= now).is_some() {
            self.sim.release();
        }
    }
}

/// The display's thread.
struct Pump {
    shared: Arc<Shared>,
    messages: Receiver<Message>,
}

impl Pump {
    fn run(self) {
        let mut generation = None;
        let mut last = Instant::now();
        loop {
            let first = match self.messages.recv_timeout(TICK) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };

            // With the simulator locked, only what needs it.
            let drawn = generation;
            let mut stop = false;
            let mut steps = {
                let mut display = self.shared.display.lock();
                let mut steps = std::mem::take(&mut display.pending);
                let now = Instant::now();
                display.sim.advance(now - last);
                last = now;
                display.end_stale_touch(now);
                steps.push(display.step(Vec::new()));
                let messages = first.into_iter().chain(self.messages.try_iter());
                for message in messages.take(BATCH) {
                    match message {
                        Message::Bytes(rx) => {
                            display.sim.feed(&rx);
                            steps.push(display.step(rx));
                        }
                        Message::Changed => {}
                        Message::Stop => stop = true,
                    }
                }
                generation = Some(display.sim.generation());
                steps
            };
            steps.retain(|step| !step.is_empty());

            // The host first: it waits.
            let dropping: Vec<bool> = steps
                .iter()
                .map(|step| !step.tx.is_empty() && self.shared.outbox.push(&step.tx))
                .collect();
            if let Some(speaker) = &*self.shared.speaker.lock() {
                for event in steps.iter().flat_map(|step| &step.events) {
                    if let Event::Sound(sound) = event {
                        speaker.play(sound.clone());
                    }
                }
            }
            let seen = self.shared.publish(&steps, &dropping);
            if seen || generation != drawn {
                self.shared.notifier.notify();
            }
            if stop {
                break;
            }
        }
    }
}

/// Tells the host and the monitors that the display is gone, also when the
/// thread panics.
impl Drop for Pump {
    fn drop(&mut self) {
        self.shared.outbox.close();
        let mut monitors = self.shared.monitors.lock();
        monitors.closed = true;
        for queue in monitors.queues.iter().filter_map(Weak::upgrade) {
            queue.close();
        }
    }
}

impl Shared {
    /// Passes `steps` on to the monitors. Returns whether there were any.
    fn publish(&self, steps: &[Step], dropping: &[bool]) -> bool {
        let mut monitors = self.monitors.lock();
        monitors.queues.retain(|queue| queue.strong_count() > 0);
        if monitors.queues.is_empty() || steps.is_empty() {
            return false;
        }
        let mut activity = Vec::new();
        for (step, &dropping) in steps.iter().zip(dropping) {
            let mut seen = |kind| activity.push(Activity { at: step.at, kind });
            if !step.rx.is_empty() {
                seen(ActivityKind::FromHost(step.rx.clone()));
            }
            if !step.tx.is_empty() {
                seen(ActivityKind::ToHost(step.tx.clone()));
            }
            if dropping {
                seen(ActivityKind::Overflow);
            }
            for event in &step.events {
                seen(ActivityKind::Event(event.clone()));
            }
        }
        for queue in monitors.queues.iter().filter_map(Weak::upgrade) {
            queue.extend(activity.iter().cloned());
        }
        true
    }
}
