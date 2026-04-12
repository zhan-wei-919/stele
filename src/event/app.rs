//! Desktop app shell that coordinates routed window events and SceneDiff apply.

use std::sync::Arc;
use std::time::Duration;

use log::{info, warn};
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::window::{Window, WindowId};

#[path = "app_support.rs"]
mod support;

use crate::event::{EventRouter, RouteAction, ViewportSnapshot};
use crate::io::{BlockOp, IoRuntime, SceneDiff, SceneDiffDriver};
use crate::renderer::Renderer;
use crate::scene::ViewState;
pub(crate) use support::{AppRenderer, AppRuntime, AppWindow};

const MAX_SCENE_DIFF_DRAIN: usize = 4096;
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100);

pub(crate) type DesktopApp = SteleApp<IoRuntime, Arc<Window>, Renderer<'static>>;
type StoreBootstrap<Rt> = Box<dyn FnOnce(&Rt, PhysicalSize<u32>, f32)>;

/// Owns desktop app state and applies routed window or scene-diff actions.
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
    scene_diff_driver: SceneDiffDriver,
    router: Option<EventRouter>,
    view_state: ViewState,
    store_bootstrap: Option<StoreBootstrap<Rt>>,
    shutting_down: bool,
}

impl<Rt, Win, Rend> SteleApp<Rt, Win, Rend>
where
    Rt: AppRuntime,
    Win: AppWindow,
    Rend: AppRenderer,
{
    /// Creates an app shell around the store runtime, diff driver, and router.
    pub(crate) fn new(
        io_runtime: Rt,
        scene_diff_driver: SceneDiffDriver,
        router: EventRouter,
    ) -> Self {
        Self {
            window: None,
            window_id: None,
            renderer: None,
            io_runtime: Some(io_runtime),
            scene_diff_driver,
            router: Some(router),
            view_state: ViewState::new(),
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
        let outcome = self.scene_diff_driver.on_wake(MAX_SCENE_DIFF_DRAIN);
        info!("view.wake drained={}", outcome.drained);

        if outcome.disconnected {
            return self.begin_shutdown();
        }

        let mut applied_any = false;
        for diff in outcome.diffs {
            applied_any |= self.apply_scene_diff(diff);
        }

        if applied_any {
            self.rebuild_view_state();
        }

        if outcome.wake_again {
            warn!(
                "view.drain_overflow count={} limit={}",
                outcome.drained, MAX_SCENE_DIFF_DRAIN
            );
            if let Some(io_runtime) = self.io_runtime.as_ref() {
                io_runtime.wake_loop();
            }
        }

        false
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
        self.view_state
            .set_requested_viewport_revision(viewport_revision);

        if size.width > 0 && size.height > 0 {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn apply_scene_diff(&mut self, diff: SceneDiff) -> bool {
        if diff.viewport_revision < self.view_state.requested_viewport_revision() {
            return false;
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return false;
        };
        if diff.viewport_revision > self.view_state.applied_viewport_revision() {
            self.view_state.clear_scene();
        }
        let patch_count = diff.atlas_patches.len();
        if let Some(new_size) = diff.requested_atlas_size {
            renderer.recreate_atlas(new_size);
        }
        if diff.clear_tessellation_cache {
            renderer.clear_tessellation_cache();
        }
        for patch in &diff.atlas_patches {
            renderer.write_atlas_patch(patch);
        }
        if let Some(block_order) = diff.block_order {
            self.view_state.set_block_order(block_order);
        }

        let mut replaced_blocks = 0usize;
        let mut removed_blocks = 0usize;
        for op in diff.block_ops {
            match op {
                BlockOp::Replace { block_id, batch } => {
                    self.view_state.replace_block(block_id, batch);
                    replaced_blocks += 1;
                }
                BlockOp::Remove { block_id } => {
                    self.view_state.remove_block(block_id);
                    removed_blocks += 1;
                }
            }
        }
        self.view_state
            .set_applied_viewport_revision(diff.viewport_revision);
        info!(
            "view.apply replaced_blocks={} removed_blocks={} atlas_patches={}",
            replaced_blocks, removed_blocks, patch_count
        );
        true
    }

    fn rebuild_view_state(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        renderer.rebuild_from_view_state(&self.view_state);

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
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
