//! A test-pattern view texture: the path every 3D view takes into the UI (architecture 13.1).
//!
//! The pattern is rendered offscreen into an `Rgba8Unorm` texture, registered with the egui
//! renderer as a native texture and drawn as an image whose texels map one to one to physical
//! pixels. With alpha 1 and dithering off, egui leaves such pixels unchanged (fact F4).

use egui::{Color32, Painter, Pos2, Rect, TextureId, vec2};
use egui_wgpu::RenderState;

/// The pixel the pattern holds at column `x` and row `y` (row 0 on top), as the shader paints it.
///
/// spec: arch 13.1, arch 1.2 (fact F4)
pub fn pattern_pixel(x: u32, y: u32) -> [u8; 4] {
    let byte = |v: u32| (v & 255) as u8;
    [byte(x), byte(y), byte(x ^ y), 255]
}

/// An offscreen texture holding the test pattern, registered as an egui native texture.
#[derive(Debug)]
pub struct TestPattern {
    id: TextureId,
    size: (u32, u32),
}

impl TestPattern {
    /// Renders the pattern into a new `size` texture and registers it with nearest filtering.
    pub fn new(gpu: &RenderState, size: (u32, u32)) -> Self {
        let device = &gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test_pattern"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let module = device.create_shader_module(wgpu::include_wgsl!("pattern.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("test_pattern"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test_pattern"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("test_pattern"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);

        let id =
            gpu.renderer
                .write()
                .register_native_texture(device, &view, wgpu::FilterMode::Nearest);
        Self { id, size }
    }

    /// The egui id of the texture.
    pub fn id(&self) -> TextureId {
        self.id
    }

    /// The texture size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Draws the pattern with its top-left corner at `top_left` (in points). The rectangle is
    /// `size / ui_scale` points large, so that one texel covers one physical pixel.
    ///
    /// The rectangle is not snapped to physical pixels (architecture 13.1 step 3): the result is
    /// texel-exact only where `top_left` falls on a whole pixel. The renderer snaps its own views.
    pub fn paint(&self, painter: &Painter, top_left: Pos2, ui_scale: f32) {
        let extent = vec2(self.size.0 as f32, self.size.1 as f32) / ui_scale;
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(
            self.id,
            Rect::from_min_size(top_left, extent),
            uv,
            Color32::WHITE,
        );
    }
}
