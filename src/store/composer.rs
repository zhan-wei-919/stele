//! Store-side full-scene composition from layout output into arena-backed scene buffers.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bumpalo::Bump;
use bytemuck::cast_slice;
use log::{info, warn};

use crate::draw_list::{ClipRect, ImageCmd, PathCmd, PathVerb, RectCmd, RenderLayer, StrokeStyle};
use crate::font::FreeTypeRasterizer;
use crate::layout::layout_tree::{
    self, LayoutAtomPayload, LayoutBlock as TreeLayoutBlock,
    LayoutBlockContent as TreeLayoutBlockContent, LayoutConstraints, LayoutEmbedKind,
    LayoutRect as TreeLayoutRect, LayoutRun as TreeLayoutRun, ScrollAnchor,
};
use crate::layout::tree::{BorderStyle, LocalPaintCommand, PathStroke};
use crate::scene::instance::{GlyphInstance, RectInstance};
use crate::scene::{BlockDataArena, BlockId, SceneBufferInner, SceneFrameMetadata};

use super::logical_atlas::LogicalAtlas;
use super::model::{LayoutCache, Model};
use super::types::ViewportState;

/// Stateless composer from logical layout output to one render-ready scene buffer.
pub(crate) struct Composer;

type OrderedBlock<'a> = (u32, BlockDataArena<'a>);

/// One compose pass plus the document extent measured before viewport clipping.
pub(crate) struct ComposeOutcome<'a> {
    pub(crate) scene: SceneBufferInner<'a>,
    pub(crate) content_extent: [f32; 2],
}

#[derive(Default)]
struct MaterializedPrimitives {
    glyphs: Vec<GlyphInstance>,
    rects: Vec<RectInstance>,
    paths: Vec<PathCmd>,
    images: Vec<ImageCmd>,
}

impl MaterializedPrimitives {
    fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
            && self.rects.is_empty()
            && self.paths.is_empty()
            && self.images.is_empty()
    }
}

impl Composer {
    /// Recomputes the full scene payload into one arena-backed scene buffer.
    pub(crate) fn compose_into_buffer<'a>(
        &self,
        owner: &'a Bump,
        model: &Model,
        layout_cache: &LayoutCache,
        logical_atlas: &mut LogicalAtlas,
        rasterizer: &FreeTypeRasterizer,
        viewport: ViewportState,
        scroll_offset: [f32; 2],
        clear_tessellation_cache: bool,
        max_blocks_per_scene: usize,
    ) -> ComposeOutcome<'a> {
        let prepared_tree = layout_cache.prepared();
        debug_assert_eq!(
            model.document().anchor_index().len(),
            prepared_tree.anchor_index.len(),
            "model and prepared tree must stay in sync",
        );
        let (mut entries, content_extent) = compose_tree_entries(
            owner,
            prepared_tree,
            logical_atlas,
            rasterizer,
            viewport,
            scroll_offset,
        );

        sort_ordered_entries(&mut entries);
        if entries.len() > max_blocks_per_scene {
            warn!(
                "store.scene_block_limit_exceeded blocks={} limit={}",
                entries.len(),
                max_blocks_per_scene
            );
        }
        debug_assert!(
            entries.len() <= max_blocks_per_scene,
            "scene block count exceeded the configured limit"
        );

        let metadata = SceneFrameMetadata {
            viewport_revision: viewport.viewport_revision,
            required_atlas_generation: entries
                .iter()
                .any(|(_, block)| !block.glyphs().is_empty())
                .then_some(logical_atlas.generation),
            clear_tessellation_cache,
            resize_started_at: viewport.resize_started_at,
        };
        let mut scene = SceneBufferInner::empty_in(owner, metadata);
        for (_, block) in entries {
            scene.order_mut().push(block.block_id());
            scene.blocks_mut().push(block);
        }
        ComposeOutcome {
            scene,
            content_extent,
        }
    }
}

fn compose_tree_entries<'a>(
    owner: &'a Bump,
    prepared_tree: &crate::layout::prepare_tree::PreparedTree,
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    viewport: ViewportState,
    scroll_offset: [f32; 2],
) -> (Vec<OrderedBlock<'a>>, [f32; 2]) {
    let layout_tree: layout_tree::LayoutTree = layout_tree::layout_tree(
        prepared_tree,
        LayoutConstraints::new(viewport.logical_size()[0].max(1.0)),
    );
    let mut entries = Vec::new();
    let mut content_extent = [0.0, 0.0];
    let viewport_size = viewport.logical_size();
    let viewport_rect = TreeLayoutRect::new(0.0, 0.0, viewport_size[0], viewport_size[1]);
    // Layout runs in document coordinates; composition is the first point where the store knows
    // the live scroll offset and can translate content into viewport space.
    let scroll_translation = [-scroll_offset[0], -scroll_offset[1]];
    collect_tree_entries(
        owner,
        &layout_tree.root,
        &mut entries,
        &mut content_extent,
        logical_atlas,
        rasterizer,
        viewport.scale_factor,
        viewport_rect,
        scroll_translation,
        true,
    );
    for overlay in &layout_tree.overlays {
        collect_tree_entries(
            owner,
            overlay,
            &mut entries,
            &mut content_extent,
            logical_atlas,
            rasterizer,
            viewport.scale_factor,
            viewport_rect,
            scroll_translation,
            false,
        );
    }
    info!(
        "layout.tree.compose block_count={} overlay_count={}",
        entries.len(),
        layout_tree.overlays.len()
    );
    (entries, content_extent)
}

fn sort_ordered_entries(entries: &mut [OrderedBlock<'_>]) {
    entries.sort_by_key(|(doc_order, block)| (block.z_order(), *doc_order));
}

fn collect_tree_entries<'a>(
    owner: &'a Bump,
    block: &TreeLayoutBlock,
    entries: &mut Vec<OrderedBlock<'a>>,
    content_extent: &mut [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
    viewport_rect: TreeLayoutRect,
    scroll_translation: [f32; 2],
    measure_content_extent: bool,
) {
    if measure_content_extent {
        // Overlays stay out of the document scroll range because their anchors can be viewport-
        // relative even when their source nodes live in the content tree.
        content_extent[0] = content_extent[0].max(block.rect.right());
        content_extent[1] = content_extent[1].max(block.rect.bottom());
    }

    let translation = match block.scroll_anchor {
        ScrollAnchor::FollowsContent => scroll_translation,
        ScrollAnchor::FixedToViewport => [0.0, 0.0],
    };
    let translated_rect = translate_layout_rect(block.rect, translation);
    let translated_clip_rect = resolve_block_clip_rect(block, translation, viewport_rect);

    if let Some(materialized) = compose_tree_block(
        owner,
        block,
        translated_rect,
        translated_clip_rect,
        translation,
        logical_atlas,
        rasterizer,
        scale_factor,
    ) {
        entries.push((block.doc_order, materialized));
    }

    if let TreeLayoutBlockContent::Stack { children } = &block.content {
        for child in children {
            collect_tree_entries(
                owner,
                child,
                entries,
                content_extent,
                logical_atlas,
                rasterizer,
                scale_factor,
                viewport_rect,
                scroll_translation,
                measure_content_extent,
            );
        }
    }
}

fn compose_tree_block<'a>(
    owner: &'a Bump,
    block: &TreeLayoutBlock,
    rect: TreeLayoutRect,
    clip_rect: TreeLayoutRect,
    translation: [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) -> Option<BlockDataArena<'a>> {
    if rect.is_empty() || clip_rect.is_empty() {
        return None;
    }

    let mut batch = MaterializedPrimitives::default();
    push_block_background(&mut batch, block.background, rect, scale_factor);
    materialize_block_content(
        &mut batch,
        &block.content,
        translation,
        logical_atlas,
        rasterizer,
        scale_factor,
    );
    build_block_arena(owner, block, clip_rect, logical_atlas.generation, batch)
}

fn push_block_background(
    batch: &mut MaterializedPrimitives,
    background: Option<[f32; 4]>,
    rect: TreeLayoutRect,
    scale_factor: f32,
) {
    if let Some(background) = background {
        push_rect_instance(&mut batch.rects, rect, background, scale_factor);
    }
}

fn materialize_block_content(
    batch: &mut MaterializedPrimitives,
    content: &TreeLayoutBlockContent,
    translation: [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) {
    match content {
        TreeLayoutBlockContent::Stack { .. } => {}
        TreeLayoutBlockContent::Paragraph(paragraph) => {
            for line in &paragraph.lines {
                materialize_runs(
                    batch,
                    &line.runs,
                    translation,
                    logical_atlas,
                    rasterizer,
                    scale_factor,
                );
            }
        }
        TreeLayoutBlockContent::Embed(embed) => materialize_embed(
            batch,
            embed.rect,
            embed.intrinsic_size,
            &embed.kind,
            translation,
            scale_factor,
        ),
    }
}

fn materialize_runs(
    batch: &mut MaterializedPrimitives,
    runs: &[TreeLayoutRun],
    translation: [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) {
    for run in runs {
        materialize_layout_run(
            batch,
            run,
            translation,
            logical_atlas,
            rasterizer,
            scale_factor,
        );
    }
}

fn materialize_layout_run(
    batch: &mut MaterializedPrimitives,
    run: &TreeLayoutRun,
    translation: [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) {
    match run {
        TreeLayoutRun::Text(run) => {
            materialize_text_run(
                batch,
                &run.glyphs,
                &run.decoration_rects,
                translation,
                logical_atlas,
                rasterizer,
                scale_factor,
            );
        }
        TreeLayoutRun::Atom(run) => {
            materialize_atom_run(
                batch,
                run.rect,
                run.content_rect,
                run.background,
                run.border,
                &run.payload,
                translation,
                logical_atlas,
                rasterizer,
                scale_factor,
            );
        }
    }
}

fn materialize_text_run(
    batch: &mut MaterializedPrimitives,
    glyphs: &[crate::draw_list::PositionedGlyph],
    decoration_rects: &[RectCmd],
    translation: [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) {
    for glyph in glyphs {
        push_glyph_instance(
            &mut batch.glyphs,
            glyph,
            translation,
            logical_atlas,
            rasterizer,
            scale_factor,
        );
    }
    batch.rects.extend(
        decoration_rects
            .iter()
            .copied()
            .map(|rect| translate_rect_cmd(rect, translation))
            .map(|rect| RectInstance::from_rect(rect, scale_factor)),
    );
}

fn materialize_atom_run(
    batch: &mut MaterializedPrimitives,
    rect: TreeLayoutRect,
    content_rect: TreeLayoutRect,
    background: Option<[f32; 4]>,
    border: Option<BorderStyle>,
    payload: &LayoutAtomPayload,
    translation: [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) {
    let translated_rect = translate_layout_rect(rect, translation);
    let translated_content_rect = translate_layout_rect(content_rect, translation);
    push_atom_frame(batch, translated_rect, background, border, scale_factor);
    match payload {
        LayoutAtomPayload::Chip { glyphs } => {
            for glyph in glyphs {
                push_glyph_instance(
                    &mut batch.glyphs,
                    &glyph,
                    translation,
                    logical_atlas,
                    rasterizer,
                    scale_factor,
                );
            }
        }
        LayoutAtomPayload::Icon { glyph } => {
            push_glyph_instance(
                &mut batch.glyphs,
                &glyph,
                translation,
                logical_atlas,
                rasterizer,
                scale_factor,
            );
        }
        LayoutAtomPayload::Image { data_ref } => {
            push_layout_image(
                &mut batch.images,
                translated_content_rect,
                data_ref.clone(),
                RenderLayer::Foreground,
            );
        }
        LayoutAtomPayload::Custom { paint } => append_local_paint_translated(
            &mut batch.rects,
            &mut batch.paths,
            &mut batch.images,
            paint.as_ref(),
            [translated_content_rect.x(), translated_content_rect.y()],
            scale_factor,
        ),
    }
}

fn push_atom_frame(
    batch: &mut MaterializedPrimitives,
    rect: TreeLayoutRect,
    background: Option<[f32; 4]>,
    border: Option<BorderStyle>,
    scale_factor: f32,
) {
    if let Some(color) = background {
        push_rect_instance(&mut batch.rects, rect, color, scale_factor);
    }
    if let Some(border) = border {
        push_border_instances(&mut batch.rects, rect, border, scale_factor);
    }
}

fn materialize_embed(
    batch: &mut MaterializedPrimitives,
    rect: TreeLayoutRect,
    intrinsic_size: [f32; 2],
    kind: &LayoutEmbedKind,
    translation: [f32; 2],
    scale_factor: f32,
) {
    let translated_rect = translate_layout_rect(rect, translation);
    match kind {
        LayoutEmbedKind::Image { data_ref } => {
            push_layout_image(
                &mut batch.images,
                translated_rect,
                data_ref.clone(),
                RenderLayer::Foreground,
            );
        }
        LayoutEmbedKind::Path {
            verbs,
            fill,
            stroke,
        } => materialize_path_embed(
            &mut batch.paths,
            translated_rect,
            intrinsic_size,
            verbs.as_slice(),
            fill.clone(),
            stroke.clone(),
        ),
        LayoutEmbedKind::Custom { paint } => append_local_paint_scaled(
            &mut batch.rects,
            &mut batch.paths,
            &mut batch.images,
            paint.as_ref(),
            translated_rect,
            intrinsic_size,
            scale_factor,
        ),
    }
}

fn materialize_path_embed(
    paths: &mut Vec<PathCmd>,
    rect: TreeLayoutRect,
    intrinsic_size: [f32; 2],
    verbs: &[PathVerb],
    fill: Option<[f32; 4]>,
    stroke: Option<PathStroke>,
) {
    let [scale_x, scale_y] = embed_scale(rect, intrinsic_size);
    paths.push(PathCmd::new(
        transform_path_verbs(verbs, rect, intrinsic_size),
        fill,
        stroke.map(|stroke| scale_stroke(stroke, scale_x, scale_y)),
        RenderLayer::Foreground,
    ));
}

fn embed_scale(rect: TreeLayoutRect, intrinsic_size: [f32; 2]) -> [f32; 2] {
    let scale_x = if intrinsic_size[0] > 0.0 {
        rect.width() / intrinsic_size[0]
    } else {
        1.0
    };
    let scale_y = if intrinsic_size[1] > 0.0 {
        rect.height() / intrinsic_size[1]
    } else {
        1.0
    };
    [scale_x, scale_y]
}

fn push_glyph_instance(
    glyphs: &mut Vec<GlyphInstance>,
    glyph: &crate::draw_list::PositionedGlyph,
    translation: [f32; 2],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) {
    let key = glyph.glyph_key(scale_factor);
    let region = logical_atlas.get_or_insert(key, rasterizer);
    let translated_glyph = crate::draw_list::PositionedGlyph {
        pos: translate_point(glyph.pos, translation),
        ..*glyph
    };
    glyphs.push(GlyphInstance::from_positioned_glyph(
        &translated_glyph,
        region,
        scale_factor,
    ));
}

fn push_layout_image(
    images: &mut Vec<ImageCmd>,
    rect: TreeLayoutRect,
    data_ref: std::sync::Arc<crate::draw_list::ImageData>,
    layer: RenderLayer,
) {
    if rect.is_empty() {
        return;
    }
    images.push(ImageCmd::new(
        [rect.x(), rect.y()],
        [rect.width(), rect.height()],
        data_ref,
        layer,
    ));
}

fn build_block_arena<'a>(
    owner: &'a Bump,
    block: &TreeLayoutBlock,
    clip_rect: TreeLayoutRect,
    logical_atlas_generation: u64,
    batch: MaterializedPrimitives,
) -> Option<BlockDataArena<'a>> {
    if batch.is_empty() {
        return None;
    }

    let clip_rect = tree_clip_rect(clip_rect);
    let fingerprint = fingerprint_batch(
        clip_rect,
        block.z_order,
        &batch.glyphs,
        &batch.rects,
        &batch.paths,
        &batch.images,
        (!batch.glyphs.is_empty()).then_some(logical_atlas_generation),
    );
    let mut arena = BlockDataArena::new_in(
        owner,
        BlockId::new(block.node_id.value()),
        clip_rect,
        block.z_order,
        fingerprint,
    );
    populate_block_arena(&mut arena, batch);
    Some(arena)
}

fn tree_clip_rect(rect: TreeLayoutRect) -> ClipRect {
    ClipRect::new(rect.x(), rect.y(), rect.width(), rect.height())
}

fn resolve_block_clip_rect(
    block: &TreeLayoutBlock,
    translation: [f32; 2],
    viewport_rect: TreeLayoutRect,
) -> TreeLayoutRect {
    match block.scroll_anchor {
        // Content clips are declared in document space, so translate them with the block and then
        // intersect against the live viewport to match what the user can actually see.
        ScrollAnchor::FollowsContent => {
            translated_follows_content_clip_rect(block.clip_rect, translation, viewport_rect)
        }
        // Viewport overlays already resolve in screen space during layout and should not inherit
        // the document scroll translation.
        ScrollAnchor::FixedToViewport => block.clip_rect,
    }
}

fn translated_follows_content_clip_rect(
    clip_rect: TreeLayoutRect,
    translation: [f32; 2],
    viewport_rect: TreeLayoutRect,
) -> TreeLayoutRect {
    translate_layout_rect(clip_rect, translation).intersect(viewport_rect)
}

fn translate_layout_rect(rect: TreeLayoutRect, translation: [f32; 2]) -> TreeLayoutRect {
    TreeLayoutRect::new(
        rect.x() + translation[0],
        rect.y() + translation[1],
        rect.width(),
        rect.height(),
    )
}

fn translate_rect_cmd(rect: RectCmd, translation: [f32; 2]) -> RectCmd {
    let pos = translate_point(rect.pos(), translation);
    RectCmd::new(pos, rect.size(), rect.color(), rect.layer())
}

fn translate_point(point: [f32; 2], translation: [f32; 2]) -> [f32; 2] {
    [point[0] + translation[0], point[1] + translation[1]]
}

fn populate_block_arena(arena: &mut BlockDataArena<'_>, batch: MaterializedPrimitives) {
    let MaterializedPrimitives {
        glyphs,
        rects,
        paths,
        images,
    } = batch;
    arena.glyphs_mut().extend(glyphs);
    arena.rects_mut().extend(rects);
    arena.paths_mut().extend(paths);
    arena.images_mut().extend(images);
}

fn push_rect_instance(
    rects: &mut Vec<RectInstance>,
    rect: TreeLayoutRect,
    color: [f32; 4],
    scale_factor: f32,
) {
    push_rect_instance_with_layer(rects, rect, color, RenderLayer::Background, scale_factor);
}

fn push_rect_instance_with_layer(
    rects: &mut Vec<RectInstance>,
    rect: TreeLayoutRect,
    color: [f32; 4],
    layer: RenderLayer,
    scale_factor: f32,
) {
    if rect.is_empty() {
        return;
    }
    rects.push(RectInstance::from_rect(
        RectCmd::new(
            [rect.x(), rect.y()],
            [rect.width(), rect.height()],
            color,
            layer,
        ),
        scale_factor,
    ));
}

fn push_border_instances(
    rects: &mut Vec<RectInstance>,
    rect: TreeLayoutRect,
    border: BorderStyle,
    scale_factor: f32,
) {
    let width = border
        .width
        .min(rect.width() * 0.5)
        .min(rect.height() * 0.5);
    if width <= 0.0 {
        return;
    }

    push_rect_instance_with_layer(
        rects,
        TreeLayoutRect::new(rect.x(), rect.y(), rect.width(), width),
        border.color,
        RenderLayer::Content,
        scale_factor,
    );
    push_rect_instance_with_layer(
        rects,
        TreeLayoutRect::new(
            rect.x(),
            rect.y() + rect.height() - width,
            rect.width(),
            width,
        ),
        border.color,
        RenderLayer::Content,
        scale_factor,
    );

    let inner_height = (rect.height() - width * 2.0).max(0.0);
    if inner_height > 0.0 {
        push_rect_instance_with_layer(
            rects,
            TreeLayoutRect::new(rect.x(), rect.y() + width, width, inner_height),
            border.color,
            RenderLayer::Content,
            scale_factor,
        );
        push_rect_instance_with_layer(
            rects,
            TreeLayoutRect::new(
                rect.x() + rect.width() - width,
                rect.y() + width,
                width,
                inner_height,
            ),
            border.color,
            RenderLayer::Content,
            scale_factor,
        );
    }
}

fn transform_path_verbs(
    verbs: &[PathVerb],
    rect: TreeLayoutRect,
    intrinsic_size: [f32; 2],
) -> Vec<PathVerb> {
    let scale_x = if intrinsic_size[0] > 0.0 {
        rect.width() / intrinsic_size[0]
    } else {
        1.0
    };
    let scale_y = if intrinsic_size[1] > 0.0 {
        rect.height() / intrinsic_size[1]
    } else {
        1.0
    };
    verbs
        .iter()
        .map(|verb| transform_path_verb(verb, rect.x(), rect.y(), scale_x, scale_y))
        .collect()
}

fn append_local_paint_translated(
    rects: &mut Vec<RectInstance>,
    paths: &mut Vec<PathCmd>,
    images: &mut Vec<ImageCmd>,
    paint: &[LocalPaintCommand],
    origin: [f32; 2],
    scale_factor: f32,
) {
    append_local_paint_with_transform(
        rects,
        paths,
        images,
        paint,
        origin,
        [1.0, 1.0],
        scale_factor,
    );
}

fn append_local_paint_scaled(
    rects: &mut Vec<RectInstance>,
    paths: &mut Vec<PathCmd>,
    images: &mut Vec<ImageCmd>,
    paint: &[LocalPaintCommand],
    rect: TreeLayoutRect,
    intrinsic_size: [f32; 2],
    scale_factor: f32,
) {
    let scale_x = if intrinsic_size[0] > 0.0 {
        rect.width() / intrinsic_size[0]
    } else {
        1.0
    };
    let scale_y = if intrinsic_size[1] > 0.0 {
        rect.height() / intrinsic_size[1]
    } else {
        1.0
    };
    append_local_paint_with_transform(
        rects,
        paths,
        images,
        paint,
        [rect.x(), rect.y()],
        [scale_x, scale_y],
        scale_factor,
    );
}

fn append_local_paint_with_transform(
    rects: &mut Vec<RectInstance>,
    paths: &mut Vec<PathCmd>,
    images: &mut Vec<ImageCmd>,
    paint: &[LocalPaintCommand],
    origin: [f32; 2],
    scale: [f32; 2],
    scale_factor: f32,
) {
    for command in paint {
        match command {
            LocalPaintCommand::Rect { pos, size, color } => {
                push_rect_instance_with_layer(
                    rects,
                    TreeLayoutRect::new(
                        origin[0] + pos[0] * scale[0],
                        origin[1] + pos[1] * scale[1],
                        size[0] * scale[0],
                        size[1] * scale[1],
                    ),
                    *color,
                    RenderLayer::Content,
                    scale_factor,
                );
            }
            LocalPaintCommand::Path {
                verbs,
                fill,
                stroke,
            } => {
                paths.push(PathCmd::new(
                    verbs
                        .iter()
                        .map(|verb| {
                            transform_path_verb(verb, origin[0], origin[1], scale[0], scale[1])
                        })
                        .collect(),
                    *fill,
                    stroke.map(|stroke| scale_stroke(stroke, scale[0], scale[1])),
                    RenderLayer::Content,
                ));
            }
            LocalPaintCommand::Image {
                pos,
                size,
                data_ref,
            } => {
                let transformed_size = [size[0] * scale[0], size[1] * scale[1]];
                if transformed_size[0] > 0.0 && transformed_size[1] > 0.0 {
                    images.push(ImageCmd::new(
                        [origin[0] + pos[0] * scale[0], origin[1] + pos[1] * scale[1]],
                        transformed_size,
                        data_ref.clone(),
                        RenderLayer::Content,
                    ));
                }
            }
        }
    }
}

fn scale_stroke(stroke: PathStroke, scale_x: f32, scale_y: f32) -> StrokeStyle {
    let width_scale = ((scale_x + scale_y) * 0.5).max(0.0);
    StrokeStyle::new(
        stroke.color,
        (stroke.width * width_scale).max(1.0),
        stroke.line_cap,
        stroke.line_join,
    )
}

fn transform_path_verb(verb: &PathVerb, x: f32, y: f32, scale_x: f32, scale_y: f32) -> PathVerb {
    match verb {
        PathVerb::MoveTo { to } => PathVerb::MoveTo {
            to: [x + to[0] * scale_x, y + to[1] * scale_y],
        },
        PathVerb::LineTo { to } => PathVerb::LineTo {
            to: [x + to[0] * scale_x, y + to[1] * scale_y],
        },
        PathVerb::QuadTo { ctrl, to } => PathVerb::QuadTo {
            ctrl: [x + ctrl[0] * scale_x, y + ctrl[1] * scale_y],
            to: [x + to[0] * scale_x, y + to[1] * scale_y],
        },
        PathVerb::CubicTo { ctrl1, ctrl2, to } => PathVerb::CubicTo {
            ctrl1: [x + ctrl1[0] * scale_x, y + ctrl1[1] * scale_y],
            ctrl2: [x + ctrl2[0] * scale_x, y + ctrl2[1] * scale_y],
            to: [x + to[0] * scale_x, y + to[1] * scale_y],
        },
        PathVerb::Close => PathVerb::Close,
    }
}

fn fingerprint_batch(
    clip_rect: ClipRect,
    z_order: u32,
    glyphs: &[GlyphInstance],
    rects: &[RectInstance],
    paths: &[PathCmd],
    images: &[ImageCmd],
    atlas_generation: Option<u64>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_clip_rect(&mut hasher, clip_rect);
    z_order.hash(&mut hasher);
    hash_pod_slice(&mut hasher, glyphs);
    hash_pod_slice(&mut hasher, rects);
    hash_paths(&mut hasher, paths);
    hash_images(&mut hasher, images);
    atlas_generation.hash(&mut hasher);
    hasher.finish()
}

fn hash_clip_rect(hasher: &mut impl Hasher, clip_rect: ClipRect) {
    let [origin_x, origin_y] = clip_rect.origin();
    let [width, height] = clip_rect.size();
    origin_x.to_bits().hash(hasher);
    origin_y.to_bits().hash(hasher);
    width.to_bits().hash(hasher);
    height.to_bits().hash(hasher);
}

fn hash_pod_slice<T: bytemuck::Pod>(hasher: &mut impl Hasher, values: &[T]) {
    values.len().hash(hasher);
    hasher.write(cast_slice(values));
}

fn hash_paths(hasher: &mut impl Hasher, paths: &[PathCmd]) {
    paths.len().hash(hasher);
    for path in paths {
        path.content_hash().hash(hasher);
        path.layer().hash(hasher);
    }
}

fn hash_images(hasher: &mut impl Hasher, images: &[ImageCmd]) {
    images.len().hash(hasher);
    for image in images {
        let [x, y] = image.pos();
        let [width, height] = image.size();
        x.to_bits().hash(hasher);
        y.to_bits().hash(hasher);
        width.to_bits().hash(hasher);
        height.to_bits().hash(hasher);
        image.data().content_hash().hash(hasher);
        image.layer().hash(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bumpalo::Bump;

    use super::{sort_ordered_entries, Composer};
    use crate::draw_list::{
        ClipRect, ImageCmd, ImageData, LineCap, LineJoin, PathCmd, PathVerb, RenderLayer,
        StrokeStyle,
    };
    use crate::font::{FontDiscovery, FreeTypeRasterizer};
    use crate::layout::prepare_tree::prepare_tree;
    use crate::layout::tree::{
        Align, AnchorKey, BlockEmbedKind, BlockEmbedNode, BlockNode, BlockStyle, BorderStyle,
        ClipMode, DocumentTree, Edges, FlowDirection, InlineAtom, InlineAtomKind, InlineAtomStyle,
        InlineNode, LocalPaintCommand, OverlayAnchor, OverlayNode, ParagraphNode, ParagraphStyle,
        StackNode, TextRun, TextStyle,
    };
    use crate::renderer::subpixel::detect_subpixel_layout;
    use crate::scene::{BlockDataArena, BlockId};
    use crate::store::logical_atlas::LogicalAtlas;
    use crate::store::{Model, ViewportState};

    use super::super::model::LayoutCache;

    #[test]
    fn stable_z_order_sort_preserves_upstream_document_order_within_a_layer() {
        let owner = Bump::new();
        let mut entries = vec![
            (1, sample_block(&owner, BlockId::new(30), 1)),
            (0, sample_block(&owner, BlockId::new(99), 0)),
            (0, sample_block(&owner, BlockId::new(10), 0)),
            (1, sample_block(&owner, BlockId::new(40), 1)),
        ];

        sort_ordered_entries(&mut entries);

        let order = entries
            .into_iter()
            .map(|(_, block)| block.block_id())
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            vec![
                BlockId::new(99),
                BlockId::new(10),
                BlockId::new(30),
                BlockId::new(40),
            ]
        );
    }

    #[test]
    fn fingerprint_changes_when_image_content_changes() {
        let before = super::fingerprint_batch(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            &[],
            &[],
            &[],
            &[sample_image(
                [10.0, 12.0],
                [16.0, 18.0],
                vec![255, 0, 0, 255],
                RenderLayer::Foreground,
            )],
            None,
        );
        let after = super::fingerprint_batch(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            &[],
            &[],
            &[],
            &[sample_image(
                [10.0, 12.0],
                [16.0, 18.0],
                vec![0, 255, 0, 255],
                RenderLayer::Foreground,
            )],
            None,
        );

        assert_ne!(before, after);
    }

    #[test]
    fn fingerprint_changes_when_image_geometry_or_layer_changes() {
        let baseline = super::fingerprint_batch(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            &[],
            &[],
            &[],
            &[sample_image(
                [10.0, 12.0],
                [16.0, 18.0],
                vec![255, 0, 0, 255],
                RenderLayer::Foreground,
            )],
            None,
        );
        let moved = super::fingerprint_batch(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            &[],
            &[],
            &[],
            &[sample_image(
                [11.0, 12.0],
                [16.0, 18.0],
                vec![255, 0, 0, 255],
                RenderLayer::Foreground,
            )],
            None,
        );
        let relayered = super::fingerprint_batch(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            &[],
            &[],
            &[],
            &[sample_image(
                [10.0, 12.0],
                [16.0, 18.0],
                vec![255, 0, 0, 255],
                RenderLayer::Overlay,
            )],
            None,
        );

        assert_ne!(baseline, moved);
        assert_ne!(baseline, relayered);
    }

    #[test]
    fn compose_into_buffer_attaches_paths_to_block_batch() {
        let owner = Bump::new();
        let path = sample_path(18.0);
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Embed(
                    BlockEmbedNode::new(
                        BlockEmbedKind::Path {
                            verbs: vec![
                                PathVerb::MoveTo { to: [18.0, 10.0] },
                                PathVerb::LineTo { to: [42.0, 10.0] },
                                PathVerb::LineTo { to: [30.0, 32.0] },
                                PathVerb::Close,
                            ],
                            fill: Some([0.9, 0.6, 0.2, 1.0]),
                            stroke: Some(crate::layout::tree::PathStroke {
                                color: [1.0, 0.95, 0.8, 1.0],
                                width: 2.0,
                                line_cap: LineCap::Round,
                                line_join: LineJoin::Round,
                            }),
                            intrinsic_size: [64.0, 64.0],
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
        let rasterizer = build_rasterizer_for_test();
        let prepared_tree = prepare_tree(&tree, &rasterizer);
        let model = Model::new(tree);
        let layout_cache = LayoutCache::new(prepared_tree);
        let rasterizer = build_rasterizer_for_test();
        let mut logical_atlas = LogicalAtlas::new(1.0);
        let scene = Composer
            .compose_into_buffer(
                &owner,
                &model,
                &layout_cache,
                &mut logical_atlas,
                &rasterizer,
                ViewportState::new(120, 90, 1.0, 7, None),
                [0.0, 0.0],
                false,
                512,
            )
            .scene;

        let batch = scene
            .blocks()
            .iter()
            .find(|batch| !batch.paths().is_empty())
            .expect("scene must contain a path batch");
        assert_eq!(batch.paths().len(), 1);
        assert_eq!(batch.paths()[0].content_hash(), path.content_hash());
        assert_eq!(batch.paths()[0].layer(), RenderLayer::Foreground);
    }

    #[test]
    fn fingerprint_changes_when_path_content_changes() {
        let before = super::fingerprint_batch(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            &[],
            &[],
            &[sample_path(12.0)],
            &[],
            None,
        );
        let after = super::fingerprint_batch(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            &[],
            &[],
            &[sample_path(24.0)],
            &[],
            None,
        );

        assert_ne!(before, after);
    }

    #[test]
    fn background_only_custom_atom_materializes_a_paragraph_batch() {
        let owner = Bump::new();
        let atom = InlineAtom::new(
            InlineAtomKind::Custom {
                measured_size: [8.0, 8.0],
                paint: Arc::<[LocalPaintCommand]>::from(Vec::<LocalPaintCommand>::new()),
            },
            InlineAtomStyle {
                background: Some([0.2, 0.4, 0.8, 1.0]),
                ..InlineAtomStyle::default()
            },
        )
        .expect("atom must be valid");
        let tree = tree_with_paragraph_atom(atom);

        let scene = compose_scene(&owner, tree);
        assert_eq!(scene.blocks().len(), 1);
        let batch = &scene.blocks()[0];
        assert_eq!(batch.rects().len(), 1);
        assert!(batch.paths().is_empty());
        assert!(batch.images().is_empty());
        assert!(batch.glyphs().is_empty());
    }

    #[test]
    fn image_atom_uses_content_rect_for_payload_and_emits_background_and_border() {
        let owner = Bump::new();
        let atom = InlineAtom::new(
            InlineAtomKind::Image {
                data_ref: Arc::new(ImageData::new(vec![255; 10 * 6 * 4], 10, 6)),
            },
            InlineAtomStyle {
                padding: Edges::all(2.0).expect("padding must be valid"),
                background: Some([0.1, 0.2, 0.3, 1.0]),
                border: Some(
                    BorderStyle::new([0.9, 0.8, 0.2, 1.0], 3.0).expect("border must be valid"),
                ),
                ..InlineAtomStyle::default()
            },
        )
        .expect("atom must be valid");
        let tree = tree_with_paragraph_atom(atom);

        let scene = compose_scene(&owner, tree);
        let batch = scene
            .blocks()
            .iter()
            .find(|batch| !batch.images().is_empty())
            .expect("scene must contain the atom image");
        assert_eq!(batch.rects().len(), 5);
        assert_eq!(batch.images().len(), 1);
        assert_close(batch.images()[0].pos()[0], 5.0);
        assert_close(batch.images()[0].pos()[1], 5.0);
        assert_close(batch.images()[0].size()[0], 10.0);
        assert_close(batch.images()[0].size()[1], 6.0);
    }

    #[test]
    fn custom_atom_lowers_local_rect_path_and_image_commands() {
        let owner = Bump::new();
        let atom = InlineAtom::new(
            InlineAtomKind::Custom {
                measured_size: [12.0, 12.0],
                paint: Arc::from(vec![
                    LocalPaintCommand::Rect {
                        pos: [1.0, 2.0],
                        size: [3.0, 4.0],
                        color: [0.9, 0.4, 0.2, 1.0],
                    },
                    LocalPaintCommand::Path {
                        verbs: vec![
                            PathVerb::MoveTo { to: [0.0, 0.0] },
                            PathVerb::LineTo { to: [6.0, 0.0] },
                            PathVerb::LineTo { to: [3.0, 6.0] },
                            PathVerb::Close,
                        ],
                        fill: Some([0.2, 0.8, 0.4, 1.0]),
                        stroke: None,
                    },
                    LocalPaintCommand::Image {
                        pos: [4.0, 5.0],
                        size: [2.0, 3.0],
                        data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                    },
                ]),
            },
            InlineAtomStyle::default(),
        )
        .expect("atom must be valid");
        let tree = tree_with_paragraph_atom(atom);

        let scene = compose_scene(&owner, tree);
        assert_eq!(scene.blocks().len(), 1);
        let batch = &scene.blocks()[0];
        assert_eq!(batch.rects().len(), 1);
        assert_eq!(batch.paths().len(), 1);
        assert_eq!(batch.images().len(), 1);
    }

    #[test]
    fn custom_embed_scales_local_rect_commands_into_embed_rect() {
        let owner = Bump::new();
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Embed(
                    BlockEmbedNode::new(
                        BlockEmbedKind::Custom {
                            intrinsic_size: [10.0, 10.0],
                            paint: Arc::from(vec![LocalPaintCommand::Rect {
                                pos: [1.0, 2.0],
                                size: [3.0, 4.0],
                                color: [0.7, 0.3, 0.9, 1.0],
                            }]),
                        },
                        BlockStyle {
                            align_self: Align::Start,
                            min_width: Some(20.0),
                            min_height: Some(30.0),
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

        let scene = compose_scene(&owner, tree);
        assert_eq!(scene.blocks().len(), 1);
        let batch = &scene.blocks()[0];
        assert_eq!(batch.rects().len(), 1);
        assert_close(batch.rects()[0].pos[0], 2.0);
        assert_close(batch.rects()[0].pos[1], 6.0);
        assert_close(batch.rects()[0].size[0], 6.0);
        assert_close(batch.rects()[0].size[1], 12.0);
    }

    #[test]
    fn scroll_translation_moves_content_overlay_but_not_viewport_overlay() {
        let baseline_owner = Bump::new();
        let scrolled_owner = Bump::new();

        let baseline =
            compose_scene_with_scroll(&baseline_owner, build_overlay_test_tree(), [0.0, 0.0]);
        let (content_overlay_id, viewport_overlay_id) =
            overlay_test_block_ids(&build_overlay_test_tree());
        let scrolled =
            compose_scene_with_scroll(&scrolled_owner, build_overlay_test_tree(), [0.0, 20.0]);

        let baseline_content = find_block(&baseline, content_overlay_id);
        let scrolled_content = find_block(&scrolled, content_overlay_id);
        let baseline_viewport = find_block(&baseline, viewport_overlay_id);
        let scrolled_viewport = find_block(&scrolled, viewport_overlay_id);

        assert_close(
            scrolled_content.clip_rect().origin()[1],
            baseline_content.clip_rect().origin()[1] - 20.0,
        );
        assert_close(
            scrolled_content.glyphs()[0].screen_pos[1],
            baseline_content.glyphs()[0].screen_pos[1] - 20.0,
        );
        assert_close(
            scrolled_viewport.clip_rect().origin()[1],
            baseline_viewport.clip_rect().origin()[1],
        );
        assert_close(
            scrolled_viewport.glyphs()[0].screen_pos[1],
            baseline_viewport.glyphs()[0].screen_pos[1],
        );
    }

    #[test]
    fn viewport_inherited_clip_does_not_shrink_when_content_scrolls() {
        let baseline_owner = Bump::new();
        let scrolled_owner = Bump::new();

        let baseline = compose_scene_with_scroll(
            &baseline_owner,
            tree_with_text_paragraph("scroll clip regression"),
            [0.0, 0.0],
        );
        let scrolled = compose_scene_with_scroll(
            &scrolled_owner,
            tree_with_text_paragraph("scroll clip regression"),
            [0.0, 20.0],
        );

        let baseline_batch = &baseline.blocks()[0];
        let scrolled_batch = &scrolled.blocks()[0];

        assert_close(
            scrolled_batch.clip_rect().origin()[0],
            baseline_batch.clip_rect().origin()[0],
        );
        assert_close(
            scrolled_batch.clip_rect().origin()[1],
            baseline_batch.clip_rect().origin()[1],
        );
        assert_close(
            scrolled_batch.clip_rect().size()[0],
            baseline_batch.clip_rect().size()[0],
        );
        assert_close(
            scrolled_batch.clip_rect().size()[1],
            baseline_batch.clip_rect().size()[1],
        );
        assert_close(
            scrolled_batch.glyphs()[0].screen_pos[1],
            baseline_batch.glyphs()[0].screen_pos[1] - 20.0,
        );
    }

    #[test]
    fn clipped_container_below_initial_viewport_enters_scene_after_scroll() {
        let baseline_owner = Bump::new();
        let scrolled_owner = Bump::new();

        let baseline = compose_scene_with_scroll(
            &baseline_owner,
            build_clipped_scroll_regression_tree(),
            [0.0, 0.0],
        );
        let scrolled_tree = build_clipped_scroll_regression_tree();
        let target_block_id = clipped_scroll_target_block_id(&scrolled_tree);
        let scrolled = compose_scene_with_scroll(&scrolled_owner, scrolled_tree, [0.0, 100.0]);

        assert!(
            !scene_contains_block(&baseline, target_block_id),
            "target block should start outside the initial viewport"
        );
        assert!(
            scene_contains_block(&scrolled, target_block_id),
            "scroll should bring the clipped container's target block into the scene"
        );

        let scrolled_target = find_block(&scrolled, target_block_id);
        assert!(
            !scrolled_target.glyphs().is_empty(),
            "scrolled target block should materialize glyphs once it enters the viewport"
        );
    }

    fn sample_block<'a>(owner: &'a Bump, block_id: BlockId, z_order: u32) -> BlockDataArena<'a> {
        BlockDataArena::new_in(
            owner,
            block_id,
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            z_order,
            z_order as u64,
        )
    }

    fn sample_image(pos: [f32; 2], size: [f32; 2], rgba: Vec<u8>, layer: RenderLayer) -> ImageCmd {
        ImageCmd::new(pos, size, Arc::new(ImageData::new(rgba, 1, 1)), layer)
    }

    fn sample_path(offset: f32) -> PathCmd {
        PathCmd::new(
            vec![
                PathVerb::MoveTo { to: [offset, 10.0] },
                PathVerb::LineTo {
                    to: [offset + 24.0, 10.0],
                },
                PathVerb::LineTo {
                    to: [offset + 12.0, 32.0],
                },
                PathVerb::Close,
            ],
            Some([0.9, 0.6, 0.2, 1.0]),
            Some(StrokeStyle::new(
                [1.0, 0.95, 0.8, 1.0],
                2.0,
                LineCap::Round,
                LineJoin::Round,
            )),
            RenderLayer::Overlay,
        )
    }

    fn build_rasterizer_for_test() -> FreeTypeRasterizer {
        let font_discovery = FontDiscovery::new().expect("failed to discover system fonts");
        FreeTypeRasterizer::new(font_discovery, detect_subpixel_layout())
            .expect("failed to initialize FreeType rasterizer")
    }

    fn tree_with_paragraph_atom(atom: InlineAtom) -> DocumentTree {
        let text_style =
            TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Paragraph(
                    ParagraphNode::new(
                        vec![
                            InlineNode::Text(TextRun::new("", text_style)),
                            InlineNode::Atom(atom),
                        ],
                        ParagraphStyle::default(),
                    )
                    .expect("paragraph must be valid"),
                )],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid")
    }

    fn tree_with_text_paragraph(text: &str) -> DocumentTree {
        let text_style =
            TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Paragraph(
                    ParagraphNode::new(
                        vec![InlineNode::Text(TextRun::new(text, text_style))],
                        ParagraphStyle::default(),
                    )
                    .expect("paragraph must be valid"),
                )],
                BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid")
    }

    fn compose_scene<'a>(
        owner: &'a Bump,
        tree: DocumentTree,
    ) -> crate::scene::SceneBufferInner<'a> {
        compose_scene_with_scroll(owner, tree, [0.0, 0.0])
    }

    fn compose_scene_with_scroll<'a>(
        owner: &'a Bump,
        tree: DocumentTree,
        scroll_offset: [f32; 2],
    ) -> crate::scene::SceneBufferInner<'a> {
        let rasterizer = build_rasterizer_for_test();
        let prepared_tree = prepare_tree(&tree, &rasterizer);
        let model = Model::new(tree);
        let layout_cache = LayoutCache::new(prepared_tree);
        let mut logical_atlas = LogicalAtlas::new(1.0);
        Composer
            .compose_into_buffer(
                owner,
                &model,
                &layout_cache,
                &mut logical_atlas,
                &rasterizer,
                ViewportState::new(120, 90, 1.0, 7, None),
                scroll_offset,
                false,
                512,
            )
            .scene
    }

    fn build_overlay_test_tree() -> DocumentTree {
        let anchor_key = AnchorKey::new("overlay-anchor").expect("anchor must be valid");
        let text_style =
            TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let mut anchor_paragraph = ParagraphNode::new(
            vec![InlineNode::Text(TextRun::new(
                "anchor paragraph",
                text_style,
            ))],
            ParagraphStyle {
                block: BlockStyle {
                    padding: Edges::all(8.0).expect("padding must be valid"),
                    background: Some([0.2, 0.24, 0.32, 1.0]),
                    ..BlockStyle::default()
                },
                ..ParagraphStyle::default()
            },
        )
        .expect("paragraph must be valid");
        anchor_paragraph.anchor_key = Some(anchor_key.clone());

        let content_overlay = BlockNode::Overlay(OverlayNode::new(
            OverlayAnchor::BlockRelative {
                target: anchor_key,
                offset: [0.0, 24.0],
            },
            BlockNode::Paragraph(
                ParagraphNode::new(
                    vec![InlineNode::Text(TextRun::new(
                        "content overlay",
                        text_style,
                    ))],
                    ParagraphStyle {
                        block: BlockStyle {
                            padding: Edges::all(6.0).expect("padding must be valid"),
                            clip: ClipMode::Rect,
                            background: Some([0.7, 0.35, 0.25, 1.0]),
                            ..BlockStyle::default()
                        },
                        ..ParagraphStyle::default()
                    },
                )
                .expect("overlay paragraph must be valid"),
            ),
        ));

        let viewport_overlay = BlockNode::Overlay(OverlayNode::new(
            OverlayAnchor::Viewport {
                offset: [12.0, 10.0],
            },
            BlockNode::Paragraph(
                ParagraphNode::new(
                    vec![InlineNode::Text(TextRun::new(
                        "viewport overlay",
                        text_style,
                    ))],
                    ParagraphStyle {
                        block: BlockStyle {
                            padding: Edges::all(6.0).expect("padding must be valid"),
                            clip: ClipMode::Rect,
                            background: Some([0.2, 0.55, 0.35, 1.0]),
                            ..BlockStyle::default()
                        },
                        ..ParagraphStyle::default()
                    },
                )
                .expect("viewport paragraph must be valid"),
            ),
        ));

        DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![
                    BlockNode::Paragraph(anchor_paragraph),
                    BlockNode::Paragraph(
                        ParagraphNode::new(
                            vec![InlineNode::Text(TextRun::new(
                                "second paragraph to make scrolling visible",
                                text_style,
                            ))],
                            ParagraphStyle {
                                block: BlockStyle {
                                    padding: Edges::all(8.0).expect("padding must be valid"),
                                    margin: Edges::new(0.0, 0.0, 0.0, 60.0)
                                        .expect("margin must be valid"),
                                    background: Some([0.15, 0.18, 0.24, 1.0]),
                                    ..BlockStyle::default()
                                },
                                ..ParagraphStyle::default()
                            },
                        )
                        .expect("body paragraph must be valid"),
                    ),
                    content_overlay,
                    viewport_overlay,
                ],
                BlockStyle {
                    clip: ClipMode::Rect,
                    ..BlockStyle::default()
                },
            )
            .expect("stack must be valid"),
        ))
        .expect("overlay test tree must be valid")
    }

    fn build_clipped_scroll_regression_tree() -> DocumentTree {
        let text_style =
            TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let spacer = BlockNode::Paragraph(
            ParagraphNode::new(
                vec![InlineNode::Text(TextRun::new("spacer", text_style))],
                ParagraphStyle {
                    block: BlockStyle {
                        min_height: Some(120.0),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            )
            .expect("spacer paragraph must be valid"),
        );
        let target = BlockNode::Paragraph(
            ParagraphNode::new(
                vec![InlineNode::Text(TextRun::new(
                    "target content should appear after scroll",
                    text_style,
                ))],
                ParagraphStyle {
                    block: BlockStyle {
                        padding: Edges::all(8.0).expect("padding must be valid"),
                        background: Some([0.68, 0.42, 0.28, 1.0]),
                        ..BlockStyle::default()
                    },
                    ..ParagraphStyle::default()
                },
            )
            .expect("target paragraph must be valid"),
        );
        let clipped_card = BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![target],
                BlockStyle {
                    clip: ClipMode::Rect,
                    padding: Edges::all(8.0).expect("padding must be valid"),
                    background: Some([0.24, 0.18, 0.16, 1.0]),
                    ..BlockStyle::default()
                },
            )
            .expect("clipped card stack must be valid"),
        );

        DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![spacer, clipped_card],
                BlockStyle {
                    clip: ClipMode::Rect,
                    background: Some([0.08, 0.1, 0.14, 1.0]),
                    ..BlockStyle::default()
                },
            )
            .expect("root stack must be valid"),
        ))
        .expect("clipped scroll regression tree must be valid")
    }

    fn overlay_test_block_ids(tree: &DocumentTree) -> (u64, u64) {
        let BlockNode::Stack(root) = tree.root() else {
            panic!("overlay test tree root must be a stack");
        };
        let BlockNode::Overlay(content_overlay) = &root.children[2] else {
            panic!("expected block-relative overlay");
        };
        let BlockNode::Overlay(viewport_overlay) = &root.children[3] else {
            panic!("expected viewport overlay");
        };
        (
            block_node_id(content_overlay.child.as_ref()),
            block_node_id(viewport_overlay.child.as_ref()),
        )
    }

    fn clipped_scroll_target_block_id(tree: &DocumentTree) -> u64 {
        let BlockNode::Stack(root) = tree.root() else {
            panic!("clipped scroll regression tree root must be a stack");
        };
        let BlockNode::Stack(clipped_card) = &root.children[1] else {
            panic!("expected clipped card stack");
        };
        let BlockNode::Paragraph(target) = &clipped_card.children[0] else {
            panic!("expected target paragraph");
        };
        target.node_id.value()
    }

    fn find_block<'a>(
        scene: &'a crate::scene::SceneBufferInner<'a>,
        block_id: u64,
    ) -> &'a BlockDataArena<'a> {
        scene
            .blocks()
            .iter()
            .find(|block| block.block_id() == BlockId::new(block_id))
            .expect("block must exist in scene")
    }

    fn scene_contains_block(scene: &crate::scene::SceneBufferInner<'_>, block_id: u64) -> bool {
        scene
            .blocks()
            .iter()
            .any(|block| block.block_id() == BlockId::new(block_id))
    }

    fn block_node_id(block: &BlockNode) -> u64 {
        match block {
            BlockNode::Stack(node) => node.node_id.value(),
            BlockNode::Paragraph(node) => node.node_id.value(),
            BlockNode::Embed(node) => node.node_id.value(),
            BlockNode::Overlay(node) => node.node_id.value(),
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }
}
