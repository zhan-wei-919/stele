//! wgpu vertex-buffer layout for the glyph render pipeline.
//!
//! The `GlyphInstance` struct itself lives in `scene::instance` — it is a
//! scene-layer product. This file owns only the GPU pipeline glue that
//! depends on `wgpu`.

use crate::scene::instance::GlyphInstance;

/// Returns the vertex-buffer layout expected by the glyph render pipeline.
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
