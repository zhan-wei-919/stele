//! Image upload cache keyed by immutable RGBA content hashes.

use std::collections::HashMap;

use crate::draw_list::ImageData;
use crate::renderer::pipeline::create_image_bind_group;

enum CachedImageResources {
    Live {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        bind_group: wgpu::BindGroup,
    },
    #[cfg(test)]
    Stub,
}

/// A GPU image resource stored in the cache.
pub struct CachedImage {
    pub(crate) last_used: u64,
    resources: CachedImageResources,
}

/// Deduplicates uploaded textures across frames using content hashes.
#[derive(Default)]
pub struct ImageCache {
    pub(crate) entries: HashMap<u64, CachedImage>,
    pub(crate) generation: u64,
}

impl ImageCache {
    /// Advances the cache generation so later rebuild work can refresh liveness.
    pub fn begin_frame(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }

    /// Returns the cached GPU image for a content hash if it exists.
    pub fn get(&self, content_hash: u64) -> Option<&CachedImage> {
        self.entries.get(&content_hash)
    }

    /// Refreshes a cached image liveness from the frame path without rebuilding it.
    pub fn touch(&mut self, content_hash: u64) -> bool {
        let Some(entry) = self.entries.get_mut(&content_hash) else {
            return false;
        };
        entry.last_used = self.generation;
        true
    }

    /// Uploads an image on first use and refreshes its liveness every rebuild.
    pub fn get_or_insert(
        &mut self,
        data: &ImageData,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        screen_buffer: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
    ) -> bool {
        let content_hash = data.content_hash();
        if let Some(entry) = self.entries.get_mut(&content_hash) {
            entry.last_used = self.generation;
            return false;
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("stele.image_texture"),
            size: wgpu::Extent3d {
                width: data.width(),
                height: data.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data.rgba(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(data.width() * 4),
                rows_per_image: Some(data.height()),
            },
            wgpu::Extent3d {
                width: data.width(),
                height: data.height(),
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group =
            create_image_bind_group(device, bind_group_layout, screen_buffer, &view, sampler);
        self.entries.insert(
            content_hash,
            CachedImage::live(texture, view, bind_group, self.generation),
        );
        true
    }

    /// Drops textures that have not been referenced for `max_age` generations.
    pub fn evict_stale(&mut self, max_age: u64) {
        let min_generation = self.generation.saturating_sub(max_age);
        self.entries
            .retain(|_, entry| entry.last_used >= min_generation);
    }

    #[cfg(test)]
    pub(crate) fn insert_stub(&mut self, content_hash: u64, last_used: u64) {
        self.entries
            .insert(content_hash, CachedImage::stub(last_used));
    }

    #[cfg(test)]
    pub(crate) fn last_used(&self, content_hash: u64) -> Option<u64> {
        self.entries.get(&content_hash).map(|entry| entry.last_used)
    }
}

impl CachedImage {
    fn live(
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        bind_group: wgpu::BindGroup,
        last_used: u64,
    ) -> Self {
        Self {
            last_used,
            resources: CachedImageResources::Live {
                texture,
                view,
                bind_group,
            },
        }
    }

    pub(crate) fn bind_group(&self) -> Option<&wgpu::BindGroup> {
        match &self.resources {
            CachedImageResources::Live {
                texture: _texture,
                view: _view,
                bind_group,
            } => Some(bind_group),
            #[cfg(test)]
            CachedImageResources::Stub => None,
        }
    }

    #[cfg(test)]
    fn stub(last_used: u64) -> Self {
        Self {
            last_used,
            resources: CachedImageResources::Stub,
        }
    }
}
