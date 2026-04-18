//! Tree layout solver that positions blocks, paragraphs, embeds, and overlays.

use std::collections::HashMap;

use log::info;

use crate::layout::prepare_tree::{
    PreparedBlockNode, PreparedEmbed, PreparedEmbedPayload, PreparedOverlay, PreparedParagraph,
    PreparedStack, PreparedTree,
};
use crate::layout::tree::{Align, BlockStyle, ClipMode, OverlayAnchor};

use super::paragraph::layout_paragraph;
use super::types::{
    LayoutBlock, LayoutBlockContent, LayoutConstraints, LayoutEmbed, LayoutEmbedKind, LayoutRect,
    LayoutTree,
};

struct SolveContext<'a> {
    anchor_layouts: HashMap<crate::layout::tree::NodeId, AnchorPlacement>,
    overlays: Vec<DeferredOverlay<'a>>,
    block_count: usize,
    line_count: usize,
}

struct DeferredOverlay<'a> {
    anchor: &'a OverlayAnchor,
    child: &'a PreparedBlockNode,
    declaration_clip: LayoutRect,
    doc_order: u32,
}

#[derive(Clone, Copy)]
struct AnchorPlacement {
    rect: LayoutRect,
    z_order: Option<u32>,
}

struct ResolvedOverlayTarget {
    origin: [f32; 2],
    anchor_z_order: Option<u32>,
}

struct SolvedBlock {
    block: LayoutBlock,
    outer_size: [f32; 2],
}

/// Solves the prepared rich-text tree into concrete layout geometry.
pub(crate) fn layout_tree(prepared: &PreparedTree, constraints: LayoutConstraints) -> LayoutTree {
    let viewport_rect = constraints.viewport_rect();
    let mut context = SolveContext {
        anchor_layouts: HashMap::new(),
        overlays: Vec::new(),
        block_count: 0,
        line_count: 0,
    };
    let root = layout_flow_block(
        &prepared.root,
        [0.0, 0.0],
        constraints.max_width,
        viewport_rect,
        &mut context,
        true,
    )
    .block;

    let mut overlays = Vec::new();
    let mut overlay_index = 0usize;
    while overlay_index < context.overlays.len() {
        let (anchor, child, declaration_clip, doc_order) = {
            let deferred = &context.overlays[overlay_index];
            (
                deferred.anchor,
                deferred.child,
                deferred.declaration_clip,
                deferred.doc_order,
            )
        };
        overlay_index += 1;
        let target = resolve_overlay_target(anchor, prepared, &context.anchor_layouts);
        let Some(target) = target else {
            continue;
        };
        let mut block = layout_flow_block(
            child,
            target.origin,
            constraints.max_width,
            declaration_clip,
            &mut context,
            false,
        )
        .block;
        block.doc_order = doc_order;
        if let Some(anchor_z_order) = target.anchor_z_order {
            lift_subtree_above_z(&mut block, anchor_z_order.saturating_add(1));
        }
        overlays.push(block);
    }

    info!(
        "layout.tree.layout block_count={} line_count={}",
        context.block_count, context.line_count
    );
    LayoutTree { root, overlays }
}

fn layout_flow_block<'a>(
    node: &'a PreparedBlockNode,
    origin: [f32; 2],
    available_width: f32,
    inherited_clip: LayoutRect,
    context: &mut SolveContext<'a>,
    record_anchors: bool,
) -> SolvedBlock {
    context.block_count += 1;
    match node {
        PreparedBlockNode::Stack(stack) => layout_stack(
            stack,
            origin,
            available_width,
            inherited_clip,
            context,
            record_anchors,
        ),
        PreparedBlockNode::Paragraph(paragraph) => layout_leaf_paragraph(
            paragraph,
            origin,
            available_width,
            inherited_clip,
            context,
            record_anchors,
        ),
        PreparedBlockNode::Embed(embed) => layout_leaf_embed(
            embed,
            origin,
            available_width,
            inherited_clip,
            context,
            record_anchors,
        ),
        PreparedBlockNode::Overlay(overlay) => {
            layout_overlay_placeholder(overlay, inherited_clip, context)
        }
    }
}

fn layout_stack<'a>(
    stack: &'a PreparedStack,
    origin: [f32; 2],
    available_width: f32,
    inherited_clip: LayoutRect,
    context: &mut SolveContext<'a>,
    record_anchors: bool,
) -> SolvedBlock {
    let style = stack.style;
    let width_limit = width_limit(style, available_width);
    let content_available_width = (width_limit - style.padding.horizontal()).max(0.0);
    let overlay_start = context.overlays.len();
    let mut children = Vec::new();
    let mut pending_overlays = Vec::new();
    let mut content_width: f32 = 0.0;
    let mut content_height: f32 = 0.0;
    let mut cursor_x: f32 = 0.0;
    let mut cursor_y: f32 = 0.0;
    let mut prev_bottom_margin: f32 = 0.0;
    let mut has_flow_child = false;

    for child in &stack.children {
        if let PreparedBlockNode::Overlay(overlay) = child {
            pending_overlays.push(overlay);
            continue;
        }
        let child_style = block_style(child);
        let collapsed_top_margin = if matches!(
            stack.direction,
            crate::layout::tree::FlowDirection::Vertical
        ) {
            if has_flow_child {
                prev_bottom_margin.max(child_style.margin.top)
            } else {
                child_style.margin.top
            }
        } else {
            0.0
        };
        let child_available_width = match stack.direction {
            crate::layout::tree::FlowDirection::Vertical => {
                (content_available_width - child_style.margin.horizontal()).max(0.0)
            }
            crate::layout::tree::FlowDirection::Horizontal => {
                (content_available_width - cursor_x - child_style.margin.horizontal()).max(0.0)
            }
        };
        let child_origin = [
            origin[0] + style.padding.left + cursor_x + child_style.margin.left,
            origin[1] + style.padding.top + cursor_y + collapsed_top_margin,
        ];
        let solved_child = layout_flow_block(
            child,
            child_origin,
            child_available_width,
            inherited_clip,
            context,
            record_anchors,
        );
        let child_outer_width = solved_child.outer_size[0];
        let child_outer_height = solved_child.outer_size[1];
        match stack.direction {
            crate::layout::tree::FlowDirection::Vertical => {
                cursor_y += collapsed_top_margin + solved_child.block.rect.height();
                prev_bottom_margin = child_style.margin.bottom;
                content_width = content_width.max(child_outer_width);
                content_height = cursor_y + prev_bottom_margin;
            }
            crate::layout::tree::FlowDirection::Horizontal => {
                cursor_x += child_outer_width;
                content_width = cursor_x;
                content_height = content_height.max(child_outer_height);
            }
        }
        has_flow_child = true;
        children.push(solved_child.block);
    }

    let width = resolve_width(
        style,
        available_width,
        content_width + style.padding.horizontal(),
    );
    let height = resolve_height(style, content_height + style.padding.vertical());
    let rect = LayoutRect::new(origin[0], origin[1], width, height);
    let clip_rect = effective_clip(rect, inherited_clip, style.clip);
    for overlay in &mut context.overlays[overlay_start..] {
        overlay.declaration_clip = overlay.declaration_clip.intersect(clip_rect);
    }
    for child in &mut children {
        clamp_subtree_clip(child, clip_rect);
    }
    for overlay in pending_overlays {
        context.overlays.push(DeferredOverlay {
            anchor: &overlay.anchor,
            child: overlay.child.as_ref(),
            declaration_clip: clip_rect,
            doc_order: overlay.node_id.value() as u32,
        });
    }
    let block = LayoutBlock {
        node_id: stack.node_id,
        doc_order: stack.node_id.value() as u32,
        rect,
        clip_rect,
        z_order: style.z_index,
        background: style.background,
        content: LayoutBlockContent::Stack { children },
    };
    if record_anchors {
        context.anchor_layouts.insert(
            stack.node_id,
            AnchorPlacement {
                rect,
                z_order: subtree_max_materialized_z(&block),
            },
        );
    }

    SolvedBlock {
        outer_size: [
            width + style.margin.horizontal(),
            height + style.margin.vertical(),
        ],
        block,
    }
}

fn layout_leaf_paragraph<'a>(
    paragraph: &'a PreparedParagraph,
    origin: [f32; 2],
    available_width: f32,
    inherited_clip: LayoutRect,
    context: &mut SolveContext<'a>,
    record_anchors: bool,
) -> SolvedBlock {
    let style = paragraph.style.block;
    let width = resolve_width(style, available_width, available_width);
    let content_rect = LayoutRect::new(
        origin[0] + style.padding.left,
        origin[1] + style.padding.top,
        (width - style.padding.horizontal()).max(0.0),
        0.0,
    );
    let laid_out = layout_paragraph(paragraph, content_rect);
    context.line_count += laid_out.lines.len();
    let content_height = laid_out.rect.height();
    let height = resolve_height(style, content_height + style.padding.vertical());
    let rect = LayoutRect::new(origin[0], origin[1], width, height);
    let clip_rect = effective_clip(rect, inherited_clip, style.clip);
    let block = LayoutBlock {
        node_id: paragraph.node_id,
        doc_order: paragraph.node_id.value() as u32,
        rect,
        clip_rect,
        z_order: style.z_index,
        background: style.background,
        content: LayoutBlockContent::Paragraph(laid_out),
    };
    if record_anchors {
        context.anchor_layouts.insert(
            paragraph.node_id,
            AnchorPlacement {
                rect,
                z_order: subtree_max_materialized_z(&block),
            },
        );
    }
    SolvedBlock {
        outer_size: [
            width + style.margin.horizontal(),
            height + style.margin.vertical(),
        ],
        block,
    }
}

fn layout_leaf_embed<'a>(
    embed: &'a PreparedEmbed,
    origin: [f32; 2],
    _available_width: f32,
    inherited_clip: LayoutRect,
    context: &mut SolveContext<'a>,
    record_anchors: bool,
) -> SolvedBlock {
    let style = embed.style;
    let intrinsic_width = embed.intrinsic_size[0] + style.padding.horizontal();
    let intrinsic_height = embed.intrinsic_size[1] + style.padding.vertical();
    let width = resolve_intrinsic_width(style, intrinsic_width);
    let height = resolve_height(style, intrinsic_height);
    let rect = LayoutRect::new(origin[0], origin[1], width, height);
    let content_rect = LayoutRect::new(
        origin[0] + style.padding.left,
        origin[1] + style.padding.top,
        (width - style.padding.horizontal()).max(0.0),
        (height - style.padding.vertical()).max(0.0),
    );
    let clip_rect = effective_clip(rect, inherited_clip, style.clip);
    let kind = match &embed.payload {
        PreparedEmbedPayload::Image { data_ref } => LayoutEmbedKind::Image {
            data_ref: data_ref.clone(),
        },
        PreparedEmbedPayload::Path {
            verbs,
            fill,
            stroke,
        } => LayoutEmbedKind::Path {
            verbs: verbs.clone(),
            fill: *fill,
            stroke: *stroke,
        },
        PreparedEmbedPayload::Custom => LayoutEmbedKind::Custom,
    };
    let block = LayoutBlock {
        node_id: embed.node_id,
        doc_order: embed.node_id.value() as u32,
        rect,
        clip_rect,
        z_order: style.z_index,
        background: style.background,
        content: LayoutBlockContent::Embed(LayoutEmbed {
            rect: content_rect,
            kind,
            intrinsic_size: embed.intrinsic_size,
        }),
    };
    if record_anchors {
        context.anchor_layouts.insert(
            embed.node_id,
            AnchorPlacement {
                rect,
                z_order: subtree_max_materialized_z(&block),
            },
        );
    }
    SolvedBlock {
        outer_size: [
            width + style.margin.horizontal(),
            height + style.margin.vertical(),
        ],
        block,
    }
}

fn layout_overlay_placeholder<'a>(
    overlay: &'a PreparedOverlay,
    inherited_clip: LayoutRect,
    context: &mut SolveContext<'a>,
) -> SolvedBlock {
    context.overlays.push(DeferredOverlay {
        anchor: &overlay.anchor,
        child: overlay.child.as_ref(),
        declaration_clip: inherited_clip,
        doc_order: overlay.node_id.value() as u32,
    });
    SolvedBlock {
        block: LayoutBlock {
            node_id: overlay.node_id,
            doc_order: overlay.node_id.value() as u32,
            rect: LayoutRect::new(0.0, 0.0, 0.0, 0.0),
            clip_rect: inherited_clip,
            z_order: 0,
            background: None,
            content: LayoutBlockContent::Stack {
                children: Vec::new(),
            },
        },
        outer_size: [0.0, 0.0],
    }
}

fn clamp_subtree_clip(block: &mut LayoutBlock, clip_rect: LayoutRect) {
    block.clip_rect = block.clip_rect.intersect(clip_rect);
    if let LayoutBlockContent::Stack { children } = &mut block.content {
        for child in children {
            clamp_subtree_clip(child, clip_rect);
        }
    }
}

fn resolve_overlay_target(
    anchor: &OverlayAnchor,
    prepared: &PreparedTree,
    anchor_layouts: &HashMap<crate::layout::tree::NodeId, AnchorPlacement>,
) -> Option<ResolvedOverlayTarget> {
    match anchor {
        OverlayAnchor::Viewport { offset } => Some(ResolvedOverlayTarget {
            origin: *offset,
            anchor_z_order: None,
        }),
        OverlayAnchor::BlockRelative { target, offset } => prepared
            .anchor_index
            .get(target)
            .and_then(|node_id| anchor_layouts.get(node_id))
            .map(|placement| ResolvedOverlayTarget {
                origin: [
                    placement.rect.x() + offset[0],
                    placement.rect.y() + offset[1],
                ],
                anchor_z_order: placement.z_order,
            }),
    }
}

fn effective_clip(rect: LayoutRect, inherited_clip: LayoutRect, clip_mode: ClipMode) -> LayoutRect {
    match clip_mode {
        ClipMode::None => inherited_clip,
        ClipMode::Rect => inherited_clip.intersect(rect),
    }
}

fn block_style(node: &PreparedBlockNode) -> BlockStyle {
    match node {
        PreparedBlockNode::Stack(stack) => stack.style,
        PreparedBlockNode::Paragraph(paragraph) => paragraph.style.block,
        PreparedBlockNode::Embed(embed) => embed.style,
        PreparedBlockNode::Overlay(_) => BlockStyle::default(),
    }
}

fn width_limit(style: BlockStyle, available_width: f32) -> f32 {
    style
        .max_width
        .map_or(available_width, |max_width| available_width.min(max_width))
}

fn resolve_width(style: BlockStyle, available_width: f32, content_width: f32) -> f32 {
    let limit = width_limit(style, available_width);
    let base = if style.align_self == Align::Stretch {
        limit
    } else {
        content_width.min(limit)
    };
    let width = style
        .min_width
        .map_or(base, |min_width| base.max(min_width));
    width.max(0.0)
}

fn resolve_intrinsic_width(style: BlockStyle, intrinsic_width: f32) -> f32 {
    let mut width = intrinsic_width.max(0.0);
    if let Some(min_width) = style.min_width {
        width = width.max(min_width);
    }
    if let Some(max_width) = style.max_width {
        width = width.min(max_width);
    }
    width
}

fn resolve_height(style: BlockStyle, content_height: f32) -> f32 {
    let mut height = style
        .min_height
        .map_or(content_height, |min_height| content_height.max(min_height));
    if let Some(max_height) = style.max_height {
        height = height.min(max_height);
    }
    height.max(0.0)
}

fn lift_subtree_above_z(block: &mut LayoutBlock, min_z_order: u32) {
    let Some(current_min_z) = subtree_min_materialized_z(block) else {
        return;
    };
    let delta = min_z_order.saturating_sub(current_min_z);
    if delta == 0 {
        return;
    }
    offset_subtree_z(block, delta);
}

fn subtree_min_materialized_z(block: &LayoutBlock) -> Option<u32> {
    let self_z = block_materializes(block).then_some(block.z_order);
    match &block.content {
        LayoutBlockContent::Stack { children } => children.iter().fold(self_z, |current, child| {
            match (current, subtree_min_materialized_z(child)) {
                (Some(current), Some(child)) => Some(current.min(child)),
                (Some(current), None) => Some(current),
                (None, Some(child)) => Some(child),
                (None, None) => None,
            }
        }),
        LayoutBlockContent::Paragraph(_) | LayoutBlockContent::Embed(_) => self_z,
    }
}

fn subtree_max_materialized_z(block: &LayoutBlock) -> Option<u32> {
    let self_z = block_materializes(block).then_some(block.z_order);
    match &block.content {
        LayoutBlockContent::Stack { children } => children.iter().fold(self_z, |current, child| {
            match (current, subtree_max_materialized_z(child)) {
                (Some(current), Some(child)) => Some(current.max(child)),
                (Some(current), None) => Some(current),
                (None, Some(child)) => Some(child),
                (None, None) => None,
            }
        }),
        LayoutBlockContent::Paragraph(_) | LayoutBlockContent::Embed(_) => self_z,
    }
}

fn offset_subtree_z(block: &mut LayoutBlock, delta: u32) {
    block.z_order = block.z_order.saturating_add(delta);
    if let LayoutBlockContent::Stack { children } = &mut block.content {
        for child in children {
            offset_subtree_z(child, delta);
        }
    }
}

fn block_materializes(block: &LayoutBlock) -> bool {
    match &block.content {
        LayoutBlockContent::Stack { .. } => block.background.is_some(),
        LayoutBlockContent::Paragraph(paragraph) => {
            block.background.is_some()
                || paragraph.lines.iter().any(|line| {
                    line.runs.iter().any(|run| match run {
                        super::types::LayoutRun::Text(run) => {
                            !run.glyphs.is_empty() || !run.decoration_rects.is_empty()
                        }
                        super::types::LayoutRun::Atom(run) => {
                            !matches!(run.payload, super::types::LayoutAtomPayload::Custom)
                        }
                    })
                })
        }
        LayoutBlockContent::Embed(embed) => {
            block.background.is_some() || !matches!(embed.kind, LayoutEmbedKind::Custom)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::draw_list::ImageData;
    use crate::layout::prepare_tree::prepare_tree;
    use crate::layout::tree::{
        AnchorKey, BlockEmbedKind, BlockEmbedNode, BlockNode, BlockStyle, ClipMode, DocumentTree,
        Edges, FlowDirection, InlineNode, OverlayAnchor, OverlayNode, ParagraphNode,
        ParagraphStyle, StackNode, TextRun, TextStyle,
    };

    use super::{layout_tree, LayoutBlockContent, LayoutConstraints};
    use crate::font::{FontDiscovery, FreeTypeRasterizer};
    use crate::renderer::subpixel::detect_subpixel_layout;

    fn rasterizer() -> FreeTypeRasterizer {
        let font_discovery = FontDiscovery::new().expect("fonts must exist");
        FreeTypeRasterizer::new(font_discovery, detect_subpixel_layout())
            .expect("rasterizer must initialize")
    }

    #[test]
    fn overlays_resolve_against_anchor_rects() {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let mut paragraph = ParagraphNode::new(
            vec![InlineNode::Text(TextRun::new("anchor", style))],
            ParagraphStyle::default(),
        )
        .expect("paragraph must be valid");
        paragraph.anchor_key = Some(AnchorKey::new("anchor").expect("anchor"));
        let overlay_child = BlockNode::Embed(
            BlockEmbedNode::new(
                BlockEmbedKind::Image {
                    data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                    intrinsic_size: [16.0, 16.0],
                },
                BlockStyle::default(),
            )
            .expect("embed must be valid"),
        );
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![
                    BlockNode::Paragraph(paragraph),
                    BlockNode::Overlay(OverlayNode::new(
                        OverlayAnchor::BlockRelative {
                            target: AnchorKey::new("anchor").expect("anchor"),
                            offset: [8.0, 4.0],
                        },
                        overlay_child,
                    )),
                ],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(
            &prepared,
            LayoutConstraints::new(200.0, Some(100.0), 1.0, [200.0, 100.0]),
        );
        assert_eq!(laid_out.overlays.len(), 1);
        assert!(laid_out.overlays[0].rect.x() >= laid_out.root.rect.x() + 8.0);
    }

    #[test]
    fn embed_uses_intrinsic_width_instead_of_stretching_to_container() {
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Embed(
                    BlockEmbedNode::new(
                        BlockEmbedKind::Image {
                            data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                            intrinsic_size: [24.0, 12.0],
                        },
                        BlockStyle::default(),
                    )
                    .expect("embed must be valid"),
                )],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(
            &prepared,
            LayoutConstraints::new(200.0, Some(100.0), 1.0, [200.0, 100.0]),
        );
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        assert_eq!(children.len(), 1);
        assert!((children[0].rect.width() - 24.0).abs() < 0.01);
    }

    #[test]
    fn overlay_subtree_is_lifted_above_anchor_z_order() {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let mut anchor = ParagraphNode::new(
            vec![InlineNode::Text(TextRun::new("anchor", style))],
            ParagraphStyle {
                block: BlockStyle {
                    z_index: 5,
                    ..BlockStyle::default()
                },
                ..ParagraphStyle::default()
            },
        )
        .expect("paragraph must be valid");
        anchor.anchor_key = Some(AnchorKey::new("anchor").expect("anchor"));
        let overlay_child = BlockNode::Embed(
            BlockEmbedNode::new(
                BlockEmbedKind::Image {
                    data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                    intrinsic_size: [16.0, 16.0],
                },
                BlockStyle::default(),
            )
            .expect("embed must be valid"),
        );
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![
                    BlockNode::Paragraph(anchor),
                    BlockNode::Overlay(OverlayNode::new(
                        OverlayAnchor::BlockRelative {
                            target: AnchorKey::new("anchor").expect("anchor"),
                            offset: [0.0, 0.0],
                        },
                        overlay_child,
                    )),
                ],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(
            &prepared,
            LayoutConstraints::new(200.0, Some(100.0), 1.0, [200.0, 100.0]),
        );
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        assert_eq!(children[0].z_order, 5);
        assert!(laid_out.overlays[0].z_order > children[0].z_order);
    }

    #[test]
    fn nested_overlay_inherits_effective_clip_from_clipped_ancestors() {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let mut paragraph = ParagraphNode::new(
            vec![InlineNode::Text(TextRun::new("anchor", style))],
            ParagraphStyle::default(),
        )
        .expect("paragraph must be valid");
        paragraph.anchor_key = Some(AnchorKey::new("anchor").expect("anchor"));
        let nested = BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![
                    BlockNode::Paragraph(paragraph),
                    BlockNode::Overlay(OverlayNode::new(
                        OverlayAnchor::BlockRelative {
                            target: AnchorKey::new("anchor").expect("anchor"),
                            offset: [0.0, 0.0],
                        },
                        BlockNode::Embed(
                            BlockEmbedNode::new(
                                BlockEmbedKind::Image {
                                    data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                                    intrinsic_size: [120.0, 16.0],
                                },
                                BlockStyle::default(),
                            )
                            .expect("embed must be valid"),
                        ),
                    )),
                ],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        );
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![nested],
                BlockStyle {
                    clip: ClipMode::Rect,
                    max_width: Some(80.0),
                    ..BlockStyle::default()
                },
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(
            &prepared,
            LayoutConstraints::new(200.0, Some(100.0), 1.0, [200.0, 100.0]),
        );
        assert_eq!(laid_out.overlays.len(), 1);
        assert!(laid_out.overlays[0].clip_rect.right() <= laid_out.root.clip_rect.right() + 0.01);
        assert!(laid_out.overlays[0].clip_rect.width() < laid_out.overlays[0].rect.width());
    }

    #[test]
    fn overlay_anchor_without_materialized_scene_content_does_not_raise_overlay_z() {
        let mut anchor = StackNode::new(
            FlowDirection::Vertical,
            Vec::new(),
            BlockStyle {
                z_index: 9,
                ..BlockStyle::default()
            },
        )
        .expect("stack must be valid");
        anchor.anchor_key = Some(AnchorKey::new("anchor").expect("anchor"));
        let overlay_child = BlockNode::Embed(
            BlockEmbedNode::new(
                BlockEmbedKind::Image {
                    data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                    intrinsic_size: [16.0, 16.0],
                },
                BlockStyle {
                    z_index: 2,
                    ..BlockStyle::default()
                },
            )
            .expect("embed must be valid"),
        );
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![
                    BlockNode::Stack(anchor),
                    BlockNode::Overlay(OverlayNode::new(
                        OverlayAnchor::BlockRelative {
                            target: AnchorKey::new("anchor").expect("anchor"),
                            offset: [0.0, 0.0],
                        },
                        overlay_child,
                    )),
                ],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(
            &prepared,
            LayoutConstraints::new(200.0, Some(100.0), 1.0, [200.0, 100.0]),
        );
        assert_eq!(laid_out.overlays.len(), 1);
        assert_eq!(laid_out.overlays[0].z_order, 2);
    }

    #[test]
    fn vertical_stack_collapses_adjacent_margins() {
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![
                    BlockNode::Embed(
                        BlockEmbedNode::new(
                            BlockEmbedKind::Image {
                                data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                                intrinsic_size: [10.0, 10.0],
                            },
                            BlockStyle {
                                margin: Edges::new(0.0, 0.0, 0.0, 12.0)
                                    .expect("edges must be valid"),
                                ..BlockStyle::default()
                            },
                        )
                        .expect("embed must be valid"),
                    ),
                    BlockNode::Embed(
                        BlockEmbedNode::new(
                            BlockEmbedKind::Image {
                                data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                                intrinsic_size: [10.0, 10.0],
                            },
                            BlockStyle {
                                margin: Edges::new(0.0, 20.0, 0.0, 0.0)
                                    .expect("edges must be valid"),
                                ..BlockStyle::default()
                            },
                        )
                        .expect("embed must be valid"),
                    ),
                ],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(
            &prepared,
            LayoutConstraints::new(200.0, Some(100.0), 1.0, [200.0, 100.0]),
        );
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        assert_eq!(children.len(), 2);
        let gap = children[1].rect.y() - children[0].rect.bottom();
        assert!(
            (gap - 20.0).abs() < 0.01,
            "expected collapsed margin gap, got {gap}"
        );
    }

    #[test]
    fn paragraph_layout_rect_height_tracks_actual_lines() {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Paragraph(
                    ParagraphNode::new(
                        vec![InlineNode::Text(TextRun::new(
                            "wrapped paragraph content should occupy multiple lines in a narrow width",
                            style,
                        ))],
                        ParagraphStyle::default(),
                    )
                    .expect("paragraph must be valid"),
                )],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(
            &prepared,
            LayoutConstraints::new(80.0, Some(120.0), 1.0, [80.0, 120.0]),
        );
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        let LayoutBlockContent::Paragraph(paragraph) = &children[0].content else {
            panic!("child must be paragraph");
        };
        assert!(paragraph.lines.len() > 1);
        assert!(paragraph.rect.height() > 0.0);
    }
}
