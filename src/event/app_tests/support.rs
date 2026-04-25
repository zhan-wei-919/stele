use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bumpalo::Bump;
use tokio::sync::mpsc;
use winit::dpi::PhysicalSize;
use winit::window::WindowId;

use super::super::{AppRenderer, AppRuntime, AppWindow, SteleApp};
use crate::draw_list::ClipRect;
use crate::event::EventRouter;
use crate::io::{Action, AtlasPatch, UiEffectDriver, ViewUpdate, ViewUpdateDriver};
use crate::scene::instance::AtlasRegion;
use crate::scene::{
    BlockDataArena, BlockId, SceneBuffer, SceneBufferInner, SceneConfig, SceneFrameMetadata,
    ScenePipeline,
};
pub(super) use crate::test_support::log_capture::LogCapture;

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
    rebuild_delay: Arc<Mutex<Duration>>,
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

    fn rebuild_from_scene_buffer(&mut self, scene_buffer: &SceneBuffer) {
        let rebuild_delay = *self.rebuild_delay.lock().expect("renderer delay must lock");
        if !rebuild_delay.is_zero() {
            std::thread::sleep(rebuild_delay);
        }
        self.log
            .lock()
            .expect("renderer log must lock")
            .rebuild_block_counts
            .push(scene_buffer.blocks().len());
    }
}

pub(super) type TestApp = SteleApp<FakeRuntime, FakeWindow, FakeRenderer>;

pub(super) struct Harness {
    pub(super) app: TestApp,
    pub(super) runtime_log: Arc<Mutex<RuntimeLog>>,
    pub(super) window_log: Arc<Mutex<WindowLog>>,
    pub(super) renderer_log: Arc<Mutex<RendererLog>>,
    pub(super) view_update_tx: mpsc::Sender<ViewUpdate>,
    pub(super) return_rx: mpsc::Receiver<Bump>,
    rebuild_delay: Arc<Mutex<Duration>>,
}

pub(super) fn build_app() -> Harness {
    build_app_with_scene_config(SceneConfig::default())
}

pub(super) fn build_app_with_scene_config(scene_config: SceneConfig) -> Harness {
    let runtime = FakeRuntime::default();
    let runtime_log = runtime.log.clone();
    let (action_tx, _action_rx) = mpsc::unbounded_channel::<Action>();
    let router = EventRouter::new(action_tx);
    let (view_update_tx, view_update_rx) = mpsc::channel(4);
    let view_update_driver = ViewUpdateDriver::new(view_update_rx);
    let (_ui_effect_tx, ui_effect_rx) = mpsc::unbounded_channel();
    let ui_effect_driver = UiEffectDriver::new(ui_effect_rx);
    let (return_tx, return_rx) = mpsc::channel(3);
    let scene_pipeline = ScenePipeline::new(return_tx);
    let mut app = SteleApp::new(
        runtime,
        view_update_driver,
        ui_effect_driver,
        router,
        scene_pipeline,
        scene_config,
    );

    let (window, window_log) = FakeWindow::new();
    let renderer = FakeRenderer::default();
    let renderer_log = renderer.log.clone();
    let rebuild_delay = renderer.rebuild_delay.clone();
    app.attach_surface(window, renderer);

    Harness {
        app,
        runtime_log,
        window_log,
        renderer_log,
        view_update_tx,
        return_rx,
        rebuild_delay,
    }
}

impl Harness {
    pub(super) fn set_rebuild_delay(&self, rebuild_delay: Duration) {
        *self.rebuild_delay.lock().expect("renderer delay must lock") = rebuild_delay;
    }
}

pub(super) fn sample_scene_frame(
    viewport_revision: u64,
    required_atlas_generation: Option<u64>,
    block_ids: &[u64],
    clear_tessellation_cache: bool,
) -> crate::io::SceneFrame {
    sample_scene_frame_with_resize_started_at(
        viewport_revision,
        required_atlas_generation,
        block_ids,
        clear_tessellation_cache,
        None,
    )
}

pub(super) fn sample_scene_frame_with_resize_started_at(
    viewport_revision: u64,
    required_atlas_generation: Option<u64>,
    block_ids: &[u64],
    clear_tessellation_cache: bool,
    resize_started_at: Option<Instant>,
) -> crate::io::SceneFrame {
    crate::io::SceneFrame::new(sample_scene_buffer(
        viewport_revision,
        required_atlas_generation,
        block_ids,
        clear_tessellation_cache,
        resize_started_at,
    ))
}

pub(super) fn sample_scene_buffer(
    viewport_revision: u64,
    required_atlas_generation: Option<u64>,
    block_ids: &[u64],
    clear_tessellation_cache: bool,
    resize_started_at: Option<Instant>,
) -> Box<SceneBuffer> {
    let metadata = SceneFrameMetadata {
        viewport_revision,
        required_atlas_generation,
        clear_tessellation_cache,
        resize_started_at,
    };
    let buffer = SceneBuffer::new(Bump::with_capacity(4096), |owner| {
        let mut scene = SceneBufferInner::empty_in(owner, metadata);
        for block_id in block_ids {
            scene.order_mut().push(BlockId::new(*block_id));
            scene.blocks_mut().push(sample_block(owner, *block_id));
        }
        scene
    });
    Box::new(buffer)
}

fn sample_block<'a>(owner: &'a Bump, block_id: u64) -> BlockDataArena<'a> {
    BlockDataArena::new_in(
        owner,
        BlockId::new(block_id),
        ClipRect::new(0.0, 0.0, 100.0, 80.0),
        0,
        block_id,
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
