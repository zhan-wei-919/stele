//! Testable SceneDiff drain state machine shared by the app and integration tests.

use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::UnboundedReceiver;

use super::SceneDiff;

/// Result of draining pending SceneDiffs on one wake.
#[derive(Debug, Clone)]
pub(crate) struct WakeOutcome {
    pub(crate) drained: usize,
    pub(crate) diffs: Vec<SceneDiff>,
    pub(crate) wake_again: bool,
    pub(crate) disconnected: bool,
}

/// Owns winit-side drain state for queued SceneDiff payloads.
pub(crate) struct SceneDiffDriver {
    scene_diff_rx: UnboundedReceiver<SceneDiff>,
    overflow_scene_diff: Option<SceneDiff>,
}

impl SceneDiffDriver {
    /// Creates a new driver around the winit-side SceneDiff receiver.
    pub(crate) fn new(scene_diff_rx: UnboundedReceiver<SceneDiff>) -> Self {
        Self {
            scene_diff_rx,
            overflow_scene_diff: None,
        }
    }

    /// Drains pending SceneDiffs and reports what the app should do next.
    pub(crate) fn on_wake(&mut self, limit: usize) -> WakeOutcome {
        let (diffs, wake_again, disconnected) = self.drain_diffs(limit);
        let drained = diffs.len();

        WakeOutcome {
            drained,
            diffs,
            wake_again,
            disconnected,
        }
    }

    fn drain_diffs(&mut self, limit: usize) -> (Vec<SceneDiff>, bool, bool) {
        debug_assert!(limit > 0, "drain limit must stay positive");

        let mut diffs = Vec::new();
        let mut disconnected = false;
        if let Some(diff) = self.overflow_scene_diff.take() {
            diffs.push(diff);
        }

        while diffs.len() < limit {
            match self.scene_diff_rx.try_recv() {
                Ok(diff) => diffs.push(diff),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    return (diffs, false, disconnected);
                }
            }
        }

        let mut wake_again = false;
        if diffs.len() == limit {
            match self.scene_diff_rx.try_recv() {
                Ok(diff) => {
                    self.overflow_scene_diff = Some(diff);
                    wake_again = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => disconnected = true,
            }
        }

        (diffs, wake_again, disconnected)
    }
}
