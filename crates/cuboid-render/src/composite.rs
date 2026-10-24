//! Full-view composite pass with the outline into the view texture (spec: app 8.1, 8.7; arch
//! 13.1).

use cuboid_scene::{DrawTarget, OverlayKind, ViewDrawList};

use crate::contract::ViewInput;
use crate::elements::VERTICES;
use crate::gpu::{Gpu, Renderer, VIEW_STRIDE};
use crate::grid::{PLANE_STRIDE, plane_index};
use crate::look::clear_color;
use crate::pipelines::Pipelines;
use crate::sort::merge_runs;
use crate::targets::{COLOR, ViewTexture};
use crate::util::to_u32;
use crate::voxels::DIR_STRIDE;

impl Renderer {
    /// Renders every visible view whose frame key changed into the shared attachments (MSAA
    /// colour when anti-aliasing is on, reversed-Z depth, highlight mask), resolves, and runs the
    /// full-view composite pass into the view's own texture with alpha forced to 1. `views` has
    /// one entry per view in quadrant order; a hidden view (another one maximized) is `None` and
    /// keeps its picture. Returns the four view textures; a view that was never rendered or whose
    /// texture cannot be had yields a 1 x 1 placeholder.
    ///
    /// spec: arch 13.1 (steps 2 and 3), arch 13.2
    pub fn render_views(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        views: &[Option<ViewInput<'_>>; 4],
    ) -> [ViewTexture; 4] {
        self.retry_pipelines();
        let limit = self.caps.max_texture_size;
        let mut todo = Vec::with_capacity(4);
        let mut largest = (1, 1);
        for (i, view) in views.iter().enumerate() {
            let wanted = view
                .map(|v| v.params.size)
                .filter(|s| s.0 > 0 && s.1 > 0 && s.0 <= limit && s.1 <= limit);
            let Some(size) = wanted.or(self.slots[i].texture.is_none().then_some((1, 1))) else {
                continue;
            };
            let fitted = self.slots[i].fit(&self.device, size, &mut self.generation);
            let is_new = match fitted {
                Ok(is_new) => is_new,
                Err(err) => {
                    log::error!("{err}");
                    self.degraded = true;
                    continue;
                }
            };
            if let (Some(view), Some(_)) = (view, wanted)
                && (is_new || self.slots[i].key != Some(view.list.key))
            {
                todo.push(i);
                largest = (largest.0.max(size.0), largest.1.max(size.1));
            }
        }
        if !todo.is_empty() && !self.device_lost() && self.fit_attachments(largest) {
            for i in todo {
                if let Some(view) = &views[i] {
                    self.render_view(enc, i, view);
                }
            }
        }
        self.view_textures()
    }

    /// The four textures, placeholders where a view has none.
    fn view_textures(&mut self) -> [ViewTexture; 4] {
        let blank = self
            .slots
            .iter()
            .any(|s| s.texture.is_none())
            .then(|| blank_view(&self.device));
        std::array::from_fn(|i| {
            self.slots[i]
                .handle()
                .or_else(|| blank.clone())
                .unwrap_or_else(|| blank_view(&self.device))
        })
    }

    /// Renders one view: the scene pass, the outline mask when something is highlighted, and
    /// the composite into the view texture.
    fn render_view(&mut self, enc: &mut wgpu::CommandEncoder, i: usize, view: &ViewInput<'_>) {
        self.write_view(i, &view.params);
        let lines = self.upload_lines(i, view.params.view_index, &view.list.overlays);
        let hovered = self.hovered_mesh;
        let (w, h) = view.params.size;
        let (Some(gpu), Some((_, target))) = (&self.gpu, &self.slots[i].texture) else {
            return;
        };
        let (Some(pipes), Some(att), Some(composite_bg)) =
            (&gpu.pipes, &gpu.attach, &gpu.composite_bg)
        else {
            return;
        };
        let (color, resolve) = match &att.msaa {
            Some(msaa) => (msaa, Some(&att.resolve)),
            None => (&att.resolve, None),
        };
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("view_scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    resolve_target: resolve,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color(self.settings.clear)),
                        store: if resolve.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(depth_attachment(&att.depth)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, 0, w, h);
            draw_list(gpu, pipes, &mut pass, view.list, i, Some(lines));
        }

        let outlined = !view.list.outline.is_empty();
        if outlined {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("outline_mask"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &att.mask,
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
            pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, 0, w, h);
            pass.set_bind_group(0, &gpu.view_bg, &[view_offset(i)]);
            let mut in_elements = false;
            for item in &view.list.outline {
                match item.target {
                    DrawTarget::Elements { first, count } => {
                        if !in_elements {
                            in_elements = true;
                            pass.set_pipeline(&pipes.mask);
                            pass.set_bind_group(1, &gpu.atlas.bind_group, &[]);
                            pass.set_vertex_buffer(0, gpu.elements.buffer().slice(..));
                        }
                        draw_elements(gpu, &mut pass, first, count);
                    }
                    DrawTarget::Mesh { id, submesh } => {
                        let Some((vertices, indices, n, texture)) =
                            gpu.meshes.draw_data(id, submesh)
                        else {
                            continue;
                        };
                        in_elements = false;
                        pass.set_pipeline(if hovered == Some(id) {
                            &pipes.mesh_mask_hover
                        } else {
                            &pipes.mesh_mask
                        });
                        pass.set_bind_group(1, texture, &[]);
                        pass.set_vertex_buffer(0, vertices.slice(..));
                        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..n, 0, 0..1);
                    }
                    DrawTarget::SubGrid(_) | DrawTarget::Background(_) => {}
                }
            }
        }

        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("view_composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(if outlined {
            &pipes.outline
        } else {
            &pipes.plain
        });
        pass.set_bind_group(0, composite_bg, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        self.slots[i].key = Some(view.list.key);
    }
}

/// A 1 x 1 texture for a view that was never rendered.
fn blank_view(device: &wgpu::Device) -> ViewTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("blank_view"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    ViewTexture {
        view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        texture,
        size: (1, 1),
        generation: 0,
    }
}

/// The dynamic offset of a view slot.
pub(crate) fn view_offset(slot: usize) -> u32 {
    to_u32(slot) * to_u32(usize::try_from(VIEW_STRIDE).unwrap_or(256))
}

/// The reversed-Z depth attachment: cleared to 0, not kept.
pub(crate) fn depth_attachment(
    view: &wgpu::TextureView,
) -> wgpu::RenderPassDepthStencilAttachment<'_> {
    wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(0.0),
            store: wgpu::StoreOp::Discard,
        }),
        stencil_ops: None,
    }
}

/// Draws a run of element instances with the pipeline and bindings already set.
fn draw_elements(gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>, first: u32, count: u32) {
    let end = first.saturating_add(count).min(gpu.elements.drawable());
    if first < end {
        pass.draw(0..VERTICES, first..end);
    }
}

/// Encodes a draw list into a pass: the voxel pools by direction, then the transparent queue in
/// order. With `lines` (the vertex counts of the tested and the over lines in the view's line
/// buffer) the view's own content follows: leftover sub-grid planes and the overlay lines; PNG
/// renders pass `None` and draw the model only.
pub(crate) fn draw_list(
    gpu: &Gpu,
    pipes: &Pipelines,
    pass: &mut wgpu::RenderPass<'_>,
    list: &ViewDrawList,
    slot: usize,
    lines: Option<(u32, u32)>,
) {
    let model_only = lines.is_none();
    pass.set_bind_group(0, &gpu.view_bg, &[view_offset(slot)]);

    pass.set_pipeline(&pipes.voxel);
    for (dir, ranges) in list.voxel_ranges.iter().enumerate() {
        let draws = gpu.voxels.draws(dir, ranges);
        if draws.is_empty() {
            continue;
        }
        let offset = to_u32(dir) * DIR_STRIDE;
        pass.set_bind_group(1, &gpu.dir_bg, &[offset]);
        pass.set_vertex_buffer(0, gpu.voxels.buffer(dir).slice(..));
        for (first, count) in draws {
            pass.draw(0..6, first..first + count);
        }
    }

    let mut grid_in_queue = [false; 3];
    // Group 1 and the vertex buffer stay bound across consecutive element items.
    let mut in_elements = false;
    for item in merge_runs(&list.items) {
        if !matches!(item.target, DrawTarget::Elements { .. }) {
            in_elements = false;
        }
        match item.target {
            DrawTarget::Elements { first, count } => {
                if !in_elements {
                    in_elements = true;
                    pass.set_bind_group(1, &gpu.atlas.bind_group, &[]);
                    pass.set_vertex_buffer(0, gpu.elements.buffer().slice(..));
                }
                if item.translucent {
                    pass.set_pipeline(&pipes.prepass);
                    draw_elements(gpu, pass, first, count);
                    pass.set_pipeline(&pipes.blend);
                } else {
                    pass.set_pipeline(&pipes.cutout);
                }
                draw_elements(gpu, pass, first, count);
            }
            DrawTarget::Mesh { id, submesh } => {
                let Some((vertices, indices, n, texture)) = gpu.meshes.draw_data(id, submesh)
                else {
                    continue;
                };
                if n == 0 {
                    continue;
                }
                pass.set_bind_group(1, texture, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                if item.translucent {
                    pass.set_pipeline(&pipes.mesh_prepass);
                    pass.draw_indexed(0..n, 0, 0..1);
                    pass.set_pipeline(&pipes.mesh_blend);
                } else {
                    pass.set_pipeline(&pipes.mesh_cutout);
                }
                pass.draw_indexed(0..n, 0, 0..1);
            }
            DrawTarget::SubGrid(plane) => {
                grid_in_queue[plane_index(plane)] = true;
                if !model_only {
                    draw_plane(gpu, pipes, pass, plane_index(plane));
                }
            }
            DrawTarget::Background(_) => {}
        }
    }

    let Some((tested, over)) = lines else {
        return;
    };
    for overlay in &list.overlays {
        if let OverlayKind::SubGrid(plane) = overlay.kind
            && !grid_in_queue[plane_index(plane)]
        {
            draw_plane(gpu, pipes, pass, plane_index(plane));
        }
    }
    let Some(buffer) = &gpu.lines[slot].buffer else {
        return;
    };
    pass.set_bind_group(0, &gpu.view_bg, &[view_offset(slot)]);
    pass.set_vertex_buffer(0, buffer.slice(..));
    if tested > 0 {
        pass.set_pipeline(&pipes.lines_tested);
        pass.draw(0..tested, 0..1);
    }
    if over > 0 {
        pass.set_pipeline(&pipes.lines_over);
        pass.draw(tested..tested + over, 0..1);
    }
}

fn draw_plane(gpu: &Gpu, pipes: &Pipelines, pass: &mut wgpu::RenderPass<'_>, index: usize) {
    pass.set_pipeline(&pipes.grid);
    pass.set_bind_group(1, &gpu.plane_bg, &[to_u32(index) * PLANE_STRIDE]);
    pass.draw(0..6, 0..1);
}
