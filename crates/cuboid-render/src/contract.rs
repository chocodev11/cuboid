//! Public contract stubs of this crate: cross-crate types and traits with their spec references
//! (W0.2).
//!
//! This file holds the vocabulary the application glue shares with the renderer: adapter
//! capabilities, upload budgets, read-back ids, PNG render jobs and errors. The `Renderer` and
//! its methods live in the modules of the packages that implement them (`gpu`, `composite`,
//! `offscreen`, `readback`); `lib.rs` re-exports everything at the crate root.

use crate::look::{
    CLEAR_VIEW, CUTOUT_BLOCK, CUTOUT_CUBIK, CUTOUT_ITEM, GUI_FRONT_LIGHTS, GUI_SIDE_LIGHTS,
    level_lights,
};
use cuboid_scene::{Camera, RenderExtract, ViewDrawList};
use glam::{Mat4, Quat, Vec3, Vec4};
use thiserror::Error;

/// What the adapter supports; it decides the voxel submission tier and the wireframe path.
///
/// spec: arch 13.5 (Submission tiers), arch 13.6, arch 13.9, arch 1.2 (fact F6)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AdapterCaps {
    /// `DownlevelFlags::INDIRECT_EXECUTION`: `multi_draw_indirect` is available.
    pub indirect_execution: bool,
    /// `INDIRECT_FIRST_INSTANCE`: indirect draws may carry a first instance.
    pub indirect_first_instance: bool,
    /// `VERTEX_STORAGE`: vertex pulling from storage buffers in the vertex stage.
    pub vertex_storage: bool,
    /// `POLYGON_MODE_LINE`: wireframe without the barycentric shader.
    pub polygon_mode_line: bool,
    /// `TIMESTAMP_QUERY`: GPU timings for the debug overlay.
    pub timestamp_query: bool,
    /// Largest 2D texture edge.
    pub max_texture_size: u32,
}

impl AdapterCaps {
    /// Reads the capabilities of the device and its adapter. Features count only when the device
    /// was created with them.
    pub fn detect(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Self {
        let downlevel = adapter.get_downlevel_capabilities().flags;
        let features = device.features();
        Self {
            indirect_execution: downlevel.contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION),
            indirect_first_instance: features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE),
            vertex_storage: downlevel.contains(wgpu::DownlevelFlags::VERTEX_STORAGE),
            polygon_mode_line: features.contains(wgpu::Features::POLYGON_MODE_LINE),
            timestamp_query: features.contains(wgpu::Features::TIMESTAMP_QUERY),
            max_texture_size: device.limits().max_texture_dimension_2d,
        }
    }
}

/// How much one frame may upload; leftovers carry over to the next frame.
///
/// spec: arch 13.1 (step 1)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct UploadBudget {
    /// Bytes per frame.
    pub bytes: usize,
    /// Milliseconds of CPU time per frame.
    pub millis: f32,
}

impl Default for UploadBudget {
    /// 8 MiB or 4 ms.
    fn default() -> Self {
        Self {
            bytes: 8 * 1024 * 1024,
            millis: 4.0,
        }
    }
}

/// How faces are lit. The mode only changes uniforms, so switching it renders every view again
/// but rebuilds nothing.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ShadingMode {
    /// Cubik's look: flat ambient plus two directional lights (app 8.2, 8.3). Cutout at alpha 0.5.
    Cubik = 0,
    /// A Minecraft block model placed in the world: one constant per face from the nearest axis
    /// of its normal (up 1.0, down 0.5, north and south 0.8, east and west 0.6), 1.0 for an
    /// element with `shade` false. [`RenderSettings::smooth_block_light`] switches to
    /// Blockbench's continuous light. Cutout at alpha 0.5.
    #[default]
    MinecraftBlock = 1,
    /// A Minecraft item or entity: `min(1, (max(0, n.l0) + max(0, n.l1)) * 0.6 + 0.4)` under the
    /// two lights of [`RenderSettings::item_light`]. Cutout at alpha 0.1.
    MinecraftItem = 2,
    /// No lighting: every face shows its texture and tint. Cutout at alpha 0.1.
    Flat = 3,
}

impl ShadingMode {
    /// All modes in menu order.
    pub const ALL: [ShadingMode; 4] = [
        ShadingMode::Cubik,
        ShadingMode::MinecraftBlock,
        ShadingMode::MinecraftItem,
        ShadingMode::Flat,
    ];

    /// The alpha below which a texel is discarded.
    pub fn cutout(self) -> f32 {
        match self {
            ShadingMode::Cubik => CUTOUT_CUBIK,
            ShadingMode::MinecraftBlock => CUTOUT_BLOCK,
            ShadingMode::MinecraftItem | ShadingMode::Flat => CUTOUT_ITEM,
        }
    }
}

/// The light setup of the Minecraft item mode: where an item is lit from. The lights are two
/// directions of the client's lighting (`light.glsl`, `Lighting.java`, verified in the 1.21.11
/// client).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ItemLight {
    /// Items in the world (hand, ground, frame, head): lights fixed in the world, so orbiting
    /// the camera changes the lighting.
    #[default]
    Level,
    /// A GUI item shaped like a block (`gui_light` side): lights fixed to the camera.
    GuiSide,
    /// A flat GUI item (`gui_light` front): lights fixed to the camera, from the front.
    GuiFront,
}

impl ItemLight {
    /// Directions toward the two lights in the world of the view, internal axes. The GUI
    /// setups are given in the eye frame and turned by the camera rotation.
    pub(crate) fn world_directions(self, camera_rot: Quat) -> [Vec3; 2] {
        // The internal world is Minecraft's with z negated.
        let eye = |l: Vec3| camera_rot * Vec3::new(l.x, l.y, -l.z);
        match self {
            ItemLight::Level => level_lights().map(|l| Vec3::new(l.x, l.y, -l.z)),
            ItemLight::GuiSide => GUI_SIDE_LIGHTS.map(eye),
            ItemLight::GuiFront => GUI_FRONT_LIGHTS.map(eye),
        }
    }
}

/// The camera of one view as the renderer needs it: the matrices of a camera in the left-handed
/// internal space and the size of the picture.
///
/// spec: app 5.1, arch 13.2 (reversed Z)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ViewParams {
    /// World to clip space with reversed Z (near maps to 1, far to 0).
    pub view_proj: Mat4,
    /// World position of the camera.
    pub camera_pos: Vec3,
    /// Position in quadrant order; element instances carry a view mask over these four bits.
    pub view_index: u8,
    /// Size of the picture in physical pixels.
    pub size: (u32, u32),
    /// World rotation of the camera; it turns the lights of the GUI item setups.
    pub rotation: Quat,
    /// A display-context preview: the view is lit like an item with this setup instead of the
    /// shading mode of the settings. It applies to the Minecraft modes only; the Cubik and flat
    /// modes keep their own look. `None` for the ordinary viewport.
    pub item_light: Option<ItemLight>,
}

impl ViewParams {
    /// The params of a scene camera: Unity-style left-handed pose (looking along local +Z), the
    /// vertical field of view or the orthographic half height, and the clip planes of the camera.
    pub fn from_camera(camera: &Camera, view_index: u8, size: (u32, u32)) -> Self {
        let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
        // z' = w - z flips the depth range of the camera's projection: near 1, far 0.
        let flip = Mat4::from_cols(
            Vec4::X,
            Vec4::Y,
            Vec4::new(0.0, 0.0, -1.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 1.0),
        );
        Self {
            view_proj: flip * camera.view_proj(aspect),
            camera_pos: camera.pos,
            view_index,
            size,
            rotation: camera.rot,
            item_light: None,
        }
    }

    /// The same view lit as a display context: `Level` for hand, ground, frame and head, the
    /// GUI setups for the inventory (the model's `gui_light`).
    #[must_use]
    pub fn with_item_light(self, light: ItemLight) -> Self {
        Self {
            item_light: Some(light),
            ..self
        }
    }
}

/// One view to render: its camera and its draw list.
#[derive(Copy, Clone, Debug)]
pub struct ViewInput<'a> {
    /// The camera.
    pub params: ViewParams,
    /// What to draw.
    pub list: &'a ViewDrawList,
}

/// Renderer preferences. A change of `msaa` rebuilds the pipelines and attachments; the voxel
/// cell and the grid step only reach the next frame.
///
/// spec: app 8.1, app 6.3, arch 13.5
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RenderSettings {
    /// 4x multisampling ("Enable anti-aliasing"); off by default.
    pub msaa: bool,
    /// Voxel edge in world units, `16 / resolution` of the voxel world.
    pub voxel_cell: f32,
    /// Spacing of the sub-grid lines in world units.
    pub grid_step: f32,
    /// Bytes of GPU memory the renderer may hold for pools, atlas, meshes and attachments; over
    /// it, multisampling is dropped first and uploads that do not fit are refused.
    pub memory_budget: u64,
    /// Clear colour of a view as 8-bit RGB, the viewport colour of the application theme.
    pub clear: [u8; 3],
    /// How faces are lit; [`ShadingMode::MinecraftBlock`] by default.
    pub shading: ShadingMode,
    /// In the block mode, light by the normal continuously (Blockbench) instead of by the
    /// nearest axis (the client). The two agree on axis-aligned faces.
    pub smooth_block_light: bool,
    /// The lights of the item mode; a view with its own `ViewParams::item_light` overrides it.
    pub item_light: ItemLight,
    /// Darken voxel corners by their ambient-occlusion codes, in every shading mode.
    pub voxel_ao: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            msaa: false,
            voxel_cell: 1.0,
            grid_step: 1.0,
            memory_budget: 1 << 30,
            clear: CLEAR_VIEW,
            shading: ShadingMode::default(),
            smooth_block_light: false,
            item_light: ItemLight::default(),
            voxel_ao: true,
        }
    }
}

/// How the renderer is doing, for the status bar and the crash path of the application.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Health {
    /// The device was lost: call [`crate::Renderer::rebuild`] with a new device.
    pub device_lost: bool,
    /// The memory budget forced a reduction (multisampling off, uploads refused).
    pub degraded: bool,
    /// GPU bytes held by the renderer.
    pub gpu_bytes: u64,
    /// Element instances held.
    pub elements: u32,
    /// Voxel face records held, all directions.
    pub voxel_faces: u64,
    /// The last error that the device or an error scope reported.
    pub last_error: Option<String>,
}

/// Identifies a PNG render whose read-back is pending.
///
/// spec: arch 6 (cuboid-render), arch 13.8
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ReadbackId(pub u64);

/// A PNG render for the renderer: the draw list of the offscreen camera with the extract that
/// fills the renderer's buffers. The application glue builds it from the exporter's
/// `RenderRequest` through the scene crate, so the renderer never reads the document.
///
/// The section 6 sketch names this `RenderRequest`; the exporter-level request lives in
/// `cuboid-model`, which the renderer cannot see.
///
/// spec: app 15, arch 13.8, arch 6 (cuboid-render)
#[derive(Clone, Debug)]
pub struct PngJob {
    /// What to upload before drawing.
    pub extract: RenderExtract,
    /// What to draw: the model of the snapshot seen by the offscreen camera. Only the voxel
    /// ranges and element or mesh items count; overlays and the outline are not drawn.
    pub list: ViewDrawList,
    /// The offscreen camera; its `size` is the size of the final picture without the
    /// anti-aliasing factor.
    pub camera: ViewParams,
    /// Render at twice the size and downscale 2 x 2.
    pub antialias: bool,
}

/// Why a render failed.
///
/// spec: arch 16, arch 13.9
#[derive(Debug, Error)]
pub enum RenderError {
    /// The device was lost; the application writes a crash backup and offers a restart.
    #[error("the graphics device was lost")]
    DeviceLost,
    /// The read-back of a render failed.
    #[error("cannot read back the render: {0}")]
    Readback(String),
    /// The adapter lacks a capability the render needs.
    #[error("the adapter does not support {0}")]
    Unsupported(&'static str),
    /// A resource did not fit into the GPU memory.
    #[error("out of GPU memory creating {0}")]
    OutOfMemory(&'static str),
    /// The device reported an error for a resource or a pipeline.
    #[error("graphics error: {0}")]
    Gpu(String),
}
