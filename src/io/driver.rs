//! Testable IO drain state machine shared by the app and integration tests.

use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::UnboundedReceiver;

use super::IoEvent;

/// Result of handling one IO wake-up on the winit thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WakeOutcome {
    pub drained: usize,
    pub events: Vec<IoEvent>,
    pub wake_again: bool,
    pub disconnected: bool,
}

/// Owns IO drain state that can be driven from winit user events.
pub(crate) struct IoEventDriver {
    io_event_rx: UnboundedReceiver<IoEvent>,
    overflow_io_event: Option<IoEvent>,
}

impl IoEventDriver {
    /// Creates a new driver around the winit-side IO receiver.
    pub(crate) fn new(io_event_rx: UnboundedReceiver<IoEvent>) -> Self {
        Self {
            io_event_rx,
            overflow_io_event: None,
        }
    }

    /// Drains pending IO and reports what the app should do next.
    pub(crate) fn on_wake(&mut self, limit: usize) -> WakeOutcome {
        let (events, wake_again, disconnected) = self.drain_events(limit);
        let drained = events.len();

        WakeOutcome {
            drained,
            events,
            wake_again,
            disconnected,
        }
    }

    fn drain_events(&mut self, limit: usize) -> (Vec<IoEvent>, bool, bool) {
        debug_assert!(limit > 0, "drain limit must be positive");

        let mut events = Vec::new();
        let mut disconnected = false;

        if let Some(event) = self.overflow_io_event.take() {
            events.push(event);
        }

        while events.len() < limit {
            match self.io_event_rx.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    return (events, false, disconnected);
                }
            }
        }

        let mut wake_again = false;
        if events.len() == limit {
            match self.io_event_rx.try_recv() {
                Ok(event) => {
                    self.overflow_io_event = Some(event);
                    wake_again = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => disconnected = true,
            }
        }

        (events, wake_again, disconnected)
    }
}
