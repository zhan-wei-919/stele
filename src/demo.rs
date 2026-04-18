//! Demo document assembly reused by the async store bootstrap.

use std::f32::consts::PI;
use std::sync::{Arc, OnceLock};

use crate::draw_list::{ImageData, LineCap, LineJoin, PathVerb};
use crate::font::FreeTypeRasterizer;
use crate::layout::prepare_tree::prepare_tree;
use crate::layout::tree::{
    AnchorKey, BlockEmbedKind, BlockEmbedNode, BlockNode, BlockStyle, ClipMode, DocumentTree,
    Edges, FlowDirection, InlineAtom, InlineAtomKind, InlineAtomStyle, InlineNode, OverlayAnchor,
    OverlayNode, ParagraphNode, ParagraphStyle, PathStroke, StackNode, TextRun, TextStyle,
};
use crate::store::{Model, Store, StoreBootstrap, StoreDelegate, ViewportState};

const PAGE_BG: [f32; 4] = [0.05, 0.08, 0.12, 1.0];
const PANEL_ACCENT_BG: [f32; 4] = [0.16, 0.21, 0.28, 0.98];
const TEXT_PRIMARY: [f32; 4] = [0.92, 0.95, 0.97, 1.0];
const TEXT_MUTED: [f32; 4] = [0.75, 0.80, 0.86, 1.0];
const TEXT_ACCENT: [f32; 4] = [0.94, 0.69, 0.28, 1.0];
const INLINE_BG: [f32; 4] = [0.19, 0.25, 0.33, 0.95];
const OVERLAY_INLINE_BG: [f32; 4] = [0.24, 0.30, 0.38, 0.92];
const SMOKE_IMAGE_SIZE_PX: u32 = 64;
const STAR_LOGICAL_SIZE: f32 = 96.0;
const STAR_FILL: [f32; 4] = [0.96, 0.73, 0.22, 0.92];
const STAR_STROKE: [f32; 4] = [1.0, 0.94, 0.72, 1.0];

/// Demo-only store delegate that consumes the generic store boundary.
pub(crate) struct DemoStoreDelegate;

/// Builds a store instance backed by the demo document.
pub(crate) fn build_store(rasterizer: FreeTypeRasterizer, viewport: ViewportState) -> Store {
    Store::new(rasterizer, viewport, Arc::new(DemoStoreDelegate))
}

impl StoreDelegate for DemoStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let document = build_demo_document_tree(rasterizer.default_font_id());
        let prepared_tree = prepare_tree(&document, rasterizer);
        StoreBootstrap::new(document, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

#[derive(Clone, Copy)]
struct DemoStyles {
    title: TextStyle,
    badge: TextStyle,
    body: TextStyle,
    overlay_title: TextStyle,
    overlay_body: TextStyle,
}

fn build_demo_document_tree(font_id: u32) -> DocumentTree {
    let styles = build_demo_styles(font_id);
    let body_anchor = AnchorKey::new("hero-body").expect("anchor must be valid");
    let smoke_embed = BlockNode::Embed(
        BlockEmbedNode::new(
            BlockEmbedKind::Image {
                data_ref: demo_smoke_image_data(),
                intrinsic_size: [SMOKE_IMAGE_SIZE_PX as f32, SMOKE_IMAGE_SIZE_PX as f32],
            },
            BlockStyle {
                margin: Edges::new(0.0, 22.0, 0.0, 0.0).expect("edges must be valid"),
                ..BlockStyle::default()
            },
        )
        .expect("image embed must be valid"),
    );
    let overlay_card = BlockNode::Overlay(OverlayNode::new(
        OverlayAnchor::BlockRelative {
            target: body_anchor.clone(),
            offset: [28.0, 104.0],
        },
        build_overlay_card(&styles),
    ));
    let overlay_star = BlockNode::Overlay(OverlayNode::new(
        OverlayAnchor::BlockRelative {
            target: body_anchor.clone(),
            offset: [360.0, -18.0],
        },
        BlockNode::Embed(
            BlockEmbedNode::new(
                BlockEmbedKind::Path {
                    verbs: build_star_verbs(STAR_LOGICAL_SIZE),
                    fill: Some(STAR_FILL),
                    stroke: Some(PathStroke {
                        color: STAR_STROKE,
                        width: 2.0,
                        line_cap: LineCap::Round,
                        line_join: LineJoin::Round,
                    }),
                    intrinsic_size: [STAR_LOGICAL_SIZE, STAR_LOGICAL_SIZE],
                },
                BlockStyle {
                    z_index: 2,
                    ..BlockStyle::default()
                },
            )
            .expect("path embed must be valid"),
        ),
    ));

    let mut hero = ParagraphNode::new(
        vec![
            InlineNode::Text(TextRun::new("Stele Layout Engine", styles.title)),
            InlineNode::Text(TextRun::new(
                "\n最小 LayoutTree 已经接进 prepare -> layout -> scene 管线；现在段落负责断行、baseline 和 inline mixed flow。\n",
                styles.body,
            )),
            InlineNode::Atom(
                InlineAtom::new(
                    InlineAtomKind::Chip {
                        label: String::from("layout tree ready"),
                        text_style: styles.badge,
                    },
                    InlineAtomStyle {
                        background: Some(INLINE_BG),
                        padding: Edges::new(10.0, 4.0, 10.0, 4.0).expect("edges must be valid"),
                        ..InlineAtomStyle::default()
                    },
                )
                .expect("chip atom must be valid"),
            ),
            InlineNode::Text(TextRun::new(
                " with mixed ASCII/CJK content. The quick brown fox jumps over the lazy dog, 你好世界ABC测试，长单词Supercalifragilisticexpialidocious也会按字符强制换行。",
                styles.body,
            )),
        ],
        ParagraphStyle {
            block: BlockStyle {
                background: Some(PAGE_BG),
                padding: Edges::all(28.0).expect("padding must be valid"),
                clip: ClipMode::Rect,
                ..BlockStyle::default()
            },
            ..ParagraphStyle::default()
        },
    )
    .expect("hero paragraph must be valid");
    hero.anchor_key = Some(body_anchor);

    DocumentTree::new(BlockNode::Stack(
        StackNode::new(
            FlowDirection::Vertical,
            vec![
                BlockNode::Paragraph(hero),
                smoke_embed,
                overlay_card,
                overlay_star,
            ],
            BlockStyle::default(),
        )
        .expect("root stack must be valid"),
    ))
    .expect("demo tree must be valid")
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

fn build_overlay_card(styles: &DemoStyles) -> BlockNode {
    BlockNode::Stack(
        StackNode::new(
            FlowDirection::Vertical,
            vec![BlockNode::Paragraph(
                ParagraphNode::new(
                    vec![
                        InlineNode::Text(TextRun::new("Overlay Block", styles.overlay_title)),
                        InlineNode::Text(TextRun::new(
                            "\nThis card is anchored by AnchorKey and rendered as an independent block batch. Resize the window to reflow paragraphs without rerunning prepare.",
                            styles.overlay_body,
                        )),
                    ],
                    ParagraphStyle {
                        block: BlockStyle {
                            background: Some(PANEL_ACCENT_BG),
                            padding: Edges::all(18.0).expect("padding must be valid"),
                            clip: ClipMode::Rect,
                            z_index: 1,
                            max_width: Some(320.0),
                            ..BlockStyle::default()
                        },
                        ..ParagraphStyle::default()
                    },
                )
                .expect("overlay paragraph must be valid"),
            )],
            BlockStyle::default(),
        )
        .expect("overlay stack must be valid"),
    )
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

fn build_star_verbs(size: f32) -> Vec<PathVerb> {
    let center = [size * 0.5, size * 0.5];
    let outer_radius = size * 0.5;
    let inner_radius = outer_radius * 0.46;
    let mut verbs = Vec::with_capacity(11);
    for point_index in 0..10 {
        let angle = -PI * 0.5 + point_index as f32 * (PI / 5.0);
        let radius = if point_index % 2 == 0 {
            outer_radius
        } else {
            inner_radius
        };
        let point = [
            center[0] + radius * angle.cos(),
            center[1] + radius * angle.sin(),
        ];
        if point_index == 0 {
            verbs.push(PathVerb::MoveTo { to: point });
        } else {
            verbs.push(PathVerb::LineTo { to: point });
        }
    }
    verbs.push(PathVerb::Close);
    verbs
}
