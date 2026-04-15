//! Store-side full-scene composition from layout output into render-ready block batches.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bytemuck::cast_slice;

use crate::draw_list::{ClipRect, ImageCmd, PathCmd};
use crate::font::FreeTypeRasterizer;
use crate::layout::layout_document;
use crate::renderer::instance::{GlyphInstance, RectInstance};
use crate::scene::{BlockId, BlockSceneBatch};

use super::logical_atlas::LogicalAtlas;
use super::model::{LayoutCache, Model};
use super::types::{SceneSnapshot, ViewportState};

/// Stateless composer from logical layout output to render-ready block batches.
pub(crate) struct Composer;

impl Composer {
    /// Recomputes the full scene snapshot for the current model and viewport.
    pub(crate) fn compose_snapshot(
        &self,
        model: &Model,
        layout_cache: &LayoutCache,
        logical_atlas: &mut LogicalAtlas,
        rasterizer: &FreeTypeRasterizer,
        viewport: ViewportState,
    ) -> SceneSnapshot {
        let mut entries = layout_document(model.document(), layout_cache.prepared_blocks())
            .into_iter()
            .map(|layout_block| {
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
                compose_block(
                    layout_block,
                    logical_atlas,
                    rasterizer,
                    viewport.scale_factor,
                    paths,
                    images,
                )
            })
            .collect::<Vec<_>>();

        sort_entries_by_z_order(&mut entries);
        let required_atlas_generation = entries
            .iter()
            .any(|(_, batch)| !batch.glyphs().is_empty())
            .then_some(logical_atlas.generation);
        let order = entries.iter().map(|(block_id, _)| *block_id).collect();
        let blocks = entries.into_iter().collect::<HashMap<_, _>>();

        SceneSnapshot {
            viewport_revision: viewport.viewport_revision,
            required_atlas_generation,
            order,
            blocks,
        }
    }
}

fn sort_entries_by_z_order(entries: &mut [(BlockId, BlockSceneBatch)]) {
    // Keep the sort key to z-order only. This works because prepare/layout already emit
    // blocks in current document order, and Rust's `sort_by_key` is stable, so blocks
    // that share a z-order keep the upstream document order instead of falling back to
    // stable ids or creation order.
    entries.sort_by_key(|(_, batch)| batch.z_order());
}

fn compose_block(
    layout_block: crate::layout::LayoutBlock,
    logical_atlas: &mut LogicalAtlas,
    rasterizer: &FreeTypeRasterizer,
    scale_factor: f32,
    paths: Vec<PathCmd>,
    images: Vec<ImageCmd>,
) -> (BlockId, BlockSceneBatch) {
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

    (
        layout_block.block_id,
        BlockSceneBatch::new(
            clip_rect,
            layout_block.z_order,
            glyphs,
            rects,
            paths,
            images,
            fingerprint,
        ),
    )
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

    use super::sort_entries_by_z_order;
    use super::Composer;
    use crate::draw_list::{
        ClipRect, ImageCmd, ImageData, LineCap, LineJoin, PathCmd, PathVerb, RenderLayer,
        StrokeStyle,
    };
    use crate::font::{FontDiscovery, FreeTypeRasterizer};
    use crate::layout::{Block, BlockRect, Document, PreparedBlock};
    use crate::renderer::subpixel::detect_subpixel_layout;
    use crate::scene::{BlockId, BlockSceneBatch};
    use crate::store::logical_atlas::LogicalAtlas;
    use crate::store::{BlockDrawCommands, Model, ViewportState};

    use super::super::model::LayoutCache;

    #[test]
    fn stable_z_order_sort_preserves_upstream_document_order_within_a_layer() {
        let mut entries = vec![
            (BlockId::new(30), sample_batch(1)),
            (BlockId::new(99), sample_batch(0)),
            (BlockId::new(10), sample_batch(0)),
            (BlockId::new(40), sample_batch(1)),
        ];

        sort_entries_by_z_order(&mut entries);

        let order = entries
            .into_iter()
            .map(|(block_id, _)| block_id)
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
    fn compose_snapshot_attaches_paths_to_block_batch() {
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
        let snapshot = Composer.compose_snapshot(
            &model,
            &layout_cache,
            &mut logical_atlas,
            &rasterizer,
            ViewportState::new(120, 90, 1.0, 7),
        );

        let batch = snapshot
            .blocks
            .get(&BlockId::new(0))
            .expect("snapshot must contain the block batch");
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

    fn sample_batch(z_order: u32) -> BlockSceneBatch {
        BlockSceneBatch::new(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            z_order,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
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
