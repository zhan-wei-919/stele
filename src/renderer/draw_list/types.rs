use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SubpixelBin {
    pub x: u8,
    pub y: u8,
}

impl SubpixelBin {
    pub fn new(x: u8, y: u8) -> Self {
        debug_assert!(x < 4, "SubpixelBin.x must be in 0..=3");
        debug_assert!(y < 4, "SubpixelBin.y must be in 0..=3");
        Self { x, y }
    }
}

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
    pub fn glyph_key(&self, scale_factor: f32) -> GlyphKey {
        GlyphKey {
            font_id: self.font_id,
            glyph_id: self.glyph_id,
            font_size_bits: self.font_size.to_bits(),
            scale_factor_bits: scale_factor.to_bits(),
            subpixel_offset: self.subpixel_offset,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlyphKey {
    pub font_id: u32,
    pub glyph_id: u16,
    pub font_size_bits: u32,
    pub scale_factor_bits: u32,
    pub subpixel_offset: SubpixelBin,
}

impl GlyphKey {
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

    pub fn font_size(&self) -> f32 {
        f32::from_bits(self.font_size_bits)
    }

    pub fn scale_factor(&self) -> f32 {
        f32::from_bits(self.scale_factor_bits)
    }
}

impl Hash for GlyphKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.font_id.hash(state);
        self.glyph_id.hash(state);
        self.font_size_bits.hash(state);
        self.scale_factor_bits.hash(state);
        self.subpixel_offset.hash(state);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RectCmd {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
}
