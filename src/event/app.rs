//! Desktop app shell that coordinates routed events, IO drain, and redraw policy.

use std::sync::Arc;
use std::time::Duration;

use log::{info, warn};
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::window::{Window, WindowId};

#[path = "app_support.rs"]
mod support;

use crate::demo::LayoutDemo;
use crate::event::{EventRouter, RedrawThrottle, RouteAction, ViewportSnapshot};
use crate::io::{IoEvent, IoEventDriver, IoRuntime};
use crate::renderer::Renderer;

const MAX_IO_DRAIN: usize = 4096;
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100);
pub(crate) use support::{
    logical_viewport, AppDemo, AppRenderer, AppRuntime, AppWindow, REDRAW_MIN_INTERVAL,
};

pub(crate) type DesktopApp = SteleApp<IoRuntime, Arc<Window>, Renderer<'static>, LayoutDemo>;

/// Owns desktop app state and applies routed window or IO actions.
pub(crate) struct SteleApp<Rt, Win, Rend, Demo>
where
    Rt: AppRuntime,
    Win: AppWindow,
    Rend: AppRenderer,
    Demo: AppDemo<Rend>,
{
    window: Option<Win>,
    window_id: Option<WindowId>,
    renderer: Option<Rend>,
    demo: Option<Demo>,
    io_runtime: Option<Rt>,
    io_driver: IoEventDriver,
    router: Option<EventRouter>,
    throttle: RedrawThrottle,
    shutting_down: bool,
}

impl<Rt, Win, Rend, Demo> SteleApp<Rt, Win, Rend, Demo>
where
    Rt: AppRuntime,
    Win: AppWindow,
    Rend: AppRenderer,
    Demo: AppDemo<Rend>,
{
    /// Creates an app shell around the IO runtime, driver, router, and redraw throttle.
    pub(crate) fn new(
        io_runtime: Rt,
        io_driver: IoEventDriver,
        router: EventRouter,
        min_redraw_interval: Duration,
    ) -> Self {
        Self {
            window: None,
            window_id: None,
            renderer: None,
            demo: None,
            io_runtime: Some(io_runtime),
            io_driver,
            router: Some(router),
            throttle: RedrawThrottle::new(min_redraw_interval),
            shutting_down: false,
        }
    }

    /// Returns whether the desktop shell should skip a resume attempt.
    pub(crate) fn should_skip_resume(&self) -> bool {
        self.window.is_some() || self.shutting_down
    }

    /// Returns whether shutdown has already started.
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    /// Attaches the window-backed rendering surface after startup.
    pub(crate) fn attach_surface(&mut self, window: Win, renderer: Rend, demo: Demo) {
        self.window_id = Some(window.id());
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.demo = Some(demo);
    }

    /// Routes and applies one window event, returning whether the event loop should exit.
    pub(crate) fn on_window_event(&mut self, window_id: WindowId, event: &WindowEvent) -> bool {
        if self.shutting_down || Some(window_id) != self.window_id {
            return false;
        }

        let Some(window) = self.window.as_ref() else {
            return false;
        };
        let Some(router) = self.router.as_ref() else {
            return false;
        };

        let viewport = ViewportSnapshot::new(window.inner_size(), window.scale_factor() as f32);
        let action = router.dispatch(event, viewport);
        self.apply_route_action(action)
    }

    /// Applies one routed action and reports whether the event loop should exit.
    pub(crate) fn apply_route_action(&mut self, action: RouteAction) -> bool {
        match action {
            RouteAction::None => false,
            RouteAction::Resize(update) => {
                self.handle_resize(update.size, update.scale_factor);
                false
            }
            RouteAction::RedrawRequested => {
                self.render_frame();
                false
            }
            RouteAction::CloseRequested => self.begin_shutdown(),
        }
    }

    /// Handles one async-to-winit wake and reports whether shutdown began.
    pub(crate) fn on_wake(&mut self) -> bool {
        let outcome = self.io_driver.on_wake(MAX_IO_DRAIN);
        info!("io.event.wake drained={}", outcome.drained);

        for event in outcome.events {
            self.handle_io_event(event);
        }

        if outcome.disconnected {
            return self.begin_shutdown();
        }

        if outcome.drained > 0 {
            self.handle_redraw_after_wake();
        }

        if outcome.wake_again {
            warn!(
                "io.event.drain_overflow count={} limit={}",
                outcome.drained, MAX_IO_DRAIN
            );
            if let Some(io_runtime) = self.io_runtime.as_ref() {
                io_runtime.wake_loop();
            }
        }

        false
    }

    /// Applies one deferred redraw deadline.
    pub(crate) fn on_deadline(&mut self) {
        self.throttle.finish_deadline();

        if self.throttle.is_dirty() {
            let interval = self.record_redraw();
            self.request_io_redraw(interval);
        }
    }

    /// Shuts the runtime down during app exit.
    pub(crate) fn on_exit(&mut self) {
        self.shutdown_runtime();
    }

    /// Starts shutdown exactly once and reports whether it transitioned state.
    pub(crate) fn begin_shutdown(&mut self) -> bool {
        if self.shutting_down {
            return false;
        }

        self.shutting_down = true;
        self.demo = None;
        self.renderer = None;
        self.window = None;
        self.window_id = None;
        self.router = None;
        self.shutdown_runtime();
        true
    }

    fn render_frame(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        window.pre_present_notify();
        renderer.frame();
    }

    fn handle_resize(&mut self, size: PhysicalSize<u32>, scale_factor: f32) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        renderer.resize(size, scale_factor);

        if let Some(demo) = self.demo.as_mut() {
            demo.resize(logical_viewport(size, scale_factor));
            demo.apply(renderer);
        }

        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn handle_io_event(&mut self, event: IoEvent) {
        let event_type = event.kind();
        match event {
            IoEvent::MockTick { payload } => {
                info!("io.event.recv type={} payload={:?}", event_type, payload);
            }
        }
    }

    fn handle_redraw_after_wake(&mut self) {
        if self.throttle.should_redraw_now() {
            let interval = self.record_redraw();
            self.request_io_redraw(interval);
            return;
        }

        self.throttle.mark_dirty();
        if self.throttle.pending_deadline() {
            return;
        }

        self.throttle.start_deadline();
        self.schedule_deadline(self.throttle.deadline_delay());
        info!("event.throttle.deferred");
    }

    fn record_redraw(&mut self) -> Duration {
        let interval = self
            .throttle
            .elapsed_since_last_redraw()
            .unwrap_or_default();
        self.throttle.record_redraw();
        self.throttle.clear_dirty();
        interval
    }

    fn request_io_redraw(&mut self, interval: Duration) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        window.request_redraw();
        info!("event.throttle.redraw interval_ms={}", interval.as_millis());
    }

    fn schedule_deadline(&mut self, delay: Duration) {
        let Some(io_runtime) = self.io_runtime.as_ref() else {
            return;
        };

        io_runtime.schedule_deadline(delay);
    }

    fn shutdown_runtime(&mut self) {
        if let Some(io_runtime) = self.io_runtime.take() {
            io_runtime.shutdown(RUNTIME_SHUTDOWN_TIMEOUT);
        }
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
