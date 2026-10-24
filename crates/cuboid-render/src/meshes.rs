//! Mesh vertex and index buffers and per-sub-mesh draws (spec: app 8.3; arch 13.6).
//!
//! A mesh object has one vertex buffer (position, normal, UV) and one index buffer, texture and
//! uniform (model matrix, normal matrix, colour) per sub-mesh. A mesh without sub-mesh records
//! is one sub-mesh over all triangles with the main texture. Sources are kept as the shared
//! `Arc`s of the extract, which is all a rebuild after device loss needs.
//!
//! The scene sends a new `MeshUpdate` for every change of the mesh object. When the geometry, the
//! texture and the sub-mesh records are the same `Arc`s as the ones on the device, only the
//! uniforms (model, normal matrix, colour) are rewritten, outside the byte budget. A new
//! geometry or texture is built next to the old resources, which keep drawing until then.

use std::sync::Arc;

use cuboid_image::Image;
use cuboid_model::{MeshGeometry, ObjId, Submesh};
use cuboid_scene::MeshUpdate;
use glam::{Mat3, Mat4, Vec3};
use rustc_hash::FxHashMap;

use crate::contract::RenderError;
use crate::elements::{BLEND, FRONT, depth_state};
use crate::targets::COLOR;
use crate::util::{guarded, to_u32, to_u64};

/// Bytes of one mesh vertex: position, normal and UV.
const VERTEX_BYTES: u64 = 32;

/// Bytes of the uniform of a sub-mesh: model matrix, normal matrix as three padded columns and
/// the colour.
pub(crate) const UNIFORM_BYTES: u64 = 128;

/// One sub-mesh on the device.
#[derive(Debug)]
struct Sub {
    indices: wgpu::Buffer,
    index_count: u32,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// A mesh on the device, with the sources it was built from.
#[derive(Debug)]
struct Gpu {
    vertices: wgpu::Buffer,
    subs: Vec<Sub>,
    bytes: u64,
    geometry: Arc<MeshGeometry>,
    texture: Option<Arc<Image>>,
    submeshes: Arc<[Submesh]>,
}

impl Gpu {
    /// Whether the resources were built from the sources of `update`.
    fn matches(&self, update: &MeshUpdate) -> bool {
        let same_texture = match (&self.texture, &update.texture) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        same_texture
            && Arc::ptr_eq(&self.geometry, &update.geometry)
            && (Arc::ptr_eq(&self.submeshes, &update.submeshes)
                || (self.submeshes.is_empty() && update.submeshes.is_empty()))
    }
}

/// A mesh object: its latest update and, once uploaded, its device resources.
#[derive(Debug)]
struct Entry {
    update: MeshUpdate,
    gpu: Option<Gpu>,
    /// The id is in `Meshes::pending`.
    queued: bool,
    /// The id is in `Meshes::moved`.
    moved: bool,
}

/// The mesh objects.
#[derive(Debug, Default)]
pub(crate) struct Meshes {
    entries: FxHashMap<ObjId, Entry>,
    /// Objects whose geometry or texture must be built, oldest first.
    pending: Vec<ObjId>,
    /// Objects whose uniforms must be rewritten.
    moved: Vec<ObjId>,
    /// Geometry and texture bytes created so far.
    pub uploaded: u64,
    /// Uniforms were rewritten since the flag was last taken.
    pub wrote: bool,
}

/// The bind group layout of a sub-mesh: texture, sampler and uniform.
pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mesh_layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                },
                count: None,
            },
        ],
    })
}

/// Vertex data of a mesh: position, normal and UV per vertex. Missing normals are the
/// area-weighted average of the adjoining triangles; missing UVs are zero.
fn vertex_data(geometry: &MeshGeometry) -> Vec<f32> {
    let count = geometry.vertices.len();
    let mut normals: Vec<Vec3> = match &geometry.normals {
        Some(n) if n.len() == count => n.clone(),
        _ => {
            let mut sums = vec![Vec3::ZERO; count];
            for tri in geometry.triangles.chunks_exact(3) {
                let at = |i: u32| geometry.vertices.get(i as usize).copied();
                if let (Some(a), Some(b), Some(c)) = (at(tri[0]), at(tri[1]), at(tri[2])) {
                    let n = (b - a).cross(c - a);
                    for &i in tri {
                        if let Some(s) = sums.get_mut(i as usize) {
                            *s += n;
                        }
                    }
                }
            }
            sums
        }
    };
    for n in &mut normals {
        *n = n.normalize_or_zero();
    }
    let mut data = Vec::with_capacity(count * 8);
    for (i, (p, n)) in geometry.vertices.iter().zip(&normals).enumerate() {
        let uv = geometry.uvs.get(i).copied().unwrap_or_default();
        data.extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z, uv.x, uv.y]);
    }
    data
}

/// The triangles of `indices` whose vertices all exist.
fn valid_triangles(indices: &[u32], vertices: usize) -> Vec<u32> {
    indices
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&i| (i as usize) < vertices))
        .flatten()
        .copied()
        .collect()
}

/// The uniform of a sub-mesh as 32 floats.
fn uniform(model: Mat4, color: [f32; 4]) -> [f32; 32] {
    let linear = Mat3::from_mat4(model);
    let normal = if linear.determinant().abs() > 1e-12 {
        linear.inverse().transpose()
    } else {
        linear
    };
    let mut words = [0.0; 32];
    words[..16].copy_from_slice(&model.to_cols_array());
    for (i, col) in [normal.x_axis, normal.y_axis, normal.z_axis]
        .into_iter()
        .enumerate()
    {
        words[16 + i * 4..19 + i * 4].copy_from_slice(&col.to_array());
    }
    words[28..].copy_from_slice(&color);
    words
}

impl Meshes {
    /// An empty store.
    pub fn new() -> Meshes {
        Meshes::default()
    }

    /// Moves the meshes of a lost device into this store and queues them all.
    pub fn adopt(&mut self, old: Meshes) {
        self.entries = old.entries;
        self.moved.clear();
        self.pending = self.entries.keys().copied().collect();
        for entry in self.entries.values_mut() {
            entry.gpu = None;
            entry.queued = true;
            entry.moved = false;
        }
    }

    /// GPU bytes of all meshes.
    pub fn bytes(&self) -> u64 {
        self.entries
            .values()
            .filter_map(|e| e.gpu.as_ref())
            .map(|g| g.bytes)
            .sum()
    }

    /// Takes a new or changed mesh object. With the sources already on the device only the
    /// uniforms are rewritten; otherwise the resources are built over the next uploads.
    pub fn apply(&mut self, update: &MeshUpdate) {
        let Some(entry) = self.entries.get_mut(&update.id) else {
            self.entries.insert(
                update.id,
                Entry {
                    update: update.clone(),
                    gpu: None,
                    queued: true,
                    moved: false,
                },
            );
            self.pending.push(update.id);
            return;
        };
        let same = entry.gpu.as_ref().is_some_and(|g| g.matches(update));
        entry.update = update.clone();
        if same && !entry.moved {
            entry.moved = true;
            self.moved.push(update.id);
        } else if !same && !entry.queued {
            entry.queued = true;
            self.pending.push(update.id);
        }
    }

    /// Drops a mesh object the scene no longer lists.
    pub fn remove(&mut self, id: ObjId) {
        self.entries.remove(&id);
    }

    /// Bytes waiting for upload.
    pub fn pending_bytes(&self) -> u64 {
        self.pending
            .iter()
            .filter_map(|id| self.entries.get(id))
            .map(|e| to_u64(upload_cost(&e.update)))
            .sum()
    }

    /// Rewrites the uniforms of moved meshes, then creates the device resources of waiting
    /// meshes until `bytes` are used up or `out_of_time` says so; at least one goes out per call.
    /// Returns the bytes uploaded.
    #[expect(
        clippy::too_many_arguments,
        reason = "the device-level objects of one upload"
    )]
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        mut room: u64,
        bytes: usize,
        out_of_time: impl Fn() -> bool,
    ) -> usize {
        for id in std::mem::take(&mut self.moved) {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            entry.moved = false;
            let Some(gpu) = &entry.gpu else {
                continue;
            };
            self.wrote = true;
            for (i, sub) in gpu.subs.iter().enumerate() {
                let words = uniform(entry.update.model, sub_color(&entry.update, i));
                queue.write_buffer(&sub.uniform, 0, bytemuck::cast_slice(&words));
            }
        }
        let mut used = 0usize;
        let mut done = 0;
        while done < self.pending.len() {
            let id = self.pending[done];
            let Some(entry) = self.entries.get_mut(&id) else {
                done += 1;
                continue;
            };
            if entry.gpu.as_ref().is_some_and(|g| g.matches(&entry.update)) {
                entry.queued = false;
                done += 1;
                continue;
            }
            let cost = upload_cost(&entry.update);
            if used > 0 && (used + cost > bytes || out_of_time()) {
                break;
            }
            done += 1;
            entry.queued = false;
            let held = entry.gpu.as_ref().map_or(0, |g| g.bytes);
            if to_u64(cost) > room + held {
                log::error!("no GPU memory budget left for a mesh of {cost} bytes");
                continue;
            }
            match create(device, queue, layout, sampler, &entry.update) {
                Ok(gpu) => {
                    room = (room + held).saturating_sub(gpu.bytes);
                    entry.gpu = Some(gpu);
                    self.uploaded += to_u64(cost);
                }
                Err(err) => log::error!("cannot upload a mesh: {err}"),
            }
            used += cost;
        }
        self.pending.drain(..done);
        used
    }

    /// What to bind and draw for a sub-mesh: vertex buffer, index buffer, index count and the
    /// bind group; `None` before the upload, for a hidden mesh and for an unknown sub-mesh.
    pub fn draw_data(
        &self,
        id: ObjId,
        submesh: u32,
    ) -> Option<(&wgpu::Buffer, &wgpu::Buffer, u32, &wgpu::BindGroup)> {
        let entry = self.entries.get(&id).filter(|e| !e.update.hidden)?;
        let gpu = entry.gpu.as_ref()?;
        let sub = gpu.subs.get(submesh as usize)?;
        Some((
            &gpu.vertices,
            &sub.indices,
            sub.index_count,
            &sub.bind_group,
        ))
    }
}

/// Bytes of the texture made from `image`; a missing or empty image becomes a white texel.
fn texture_bytes(image: Option<&Image>) -> usize {
    match image {
        Some(image) if image.w > 0 && image.h > 0 => image.w as usize * image.h as usize * 4,
        _ => 4,
    }
}

/// The image a sub-mesh samples: its own texture or the main one.
fn sub_image<'a>(sub: &'a Submesh, main: Option<&'a Image>) -> Option<&'a Image> {
    sub.texture.as_ref().map(|t| &*t.image).or(main)
}

/// The colour of sub-mesh `i`: its material colour times the tint of the mesh.
fn sub_color(update: &MeshUpdate, i: usize) -> [f32; 4] {
    let base = update.submeshes.get(i).map_or([1.0; 4], |s| s.color);
    std::array::from_fn(|k| base[k] * update.tint[k])
}

/// Bytes a mesh costs on the device: vertices, indices, uniforms and textures.
fn upload_cost(update: &MeshUpdate) -> usize {
    let geometry = &update.geometry;
    let vertices = geometry.vertices.len() * 32;
    let main = update.texture.as_deref();
    let uniform = 128;
    if update.submeshes.is_empty() {
        return vertices + geometry.triangles.len() * 4 + texture_bytes(main) + uniform;
    }
    update
        .submeshes
        .iter()
        .map(|sub| sub.triangles.len() * 4 + texture_bytes(sub_image(sub, main)) + uniform)
        .sum::<usize>()
        + vertices
}

/// One texture with the view and bytes it needs.
fn texture_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: Option<&Image>,
) -> (wgpu::TextureView, u64) {
    let (size, white);
    let rgba: &[u8] = match image {
        Some(image) if image.w > 0 && image.h > 0 => {
            size = (image.w, image.h);
            image.rgba()
        }
        _ => {
            size = (1, 1);
            white = [255u8; 4];
            &white
        }
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mesh_texture"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.0 * 4),
            rows_per_image: Some(size.1),
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    (
        texture.create_view(&wgpu::TextureViewDescriptor::default()),
        to_u64(texture_bytes(image)),
    )
}

fn create(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    update: &MeshUpdate,
) -> Result<Gpu, RenderError> {
    let geometry = &update.geometry;
    let data = vertex_data(geometry);
    let count = geometry.vertices.len();
    // Without sub-mesh records the whole mesh is one sub-mesh with the main texture and the tint.
    let whole = [Submesh {
        triangles: geometry.triangles.clone(),
        color: [1.0; 4],
        texture: None,
    }];
    let subs: &[Submesh] = if update.submeshes.is_empty() {
        &whole
    } else {
        &update.submeshes
    };
    guarded(device, "mesh buffers", || {
        let make = |label, contents: &[u8], usage| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: to_u64(contents.len()).max(4).next_multiple_of(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buffer, 0, contents);
            buffer
        };
        let vertices = make(
            "mesh_vertices",
            bytemuck::cast_slice(&data),
            wgpu::BufferUsages::VERTEX,
        );
        let mut bytes = to_u64(data.len() * 4);
        let mut out = Vec::with_capacity(subs.len());
        for sub in subs {
            let indices = valid_triangles(&sub.triangles, count);
            let index_buffer = make(
                "mesh_indices",
                bytemuck::cast_slice(&indices),
                wgpu::BufferUsages::INDEX,
            );
            let image = sub_image(sub, update.texture.as_deref());
            let (view, texture_size) = texture_view(device, queue, image);
            let uniform_buffer = make(
                "mesh_uniform",
                bytemuck::cast_slice(&uniform(update.model, sub_color(update, out.len()))),
                wgpu::BufferUsages::UNIFORM,
            );
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mesh"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                ],
            });
            bytes += to_u64(indices.len() * 4) + texture_size + UNIFORM_BYTES;
            out.push(Sub {
                indices: index_buffer,
                index_count: to_u32(indices.len()),
                uniform: uniform_buffer,
                bind_group,
            });
        }
        Gpu {
            vertices,
            subs: out,
            bytes,
            geometry: Arc::clone(&update.geometry),
            texture: update.texture.clone(),
            submeshes: Arc::clone(&update.submeshes),
        }
    })
}

/// Which pass a mesh pipeline serves.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Pass {
    Cutout,
    Prepass,
    Blend,
    /// Silhouette mask of a selected mesh, single sample, no depth.
    Mask,
    /// Silhouette mask of a hovered mesh.
    MaskHover,
}

/// The pipeline of one mesh pass.
pub(crate) fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    pass: Pass,
    samples: u32,
) -> wgpu::RenderPipeline {
    let (entry, write_mask, depth) = match pass {
        Pass::Cutout => ("fs_cutout", wgpu::ColorWrites::ALL, Some(depth_state(true))),
        Pass::Prepass => (
            "fs_cutout",
            wgpu::ColorWrites::empty(),
            Some(depth_state(true)),
        ),
        Pass::Blend => ("fs_blend", wgpu::ColorWrites::ALL, Some(depth_state(false))),
        Pass::Mask => ("fs_mask", wgpu::ColorWrites::ALL, None),
        Pass::MaskHover => ("fs_mask_hover", wgpu::ColorWrites::ALL, None),
    };
    let mask = matches!(pass, Pass::Mask | Pass::MaskHover);
    let attributes = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 12,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 24,
            shader_location: 2,
        },
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("meshes"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
                blend: (!mask).then_some(BLEND),
                write_mask,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            front_face: FRONT,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: depth,
        multisample: wgpu::MultisampleState {
            count: if mask { 1 } else { samples },
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_normals_are_the_triangle_normals() {
        let geometry = MeshGeometry {
            vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            normals: None,
            uvs: Vec::new(),
            triangles: vec![0, 1, 2],
        };
        let data = vertex_data(&geometry);
        assert_eq!(&data[3..6], &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn triangles_with_missing_vertices_are_dropped() {
        assert_eq!(valid_triangles(&[0, 1, 2, 0, 1, 9, 2], 3), [0, 1, 2]);
    }
}
