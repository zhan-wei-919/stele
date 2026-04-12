//! GPU texture atlas owned by the renderer.

const DEFAULT_ATLAS_SIZE: u32 = 2048;

/// UVs, pixel size, and bearing for a glyph cached in the atlas.
#[derive(Clone, Copy, Debug, Default)]
pub struct AtlasRegion {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [f32; 2],
    pub bearing: [f32; 2],
}

/// GPU texture atlas that caches rasterized glyph bitmaps.
pub struct GlyphAtlas {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) current_size: u32,
}

impl GlyphAtlas {
    /// Creates an empty glyph atlas with the requested minimum texture size.
    pub fn new(device: &wgpu::Device, initial_size: u32, format: wgpu::TextureFormat) -> Self {
        let current_size = initial_size.max(DEFAULT_ATLAS_SIZE).next_power_of_two();
        let (texture, view, sampler) = Self::create_gpu_resources(device, current_size, format);
        Self {
            texture,
            view,
            sampler,
            current_size,
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

    /// Writes one RGBA patch into the existing atlas texture.
    pub(crate) fn write_region(&self, queue: &wgpu::Queue, region: AtlasRegion, rgba: &[u8]) {
        let width = region.size[0] as u32;
        let height = region.size[1] as u32;
        if width == 0 || height == 0 {
            return;
        }

        debug_assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "atlas patch pixels must match the target region size"
        );
        let origin = (
            (region.uv_min[0] * self.current_size as f32).round() as u32,
            (region.uv_min[1] * self.current_size as f32).round() as u32,
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin.0,
                    y: origin.1,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}
