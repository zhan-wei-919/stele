//! Shared glyph cache keys used by layout, rasterization, and atlas caching.

/// Quantized subpixel offset used to key LCD-rendered glyph variants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SubpixelBin {
    pub x: u8,
    pub y: u8,
}

impl SubpixelBin {
    /// Creates a subpixel bin that callers must keep within the 0..=3 range.
    pub fn new(x: u8, y: u8) -> Self {
        debug_assert!(x < 4, "SubpixelBin.x must be in 0..=3");
        debug_assert!(y < 4, "SubpixelBin.y must be in 0..=3");
        Self { x, y }
    }
}

/// Cache key for a rasterized glyph variant in the atlas.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GlyphKey {
    pub font_id: u32,
    pub glyph_id: u16,
    pub font_size_bits: u32,
    pub scale_factor_bits: u32,
    pub subpixel_offset: SubpixelBin,
}

impl GlyphKey {
    /// Creates a cache key from logical font settings and quantized subpixel placement.
    pub fn new(
        font_id: u32,
        glyph_id: u16,
        font_size: f32,
        scale_factor: f32,
        subpixel_offset: SubpixelBin,
    ) -> Self {
        Self {
            font_id,
            glyph_id,
            font_size_bits: font_size.to_bits(),
            scale_factor_bits: scale_factor.to_bits(),
            subpixel_offset,
        }
    }

    /// Returns the font size encoded into this cache key.
    pub fn font_size(&self) -> f32 {
        f32::from_bits(self.font_size_bits)
    }

    /// Returns the scale factor encoded into this cache key.
    pub fn scale_factor(&self) -> f32 {
        f32::from_bits(self.scale_factor_bits)
    }
}
