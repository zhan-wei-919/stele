//! Core draw-list types consumed by the renderer runtime.

use crate::font::{GlyphKey, SubpixelBin};

/// Glyph positioned in logical pixel coordinates, ready for atlas lookup.
#[derive(Clone, Copy, Debug)]
pub struct PositionedGlyph {
    pub font_id: u32,
    pub glyph_id: u16,
    pub font_size: f32,
    pub pos: [f32; 2],
    pub color: [f32; 4],
    pub subpixel_offset: SubpixelBin,
}

impl PositionedGlyph {
    /// Builds the rasterization cache key for the current scale factor.
    pub fn glyph_key(&self, scale_factor: f32) -> GlyphKey {
        GlyphKey::new(
            self.font_id,
            self.glyph_id,
            self.font_size,
            scale_factor,
            self.subpixel_offset,
        )
    }
}

/// Solid rectangle command used for backgrounds and cursor rendering.
#[derive(Clone, Copy, Debug, Default)]
pub struct RectCmd {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
}
