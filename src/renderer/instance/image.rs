//! Image instance encoding shared between the CPU draw list and WGSL shaders.
//
// The renderer-side image bridge is staged behind store/view payload plumbing.
// Keep the instance conversion here so the GPU ABI stays tested and ready.
#![allow(dead_code)]

use bytemuck::{Pod, Zeroable};

use crate::draw_list::ImageCmd;

/// Per-instance data for a textured image quad.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ImageInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub opacity: f32,
    pub _padding: [f32; 3],
}

impl ImageInstance {
    /// Converts logical image placement into physical pixels for the GPU.
    pub fn from_image(cmd: &ImageCmd, scale_factor: f32) -> Self {
        let pos = cmd.pos();
        let size = cmd.size();
        Self {
            pos: [pos[0] * scale_factor, pos[1] * scale_factor],
            size: [size[0] * scale_factor, size[1] * scale_factor],
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
            opacity: 1.0,
            _padding: [0.0; 3],
        }
    }
}

/// Returns the vertex-buffer layout expected by the image render pipeline.
pub fn image_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32,
        5 => Float32x3
    ];

    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ImageInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}
