//! wgpu vertex-buffer layout for the rectangle render pipeline.
//!
//! The `RectInstance` struct itself lives in `scene::instance` — it is a
//! scene-layer product. This file owns only the GPU pipeline glue that
//! depends on `wgpu`.

use crate::scene::instance::RectInstance;

/// Returns the vertex-buffer layout expected by the rectangle render pipeline.
pub fn rect_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];

    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}
