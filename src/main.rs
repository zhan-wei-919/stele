#[path = "event/app.rs"]
mod app;
mod demo;
mod draw_list;
mod event;
mod font;
mod io;
mod layout;
mod renderer;
mod scene;
mod store;

use std::error::Error;
use std::sync::Arc;

use self::app::DesktopApp;
use self::event::EventRouter;
use self::font::{FontDiscovery, FreeTypeRasterizer};
use self::io::{IoRuntime, SceneDiffDriver, WakeEvent};
use self::renderer::Renderer;
use self::store::{run_store, Store, ViewportState};
use pollster::block_on;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const WINDOW_WIDTH: f64 = 960.0;
const WINDOW_HEIGHT: f64 = 640.0;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let event_loop = EventLoop::<WakeEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let (mut io_runtime, io_handle) = IoRuntime::new(proxy)?;
    let scene_diff_driver = SceneDiffDriver::new(io_runtime.take_scene_diff_rx());
    let router = EventRouter::new(io_runtime.action_tx());

    let mut app = DesktopApp::new(io_runtime, scene_diff_driver, router);
    app.install_store_bootstrap(Box::new(move |runtime: &IoRuntime, size, scale_factor| {
        let rasterizer = build_rasterizer();
        let store = Store::new(
            rasterizer,
            ViewportState::new(size.width, size.height, scale_factor, 0),
        );
        runtime.spawn_task(run_store(store, io_handle));
    }));
    event_loop.run_app(&mut app)?;
    Ok(())
}

impl ApplicationHandler<WakeEvent> for DesktopApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.should_skip_resume() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Stele")
            .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("failed to create window"),
        );
        let renderer = block_on(init_renderer(window.clone()));

        self.attach_surface(window.clone(), renderer);
        self.bootstrap_store(window.inner_size(), window.scale_factor() as f32);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: WakeEvent) {
        if self.is_shutting_down() {
            return;
        }

        match event {
            WakeEvent::Wake => {
                if self.on_wake() {
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.on_window_event(window_id, &event) {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.on_exit();
    }
}

async fn init_renderer(window: Arc<Window>) -> Renderer<'static> {
    let instance = wgpu::Instance::default();
    let surface = instance
        .create_surface(window.clone())
        .expect("failed to create wgpu surface");
    let adapter = request_adapter(&instance, &surface).await;
    assert_dual_source_blending(&adapter);
    let (device, queue) = request_device(&adapter).await;
    let size = window.inner_size();
    let surface_config = surface
        .get_default_config(&adapter, size.width.max(1), size.height.max(1))
        .expect("surface is not supported by the selected adapter");
    Renderer::new(
        device,
        queue,
        surface,
        surface_config,
        window.scale_factor() as f32,
    )
}

async fn request_adapter(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> wgpu::Adapter {
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
        })
        .await
        .expect("failed to request wgpu adapter")
}

fn assert_dual_source_blending(adapter: &wgpu::Adapter) {
    if !adapter
        .features()
        .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
    {
        panic!("GPU does not support dual-source blending, required for subpixel text rendering");
    }
}

async fn request_device(adapter: &wgpu::Adapter) -> (wgpu::Device, wgpu::Queue) {
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("stele.device"),
            required_features: wgpu::Features::DUAL_SOURCE_BLENDING,
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("failed to request wgpu device")
}

fn build_rasterizer() -> FreeTypeRasterizer {
    let font_discovery = FontDiscovery::new().expect("failed to discover system fonts");
    let subpixel_layout = renderer::subpixel::detect_subpixel_layout();
    FreeTypeRasterizer::new(font_discovery, subpixel_layout)
        .expect("failed to initialize FreeType rasterizer")
}
