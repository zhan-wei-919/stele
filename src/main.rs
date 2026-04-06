mod demo;
mod font;
mod layout;
mod renderer;

use std::sync::Arc;

use crate::demo::LayoutDemo;
use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::renderer::Renderer;
use pollster::block_on;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const WINDOW_WIDTH: f64 = 960.0;
const WINDOW_HEIGHT: f64 = 640.0;

fn main() -> Result<(), winit::error::EventLoopError> {
    env_logger::init();

    let event_loop = EventLoop::new()?;
    let mut app = SteleApp::default();
    event_loop.run_app(&mut app)
}

#[derive(Default)]
struct SteleApp {
    window: Option<Arc<Window>>,
    window_id: Option<WindowId>,
    renderer: Option<Renderer<'static>>,
    demo: Option<LayoutDemo>,
}

impl ApplicationHandler for SteleApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
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
        let viewport = logical_viewport(window.inner_size(), window.scale_factor() as f32);
        let (renderer, demo) = block_on(init_renderer(window.clone(), viewport));

        self.window_id = Some(window.id());
        self.renderer = Some(renderer);
        self.demo = Some(demo);
        self.window = Some(window.clone());
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if Some(window_id) != self.window_id {
            return;
        }

        let Some(window) = self.window.as_ref() else {
            return;
        };

        match event {
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    window.pre_present_notify();
                    renderer.frame();
                }
            }
            WindowEvent::Resized(size) => {
                self.handle_resize(size, window.scale_factor() as f32);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.handle_resize(window.inner_size(), scale_factor as f32);
            }
            WindowEvent::CloseRequested => {
                self.demo = None;
                self.renderer = None;
                self.window = None;
                self.window_id = None;
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

impl SteleApp {
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
}

async fn init_renderer(window: Arc<Window>, viewport: [f32; 2]) -> (Renderer<'static>, LayoutDemo) {
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
    let rasterizer = build_rasterizer();
    let demo = LayoutDemo::new(&rasterizer, viewport);

    let mut renderer = Renderer::new(
        device,
        queue,
        surface,
        surface_config,
        rasterizer,
        window.scale_factor() as f32,
    );
    demo.apply(&mut renderer);
    (renderer, demo)
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

fn logical_viewport(size: PhysicalSize<u32>, scale_factor: f32) -> [f32; 2] {
    [
        size.width as f32 / scale_factor.max(1.0),
        size.height as f32 / scale_factor.max(1.0),
    ]
}
