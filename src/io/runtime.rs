//! Tokio runtime ownership and helper operations for the store/view bridge.

use std::future::Future;
use std::io;
use std::time::Duration;

use log::{info, warn};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{Receiver, UnboundedReceiver, UnboundedSender};
use winit::event_loop::EventLoopProxy;

use super::{Action, ViewUpdate, WakeEvent};
use crate::scene::{
    SceneBufferPool, SceneConfig, ScenePipeline, SCENE_BUFFER_SLOTS, VIEW_UPDATE_CHANNEL_CAPACITY,
};

/// Cloneable wake adapter shared by the async runtime and scene buffer pool.
#[derive(Clone)]
pub(crate) struct WakeHandle {
    proxy: EventLoopProxy<WakeEvent>,
}

impl WakeHandle {
    /// Creates a wake handle from the shared event-loop proxy.
    pub(crate) fn new(proxy: EventLoopProxy<WakeEvent>) -> Self {
        Self { proxy }
    }

    /// Requests one winit wake.
    pub(crate) fn wake(&self) -> bool {
        if self.proxy.send_event(WakeEvent::Wake).is_err() {
            warn!("io.runtime.wake_failed event=wake");
            return false;
        }
        true
    }
}

/// Owns the async runtime and the winit-side ends of the bridge channels.
pub(crate) struct IoRuntime {
    runtime: Runtime,
    view_update_rx: Option<Receiver<ViewUpdate>>,
    action_tx: UnboundedSender<Action>,
    wake_handle: WakeHandle,
}

/// Async-side handle used by the store task.
pub(crate) struct IoHandle {
    action_rx: UnboundedReceiver<Action>,
    pending_action: Option<Action>,
}

impl IoRuntime {
    /// Creates the runtime and splits the bridge into winit-side and async-side halves.
    pub(crate) fn new(
        proxy: EventLoopProxy<WakeEvent>,
        scene_config: SceneConfig,
    ) -> io::Result<(Self, IoHandle, SceneBufferPool, ScenePipeline)> {
        let runtime = Runtime::new()?;
        let wake_handle = WakeHandle::new(proxy);
        let (view_update_tx, view_update_rx) = mpsc::channel(VIEW_UPDATE_CHANNEL_CAPACITY);
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        let (return_tx, return_rx) = mpsc::channel(SCENE_BUFFER_SLOTS);

        for _ in 0..SCENE_BUFFER_SLOTS {
            return_tx
                .blocking_send(bumpalo::Bump::with_capacity(
                    scene_config.arena_initial_chunk_bytes,
                ))
                .expect("scene return channel must accept warm-up buffers");
        }

        info!("io.runtime.start runtime=tokio");

        Ok((
            Self {
                runtime,
                view_update_rx: Some(view_update_rx),
                action_tx,
                wake_handle: wake_handle.clone(),
            },
            IoHandle {
                action_rx,
                pending_action: None,
            },
            SceneBufferPool::new(return_rx, view_update_tx, wake_handle, scene_config),
            ScenePipeline::new(return_tx),
        ))
    }

    /// Returns a sender for actions produced by the winit thread.
    pub(crate) fn action_tx(&self) -> UnboundedSender<Action> {
        self.action_tx.clone()
    }

    /// Moves the winit-side view-update receiver into the shared driver.
    pub(crate) fn take_view_update_rx(&mut self) -> Receiver<ViewUpdate> {
        self.view_update_rx
            .take()
            .expect("view update receiver must only be taken once")
    }

    /// Spawns one async task on the owned runtime.
    pub(crate) fn spawn_task<F>(&self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.runtime.handle().spawn(task);
    }

    /// Wakes the winit loop through the shared proxy.
    pub(crate) fn wake_loop(&self) {
        let _ = self.wake_handle.wake();
    }

    /// Shuts the runtime down with a bounded wait.
    pub(crate) fn shutdown(self, timeout: Duration) {
        info!("io.runtime.shutdown");
        self.runtime.shutdown_timeout(timeout);
    }
}

impl IoHandle {
    /// Waits for the next action from the winit thread.
    pub(crate) async fn next_action(&mut self) -> Option<Action> {
        if let Some(action) = self.pending_action.take() {
            return Some(action);
        }
        self.action_rx.recv().await
    }

    /// Tries to receive the next action without waiting.
    pub(crate) fn try_next_action(&mut self) -> Result<Action, TryRecvError> {
        if let Some(action) = self.pending_action.take() {
            return Ok(action);
        }
        self.action_rx.try_recv()
    }

    /// Pushes one action back so the store can preserve queue order after lookahead.
    pub(crate) fn push_front_action(&mut self, action: Action) {
        debug_assert!(
            self.pending_action.is_none(),
            "pending action slot must stay empty before push_front"
        );
        self.pending_action = Some(action);
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> (UnboundedSender<Action>, Self) {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        (
            action_tx,
            Self {
                action_rx,
                pending_action: None,
            },
        )
    }
}
