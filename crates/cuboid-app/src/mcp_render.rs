//! The renderer behind the render tools of `cuboid mcp` (spec: agent toolkit R1, 5.2, 6.1): an
//! offscreen device that stays open between calls, drawing with the PNG path of the live renderer.
//!
//! The device opens at the first picture, so a session that never renders pays nothing. It is on
//! the best hardware adapter, or on a software one when that cannot give a device; with neither,
//! every picture is `RenderError::NoGpu` and the tools answer `E_NO_GPU`. The views are
//! orthographic and framed by the function the Render to PNG dialog uses, with the lighting of a
//! Minecraft block; `gui` is the inventory slot of the model, lit as the game lights it.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cuboid_mcp::{Camera, Picture, RenderError, Renderer};
use cuboid_model::image::Image;
use cuboid_model::{DisplaySlotId, Document, RenderPrefs, Rev};
use cuboid_render::{AdapterCaps, PngJob, RenderSettings, ShadingMode, ViewParams};
use cuboid_scene::{
    Camera as SceneCamera, DisplayMode, ExtractBudget, Extractor, OverlayState, Rect,
    RenderExtract, SceneIndex, StandardView, ViewId, ViewSet, anchor_transform, compile_draw_list,
    orbit_degrees, snap_view,
};
use cuboid_shell::{BackendPlan, instance_flags};
use egui_wgpu::{WgpuSetup, WgpuSetupCreateNew};
use glam::{Mat3, Mat4, Quat, Vec3};

use crate::frontend::render_job::frame_model;
use crate::preset::display_light;
use crate::render_glue::{OPTIONAL_FEATURES, adapter_caps};

/// How long a picture may take before the device counts as stuck.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Time between two looks at a read-back that is not done.
const POLL: Duration = Duration::from_millis(1);
/// The `iso` camera sits this far round from the north face towards the east face and this far
/// above the horizon, the angles of the game's inventory view of a block.
const ISO_YAW: f32 = 45.0;
const ISO_PITCH: f32 = 30.0;
/// The `gui` camera of the game looks at the centre of the 16-unit slot, which fills the picture.
const SLOT_CENTRE: Vec3 = Vec3::new(8.0, 8.0, -8.0);
const SLOT_HALF: f32 = 8.0;
/// How far the `gui` camera stands from the point it looks at, beyond any model.
const GUI_DISTANCE: f32 = 64.0;

/// The pictures of the render tools. Nothing is opened until the first picture.
#[derive(Default)]
pub struct GpuRenderer {
    /// `None` before the first picture; the error says why there is no device.
    gpu: Option<Result<Gpu, String>>,
}

/// An open device and the renderer on it.
struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    caps: AdapterCaps,
    renderer: cuboid_render::Renderer,
    /// The renderer holds meshes or voxels of an earlier upload. It drops a mesh only when told
    /// and a voxel chunk only for a newer one, and a document loaded again has new mesh ids and
    /// older voxel stamps, so such a renderer is made anew before the next upload.
    heavy: bool,
    /// What the renderer holds of the last picture's document.
    scene: Option<Scene>,
}

/// A document that is in the renderer, with the extractor that put it there.
struct Scene {
    rev: Rev,
    extractor: Extractor,
    index: SceneIndex,
}

impl GpuRenderer {
    /// The adapter in use, for `--check`.
    pub fn describe(&self) -> String {
        match &self.gpu {
            None => "not opened".to_owned(),
            Some(Err(why)) => format!("none ({why})"),
            Some(Ok(gpu)) => {
                let info = gpu.adapter.get_info();
                format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
            }
        }
    }
}

impl Renderer for GpuRenderer {
    fn render(
        &mut self,
        doc: &Document,
        camera: Camera,
        size: u32,
    ) -> Result<Picture, RenderError> {
        let Ok(gpu) = self.gpu.get_or_insert_with(Gpu::open) else {
            return Err(RenderError::NoGpu);
        };
        let picture = gpu.draw(doc, camera, size);
        // A lost device is opened again by the next picture.
        if picture.is_err() && gpu.renderer.device_lost() {
            self.gpu = None;
        }
        picture
    }
}

type Select = fn(&[wgpu::Adapter]) -> Result<wgpu::Adapter, String>;

/// The adapter to draw with: a discrete GPU before an integrated one, any GPU before a software
/// adapter.
fn best_adapter(adapters: &[wgpu::Adapter]) -> Result<wgpu::Adapter, String> {
    adapters
        .iter()
        .min_by_key(|adapter| match adapter.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu => 2,
            wgpu::DeviceType::Other => 3,
            wgpu::DeviceType::Cpu => 4,
        })
        .cloned()
        .ok_or_else(|| "no adapter found".to_owned())
}

fn software_adapter(adapters: &[wgpu::Adapter]) -> Result<wgpu::Adapter, String> {
    adapters
        .iter()
        .find(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu)
        .cloned()
        .ok_or_else(|| "no software adapter".to_owned())
}

/// The adapter that `select` picks, with a device and queue on it. The limits are those of the
/// window's device; its egui renderer is not built, since the server draws no interface.
fn open_device(select: Select) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    // `WGPU_BACKEND`, else DX12 alone on Windows, where WARP is the software adapter (the window
    // starts on Vulkan there). An instance that also loads Vulkan makes the process end about
    // 45 ms later on the development machine, and a game or capture tool may have broken its
    // layers; the client waits only 300 ms for the process to end. The server has no fallback
    // set: its own software adapter step covers a machine without a hardware one.
    let backends = BackendPlan::offscreen().first;
    setup.instance_descriptor.backends = backends;
    setup.instance_descriptor.flags = instance_flags();
    let describe = Arc::clone(&setup.device_descriptor);
    let instance = pollster::block_on(WgpuSetup::CreateNew(setup).new_instance());
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    let adapter = select(&adapters)?;
    let mut descriptor = describe(&adapter);
    descriptor.required_features |= OPTIONAL_FEATURES & adapter.features();
    let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
        .map_err(|error| error.to_string())?;
    Ok((adapter, device, queue))
}

impl Gpu {
    fn open() -> Result<Gpu, String> {
        let (adapter, device, queue) = open_device(best_adapter).or_else(|first| {
            log::warn!("no device on the best adapter ({first}); trying a software one");
            open_device(software_adapter).map_err(|second| format!("{first}; {second}"))
        })?;
        let caps = adapter_caps(&adapter, &device);
        let info = adapter.get_info();
        log::info!(
            "graphics: {} ({:?}, {:?}), {caps:?}",
            info.name,
            info.backend,
            info.device_type
        );
        Ok(Gpu {
            renderer: cuboid_render::Renderer::new(&device, &queue, &caps),
            adapter,
            device,
            queue,
            caps,
            heavy: false,
            scene: None,
        })
    }

    /// Draws the document from `camera` into a square of `size` pixels and waits for the pixels.
    /// The document is uploaded only when it is not the one of the last picture, so the views of
    /// a sheet share one upload.
    fn draw(&mut self, doc: &Document, camera: Camera, size: u32) -> Result<Picture, RenderError> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if size > limit {
            return Err(RenderError::TooLarge(limit));
        }
        let prefs = RenderPrefs::default();
        // A scene that fails to draw is not put back: the next picture uploads it again.
        let (mut scene, fresh) = match self.scene.take() {
            Some(scene) if scene.rev == doc.revision => (scene, false),
            _ => {
                if self.heavy {
                    self.renderer =
                        cuboid_render::Renderer::new(&self.device, &self.queue, &self.caps);
                }
                self.heavy = !doc.meshes.is_empty() || doc.voxels.is_some();
                let mut index = SceneIndex::default();
                index.sync(doc, &doc.journal);
                let scene = Scene {
                    rev: doc.revision,
                    extractor: Extractor::default(),
                    index,
                };
                (scene, true)
            }
        };
        let extract = if fresh {
            let mut copy = doc.clone();
            scene
                .extractor
                .extract(&mut copy, &prefs, &ExtractBudget::UNLIMITED)
        } else {
            RenderExtract::default()
        };

        let gui = camera == Camera::Gui;
        let eye = scene_camera(camera, doc, &scene.index);
        let mut view = ViewSet::default().view(ViewId::Main).clone();
        view.camera = eye;
        view.rect = Rect {
            x: 0.0,
            y: 0.0,
            w: size as f32,
            h: size as f32,
        };
        scene.extractor.cache_mut().set_overlay(OverlayState {
            grid: false,
            ..OverlayState::default()
        });
        let list = compile_draw_list(&view, scene.extractor.cache(), &prefs);
        let mut params = ViewParams::from_camera(&eye, ViewId::Main as u8, (size, size));
        if gui {
            params = params.with_item_light(display_light(true, doc.props.minecraft.gui_light));
        }
        let job = PngJob {
            extract,
            list,
            camera: params,
            antialias: false,
        };

        let renderer = &mut self.renderer;
        let current = renderer.settings();
        let wanted = RenderSettings {
            msaa: false,
            voxel_cell: doc
                .voxels
                .as_ref()
                .map_or(1.0, |world| 16.0 / world.res.max(1) as f32),
            shading: if gui {
                ShadingMode::MinecraftItem
            } else {
                ShadingMode::MinecraftBlock
            },
            smooth_block_light: false,
            ..current
        };
        if wanted != current {
            renderer.set_settings(wanted);
        }
        let device = &self.device;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("cuboid_mcp"),
        });
        let id = renderer.start_png_render(&mut encoder, &job);
        self.queue.submit([encoder.finish()]);

        let end = Instant::now() + TIMEOUT;
        let pixels = loop {
            let done = renderer
                .poll_readbacks(device)
                .into_iter()
                .find(|(got, _)| *got == id);
            if let Some((_, result)) = done {
                break result.map_err(|error| RenderError::Failed(error.to_string()))?;
            }
            if Instant::now() >= end {
                return Err(RenderError::Failed(
                    "the graphics device did not answer in 30 seconds".to_owned(),
                ));
            }
            thread::sleep(POLL);
        };
        self.scene = Some(scene);
        Ok(Picture::of_texture(&Image::from_rgba(pixels)))
    }
}

/// The scene camera of a view of the document. The scene's +Z is the game's north, so the view
/// that sees the north face looks along -Z, which is the scene's Back one. The orthographic views
/// of a document have one scale: [`frame_model`] frames the box of the model, not its projection.
fn scene_camera(camera: Camera, doc: &Document, index: &SceneIndex) -> SceneCamera {
    let side = match camera {
        Camera::Front | Camera::Iso => StandardView::Back,
        Camera::Back => StandardView::Front,
        Camera::Left => StandardView::Left,
        Camera::Right => StandardView::Right,
        Camera::Top => StandardView::Top,
        Camera::Bottom => StandardView::Bottom,
        Camera::Gui => return slot_camera(doc),
    };
    let mut eye = SceneCamera::default_for(ViewId::Main);
    let mut pivot = ViewSet::DEFAULT_PIVOT;
    snap_view(&mut eye, pivot, side);
    if camera == Camera::Iso {
        // From the north the camera moves to the left of the picture, which is the east.
        orbit_degrees(&mut eye, pivot, ISO_YAW, ISO_PITCH);
    }
    frame_model(&mut eye, &mut pivot, doc, index, 1.0);
    eye
}

/// The camera of the inventory slot: the camera of the game's GUI looks at a model posed by its
/// `gui` slot. The scene cannot pose a model, so the camera moves the other way instead; the
/// lights of the GUI belong to the camera and follow it, which is the same picture. A scale that
/// differs between the axes is averaged, since an orthographic camera has one.
fn slot_camera(doc: &Document) -> SceneCamera {
    let slot = &doc.display[DisplaySlotId::Gui];
    let pose = Mat4::from_translation(SLOT_CENTRE)
        * anchor_transform(DisplayMode::Gui, slot, 0.0)
        * Mat4::from_translation(-SLOT_CENTRE);
    let back = pose.inverse();
    let (across, rising) = (
        back.transform_vector3(Vec3::X),
        back.transform_vector3(Vec3::Y),
    );
    let scale = (across.length() + rising.length()) / 2.0;
    let forward = back.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
    let right = rising.cross(forward).normalize_or(Vec3::X);
    let up = forward.cross(right);
    let mut eye = SceneCamera::default_for(ViewId::Front);
    eye.rot = Quat::from_mat3(&Mat3::from_cols(right, up, forward)).normalize();
    eye.pos = back.transform_point3(SLOT_CENTRE) - forward * GUI_DISTANCE;
    eye.ortho_size = SLOT_HALF * scale;
    eye
}

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, Compat, Core, ElementKind, MeshShape};
    use cuboid_model::ObjRef;
    use cuboid_model::voxel::{I3, VoxelWorld, mapping};

    use super::*;

    fn cube() -> Core {
        let mut core = Core::new(Compat::port(), 0);
        assert!(
            core.execute(Command::AddElement(ElementKind::Cube))
                .is_done()
        );
        core
    }

    /// A renderer with its device open, or `None` with a notice when there is no adapter (an
    /// error under CI).
    fn opened() -> Option<GpuRenderer> {
        let mut renderer = GpuRenderer::default();
        match renderer.gpu.get_or_insert_with(Gpu::open) {
            Ok(_) => Some(renderer),
            Err(error) => {
                assert!(
                    std::env::var_os("CI").is_none(),
                    "no graphics adapter under CI: {error}"
                );
                eprintln!("SKIPPED (no graphics adapter): {error}");
                None
            }
        }
    }

    fn gpu_bytes(renderer: &GpuRenderer) -> u64 {
        let Some(Ok(gpu)) = &renderer.gpu else {
            panic!("the device is open");
        };
        gpu.renderer.health().gpu_bytes
    }

    /// A model of one voxel of a colour, whose world is stamped when it is made.
    fn voxel_model(cell: I3, color: u32) -> Document {
        let mut doc = Document::new(cuboid_model::TextureList::default());
        let mut world = VoxelWorld::new(16, false, mapping::grid_box(16));
        world.write_full(cell, color);
        doc.voxels = Some(world);
        doc
    }

    #[test]
    fn a_mesh_model_loaded_again_for_each_picture_keeps_the_gpu_memory_bounded() {
        let Some(mut renderer) = opened() else {
            return;
        };
        let mut held = Vec::new();
        for _ in 0..5 {
            // A reference is loaded anew for each picture, and its meshes get new ids.
            let mut core = Core::new(Compat::port(), 0);
            assert!(core.execute(Command::AddMesh(MeshShape::Cube)).is_done());
            let picture = renderer.render(core.doc(), Camera::Iso, 64).unwrap();
            assert!(picture.coverage().1 > 0.1, "the mesh is drawn");
            held.push(gpu_bytes(&renderer));
        }
        assert!(held[0] > 0, "{held:?}");
        assert!(held.iter().all(|bytes| *bytes == held[0]), "{held:?}");
    }

    #[test]
    fn a_voxel_model_drawn_again_after_another_shows_its_own_faces() {
        let Some(mut renderer) = opened() else {
            return;
        };
        // The worlds are stamped in the order they are made, so b is newer than a.
        let a = voxel_model(I3::new(18, 18, 18), 0xFFFF_0000);
        let b = voxel_model(I3::new(28, 28, 28), 0xFF00_FF00);
        let mut draw = |doc: &Document| renderer.render(doc, Camera::Iso, 64).unwrap();
        let first = draw(&a);
        let other = draw(&b);
        let again = draw(&a);
        assert!(first.coverage().1 > 0.0 && other.coverage().1 > 0.0);
        assert!(first != other, "two models give the same picture");
        assert!(
            first == again,
            "the first model changed in the third picture"
        );
    }

    fn index(doc: &Document) -> SceneIndex {
        let mut index = SceneIndex::default();
        index.sync(doc, &doc.journal);
        index
    }

    /// Where the camera looks and what is up in its picture, in the scene's axes.
    fn directions(eye: &SceneCamera) -> (Vec3, Vec3) {
        (eye.forward().round(), eye.up().round())
    }

    #[test]
    fn each_view_looks_at_the_face_it_is_named_after() {
        let core = cube();
        let doc = core.doc();
        let index = index(doc);
        // The scene's +Z is the game's north, so the north face is seen along -Z.
        let (north, south) = (Vec3::NEG_Z, Vec3::Z);
        let views = [
            (Camera::Front, north, Vec3::Y),
            (Camera::Back, south, Vec3::Y),
            (Camera::Left, Vec3::X, Vec3::Y),
            (Camera::Right, Vec3::NEG_X, Vec3::Y),
            // From above and below, the north of the model is up in the picture.
            (Camera::Top, Vec3::NEG_Y, Vec3::Z),
        ];
        for (camera, forward, up) in views {
            let eye = scene_camera(camera, doc, &index);
            assert!(eye.ortho, "{camera:?}");
            assert_eq!(directions(&eye), (forward, up), "{camera:?}");
        }
        let bottom = scene_camera(Camera::Bottom, doc, &index);
        assert_eq!(bottom.forward().round(), Vec3::Y);
    }

    #[test]
    fn the_iso_view_looks_from_above_at_the_corner_of_the_north_and_east_faces() {
        let core = cube();
        let eye = scene_camera(Camera::Iso, core.doc(), &index(core.doc()));
        // The camera stands towards the east (+X), above, and to the north (+Z of the scene).
        let from = -eye.forward();
        assert!(from.x > 0.5 && from.z > 0.5 && from.y > 0.4, "{from}");
        assert!(
            (from.y - ISO_PITCH.to_radians().sin()).abs() < 1e-4,
            "{from}"
        );
        assert!((from.x - from.z).abs() < 1e-4, "{from}");
        assert!(eye.up().y > 0.5);
    }

    #[test]
    fn the_orthographic_views_of_a_model_share_one_scale_and_centre() {
        let core = cube();
        let doc = core.doc();
        let index = index(doc);
        let element = ObjRef::Element(doc.elements.ids()[0]);
        let centre = index.bounds_of(&[element]).unwrap().center();
        let size = scene_camera(Camera::Front, doc, &index).ortho_size;
        // The new cube is one unit wide: half its diagonal is 0.87, and the frame has a margin.
        assert!(size > 0.9 && size < 1.1, "the box is framed: {size}");
        for camera in Camera::ALL.into_iter().filter(|c| *c != Camera::Gui) {
            let eye = scene_camera(camera, doc, &index);
            assert!((eye.ortho_size - size).abs() < 1e-4, "{camera:?}");
            // The line of sight of every view passes through the middle of the model.
            let sight = eye.forward().cross(centre - eye.pos);
            assert!(sight.length() < 1e-3, "{camera:?}: {sight}");
        }
    }

    #[test]
    fn the_inventory_camera_follows_the_gui_slot() {
        let mut doc = Document::new(cuboid_model::TextureList::default());
        // An untouched slot is the camera of the game: the 16-unit slot fills the picture.
        let eye = slot_camera(&doc);
        assert!(eye.ortho);
        assert!((eye.ortho_size - 8.0).abs() < 1e-4);
        assert!((eye.forward() - Vec3::Z).length() < 1e-5);
        assert!((eye.pos - Vec3::new(8.0, 8.0, -8.0 - GUI_DISTANCE)).length() < 1e-4);

        // A model scaled by 0.5 and turned a quarter about Y shows a slot twice as large, seen
        // from the side of the model.
        {
            let slot = &mut std::sync::Arc::make_mut(&mut doc.display)[DisplaySlotId::Gui];
            slot.scale = Vec3::splat(0.5);
            slot.rotation = Vec3::new(0.0, 90.0, 0.0);
        }
        let eye = slot_camera(&doc);
        assert!((eye.ortho_size - 16.0).abs() < 1e-3, "{}", eye.ortho_size);
        assert!(eye.forward().z.abs() < 1e-4 && eye.forward().x.abs() > 0.99);
        // The model's centre stays on the line of sight.
        let across = eye.forward().cross(SLOT_CENTRE - eye.pos);
        assert!(across.length() < 1e-3, "{across}");
    }
}
