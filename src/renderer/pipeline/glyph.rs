use std::borrow::Cow;

use crate::renderer::instance::glyph_instance_layout;

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
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("../shaders/glyph.wgsl"))),
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
