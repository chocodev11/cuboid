//! Analytic sub-grid shader and main grid lines (spec: app 6.3-6.4; arch 13.6).
//!
//! The three sub-grid planes are drawn additively inside the transparent queue. The main grid
//! lines are plain overlay lines, see [`crate::overlays`]. The line pipelines live here as well.

use cuboid_scene::GridPlane;

use crate::elements::depth_state;
use crate::overlays::LINE_BYTES;
use crate::targets::{COLOR, DEPTH};

/// Stride of the plane slots in the plane uniform buffer.
pub(crate) const PLANE_STRIDE: u32 = 256;

/// The position of a plane in the uniform buffer.
pub(crate) fn plane_index(plane: GridPlane) -> usize {
    match plane {
        GridPlane::Floor => 0,
        GridPlane::Side => 1,
        GridPlane::Front => 2,
    }
}

/// The plane uniforms: origin and the two full-length spans, per plane (app 6.3).
pub(crate) fn plane_data() -> Vec<u8> {
    // Floor y = -0.001 over x [-16, 32], z [-32, 16]; side x = -0.001 over y [-16, 32],
    // z [-32, 16]; front z = +0.001 over x [-16, 32], y [-16, 32]. Each plane is origin, u, v
    // as three padded vectors.
    let planes: [[f32; 12]; 3] = [
        [
            -16.0, -0.001, -32.0, 0.0, 48.0, 0.0, 0.0, 0.0, 0.0, 0.0, 48.0, 0.0,
        ],
        [
            -0.001, -16.0, -32.0, 0.0, 0.0, 48.0, 0.0, 0.0, 0.0, 0.0, 48.0, 0.0,
        ],
        [
            -16.0, -16.0, 0.001, 0.0, 48.0, 0.0, 0.0, 0.0, 0.0, 48.0, 0.0, 0.0,
        ],
    ];
    let stride = PLANE_STRIDE as usize;
    let mut bytes = vec![0u8; stride * 3];
    for (plane, slot) in planes.iter().zip(bytes.chunks_exact_mut(stride)) {
        slot[..48].copy_from_slice(bytemuck::cast_slice(plane));
    }
    bytes
}

/// The additive pipeline of the sub-grid planes: no culling, depth tested, no depth write.
pub(crate) fn grid_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    samples: u32,
) -> wgpu::RenderPipeline {
    let add = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sub_grid"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
                blend: Some(wgpu::BlendState {
                    color: add,
                    alpha: add,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(depth_state(false)),
        multisample: wgpu::MultisampleState {
            count: samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}

/// The line pipeline; `tested` lines compare against the depth buffer, the others draw over it.
pub(crate) fn line_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    tested: bool,
    samples: u32,
) -> wgpu::RenderPipeline {
    let depth = if tested {
        depth_state(false)
    } else {
        wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lines"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: LINE_BYTES,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Unorm8x4,
                        offset: 12,
                        shader_location: 1,
                    },
                ],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
                blend: Some(crate::elements::BLEND),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(depth),
        multisample: wgpu::MultisampleState {
            count: samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}
