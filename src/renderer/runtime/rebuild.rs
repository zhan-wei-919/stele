//! Dirty-path rebuilding of atlas-backed instance data.

use bytemuck::cast_slice;
use log::info;

use super::bind_group::create_glyph_bind_group;
use super::buffer::ensure_vertex_capacity;
use super::Renderer;
use crate::renderer::instance::{GlyphInstance, RectInstance};

impl<'window> Renderer<'window> {
    pub(super) fn rebuild_gpu_data(&mut self) {
        let (glyph_instances, atlas_uploads) = self.build_glyph_instances();
        let rect_instances = self.build_rect_instances();

        self.ensure_instance_capacity(glyph_instances.len(), rect_instances.len());
        self.upload_glyph_instances(&glyph_instances);
        self.upload_rect_instances(&rect_instances);
        self.refresh_glyph_bind_group();
        self.dirty = false;

        if atlas_uploads > 0 {
            info!("frame.write_texture target=atlas count={atlas_uploads}");
        }
    }

    fn build_glyph_instances(&mut self) -> (Vec<GlyphInstance>, usize) {
        let mut atlas_uploads = 0usize;
        let mut glyph_instances = Vec::new();

        for line in &self.draw_list.lines {
            for glyph in line {
                let key = glyph.glyph_key(self.scale_factor);
                let cached = self.atlas.cache.contains_key(&key);
                let region = self.atlas.get_or_insert(key, &self.queue, &self.rasterizer);
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

        (glyph_instances, atlas_uploads)
    }

    fn build_rect_instances(&mut self) -> Vec<RectInstance> {
        let mut rect_instances = self
            .draw_list
            .rects
            .iter()
            .copied()
            .map(RectInstance::from_rect)
            .collect::<Vec<_>>();
        self.rect_instance_count = rect_instances.len() as u32;

        let cursor = self.draw_list.cursor.map(RectInstance::from_rect);
        self.cursor_instance_count = u32::from(cursor.is_some());
        if let Some(cursor) = cursor {
            rect_instances.push(cursor);
        }

        rect_instances
    }

    fn ensure_instance_capacity(&mut self, glyph_count: usize, rect_count: usize) {
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

    pub(super) fn refresh_glyph_bind_group(&mut self) {
        self.glyph_bind_group = create_glyph_bind_group(
            &self.device,
            &self.glyph_bind_group_layout,
            &self.screen_buffer,
            &self.atlas,
        );
    }
}
