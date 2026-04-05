//! Dirty-path rebuilding of atlas-backed instance data.

use std::mem::size_of;

use bytemuck::cast_slice;
use log::{info, warn};

use super::bind_group::create_glyph_bind_group;
use super::buffer::{ensure_index_capacity, ensure_vertex_capacity};
use super::state::{ImageBatch, PrimitiveRange};
use super::Renderer;
use crate::renderer::draw_list::RenderLayer;
use crate::renderer::instance::{GlyphInstance, ImageInstance, PathVertex, RectInstance};

const CACHE_EVICTION_AGE: u64 = 120;
const MAX_PATH_VERTEX_BYTES: usize = 1024 * 1024;

impl<'window> Renderer<'window> {
    pub(super) fn rebuild_gpu_data(&mut self) {
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

    fn build_glyph_instances(&mut self) -> (Vec<GlyphInstance>, [PrimitiveRange; 4], usize) {
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

        let mut glyph_ranges = [PrimitiveRange::default(); 4];
        glyph_ranges[RenderLayer::Content.index()] =
            PrimitiveRange::new(0, glyph_instances.len() as u32);
        (glyph_instances, glyph_ranges, atlas_uploads)
    }

    fn build_rect_instances(&self) -> (Vec<RectInstance>, [PrimitiveRange; 4]) {
        let mut rect_instances = Vec::new();
        let mut rect_ranges = [PrimitiveRange::default(); 4];

        for layer in RenderLayer::ALL {
            let start = rect_instances.len() as u32;
            for cmd in self.draw_list.rects.iter().filter(|cmd| cmd.layer == layer) {
                if !cmd.is_valid() {
                    debug_assert!(
                        false,
                        "RectCmd size must stay positive and color normalized"
                    );
                    continue;
                }
                rect_instances.push(RectInstance::from_rect(*cmd, self.scale_factor));
            }
            rect_ranges[layer.index()] =
                PrimitiveRange::new(start, rect_instances.len() as u32 - start);
        }

        (rect_instances, rect_ranges)
    }

    fn build_path_geometry(&mut self) -> (Vec<PathVertex>, Vec<u32>, [PrimitiveRange; 4], usize) {
        let mut path_vertices = Vec::new();
        let mut path_indices = Vec::new();
        let mut path_ranges = [PrimitiveRange::default(); 4];
        let mut tessellation_count = 0usize;

        for layer in RenderLayer::ALL {
            let start = path_indices.len() as u32;
            for cmd in self.draw_list.paths.iter().filter(|cmd| cmd.layer == layer) {
                if cmd.verbs.is_empty() {
                    continue;
                }
                if !cmd.is_visible() {
                    debug_assert!(false, "PathCmd fill and stroke cannot both be None");
                    continue;
                }

                let (mesh, created) = self
                    .tessellation_cache
                    .get_or_insert(cmd, self.scale_factor);
                if created {
                    tessellation_count += 1;
                }
                let vertex_offset = path_vertices.len() as u32;
                path_vertices.extend_from_slice(&mesh.vertices);
                path_indices.extend(mesh.indices.iter().map(|index| index + vertex_offset));
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

    fn build_image_instances(&mut self) -> (Vec<ImageInstance>, [Vec<ImageBatch>; 4], usize) {
        let mut image_instances = Vec::new();
        let mut image_batches = std::array::from_fn(|_| Vec::<ImageBatch>::new());
        let mut image_uploads = 0usize;

        for layer in RenderLayer::ALL {
            let layer_batches = &mut image_batches[layer.index()];
            for cmd in self
                .draw_list
                .images
                .iter()
                .filter(|cmd| cmd.layer == layer)
            {
                if !cmd.is_valid() {
                    debug_assert!(false, "ImageCmd size and RGBA payload must stay valid");
                    continue;
                }
                if self.image_cache.get_or_insert(
                    &cmd.data,
                    &self.device,
                    &self.queue,
                    &self.image_bind_group_layout,
                    &self.screen_buffer,
                    &self.image_sampler,
                ) {
                    image_uploads += 1;
                }

                let content_hash = cmd.data.content_hash();
                let instance_start = image_instances.len() as u32;
                image_instances.push(ImageInstance::from_image(cmd, self.scale_factor));

                match layer_batches.last_mut() {
                    Some(batch)
                        if batch.content_hash == content_hash
                            && batch.range.end() == instance_start =>
                    {
                        batch.range.count += 1;
                    }
                    _ => layer_batches.push(ImageBatch {
                        content_hash,
                        range: PrimitiveRange::new(instance_start, 1),
                    }),
                }
            }
        }

        (image_instances, image_batches, image_uploads)
    }

    fn ensure_instance_capacity(
        &mut self,
        glyph_count: usize,
        rect_count: usize,
        path_vertex_count: usize,
        path_index_count: usize,
        image_count: usize,
    ) {
        ensure_vertex_capacity::<GlyphInstance>(
            &self.device,
            glyph_count,
            &mut self.instance_buffer,
            &mut self.instance_capacity,
            "stele.glyph_instances",
        );
        ensure_vertex_capacity::<RectInstance>(
            &self.device,
            rect_count,
            &mut self.rect_buffer,
            &mut self.rect_capacity,
            "stele.rect_instances",
        );
        ensure_vertex_capacity::<PathVertex>(
            &self.device,
            path_vertex_count,
            &mut self.path_vertex_buffer,
            &mut self.path_vertex_capacity,
            "stele.path_vertices",
        );
        ensure_index_capacity::<u32>(
            &self.device,
            path_index_count,
            &mut self.path_index_buffer,
            &mut self.path_index_capacity,
            "stele.path_indices",
        );
        ensure_vertex_capacity::<ImageInstance>(
            &self.device,
            image_count,
            &mut self.image_instance_buffer,
            &mut self.image_instance_capacity,
            "stele.image_instances",
        );
    }

    fn upload_glyph_instances(&mut self, glyph_instances: &[GlyphInstance]) {
        self.glyph_instance_count = glyph_instances.len() as u32;
        if glyph_instances.is_empty() {
            return;
        }

        info!(
            "frame.write_buffer target=glyph_instances count={}",
            glyph_instances.len()
        );
        self.queue
            .write_buffer(&self.instance_buffer, 0, cast_slice(glyph_instances));
    }

    fn upload_rect_instances(&mut self, rect_instances: &[RectInstance]) {
        self.rect_instance_count = rect_instances.len() as u32;
        if rect_instances.is_empty() {
            return;
        }

        info!(
            "frame.write_buffer target=rect_instances count={}",
            rect_instances.len()
        );
        self.queue
            .write_buffer(&self.rect_buffer, 0, cast_slice(rect_instances));
    }

    fn upload_path_geometry(&mut self, path_vertices: &[PathVertex], path_indices: &[u32]) {
        self.path_vertex_count = path_vertices.len() as u32;
        self.path_index_count = path_indices.len() as u32;
        if path_vertices.is_empty() || path_indices.is_empty() {
            return;
        }

        info!(
            "frame.write_buffer target=path_vertices count={}",
            path_vertices.len()
        );
        self.queue
            .write_buffer(&self.path_vertex_buffer, 0, cast_slice(path_vertices));
        info!(
            "frame.write_buffer target=path_indices count={}",
            path_indices.len()
        );
        self.queue
            .write_buffer(&self.path_index_buffer, 0, cast_slice(path_indices));
    }

    fn upload_image_instances(&mut self, image_instances: &[ImageInstance]) {
        self.image_instance_count = image_instances.len() as u32;
        if image_instances.is_empty() {
            return;
        }

        info!(
            "frame.write_buffer target=image_instances count={}",
            image_instances.len()
        );
        self.queue
            .write_buffer(&self.image_instance_buffer, 0, cast_slice(image_instances));
    }

    pub(super) fn refresh_glyph_bind_group(&mut self) {
        self.glyph_bind_group = create_glyph_bind_group(
            &self.device,
            &self.glyph_bind_group_layout,
            &self.screen_buffer,
            &self.atlas,
        );
    }
}
