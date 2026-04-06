//! Tokio runtime ownership and helper operations for the IO event layer.

use std::future::Future;
use std::io;
use std::time::Duration;

use log::{info, warn};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use winit::event_loop::EventLoopProxy;

use super::{AppCommand, IoEvent, WakeEvent};

/// Owns the async runtime and the winit-side ends of the IO channels.
pub(crate) struct IoRuntime {
    runtime: Runtime,
    io_event_rx: Option<UnboundedReceiver<IoEvent>>,
    app_command_tx: UnboundedSender<AppCommand>,
    proxy: EventLoopProxy<WakeEvent>,
}

/// Async-side handle used by background tasks such as mock producers or future PTY/VT adapters.
pub(crate) struct IoHandle {
    io_event_tx: UnboundedSender<IoEvent>,
    app_command_rx: UnboundedReceiver<AppCommand>,
    proxy: EventLoopProxy<WakeEvent>,
}

impl IoRuntime {
    /// Creates the runtime and splits the IO layer into winit-side and async-side halves.
    pub(crate) fn new(proxy: EventLoopProxy<WakeEvent>) -> io::Result<(Self, IoHandle)> {
        let runtime = Runtime::new()?;
        let (io_event_tx, io_event_rx) = mpsc::unbounded_channel();
        let (app_command_tx, app_command_rx) = mpsc::unbounded_channel();

        info!("io.runtime.start runtime=tokio");

        Ok((
            Self {
                runtime,
                io_event_rx: Some(io_event_rx),
                app_command_tx,
                proxy: proxy.clone(),
            },
            IoHandle {
                io_event_tx,
                app_command_rx,
                proxy,
            },
        ))
    }

    /// Returns a sender for semantic commands produced by the winit thread.
    pub(crate) fn app_command_tx(&self) -> UnboundedSender<AppCommand> {
        self.app_command_tx.clone()
    }

    /// Moves the winit-side IO receiver into the shared event driver.
    pub(crate) fn take_io_event_rx(&mut self) -> UnboundedReceiver<IoEvent> {
        self.io_event_rx
            .take()
            .expect("io event receiver must only be taken once")
    }

    /// Spawns one async task on the owned runtime.
    pub(crate) fn spawn_task<F>(&self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.runtime.handle().spawn(task);
    }

    /// Schedules a deadline wake-up on the async runtime.
    pub(crate) fn schedule_deadline(&self, delay: Duration) {
        let proxy = self.proxy.clone();
        self.runtime.handle().spawn(async move {
            tokio::time::sleep(delay).await;
            if proxy.send_event(WakeEvent::DeadlineExpired).is_err() {
                warn!("io.runtime.wake_failed event=deadline_expired");
            }
        });
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
    /// Sends one IO event toward the winit side.
    pub(crate) fn send_io_event(&self, event: IoEvent) -> bool {
        if self.io_event_tx.send(event).is_err() {
            warn!("io.runtime.send_failed event=io_event");
            return false;
        }

        true
    }

    /// Wakes the winit event loop through the shared proxy.
    pub(crate) fn wake_loop(&self) -> bool {
        if self.proxy.send_event(WakeEvent::Wake).is_err() {
            warn!("io.runtime.wake_failed event=wake");
            return false;
        }

        true
    }

    /// Sends one IO event and wakes the winit loop for delivery.
    pub(crate) fn dispatch_io_event(&self, event: IoEvent) -> bool {
        self.send_io_event(event) && self.wake_loop()
    }

    /// Waits for the next semantic app command from the winit side.
    pub(crate) async fn next_app_command(&mut self) -> Option<AppCommand> {
        self.app_command_rx.recv().await
    }
}
