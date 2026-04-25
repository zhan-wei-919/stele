//! CPU-side assembly of glyph, rect, path, and image data from view-owned scene cache.

use std::mem::size_of;

use log::info;

use super::super::state::{BlockGpuBatch, ImageBatch, PrimitiveRange, Renderer};
use crate::draw_list::ImageCmd;
use crate::renderer::instance::{GlyphInstance, ImageInstance, PathVertex, RectInstance};
use crate::scene::{BlockDataArena, SceneBuffer};

const MAX_PATH_VERTEX_BYTES: usize = 1024 * 1024;
const TESSELLATION_CACHE_EVICTION_AGE: u64 = 120;

impl<'window> Renderer<'window> {
    pub(in crate::renderer::runtime) fn rebuild_gpu_data(&mut self, scene_buffer: &SceneBuffer) {
        debug_assert_scene_buffer_order(scene_buffer);
        let ordered_blocks = scene_buffer.blocks();
        let (glyph_instances, glyph_ranges) = self.build_glyph_instances(ordered_blocks);
        let (rect_instances, rect_ranges, foreground_rect_ranges) =
            self.build_rect_instances(ordered_blocks);
        let (path_vertices, path_indices, path_ranges, tessellation_count) =
            self.build_path_geometry(ordered_blocks);
        let (image_instances, image_batches) = self.build_image_instances(ordered_blocks);

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
            ordered_blocks,
            &glyph_ranges,
            &rect_ranges,
            &foreground_rect_ranges,
            &path_ranges,
            &image_batches,
        );
        self.refresh_glyph_bind_group();
        self.tessellation_cache
            .evict_stale(TESSELLATION_CACHE_EVICTION_AGE);

        if tessellation_count > 0 {
            info!("path.tessellate count={tessellation_count}");
        }
    }

    fn build_glyph_instances(
        &self,
        ordered_blocks: &[BlockDataArena<'_>],
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
        ordered_blocks: &[BlockDataArena<'_>],
    ) -> (Vec<RectInstance>, Vec<PrimitiveRange>, Vec<PrimitiveRange>) {
        let mut rect_instances = Vec::new();
        let mut block_ranges = Vec::with_capacity(ordered_blocks.len());
        let mut foreground_block_ranges = Vec::with_capacity(ordered_blocks.len());

        for block in ordered_blocks {
            let start = rect_instances.len() as u32;
            rect_instances.extend_from_slice(block.rects());
            block_ranges.push(PrimitiveRange::new(
                start,
                rect_instances.len() as u32 - start,
            ));

            let foreground_start = rect_instances.len() as u32;
            rect_instances.extend_from_slice(block.foreground_rects());
            foreground_block_ranges.push(PrimitiveRange::new(
                foreground_start,
                rect_instances.len() as u32 - foreground_start,
            ));
        }

        (rect_instances, block_ranges, foreground_block_ranges)
    }

    fn build_path_geometry(
        &mut self,
        ordered_blocks: &[BlockDataArena<'_>],
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
        &mut self,
        ordered_blocks: &[BlockDataArena<'_>],
    ) -> (Vec<ImageInstance>, Vec<Vec<ImageBatch>>) {
        let device = &self.device;
        let queue = &self.queue;
        let bind_group_layout = &self.image_bind_group_layout;
        let screen_buffer = &self.screen_buffer;
        let sampler = &self.image_sampler;
        let image_cache = &mut self.image_cache;

        collect_image_instances(ordered_blocks, self.scale_factor, |image| {
            image_cache.get_or_insert(
                image.data(),
                device,
                queue,
                bind_group_layout,
                screen_buffer,
                sampler,
            );
        })
    }
}

fn debug_assert_scene_buffer_order(scene_buffer: &SceneBuffer) {
    debug_assert_eq!(
        scene_buffer.order().len(),
        scene_buffer.blocks().len(),
        "scene buffer order and block payloads must stay aligned",
    );
    debug_assert!(
        scene_buffer
            .order()
            .iter()
            .zip(scene_buffer.blocks())
            .all(|(block_id, block)| *block_id == block.block_id()),
        "scene buffer order must mirror the renderer block payload order",
    );
}

fn assemble_block_batches(
    ordered_blocks: &[BlockDataArena<'_>],
    glyph_ranges: &[PrimitiveRange],
    rect_ranges: &[PrimitiveRange],
    foreground_rect_ranges: &[PrimitiveRange],
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
            batch.foreground_rect_range = foreground_rect_ranges[index];
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

fn collect_image_instances<F>(
    ordered_blocks: &[BlockDataArena<'_>],
    scale_factor: f32,
    mut cache_image: F,
) -> (Vec<ImageInstance>, Vec<Vec<ImageBatch>>)
where
    F: FnMut(&ImageCmd),
{
    let mut image_instances = Vec::new();
    let mut block_batches = Vec::with_capacity(ordered_blocks.len());

    for block in ordered_blocks {
        let mut image_batches = Vec::new();
        let mut active_hash = None;
        let mut active_start = 0u32;

        for image in block.images() {
            let content_hash = image.data().content_hash();
            let instance_index = image_instances.len() as u32;
            cache_image(image);
            image_instances.push(ImageInstance::from_image(image, scale_factor));

            match active_hash {
                Some(current_hash) if current_hash == content_hash => {}
                Some(current_hash) => {
                    image_batches.push(ImageBatch {
                        content_hash: current_hash,
                        range: PrimitiveRange::new(active_start, instance_index - active_start),
                    });
                    active_hash = Some(content_hash);
                    active_start = instance_index;
                }
                None => {
                    active_hash = Some(content_hash);
                    active_start = instance_index;
                }
            }
        }

        if let Some(content_hash) = active_hash {
            let end = image_instances.len() as u32;
            image_batches.push(ImageBatch {
                content_hash,
                range: PrimitiveRange::new(active_start, end - active_start),
            });
        }

        block_batches.push(image_batches);
    }

    (image_instances, block_batches)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use bumpalo::Bump;

    use super::collect_image_instances;
    use crate::draw_list::{ClipRect, ImageCmd, ImageData, RenderLayer};
    use crate::renderer::runtime::state::PrimitiveRange;
    use crate::scene::{BlockDataArena, BlockId};

    #[test]
    fn image_rebuild_generates_instances_and_contiguous_batches() {
        let owner = Bump::new();
        let block_a = sample_block(
            &owner,
            0,
            vec![
                sample_image([1.0, 2.0], [3.0, 4.0], [255, 0, 0, 255]),
                sample_image([5.0, 6.0], [7.0, 8.0], [255, 0, 0, 255]),
                sample_image([9.0, 10.0], [11.0, 12.0], [0, 255, 0, 255]),
            ],
        );
        let block_b = sample_block(
            &owner,
            1,
            vec![sample_image([13.0, 14.0], [15.0, 16.0], [255, 0, 0, 255])],
        );
        let ordered_blocks = vec![block_a, block_b];

        let (instances, batches) = collect_image_instances(&ordered_blocks, 2.0, |_| {});

        assert_eq!(instances.len(), 4);
        assert_eq!(instances[0].pos, [2.0, 4.0]);
        assert_eq!(instances[0].size, [6.0, 8.0]);
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[0][0].range, PrimitiveRange::new(0, 2),);
        assert_eq!(batches[0][1].range, PrimitiveRange::new(2, 1),);
        assert_eq!(batches[0][0].content_hash, batches[1][0].content_hash);
        assert_eq!(
            batches[1],
            vec![super::ImageBatch {
                content_hash: batches[0][0].content_hash,
                range: PrimitiveRange::new(3, 1),
            }]
        );
    }

    #[test]
    fn duplicate_content_hashes_only_create_one_cache_entry() {
        let owner = Bump::new();
        let block_a = sample_block(
            &owner,
            0,
            vec![
                sample_image([1.0, 2.0], [3.0, 4.0], [255, 0, 0, 255]),
                sample_image([5.0, 6.0], [7.0, 8.0], [255, 0, 0, 255]),
            ],
        );
        let block_b = sample_block(
            &owner,
            1,
            vec![sample_image([9.0, 10.0], [11.0, 12.0], [255, 0, 0, 255])],
        );
        let ordered_blocks = vec![block_a, block_b];
        let mut seen_hashes = HashSet::new();
        let mut created = 0usize;

        collect_image_instances(&ordered_blocks, 1.0, |image| {
            if seen_hashes.insert(image.data().content_hash()) {
                created += 1;
            }
        });

        assert_eq!(created, 1);
    }

    fn sample_block<'a>(
        owner: &'a Bump,
        block_id: u64,
        images: Vec<ImageCmd>,
    ) -> BlockDataArena<'a> {
        let mut block = BlockDataArena::new_in(
            owner,
            BlockId::new(block_id),
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            0,
        );
        block.images_mut().extend(images);
        block
    }

    fn sample_image(pos: [f32; 2], size: [f32; 2], rgba: [u8; 4]) -> ImageCmd {
        ImageCmd::new(
            pos,
            size,
            Arc::new(ImageData::new(rgba.to_vec(), 1, 1)),
            RenderLayer::Foreground,
        )
    }
}
