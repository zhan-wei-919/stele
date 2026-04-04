//! Internal helpers for allocating vertex buffers used by the renderer runtime.

use std::mem::size_of;

/// Allocates a vertex buffer with enough capacity for at least one instance.
pub(super) fn create_vertex_buffer<T>(
    device: &wgpu::Device,
    capacity: usize,
    label: &str,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (capacity.max(1) * size_of::<T>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Grows an instance buffer only when the current capacity is insufficient.
pub(super) fn ensure_vertex_capacity<T>(
    device: &wgpu::Device,
    required: usize,
    buffer: &mut wgpu::Buffer,
    capacity: &mut usize,
    label: &str,
) {
    if required <= *capacity {
        return;
    }

    *capacity = required.max(1).next_power_of_two();
    *buffer = create_vertex_buffer::<T>(device, *capacity, label);
}
