//! Store-side full-scene composition from layout output into arena-backed scene buffers.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bumpalo::Bump;
use bytemuck::cast_slice;
use log::{info, warn};

use crate::draw_list::{ClipRect, ImageCmd, PathCmd, PathVerb, RectCmd, RenderLayer, StrokeStyle};
use crate::font::FreeTypeRasterizer;
use crate::layout::layout_document;
use crate::layout::layout_tree::{
    self, LayoutAtomPayload, LayoutBlock as TreeLayoutBlock,
    LayoutBlockContent as TreeLayoutBlockContent, LayoutConstraints, LayoutEmbedKind,
    LayoutRect as TreeLayoutRect, LayoutRun as TreeLayoutRun,
};
use crate::renderer::instance::{GlyphInstance, RectInstance};
use crate::scene::{BlockDataArena, BlockId, SceneBufferInner, SceneFrameMetadata};

use super::logical_atlas::LogicalAtlas;
use super::model::{DocumentSource, LayoutCache, Model, PreparedLayoutCache};
use super::types::ViewportState;

/// Stateless composer from logical layout output to one render-ready scene buffer.
pub(crate) struct Composer;

type OrderedBlock<'a> = (u32, BlockDataArena<'a>);

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
        clear_tessellation_cache: bool,
        max_blocks_per_scene: usize,
    ) -> SceneBufferInner<'a> {
        let mut entries = match (model.document_source(), layout_cache.prepared()) {
            (DocumentSource::Legacy(document), PreparedLayoutCache::Legacy(prepared_blocks)) => {
                compose_legacy_entries(
                    owner,
                    document,
                    model,
                    prepared_blocks,
                    logical_atlas,
                    rasterizer,
                    viewport.scale_factor,
                )
            }
            (DocumentSource::Tree(_), PreparedLayoutCache::Tree(prepared_tree)) => {
                compose_tree_entries(owner, prepared_tree, logical_atlas, rasterizer, viewport)
            }
            _ => {
                debug_assert!(
                    false,
                    "model and layout cache must share the same source kind"
                );
                Vec::new()
            }
        };

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
        scene
    }
}

fn sort_entries_by_z_order(entries: &mut [BlockDataArena<'_>]) {
    // Keep the sort key to z-order only. This works because prepare/layout already emit
    // blocks in current document order, and Rust's `sort_by_key` is stable, so blocks
    // that share a z-order keep the upstream document order instead of falling back to
    // stable ids or creation order.
    entries.sort_by_key(BlockDataArena::z_order);
}

fn compose_legacy_entries<'a>(
    owner: &'a Bump,
    document: &crate::layout::Document,
    model: &Model,
    prepared_blocks: &[crate::layout::PreparedBlock],
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) -> Vec<OrderedBlock<'a>> {
    layout_document(document, prepared_blocks)
        .into_iter()
        .enumerate()
        .map(|(doc_order, layout_block)| {
            let images = model
                .block_draw_commands()
                .images()
                .get(&layout_block.block_id)
                .cloned()
                .unwrap_or_default();
            let paths = model
                .block_draw_commands()
                .paths()
                .get(&layout_block.block_id)
                .cloned()
                .unwrap_or_default();
            (
                doc_order as u32,
                compose_legacy_block(
                    owner,
                    layout_block,
                    logical_atlas,
                    rasterizer,
                    scale_factor,
                    paths,
                    images,
                ),
            )
        })
        .collect()
}

fn compose_tree_entries<'a>(
    owner: &'a Bump,
    prepared_tree: &crate::layout::prepare_tree::PreparedTree,
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    viewport: ViewportState,
) -> Vec<OrderedBlock<'a>> {
    let layout_tree = layout_tree::layout_tree(
        prepared_tree,
        LayoutConstraints::new(
            viewport.logical_size()[0].max(1.0),
            Some(viewport.logical_size()[1].max(1.0)),
            viewport.scale_factor,
            viewport.logical_size(),
        ),
    );
    let mut entries = Vec::new();
    collect_tree_entries(
        owner,
        &layout_tree.root,
        &mut entries,
        logical_atlas,
        rasterizer,
        viewport.scale_factor,
    );
    for overlay in &layout_tree.overlays {
        collect_tree_entries(
            owner,
            overlay,
            &mut entries,
            logical_atlas,
            rasterizer,
            viewport.scale_factor,
        );
    }
    info!(
        "layout.tree.compose block_count={} overlay_count={}",
        entries.len(),
        layout_tree.overlays.len()
    );
    entries
}

fn sort_ordered_entries(entries: &mut [OrderedBlock<'_>]) {
    entries.sort_by_key(|(doc_order, block)| (block.z_order(), *doc_order));
}

fn compose_legacy_block<'a>(
    owner: &'a Bump,
    layout_block: crate::layout::LayoutBlock,
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
    paths: Vec<PathCmd>,
    images: Vec<ImageCmd>,
) -> BlockDataArena<'a> {
    let mut glyphs = Vec::new();
    let mut rects = Vec::new();

    if let Some(background_rect) = layout_block.background_rect {
        rects.push(RectInstance::from_rect(background_rect, scale_factor));
    }

    for line in &layout_block.lines {
        for run in &line.runs {
            for glyph in &run.glyphs {
                let key = glyph.glyph_key(scale_factor);
                let region = logical_atlas.get_or_insert(key, rasterizer);
                glyphs.push(GlyphInstance::from_positioned_glyph(
                    glyph,
                    region,
                    scale_factor,
                ));
            }
            rects.extend(
                run.decoration_rects
                    .iter()
                    .copied()
                    .map(|rect| RectInstance::from_rect(rect, scale_factor)),
            );
        }
    }

    let clip_rect = ClipRect::new(
        layout_block.clip_rect.x(),
        layout_block.clip_rect.y(),
        layout_block.clip_rect.width(),
        layout_block.clip_rect.height(),
    );
    let fingerprint = fingerprint_batch(
        clip_rect,
        layout_block.z_order,
        &glyphs,
        &rects,
        &paths,
        &images,
        (!glyphs.is_empty()).then_some(logical_atlas.generation),
    );

    let mut block = BlockDataArena::new_in(
        owner,
        layout_block.block_id,
        clip_rect,
        layout_block.z_order,
        fingerprint,
    );
    block.glyphs_mut().extend(glyphs);
    block.rects_mut().extend(rects);
    block.paths_mut().extend(paths);
    block.images_mut().extend(images);
    block
}

fn collect_tree_entries<'a>(
    owner: &'a Bump,
    block: &TreeLayoutBlock,
    entries: &mut Vec<OrderedBlock<'a>>,
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) {
    if let Some(materialized) =
        compose_tree_block(owner, block, logical_atlas, rasterizer, scale_factor)
    {
        entries.push((block.doc_order, materialized));
    }

    if let TreeLayoutBlockContent::Stack { children } = &block.content {
        for child in children {
            collect_tree_entries(
                owner,
                child,
                entries,
                logical_atlas,
                rasterizer,
                scale_factor,
            );
        }
    }
}

fn compose_tree_block<'a>(
    owner: &'a Bump,
    block: &TreeLayoutBlock,
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
) -> Option<BlockDataArena<'a>> {
    if block.rect.is_empty() || block.clip_rect.is_empty() {
        return None;
    }

    let mut glyphs = Vec::new();
    let mut rects = Vec::new();
    let mut paths = Vec::new();
    let mut images = Vec::new();

    if let Some(background) = block.background {
        push_rect_instance(&mut rects, block.rect, background, scale_factor);
    }

    match &block.content {
        TreeLayoutBlockContent::Stack { .. } => {}
        TreeLayoutBlockContent::Paragraph(paragraph) => {
            for line in &paragraph.lines {
                for run in &line.runs {
                    match run {
                        TreeLayoutRun::Text(run) => {
                            for glyph in &run.glyphs {
                                let key = glyph.glyph_key(scale_factor);
                                let region = logical_atlas.get_or_insert(key, rasterizer);
                                glyphs.push(GlyphInstance::from_positioned_glyph(
                                    glyph,
                                    region,
                                    scale_factor,
                                ));
                            }
                            rects.extend(
                                run.decoration_rects
                                    .iter()
                                    .copied()
                                    .map(|rect| RectInstance::from_rect(rect, scale_factor)),
                            );
                        }
                        TreeLayoutRun::Atom(run) => match &run.payload {
                            LayoutAtomPayload::Chip {
                                background,
                                glyphs: chip_glyphs,
                            } => {
                                if let Some(color) = background {
                                    push_rect_instance(&mut rects, run.rect, *color, scale_factor);
                                }
                                for glyph in chip_glyphs {
                                    let key = glyph.glyph_key(scale_factor);
                                    let region = logical_atlas.get_or_insert(key, rasterizer);
                                    glyphs.push(GlyphInstance::from_positioned_glyph(
                                        glyph,
                                        region,
                                        scale_factor,
                                    ));
                                }
                            }
                            LayoutAtomPayload::Icon { glyph } => {
                                let key = glyph.glyph_key(scale_factor);
                                let region = logical_atlas.get_or_insert(key, rasterizer);
                                glyphs.push(GlyphInstance::from_positioned_glyph(
                                    glyph,
                                    region,
                                    scale_factor,
                                ));
                            }
                            LayoutAtomPayload::Image { data_ref } => {
                                images.push(ImageCmd::new(
                                    [run.rect.x(), run.rect.y()],
                                    [run.rect.width(), run.rect.height()],
                                    data_ref.clone(),
                                    RenderLayer::Foreground,
                                ));
                            }
                            LayoutAtomPayload::Custom => {}
                        },
                    }
                }
            }
        }
        TreeLayoutBlockContent::Embed(embed) => match &embed.kind {
            LayoutEmbedKind::Image { data_ref } => {
                images.push(ImageCmd::new(
                    [embed.rect.x(), embed.rect.y()],
                    [embed.rect.width(), embed.rect.height()],
                    data_ref.clone(),
                    RenderLayer::Foreground,
                ));
            }
            LayoutEmbedKind::Path {
                verbs,
                fill,
                stroke,
            } => {
                let scale_x = if embed.intrinsic_size[0] > 0.0 {
                    embed.rect.width() / embed.intrinsic_size[0]
                } else {
                    1.0
                };
                let scale_y = if embed.intrinsic_size[1] > 0.0 {
                    embed.rect.height() / embed.intrinsic_size[1]
                } else {
                    1.0
                };
                let width_scale = ((scale_x + scale_y) * 0.5).max(0.0);
                paths.push(PathCmd::new(
                    transform_path_verbs(verbs, embed.rect, embed.intrinsic_size),
                    *fill,
                    stroke.map(|stroke| {
                        StrokeStyle::new(
                            stroke.color,
                            (stroke.width * width_scale).max(1.0),
                            stroke.line_cap,
                            stroke.line_join,
                        )
                    }),
                    RenderLayer::Foreground,
                ));
            }
            LayoutEmbedKind::Custom => {}
        },
    }

    if glyphs.is_empty() && rects.is_empty() && paths.is_empty() && images.is_empty() {
        return None;
    }

    let clip_rect = ClipRect::new(
        block.clip_rect.x(),
        block.clip_rect.y(),
        block.clip_rect.width(),
        block.clip_rect.height(),
    );
    let fingerprint = fingerprint_batch(
        clip_rect,
        block.z_order,
        &glyphs,
        &rects,
        &paths,
        &images,
        (!glyphs.is_empty()).then_some(logical_atlas.generation),
    );
    let mut arena = BlockDataArena::new_in(
        owner,
        BlockId::new(block.node_id.value()),
        clip_rect,
        block.z_order,
        fingerprint,
    );
    arena.glyphs_mut().extend(glyphs);
    arena.rects_mut().extend(rects);
    arena.paths_mut().extend(paths);
    arena.images_mut().extend(images);
    Some(arena)
}

fn push_rect_instance(
    rects: &mut Vec<RectInstance>,
    rect: TreeLayoutRect,
    color: [f32; 4],
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
            RenderLayer::Background,
        ),
        scale_factor,
    ));
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
    use std::collections::HashMap;
    use std::sync::Arc;

    use bumpalo::Bump;

    use super::sort_entries_by_z_order;
    use super::Composer;
    use crate::draw_list::{
        ClipRect, ImageCmd, ImageData, LineCap, LineJoin, PathCmd, PathVerb, RenderLayer,
        StrokeStyle,
    };
    use crate::font::{FontDiscovery, FreeTypeRasterizer};
    use crate::layout::{Block, BlockRect, Document, PreparedBlock};
    use crate::renderer::subpixel::detect_subpixel_layout;
    use crate::scene::{BlockDataArena, BlockId};
    use crate::store::logical_atlas::LogicalAtlas;
    use crate::store::{BlockDrawCommands, Model, ViewportState};

    use super::super::model::LayoutCache;

    #[test]
    fn stable_z_order_sort_preserves_upstream_document_order_within_a_layer() {
        let owner = Bump::new();
        let mut entries = vec![
            sample_block(&owner, BlockId::new(30), 1),
            sample_block(&owner, BlockId::new(99), 0),
            sample_block(&owner, BlockId::new(10), 0),
            sample_block(&owner, BlockId::new(40), 1),
        ];

        sort_entries_by_z_order(&mut entries);

        let order = entries
            .into_iter()
            .map(|block| block.block_id())
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
        let clip_rect = BlockRect::new(0.0, 0.0, 120.0, 90.0).expect("rect must be valid");
        let block = Block::new(clip_rect, 0.0, None, Vec::new(), 0).expect("block must be valid");
        let document = Document::new(vec![block]);
        let path = sample_path(18.0);
        let model = Model::new(
            document,
            BlockDrawCommands::new(
                HashMap::new(),
                HashMap::from([(BlockId::new(0), vec![path.clone()])]),
            ),
        );
        let layout_cache = LayoutCache::new(vec![PreparedBlock {
            block_id: BlockId::new(0),
            document_index: 0,
            items: Vec::new(),
            default_ascent: 0.0,
            default_line_height: 0.0,
        }]);
        let rasterizer = build_rasterizer_for_test();
        let mut logical_atlas = LogicalAtlas::new(1.0);
        let scene = Composer.compose_into_buffer(
            &owner,
            &model,
            &layout_cache,
            &mut logical_atlas,
            &rasterizer,
            ViewportState::new(120, 90, 1.0, 7, None),
            false,
            512,
        );

        let batch = scene
            .blocks()
            .iter()
            .find(|batch| batch.block_id() == BlockId::new(0))
            .expect("scene must contain the block batch");
        assert_eq!(batch.paths().len(), 1);
        assert_eq!(batch.paths()[0].content_hash(), path.content_hash());
        assert_eq!(batch.paths()[0].layer(), path.layer());
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
}
