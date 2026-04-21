//! Glyph instance encoding shared between the CPU draw list and WGSL shaders.

use bytemuck::{Pod, Zeroable};

use super::AtlasRegion;
use crate::draw_list::PositionedGlyph;

/// Per-instance data for a glyph quad in the atlas-backed text pipeline.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct GlyphInstance {
    pub screen_pos: [f32; 2],
    pub size: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub color: [f32; 4],
    pub bearing: [f32; 2],
}

impl GlyphInstance {
    /// Converts a positioned glyph and atlas lookup result into GPU instance data.
    pub fn from_positioned_glyph(
        glyph: &PositionedGlyph,
        atlas_region: AtlasRegion,
        scale_factor: f32,
    ) -> Self {
        Self {
            screen_pos: [glyph.pos[0] * scale_factor, glyph.pos[1] * scale_factor],
            size: atlas_region.size,
            uv_min: atlas_region.uv_min,
            uv_max: atlas_region.uv_max,
            color: glyph.color,
            bearing: atlas_region.bearing,
        }
    }
}
