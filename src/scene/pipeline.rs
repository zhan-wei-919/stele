//! View-side scene-buffer retirement back into the reusable composer pool.

use log::warn;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use super::SceneBuffer;

/// View-side adapter responsible for retiring consumed scene buffers back to the composer pool.
#[derive(Clone)]
pub(crate) struct ScenePipeline {
    return_tx: mpsc::Sender<bumpalo::Bump>,
}

impl ScenePipeline {
    /// Creates the view-side scene pipeline endpoint.
    pub(crate) fn new(return_tx: mpsc::Sender<bumpalo::Bump>) -> Self {
        Self { return_tx }
    }

    /// Retires one scene buffer, returning its owner bump to the pool when the channel is live.
    pub(crate) fn retire(&self, buffer: Box<SceneBuffer>) {
        let bump = SceneBuffer::into_owner(*buffer);
        match self.return_tx.try_send(bump) {
            Ok(()) => {}
            Err(TrySendError::Closed(_)) => {}
            Err(TrySendError::Full(_)) => {
                warn!("view.retire_drop reason=return_channel_full");
            }
        }
    }
}
