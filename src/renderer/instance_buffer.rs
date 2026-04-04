use bytemuck::{Pod, Zeroable};

use crate::{AtlasRegion, PositionedGlyph, RectCmd};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GlyphInstance {
    pub screen_pos: [f32; 2],
    pub size: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub color: [f32; 4],
    pub bearing: [f32; 2],
}

impl GlyphInstance {
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct RectInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
}

impl RectInstance {
    pub fn from_rect(cmd: RectCmd) -> Self {
        Self {
            pos: cmd.pos,
            size: cmd.size,
            color: cmd.color,
        }
    }
}

pub fn glyph_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x4,
        5 => Float32x2
    ];

    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GlyphInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}

pub fn rect_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];

    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}
