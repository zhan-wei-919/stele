//! Demo document assembly reused by the async store bootstrap.

use std::sync::Arc;

use crate::ui::{
    Align, AnchorKey, AtomBaseline, BlockEmbedKind, BlockEmbedNode, BlockNode, BlockStyle,
    BorderStyle, ClipMode, DocumentTree, Edges, FlowDirection, FreeTypeRasterizer, ImageData,
    InlineAtom, InlineAtomKind, InlineAtomStyle, InlineNode, LineCap, LineHeight, LineJoin,
    LocalPaintCommand, Model, OverlayAnchor, OverlayNode, ParagraphNode, ParagraphStyle,
    PathStroke, PathVerb, StackNode, Store, StoreBootstrap, StoreDelegate, TextInputId,
    TextInputNode, TextInputStyle, TextRun, TextStyle, ViewportState, WrapMode,
};

const PAGE_BG: [f32; 4] = [0.05, 0.07, 0.08, 1.0];
const PANEL_BG: [f32; 4] = [0.11, 0.14, 0.17, 1.0];
const PANEL_BG_STRONG: [f32; 4] = [0.13, 0.18, 0.20, 1.0];
const PANEL_BG_SOFT: [f32; 4] = [0.15, 0.17, 0.21, 1.0];
const INPUT_BG: [f32; 4] = [0.04, 0.05, 0.06, 1.0];
const INPUT_BG_ACTIVE: [f32; 4] = [0.06, 0.08, 0.10, 1.0];
const BORDER_MUTED: [f32; 4] = [0.27, 0.33, 0.37, 1.0];
const BORDER_ACCENT: [f32; 4] = [0.38, 0.68, 0.72, 1.0];
const TEXT_PRIMARY: [f32; 4] = [0.94, 0.96, 0.95, 1.0];
const TEXT_SECONDARY: [f32; 4] = [0.72, 0.78, 0.80, 1.0];
const TEXT_MUTED: [f32; 4] = [0.53, 0.60, 0.63, 1.0];
const TEXT_ACCENT: [f32; 4] = [0.67, 0.88, 0.84, 1.0];
const TEXT_WARNING: [f32; 4] = [0.95, 0.75, 0.45, 1.0];

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
        let document = build_text_input_demo_tree(rasterizer.default_font_id(), logical_viewport);
        StoreBootstrap::new(document, rasterizer)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

#[derive(Clone, Copy)]
struct DemoStyles {
    title: TextStyle,
    section_title: TextStyle,
    label: TextStyle,
    body: TextStyle,
    muted: TextStyle,
    caption: TextStyle,
    input: TextStyle,
    verified: TextStyle,
    archived: TextStyle,
}

fn build_text_input_demo_tree(font_id: u32, logical_viewport: [f32; 2]) -> DocumentTree {
    let styles = build_demo_styles(font_id);
    let content_width = (logical_viewport[0] - 64.0).clamp(320.0, 920.0);
    let compact = logical_viewport[0] < 760.0;
    let search_anchor = AnchorKey::new("contact-search").expect("demo anchor must be valid");

    let root = stack(
        FlowDirection::Vertical,
        vec![stack(
            FlowDirection::Vertical,
            vec![
                build_header(&styles),
                build_search_panel(&styles, search_anchor.clone()),
                build_editor_panels(&styles, compact),
                build_footer_panel(&styles),
                build_search_overlay(&styles, search_anchor),
            ],
            BlockStyle {
                align_self: Align::Center,
                max_width: Some(content_width),
                ..BlockStyle::default()
            },
        )],
        BlockStyle {
            background: Some(PAGE_BG),
            clip: ClipMode::Rect,
            padding: page_padding(logical_viewport),
            min_height: Some(logical_viewport[1].max(360.0)),
            ..BlockStyle::default()
        },
    );

    DocumentTree::new(root).expect("demo tree must be valid")
}

fn build_header(styles: &DemoStyles) -> BlockNode {
    stack(
        FlowDirection::Horizontal,
        vec![
            build_avatar_embed(),
            paragraph(
                vec![
                    text("Contact Workbench", styles.title),
                    text("\nDraft contact records and message details.", styles.body),
                    text("\n", styles.caption),
                    chip("active", styles.caption),
                    text(" ", styles.caption),
                    text("verified", styles.verified),
                ],
                ParagraphStyle::default(),
            ),
        ],
        BlockStyle {
            background: Some(PANEL_BG_STRONG),
            clip: ClipMode::Rect,
            padding: edges(22.0, 20.0, 22.0, 20.0),
            margin: edges(0.0, 0.0, 0.0, 18.0),
            ..BlockStyle::default()
        },
    )
}

fn build_search_panel(styles: &DemoStyles, anchor_key: AnchorKey) -> BlockNode {
    anchored_stack(
        anchor_key,
        FlowDirection::Vertical,
        vec![
            paragraph(
                vec![text("Search", styles.section_title)],
                label_paragraph_style(),
            ),
            text_input(
                TextInputId::new(1),
                "Find a contact, note, or saved reply",
                styles.input,
                search_input_style(),
            ),
        ],
        BlockStyle {
            background: Some(PANEL_BG),
            clip: ClipMode::Rect,
            padding: all_edges(18.0),
            margin: edges(0.0, 0.0, 0.0, 18.0),
            ..BlockStyle::default()
        },
    )
}

fn build_editor_panels(styles: &DemoStyles, compact: bool) -> BlockNode {
    let direction = if compact {
        FlowDirection::Vertical
    } else {
        FlowDirection::Horizontal
    };

    stack(
        direction,
        vec![
            build_contact_panel(styles, compact),
            build_message_panel(styles, compact),
        ],
        BlockStyle {
            margin: edges(0.0, 0.0, 0.0, 18.0),
            ..BlockStyle::default()
        },
    )
}

fn build_contact_panel(styles: &DemoStyles, compact: bool) -> BlockNode {
    panel(
        styles,
        "Contact",
        "Primary fields",
        compact,
        false,
        vec![
            field(styles, "Name", TextInputId::new(2), "Ada Lovelace"),
            field(styles, "Email", TextInputId::new(3), "ada@example.com"),
            field(
                styles,
                "Company",
                TextInputId::new(4),
                "Analytical Engines Ltd.",
            ),
        ],
    )
}

fn build_message_panel(styles: &DemoStyles, compact: bool) -> BlockNode {
    panel(
        styles,
        "Message",
        "Single-line composition",
        compact,
        true,
        vec![
            field(styles, "Subject", TextInputId::new(5), "Quarterly planning"),
            field(styles, "Tag", TextInputId::new(6), "follow-up"),
            field(
                styles,
                "Status",
                TextInputId::new(7),
                "Waiting on design review",
            ),
        ],
    )
}

fn build_footer_panel(styles: &DemoStyles) -> BlockNode {
    stack(
        FlowDirection::Horizontal,
        vec![
            build_activity_embed(),
            stack(
                FlowDirection::Vertical,
                vec![
                    paragraph(
                        vec![
                            text("Internal note", styles.section_title),
                            text(
                                "\nKeep the next follow-up short and actionable.",
                                styles.muted,
                            ),
                            text("\n", styles.muted),
                            text("stale draft", styles.archived),
                        ],
                        ParagraphStyle::default(),
                    ),
                    text_input(
                        TextInputId::new(8),
                        "Add a short internal note",
                        styles.input,
                        input_style(TEXT_WARNING),
                    ),
                ],
                BlockStyle::default(),
            ),
        ],
        BlockStyle {
            background: Some(PANEL_BG_SOFT),
            clip: ClipMode::Rect,
            padding: all_edges(18.0),
            ..BlockStyle::default()
        },
    )
}

fn build_search_overlay(styles: &DemoStyles, target: AnchorKey) -> BlockNode {
    BlockNode::Overlay(OverlayNode::new(
        OverlayAnchor::BlockRelative {
            target,
            offset: [18.0, -12.0],
        },
        stack(
            FlowDirection::Vertical,
            vec![paragraph(
                vec![text("indexed", styles.caption)],
                label_paragraph_style(),
            )],
            BlockStyle {
                align_self: Align::Start,
                background: Some([0.12, 0.24, 0.24, 0.96]),
                clip: ClipMode::Rect,
                padding: edges(10.0, 5.0, 10.0, 5.0),
                ..BlockStyle::default()
            },
        ),
    ))
}

fn panel(
    styles: &DemoStyles,
    title: &str,
    caption: &str,
    compact: bool,
    is_last: bool,
    fields: Vec<BlockNode>,
) -> BlockNode {
    let margin = if compact && !is_last {
        edges(0.0, 0.0, 0.0, 18.0)
    } else if !compact && !is_last {
        edges(0.0, 0.0, 18.0, 0.0)
    } else {
        Edges::ZERO
    };
    let min_width = if compact { None } else { Some(300.0) };
    let max_width = Some(if compact { 920.0 } else { 430.0 });

    let mut children = Vec::with_capacity(fields.len() + 1);
    children.push(paragraph(
        vec![
            text(title, styles.section_title),
            text("\n", styles.muted),
            text(caption, styles.muted),
        ],
        ParagraphStyle {
            block: BlockStyle {
                margin: edges(0.0, 0.0, 0.0, 16.0),
                ..BlockStyle::default()
            },
            ..ParagraphStyle::default()
        },
    ));
    children.extend(fields);

    stack(
        FlowDirection::Vertical,
        children,
        BlockStyle {
            background: Some(PANEL_BG),
            clip: ClipMode::Rect,
            padding: all_edges(18.0),
            margin,
            min_width,
            max_width,
            ..BlockStyle::default()
        },
    )
}

fn field(
    styles: &DemoStyles,
    label: &str,
    text_input_id: TextInputId,
    placeholder: &str,
) -> BlockNode {
    stack(
        FlowDirection::Vertical,
        vec![
            paragraph(vec![text(label, styles.label)], label_paragraph_style()),
            text_input(
                text_input_id,
                placeholder,
                styles.input,
                input_style(BORDER_ACCENT),
            ),
        ],
        BlockStyle {
            margin: edges(0.0, 0.0, 0.0, 14.0),
            ..BlockStyle::default()
        },
    )
}

fn build_avatar_embed() -> BlockNode {
    embed(
        BlockEmbedKind::Image {
            data_ref: demo_avatar_image(),
            intrinsic_size: [48.0, 48.0],
        },
        BlockStyle {
            align_self: Align::Start,
            background: Some([0.08, 0.12, 0.13, 1.0]),
            clip: ClipMode::Rect,
            padding: edges(6.0, 6.0, 6.0, 6.0),
            margin: edges(0.0, 0.0, 18.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn build_activity_embed() -> BlockNode {
    let paint: Arc<[LocalPaintCommand]> = Arc::from([
        LocalPaintCommand::Rect {
            pos: [0.0, 0.0],
            size: [72.0, 72.0],
            color: [0.08, 0.11, 0.13, 1.0],
        },
        LocalPaintCommand::Path {
            verbs: vec![
                PathVerb::MoveTo { to: [12.0, 48.0] },
                PathVerb::CubicTo {
                    ctrl1: [22.0, 34.0],
                    ctrl2: [32.0, 58.0],
                    to: [42.0, 34.0],
                },
                PathVerb::QuadTo {
                    ctrl: [52.0, 12.0],
                    to: [60.0, 28.0],
                },
                PathVerb::LineTo { to: [60.0, 52.0] },
                PathVerb::Close,
            ],
            fill: Some([0.19, 0.38, 0.38, 1.0]),
            stroke: Some(PathStroke {
                color: TEXT_ACCENT,
                width: 1.0,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
            }),
        },
        LocalPaintCommand::Image {
            pos: [44.0, 8.0],
            size: [18.0, 18.0],
            data_ref: demo_avatar_image(),
        },
    ]);

    embed(
        BlockEmbedKind::Custom {
            intrinsic_size: [72.0, 72.0],
            paint,
        },
        BlockStyle {
            align_self: Align::Start,
            clip: ClipMode::Rect,
            margin: edges(0.0, 0.0, 18.0, 0.0),
            ..BlockStyle::default()
        },
    )
}

fn chip(label: &str, text_style: TextStyle) -> InlineNode {
    InlineNode::Atom(
        InlineAtom::new(
            InlineAtomKind::Chip {
                label: label.to_owned(),
                text_style,
            },
            InlineAtomStyle {
                margin: edges(0.0, 2.0, 4.0, 0.0),
                padding: edges(7.0, 3.0, 7.0, 3.0),
                baseline: AtomBaseline::MiddleOfLine,
                background: Some([0.08, 0.18, 0.18, 1.0]),
                border: Some(border(BORDER_ACCENT, 1.0)),
            },
        )
        .expect("demo chip must be valid"),
    )
}

fn build_demo_styles(font_id: u32) -> DemoStyles {
    let title = TextStyle::new(font_id, 30.0, TEXT_PRIMARY)
        .expect("demo title style must be valid")
        .with_bold(true)
        .with_letter_spacing(0.2)
        .expect("demo title spacing must be valid");
    let section_title = TextStyle::new(font_id, 18.0, TEXT_PRIMARY)
        .expect("demo section style must be valid")
        .with_bold(true);
    let label = TextStyle::new(font_id, 12.0, TEXT_SECONDARY)
        .expect("demo label style must be valid")
        .with_bold(true)
        .with_letter_spacing(0.6)
        .expect("demo label spacing must be valid");
    let body =
        TextStyle::new(font_id, 15.0, TEXT_SECONDARY).expect("demo body style must be valid");
    let muted = TextStyle::new(font_id, 13.0, TEXT_MUTED).expect("demo muted style must be valid");
    let caption = TextStyle::new(font_id, 12.0, TEXT_MUTED)
        .expect("demo caption style must be valid")
        .with_italic(true);
    let input =
        TextStyle::new(font_id, 15.0, TEXT_PRIMARY).expect("demo input style must be valid");
    let verified = TextStyle::new(font_id, 12.0, TEXT_ACCENT)
        .expect("demo verified style must be valid")
        .with_underline(true)
        .with_background_color(Some([0.08, 0.16, 0.15, 1.0]))
        .expect("demo verified background must be valid");
    let archived = TextStyle::new(font_id, 12.0, TEXT_MUTED)
        .expect("demo archived style must be valid")
        .with_strikethrough(true);

    DemoStyles {
        title,
        section_title,
        label,
        body,
        muted,
        caption,
        input,
        verified,
        archived,
    }
}

fn text_input(
    text_input_id: TextInputId,
    placeholder: &str,
    text_style: TextStyle,
    style: TextInputStyle,
) -> BlockNode {
    BlockNode::TextInput(
        TextInputNode::new(text_input_id, placeholder, text_style, style)
            .expect("demo text input must be valid"),
    )
}

fn input_style(border_color: [f32; 4]) -> TextInputStyle {
    TextInputStyle {
        block: BlockStyle {
            background: Some(INPUT_BG),
            clip: ClipMode::Rect,
            padding: edges(12.0, 9.0, 12.0, 9.0),
            min_width: Some(220.0),
            ..BlockStyle::default()
        },
        border: Some(border(border_color, 1.0)),
        caret_color: TEXT_ACCENT,
        selection_color: [0.18, 0.42, 0.92, 0.35],
    }
}

fn search_input_style() -> TextInputStyle {
    TextInputStyle {
        block: BlockStyle {
            background: Some(INPUT_BG_ACTIVE),
            clip: ClipMode::Rect,
            padding: edges(14.0, 10.0, 14.0, 10.0),
            min_width: Some(280.0),
            ..BlockStyle::default()
        },
        border: Some(border(BORDER_MUTED, 1.0)),
        caret_color: TEXT_ACCENT,
        selection_color: [0.18, 0.42, 0.92, 0.35],
    }
}

fn label_paragraph_style() -> ParagraphStyle {
    ParagraphStyle {
        block: BlockStyle {
            margin: edges(0.0, 0.0, 0.0, 6.0),
            ..BlockStyle::default()
        },
        line_height: LineHeight::Factor(1.15),
        wrap: WrapMode::NoWrap,
        ..ParagraphStyle::default()
    }
}

fn paragraph(inlines: Vec<InlineNode>, style: ParagraphStyle) -> BlockNode {
    BlockNode::Paragraph(ParagraphNode::new(inlines, style).expect("demo paragraph must be valid"))
}

fn text(content: impl Into<String>, style: TextStyle) -> InlineNode {
    InlineNode::Text(TextRun::new(content, style))
}

fn stack(direction: FlowDirection, children: Vec<BlockNode>, style: BlockStyle) -> BlockNode {
    BlockNode::Stack(StackNode::new(direction, children, style).expect("demo stack must be valid"))
}

fn anchored_stack(
    anchor_key: AnchorKey,
    direction: FlowDirection,
    children: Vec<BlockNode>,
    style: BlockStyle,
) -> BlockNode {
    let mut stack = StackNode::new(direction, children, style).expect("demo stack must be valid");
    stack.anchor_key = Some(anchor_key);
    BlockNode::Stack(stack)
}

fn embed(kind: BlockEmbedKind, style: BlockStyle) -> BlockNode {
    BlockNode::Embed(BlockEmbedNode::new(kind, style).expect("demo embed must be valid"))
}

fn page_padding(logical_viewport: [f32; 2]) -> Edges {
    let horizontal = if logical_viewport[0] < 520.0 {
        18.0
    } else {
        32.0
    };
    edges(horizontal, 28.0, horizontal, 28.0)
}

fn demo_avatar_image() -> Arc<ImageData> {
    Arc::new(ImageData::new(
        vec![
            80, 132, 140, 255, 102, 162, 154, 255, 226, 186, 112, 255, 238, 224, 174, 255,
        ],
        2,
        2,
    ))
}

fn border(color: [f32; 4], width: f32) -> BorderStyle {
    BorderStyle::new(color, width).expect("demo border must be valid")
}

fn all_edges(value: f32) -> Edges {
    Edges::all(value).expect("demo edges must be valid")
}

fn edges(left: f32, top: f32, right: f32, bottom: f32) -> Edges {
    Edges::new(left, top, right, bottom).expect("demo edges must be valid")
}
