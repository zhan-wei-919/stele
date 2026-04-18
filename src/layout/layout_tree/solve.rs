//! Tree layout solver that positions blocks, paragraphs, embeds, and overlays.

use std::collections::HashMap;

use log::info;

use crate::layout::prepare_tree::{
    PreparedBlockNode, PreparedEmbed, PreparedEmbedPayload, PreparedOverlay, PreparedParagraph,
    PreparedStack, PreparedTree,
};
use crate::layout::tree::{Align, BlockStyle, ClipMode, FlowDirection, OverlayAnchor};

use super::paragraph::{layout_paragraph, measure_paragraph_content_width};
use super::types::{
    LayoutBlock, LayoutBlockContent, LayoutConstraints, LayoutEmbed, LayoutEmbedKind, LayoutRect,
    LayoutTree, ScrollAnchor,
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
    scroll_anchor: ScrollAnchor,
}

struct SolvedBlock {
    block: LayoutBlock,
    outer_size: [f32; 2],
}

#[derive(Clone, Copy)]
enum ChildWidthMode {
    Default,
    VerticalFitContent,
    VerticalStretch,
}

struct MeasuredChild<'a> {
    child: &'a PreparedBlockNode,
    style: BlockStyle,
    available_width: f32,
    collapsed_top_margin: f32,
    width_mode: ChildWidthMode,
    outer_size: [f32; 2],
}

struct StackMeasureResult<'a> {
    measured_children: Vec<MeasuredChild<'a>>,
    pending_overlays: Vec<&'a PreparedOverlay>,
    content_size: [f32; 2],
}

/// Solves the prepared rich-text tree into concrete layout geometry.
pub(crate) fn layout_tree(prepared: &PreparedTree, constraints: LayoutConstraints) -> LayoutTree {
    let document_clip_rect = constraints.document_clip_rect();
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
        document_clip_rect,
        &mut context,
        true,
        ChildWidthMode::Default,
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
            ChildWidthMode::Default,
        )
        .block;
        set_subtree_scroll_anchor(&mut block, target.scroll_anchor);
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
    width_mode: ChildWidthMode,
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
            width_mode,
        ),
        PreparedBlockNode::Paragraph(paragraph) => layout_leaf_paragraph(
            paragraph,
            origin,
            available_width,
            inherited_clip,
            context,
            record_anchors,
            width_mode,
        ),
        PreparedBlockNode::Embed(embed) => layout_leaf_embed(
            embed,
            origin,
            available_width,
            inherited_clip,
            context,
            record_anchors,
            width_mode,
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
    _width_mode: ChildWidthMode,
) -> SolvedBlock {
    let style = stack.style;
    let overlay_start = context.overlays.len();
    let StackMeasureResult {
        measured_children,
        pending_overlays,
        content_size,
    } = measure_stack_children(stack, stack_content_available_width(style, available_width));
    let [width, height] = resolve_stack_size(style, available_width, content_size);
    let rect = LayoutRect::new(origin[0], origin[1], width, height);
    let clip_rect = effective_clip(rect, inherited_clip, style.clip);
    let children = place_stack_children(
        measured_children,
        stack.direction,
        origin,
        style,
        [
            content_box_width(width, style),
            content_box_height(height, style),
        ],
        clip_rect,
        context,
        record_anchors,
    );
    finalize_stack_overlays(context, overlay_start, pending_overlays, clip_rect);
    let block = stack_block(stack, rect, clip_rect, style, children);
    record_anchor_layout(context, stack.node_id, rect, &block, record_anchors);
    SolvedBlock {
        outer_size: solved_outer_size(style, width, height),
        block,
    }
}

fn finalize_stack_overlays<'a>(
    context: &mut SolveContext<'a>,
    overlay_start: usize,
    pending_overlays: Vec<&'a PreparedOverlay>,
    clip_rect: LayoutRect,
) {
    intersect_deferred_overlay_clips(context, overlay_start, clip_rect);
    enqueue_pending_overlays(context, pending_overlays, clip_rect);
}

fn intersect_deferred_overlay_clips(
    context: &mut SolveContext<'_>,
    overlay_start: usize,
    clip_rect: LayoutRect,
) {
    for overlay in &mut context.overlays[overlay_start..] {
        overlay.declaration_clip = overlay.declaration_clip.intersect(clip_rect);
    }
}

fn enqueue_pending_overlays<'a>(
    context: &mut SolveContext<'a>,
    pending_overlays: Vec<&'a PreparedOverlay>,
    clip_rect: LayoutRect,
) {
    for overlay in pending_overlays {
        context.overlays.push(DeferredOverlay {
            anchor: &overlay.anchor,
            child: overlay.child.as_ref(),
            declaration_clip: clip_rect,
            doc_order: overlay.node_id.value() as u32,
        });
    }
}

fn stack_block(
    stack: &PreparedStack,
    rect: LayoutRect,
    clip_rect: LayoutRect,
    style: BlockStyle,
    children: Vec<LayoutBlock>,
) -> LayoutBlock {
    LayoutBlock {
        node_id: stack.node_id,
        doc_order: stack.node_id.value() as u32,
        rect,
        clip_rect,
        scroll_anchor: ScrollAnchor::FollowsContent,
        z_order: style.z_index,
        background: style.background,
        content: LayoutBlockContent::Stack { children },
    }
}

fn measure_stack_children<'a>(
    stack: &'a PreparedStack,
    content_available_width: f32,
) -> StackMeasureResult<'a> {
    let mut measured_children = Vec::new();
    let mut pending_overlays = Vec::new();
    let mut content_width = 0.0;
    let mut content_height = 0.0;
    let mut cursor_x = 0.0;
    let mut cursor_y = 0.0;
    let mut prev_bottom_margin = 0.0;
    let mut has_flow_child = false;

    for child in &stack.children {
        if let PreparedBlockNode::Overlay(overlay) = child {
            pending_overlays.push(overlay);
            continue;
        }
        let measured = measure_stack_child(
            child,
            stack.direction,
            content_available_width,
            cursor_x,
            prev_bottom_margin,
            has_flow_child,
        );
        update_stack_content_metrics(
            stack.direction,
            measured.style,
            measured.outer_size,
            measured.collapsed_top_margin,
            &mut cursor_x,
            &mut cursor_y,
            &mut prev_bottom_margin,
            &mut content_width,
            &mut content_height,
        );
        has_flow_child = true;
        measured_children.push(measured);
    }

    StackMeasureResult {
        measured_children,
        pending_overlays,
        content_size: [content_width, content_height],
    }
}

fn measure_stack_child<'a>(
    child: &'a PreparedBlockNode,
    direction: FlowDirection,
    content_available_width: f32,
    cursor_x: f32,
    prev_bottom_margin: f32,
    has_flow_child: bool,
) -> MeasuredChild<'a> {
    let style = block_style(child);
    let collapsed_top_margin =
        collapsed_top_margin(direction, style, prev_bottom_margin, has_flow_child);
    let available_width =
        stack_child_available_width(direction, style, content_available_width, cursor_x);
    let width_mode = child_width_mode(direction, style.align_self);
    let outer_size = measure_flow_block(child, available_width, width_mode);
    MeasuredChild {
        child,
        style,
        available_width,
        collapsed_top_margin,
        width_mode,
        outer_size,
    }
}

fn place_stack_children<'a>(
    measured_children: Vec<MeasuredChild<'a>>,
    direction: FlowDirection,
    origin: [f32; 2],
    style: BlockStyle,
    content_box_size: [f32; 2],
    clip_rect: LayoutRect,
    context: &mut SolveContext<'a>,
    record_anchors: bool,
) -> Vec<LayoutBlock> {
    let mut children = Vec::with_capacity(measured_children.len());
    let mut cursor_x = 0.0;
    let mut cursor_y = 0.0;

    for measured in measured_children {
        let child_origin = stack_child_origin(
            direction,
            origin,
            style,
            content_box_size,
            cursor_x,
            cursor_y,
            &measured,
        );
        let solved_child = layout_flow_block(
            measured.child,
            child_origin,
            measured.available_width,
            clip_rect,
            context,
            record_anchors,
            measured.width_mode,
        );
        match direction {
            FlowDirection::Vertical => {
                cursor_y += measured.collapsed_top_margin + solved_child.block.rect.height();
            }
            FlowDirection::Horizontal => {
                cursor_x += solved_child.outer_size[0];
            }
        }
        children.push(solved_child.block);
    }

    children
}

fn stack_child_origin(
    direction: FlowDirection,
    origin: [f32; 2],
    style: BlockStyle,
    content_box_size: [f32; 2],
    cursor_x: f32,
    cursor_y: f32,
    measured: &MeasuredChild<'_>,
) -> [f32; 2] {
    let cross_offset = stack_child_cross_offset(direction, content_box_size, measured);
    match direction {
        FlowDirection::Vertical => [
            origin[0] + style.padding.left + cross_offset + measured.style.margin.left,
            origin[1] + style.padding.top + cursor_y + measured.collapsed_top_margin,
        ],
        FlowDirection::Horizontal => [
            origin[0] + style.padding.left + cursor_x + measured.style.margin.left,
            origin[1] + style.padding.top + cross_offset + measured.style.margin.top,
        ],
    }
}

fn stack_child_cross_offset(
    direction: FlowDirection,
    content_box_size: [f32; 2],
    measured: &MeasuredChild<'_>,
) -> f32 {
    match direction {
        FlowDirection::Vertical => cross_axis_offset(
            measured.style.align_self,
            content_box_size[0],
            measured.outer_size[0],
        ),
        FlowDirection::Horizontal => cross_axis_offset(
            measured.style.align_self,
            content_box_size[1],
            measured.outer_size[1],
        ),
    }
}

fn update_stack_content_metrics(
    direction: FlowDirection,
    style: BlockStyle,
    outer_size: [f32; 2],
    collapsed_top_margin: f32,
    cursor_x: &mut f32,
    cursor_y: &mut f32,
    prev_bottom_margin: &mut f32,
    content_width: &mut f32,
    content_height: &mut f32,
) {
    match direction {
        FlowDirection::Vertical => {
            *cursor_y += collapsed_top_margin + (outer_size[1] - style.margin.vertical());
            *prev_bottom_margin = style.margin.bottom;
            *content_width = content_width.max(outer_size[0]);
            *content_height = *cursor_y + *prev_bottom_margin;
        }
        FlowDirection::Horizontal => {
            *cursor_x += outer_size[0];
            *content_width = *cursor_x;
            *content_height = content_height.max(outer_size[1]);
        }
    }
}

fn layout_leaf_paragraph<'a>(
    paragraph: &'a PreparedParagraph,
    origin: [f32; 2],
    available_width: f32,
    inherited_clip: LayoutRect,
    context: &mut SolveContext<'a>,
    record_anchors: bool,
    width_mode: ChildWidthMode,
) -> SolvedBlock {
    let style = paragraph.style.block;
    let width = resolve_paragraph_width(paragraph, available_width, width_mode);
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
        scroll_anchor: ScrollAnchor::FollowsContent,
        z_order: style.z_index,
        background: style.background,
        content: LayoutBlockContent::Paragraph(laid_out),
    };
    record_anchor_layout(context, paragraph.node_id, rect, &block, record_anchors);
    SolvedBlock {
        outer_size: solved_outer_size(style, width, height),
        block,
    }
}

fn layout_leaf_embed<'a>(
    embed: &'a PreparedEmbed,
    origin: [f32; 2],
    available_width: f32,
    inherited_clip: LayoutRect,
    context: &mut SolveContext<'a>,
    record_anchors: bool,
    width_mode: ChildWidthMode,
) -> SolvedBlock {
    let style = embed.style;
    let intrinsic_width = embed.intrinsic_size[0] + style.padding.horizontal();
    let intrinsic_height = embed.intrinsic_size[1] + style.padding.vertical();
    let width = resolve_embed_width(style, available_width, intrinsic_width, width_mode);
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
        PreparedEmbedPayload::Custom { paint } => LayoutEmbedKind::Custom {
            paint: paint.clone(),
        },
    };
    let block = LayoutBlock {
        node_id: embed.node_id,
        doc_order: embed.node_id.value() as u32,
        rect,
        clip_rect,
        scroll_anchor: ScrollAnchor::FollowsContent,
        z_order: style.z_index,
        background: style.background,
        content: LayoutBlockContent::Embed(LayoutEmbed {
            rect: content_rect,
            kind,
            intrinsic_size: embed.intrinsic_size,
        }),
    };
    record_anchor_layout(context, embed.node_id, rect, &block, record_anchors);
    SolvedBlock {
        outer_size: solved_outer_size(style, width, height),
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
            scroll_anchor: ScrollAnchor::FollowsContent,
            z_order: 0,
            background: None,
            content: LayoutBlockContent::Stack {
                children: Vec::new(),
            },
        },
        outer_size: [0.0, 0.0],
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
            scroll_anchor: ScrollAnchor::FixedToViewport,
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
                scroll_anchor: ScrollAnchor::FollowsContent,
            }),
    }
}

fn set_subtree_scroll_anchor(block: &mut LayoutBlock, scroll_anchor: ScrollAnchor) {
    block.scroll_anchor = scroll_anchor;
    if let LayoutBlockContent::Stack { children } = &mut block.content {
        for child in children {
            set_subtree_scroll_anchor(child, scroll_anchor);
        }
    }
}

fn effective_clip(rect: LayoutRect, inherited_clip: LayoutRect, clip_mode: ClipMode) -> LayoutRect {
    match clip_mode {
        ClipMode::None => inherited_clip,
        ClipMode::Rect => inherited_clip.intersect(rect),
    }
}

fn measure_flow_block(
    node: &PreparedBlockNode,
    available_width: f32,
    width_mode: ChildWidthMode,
) -> [f32; 2] {
    match node {
        PreparedBlockNode::Stack(stack) => measure_stack(stack, available_width),
        PreparedBlockNode::Paragraph(paragraph) => {
            measure_leaf_paragraph(paragraph, available_width, width_mode)
        }
        PreparedBlockNode::Embed(embed) => measure_leaf_embed(embed, available_width, width_mode),
        PreparedBlockNode::Overlay(_) => [0.0, 0.0],
    }
}

fn measure_stack(stack: &PreparedStack, available_width: f32) -> [f32; 2] {
    let style = stack.style;
    let measurement =
        measure_stack_children(stack, stack_content_available_width(style, available_width));
    stack_outer_size(style, available_width, measurement.content_size)
}

fn measure_leaf_paragraph(
    paragraph: &PreparedParagraph,
    available_width: f32,
    width_mode: ChildWidthMode,
) -> [f32; 2] {
    let style = paragraph.style.block;
    let width = resolve_paragraph_width(paragraph, available_width, width_mode);
    let content_rect =
        LayoutRect::new(0.0, 0.0, (width - style.padding.horizontal()).max(0.0), 0.0);
    let content_height = layout_paragraph(paragraph, content_rect).rect.height();
    let height = resolve_height(style, content_height + style.padding.vertical());
    [
        width + style.margin.horizontal(),
        height + style.margin.vertical(),
    ]
}

fn measure_leaf_embed(
    embed: &PreparedEmbed,
    available_width: f32,
    width_mode: ChildWidthMode,
) -> [f32; 2] {
    let style = embed.style;
    let intrinsic_width = embed.intrinsic_size[0] + style.padding.horizontal();
    let intrinsic_height = embed.intrinsic_size[1] + style.padding.vertical();
    let width = resolve_embed_width(style, available_width, intrinsic_width, width_mode);
    let height = resolve_height(style, intrinsic_height);
    [
        width + style.margin.horizontal(),
        height + style.margin.vertical(),
    ]
}

fn block_style(node: &PreparedBlockNode) -> BlockStyle {
    match node {
        PreparedBlockNode::Stack(stack) => stack.style,
        PreparedBlockNode::Paragraph(paragraph) => paragraph.style.block,
        PreparedBlockNode::Embed(embed) => embed.style,
        PreparedBlockNode::Overlay(_) => BlockStyle::default(),
    }
}

fn record_anchor_layout(
    context: &mut SolveContext<'_>,
    node_id: crate::layout::tree::NodeId,
    rect: LayoutRect,
    block: &LayoutBlock,
    record_anchors: bool,
) {
    if record_anchors {
        context.anchor_layouts.insert(
            node_id,
            AnchorPlacement {
                rect,
                z_order: subtree_max_materialized_z(block),
            },
        );
    }
}

fn stack_content_available_width(style: BlockStyle, available_width: f32) -> f32 {
    let width_limit = width_limit(style, available_width);
    (width_limit - style.padding.horizontal()).max(0.0)
}

fn stack_child_available_width(
    direction: FlowDirection,
    style: BlockStyle,
    content_available_width: f32,
    cursor_x: f32,
) -> f32 {
    match direction {
        FlowDirection::Vertical => (content_available_width - style.margin.horizontal()).max(0.0),
        FlowDirection::Horizontal => {
            (content_available_width - cursor_x - style.margin.horizontal()).max(0.0)
        }
    }
}

fn collapsed_top_margin(
    direction: FlowDirection,
    style: BlockStyle,
    prev_bottom_margin: f32,
    has_flow_child: bool,
) -> f32 {
    if direction != FlowDirection::Vertical {
        return 0.0;
    }
    if has_flow_child {
        prev_bottom_margin.max(style.margin.top)
    } else {
        style.margin.top
    }
}

fn resolve_stack_size(style: BlockStyle, available_width: f32, content_size: [f32; 2]) -> [f32; 2] {
    [
        resolve_width(
            style,
            available_width,
            content_size[0] + style.padding.horizontal(),
        ),
        resolve_height(style, content_size[1] + style.padding.vertical()),
    ]
}

fn stack_outer_size(style: BlockStyle, available_width: f32, content_size: [f32; 2]) -> [f32; 2] {
    let [width, height] = resolve_stack_size(style, available_width, content_size);
    [
        width + style.margin.horizontal(),
        height + style.margin.vertical(),
    ]
}

fn content_box_width(width: f32, style: BlockStyle) -> f32 {
    (width - style.padding.horizontal()).max(0.0)
}

fn content_box_height(height: f32, style: BlockStyle) -> f32 {
    (height - style.padding.vertical()).max(0.0)
}

fn solved_outer_size(style: BlockStyle, width: f32, height: f32) -> [f32; 2] {
    [
        width + style.margin.horizontal(),
        height + style.margin.vertical(),
    ]
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
    let min_width = style
        .min_width
        .map_or(0.0, |min_width| min_width.min(limit));
    let width = base.max(min_width);
    width.max(0.0)
}

fn resolve_paragraph_width(
    paragraph: &PreparedParagraph,
    available_width: f32,
    width_mode: ChildWidthMode,
) -> f32 {
    let style = paragraph.style.block;
    let content_width = if matches!(width_mode, ChildWidthMode::VerticalFitContent) {
        let width_limit = width_limit(style, available_width);
        let available_content_width = (width_limit - style.padding.horizontal()).max(0.0);
        measure_paragraph_content_width(paragraph, available_content_width)
            + style.padding.horizontal()
    } else {
        available_width
    };
    resolve_width(style, available_width, content_width)
}

fn resolve_embed_width(
    style: BlockStyle,
    available_width: f32,
    intrinsic_width: f32,
    width_mode: ChildWidthMode,
) -> f32 {
    match width_mode {
        ChildWidthMode::VerticalStretch => resolve_width(style, available_width, available_width),
        ChildWidthMode::Default | ChildWidthMode::VerticalFitContent => {
            resolve_intrinsic_width(style, intrinsic_width)
        }
    }
}

fn resolve_intrinsic_width(style: BlockStyle, intrinsic_width: f32) -> f32 {
    let parent_limit = width_limit(style, f32::INFINITY);
    let mut width = intrinsic_width.max(0.0);
    if let Some(min_width) = style.min_width {
        width = width.max(min_width.min(parent_limit));
    }
    if let Some(max_width) = style.max_width {
        width = width.min(max_width);
    }
    width
}

fn child_width_mode(
    direction: crate::layout::tree::FlowDirection,
    align_self: Align,
) -> ChildWidthMode {
    match direction {
        crate::layout::tree::FlowDirection::Vertical => {
            if align_self == Align::Stretch {
                ChildWidthMode::VerticalStretch
            } else {
                ChildWidthMode::VerticalFitContent
            }
        }
        crate::layout::tree::FlowDirection::Horizontal => ChildWidthMode::Default,
    }
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

fn cross_axis_offset(align: Align, available_cross_size: f32, child_outer_cross_size: f32) -> f32 {
    let free = (available_cross_size - child_outer_cross_size).max(0.0);
    match align {
        Align::Start | Align::Stretch => 0.0,
        Align::Center => free * 0.5,
        Align::End => free,
    }
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
                            run.background.is_some()
                                || run.border.is_some()
                                || match &run.payload {
                                    super::types::LayoutAtomPayload::Chip { glyphs } => {
                                        !glyphs.is_empty()
                                    }
                                    super::types::LayoutAtomPayload::Icon { .. }
                                    | super::types::LayoutAtomPayload::Image { .. } => true,
                                    super::types::LayoutAtomPayload::Custom { paint } => {
                                        !paint.is_empty()
                                    }
                                }
                        }
                    })
                })
        }
        LayoutBlockContent::Embed(embed) => {
            block.background.is_some()
                || match &embed.kind {
                    LayoutEmbedKind::Custom { paint } => !paint.is_empty(),
                    LayoutEmbedKind::Image { .. } | LayoutEmbedKind::Path { .. } => true,
                }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::draw_list::ImageData;
    use crate::layout::prepare_tree::prepare_tree;
    use crate::layout::tree::{
        Align, AnchorKey, BlockEmbedKind, BlockEmbedNode, BlockNode, BlockStyle, ClipMode,
        DocumentTree, Edges, FlowDirection, InlineNode, OverlayAnchor, OverlayNode, ParagraphNode,
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(200.0, [200.0, 100.0]));
        assert_eq!(laid_out.overlays.len(), 1);
        assert!(laid_out.overlays[0].rect.x() >= laid_out.root.rect.x() + 8.0);
    }

    #[test]
    fn vertical_stack_stretch_embed_fills_available_width() {
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(200.0, [200.0, 100.0]));
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        assert_eq!(children.len(), 1);
        assert!((children[0].rect.width() - 200.0).abs() < 0.01);
    }

    #[test]
    fn vertical_stack_non_stretch_embed_keeps_intrinsic_width() {
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Embed(
                    BlockEmbedNode::new(
                        BlockEmbedKind::Image {
                            data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                            intrinsic_size: [24.0, 12.0],
                        },
                        BlockStyle {
                            align_self: Align::Start,
                            ..BlockStyle::default()
                        },
                    )
                    .expect("embed must be valid"),
                )],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(200.0, [200.0, 100.0]));
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(200.0, [200.0, 100.0]));
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(200.0, [200.0, 100.0]));
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(200.0, [200.0, 100.0]));
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(200.0, [200.0, 100.0]));
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(80.0, [80.0, 120.0]));
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        let LayoutBlockContent::Paragraph(paragraph) = &children[0].content else {
            panic!("child must be paragraph");
        };
        assert!(paragraph.lines.len() > 1);
        assert!(paragraph.rect.height() > 0.0);
    }

    #[test]
    fn vertical_stack_non_stretch_paragraphs_gain_real_cross_axis_offsets() {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let make_paragraph = |align_self| {
            BlockNode::Paragraph(
                ParagraphNode::new(
                    vec![InlineNode::Text(TextRun::new("short", style))],
                    ParagraphStyle {
                        block: BlockStyle {
                            align_self,
                            ..BlockStyle::default()
                        },
                        ..ParagraphStyle::default()
                    },
                )
                .expect("paragraph must be valid"),
            )
        };
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![
                    make_paragraph(Align::Start),
                    make_paragraph(Align::Center),
                    make_paragraph(Align::End),
                ],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(120.0, [120.0, 120.0]));
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        assert_eq!(children.len(), 3);
        assert!(children
            .iter()
            .all(|child| child.rect.width() < laid_out.root.rect.width()));
        assert!((children[0].rect.x() - 0.0).abs() < 0.01);
        assert!(children[1].rect.x() > children[0].rect.x());
        assert!(children[2].rect.x() > children[1].rect.x());
    }

    #[test]
    fn vertical_stack_non_stretch_paragraph_min_width_is_clamped_by_parent_width() {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Paragraph(
                    ParagraphNode::new(
                        vec![InlineNode::Text(TextRun::new("short", style))],
                        ParagraphStyle {
                            block: BlockStyle {
                                align_self: Align::End,
                                min_width: Some(120.0),
                                ..BlockStyle::default()
                            },
                            ..ParagraphStyle::default()
                        },
                    )
                    .expect("paragraph must be valid"),
                )],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(80.0, [80.0, 120.0]));
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        assert_eq!(children.len(), 1);
        assert!((children[0].rect.width() - 80.0).abs() < 0.01);
        assert!((children[0].rect.x() - 0.0).abs() < 0.01);
    }

    #[test]
    fn horizontal_stack_end_alignment_moves_child_to_bottom_edge() {
        let image = Arc::new(ImageData::new(vec![255; 4], 1, 1));
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Horizontal,
                vec![
                    BlockNode::Embed(
                        BlockEmbedNode::new(
                            BlockEmbedKind::Image {
                                data_ref: image.clone(),
                                intrinsic_size: [10.0, 40.0],
                            },
                            BlockStyle::default(),
                        )
                        .expect("embed must be valid"),
                    ),
                    BlockNode::Embed(
                        BlockEmbedNode::new(
                            BlockEmbedKind::Image {
                                data_ref: image,
                                intrinsic_size: [10.0, 10.0],
                            },
                            BlockStyle {
                                align_self: Align::End,
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
        let laid_out = layout_tree(&prepared, LayoutConstraints::new(120.0, [120.0, 120.0]));
        let LayoutBlockContent::Stack { children } = &laid_out.root.content else {
            panic!("root must be stack");
        };
        assert_eq!(children.len(), 2);
        assert!((children[0].rect.y() - 0.0).abs() < 0.01);
        assert!((children[1].rect.y() - 30.0).abs() < 0.01);
    }
}
