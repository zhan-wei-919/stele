//! Desktop app shell that coordinates routed window events and async view-update apply.

use std::sync::Arc;
use std::time::Duration;

use log::{info, warn};
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::window::{Window, WindowId};

#[path = "app_support.rs"]
mod support;
#[path = "app_updates.rs"]
mod updates;

use crate::event::{EventRouter, RouteAction, ViewportSnapshot};
use crate::io::{IoRuntime, ViewUpdate, ViewUpdateDriver};
use crate::renderer::Renderer;
use crate::scene::{SceneBuffer, SceneConfig, ScenePipeline, SceneProtocolState};
pub(crate) use support::{AppRenderer, AppRuntime, AppWindow};

const MAX_VIEW_UPDATE_DRAIN: usize = 4096;
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100);

pub(crate) type DesktopApp = SteleApp<IoRuntime, Arc<Window>, Renderer<'static>>;
type StoreBootstrap<Rt> = Box<dyn FnOnce(&Rt, PhysicalSize<u32>, f32)>;

/// Owns desktop app state and applies routed window or view-update actions.
pub(crate) struct SteleApp<Rt, Win, Rend>
where
    Rt: AppRuntime,
    Win: AppWindow,
    Rend: AppRenderer,
{
    window: Option<Win>,
    window_id: Option<WindowId>,
    renderer: Option<Rend>,
    io_runtime: Option<Rt>,
    view_update_driver: Option<ViewUpdateDriver>,
    router: Option<EventRouter>,
    scene_pipeline: Option<ScenePipeline>,
    scene_protocol: SceneProtocolState,
    current_scene_buffer: Option<Box<SceneBuffer>>,
    scene_config: SceneConfig,
    store_bootstrap: Option<StoreBootstrap<Rt>>,
    shutting_down: bool,
}

impl<Rt, Win, Rend> SteleApp<Rt, Win, Rend>
where
    Rt: AppRuntime,
    Win: AppWindow,
    Rend: AppRenderer,
{
    /// Creates an app shell around the store runtime, view-update driver, and router.
    pub(crate) fn new(
        io_runtime: Rt,
        view_update_driver: ViewUpdateDriver,
        router: EventRouter,
        scene_pipeline: ScenePipeline,
        scene_config: SceneConfig,
    ) -> Self {
        Self {
            window: None,
            window_id: None,
            renderer: None,
            io_runtime: Some(io_runtime),
            view_update_driver: Some(view_update_driver),
            router: Some(router),
            scene_pipeline: Some(scene_pipeline),
            scene_protocol: SceneProtocolState::new(),
            current_scene_buffer: None,
            scene_config,
            store_bootstrap: None,
            shutting_down: false,
        }
    }

    /// Installs a one-shot store bootstrap that runs after the window exists.
    pub(crate) fn install_store_bootstrap(&mut self, bootstrap: StoreBootstrap<Rt>) {
        self.store_bootstrap = Some(bootstrap);
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
    pub(crate) fn attach_surface(&mut self, window: Win, renderer: Rend) {
        self.window_id = Some(window.id());
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    /// Starts the async store once the real window metrics are known.
    pub(crate) fn bootstrap_store(&mut self, size: PhysicalSize<u32>, scale_factor: f32) {
        let Some(io_runtime) = self.io_runtime.as_ref() else {
            return;
        };
        let Some(bootstrap) = self.store_bootstrap.take() else {
            return;
        };
        bootstrap(io_runtime, size, scale_factor);
    }

    /// Routes and applies one window event, returning whether the event loop should exit.
    pub(crate) fn on_window_event(&mut self, window_id: WindowId, event: &WindowEvent) -> bool {
        if self.shutting_down || Some(window_id) != self.window_id {
            return false;
        }

        let Some(window) = self.window.as_ref() else {
            return false;
        };
        let Some(router) = self.router.as_mut() else {
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
                self.handle_resize(update.size, update.scale_factor, update.viewport_revision);
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
        let Some(view_update_driver) = self.view_update_driver.as_mut() else {
            return false;
        };
        let outcome = view_update_driver.on_wake(MAX_VIEW_UPDATE_DRAIN);
        info!("view.wake drained={}", outcome.drained);

        if outcome.disconnected {
            return self.begin_shutdown();
        }

        let mut rebuild_needed = false;
        for update in outcome.updates {
            match update {
                ViewUpdate::Atlas(atlas_update) => {
                    self.apply_atlas_update(atlas_update);
                    rebuild_needed |= self.apply_pending_scene_buffer_if_ready();
                }
                ViewUpdate::Scene(scene_frame) => {
                    rebuild_needed |= self.handle_scene_frame(scene_frame);
                }
            }
        }

        if rebuild_needed {
            self.rebuild_current_scene();
        }

        if outcome.wake_again {
            warn!(
                "view.drain_overflow count={} limit={}",
                outcome.drained, MAX_VIEW_UPDATE_DRAIN
            );
            if let Some(io_runtime) = self.io_runtime.as_ref() {
                io_runtime.wake_loop();
            }
        }

        false
    }

    /// Shuts the runtime down during app exit.
    pub(crate) fn on_exit(&mut self) {
        self.drop_scene_transport();
        self.shutdown_runtime();
    }

    /// Starts shutdown exactly once and reports whether it transitioned state.
    pub(crate) fn begin_shutdown(&mut self) -> bool {
        if self.shutting_down {
            return false;
        }

        self.shutting_down = true;
        if let Some(current_scene_buffer) = self.current_scene_buffer.take() {
            self.retire_scene_buffer(current_scene_buffer, "shutdown");
        }
        if let Some(pending_scene_buffer) = self.scene_protocol.take_pending_scene_buffer() {
            self.retire_scene_buffer(pending_scene_buffer, "shutdown");
        }
        self.drop_scene_transport();
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

    fn handle_resize(
        &mut self,
        size: PhysicalSize<u32>,
        scale_factor: f32,
        viewport_revision: u64,
    ) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        renderer.resize_surface(size, scale_factor);
        if let Some(stale_pending) = self
            .scene_protocol
            .set_requested_viewport_revision(viewport_revision)
        {
            self.retire_scene_buffer(stale_pending, "stale_revision");
        }

        if size.width > 0 && size.height > 0 {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn rebuild_current_scene(&mut self) {
        let Some(scene_buffer) = self.current_scene_buffer.as_deref() else {
            return;
        };
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let rebuild_started = std::time::Instant::now();
        renderer.rebuild_from_scene_buffer(scene_buffer);
        let elapsed = rebuild_started.elapsed();
        if elapsed > duration_budget(self.scene_config.rebuild_budget_ms) {
            warn!(
                "scene.budget_exceeded phase=rebuild elapsed_us={} limit_ms={}",
                elapsed.as_micros(),
                self.scene_config.rebuild_budget_ms
            );
        }

        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        if size.width > 0 && size.height > 0 {
            window.request_redraw();
        }
    }

    fn shutdown_runtime(&mut self) {
        if let Some(io_runtime) = self.io_runtime.take() {
            io_runtime.shutdown(RUNTIME_SHUTDOWN_TIMEOUT);
        }
    }

    fn drop_scene_transport(&mut self) {
        self.view_update_driver = None;
        self.scene_pipeline = None;
    }

    fn retire_scene_buffer(&self, scene_buffer: Box<SceneBuffer>, reason: &'static str) {
        info!("view.retire reason={reason}");
        if let Some(scene_pipeline) = self.scene_pipeline.as_ref() {
            scene_pipeline.retire(scene_buffer);
        }
    }
}

fn duration_budget(limit_ms: u32) -> Duration {
    Duration::from_millis(u64::from(limit_ms))
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
