use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use winit::dpi::PhysicalSize;
use winit::window::WindowId;

use super::super::{AppRenderer, AppRuntime, AppWindow, SteleApp};
use crate::draw_list::ClipRect;
use crate::event::EventRouter;
use crate::io::{Action, AtlasPatch, ViewUpdate, ViewUpdateDriver};
use crate::renderer::atlas::AtlasRegion;
use crate::scene::{BlockSceneBatch, ViewState};

#[derive(Debug, Default)]
pub(super) struct RuntimeLog {
    pub(super) wake_count: usize,
    pub(super) shutdown_timeouts: Vec<Duration>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct FakeRuntime {
    pub(super) log: Arc<Mutex<RuntimeLog>>,
}

impl AppRuntime for FakeRuntime {
    fn wake_loop(&self) {
        self.log.lock().expect("runtime log must lock").wake_count += 1;
    }

    fn shutdown(self, timeout: Duration) {
        self.log
            .lock()
            .expect("runtime log must lock")
            .shutdown_timeouts
            .push(timeout);
    }
}

#[derive(Debug, Default)]
pub(super) struct WindowLog {
    pub(super) redraw_requests: usize,
    pub(super) pre_present_notify_calls: usize,
}

#[derive(Clone, Debug)]
pub(super) struct FakeWindow {
    id: WindowId,
    log: Arc<Mutex<WindowLog>>,
}

impl FakeWindow {
    fn new() -> (Self, Arc<Mutex<WindowLog>>) {
        let log = Arc::new(Mutex::new(WindowLog::default()));
        (
            Self {
                id: WindowId::dummy(),
                log: log.clone(),
            },
            log,
        )
    }
}

impl AppWindow for FakeWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn request_redraw(&self) {
        self.log
            .lock()
            .expect("window log must lock")
            .redraw_requests += 1;
    }

    fn pre_present_notify(&self) {
        self.log
            .lock()
            .expect("window log must lock")
            .pre_present_notify_calls += 1;
    }

    fn inner_size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(1280, 720)
    }

    fn scale_factor(&self) -> f64 {
        2.0
    }
}

#[derive(Debug, Default)]
pub(super) struct RendererLog {
    pub(super) frame_calls: usize,
    pub(super) resize_calls: Vec<(PhysicalSize<u32>, f32)>,
    pub(super) recreate_atlas_sizes: Vec<u32>,
    pub(super) clear_tessellation_calls: usize,
    pub(super) atlas_patch_writes: usize,
    pub(super) rebuild_block_counts: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct FakeRenderer {
    pub(super) log: Arc<Mutex<RendererLog>>,
}

impl AppRenderer for FakeRenderer {
    fn frame(&mut self) {
        self.log.lock().expect("renderer log must lock").frame_calls += 1;
    }

    fn resize_surface(&mut self, size: PhysicalSize<u32>, scale_factor: f32) {
        self.log
            .lock()
            .expect("renderer log must lock")
            .resize_calls
            .push((size, scale_factor));
    }

    fn recreate_atlas(&mut self, size: u32) {
        self.log
            .lock()
            .expect("renderer log must lock")
            .recreate_atlas_sizes
            .push(size);
    }

    fn clear_tessellation_cache(&mut self) {
        self.log
            .lock()
            .expect("renderer log must lock")
            .clear_tessellation_calls += 1;
    }

    fn write_atlas_patch(&mut self, _patch: &AtlasPatch) {
        self.log
            .lock()
            .expect("renderer log must lock")
            .atlas_patch_writes += 1;
    }

    fn rebuild_from_view_state(&mut self, view_state: &ViewState) {
        self.log
            .lock()
            .expect("renderer log must lock")
            .rebuild_block_counts
            .push(view_state.blocks().len());
    }
}

pub(super) type TestApp = SteleApp<FakeRuntime, FakeWindow, FakeRenderer>;

pub(super) struct Harness {
    pub(super) app: TestApp,
    pub(super) runtime_log: Arc<Mutex<RuntimeLog>>,
    pub(super) window_log: Arc<Mutex<WindowLog>>,
    pub(super) renderer_log: Arc<Mutex<RendererLog>>,
    pub(super) view_update_tx: mpsc::UnboundedSender<ViewUpdate>,
}

pub(super) fn build_app() -> Harness {
    let runtime = FakeRuntime::default();
    let runtime_log = runtime.log.clone();
    let (action_tx, _action_rx) = mpsc::unbounded_channel::<Action>();
    let router = EventRouter::new(action_tx);
    let (view_update_tx, view_update_rx) = mpsc::unbounded_channel();
    let view_update_driver = ViewUpdateDriver::new(view_update_rx);
    let mut app = SteleApp::new(runtime, view_update_driver, router);

    let (window, window_log) = FakeWindow::new();
    let renderer = FakeRenderer::default();
    let renderer_log = renderer.log.clone();
    app.attach_surface(window, renderer);

    Harness {
        app,
        runtime_log,
        window_log,
        renderer_log,
        view_update_tx,
    }
}

pub(super) fn sample_batch(fingerprint: u64) -> BlockSceneBatch {
    BlockSceneBatch::new(
        ClipRect::new(0.0, 0.0, 100.0, 80.0),
        0,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        fingerprint,
    )
}

pub(super) fn sample_patch() -> AtlasPatch {
    AtlasPatch::new(
        AtlasRegion {
            uv_min: [0.0, 0.0],
            uv_max: [0.25, 0.25],
            size: [8.0, 8.0],
            bearing: [0.0, 0.0],
        },
        vec![255; 8 * 8 * 4],
    )
}
