//! Tells the window that there is something new to draw, from a thread of
//! its own: the simulator's thread must not wait for whatever the window's
//! callback does.

use std::sync::Arc;

use parking_lot::{Condvar, Mutex, MutexGuard};

type Callback = Arc<dyn Fn() + Send + Sync>;

/// Calls the window's callback. Notices that come while it runs make one
/// more call.
#[derive(Default)]
pub(crate) struct Notifier {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    callback: Option<Callback>,
    pending: bool,
    stopped: bool,
}

impl Notifier {
    pub fn set_callback(&self, callback: Callback) {
        let mut state = self.state.lock();
        state.callback = Some(callback);
        // The window has drawn nothing yet.
        state.pending = true;
        self.changed.notify_one();
    }

    pub fn notify(&self) {
        self.state.lock().pending = true;
        self.changed.notify_one();
    }

    /// Ends [`run`](Notifier::run), once the callback returns.
    pub fn stop(&self) {
        self.state.lock().stopped = true;
        self.changed.notify_one();
    }

    /// Calls the callback for the notices, until stopped.
    pub fn run(&self) {
        let mut state = self.state.lock();
        while !state.stopped {
            match state.callback.clone() {
                Some(callback) if state.pending => {
                    state.pending = false;
                    MutexGuard::unlocked(&mut state, || callback());
                }
                _ => self.changed.wait(&mut state),
            }
        }
    }
}
