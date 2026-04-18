//! Support traits and concrete adapters for the desktop app shell.
use std::sync::Arc;
use std::time::Duration;

use winit::dpi::PhysicalSize;
use winit::window::{Window, WindowId};

use crate::io::{AtlasPatch, IoRuntime};
use crate::renderer::Renderer;
use crate::scene::SceneBuffer;

/// Operations `SteleApp` needs from the async runtime owner.
pub(crate) trait AppRuntime {
    /// Re-wakes the winit loop after a bounded diff drain.
    fn wake_loop(&self);

    /// Shuts the runtime down with a bounded wait.
    fn shutdown(self, timeout: Duration);
}

impl AppRuntime for IoRuntime {
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

    /// Updates the surface and viewport scale without semantic recompute.
    fn resize_surface(&mut self, size: PhysicalSize<u32>, scale_factor: f32);

    /// Recreates the physical atlas texture.
    fn recreate_atlas(&mut self, size: u32);

    /// Clears the path tessellation cache.
    fn clear_tessellation_cache(&mut self);

    /// Writes one atlas patch into the physical atlas texture.
    fn write_atlas_patch(&mut self, patch: &AtlasPatch);

    /// Rebuilds GPU buffers from the latest current scene buffer.
    fn rebuild_from_scene_buffer(&mut self, scene_buffer: &SceneBuffer);
}

impl AppRenderer for Renderer<'static> {
    fn frame(&mut self) {
        Renderer::frame(self);
    }

    fn resize_surface(&mut self, size: PhysicalSize<u32>, scale_factor: f32) {
        Renderer::resize_surface(self, size, scale_factor);
    }

    fn recreate_atlas(&mut self, size: u32) {
        Renderer::recreate_atlas(self, size);
    }

    fn clear_tessellation_cache(&mut self) {
        Renderer::clear_tessellation_cache(self);
    }

    fn write_atlas_patch(&mut self, patch: &AtlasPatch) {
        Renderer::write_atlas_patch(self, patch.region, &patch.pixels);
    }

    fn rebuild_from_scene_buffer(&mut self, scene_buffer: &SceneBuffer) {
        Renderer::rebuild_from_scene_buffer(self, scene_buffer);
    }
}
