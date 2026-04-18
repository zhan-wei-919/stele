//! Composer-side bump reuse and view-update dispatch for the scene pipeline.

use log::{info, warn};
use tokio::sync::mpsc;

use crate::io::{ViewUpdate, WakeHandle};

use super::{SceneConfig, SCENE_BUFFER_SLOTS};

/// Error returned when the view-side transport has disconnected.
#[derive(Debug)]
pub(crate) struct ViewDisconnected;

/// Composer-side buffer pool plus shared scene/atlas dispatch adapter.
pub(crate) struct SceneBufferPool {
    return_rx: mpsc::Receiver<bumpalo::Bump>,
    view_update_tx: mpsc::Sender<ViewUpdate>,
    view_wake_handle: WakeHandle,
    config: SceneConfig,
}

impl SceneBufferPool {
    /// Creates a new scene buffer pool bound to one shared view-update queue and wake path.
    pub(crate) fn new(
        return_rx: mpsc::Receiver<bumpalo::Bump>,
        view_update_tx: mpsc::Sender<ViewUpdate>,
        view_wake_handle: WakeHandle,
        config: SceneConfig,
    ) -> Self {
        Self {
            return_rx,
            view_update_tx,
            view_wake_handle,
            config,
        }
    }

    /// Returns the scene runtime config injected at startup.
    pub(crate) fn config(&self) -> &SceneConfig {
        &self.config
    }

    /// Acquires one empty bump arena for the next composed scene.
    pub(crate) async fn acquire_empty_bump(&mut self) -> Result<bumpalo::Bump, ViewDisconnected> {
        let mut bump = self.return_rx.recv().await.ok_or(ViewDisconnected)?;
        bump.reset();
        info!(
            "pool.bump_reset allocated_bytes={} slots={}",
            bump.allocated_bytes(),
            SCENE_BUFFER_SLOTS
        );
        Ok(bump)
    }

    /// Enqueues one view update and immediately wakes winit for delivery.
    pub(crate) async fn dispatch_view_update(
        &self,
        update: ViewUpdate,
    ) -> Result<(), ViewDisconnected> {
        self.view_update_tx
            .send(update)
            .await
            .map_err(|_| ViewDisconnected)?;
        if self.view_wake_handle.wake() {
            return Ok(());
        }
        warn!("io.runtime.send_failed payload=view_update_wake");
        Err(ViewDisconnected)
    }
}
