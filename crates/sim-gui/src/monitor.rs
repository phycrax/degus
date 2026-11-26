//! The monitor: what passes over the UART and what happens in the display,
//! in order, as a logic analyser on the line would show it.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use degus_sim::Event;
use parking_lot::{Condvar, Mutex};

/// How much activity a monitor keeps when it isn't read.
const BACKLOG: usize = 10_000;

/// Something the monitor saw, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub at: Instant,
    pub kind: ActivityKind,
}

/// What a monitor sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityKind {
    /// Bytes from the host, in the chunks they came in.
    FromHost(Vec<u8>),
    /// Bytes the display sent.
    ToHost(Vec<u8>),
    /// Something that happened in the display.
    Event(Event),
    /// The host doesn't read: what the display sends is dropped until it
    /// reads again.
    Overflow,
}

/// What passes over the display's UART and what happens in the display,
/// from [`SimThread::monitor`](crate::SimThread::monitor), for tools such
/// as a log.
///
/// It sees what happens from its creation on, and keeps the latest 10 000
/// [`Activity`]s that aren't taken.
pub struct Monitor(Arc<Queue>);

#[derive(Default)]
pub(crate) struct Queue {
    state: Mutex<QueueState>,
    changed: Condvar,
}

#[derive(Default)]
struct QueueState {
    activity: VecDeque<Activity>,
    /// The display is gone.
    closed: bool,
}

impl Queue {
    pub fn extend(&self, activity: impl IntoIterator<Item = Activity>) {
        let mut state = self.state.lock();
        for item in activity {
            if state.activity.len() == BACKLOG {
                state.activity.pop_front();
            }
            state.activity.push_back(item);
        }
        self.changed.notify_all();
    }

    pub fn close(&self) {
        self.state.lock().closed = true;
        self.changed.notify_all();
    }
}

impl Monitor {
    pub(crate) fn new() -> (Self, Arc<Queue>) {
        let queue = Arc::new(Queue::default());
        (Self(queue.clone()), queue)
    }

    /// What was seen since the last call, without waiting.
    pub fn take(&self) -> Vec<Activity> {
        self.0.state.lock().activity.drain(..).collect()
    }

    /// Waits for the next activity. `None` once the display is gone and
    /// everything is taken.
    pub fn recv(&self) -> Option<Activity> {
        let mut state = self.0.state.lock();
        loop {
            if let Some(item) = state.activity.pop_front() {
                return Some(item);
            }
            if state.closed {
                return None;
            }
            self.0.changed.wait(&mut state);
        }
    }
}

impl fmt::Debug for Monitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let waiting = self.0.state.lock().activity.len();
        f.debug_struct("Monitor")
            .field("waiting", &waiting)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(n: u8) -> Activity {
        Activity {
            at: Instant::now(),
            kind: ActivityKind::FromHost(vec![n]),
        }
    }

    #[test]
    fn keeps_the_latest_activity() {
        let (monitor, queue) = Monitor::new();
        queue.extend((0..BACKLOG + 2).map(|n| bytes(n as u8)));
        let taken = monitor.take();
        assert_eq!(taken.len(), BACKLOG);
        assert_eq!(taken[0].kind, ActivityKind::FromHost(vec![2]));
        assert!(monitor.take().is_empty());
    }

    #[test]
    fn ends_after_what_is_left() {
        let (monitor, queue) = Monitor::new();
        queue.extend([bytes(1)]);
        queue.close();
        assert_eq!(
            monitor.recv().unwrap().kind,
            ActivityKind::FromHost(vec![1])
        );
        assert_eq!(monitor.recv(), None);
    }
}
