//! Store-side full-scene composition from layout output into render-ready block batches.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bytemuck::cast_slice;

use crate::draw_list::{ClipRect, PathCmd};
use crate::font::FreeTypeRasterizer;
use crate::layout::layout_document;
use crate::renderer::instance::{GlyphInstance, ImageInstance, RectInstance};
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
                compose_block(
                    layout_block,
                    logical_atlas,
                    rasterizer,
                    viewport.scale_factor,
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
    let paths = Vec::<PathCmd>::new();
    let images = Vec::<ImageInstance>::new();
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
    images: &[ImageInstance],
    atlas_generation: Option<u64>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_clip_rect(&mut hasher, clip_rect);
    z_order.hash(&mut hasher);
    hash_pod_slice(&mut hasher, glyphs);
    hash_pod_slice(&mut hasher, rects);
    hash_paths(&mut hasher, paths);
    hash_pod_slice(&mut hasher, images);
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

#[cfg(test)]
mod tests {
    use super::sort_entries_by_z_order;
    use crate::draw_list::ClipRect;
    use crate::scene::{BlockId, BlockSceneBatch};

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
}
