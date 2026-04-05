//! GPU buffer growth, uploads, and bind-group refresh for rebuilt draw data.

use bytemuck::cast_slice;
use log::info;

use super::super::bind_group::create_glyph_bind_group;
use super::super::buffer::{ensure_index_capacity, ensure_vertex_capacity};
use super::super::state::Renderer;
use crate::renderer::instance::{GlyphInstance, ImageInstance, PathVertex, RectInstance};

impl<'window> Renderer<'window> {
    pub(super) fn ensure_instance_capacity(
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

    pub(super) fn upload_glyph_instances(&mut self, glyph_instances: &[GlyphInstance]) {
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

    pub(super) fn upload_rect_instances(&mut self, rect_instances: &[RectInstance]) {
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

    pub(super) fn upload_path_geometry(
        &mut self,
        path_vertices: &[PathVertex],
        path_indices: &[u32],
    ) {
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

    pub(super) fn upload_image_instances(&mut self, image_instances: &[ImageInstance]) {
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

    pub(in crate::renderer::runtime) fn refresh_glyph_bind_group(&mut self) {
        self.glyph_bind_group = create_glyph_bind_group(
            &self.device,
            &self.glyph_bind_group_layout,
            &self.screen_buffer,
            &self.atlas,
        );
    }
}
