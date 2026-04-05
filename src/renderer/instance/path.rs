//! Path vertex encoding shared between lyon tessellation output and WGSL shaders.

use bytemuck::{Pod, Zeroable};

/// Per-vertex data for tessellated vector paths.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct PathVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
    pub coverage: f32,
}

/// Returns the vertex-buffer layout expected by the path render pipeline.
pub fn path_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32];

    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<PathVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}
