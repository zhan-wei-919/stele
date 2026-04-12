//! Glyph draw commands stored in the draw list.

use crate::font::{GlyphKey, SubpixelBin};

/// Glyph positioned in logical pixel coordinates, ready for atlas lookup.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PositionedGlyph {
    pub(crate) font_id: u32,
    pub(crate) glyph_id: u16,
    pub(crate) font_size: f32,
    pub(crate) pos: [f32; 2],
    pub(crate) color: [f32; 4],
    pub(crate) subpixel_offset: SubpixelBin,
}

impl PositionedGlyph {
    /// Builds the rasterization cache key for the current scale factor.
    pub(crate) fn glyph_key(&self, scale_factor: f32) -> GlyphKey {
        GlyphKey::new(
            self.font_id,
            self.glyph_id,
            self.font_size,
            scale_factor,
            self.subpixel_offset,
        )
    }
}
