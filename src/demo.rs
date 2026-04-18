//! Demo document assembly reused by the async store bootstrap.

use std::sync::{Arc, OnceLock};

use crate::draw_list::{ImageData, LineCap, LineJoin, PathVerb};
use crate::font::FreeTypeRasterizer;
use crate::layout::prepare_tree::prepare_tree;
use crate::layout::tree::{
    Align, AnchorKey, AtomBaseline, BlockEmbedKind, BlockEmbedNode, BlockNode, BlockStyle,
    ClipMode, DocumentTree, Edges, FlowDirection, InlineAtom, InlineAtomKind, InlineAtomStyle,
    InlineNode, LineHeight, OverlayAnchor, OverlayNode, ParagraphNode, ParagraphStyle, PathStroke,
    StackNode, TextRun, TextStyle, WrapMode,
};
use crate::store::{Model, Store, StoreBootstrap, StoreDelegate, ViewportState};

const PAGE_BG: [f32; 4] = [0.05, 0.08, 0.12, 1.0];
const SURFACE_BG: [f32; 4] = [0.10, 0.14, 0.19, 0.98];
const SURFACE_BG_STRONG: [f32; 4] = [0.13, 0.18, 0.24, 1.0];
const SURFACE_BG_SUBTLE: [f32; 4] = [0.16, 0.21, 0.28, 0.98];
const CHIP_BG: [f32; 4] = [0.20, 0.27, 0.35, 1.0];
const CODE_BG: [f32; 4] = [0.08, 0.11, 0.15, 1.0];
const CODE_INLINE_BG: [f32; 4] = [0.18, 0.24, 0.31, 1.0];
const IMAGE_FRAME_BG: [f32; 4] = [0.12, 0.17, 0.22, 1.0];
const OVERLAY_BG: [f32; 4] = [0.18, 0.23, 0.30, 0.98];
const VIEWPORT_OVERLAY_BG: [f32; 4] = [0.13, 0.22, 0.20, 0.96];
const TEXT_PRIMARY: [f32; 4] = [0.94, 0.96, 0.98, 1.0];
const TEXT_MUTED: [f32; 4] = [0.76, 0.82, 0.88, 1.0];
const TEXT_SUBTLE: [f32; 4] = [0.58, 0.66, 0.74, 1.0];
const TEXT_ACCENT: [f32; 4] = [0.98, 0.76, 0.36, 1.0];
const TEXT_LINK: [f32; 4] = [0.54, 0.83, 0.98, 1.0];
const TEXT_SUCCESS: [f32; 4] = [0.57, 0.90, 0.69, 1.0];
const TEXT_DANGER: [f32; 4] = [0.98, 0.55, 0.55, 1.0];
const INLINE_IMAGE_SIZE_PX: u32 = 20;
const BLOCK_IMAGE_WIDTH_PX: u32 = 320;
const BLOCK_IMAGE_HEIGHT_PX: u32 = 208;
const CHART_SIZE: [f32; 2] = [280.0, 144.0];
const CODE_ANCHOR_KEY: &str = "markdown-code-block";

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
        logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let document = build_demo_document_tree(rasterizer.default_font_id(), logical_viewport);
        let prepared_tree = prepare_tree(&document, rasterizer);
        StoreBootstrap::new(document, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

#[derive(Clone, Copy)]
struct DemoStyles {
    title: TextStyle,
    overline: TextStyle,
    heading: TextStyle,
    body: TextStyle,
    strong: TextStyle,
    emphasis: TextStyle,
    highlight: TextStyle,
    link: TextStyle,
    deleted: TextStyle,
    caption: TextStyle,
    code: TextStyle,
    code_emphasis: TextStyle,
    badge: TextStyle,
    jumbo: TextStyle,
    micro: TextStyle,
    overlay_title: TextStyle,
    overlay_body: TextStyle,
    viewport_title: TextStyle,
}

fn build_demo_document_tree(font_id: u32, logical_viewport: [f32; 2]) -> DocumentTree {
    let styles = build_demo_styles(font_id);
    let content_max_width = (logical_viewport[0] - 88.0).clamp(320.0, 880.0);
    let wide_layout = logical_viewport[0] >= 760.0;
    let code_anchor = AnchorKey::new(CODE_ANCHOR_KEY).expect("anchor must be valid");

    let root = BlockNode::Stack(
        StackNode::new(
            FlowDirection::Vertical,
            vec![
                build_content_column(&styles, content_max_width, wide_layout, code_anchor.clone()),
                build_code_overlay(&styles, code_anchor, content_max_width),
                build_viewport_overlay(&styles, logical_viewport),
            ],
            BlockStyle {
                background: Some(PAGE_BG),
                clip: ClipMode::Rect,
                padding: edges(28.0, 24.0, 28.0, 36.0),
                min_height: Some(logical_viewport[1].max(320.0)),
                ..BlockStyle::default()
            },
        )
        .expect("root stack must be valid"),
    );

    DocumentTree::new(root).expect("demo tree must be valid")
}

fn build_content_column(
    styles: &DemoStyles,
    content_max_width: f32,
    wide_layout: bool,
    code_anchor: AnchorKey,
) -> BlockNode {
    stack(
        FlowDirection::Vertical,
        vec![
            build_hero_section(styles),
            build_summary_section(styles, wide_layout),
            build_inline_showcase_section(styles),
            build_quote_and_list_section(styles),
            build_baseline_section(styles),
            build_code_section(styles, code_anchor),
            build_media_section(styles, wide_layout),
        ],
        BlockStyle {
            align_self: Align::Start,
            max_width: Some(content_max_width),
            ..BlockStyle::default()
        },
    )
}

fn build_hero_section(styles: &DemoStyles) -> BlockNode {
    stack(
        FlowDirection::Vertical,
        vec![
            paragraph(
                vec![
                    text("STELE MARKDOWN DEMO", styles.overline),
                    text("\nHand-Assembled Markdown Page", styles.title),
                ],
                ParagraphStyle::default(),
            ),
            paragraph(
                vec![text(
                    "这个 demo 不做 markdown parser，而是直接用 DocumentTree 把 markdown 页面常见的块和行内语义拼出来：标题、列表、引用、代码块、图片、图表和 overlay 注释都在同一条 tree path 上。",
                    styles.body,
                )],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 14.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
            paragraph(
                vec![
                    chip("stack", styles.badge),
                    chip("paragraph", styles.badge),
                    chip("inline atom", styles.badge),
                    chip("embed", styles.badge),
                    chip("overlay", styles.badge),
                    text(
                        " 这几个标签本身也是 inline atom，用来模拟 markdown 页面里常见的 badge / pill UI。",
                        styles.caption,
                    ),
                ],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 18.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG_STRONG),
            clip: ClipMode::Rect,
            padding: edges(24.0, 24.0, 24.0, 24.0),
            min_height: Some(172.0),
            ..BlockStyle::default()
        },
    )
}

fn build_summary_section(styles: &DemoStyles, wide_layout: bool) -> BlockNode {
    let direction = if wide_layout {
        FlowDirection::Horizontal
    } else {
        FlowDirection::Vertical
    };

    stack(
        direction,
        vec![
            build_summary_card(
                styles,
                "Layout",
                "Vertical / horizontal stack 都走同一个 block solver，margin collapse 和 clip 已经落在 tree path 里。",
                wide_layout,
                false,
            ),
            build_summary_card(
                styles,
                "Typography",
                "段落支持 inline mixed flow：粗体、斜体、下划线、删除线、背景色、不同字号和 inline atom 共用 baseline。",
                wide_layout,
                false,
            ),
            build_summary_card(
                styles,
                "Deferred",
                "overlay 先在声明期继承 clip，再在第二阶段按 anchor 解位置和 z 顺序，保持和源树父链语义一致。",
                wide_layout,
                true,
            ),
        ],
        BlockStyle {
            margin: edges(0.0, 18.0, 0.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn build_summary_card(
    styles: &DemoStyles,
    label: &str,
    body: &str,
    wide_layout: bool,
    is_last: bool,
) -> BlockNode {
    let margin = if wide_layout && !is_last {
        edges(0.0, 0.0, 16.0, 14.0)
    } else if !wide_layout && !is_last {
        edges(0.0, 0.0, 0.0, 14.0)
    } else {
        edges(0.0, 0.0, 0.0, 0.0)
    };

    stack(
        FlowDirection::Vertical,
        vec![paragraph(
            vec![
                text(label, styles.heading),
                text("\n", styles.body),
                text(body, styles.caption),
            ],
            ParagraphStyle::default(),
        )],
        BlockStyle {
            background: Some(SURFACE_BG),
            clip: ClipMode::Rect,
            padding: edges(18.0, 18.0, 18.0, 18.0),
            margin,
            align_self: Align::Start,
            min_width: Some(180.0),
            max_width: Some(240.0),
            min_height: Some(124.0),
            ..BlockStyle::default()
        },
    )
}

fn build_inline_showcase_section(styles: &DemoStyles) -> BlockNode {
    stack(
        FlowDirection::Vertical,
        vec![
            paragraph(
                vec![
                    text("## ", styles.overline),
                    text("Inline And Typography", styles.heading),
                ],
                ParagraphStyle::default(),
            ),
            paragraph(
                vec![
                    text("普通 span 可以直接混排 ", styles.body),
                    text("bold", styles.strong),
                    text("、", styles.body),
                    text("italic", styles.emphasis),
                    text("、", styles.body),
                    text("underline", styles.link),
                    text("、", styles.body),
                    text("highlight", styles.highlight),
                    text(" 和 ", styles.body),
                    text("strikethrough", styles.deleted),
                    text("。同一段里还能插入 ", styles.body),
                    chip("chip atom", styles.badge),
                    text(" 以及 inline image ", styles.body),
                    image_atom(
                        demo_inline_image_data(),
                        AtomBaseline::AlphabeticAlignedToLine,
                        Some(CODE_INLINE_BG),
                    ),
                    text(
                        "，所以拿它拼 markdown 的 callout、emoji 替身或者 badge 都比较直接。",
                        styles.body,
                    ),
                ],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 12.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
            paragraph(
                vec![
                    text("Mixed font size: ", styles.caption),
                    text("Display", styles.jumbo),
                    text(" text can stay inline with ", styles.body),
                    text("micro-note", styles.micro),
                    text(
                        " and the line box still resolves a shared baseline.",
                        styles.body,
                    ),
                ],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 14.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG),
            clip: ClipMode::Rect,
            padding: edges(22.0, 20.0, 22.0, 20.0),
            margin: edges(0.0, 18.0, 0.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn build_quote_and_list_section(styles: &DemoStyles) -> BlockNode {
    stack(
        FlowDirection::Vertical,
        vec![
            paragraph(
                vec![
                    text("## ", styles.overline),
                    text("Blockquote And Lists", styles.heading),
                ],
                ParagraphStyle::default(),
            ),
            paragraph(
                vec![text(
                    "> The goal of the demo is not a markdown parser. It is a compact proof that the semantic tree already has enough primitives to model a realistic doc page.",
                    styles.emphasis,
                )],
                ParagraphStyle {
                    block: BlockStyle {
                        background: Some(SURFACE_BG_SUBTLE),
                        clip: ClipMode::Rect,
                        padding: edges(18.0, 16.0, 18.0, 16.0),
                        margin: edges(0.0, 12.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
            paragraph(
                vec![text(
                    "- [x] 展示 vertical stack 和 horizontal stack\n- [x] 展示段落换行、混合字号和多种文本修饰\n- [x] 展示 inline atom、image embed、path embed、block-relative overlay、viewport overlay",
                    styles.body,
                )],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 14.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG),
            clip: ClipMode::Rect,
            padding: edges(22.0, 20.0, 22.0, 20.0),
            margin: edges(0.0, 18.0, 0.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn build_baseline_section(styles: &DemoStyles) -> BlockNode {
    stack(
        FlowDirection::Vertical,
        vec![
            paragraph(
                vec![
                    text("## ", styles.overline),
                    text("Inline Atom Baseline", styles.heading),
                ],
                ParagraphStyle::default(),
            ),
            paragraph(
                vec![
                    text("Alphabetic ", styles.body),
                    image_atom(
                        demo_inline_image_data(),
                        AtomBaseline::AlphabeticAlignedToLine,
                        Some(SURFACE_BG_SUBTLE),
                    ),
                    text("  Top ", styles.body),
                    image_atom(
                        demo_inline_image_data(),
                        AtomBaseline::Top,
                        Some(SURFACE_BG_SUBTLE),
                    ),
                    text("  Middle ", styles.body),
                    image_atom(
                        demo_inline_image_data(),
                        AtomBaseline::MiddleOfLine,
                        Some(SURFACE_BG_SUBTLE),
                    ),
                    text("  Bottom ", styles.body),
                    image_atom(
                        demo_inline_image_data(),
                        AtomBaseline::Bottom,
                        Some(SURFACE_BG_SUBTLE),
                    ),
                    text(
                        " 同一行会先求公共 baseline，再把不同 baseline 策略的 atom 回填到行内。",
                        styles.caption,
                    ),
                ],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 12.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG),
            clip: ClipMode::Rect,
            padding: edges(22.0, 20.0, 22.0, 20.0),
            margin: edges(0.0, 18.0, 0.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn build_code_section(styles: &DemoStyles, code_anchor: AnchorKey) -> BlockNode {
    stack(
        FlowDirection::Vertical,
        vec![
            paragraph(
                vec![
                    text("## ", styles.overline),
                    text("Code Fence", styles.heading),
                ],
                ParagraphStyle::default(),
            ),
            anchored_paragraph(
                code_anchor,
                vec![
                    text("fn render_markdown_demo(tree: &mut DocumentTree) {\n", styles.code),
                    text("    ", styles.code),
                    text("// WrapMode::NoWrap + ClipMode::Rect", styles.code_emphasis),
                    text("\n", styles.code),
                    text("    let headline = \"This line is intentionally long so the right edge gets clipped instead of reflowing into a wrapped code block\";\n", styles.code),
                    text("    tree.push(headline);\n", styles.code),
                    text("}\n", styles.code),
                ],
                ParagraphStyle {
                    block: BlockStyle {
                        background: Some(CODE_BG),
                        clip: ClipMode::Rect,
                        padding: edges(18.0, 16.0, 18.0, 16.0),
                        margin: edges(0.0, 12.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    line_height: LineHeight::Absolute(20.0),
                    wrap: WrapMode::NoWrap,
                    ..ParagraphStyle::default()
                },
            ),
            paragraph(
                vec![text(
                    "代码块这里显式用了绝对行高和不换行策略，用来演示 paragraph style 里的 line_height / wrap / clip 组合。",
                    styles.caption,
                )],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 12.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG),
            clip: ClipMode::Rect,
            padding: edges(22.0, 20.0, 22.0, 20.0),
            margin: edges(0.0, 18.0, 0.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn build_media_section(styles: &DemoStyles, wide_layout: bool) -> BlockNode {
    let direction = if wide_layout {
        FlowDirection::Horizontal
    } else {
        FlowDirection::Vertical
    };

    stack(
        FlowDirection::Vertical,
        vec![
            paragraph(
                vec![text("## ", styles.overline), text("Embeds", styles.heading)],
                ParagraphStyle::default(),
            ),
            stack(
                direction,
                vec![
                    build_image_panel(styles, wide_layout),
                    build_chart_panel(styles, wide_layout),
                ],
                BlockStyle {
                    margin: edges(0.0, 12.0, 0.0, 0.0),
                    ..BlockStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG),
            clip: ClipMode::Rect,
            padding: edges(22.0, 20.0, 22.0, 20.0),
            margin: edges(0.0, 18.0, 0.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn build_image_panel(styles: &DemoStyles, wide_layout: bool) -> BlockNode {
    let margin = if wide_layout {
        edges(0.0, 0.0, 16.0, 14.0)
    } else {
        edges(0.0, 0.0, 0.0, 14.0)
    };

    stack(
        FlowDirection::Vertical,
        vec![
            embed(
                BlockEmbedKind::Image {
                    data_ref: demo_block_image_data(),
                    intrinsic_size: [BLOCK_IMAGE_WIDTH_PX as f32, BLOCK_IMAGE_HEIGHT_PX as f32],
                },
                BlockStyle {
                    background: Some(IMAGE_FRAME_BG),
                    clip: ClipMode::Rect,
                    padding: edges(10.0, 10.0, 10.0, 10.0),
                    align_self: Align::Start,
                    max_width: Some(BLOCK_IMAGE_WIDTH_PX as f32),
                    max_height: Some(BLOCK_IMAGE_HEIGHT_PX as f32),
                    ..BlockStyle::default()
                },
            ),
            paragraph(
                vec![text(
                    "Image embed 保持 intrinsic size；这里额外给了 frame background 和 clip，用来展示 block embed 的容器语义。",
                    styles.caption,
                )],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 12.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG_SUBTLE),
            clip: ClipMode::Rect,
            padding: edges(16.0, 16.0, 16.0, 16.0),
            margin,
            align_self: Align::Start,
            min_width: Some(280.0),
            min_height: Some(252.0),
            ..BlockStyle::default()
        },
    )
}

fn build_chart_panel(styles: &DemoStyles, wide_layout: bool) -> BlockNode {
    let margin = if wide_layout {
        edges(0.0, 0.0, 0.0, 14.0)
    } else {
        edges(0.0, 0.0, 0.0, 0.0)
    };

    stack(
        FlowDirection::Vertical,
        vec![
            embed(
                BlockEmbedKind::Path {
                    verbs: build_chart_verbs(CHART_SIZE),
                    fill: Some([0.24, 0.66, 0.98, 0.18]),
                    stroke: Some(PathStroke {
                        color: TEXT_LINK,
                        width: 3.0,
                        line_cap: LineCap::Round,
                        line_join: LineJoin::Round,
                    }),
                    intrinsic_size: CHART_SIZE,
                },
                BlockStyle {
                    background: Some(IMAGE_FRAME_BG),
                    clip: ClipMode::Rect,
                    padding: edges(12.0, 12.0, 12.0, 12.0),
                    align_self: Align::Start,
                    ..BlockStyle::default()
                },
            ),
            paragraph(
                vec![text(
                    "Path embed 这里拿来模拟图表卡片，fill + stroke 组合覆盖了最小 scene path 的另一条分支。",
                    styles.caption,
                )],
                ParagraphStyle {
                    block: BlockStyle {
                        margin: edges(0.0, 12.0, 0.0, 0.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            ),
        ],
        BlockStyle {
            background: Some(SURFACE_BG_SUBTLE),
            clip: ClipMode::Rect,
            padding: edges(16.0, 16.0, 16.0, 16.0),
            margin,
            align_self: Align::Start,
            min_width: Some(280.0),
            min_height: Some(252.0),
            ..BlockStyle::default()
        },
    )
}

fn build_code_overlay(
    styles: &DemoStyles,
    code_anchor: AnchorKey,
    content_max_width: f32,
) -> BlockNode {
    let overlay_offset_x = (content_max_width - 296.0).max(24.0);

    BlockNode::Overlay(OverlayNode::new(
        OverlayAnchor::BlockRelative {
            target: code_anchor,
            offset: [overlay_offset_x, 18.0],
        },
        stack(
            FlowDirection::Vertical,
            vec![paragraph(
                vec![
                    text("OverlayAnchor::BlockRelative", styles.overlay_title),
                    text(
                        "\n这个浮层锚在代码块上，但仍然继承声明期父链的 effective clip。窗口变化时 paragraph 会重新 layout，而 prepare cache 仍然可以复用。",
                        styles.overlay_body,
                    ),
                ],
                ParagraphStyle::default(),
            )],
            BlockStyle {
                background: Some(OVERLAY_BG),
                clip: ClipMode::Rect,
                padding: all_edges(14.0),
                z_index: 3,
                max_width: Some(272.0),
                ..BlockStyle::default()
            },
        ),
    ))
}

fn build_viewport_overlay(styles: &DemoStyles, logical_viewport: [f32; 2]) -> BlockNode {
    let offset_x = (logical_viewport[0] - 236.0).max(24.0);

    BlockNode::Overlay(OverlayNode::new(
        OverlayAnchor::Viewport {
            offset: [offset_x, 22.0],
        },
        stack(
            FlowDirection::Vertical,
            vec![paragraph(
                vec![
                    text("Viewport Overlay", styles.viewport_title),
                    text(
                        "\n固定在 viewport 坐标系上的调试角标，和 block-relative overlay 分开演示。",
                        styles.overlay_body,
                    ),
                ],
                ParagraphStyle::default(),
            )],
            BlockStyle {
                background: Some(VIEWPORT_OVERLAY_BG),
                clip: ClipMode::Rect,
                padding: all_edges(12.0),
                z_index: 4,
                max_width: Some(212.0),
                ..BlockStyle::default()
            },
        ),
    ))
}

fn build_demo_styles(font_id: u32) -> DemoStyles {
    DemoStyles {
        title: TextStyle::new(font_id, 34.0, TEXT_PRIMARY)
            .expect("demo title style must be valid")
            .with_bold(true),
        overline: TextStyle::new(font_id, 12.0, TEXT_ACCENT)
            .expect("demo overline style must be valid")
            .with_bold(true)
            .with_letter_spacing(1.8)
            .expect("demo overline spacing must be valid"),
        heading: TextStyle::new(font_id, 22.0, TEXT_PRIMARY)
            .expect("demo heading style must be valid")
            .with_bold(true),
        body: TextStyle::new(font_id, 16.0, TEXT_MUTED).expect("demo body style must be valid"),
        strong: TextStyle::new(font_id, 16.0, TEXT_PRIMARY)
            .expect("demo strong style must be valid")
            .with_bold(true),
        emphasis: TextStyle::new(font_id, 16.0, TEXT_MUTED)
            .expect("demo emphasis style must be valid")
            .with_italic(true),
        highlight: TextStyle::new(font_id, 16.0, TEXT_PRIMARY)
            .expect("demo highlight style must be valid")
            .with_background_color(Some(CODE_INLINE_BG))
            .expect("demo highlight background must be valid"),
        link: TextStyle::new(font_id, 16.0, TEXT_LINK)
            .expect("demo link style must be valid")
            .with_underline(true),
        deleted: TextStyle::new(font_id, 16.0, TEXT_DANGER)
            .expect("demo deleted style must be valid")
            .with_strikethrough(true),
        caption: TextStyle::new(font_id, 14.0, TEXT_SUBTLE)
            .expect("demo caption style must be valid"),
        code: TextStyle::new(font_id, 14.0, TEXT_PRIMARY).expect("demo code style must be valid"),
        code_emphasis: TextStyle::new(font_id, 14.0, TEXT_SUCCESS)
            .expect("demo code emphasis style must be valid")
            .with_background_color(Some(CODE_INLINE_BG))
            .expect("demo code emphasis background must be valid"),
        badge: TextStyle::new(font_id, 13.0, TEXT_ACCENT)
            .expect("demo badge style must be valid")
            .with_bold(true)
            .with_letter_spacing(0.6)
            .expect("demo badge spacing must be valid"),
        jumbo: TextStyle::new(font_id, 30.0, TEXT_PRIMARY)
            .expect("demo jumbo style must be valid")
            .with_bold(true),
        micro: TextStyle::new(font_id, 11.0, TEXT_SUBTLE)
            .expect("demo micro style must be valid")
            .with_underline(true),
        overlay_title: TextStyle::new(font_id, 15.0, TEXT_PRIMARY)
            .expect("demo overlay title style must be valid")
            .with_bold(true),
        overlay_body: TextStyle::new(font_id, 13.0, TEXT_MUTED)
            .expect("demo overlay body style must be valid"),
        viewport_title: TextStyle::new(font_id, 13.0, TEXT_PRIMARY)
            .expect("demo viewport title style must be valid")
            .with_bold(true),
    }
}

fn text(content: impl Into<String>, style: TextStyle) -> InlineNode {
    InlineNode::Text(TextRun::new(content, style))
}

fn chip(label: &str, text_style: TextStyle) -> InlineNode {
    InlineNode::Atom(
        InlineAtom::new(
            InlineAtomKind::Chip {
                label: label.to_owned(),
                text_style,
            },
            InlineAtomStyle {
                margin: edges(4.0, 0.0, 4.0, 0.0),
                padding: edges(10.0, 4.0, 10.0, 4.0),
                background: Some(CHIP_BG),
                ..InlineAtomStyle::default()
            },
        )
        .expect("chip atom must be valid"),
    )
}

fn image_atom(
    data_ref: Arc<ImageData>,
    baseline: AtomBaseline,
    background: Option<[f32; 4]>,
) -> InlineNode {
    InlineNode::Atom(
        InlineAtom::new(
            InlineAtomKind::Image { data_ref },
            InlineAtomStyle {
                margin: edges(3.0, 0.0, 3.0, 0.0),
                padding: edges(2.0, 2.0, 2.0, 2.0),
                baseline,
                background,
                ..InlineAtomStyle::default()
            },
        )
        .expect("image atom must be valid"),
    )
}

fn paragraph(inlines: Vec<InlineNode>, style: ParagraphStyle) -> BlockNode {
    BlockNode::Paragraph(ParagraphNode::new(inlines, style).expect("demo paragraph must be valid"))
}

fn anchored_paragraph(
    anchor_key: AnchorKey,
    inlines: Vec<InlineNode>,
    style: ParagraphStyle,
) -> BlockNode {
    let mut paragraph = ParagraphNode::new(inlines, style).expect("demo paragraph must be valid");
    paragraph.anchor_key = Some(anchor_key);
    BlockNode::Paragraph(paragraph)
}

fn stack(direction: FlowDirection, children: Vec<BlockNode>, style: BlockStyle) -> BlockNode {
    BlockNode::Stack(StackNode::new(direction, children, style).expect("demo stack must be valid"))
}

fn embed(kind: BlockEmbedKind, style: BlockStyle) -> BlockNode {
    BlockNode::Embed(BlockEmbedNode::new(kind, style).expect("demo embed must be valid"))
}

fn edges(left: f32, top: f32, right: f32, bottom: f32) -> Edges {
    Edges::new(left, top, right, bottom).expect("demo edges must be valid")
}

fn all_edges(value: f32) -> Edges {
    Edges::all(value).expect("demo edges must be valid")
}

fn demo_inline_image_data() -> Arc<ImageData> {
    static INLINE_IMAGE: OnceLock<Arc<ImageData>> = OnceLock::new();

    INLINE_IMAGE
        .get_or_init(|| {
            Arc::new(ImageData::new(
                build_inline_image_rgba(),
                INLINE_IMAGE_SIZE_PX,
                INLINE_IMAGE_SIZE_PX,
            ))
        })
        .clone()
}

fn demo_block_image_data() -> Arc<ImageData> {
    static BLOCK_IMAGE: OnceLock<Arc<ImageData>> = OnceLock::new();

    BLOCK_IMAGE
        .get_or_init(|| {
            Arc::new(ImageData::new(
                build_block_image_rgba(),
                BLOCK_IMAGE_WIDTH_PX,
                BLOCK_IMAGE_HEIGHT_PX,
            ))
        })
        .clone()
}

fn build_inline_image_rgba() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((INLINE_IMAGE_SIZE_PX * INLINE_IMAGE_SIZE_PX * 4) as usize);
    let max = (INLINE_IMAGE_SIZE_PX - 1) as f32;

    for y in 0..INLINE_IMAGE_SIZE_PX {
        for x in 0..INLINE_IMAGE_SIZE_PX {
            let fx = x as f32 / max;
            let fy = y as f32 / max;
            let distance = ((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt();
            let alpha = if distance > 0.5 {
                0.0
            } else {
                1.0 - distance * 1.6
            };
            let r = (120.0 + fx * 80.0) as u8;
            let g = (180.0 + fy * 60.0) as u8;
            let b = 245u8;
            rgba.extend_from_slice(&[r, g, b, (alpha * 255.0) as u8]);
        }
    }

    rgba
}

fn build_block_image_rgba() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((BLOCK_IMAGE_WIDTH_PX * BLOCK_IMAGE_HEIGHT_PX * 4) as usize);
    let max_x = (BLOCK_IMAGE_WIDTH_PX - 1) as f32;
    let max_y = (BLOCK_IMAGE_HEIGHT_PX - 1) as f32;

    for y in 0..BLOCK_IMAGE_HEIGHT_PX {
        for x in 0..BLOCK_IMAGE_WIDTH_PX {
            let fx = x as f32 / max_x;
            let fy = y as f32 / max_y;
            let stripe = ((x / 20) + (y / 20)) % 2 == 0;
            let horizon = fy > 0.58;

            let (r, g, b) = if horizon {
                (
                    (30.0 + fx * 50.0) as u8,
                    (60.0 + fy * 120.0) as u8,
                    (90.0 + fx * 80.0) as u8,
                )
            } else if stripe {
                (
                    (28.0 + fx * 90.0) as u8,
                    (76.0 + fy * 60.0) as u8,
                    (138.0 + fx * 60.0) as u8,
                )
            } else {
                (
                    (90.0 + fx * 110.0) as u8,
                    (74.0 + fy * 70.0) as u8,
                    (126.0 + fy * 80.0) as u8,
                )
            };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }

    rgba
}

fn build_chart_verbs(size: [f32; 2]) -> Vec<PathVerb> {
    let width = size[0];
    let height = size[1];
    let baseline = height - 18.0;
    let points = [
        [16.0, baseline - 34.0],
        [58.0, baseline - 72.0],
        [102.0, baseline - 54.0],
        [148.0, baseline - 90.0],
        [194.0, baseline - 42.0],
        [236.0, baseline - 60.0],
        [width - 18.0, baseline - 18.0],
    ];

    let mut verbs = Vec::with_capacity(points.len() + 4);
    verbs.push(PathVerb::MoveTo {
        to: [16.0, baseline],
    });
    for point in points {
        verbs.push(PathVerb::LineTo { to: point });
    }
    verbs.push(PathVerb::LineTo {
        to: [width - 18.0, baseline],
    });
    verbs.push(PathVerb::Close);
    verbs
}
