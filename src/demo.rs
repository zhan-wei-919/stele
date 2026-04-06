//! Demo document assembly used by the desktop entry point.

use crate::font::FreeTypeRasterizer;
use crate::layout::{
    bridge_layout, layout_document, prepare_document, Block, BlockRect, Document, PreparedBlock,
    Span, TextStyle,
};
use crate::renderer::Renderer;

const PAGE_BG: [f32; 4] = [0.05, 0.08, 0.12, 1.0];
const PANEL_ACCENT_BG: [f32; 4] = [0.16, 0.21, 0.28, 0.98];
const TEXT_PRIMARY: [f32; 4] = [0.92, 0.95, 0.97, 1.0];
const TEXT_MUTED: [f32; 4] = [0.75, 0.80, 0.86, 1.0];
const TEXT_ACCENT: [f32; 4] = [0.94, 0.69, 0.28, 1.0];
const INLINE_BG: [f32; 4] = [0.19, 0.25, 0.33, 0.95];
const OVERLAY_INLINE_BG: [f32; 4] = [0.24, 0.30, 0.38, 0.92];

/// Prepared demo document reused across resize-driven relayouts.
pub(crate) struct LayoutDemo {
    document: Document,
    prepared_blocks: Vec<PreparedBlock>,
}

impl LayoutDemo {
    /// Builds the demo document once and keeps prepared blocks for relayout.
    pub(crate) fn new(rasterizer: &FreeTypeRasterizer, viewport: [f32; 2]) -> Self {
        let document = build_demo_document(rasterizer.default_font_id(), viewport);
        let prepared_blocks = prepare_document(&document, rasterizer);
        Self {
            document,
            prepared_blocks,
        }
    }

    /// Recomputes block rectangles without rerunning prepare.
    pub(crate) fn resize(&mut self, viewport: [f32; 2]) {
        apply_demo_block_rects(&mut self.document, viewport);
    }

    /// Bridges the current document layout into renderer ops.
    pub(crate) fn apply(&self, renderer: &mut Renderer<'static>) {
        let layout_blocks = layout_document(&self.document, &self.prepared_blocks);
        renderer.apply_ops(bridge_layout(&layout_blocks));
    }
}

#[derive(Clone, Copy)]
struct DemoStyles {
    title: TextStyle,
    badge: TextStyle,
    body: TextStyle,
    overlay_title: TextStyle,
    overlay_body: TextStyle,
}

fn build_demo_document(font_id: u32, viewport: [f32; 2]) -> Document {
    let styles = build_demo_styles(font_id);
    let mut document = Document::new(vec![
        build_root_block(&styles),
        build_overlay_block(&styles),
    ]);
    apply_demo_block_rects(&mut document, viewport);
    document
}

fn build_demo_styles(font_id: u32) -> DemoStyles {
    DemoStyles {
        title: TextStyle::new(font_id, 26.0, TEXT_PRIMARY)
            .expect("demo title style must be valid")
            .with_bold(true),
        badge: TextStyle::new(font_id, 14.0, TEXT_ACCENT)
            .expect("demo badge style must be valid")
            .with_underline(true)
            .with_letter_spacing(0.8)
            .expect("demo badge spacing must be valid")
            .with_background_color(Some(INLINE_BG))
            .expect("demo badge background must be valid"),
        body: TextStyle::new(font_id, 15.0, TEXT_MUTED).expect("demo body style must be valid"),
        overlay_title: TextStyle::new(font_id, 18.0, TEXT_PRIMARY)
            .expect("demo overlay title style must be valid")
            .with_italic(true),
        overlay_body: TextStyle::new(font_id, 14.0, TEXT_PRIMARY)
            .expect("demo overlay body style must be valid")
            .with_strikethrough(true)
            .with_background_color(Some(OVERLAY_INLINE_BG))
            .expect("demo overlay body background must be valid"),
    }
}

fn build_root_block(styles: &DemoStyles) -> Block {
    Block::new(
        unit_rect("demo root rect"),
        28.0,
        Some(PAGE_BG),
        vec![
            Span::new("Stele Layout Engine", styles.title),
            Span::new(
                "\n多 Block stacking、自动换行、baseline 对齐，以及 block clip 已经接入 renderer。\n",
                styles.body,
            ),
            Span::new("inline decoration sample", styles.badge),
            Span::new(
                " with mixed ASCII/CJK content. The quick brown fox jumps over the lazy dog, 你好世界ABC测试，长单词Supercalifragilisticexpialidocious也会按字符强制换行。",
                styles.body,
            ),
        ],
        0,
    )
    .expect("demo root block must be valid")
}

fn build_overlay_block(styles: &DemoStyles) -> Block {
    Block::new(
        unit_rect("demo overlay rect"),
        18.0,
        Some(PANEL_ACCENT_BG),
        vec![
            Span::new("Overlay Block", styles.overlay_title),
            Span::new(
                "\nThis block has its own clip rect and z-order. Resize the window to reflow text without rerunning prepare.",
                styles.overlay_body,
            ),
        ],
        1,
    )
    .expect("demo overlay block must be valid")
}

fn apply_demo_block_rects(document: &mut Document, viewport: [f32; 2]) {
    let width = viewport[0].max(320.0);
    let height = viewport[1].max(240.0);
    let overlay_width = width.clamp(220.0, 360.0);
    let overlay_height = height.clamp(120.0, 180.0);
    let overlay_x = (width - overlay_width - 32.0).max(24.0);
    let overlay_y = (height * 0.42)
        .min(height - overlay_height - 24.0)
        .max(24.0);

    set_block_rect(
        document,
        0,
        BlockRect::new(0.0, 0.0, width, height).expect("demo root rect must be valid"),
        "demo root",
    );
    set_block_rect(
        document,
        1,
        BlockRect::new(overlay_x, overlay_y, overlay_width, overlay_height)
            .expect("demo overlay rect must be valid"),
        "demo overlay",
    );
    set_block_background(document, 0, Some(PAGE_BG), "demo root");
    set_block_background(document, 1, Some(PANEL_ACCENT_BG), "demo overlay");
}

fn set_block_rect(document: &mut Document, block_index: usize, rect: BlockRect, label: &str) {
    document
        .set_block_rect(block_index, rect)
        .unwrap_or_else(|_| panic!("{label} block must exist"));
}

fn set_block_background(
    document: &mut Document,
    block_index: usize,
    background: Option<[f32; 4]>,
    label: &str,
) {
    document
        .set_block_background_color(block_index, background)
        .unwrap_or_else(|_| panic!("{label} background must be valid"));
}

fn unit_rect(label: &str) -> BlockRect {
    BlockRect::new(0.0, 0.0, 1.0, 1.0).unwrap_or_else(|_| panic!("{label} must be valid"))
}
