//! CPU-side assembly of block-aware glyph, rect, path, and image draw data.

use std::mem::size_of;

use log::{info, warn};

use super::super::state::{BlockGpuBatch, ImageBatch, PrimitiveRange, Renderer};
use crate::renderer::draw_list::{BlockDrawGroup, RenderLayer};
use crate::renderer::instance::{GlyphInstance, ImageInstance, PathVertex, RectInstance};

const CACHE_EVICTION_AGE: u64 = 120;
const LAYER_COUNT: usize = RenderLayer::ALL.len();
const MAX_PATH_VERTEX_BYTES: usize = 1024 * 1024;

impl<'window> Renderer<'window> {
    pub(in crate::renderer::runtime) fn rebuild_gpu_data(&mut self) {
        let block_groups = self.draw_list.block_groups(self.viewport_clip_rect());
        let (glyph_instances, glyph_ranges, atlas_uploads) =
            self.build_glyph_instances(&block_groups);
        let (rect_instances, rect_ranges) = self.build_rect_instances(&block_groups);
        let (path_vertices, path_indices, path_ranges, tessellation_count) =
            self.build_path_geometry(&block_groups);
        let (image_instances, image_batches, image_uploads) =
            self.build_image_instances(&block_groups);

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
            &block_groups,
            &glyph_ranges,
            &rect_ranges,
            &path_ranges,
            &image_batches,
        );
        self.refresh_glyph_bind_group();
        self.tessellation_cache.evict_stale(CACHE_EVICTION_AGE);
        self.image_cache.evict_stale(CACHE_EVICTION_AGE);
        self.dirty = false;

        if atlas_uploads > 0 {
            info!("frame.write_texture target=atlas count={atlas_uploads}");
        }
        if tessellation_count > 0 {
            info!("path.tessellate count={tessellation_count}");
        }
        if image_uploads > 0 {
            info!("image.upload count={image_uploads}");
        }
    }

    fn build_glyph_instances(
        &mut self,
        block_groups: &[BlockDrawGroup],
    ) -> (
        Vec<GlyphInstance>,
        Vec<[PrimitiveRange; LAYER_COUNT]>,
        usize,
    ) {
        let mut atlas_uploads = 0usize;
        let mut glyph_instances = Vec::new();
        let mut block_ranges = Vec::with_capacity(block_groups.len());

        for block in block_groups {
            let mut ranges = [PrimitiveRange::default(); LAYER_COUNT];
            for layer in RenderLayer::ALL {
                let start = glyph_instances.len() as u32;
                for glyph in block.layer(layer).glyphs() {
                    let key = glyph.glyph_key(self.scale_factor);
                    let cached = self.atlas.cache.contains_key(&key);
                    let region = match self.atlas.get_or_insert(key, &self.queue, &self.rasterizer)
                    {
                        Ok(region) => region,
                        Err(err) => {
                            warn!("atlas.full glyph_id={} error={err}", key.glyph_id);
                            continue;
                        }
                    };
                    if !cached {
                        atlas_uploads += 1;
                    }
                    if region.size[0] == 0.0 || region.size[1] == 0.0 {
                        continue;
                    }
                    glyph_instances.push(GlyphInstance::from_positioned_glyph(
                        glyph,
                        region,
                        self.scale_factor,
                    ));
                }
                ranges[layer.index()] =
                    PrimitiveRange::new(start, glyph_instances.len() as u32 - start);
            }
            block_ranges.push(ranges);
        }

        (glyph_instances, block_ranges, atlas_uploads)
    }

    fn build_rect_instances(
        &self,
        block_groups: &[BlockDrawGroup],
    ) -> (Vec<RectInstance>, Vec<[PrimitiveRange; LAYER_COUNT]>) {
        let mut rect_instances = Vec::new();
        let mut block_ranges = Vec::with_capacity(block_groups.len());

        for block in block_groups {
            let mut ranges = [PrimitiveRange::default(); LAYER_COUNT];
            for layer in RenderLayer::ALL {
                let start = rect_instances.len() as u32;
                for rect in block.layer(layer).rects() {
                    rect_instances.push(RectInstance::from_rect(*rect, self.scale_factor));
                }
                ranges[layer.index()] =
                    PrimitiveRange::new(start, rect_instances.len() as u32 - start);
            }
            block_ranges.push(ranges);
        }

        (rect_instances, block_ranges)
    }

    fn build_path_geometry(
        &mut self,
        block_groups: &[BlockDrawGroup],
    ) -> (
        Vec<PathVertex>,
        Vec<u32>,
        Vec<[PrimitiveRange; LAYER_COUNT]>,
        usize,
    ) {
        let mut path_vertices = Vec::new();
        let mut path_indices = Vec::new();
        let mut block_ranges = Vec::with_capacity(block_groups.len());
        let mut tessellation_count = 0usize;

        for block in block_groups {
            let mut ranges = [PrimitiveRange::default(); LAYER_COUNT];
            for layer in RenderLayer::ALL {
                let start = path_indices.len() as u32;
                for path in block.layer(layer).paths() {
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
                ranges[layer.index()] =
                    PrimitiveRange::new(start, path_indices.len() as u32 - start);
            }
            block_ranges.push(ranges);
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
        block_groups: &[BlockDrawGroup],
    ) -> (
        Vec<ImageInstance>,
        Vec<[Vec<ImageBatch>; LAYER_COUNT]>,
        usize,
    ) {
        let mut image_instances = Vec::new();
        let mut block_batches = Vec::with_capacity(block_groups.len());
        let mut image_uploads = 0usize;

        for block in block_groups {
            let mut per_layer = std::array::from_fn(|_| Vec::<ImageBatch>::new());
            for layer in RenderLayer::ALL {
                let layer_batches = &mut per_layer[layer.index()];
                for image in block.layer(layer).images() {
                    if self.image_cache.get_or_insert(
                        image.data(),
                        &self.device,
                        &self.queue,
                        &self.image_bind_group_layout,
                        &self.screen_buffer,
                        &self.image_sampler,
                    ) {
                        image_uploads += 1;
                    }

                    let content_hash = image.data().content_hash();
                    let instance_start = image_instances.len() as u32;
                    image_instances.push(ImageInstance::from_image(image, self.scale_factor));
                    extend_or_start_image_batch(layer_batches, content_hash, instance_start);
                }
            }
            block_batches.push(per_layer);
        }

        (image_instances, block_batches, image_uploads)
    }
}

fn assemble_block_batches(
    block_groups: &[BlockDrawGroup],
    glyph_ranges: &[[PrimitiveRange; LAYER_COUNT]],
    rect_ranges: &[[PrimitiveRange; LAYER_COUNT]],
    path_ranges: &[[PrimitiveRange; LAYER_COUNT]],
    image_batches: &[[Vec<ImageBatch>; LAYER_COUNT]],
) -> Vec<BlockGpuBatch> {
    block_groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let mut batch = BlockGpuBatch::empty(group.clip_rect());
            batch.glyph_ranges_by_layer = glyph_ranges[index];
            batch.rect_ranges_by_layer = rect_ranges[index];
            batch.path_ranges_by_layer = path_ranges[index];
            batch.image_batches_by_layer = image_batches[index].clone();
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

fn extend_or_start_image_batch(
    layer_batches: &mut Vec<ImageBatch>,
    content_hash: u64,
    instance_start: u32,
) {
    match layer_batches.last_mut() {
        Some(batch)
            if batch.content_hash == content_hash && batch.range.end() == instance_start =>
        {
            batch.range.count += 1;
        }
        _ => layer_batches.push(ImageBatch {
            content_hash,
            range: PrimitiveRange::new(instance_start, 1),
        }),
    }
}
