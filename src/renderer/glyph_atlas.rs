use std::collections::HashMap;

use log::info;

use crate::{FreeTypeRasterizer, GlyphKey, RasterizedGlyph, SubpixelLayout};

const DEFAULT_ATLAS_SIZE: u32 = 2048;
const MAX_ATLAS_SIZE: u32 = 8192;

#[derive(Clone, Copy, Debug, Default)]
pub struct AtlasRegion {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [f32; 2],
    pub bearing: [f32; 2],
}

#[derive(Clone, Debug, Default)]
pub struct Shelf {
    pub y_offset: u32,
    pub height: u32,
    pub x_cursor: u32,
}

#[derive(Clone, Debug)]
pub struct ShelfPacker {
    pub shelves: Vec<Shelf>,
    pub atlas_width: u32,
    pub atlas_height: u32,
}

impl ShelfPacker {
    pub fn new(atlas_width: u32, atlas_height: u32) -> Self {
        Self {
            shelves: Vec::new(),
            atlas_width,
            atlas_height,
        }
    }

    pub fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 || width > self.atlas_width || height > self.atlas_height {
            return None;
        }

        for shelf in &mut self.shelves {
            if height <= shelf.height && shelf.x_cursor + width <= self.atlas_width {
                let origin = (shelf.x_cursor, shelf.y_offset);
                shelf.x_cursor += width;
                return Some(origin);
            }
        }

        let y_offset = self
            .shelves
            .last()
            .map(|shelf| shelf.y_offset + shelf.height)
            .unwrap_or(0);
        if y_offset + height > self.atlas_height {
            return None;
        }

        self.shelves.push(Shelf {
            y_offset,
            height,
            x_cursor: width,
        });
        Some((0, y_offset))
    }
}

pub struct GlyphAtlas {
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub packer: ShelfPacker,
    pub cache: HashMap<GlyphKey, AtlasRegion>,
    pub current_size: u32,
}

impl GlyphAtlas {
    pub fn new(device: &wgpu::Device, initial_size: u32, format: wgpu::TextureFormat) -> Self {
        let current_size = initial_size.max(DEFAULT_ATLAS_SIZE).next_power_of_two();
        let (texture, view, sampler) = Self::create_gpu_resources(device, current_size, format);
        Self {
            device: device.clone(),
            format,
            texture,
            view,
            sampler,
            packer: ShelfPacker::new(current_size, current_size),
            cache: HashMap::new(),
            current_size,
        }
    }

    pub fn get_or_insert(
        &mut self,
        key: GlyphKey,
        queue: &wgpu::Queue,
        rasterizer: &FreeTypeRasterizer,
    ) -> AtlasRegion {
        if let Some(region) = self.cache.get(&key).copied() {
            return region;
        }

        let rasterized = rasterizer.rasterize_lcd(key);
        self.insert_rasterized(key, &rasterized, queue, rasterizer)
    }

    pub fn grow_and_repack(&mut self, queue: &wgpu::Queue, rasterizer: &FreeTypeRasterizer) {
        let keys = self.cache.keys().copied().collect::<Vec<_>>();
        let mut new_size = self.current_size.saturating_mul(2);

        loop {
            assert!(
                new_size <= MAX_ATLAS_SIZE,
                "atlas exceeded maximum size {MAX_ATLAS_SIZE}"
            );

            let (texture, view, sampler) =
                Self::create_gpu_resources(&self.device, new_size, self.format);
            let mut packer = ShelfPacker::new(new_size, new_size);
            let mut cache = HashMap::with_capacity(keys.len());
            let mut failed = false;

            for key in &keys {
                let rasterized = rasterizer.rasterize_lcd(*key);
                let upload =
                    AtlasUpload::from_rasterized(&rasterized, rasterizer.subpixel_layout());
                let region = if upload.width == 0 || upload.height == 0 {
                    Self::empty_region(&rasterized)
                } else if let Some(origin) = packer.allocate(upload.width, upload.height) {
                    Self::write_texture(&texture, queue, origin, &upload);
                    Self::region_for(origin, &upload, new_size)
                } else {
                    failed = true;
                    break;
                };
                cache.insert(*key, region);
            }

            if !failed {
                info!(
                    "atlas.repack old_size={} new_size={new_size}",
                    self.current_size
                );
                self.texture = texture;
                self.view = view;
                self.sampler = sampler;
                self.packer = packer;
                self.cache = cache;
                self.current_size = new_size;
                return;
            }

            new_size = new_size.saturating_mul(2);
        }
    }

    fn insert_rasterized(
        &mut self,
        key: GlyphKey,
        rasterized: &RasterizedGlyph,
        queue: &wgpu::Queue,
        rasterizer: &FreeTypeRasterizer,
    ) -> AtlasRegion {
        let upload = AtlasUpload::from_rasterized(rasterized, rasterizer.subpixel_layout());
        if upload.width == 0 || upload.height == 0 {
            let region = Self::empty_region(rasterized);
            self.cache.insert(key, region);
            return region;
        }

        loop {
            if let Some(origin) = self.packer.allocate(upload.width, upload.height) {
                Self::write_texture(&self.texture, queue, origin, &upload);
                let region = Self::region_for(origin, &upload, self.current_size);
                self.cache.insert(key, region);
                return region;
            }

            self.grow_and_repack(queue, rasterizer);
        }
    }

    fn region_for(origin: (u32, u32), upload: &AtlasUpload, atlas_size: u32) -> AtlasRegion {
        let inv_size = 1.0 / atlas_size as f32;
        AtlasRegion {
            uv_min: [origin.0 as f32 * inv_size, origin.1 as f32 * inv_size],
            uv_max: [
                (origin.0 + upload.width) as f32 * inv_size,
                (origin.1 + upload.height) as f32 * inv_size,
            ],
            size: [upload.width as f32, upload.height as f32],
            bearing: upload.bearing,
        }
    }

    fn empty_region(rasterized: &RasterizedGlyph) -> AtlasRegion {
        AtlasRegion {
            uv_min: [0.0, 0.0],
            uv_max: [0.0, 0.0],
            size: [0.0, 0.0],
            bearing: [rasterized.bearing_x as f32, rasterized.bearing_y as f32],
        }
    }

    fn create_gpu_resources(
        device: &wgpu::Device,
        size: u32,
        format: wgpu::TextureFormat,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::Sampler) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("stele.glyph_atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("stele.glyph_atlas_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        (texture, view, sampler)
    }

    fn write_texture(
        texture: &wgpu::Texture,
        queue: &wgpu::Queue,
        origin: (u32, u32),
        upload: &AtlasUpload,
    ) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin.0,
                    y: origin.1,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &upload.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(upload.width * 4),
                rows_per_image: Some(upload.height),
            },
            wgpu::Extent3d {
                width: upload.width,
                height: upload.height,
                depth_or_array_layers: 1,
            },
        );
    }
}

struct AtlasUpload {
    width: u32,
    height: u32,
    bearing: [f32; 2],
    rgba: Vec<u8>,
}

impl AtlasUpload {
    fn from_rasterized(rasterized: &RasterizedGlyph, layout: SubpixelLayout) -> Self {
        let bearing = [rasterized.bearing_x as f32, rasterized.bearing_y as f32];
        if rasterized.width == 0 || rasterized.height == 0 || rasterized.data.is_empty() {
            return Self {
                width: 0,
                height: 0,
                bearing,
                rgba: Vec::new(),
            };
        }

        let (width, height, rgba) = match layout {
            SubpixelLayout::HorizontalRgb | SubpixelLayout::HorizontalBgr => {
                horizontal_lcd_to_rgba(rasterized)
            }
            SubpixelLayout::VerticalRgb | SubpixelLayout::VerticalBgr => {
                vertical_lcd_to_rgba(rasterized)
            }
            SubpixelLayout::None => grayscale_to_rgba(rasterized),
        };

        Self {
            width,
            height,
            bearing,
            rgba,
        }
    }
}

fn horizontal_lcd_to_rgba(rasterized: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
    debug_assert_eq!(rasterized.width % 3, 0);
    let width = rasterized.width / 3;
    let height = rasterized.height;
    let src_row_width = rasterized.width as usize;
    let dst_row_width = width as usize;
    let mut rgba = vec![0; dst_row_width * height as usize * 4];

    for y in 0..height as usize {
        let src_row = &rasterized.data[y * src_row_width..(y + 1) * src_row_width];
        for x in 0..dst_row_width {
            let src = x * 3;
            let dst = (y * dst_row_width + x) * 4;
            rgba[dst..dst + 4].copy_from_slice(&[
                src_row[src],
                src_row[src + 1],
                src_row[src + 2],
                255,
            ]);
        }
    }

    (width, height, rgba)
}

fn vertical_lcd_to_rgba(rasterized: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
    debug_assert_eq!(rasterized.height % 3, 0);
    let width = rasterized.width;
    let height = rasterized.height / 3;
    let src_row_width = width as usize;
    let dst_row_width = width as usize;
    let mut rgba = vec![0; dst_row_width * height as usize * 4];

    for y in 0..height as usize {
        for x in 0..dst_row_width {
            let src_r = (y * 3) * src_row_width + x;
            let src_g = (y * 3 + 1) * src_row_width + x;
            let src_b = (y * 3 + 2) * src_row_width + x;
            let dst = (y * dst_row_width + x) * 4;
            rgba[dst..dst + 4].copy_from_slice(&[
                rasterized.data[src_r],
                rasterized.data[src_g],
                rasterized.data[src_b],
                255,
            ]);
        }
    }

    (width, height, rgba)
}

fn grayscale_to_rgba(rasterized: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
    let width = rasterized.width;
    let height = rasterized.height;
    let mut rgba = vec![0; width as usize * height as usize * 4];

    for (index, coverage) in rasterized.data.iter().copied().enumerate() {
        let dst = index * 4;
        rgba[dst..dst + 4].copy_from_slice(&[coverage, coverage, coverage, 255]);
    }

    (width, height, rgba)
}
