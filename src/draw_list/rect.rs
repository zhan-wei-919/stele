//! Solid rectangle draw commands.

use super::layer::RenderLayer;
use super::validation::color_is_valid;

/// Solid rectangle command used for backgrounds, underlines, and overlay blocks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RectCmd {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    // Stored now so block-level scene data keeps layer intent once the renderer starts batching by layer.
    #[allow(dead_code)]
    layer: RenderLayer,
}

impl RectCmd {
    /// Creates a rectangle command whose size and color are validated at the source.
    pub(crate) fn new(pos: [f32; 2], size: [f32; 2], color: [f32; 4], layer: RenderLayer) -> Self {
        debug_assert!(
            size[0] > 0.0 && size[1] > 0.0,
            "RectCmd size must stay positive"
        );
        debug_assert!(
            color_is_valid(color),
            "RectCmd color must stay within [0, 1]"
        );
        Self {
            pos,
            size,
            color,
            layer,
        }
    }

    /// Returns the rectangle origin in logical pixels.
    pub(crate) fn pos(&self) -> [f32; 2] {
        self.pos
    }

    /// Returns the rectangle size in logical pixels.
    pub(crate) fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Returns the rectangle color in normalized RGBA.
    pub(crate) fn color(&self) -> [f32; 4] {
        self.color
    }

    /// Returns the layer bucket that should contain this rectangle.
    ///
    /// This stays on the API surface even though the current renderer only consumes rectangle geometry.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn layer(&self) -> RenderLayer {
        self.layer
    }
}
