//! Image draw commands and payload hashing.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::layer::RenderLayer;

/// Immutable RGBA image payload whose content hash is computed once at creation.
#[derive(Debug)]
pub struct ImageData {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    content_hash: u64,
}

impl ImageData {
    /// Creates image data and precomputes the deduplication hash.
    pub fn new(rgba: Vec<u8>, width: u32, height: u32) -> Self {
        let content_hash = hash_image(&rgba, width, height);
        Self {
            rgba,
            width,
            height,
            content_hash,
        }
    }

    /// Returns whether the image payload matches its declared dimensions.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.rgba.len() == self.width as usize * self.height as usize * 4
    }

    /// Returns the deduplication hash derived from dimensions and RGBA bytes.
    pub fn content_hash(&self) -> u64 {
        self.content_hash
    }

    /// Returns the texture width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the texture height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the raw RGBA8 bytes ready for `queue.write_texture`.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// Image draw command referencing shared RGBA data.
#[derive(Clone, Debug)]
pub(crate) struct ImageCmd {
    pos: [f32; 2],
    size: [f32; 2],
    data: Arc<ImageData>,
    layer: RenderLayer,
}

impl ImageCmd {
    /// Creates an image command whose geometry and payload are validated at the source.
    pub(crate) fn new(
        pos: [f32; 2],
        size: [f32; 2],
        data: Arc<ImageData>,
        layer: RenderLayer,
    ) -> Self {
        debug_assert!(
            size[0] > 0.0 && size[1] > 0.0,
            "ImageCmd size must stay positive"
        );
        debug_assert!(
            data.is_valid(),
            "ImageCmd payload dimensions must match the RGBA bytes"
        );
        Self {
            pos,
            size,
            data,
            layer,
        }
    }

    /// Returns the image origin in logical pixels.
    pub(crate) fn pos(&self) -> [f32; 2] {
        self.pos
    }

    /// Returns the image size in logical pixels.
    pub(crate) fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Returns the immutable image payload.
    pub(crate) fn data(&self) -> &ImageData {
        self.data.as_ref()
    }

    /// Returns the layer bucket that should contain this image.
    pub(crate) fn layer(&self) -> RenderLayer {
        self.layer
    }
}

fn hash_image(rgba: &[u8], width: u32, height: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    rgba.hash(&mut hasher);
    hasher.finish()
}
