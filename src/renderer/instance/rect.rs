//! Rectangle instance encoding shared between the CPU draw list and WGSL shaders.

use bytemuck::{Pod, Zeroable};

use crate::renderer::draw_list::RectCmd;

/// Per-instance data for a solid rectangle quad.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct RectInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
}

impl RectInstance {
    /// Converts a rectangle command into GPU instance data.
    pub fn from_rect(cmd: RectCmd) -> Self {
        Self {
            pos: cmd.pos,
            size: cmd.size,
            color: cmd.color,
        }
    }
}

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
