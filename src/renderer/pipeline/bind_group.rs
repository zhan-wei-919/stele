//! Shared bind-group helpers for screen-size uniforms.

use wgpu::util::{BufferInitDescriptor, DeviceExt};

/// Creates the bind-group layout used to expose screen dimensions to shaders.
pub fn create_screen_size_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("stele.screen_size_bind_group_layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// Creates a bind group and backing uniform buffer for the current surface size.
pub fn create_screen_size_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
) -> (wgpu::BindGroup, wgpu::Buffer) {
    let uniform = screen_uniform(width, height);
    let buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("stele.screen_size_uniform"),
        contents: bytemuck::cast_slice(&[uniform]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("stele.screen_size_bind_group"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (bind_group, buffer)
}

/// Encodes the current surface dimensions into the uniform layout expected by shaders.
pub fn screen_uniform(width: u32, height: u32) -> [f32; 4] {
    [width as f32, height as f32, 0.0, 0.0]
}
