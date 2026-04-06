//! Renderer struct definition and core lifecycle methods.

use std::mem::size_of;

use bytemuck::cast_slice;
use winit::dpi::PhysicalSize;

use crate::font::FreeTypeRasterizer;

use super::super::atlas::GlyphAtlas;
use super::super::draw_list::{ClipRect, DrawList, DrawListOp, RenderLayer};
use super::super::image_cache::ImageCache;
use super::super::instance::{GlyphInstance, ImageInstance, PathVertex, RectInstance};
use super::super::pipeline::{
    create_glyph_pipeline, create_image_bind_group_layout, create_image_pipeline,
    create_image_sampler, create_path_pipeline, create_rect_pipeline,
    create_screen_size_bind_group, create_screen_size_bind_group_layout, screen_uniform,
};
use super::super::tessellation::TessellationCache;
use super::bind_group::{create_glyph_bind_group, create_glyph_bind_group_layout};
use super::buffer::{create_index_buffer, create_vertex_buffer};

const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const LAYER_COUNT: usize = RenderLayer::ALL.len();

/// A contiguous slice inside one GPU buffer for a single primitive family.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PrimitiveRange {
    pub start: u32,
    pub count: u32,
}

impl PrimitiveRange {
    /// Creates a range from an inclusive start and element count.
    pub const fn new(start: u32, count: u32) -> Self {
        Self { start, count }
    }

    /// Returns the exclusive end of the range.
    pub const fn end(self) -> u32 {
        self.start + self.count
    }
}

/// A contiguous image instance batch that can reuse one bind group.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ImageBatch {
    pub content_hash: u64,
    pub range: PrimitiveRange,
}

/// GPU ranges and image batches belonging to one block submission unit.
#[derive(Clone, Debug)]
pub(super) struct BlockGpuBatch {
    pub clip_rect: Option<ClipRect>,
    pub rect_ranges_by_layer: [PrimitiveRange; LAYER_COUNT],
    pub path_ranges_by_layer: [PrimitiveRange; LAYER_COUNT],
    pub glyph_ranges_by_layer: [PrimitiveRange; LAYER_COUNT],
    pub image_batches_by_layer: [Vec<ImageBatch>; LAYER_COUNT],
}

impl BlockGpuBatch {
    /// Creates an empty GPU batch placeholder for one block.
    pub fn empty(clip_rect: Option<ClipRect>) -> Self {
        Self {
            clip_rect,
            rect_ranges_by_layer: [PrimitiveRange::default(); LAYER_COUNT],
            path_ranges_by_layer: [PrimitiveRange::default(); LAYER_COUNT],
            glyph_ranges_by_layer: [PrimitiveRange::default(); LAYER_COUNT],
            image_batches_by_layer: std::array::from_fn(|_| Vec::new()),
        }
    }
}

/// GPU-backed renderer that turns draw-list updates into wgpu command buffers.
pub struct Renderer<'window> {
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) surface: wgpu::Surface<'window>,
    pub(super) glyph_pipeline: wgpu::RenderPipeline,
    pub(super) rect_pipeline: wgpu::RenderPipeline,
    pub(super) path_pipeline: wgpu::RenderPipeline,
    pub(super) image_pipeline: wgpu::RenderPipeline,
    pub(super) atlas: GlyphAtlas,
    pub(super) draw_list: DrawList,
    pub(super) instance_buffer: wgpu::Buffer,
    pub(super) path_vertex_buffer: wgpu::Buffer,
    pub(super) path_index_buffer: wgpu::Buffer,
    pub(super) image_instance_buffer: wgpu::Buffer,
    pub(super) dirty: bool,
    pub(super) surface_config: wgpu::SurfaceConfiguration,
    pub(super) rasterizer: FreeTypeRasterizer,
    pub(super) scale_factor: f32,
    pub(super) screen_bind_group: wgpu::BindGroup,
    pub(super) screen_buffer: wgpu::Buffer,
    pub(super) glyph_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) glyph_bind_group: wgpu::BindGroup,
    pub(super) image_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) image_sampler: wgpu::Sampler,
    pub(super) rect_buffer: wgpu::Buffer,
    pub(super) tessellation_cache: TessellationCache,
    pub(super) image_cache: ImageCache,
    pub(super) instance_capacity: usize,
    pub(super) rect_capacity: usize,
    pub(super) path_vertex_capacity: usize,
    pub(super) path_index_capacity: usize,
    pub(super) image_instance_capacity: usize,
    pub(super) glyph_instance_count: u32,
    pub(super) rect_instance_count: u32,
    pub(super) path_vertex_count: u32,
    pub(super) path_index_count: u32,
    pub(super) image_instance_count: u32,
    pub(super) block_batches: Vec<BlockGpuBatch>,
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
        let screen_bind_group_layout = create_screen_size_bind_group_layout(&device);
        let glyph_bind_group_layout = create_glyph_bind_group_layout(&device);
        let glyph_pipeline =
            create_glyph_pipeline(&device, surface_config.format, &glyph_bind_group_layout);
        let rect_pipeline =
            create_rect_pipeline(&device, surface_config.format, &screen_bind_group_layout);
        let path_pipeline =
            create_path_pipeline(&device, surface_config.format, &screen_bind_group_layout);
        let image_bind_group_layout = create_image_bind_group_layout(&device);
        let image_pipeline =
            create_image_pipeline(&device, surface_config.format, &image_bind_group_layout);
        let (screen_bind_group, screen_buffer) = create_screen_size_bind_group(
            &device,
            &screen_bind_group_layout,
            surface_config.width,
            surface_config.height,
        );
        let image_sampler = create_image_sampler(&device);
        let glyph_bind_group =
            create_glyph_bind_group(&device, &glyph_bind_group_layout, &screen_buffer, &atlas);

        Self {
            device: device.clone(),
            queue,
            surface,
            glyph_pipeline,
            rect_pipeline,
            path_pipeline,
            image_pipeline,
            atlas,
            draw_list: DrawList::default(),
            instance_buffer: create_vertex_buffer::<GlyphInstance>(
                &device,
                1,
                "stele.glyph_instances",
            ),
            path_vertex_buffer: create_vertex_buffer::<PathVertex>(
                &device,
                1,
                "stele.path_vertices",
            ),
            path_index_buffer: create_index_buffer::<u32>(&device, 1, "stele.path_indices"),
            image_instance_buffer: create_vertex_buffer::<ImageInstance>(
                &device,
                1,
                "stele.image_instances",
            ),
            dirty: false,
            surface_config,
            rasterizer,
            scale_factor,
            screen_bind_group,
            screen_buffer,
            glyph_bind_group_layout,
            glyph_bind_group,
            image_bind_group_layout,
            image_sampler,
            rect_buffer: create_vertex_buffer::<RectInstance>(&device, 1, "stele.rect_instances"),
            tessellation_cache: TessellationCache::default(),
            image_cache: ImageCache::default(),
            instance_capacity: 1,
            rect_capacity: 1,
            path_vertex_capacity: 1,
            path_index_capacity: 1,
            image_instance_capacity: 1,
            glyph_instance_count: 0,
            rect_instance_count: 0,
            path_vertex_count: 0,
            path_index_count: 0,
            image_instance_count: 0,
            block_batches: Vec::new(),
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
            self.tessellation_cache.clear();
            self.refresh_glyph_bind_group();
        }

        self.dirty = true;
    }

    pub(super) fn reconfigure_surface(&self) {
        if self.surface_config.width == 0 || self.surface_config.height == 0 {
            return;
        }
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub(super) fn glyph_slice_end(&self, count: u32) -> wgpu::BufferAddress {
        count as wgpu::BufferAddress * size_of::<GlyphInstance>() as wgpu::BufferAddress
    }

    pub(super) fn rect_slice_end(&self, count: u32) -> wgpu::BufferAddress {
        count as wgpu::BufferAddress * size_of::<RectInstance>() as wgpu::BufferAddress
    }

    pub(super) fn path_vertex_slice_end(&self, count: u32) -> wgpu::BufferAddress {
        count as wgpu::BufferAddress * size_of::<PathVertex>() as wgpu::BufferAddress
    }

    pub(super) fn path_index_slice_end(&self, count: u32) -> wgpu::BufferAddress {
        count as wgpu::BufferAddress * size_of::<u32>() as wgpu::BufferAddress
    }

    pub(super) fn image_slice_end(&self, count: u32) -> wgpu::BufferAddress {
        count as wgpu::BufferAddress * size_of::<ImageInstance>() as wgpu::BufferAddress
    }

    pub(super) fn viewport_clip_rect(&self) -> ClipRect {
        ClipRect::new(
            0.0,
            0.0,
            self.surface_config.width as f32 / self.scale_factor.max(1.0),
            self.surface_config.height as f32 / self.scale_factor.max(1.0),
        )
    }
}
