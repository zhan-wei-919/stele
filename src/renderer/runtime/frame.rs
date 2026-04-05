//! Per-frame surface acquisition and render-pass recording.

use std::time::Instant;

use log::{info, warn};

use super::Renderer;
use crate::renderer::draw_list::RenderLayer;

impl<'window> Renderer<'window> {
    /// Renders the current draw list into the swapchain surface.
    pub fn frame(&mut self) {
        if self.surface_config.width == 0 || self.surface_config.height == 0 {
            return;
        }

        let frame_start = Instant::now();
        self.tessellation_cache.begin_frame();
        self.image_cache.begin_frame();
        if self.dirty {
            self.rebuild_gpu_data();
        }

        let Some((surface_texture, suboptimal)) = self.acquire_surface_texture() else {
            return;
        };
        let draw_calls = self.render_surface_texture(&surface_texture);
        surface_texture.present();

        if suboptimal {
            self.reconfigure_surface();
        }

        info!("frame.draw_calls count={draw_calls}");
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

    fn render_surface_texture(&self, surface_texture: &wgpu::SurfaceTexture) -> u32 {
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
            let draw_calls = self.record_passes(&mut pass);
            drop(pass);
            self.queue.submit(Some(encoder.finish()));
            return draw_calls;
        }
    }

    fn record_passes<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) -> u32 {
        let mut draw_calls = 0;
        for layer in RenderLayer::ALL {
            draw_calls += self.draw_rects_in_layer(pass, layer);
            draw_calls += self.draw_paths_in_layer(pass, layer);
            draw_calls += self.draw_images_in_layer(pass, layer);
            draw_calls += self.draw_glyphs_in_layer(pass, layer);
        }
        draw_calls
    }

    fn draw_rects_in_layer<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        layer: RenderLayer,
    ) -> u32 {
        let range = self.rect_ranges_by_layer[layer.index()];
        if range.count == 0 {
            return 0;
        }

        pass.set_pipeline(&self.rect_pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(
            0,
            self.rect_buffer
                .slice(..self.rect_slice_end(self.rect_instance_count)),
        );
        pass.draw(0..6, range.start..range.end());
        1
    }

    fn draw_paths_in_layer<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        layer: RenderLayer,
    ) -> u32 {
        let range = self.path_ranges_by_layer[layer.index()];
        if range.count == 0 {
            return 0;
        }

        pass.set_pipeline(&self.path_pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(
            0,
            self.path_vertex_buffer
                .slice(..self.path_vertex_slice_end(self.path_vertex_count)),
        );
        pass.set_index_buffer(
            self.path_index_buffer
                .slice(..self.path_index_slice_end(self.path_index_count)),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(range.start..range.end(), 0, 0..1);
        1
    }

    fn draw_images_in_layer<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        layer: RenderLayer,
    ) -> u32 {
        if self.image_instance_count == 0 {
            return 0;
        }

        let mut draw_calls = 0;
        for batch in &self.image_batches_by_layer[layer.index()] {
            let Some(image) = self.image_cache.get(batch.content_hash) else {
                debug_assert!(false, "image batch referenced a missing cached texture");
                continue;
            };

            pass.set_pipeline(&self.image_pipeline);
            pass.set_bind_group(0, &image.bind_group, &[]);
            pass.set_vertex_buffer(
                0,
                self.image_instance_buffer
                    .slice(..self.image_slice_end(self.image_instance_count)),
            );
            pass.draw(0..6, batch.range.start..batch.range.end());
            draw_calls += 1;
        }
        draw_calls
    }

    fn draw_glyphs_in_layer<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        layer: RenderLayer,
    ) -> u32 {
        let range = self.glyph_ranges_by_layer[layer.index()];
        if range.count == 0 {
            return 0;
        }

        pass.set_pipeline(&self.glyph_pipeline);
        pass.set_bind_group(0, &self.glyph_bind_group, &[]);
        pass.set_vertex_buffer(
            0,
            self.instance_buffer
                .slice(..self.glyph_slice_end(self.glyph_instance_count)),
        );
        pass.draw(0..6, range.start..range.end());
        1
    }
}
