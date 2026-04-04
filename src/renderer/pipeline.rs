use std::borrow::Cow;

use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::instance_buffer::{
    glyph_instance_layout, rect_instance_layout, GlyphInstance, RectInstance,
};

pub fn create_glyph_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    atlas_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    assert!(
        device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING),
        "GPU does not support dual-source blending, required for subpixel text rendering"
    );

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stele.glyph_shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/glyph.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("stele.glyph_pipeline_layout"),
        bind_group_layouts: &[Some(atlas_bind_group_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("stele.glyph_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[glyph_instance_layout()],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Src1,
                        dst_factor: wgpu::BlendFactor::OneMinusSrc1,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Src1Alpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrc1Alpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

pub fn create_rect_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stele.rect_shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/rect.wgsl"))),
    });
    let screen_layout = create_screen_size_bind_group_layout(device);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("stele.rect_pipeline_layout"),
        bind_group_layouts: &[Some(&screen_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("stele.rect_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[rect_instance_layout()],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

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

pub fn screen_uniform(width: u32, height: u32) -> [f32; 4] {
    [width as f32, height as f32, 0.0, 0.0]
}

#[allow(dead_code)]
const _: fn() = || {
    let _ = std::mem::size_of::<GlyphInstance>();
    let _ = std::mem::size_of::<RectInstance>();
};
