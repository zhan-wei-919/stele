//! High-level renderer state and lifecycle orchestration for the M0 prototype.

mod bind_group;
mod buffer;
mod frame;
mod rebuild;

use std::mem::size_of;

use bytemuck::cast_slice;
use winit::dpi::PhysicalSize;

use crate::font::FreeTypeRasterizer;

use super::atlas::GlyphAtlas;
use super::draw_list::{DrawList, DrawListOp};
use super::instance::{GlyphInstance, RectInstance};
use super::pipeline::{
    create_glyph_pipeline, create_rect_pipeline, create_screen_size_bind_group, screen_uniform,
};
use bind_group::{create_glyph_bind_group, create_glyph_bind_group_layout};
use buffer::create_vertex_buffer;

const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// GPU-backed renderer that turns draw-list updates into wgpu command buffers.
pub struct Renderer<'window> {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'window>,
    glyph_pipeline: wgpu::RenderPipeline,
    rect_pipeline: wgpu::RenderPipeline,
    atlas: GlyphAtlas,
    draw_list: DrawList,
    instance_buffer: wgpu::Buffer,
    dirty: bool,
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
    /// Creates a renderer for the current window surface and display scale factor.
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

    /// Applies upstream draw-list mutations and marks GPU buffers dirty.
    pub fn apply_ops(&mut self, ops: impl IntoIterator<Item = DrawListOp>) {
        self.draw_list.apply_ops(ops);
        self.dirty = true;
    }

    /// Updates surface and atlas state after a window resize or scale change.
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
            self.refresh_glyph_bind_group();
        }

        self.dirty = true;
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
