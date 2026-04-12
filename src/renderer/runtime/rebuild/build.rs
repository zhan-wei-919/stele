//! CPU-side assembly of glyph, rect, path, and image data from view-owned scene cache.

use std::mem::size_of;

use log::{info, warn};

use super::super::state::{BlockGpuBatch, ImageBatch, PrimitiveRange, Renderer};
use crate::renderer::instance::{GlyphInstance, ImageInstance, PathVertex, RectInstance};
use crate::scene::{BlockSceneBatch, ViewState};

const CACHE_EVICTION_AGE: u64 = 120;
const MAX_PATH_VERTEX_BYTES: usize = 1024 * 1024;

impl<'window> Renderer<'window> {
    pub(in crate::renderer::runtime) fn rebuild_gpu_data(&mut self, view_state: &ViewState) {
        let ordered_blocks = ordered_batches(view_state);
        let (glyph_instances, glyph_ranges) = self.build_glyph_instances(&ordered_blocks);
        let (rect_instances, rect_ranges) = self.build_rect_instances(&ordered_blocks);
        let (path_vertices, path_indices, path_ranges, tessellation_count) =
            self.build_path_geometry(&ordered_blocks);
        let (image_instances, image_batches) = self.build_image_instances(&ordered_blocks);

        self.ensure_instance_capacity(
            glyph_instances.len(),
            rect_instances.len(),
            path_vertices.len(),
            path_indices.len(),
            image_instances.len(),
        );
        self.upload_glyph_instances(&glyph_instances);
        self.upload_rect_instances(&rect_instances);
        self.upload_path_geometry(&path_vertices, &path_indices);
        self.upload_image_instances(&image_instances);
        self.block_batches = assemble_block_batches(
            &ordered_blocks,
            &glyph_ranges,
            &rect_ranges,
            &path_ranges,
            &image_batches,
        );
        self.refresh_glyph_bind_group();
        self.tessellation_cache.evict_stale(CACHE_EVICTION_AGE);
        self.image_cache.evict_stale(CACHE_EVICTION_AGE);

        if tessellation_count > 0 {
            info!("path.tessellate count={tessellation_count}");
        }
    }

    fn build_glyph_instances(
        &self,
        ordered_blocks: &[&BlockSceneBatch],
    ) -> (Vec<GlyphInstance>, Vec<PrimitiveRange>) {
        let mut glyph_instances = Vec::new();
        let mut block_ranges = Vec::with_capacity(ordered_blocks.len());

        for block in ordered_blocks {
            let start = glyph_instances.len() as u32;
            glyph_instances.extend_from_slice(block.glyphs());
            block_ranges.push(PrimitiveRange::new(
                start,
                glyph_instances.len() as u32 - start,
            ));
        }

        (glyph_instances, block_ranges)
    }

    fn build_rect_instances(
        &self,
        ordered_blocks: &[&BlockSceneBatch],
    ) -> (Vec<RectInstance>, Vec<PrimitiveRange>) {
        let mut rect_instances = Vec::new();
        let mut block_ranges = Vec::with_capacity(ordered_blocks.len());

        for block in ordered_blocks {
            let start = rect_instances.len() as u32;
            rect_instances.extend_from_slice(block.rects());
            block_ranges.push(PrimitiveRange::new(
                start,
                rect_instances.len() as u32 - start,
            ));
        }

        (rect_instances, block_ranges)
    }

    fn build_path_geometry(
        &mut self,
        ordered_blocks: &[&BlockSceneBatch],
    ) -> (Vec<PathVertex>, Vec<u32>, Vec<PrimitiveRange>, usize) {
        let mut path_vertices = Vec::new();
        let mut path_indices = Vec::new();
        let mut block_ranges = Vec::with_capacity(ordered_blocks.len());
        let mut tessellation_count = 0usize;

        for block in ordered_blocks {
            let start = path_indices.len() as u32;
            for path in block.paths() {
                if path.verbs().is_empty() {
                    continue;
                }

                let (mesh, created) = self
                    .tessellation_cache
                    .get_or_insert(path, self.scale_factor);
                if created {
                    tessellation_count += 1;
                }
                append_cached_path_mesh(&mut path_vertices, &mut path_indices, mesh);
            }
            block_ranges.push(PrimitiveRange::new(
                start,
                path_indices.len() as u32 - start,
            ));
        }

        debug_assert!(
            path_vertices.len() * size_of::<PathVertex>() <= MAX_PATH_VERTEX_BYTES,
            "path vertex buffer exceeded the M0 1MB budget"
        );

        (
            path_vertices,
            path_indices,
            block_ranges,
            tessellation_count,
        )
    }

    fn build_image_instances(
        &self,
        ordered_blocks: &[&BlockSceneBatch],
    ) -> (Vec<ImageInstance>, Vec<Vec<ImageBatch>>) {
        let mut warned = false;
        let mut batches = Vec::with_capacity(ordered_blocks.len());
        for block in ordered_blocks {
            if !block.images().is_empty() && !warned {
                warn!("renderer.image.unsupported reason=missing_payload_in_scene_batch");
                warned = true;
            }
            batches.push(Vec::new());
        }
        (Vec::new(), batches)
    }
}

fn ordered_batches(view_state: &ViewState) -> Vec<&BlockSceneBatch> {
    view_state
        .block_order()
        .iter()
        .filter_map(|block_id| view_state.blocks().get(block_id))
        .collect()
}

fn assemble_block_batches(
    ordered_blocks: &[&BlockSceneBatch],
    glyph_ranges: &[PrimitiveRange],
    rect_ranges: &[PrimitiveRange],
    path_ranges: &[PrimitiveRange],
    image_batches: &[Vec<ImageBatch>],
) -> Vec<BlockGpuBatch> {
    ordered_blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let mut batch = BlockGpuBatch::empty(block.clip_rect());
            batch.glyph_range = glyph_ranges[index];
            batch.rect_range = rect_ranges[index];
            batch.path_range = path_ranges[index];
            batch.image_batches = image_batches[index].clone();
            batch
        })
        .collect()
}

fn append_cached_path_mesh(
    path_vertices: &mut Vec<PathVertex>,
    path_indices: &mut Vec<u32>,
    mesh: &crate::renderer::tessellation::CachedMesh,
) {
    let vertex_offset = path_vertices.len() as u32;
    path_vertices.extend_from_slice(&mesh.vertices);
    path_indices.extend(mesh.indices.iter().map(|index| index + vertex_offset));
}
