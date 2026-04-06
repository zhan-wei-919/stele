mod font;
mod layout;
mod renderer;

use std::sync::Arc;

use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::layout::{
    bridge_layout, layout_document, prepare_document, Block, BlockRect, Document, PreparedBlock,
    Span, TextStyle,
};
use crate::renderer::Renderer;
use pollster::block_on;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const WINDOW_WIDTH: f64 = 960.0;
const WINDOW_HEIGHT: f64 = 640.0;
const PAGE_BG: [f32; 4] = [0.05, 0.08, 0.12, 1.0];
const PANEL_ACCENT_BG: [f32; 4] = [0.16, 0.21, 0.28, 0.98];
const TEXT_PRIMARY: [f32; 4] = [0.92, 0.95, 0.97, 1.0];
const TEXT_MUTED: [f32; 4] = [0.75, 0.80, 0.86, 1.0];
const TEXT_ACCENT: [f32; 4] = [0.94, 0.69, 0.28, 1.0];
const INLINE_BG: [f32; 4] = [0.19, 0.25, 0.33, 0.95];

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
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .expect("failed to request wgpu adapter");

    if !adapter
        .features()
        .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
    {
        panic!("GPU does not support dual-source blending, required for subpixel text rendering");
    }

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("stele.device"),
            required_features: wgpu::Features::DUAL_SOURCE_BLENDING,
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("failed to request wgpu device");

    let size = window.inner_size();
    let surface_config = surface
        .get_default_config(&adapter, size.width.max(1), size.height.max(1))
        .expect("surface is not supported by the selected adapter");

    let font_discovery = FontDiscovery::new().expect("failed to discover system fonts");
    let subpixel_layout = renderer::subpixel::detect_subpixel_layout();
    let rasterizer = FreeTypeRasterizer::new(font_discovery, subpixel_layout)
        .expect("failed to initialize FreeType rasterizer");
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

struct LayoutDemo {
    document: Document,
    prepared_blocks: Vec<PreparedBlock>,
}

impl LayoutDemo {
    fn new(rasterizer: &FreeTypeRasterizer, viewport: [f32; 2]) -> Self {
        let document = build_demo_document(rasterizer.default_font_id(), viewport);
        let prepared_blocks = prepare_document(&document, rasterizer);
        Self {
            document,
            prepared_blocks,
        }
    }

    fn resize(&mut self, viewport: [f32; 2]) {
        apply_demo_block_rects(&mut self.document, viewport);
    }

    fn apply(&self, renderer: &mut Renderer<'static>) {
        let layout_blocks = layout_document(&self.document, &self.prepared_blocks);
        renderer.apply_ops(bridge_layout(&layout_blocks));
    }
}

fn build_demo_document(font_id: u32, viewport: [f32; 2]) -> Document {
    let title = TextStyle::new(font_id, 26.0, TEXT_PRIMARY)
        .expect("demo title style must be valid")
        .with_bold(true);

    let badge = TextStyle::new(font_id, 14.0, TEXT_ACCENT)
        .expect("demo badge style must be valid")
        .with_underline(true)
        .with_letter_spacing(0.8)
        .expect("demo badge spacing must be valid")
        .with_background_color(Some(INLINE_BG))
        .expect("demo badge background must be valid");

    let body = TextStyle::new(font_id, 15.0, TEXT_MUTED).expect("demo body style must be valid");

    let overlay_title = TextStyle::new(font_id, 18.0, TEXT_PRIMARY)
        .expect("demo overlay title style must be valid")
        .with_italic(true);

    let overlay_body = TextStyle::new(font_id, 14.0, TEXT_PRIMARY)
        .expect("demo overlay body style must be valid")
        .with_strikethrough(true)
        .with_background_color(Some([0.24, 0.30, 0.38, 0.92]))
        .expect("demo overlay body background must be valid");

    let mut document = Document::new(vec![
        Block::new(
            BlockRect::new(0.0, 0.0, 1.0, 1.0).expect("demo root rect must be valid"),
            28.0,
            Some(PAGE_BG),
            vec![
                Span::new("Stele Layout Engine", title),
                Span::new(
                    "\n多 Block stacking、自动换行、baseline 对齐，以及 block clip 已经接入 renderer。\n",
                    body,
                ),
                Span::new("inline decoration sample", badge),
                Span::new(
                    " with mixed ASCII/CJK content. The quick brown fox jumps over the lazy dog, 你好世界ABC测试，长单词Supercalifragilisticexpialidocious也会按字符强制换行。",
                    body,
                ),
            ],
            0,
        )
        .expect("demo root block must be valid"),
        Block::new(
            BlockRect::new(0.0, 0.0, 1.0, 1.0).expect("demo overlay rect must be valid"),
            18.0,
            Some(PANEL_ACCENT_BG),
            vec![
                Span::new("Overlay Block", overlay_title),
                Span::new(
                    "\nThis block has its own clip rect and z-order. Resize the window to reflow text without rerunning prepare.",
                    overlay_body,
                ),
            ],
            1,
        )
        .expect("demo overlay block must be valid"),
    ]);
    apply_demo_block_rects(&mut document, viewport);
    document
}

fn apply_demo_block_rects(document: &mut Document, viewport: [f32; 2]) {
    let width = viewport[0].max(320.0);
    let height = viewport[1].max(240.0);

    document
        .set_block_rect(
            0,
            BlockRect::new(0.0, 0.0, width, height).expect("demo root rect must be valid"),
        )
        .expect("demo root block must exist");
    document
        .set_block_background_color(0, Some(PAGE_BG))
        .expect("demo root background must be valid");

    let overlay_width = width.min(360.0).max(220.0);
    let overlay_height = height.min(180.0).max(120.0);
    let overlay_x = (width - overlay_width - 32.0).max(24.0);
    let overlay_y = (height * 0.42)
        .min(height - overlay_height - 24.0)
        .max(24.0);
    document
        .set_block_rect(
            1,
            BlockRect::new(overlay_x, overlay_y, overlay_width, overlay_height)
                .expect("demo overlay rect must be valid"),
        )
        .expect("demo overlay block must exist");
    document
        .set_block_background_color(1, Some(PANEL_ACCENT_BG))
        .expect("demo overlay background must be valid");
}

fn logical_viewport(size: PhysicalSize<u32>, scale_factor: f32) -> [f32; 2] {
    [
        size.width as f32 / scale_factor.max(1.0),
        size.height as f32 / scale_factor.max(1.0),
    ]
}
