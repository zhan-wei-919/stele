//! CPU-side assembly of layered glyph, rect, path, and image draw data.

use std::mem::size_of;

use log::{info, warn};

use super::super::state::{ImageBatch, PrimitiveRange, Renderer};
use crate::renderer::draw_list::RenderLayer;
use crate::renderer::instance::{GlyphInstance, ImageInstance, PathVertex, RectInstance};

const CACHE_EVICTION_AGE: u64 = 120;
const MAX_PATH_VERTEX_BYTES: usize = 1024 * 1024;

impl<'window> Renderer<'window> {
    pub(in crate::renderer::runtime) fn rebuild_gpu_data(&mut self) {
        let (glyph_instances, glyph_ranges, atlas_uploads) = self.build_glyph_instances();
        let (rect_instances, rect_ranges) = self.build_rect_instances();
        let (path_vertices, path_indices, path_ranges, tessellation_count) =
            self.build_path_geometry();
        let (image_instances, image_batches, image_uploads) = self.build_image_instances();

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
        self.rect_ranges_by_layer = rect_ranges;
        self.path_ranges_by_layer = path_ranges;
        self.glyph_ranges_by_layer = glyph_ranges;
        self.image_batches_by_layer = image_batches;
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
    ) -> (
        Vec<GlyphInstance>,
        [PrimitiveRange; RenderLayer::ALL.len()],
        usize,
    ) {
        let mut atlas_uploads = 0usize;
        let mut glyph_instances = Vec::new();

        for line in &self.draw_list.lines {
            for glyph in line {
                let key = glyph.glyph_key(self.scale_factor);
                let cached = self.atlas.cache.contains_key(&key);
                let region = match self.atlas.get_or_insert(key, &self.queue, &self.rasterizer) {
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
        }

        let mut glyph_ranges = [PrimitiveRange::default(); RenderLayer::ALL.len()];
        glyph_ranges[RenderLayer::Content.index()] =
            PrimitiveRange::new(0, glyph_instances.len() as u32);
        (glyph_instances, glyph_ranges, atlas_uploads)
    }

    fn build_rect_instances(
        &self,
    ) -> (Vec<RectInstance>, [PrimitiveRange; RenderLayer::ALL.len()]) {
        let mut rect_instances = Vec::new();
        let mut rect_ranges = [PrimitiveRange::default(); RenderLayer::ALL.len()];

        for layer in RenderLayer::ALL {
            let start = rect_instances.len() as u32;
            for cmd in self
                .draw_list
                .rects
                .iter()
                .filter(|cmd| cmd.layer() == layer)
            {
                rect_instances.push(RectInstance::from_rect(*cmd, self.scale_factor));
            }
            rect_ranges[layer.index()] =
                PrimitiveRange::new(start, rect_instances.len() as u32 - start);
        }

        (rect_instances, rect_ranges)
    }

    fn build_path_geometry(
        &mut self,
    ) -> (
        Vec<PathVertex>,
        Vec<u32>,
        [PrimitiveRange; RenderLayer::ALL.len()],
        usize,
    ) {
        let mut path_vertices = Vec::new();
        let mut path_indices = Vec::new();
        let mut path_ranges = [PrimitiveRange::default(); RenderLayer::ALL.len()];
        let mut tessellation_count = 0usize;

        for layer in RenderLayer::ALL {
            let start = path_indices.len() as u32;
            for cmd in self
                .draw_list
                .paths
                .iter()
                .filter(|cmd| cmd.layer() == layer)
            {
                if cmd.verbs().is_empty() {
                    continue;
                }

                let (mesh, created) = self
                    .tessellation_cache
                    .get_or_insert(cmd, self.scale_factor);
                if created {
                    tessellation_count += 1;
                }
                append_cached_path_mesh(&mut path_vertices, &mut path_indices, mesh);
            }
            path_ranges[layer.index()] =
                PrimitiveRange::new(start, path_indices.len() as u32 - start);
        }

        debug_assert!(
            path_vertices.len() * size_of::<PathVertex>() <= MAX_PATH_VERTEX_BYTES,
            "path vertex buffer exceeded the M0 1MB budget"
        );

        (path_vertices, path_indices, path_ranges, tessellation_count)
    }

    fn build_image_instances(
        &mut self,
    ) -> (
        Vec<ImageInstance>,
        [Vec<ImageBatch>; RenderLayer::ALL.len()],
        usize,
    ) {
        let mut image_instances = Vec::new();
        let mut image_batches = std::array::from_fn(|_| Vec::<ImageBatch>::new());
        let mut image_uploads = 0usize;

        for layer in RenderLayer::ALL {
            let layer_batches = &mut image_batches[layer.index()];
            for cmd in self
                .draw_list
                .images
                .iter()
                .filter(|cmd| cmd.layer() == layer)
            {
                if self.image_cache.get_or_insert(
                    cmd.data(),
                    &self.device,
                    &self.queue,
                    &self.image_bind_group_layout,
                    &self.screen_buffer,
                    &self.image_sampler,
                ) {
                    image_uploads += 1;
                }

                let content_hash = cmd.data().content_hash();
                let instance_start = image_instances.len() as u32;
                image_instances.push(ImageInstance::from_image(cmd, self.scale_factor));
                extend_or_start_image_batch(layer_batches, content_hash, instance_start);
            }
        }

        (image_instances, image_batches, image_uploads)
    }
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
