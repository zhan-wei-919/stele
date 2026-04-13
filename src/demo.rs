//! Demo document assembly reused by the async store bootstrap.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::draw_list::{ImageCmd, ImageData, RenderLayer};
use crate::font::FreeTypeRasterizer;
use crate::layout::{prepare_document, Block, BlockRect, Document, PreparedBlock, Span, TextStyle};
use crate::scene::BlockId;

const PAGE_BG: [f32; 4] = [0.05, 0.08, 0.12, 1.0];
const PANEL_ACCENT_BG: [f32; 4] = [0.16, 0.21, 0.28, 0.98];
const TEXT_PRIMARY: [f32; 4] = [0.92, 0.95, 0.97, 1.0];
const TEXT_MUTED: [f32; 4] = [0.75, 0.80, 0.86, 1.0];
const TEXT_ACCENT: [f32; 4] = [0.94, 0.69, 0.28, 1.0];
const INLINE_BG: [f32; 4] = [0.19, 0.25, 0.33, 0.95];
const OVERLAY_INLINE_BG: [f32; 4] = [0.24, 0.30, 0.38, 0.92];
const SMOKE_IMAGE_SIZE_PX: u32 = 64;

/// Prepared demo document reused by the async store.
pub(crate) struct DemoState {
    document: Document,
    prepared_blocks: Vec<PreparedBlock>,
}

impl DemoState {
    /// Splits the prepared demo state into document and prepare cache ownership.
    pub(crate) fn into_parts(self) -> (Document, Vec<PreparedBlock>) {
        (self.document, self.prepared_blocks)
    }
}

/// Builds the demo model once and precomputes its prepare-stage cache.
pub(crate) fn build_demo_state(
    rasterizer: &FreeTypeRasterizer,
    logical_viewport: [f32; 2],
) -> DemoState {
    let document = build_demo_document(rasterizer.default_font_id(), logical_viewport);
    let prepared_blocks = prepare_document(&document, rasterizer);
    DemoState {
        document,
        prepared_blocks,
    }
}

/// Updates demo block geometry for a new logical viewport.
pub(crate) fn resize_demo_document(document: &mut Document, logical_viewport: [f32; 2]) {
    apply_demo_block_rects(document, logical_viewport);
}

/// Returns demo-owned image commands keyed by an existing block id.
pub(crate) fn demo_block_images(document: &Document) -> HashMap<BlockId, Vec<ImageCmd>> {
    let Some(root_block) = document.block(0) else {
        return HashMap::new();
    };

    let rect = root_block.rect();
    let smoke_size = (rect.width().min(rect.height()) * 0.18)
        .clamp(72.0, 128.0)
        .min(rect.width() - 32.0)
        .min(rect.height() - 32.0);
    if smoke_size <= 0.0 {
        return HashMap::new();
    }

    let x = (rect.x() + rect.width() - smoke_size - 40.0).max(rect.x() + 16.0);
    let y = (rect.y() + 40.0).min(rect.y() + rect.height() - smoke_size - 16.0);
    let image = ImageCmd::new(
        [x, y.max(rect.y() + 16.0)],
        [smoke_size, smoke_size],
        demo_smoke_image_data(),
        RenderLayer::Foreground,
    );

    HashMap::from([(root_block.id(), vec![image])])
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

fn demo_smoke_image_data() -> Arc<ImageData> {
    static DEMO_SMOKE_IMAGE: OnceLock<Arc<ImageData>> = OnceLock::new();

    DEMO_SMOKE_IMAGE
        .get_or_init(|| {
            Arc::new(ImageData::new(
                build_demo_smoke_rgba(),
                SMOKE_IMAGE_SIZE_PX,
                SMOKE_IMAGE_SIZE_PX,
            ))
        })
        .clone()
}

fn build_demo_smoke_rgba() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((SMOKE_IMAGE_SIZE_PX * SMOKE_IMAGE_SIZE_PX * 4) as usize);
    let max = (SMOKE_IMAGE_SIZE_PX - 1) as f32;

    for y in 0..SMOKE_IMAGE_SIZE_PX {
        for x in 0..SMOKE_IMAGE_SIZE_PX {
            let fx = x as f32 / max;
            let fy = y as f32 / max;
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            let border =
                x < 3 || y < 3 || x >= SMOKE_IMAGE_SIZE_PX - 3 || y >= SMOKE_IMAGE_SIZE_PX - 3;

            let (r, g, b, a) = if border {
                (255, 245, 230, 255)
            } else if checker {
                (
                    (60.0 + fx * 150.0) as u8,
                    (120.0 + fy * 90.0) as u8,
                    220,
                    255,
                )
            } else {
                (
                    240,
                    (90.0 + fx * 100.0) as u8,
                    (80.0 + fy * 120.0) as u8,
                    255,
                )
            };

            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }

    rgba
}
