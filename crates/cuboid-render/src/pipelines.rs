//! Bind group layouts, shader modules and the render pipelines of every pass.

use crate::contract::RenderError;
use crate::grid::{grid_pipeline, line_pipeline};
use crate::look::shader;
use crate::util::guarded;
use crate::{atlas, elements, meshes, outline, voxels};

/// The bind group layouts shared by the pipelines.
#[derive(Debug)]
pub(crate) struct Layouts {
    /// Group 0 of the scene shaders: the per-view uniform with a dynamic offset.
    pub view: wgpu::BindGroupLayout,
    /// Group 1 of the element shader: atlas array and sampler.
    pub atlas: wgpu::BindGroupLayout,
    /// Group 1 of the mesh shader: texture and sampler.
    pub mesh: wgpu::BindGroupLayout,
    /// Group 1 of the voxel shader: the table of a direction, dynamic offset.
    pub dir: wgpu::BindGroupLayout,
    /// Group 1 of the grid shader: a plane, dynamic offset.
    pub plane: wgpu::BindGroupLayout,
    /// Group 0 of the composite shader.
    pub composite: wgpu::BindGroupLayout,
}

/// A uniform binding with a dynamic offset.
fn uniform_layout(
    device: &wgpu::Device,
    label: &'static str,
    visibility: wgpu::ShaderStages,
    size: u64,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        }],
    })
}

/// Bytes of the per-view uniform.
pub(crate) const VIEW_BYTES: u64 = 112;
/// Bytes of the table of one voxel direction.
pub(crate) const DIR_BYTES: u64 = 80;
/// Bytes of one grid plane.
pub(crate) const PLANE_BYTES: u64 = 48;

impl Layouts {
    pub fn new(device: &wgpu::Device) -> Layouts {
        let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        Layouts {
            view: uniform_layout(device, "view_layout", both, VIEW_BYTES),
            atlas: atlas::layout(device),
            mesh: meshes::layout(device),
            dir: uniform_layout(device, "dir_layout", wgpu::ShaderStages::VERTEX, DIR_BYTES),
            plane: uniform_layout(
                device,
                "plane_layout",
                wgpu::ShaderStages::VERTEX,
                PLANE_BYTES,
            ),
            composite: outline::layout(device),
        }
    }
}

/// Every pipeline of the renderer for one sample count.
#[derive(Debug)]
pub(crate) struct Pipelines {
    pub samples: u32,
    pub cutout: wgpu::RenderPipeline,
    pub prepass: wgpu::RenderPipeline,
    pub blend: wgpu::RenderPipeline,
    pub mask: wgpu::RenderPipeline,
    pub mesh_cutout: wgpu::RenderPipeline,
    pub mesh_prepass: wgpu::RenderPipeline,
    pub mesh_blend: wgpu::RenderPipeline,
    pub mesh_mask: wgpu::RenderPipeline,
    pub mesh_mask_hover: wgpu::RenderPipeline,
    pub voxel: wgpu::RenderPipeline,
    pub grid: wgpu::RenderPipeline,
    pub lines_tested: wgpu::RenderPipeline,
    pub lines_over: wgpu::RenderPipeline,
    pub plain: wgpu::RenderPipeline,
    pub outline: wgpu::RenderPipeline,
}

impl Pipelines {
    /// Compiles the shaders and creates the pipelines inside error scopes.
    pub fn new(
        device: &wgpu::Device,
        layouts: &Layouts,
        samples: u32,
    ) -> Result<Pipelines, RenderError> {
        guarded(device, "pipelines", || {
            let module = |label, parts: &[&str]| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Wgsl(shader(parts).into()),
                })
            };
            let common = include_str!("shaders/view.wgsl");
            let elements_module =
                module("elements", &[common, include_str!("shaders/elements.wgsl")]);
            let meshes_module = module("meshes", &[common, include_str!("shaders/meshes.wgsl")]);
            let voxels_module = module("voxels", &[common, include_str!("shaders/voxels.wgsl")]);
            let grid_module = module("grid", &[common, include_str!("shaders/grid.wgsl")]);
            let lines_module = module("lines", &[common, include_str!("shaders/lines.wgsl")]);
            let composite_module = module("composite", &[include_str!("shaders/composite.wgsl")]);

            let layout = |label, groups: &[&wgpu::BindGroupLayout]| {
                let groups: Vec<Option<&wgpu::BindGroupLayout>> =
                    groups.iter().map(|g| Some(*g)).collect();
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(label),
                    bind_group_layouts: &groups,
                    immediate_size: 0,
                })
            };
            let elements_layout = layout("elements", &[&layouts.view, &layouts.atlas]);
            let meshes_layout = layout("meshes", &[&layouts.view, &layouts.mesh]);
            let voxels_layout = layout("voxels", &[&layouts.view, &layouts.dir]);
            let grid_layout = layout("grid", &[&layouts.view, &layouts.plane]);
            let lines_layout = layout("lines", &[&layouts.view]);
            let composite_layout = layout("composite", &[&layouts.composite]);

            use elements::Pass as E;
            use meshes::Pass as M;
            let element = |pass| {
                elements::pipeline(device, &elements_module, &elements_layout, pass, samples)
            };
            let mesh =
                |pass| meshes::pipeline(device, &meshes_module, &meshes_layout, pass, samples);
            Pipelines {
                samples,
                cutout: element(E::Cutout),
                prepass: element(E::Prepass),
                blend: element(E::Blend),
                mask: element(E::Mask),
                mesh_cutout: mesh(M::Cutout),
                mesh_prepass: mesh(M::Prepass),
                mesh_blend: mesh(M::Blend),
                mesh_mask: mesh(M::Mask),
                mesh_mask_hover: mesh(M::MaskHover),
                voxel: voxels::pipeline(device, &voxels_module, &voxels_layout, samples),
                grid: grid_pipeline(device, &grid_module, &grid_layout, samples),
                lines_tested: line_pipeline(device, &lines_module, &lines_layout, true, samples),
                lines_over: line_pipeline(device, &lines_module, &lines_layout, false, samples),
                plain: outline::pipeline(device, &composite_module, &composite_layout, false),
                outline: outline::pipeline(device, &composite_module, &composite_layout, true),
            }
        })
    }
}
