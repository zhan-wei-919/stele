//! Per-frame surface acquisition and render-pass recording.

use std::time::Instant;

use log::{info, warn};

use super::Renderer;

impl<'window> Renderer<'window> {
    /// Renders the current draw list into the swapchain surface.
    pub fn frame(&mut self) {
        if self.surface_config.width == 0 || self.surface_config.height == 0 {
            return;
        }

        let frame_start = Instant::now();
        if self.dirty {
            self.rebuild_gpu_data();
        }

        let Some((surface_texture, suboptimal)) = self.acquire_surface_texture() else {
            return;
        };
        self.render_surface_texture(&surface_texture);
        surface_texture.present();

        if suboptimal {
            self.reconfigure_surface();
        }

        info!("frame.glyph_count count={}", self.glyph_instance_count);
        info!("frame.time_us value={}", frame_start.elapsed().as_micros());
    }

    fn acquire_surface_texture(&self) -> Option<(wgpu::SurfaceTexture, bool)> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => Some((texture, false)),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Some((texture, true)),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => None,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.reconfigure_surface();
                None
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                warn!("frame.surface status=validation");
                None
            }
        }
    }

    fn render_surface_texture(&self, surface_texture: &wgpu::SurfaceTexture) {
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("stele.frame_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stele.render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.record_passes(&mut pass);
        }

        self.queue.submit(Some(encoder.finish()));
    }

    fn record_passes<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        self.draw_rects(pass);
        self.draw_glyphs(pass);
        self.draw_cursor(pass);
    }

    fn draw_rects<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        pass.set_pipeline(&self.rect_pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        if self.rect_instance_count == 0 {
            return;
        }

        let end = self.rect_slice_end(self.rect_instance_count);
        pass.set_vertex_buffer(0, self.rect_buffer.slice(..end));
        pass.draw(0..6, 0..self.rect_instance_count);
    }

    fn draw_glyphs<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        pass.set_pipeline(&self.glyph_pipeline);
        pass.set_bind_group(0, &self.glyph_bind_group, &[]);
        if self.glyph_instance_count == 0 {
            return;
        }

        let end = self.glyph_slice_end(self.glyph_instance_count);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..end));
        pass.draw(0..6, 0..self.glyph_instance_count);
    }

    fn draw_cursor<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        if self.cursor_instance_count == 0 {
            return;
        }

        let start = self.rect_slice_end(self.rect_instance_count);
        let end = self.rect_slice_end(self.rect_instance_count + self.cursor_instance_count);
        pass.set_pipeline(&self.rect_pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.rect_buffer.slice(start..end));
        pass.draw(0..6, 0..self.cursor_instance_count);
    }
}
