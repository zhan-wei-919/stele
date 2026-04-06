//! Support traits and concrete adapters for the desktop app shell.

use std::sync::Arc;
use std::time::Duration;

use winit::dpi::PhysicalSize;
use winit::window::{Window, WindowId};

use crate::demo::LayoutDemo;
use crate::io::IoRuntime;
use crate::renderer::Renderer;

pub(crate) const REDRAW_MIN_INTERVAL: Duration = Duration::from_millis(16);

/// Converts a physical size plus scale factor into logical viewport dimensions.
pub(crate) fn logical_viewport(size: PhysicalSize<u32>, scale_factor: f32) -> [f32; 2] {
    [
        size.width as f32 / scale_factor.max(1.0),
        size.height as f32 / scale_factor.max(1.0),
    ]
}

/// Operations `SteleApp` needs from the async runtime owner.
pub(crate) trait AppRuntime {
    /// Schedules a deferred deadline wake.
    fn schedule_deadline(&self, delay: Duration);

    /// Re-wakes the winit loop after a bounded IO drain.
    fn wake_loop(&self);

    /// Shuts the runtime down with a bounded wait.
    fn shutdown(self, timeout: Duration);
}

impl AppRuntime for IoRuntime {
    fn schedule_deadline(&self, delay: Duration) {
        IoRuntime::schedule_deadline(self, delay);
    }

    fn wake_loop(&self) {
        IoRuntime::wake_loop(self);
    }

    fn shutdown(self, timeout: Duration) {
        IoRuntime::shutdown(self, timeout);
    }
}

/// Window operations that the app uses from the desktop shell.
pub(crate) trait AppWindow {
    /// Returns the stable winit window identifier.
    fn id(&self) -> WindowId;

    /// Requests another redraw from winit.
    fn request_redraw(&self);

    /// Notifies the platform before presenting a rendered frame.
    fn pre_present_notify(&self);

    /// Returns the current inner size in physical pixels.
    fn inner_size(&self) -> PhysicalSize<u32>;

    /// Returns the current window scale factor.
    fn scale_factor(&self) -> f64;
}

impl AppWindow for Arc<Window> {
    fn id(&self) -> WindowId {
        Window::id(self)
    }

    fn request_redraw(&self) {
        Window::request_redraw(self);
    }

    fn pre_present_notify(&self) {
        Window::pre_present_notify(self);
    }

    fn inner_size(&self) -> PhysicalSize<u32> {
        Window::inner_size(self)
    }

    fn scale_factor(&self) -> f64 {
        Window::scale_factor(self)
    }
}

/// Renderer operations that the app drives from routed events.
pub(crate) trait AppRenderer {
    /// Draws one frame.
    fn frame(&mut self);

    /// Applies the latest viewport size and scale factor.
    fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f32);
}

impl AppRenderer for Renderer<'static> {
    fn frame(&mut self) {
        Renderer::frame(self);
    }

    fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f32) {
        Renderer::resize(self, size, scale_factor);
    }
}

/// Demo-scene operations that must stay wired into resize handling.
pub(crate) trait AppDemo<R: AppRenderer> {
    /// Reflows the demo content for a new logical viewport.
    fn resize(&mut self, viewport: [f32; 2]);

    /// Applies the current demo content to the renderer.
    fn apply(&self, renderer: &mut R);
}

impl AppDemo<Renderer<'static>> for LayoutDemo {
    fn resize(&mut self, viewport: [f32; 2]) {
        LayoutDemo::resize(self, viewport);
    }

    fn apply(&self, renderer: &mut Renderer<'static>) {
        LayoutDemo::apply(self, renderer);
    }
}
