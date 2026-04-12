//! Tokio runtime ownership and helper operations for the store/view bridge.

use std::future::Future;
use std::io;
use std::time::Duration;

use log::{info, warn};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use winit::event_loop::EventLoopProxy;

use super::{Action, SceneDiff, WakeEvent};

/// Owns the async runtime and the winit-side ends of the bridge channels.
pub(crate) struct IoRuntime {
    runtime: Runtime,
    scene_diff_rx: Option<UnboundedReceiver<SceneDiff>>,
    action_tx: UnboundedSender<Action>,
    proxy: EventLoopProxy<WakeEvent>,
}

/// Async-side handle used by the store task.
pub(crate) struct IoHandle {
    scene_diff_tx: UnboundedSender<SceneDiff>,
    action_rx: UnboundedReceiver<Action>,
    proxy: EventLoopProxy<WakeEvent>,
}

impl IoRuntime {
    /// Creates the runtime and splits the bridge into winit-side and async-side halves.
    pub(crate) fn new(proxy: EventLoopProxy<WakeEvent>) -> io::Result<(Self, IoHandle)> {
        let runtime = Runtime::new()?;
        let (scene_diff_tx, scene_diff_rx) = mpsc::unbounded_channel();
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        info!("io.runtime.start runtime=tokio");

        Ok((
            Self {
                runtime,
                scene_diff_rx: Some(scene_diff_rx),
                action_tx,
                proxy: proxy.clone(),
            },
            IoHandle {
                scene_diff_tx,
                action_rx,
                proxy,
            },
        ))
    }

    /// Returns a sender for actions produced by the winit thread.
    pub(crate) fn action_tx(&self) -> UnboundedSender<Action> {
        self.action_tx.clone()
    }

    /// Moves the winit-side SceneDiff receiver into the shared driver.
    pub(crate) fn take_scene_diff_rx(&mut self) -> UnboundedReceiver<SceneDiff> {
        self.scene_diff_rx
            .take()
            .expect("scene diff receiver must only be taken once")
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
        if self.proxy.send_event(WakeEvent::Wake).is_err() {
            warn!("io.runtime.wake_failed event=wake");
        }
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
        self.action_rx.recv().await
    }

    /// Wakes the winit event loop through the shared proxy.
    pub(crate) fn wake_loop(&self) -> bool {
        if self.proxy.send_event(WakeEvent::Wake).is_err() {
            warn!("io.runtime.wake_failed event=wake");
            return false;
        }

        true
    }

    /// Sends one SceneDiff toward the view and wakes winit for delivery.
    pub(crate) fn dispatch_scene_diff(&self, diff: SceneDiff) -> bool {
        if self.scene_diff_tx.send(diff).is_err() {
            warn!("io.runtime.send_failed payload=scene_diff");
            return false;
        }

        self.wake_loop()
    }
}
