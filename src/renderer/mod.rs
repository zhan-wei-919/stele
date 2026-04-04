use std::{mem::size_of, time::Instant};

use bytemuck::cast_slice;
use log::{info, warn};
use winit::dpi::PhysicalSize;

use crate::font::FreeTypeRasterizer;

pub mod draw_list;
pub mod glyph_atlas;
pub mod instance_buffer;
pub mod pipeline;
pub mod subpixel;

pub use draw_list::{DrawList, DrawListOp, GlyphKey, PositionedGlyph, RectCmd, SubpixelBin};
pub use glyph_atlas::{AtlasRegion, GlyphAtlas, Shelf, ShelfPacker};
pub use instance_buffer::{
    glyph_instance_layout, rect_instance_layout, GlyphInstance, RectInstance,
};
pub use pipeline::{
    create_glyph_pipeline, create_rect_pipeline, create_screen_size_bind_group,
    create_screen_size_bind_group_layout, screen_uniform,
};

const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct Renderer<'window> {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'window>,
    pub glyph_pipeline: wgpu::RenderPipeline,
    pub rect_pipeline: wgpu::RenderPipeline,
    pub atlas: GlyphAtlas,
    pub draw_list: DrawList,
    pub instance_buffer: wgpu::Buffer,
    pub dirty: bool,
    surface_config: wgpu::SurfaceConfiguration,
    rasterizer: FreeTypeRasterizer,
    scale_factor: f32,
    screen_bind_group: wgpu::BindGroup,
    screen_buffer: wgpu::Buffer,
    glyph_bind_group_layout: wgpu::BindGroupLayout,
    glyph_bind_group: wgpu::BindGroup,
    rect_buffer: wgpu::Buffer,
    instance_capacity: usize,
    rect_capacity: usize,
    glyph_instance_count: u32,
    rect_instance_count: u32,
    cursor_instance_count: u32,
}

impl<'window> Renderer<'window> {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'window>,
        mut surface_config: wgpu::SurfaceConfiguration,
        rasterizer: FreeTypeRasterizer,
        scale_factor: f32,
    ) -> Self {
        surface_config.width = surface_config.width.max(1);
        surface_config.height = surface_config.height.max(1);
        surface.configure(&device, &surface_config);

        let atlas = GlyphAtlas::new(&device, 2048, ATLAS_FORMAT);
        let glyph_bind_group_layout = create_glyph_bind_group_layout(&device);
        let glyph_pipeline =
            create_glyph_pipeline(&device, surface_config.format, &glyph_bind_group_layout);
        let rect_pipeline = create_rect_pipeline(&device, surface_config.format);
        let rect_layout = rect_pipeline.get_bind_group_layout(0);
        let (screen_bind_group, screen_buffer) = create_screen_size_bind_group(
            &device,
            &rect_layout,
            surface_config.width,
            surface_config.height,
        );
        let glyph_bind_group =
            create_glyph_bind_group(&device, &glyph_bind_group_layout, &screen_buffer, &atlas);

        Self {
            device: device.clone(),
            queue,
            surface,
            glyph_pipeline,
            rect_pipeline,
            atlas,
            draw_list: DrawList::default(),
            instance_buffer: create_vertex_buffer::<GlyphInstance>(
                &device,
                1,
                "stele.glyph_instances",
            ),
            dirty: false,
            surface_config,
            rasterizer,
            scale_factor,
            screen_bind_group,
            screen_buffer,
            glyph_bind_group_layout,
            glyph_bind_group,
            rect_buffer: create_vertex_buffer::<RectInstance>(&device, 1, "stele.rect_instances"),
            instance_capacity: 1,
            rect_capacity: 1,
            glyph_instance_count: 0,
            rect_instance_count: 0,
            cursor_instance_count: 0,
        }
    }

    pub fn apply_ops(&mut self, ops: impl IntoIterator<Item = DrawListOp>) {
        self.draw_list.apply_ops(ops);
        self.dirty = true;
    }

    pub fn frame(&mut self) {
        if self.surface_config.width == 0 || self.surface_config.height == 0 {
            return;
        }

        let frame_start = Instant::now();
        if self.dirty {
            self.rebuild_gpu_data();
        }

        let (surface_texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.reconfigure_surface();
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                warn!("frame.surface status=validation");
                return;
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("stele.frame_encoder"),
            });
        let color_attachment = Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stele.render_pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.rect_pipeline);
            pass.set_bind_group(0, &self.screen_bind_group, &[]);
            if self.rect_instance_count > 0 {
                let end = self.rect_slice_end(self.rect_instance_count);
                pass.set_vertex_buffer(0, self.rect_buffer.slice(..end));
                pass.draw(0..6, 0..self.rect_instance_count);
            }

            pass.set_pipeline(&self.glyph_pipeline);
            pass.set_bind_group(0, &self.glyph_bind_group, &[]);
            if self.glyph_instance_count > 0 {
                let end = self.glyph_slice_end(self.glyph_instance_count);
                pass.set_vertex_buffer(0, self.instance_buffer.slice(..end));
                pass.draw(0..6, 0..self.glyph_instance_count);
            }

            if self.cursor_instance_count > 0 {
                let start = self.rect_slice_end(self.rect_instance_count);
                let end =
                    self.rect_slice_end(self.rect_instance_count + self.cursor_instance_count);
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_bind_group(0, &self.screen_bind_group, &[]);
                pass.set_vertex_buffer(0, self.rect_buffer.slice(start..end));
                pass.draw(0..6, 0..self.cursor_instance_count);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();
        if suboptimal {
            self.reconfigure_surface();
        }

        info!("frame.glyph_count count={}", self.glyph_instance_count);
        info!("frame.time_us value={}", frame_start.elapsed().as_micros());
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>, scale_factor: f32) {
        let scale_factor_changed = self.scale_factor.to_bits() != scale_factor.to_bits();
        self.scale_factor = scale_factor;
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;

        if new_size.width > 0 && new_size.height > 0 {
            self.reconfigure_surface();
        }
        self.queue.write_buffer(
            &self.screen_buffer,
            0,
            cast_slice(&[screen_uniform(
                new_size.width.max(1),
                new_size.height.max(1),
            )]),
        );

        if scale_factor_changed {
            self.atlas = GlyphAtlas::new(&self.device, self.atlas.current_size, ATLAS_FORMAT);
            self.glyph_bind_group = create_glyph_bind_group(
                &self.device,
                &self.glyph_bind_group_layout,
                &self.screen_buffer,
                &self.atlas,
            );
        }

        self.dirty = true;
    }

    fn rebuild_gpu_data(&mut self) {
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

        ensure_vertex_capacity::<GlyphInstance>(
            &self.device,
            glyph_instances.len(),
            &mut self.instance_buffer,
            &mut self.instance_capacity,
            "stele.glyph_instances",
        );
        ensure_vertex_capacity::<RectInstance>(
            &self.device,
            rect_instances.len(),
            &mut self.rect_buffer,
            &mut self.rect_capacity,
            "stele.rect_instances",
        );

        if !glyph_instances.is_empty() {
            info!(
                "frame.write_buffer target=glyph_instances count={}",
                glyph_instances.len()
            );
            self.queue
                .write_buffer(&self.instance_buffer, 0, cast_slice(&glyph_instances));
        }
        if !rect_instances.is_empty() {
            info!(
                "frame.write_buffer target=rect_instances count={}",
                rect_instances.len()
            );
            self.queue
                .write_buffer(&self.rect_buffer, 0, cast_slice(&rect_instances));
        }

        self.glyph_instance_count = glyph_instances.len() as u32;
        self.glyph_bind_group = create_glyph_bind_group(
            &self.device,
            &self.glyph_bind_group_layout,
            &self.screen_buffer,
            &self.atlas,
        );
        self.dirty = false;

        if atlas_uploads > 0 {
            info!("frame.write_texture target=atlas count={atlas_uploads}");
        }
    }

    fn reconfigure_surface(&self) {
        if self.surface_config.width == 0 || self.surface_config.height == 0 {
            return;
        }
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn glyph_slice_end(&self, count: u32) -> wgpu::BufferAddress {
        count as wgpu::BufferAddress * size_of::<GlyphInstance>() as wgpu::BufferAddress
    }

    fn rect_slice_end(&self, count: u32) -> wgpu::BufferAddress {
        count as wgpu::BufferAddress * size_of::<RectInstance>() as wgpu::BufferAddress
    }
}

fn create_glyph_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("stele.glyph_bind_group_layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn create_glyph_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    screen_buffer: &wgpu::Buffer,
    atlas: &GlyphAtlas,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("stele.glyph_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&atlas.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&atlas.sampler),
            },
        ],
    })
}

fn create_vertex_buffer<T>(device: &wgpu::Device, capacity: usize, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (capacity.max(1) * size_of::<T>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn ensure_vertex_capacity<T>(
    device: &wgpu::Device,
    required: usize,
    buffer: &mut wgpu::Buffer,
    capacity: &mut usize,
    label: &str,
) {
    if required <= *capacity {
        return;
    }

    *capacity = required.max(1).next_power_of_two();
    *buffer = create_vertex_buffer::<T>(device, *capacity, label);
}
