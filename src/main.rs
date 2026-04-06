mod font;
mod renderer;

use std::sync::Arc;

use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::renderer::{
    DrawListOp, ImageCmd, ImageData, LineCap, LineJoin, PathCmd, PathVerb, PositionedGlyph,
    RectCmd, RenderLayer, Renderer, StrokeStyle,
};
use pollster::block_on;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const WINDOW_WIDTH: f64 = 800.0;
const WINDOW_HEIGHT: f64 = 600.0;
const FONT_SIZE: f32 = 14.0;
const PADDING_X: f32 = 24.0;
const PADDING_Y: f32 = 24.0;
const TEXT_COLOR: [f32; 4] = [0.92, 0.92, 0.92, 1.0];
const PANEL_BG: [f32; 4] = [0.07, 0.11, 0.17, 1.0];
const UNDERLINE_COLOR: [f32; 4] = [0.94, 0.23, 0.27, 1.0];
const CURSOR_COLOR: [f32; 4] = [0.96, 0.96, 0.96, 0.85];
const HARD_CODED_TEXT: [&str; 3] = [
    "Hello, Stele! — Pixel-perfect terminal.",
    "你好世界 — CJK text rendering test.",
    "ABCDabcd 1234 !@#$ mixed content.",
];

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
        let renderer = block_on(init_renderer(window.clone()));

        self.window_id = Some(window.id());
        self.renderer = Some(renderer);
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
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size, window.scale_factor() as f32);
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(window.inner_size(), scale_factor as f32);
                    window.request_redraw();
                }
            }
            WindowEvent::CloseRequested => {
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

async fn init_renderer(window: Arc<Window>) -> Renderer<'static> {
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
    let draw_ops = build_hardcoded_draw_list(&rasterizer);

    let mut renderer = Renderer::new(
        device,
        queue,
        surface,
        surface_config,
        rasterizer,
        window.scale_factor() as f32,
    );
    renderer.apply_ops(draw_ops);
    renderer
}

fn build_hardcoded_draw_list(rasterizer: &FreeTypeRasterizer) -> Vec<DrawListOp> {
    let font_id = rasterizer.default_font_id();
    let metrics = rasterizer.line_metrics(font_id, FONT_SIZE);
    let mut y_offset = PADDING_Y;
    let image = build_demo_image();
    let mut ops = vec![
        DrawListOp::SetRects(build_demo_rects(metrics.ascent, metrics.line_height)),
        DrawListOp::SetPaths(build_demo_paths()),
        DrawListOp::SetImages(build_demo_images(image)),
    ];

    for (line_index, text) in HARD_CODED_TEXT.into_iter().enumerate() {
        let glyphs = rasterizer
            .layout_line(text, font_id, FONT_SIZE, PADDING_X, y_offset)
            .into_iter()
            .map(|glyph| PositionedGlyph {
                font_id,
                glyph_id: glyph.glyph_id,
                font_size: FONT_SIZE,
                pos: glyph.pos,
                color: TEXT_COLOR,
                subpixel_offset: glyph.subpixel_offset,
            })
            .collect();
        y_offset += metrics.line_height;
        ops.push(DrawListOp::Insert { line_index, glyphs });
    }

    ops
}

fn build_demo_rects(ascent: f32, line_height: f32) -> Vec<RectCmd> {
    vec![
        RectCmd::new(
            [12.0, 12.0],
            [WINDOW_WIDTH as f32 - 24.0, line_height * 3.2],
            PANEL_BG,
            RenderLayer::Background,
        ),
        RectCmd::new(
            [PADDING_X, PADDING_Y + ascent + 4.0],
            [250.0, 2.0],
            UNDERLINE_COLOR,
            RenderLayer::Foreground,
        ),
        RectCmd::new(
            [PADDING_X + 252.0, PADDING_Y + 4.0],
            [2.0, line_height],
            CURSOR_COLOR,
            RenderLayer::Overlay,
        ),
    ]
}

fn build_demo_paths() -> Vec<PathCmd> {
    // The M0 demo intentionally instantiates line, quadratic, and cubic paths
    // plus every cap/join style once, so the whole primitive surface is exercised
    // by `cargo run` instead of existing only as future-facing API shape.
    vec![
        PathCmd::new(
            vec![
                PathVerb::MoveTo { to: [100.0, 185.0] },
                PathVerb::LineTo { to: [400.0, 185.0] },
            ],
            None,
            Some(StrokeStyle::new(
                [1.0, 1.0, 1.0, 1.0],
                2.0,
                LineCap::Butt,
                LineJoin::Bevel,
            )),
            RenderLayer::Foreground,
        ),
        PathCmd::new(
            vec![
                PathVerb::MoveTo { to: [90.0, 260.0] },
                PathVerb::CubicTo {
                    ctrl1: [180.0, 170.0],
                    ctrl2: [310.0, 350.0],
                    to: [430.0, 250.0],
                },
            ],
            None,
            Some(StrokeStyle::new(
                [0.28, 0.85, 0.45, 1.0],
                2.0,
                LineCap::Square,
                LineJoin::Miter,
            )),
            RenderLayer::Content,
        ),
        PathCmd::new(
            vec![
                PathVerb::MoveTo { to: [475.0, 220.0] },
                PathVerb::QuadTo {
                    ctrl: [605.0, 145.0],
                    to: [725.0, 235.0],
                },
            ],
            None,
            Some(StrokeStyle::new(
                [0.98, 0.78, 0.24, 1.0],
                2.0,
                LineCap::Round,
                LineJoin::Round,
            )),
            RenderLayer::Content,
        ),
        PathCmd::new(
            vec![
                PathVerb::MoveTo { to: [500.0, 320.0] },
                PathVerb::LineTo { to: [720.0, 365.0] },
                PathVerb::LineTo { to: [565.0, 520.0] },
                PathVerb::Close,
            ],
            Some([0.2, 0.4, 0.8, 0.5]),
            Some(StrokeStyle::new(
                [0.96, 0.97, 0.99, 1.0],
                1.0,
                LineCap::Round,
                LineJoin::Bevel,
            )),
            RenderLayer::Content,
        ),
    ]
}

fn build_demo_images(image: Arc<ImageData>) -> Vec<ImageCmd> {
    vec![
        ImageCmd::new(
            [160.0, 335.0],
            [128.0, 128.0],
            image.clone(),
            RenderLayer::Content,
        ),
        ImageCmd::new([310.0, 360.0], [96.0, 96.0], image, RenderLayer::Content),
    ]
}

fn build_demo_image() -> Arc<ImageData> {
    let width = 64u32;
    let height = 64u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            let border = x < 4 || y < 4 || x >= width - 4 || y >= height - 4;
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            let color = if border {
                [255, 255, 255, 255]
            } else if checker {
                [52, 171, 220, 255]
            } else {
                [246, 143, 84, 255]
            };
            rgba.extend_from_slice(&color);
        }
    }

    Arc::new(ImageData::new(rgba, width, height))
}
