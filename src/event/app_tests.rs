use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use winit::dpi::PhysicalSize;
use winit::window::WindowId;

use super::*;
use crate::draw_list::ClipRect;
use crate::event::handlers::ViewportUpdate;
use crate::io::{
    Action, AtlasPatch, AtlasUpdate, SceneFrame, ScenePayload, ViewUpdate, ViewUpdateDriver,
};
use crate::renderer::atlas::AtlasRegion;
use crate::scene::{BlockId, BlockSceneBatch, ViewState};

#[derive(Debug, Default)]
struct RuntimeLog {
    wake_count: usize,
    shutdown_timeouts: Vec<Duration>,
}

#[derive(Clone, Debug, Default)]
struct FakeRuntime {
    log: Arc<Mutex<RuntimeLog>>,
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
struct WindowLog {
    redraw_requests: usize,
    pre_present_notify_calls: usize,
}

#[derive(Clone, Debug)]
struct FakeWindow {
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
struct RendererLog {
    frame_calls: usize,
    resize_calls: Vec<(PhysicalSize<u32>, f32)>,
    recreate_atlas_sizes: Vec<u32>,
    clear_tessellation_calls: usize,
    atlas_patch_writes: usize,
    rebuild_block_counts: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
struct FakeRenderer {
    log: Arc<Mutex<RendererLog>>,
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

type TestApp = SteleApp<FakeRuntime, FakeWindow, FakeRenderer>;

struct Harness {
    app: TestApp,
    runtime_log: Arc<Mutex<RuntimeLog>>,
    window_log: Arc<Mutex<WindowLog>>,
    renderer_log: Arc<Mutex<RendererLog>>,
    view_update_tx: mpsc::UnboundedSender<ViewUpdate>,
}

fn build_app() -> Harness {
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

#[test]
fn resize_action_updates_renderer_and_requests_redraw() {
    let mut harness = build_app();

    let should_exit = harness
        .app
        .apply_route_action(RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(800, 600),
            scale_factor: 2.0,
            viewport_revision: 1,
        }));

    assert!(!should_exit);
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .resize_calls,
        vec![(PhysicalSize::new(800, 600), 2.0)]
    );
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .redraw_requests,
        1
    );
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 1);
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 0);
}

#[test]
fn redraw_action_notifies_window_and_frames_renderer() {
    let mut harness = build_app();

    let should_exit = harness.app.apply_route_action(RouteAction::RedrawRequested);

    assert!(!should_exit);
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .pre_present_notify_calls,
        1
    );
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .frame_calls,
        1
    );
}

#[test]
fn close_action_clears_state_and_shuts_down_runtime() {
    let mut harness = build_app();

    let should_exit = harness.app.apply_route_action(RouteAction::CloseRequested);

    assert!(should_exit);
    assert!(harness.app.shutting_down);
    assert!(harness.app.window.is_none());
    assert!(harness.app.renderer.is_none());
    assert!(harness.app.router.is_none());
    assert!(harness.app.io_runtime.is_none());
    assert_eq!(
        harness
            .runtime_log
            .lock()
            .expect("runtime log must lock")
            .shutdown_timeouts,
        vec![RUNTIME_SHUTDOWN_TIMEOUT]
    );
}

#[test]
fn wake_applies_atlas_update_and_scene_frame_and_rebuilds_once() {
    let mut harness = build_app();
    let patch = AtlasPatch::new(
        AtlasRegion {
            uv_min: [0.0, 0.0],
            uv_max: [0.25, 0.25],
            size: [8.0, 8.0],
            bearing: [0.0, 0.0],
        },
        vec![255; 8 * 8 * 4],
    );
    harness
        .view_update_tx
        .send(ViewUpdate::Atlas({
            let mut atlas_update = AtlasUpdate::new(0);
            atlas_update.requested_atlas_size = Some(4096);
            atlas_update.patches.push(patch);
            atlas_update
        }))
        .expect("atlas update send must succeed");
    harness
        .view_update_tx
        .send(ViewUpdate::Scene({
            let mut scene_frame = SceneFrame::new(
                1,
                Some(0),
                ScenePayload::ReplaceAll {
                    block_order: vec![BlockId::new(7)],
                    block_batches: vec![(BlockId::new(7), sample_batch(99))],
                },
            );
            scene_frame.clear_tessellation_cache = true;
            scene_frame
        }))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert_eq!(harness.app.view_state.blocks().len(), 1);
    assert_eq!(harness.app.view_state.block_order(), &[BlockId::new(7)]);
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 1);
    assert_eq!(harness.app.view_state.ready_atlas_generation(), Some(0));
    let renderer_log = harness.renderer_log.lock().expect("renderer log must lock");
    assert_eq!(renderer_log.recreate_atlas_sizes, vec![4096]);
    assert_eq!(renderer_log.clear_tessellation_calls, 1);
    assert_eq!(renderer_log.atlas_patch_writes, 1);
    assert_eq!(renderer_log.rebuild_block_counts, vec![1]);
    drop(renderer_log);
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .redraw_requests,
        1
    );
}

#[test]
fn stale_scene_frame_is_dropped_before_apply() {
    let mut harness = build_app();
    harness.app.view_state.set_requested_viewport_revision(2);
    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            1,
            None,
            ScenePayload::ReplaceAll {
                block_order: vec![BlockId::new(7)],
                block_batches: vec![(BlockId::new(7), sample_batch(99))],
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert!(harness
        .renderer_log
        .lock()
        .expect("renderer log must lock")
        .rebuild_block_counts
        .is_empty());
}

#[test]
fn stale_scene_frame_is_dropped_after_newer_resize_arrives() {
    let mut harness = build_app();
    harness
        .app
        .apply_route_action(RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(1024, 768),
            scale_factor: 2.0,
            viewport_revision: 2,
        }));

    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            1,
            None,
            ScenePayload::ReplaceAll {
                block_order: vec![BlockId::new(7)],
                block_batches: vec![(BlockId::new(7), sample_batch(99))],
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 0);
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 2);
    assert!(harness
        .renderer_log
        .lock()
        .expect("renderer log must lock")
        .rebuild_block_counts
        .is_empty());
}

#[test]
fn newer_viewport_revision_replace_all_clears_old_scene_before_apply() {
    let mut harness = build_app();
    harness.app.view_state.set_block_order(vec![BlockId::new(7)]);
    harness
        .app
        .view_state
        .replace_block(BlockId::new(7), sample_batch(99));
    harness.app.view_state.set_applied_viewport_revision(1);
    harness.app.view_state.set_requested_viewport_revision(2);

    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            2,
            None,
            ScenePayload::ReplaceAll {
                block_order: Vec::new(),
                block_batches: Vec::new(),
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert!(harness.app.view_state.block_order().is_empty());
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 2);
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 2);
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .rebuild_block_counts,
        vec![0]
    );
}

#[test]
fn scene_frame_waits_for_required_atlas_generation_before_apply() {
    let mut harness = build_app();
    harness
        .app
        .apply_route_action(RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(1024, 768),
            scale_factor: 2.0,
            viewport_revision: 1,
        }));

    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            1,
            Some(1),
            ScenePayload::ReplaceAll {
                block_order: vec![BlockId::new(9)],
                block_batches: vec![(BlockId::new(9), sample_batch(90))],
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert!(harness.app.view_state.pending_scene_frame().is_some());
    assert!(harness
        .renderer_log
        .lock()
        .expect("renderer log must lock")
        .rebuild_block_counts
        .is_empty());

    harness
        .view_update_tx
        .send(ViewUpdate::Atlas({
            let mut atlas_update = AtlasUpdate::new(1);
            atlas_update.patches.push(sample_patch());
            atlas_update
        }))
        .expect("atlas update send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert_eq!(harness.app.view_state.block_order(), &[BlockId::new(9)]);
    assert_eq!(harness.app.view_state.blocks().len(), 1);
    assert!(harness.app.view_state.blocks().contains_key(&BlockId::new(9)));
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 1);
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 1);
    assert!(harness.app.view_state.pending_scene_frame().is_none());
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .rebuild_block_counts,
        vec![1]
    );
}

#[test]
#[ignore = "manual perf smoke test"]
fn reports_view_dispatch_to_render_perf() {
    let mut harness = build_app();
    let scene_frame = SceneFrame::new(
        1,
        None,
        ScenePayload::ReplaceAll {
            block_order: vec![BlockId::new(7)],
            block_batches: vec![(BlockId::new(7), sample_batch(99))],
        },
    );

    let dispatch_started = Instant::now();
    harness
        .view_update_tx
        .send(ViewUpdate::Scene(scene_frame))
        .expect("scene frame send must succeed");

    assert!(!harness.app.on_wake());
    let applied_at = Instant::now();

    assert!(!harness.app.apply_route_action(RouteAction::RedrawRequested));
    let frame_finished = Instant::now();

    println!(
        "perf.view dispatch_to_apply_us={} apply_to_frame_us={} dispatch_to_frame_us={}",
        applied_at.duration_since(dispatch_started).as_micros(),
        frame_finished.duration_since(applied_at).as_micros(),
        frame_finished.duration_since(dispatch_started).as_micros(),
    );
}

fn sample_batch(fingerprint: u64) -> BlockSceneBatch {
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

fn sample_patch() -> AtlasPatch {
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
