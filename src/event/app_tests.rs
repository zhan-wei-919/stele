use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use winit::dpi::PhysicalSize;
use winit::window::WindowId;

use super::*;
use crate::event::handlers::ViewportUpdate;

#[derive(Debug, Default)]
struct RuntimeLog {
    scheduled_deadlines: Vec<Duration>,
    wake_count: usize,
    shutdown_timeouts: Vec<Duration>,
}

#[derive(Clone, Debug, Default)]
struct FakeRuntime {
    log: Arc<Mutex<RuntimeLog>>,
}

impl AppRuntime for FakeRuntime {
    fn schedule_deadline(&self, delay: Duration) {
        self.log
            .lock()
            .expect("runtime log must lock")
            .scheduled_deadlines
            .push(delay);
    }

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
}

#[derive(Clone, Debug, Default)]
struct FakeRenderer {
    log: Arc<Mutex<RendererLog>>,
}

impl AppRenderer for FakeRenderer {
    fn frame(&mut self) {
        self.log.lock().expect("renderer log must lock").frame_calls += 1;
    }

    fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f32) {
        self.log
            .lock()
            .expect("renderer log must lock")
            .resize_calls
            .push((size, scale_factor));
    }
}

#[derive(Debug, Default)]
struct DemoLog {
    resized_viewports: Vec<[f32; 2]>,
    apply_calls: usize,
}

#[derive(Clone, Debug, Default)]
struct FakeDemo {
    log: Arc<Mutex<DemoLog>>,
}

impl AppDemo<FakeRenderer> for FakeDemo {
    fn resize(&mut self, viewport: [f32; 2]) {
        self.log
            .lock()
            .expect("demo log must lock")
            .resized_viewports
            .push(viewport);
    }

    fn apply(&self, _renderer: &mut FakeRenderer) {
        self.log.lock().expect("demo log must lock").apply_calls += 1;
    }
}

type TestApp = SteleApp<FakeRuntime, FakeWindow, FakeRenderer, FakeDemo>;

struct Harness {
    app: TestApp,
    runtime_log: Arc<Mutex<RuntimeLog>>,
    window_log: Arc<Mutex<WindowLog>>,
    renderer_log: Arc<Mutex<RendererLog>>,
    demo_log: Arc<Mutex<DemoLog>>,
    io_event_tx: mpsc::UnboundedSender<IoEvent>,
}

fn build_app() -> Harness {
    let runtime = FakeRuntime::default();
    let runtime_log = runtime.log.clone();
    let (command_tx, _command_rx) = mpsc::unbounded_channel();
    let router = EventRouter::new(command_tx);
    let (io_event_tx, io_event_rx) = mpsc::unbounded_channel();
    let io_driver = IoEventDriver::new(io_event_rx);
    let mut app = SteleApp::new(runtime, io_driver, router, REDRAW_MIN_INTERVAL);

    let (window, window_log) = FakeWindow::new();
    let renderer = FakeRenderer::default();
    let renderer_log = renderer.log.clone();
    let demo = FakeDemo::default();
    let demo_log = demo.log.clone();
    app.attach_surface(window, renderer, demo);

    Harness {
        app,
        runtime_log,
        window_log,
        renderer_log,
        demo_log,
        io_event_tx,
    }
}

#[test]
fn resize_action_updates_renderer_demo_and_redraw() {
    let mut harness = build_app();

    let should_exit = harness
        .app
        .apply_route_action(RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(800, 600),
            scale_factor: 2.0,
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
            .demo_log
            .lock()
            .expect("demo log must lock")
            .resized_viewports,
        vec![[400.0, 300.0]]
    );
    assert_eq!(
        harness
            .demo_log
            .lock()
            .expect("demo log must lock")
            .apply_calls,
        1
    );
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
    assert!(harness.app.demo.is_none());
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
fn first_wake_requests_immediate_redraw() {
    let mut harness = build_app();
    harness
        .io_event_tx
        .send(IoEvent::MockTick {
            payload: String::from("alpha"),
        })
        .expect("send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .redraw_requests,
        1
    );
    assert!(harness
        .runtime_log
        .lock()
        .expect("runtime log must lock")
        .scheduled_deadlines
        .is_empty());
}

#[test]
fn second_wake_inside_interval_defers_until_deadline() {
    let mut harness = build_app();
    harness
        .io_event_tx
        .send(IoEvent::MockTick {
            payload: String::from("first"),
        })
        .expect("first send must succeed");
    assert!(!harness.app.on_wake());

    harness
        .io_event_tx
        .send(IoEvent::MockTick {
            payload: String::from("second"),
        })
        .expect("second send must succeed");
    assert!(!harness.app.on_wake());

    let scheduled = harness
        .runtime_log
        .lock()
        .expect("runtime log must lock")
        .scheduled_deadlines
        .clone();
    assert_eq!(scheduled.len(), 1);
    assert!(scheduled[0] <= REDRAW_MIN_INTERVAL);

    harness.app.on_deadline();
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .redraw_requests,
        2
    );
}
