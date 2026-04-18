//! Testable view-update drain state machine shared by the app and integration tests.

use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::Receiver;

use super::ViewUpdate;

/// Result of draining pending view updates on one wake.
#[derive(Debug)]
pub(crate) struct WakeOutcome {
    pub(crate) drained: usize,
    pub(crate) updates: Vec<ViewUpdate>,
    pub(crate) wake_again: bool,
    pub(crate) disconnected: bool,
}

/// Owns winit-side drain state for queued view-update payloads.
pub(crate) struct ViewUpdateDriver {
    view_update_rx: Receiver<ViewUpdate>,
    overflow_view_update: Option<ViewUpdate>,
}

impl ViewUpdateDriver {
    /// Creates a new driver around the winit-side view-update receiver.
    pub(crate) fn new(view_update_rx: Receiver<ViewUpdate>) -> Self {
        Self {
            view_update_rx,
            overflow_view_update: None,
        }
    }

    /// Drains pending view updates and reports what the app should do next.
    pub(crate) fn on_wake(&mut self, limit: usize) -> WakeOutcome {
        let (updates, wake_again, disconnected) = self.drain_updates(limit);
        let drained = updates.len();

        WakeOutcome {
            drained,
            updates,
            wake_again,
            disconnected,
        }
    }

    fn drain_updates(&mut self, limit: usize) -> (Vec<ViewUpdate>, bool, bool) {
        debug_assert!(limit > 0, "drain limit must stay positive");

        let mut updates = Vec::new();
        let mut disconnected = false;
        if let Some(update) = self.overflow_view_update.take() {
            updates.push(update);
        }

        while updates.len() < limit {
            match self.view_update_rx.try_recv() {
                Ok(update) => updates.push(update),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    return (updates, false, disconnected);
                }
            }
        }

        let mut wake_again = false;
        if updates.len() == limit {
            match self.view_update_rx.try_recv() {
                Ok(update) => {
                    self.overflow_view_update = Some(update);
                    wake_again = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => disconnected = true,
            }
        }

        (updates, wake_again, disconnected)
    }
}
