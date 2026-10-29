//! Headless render tests on the software device the shell uses (WARP, lavapipe): they render
//! small scenes and read the pixels back.
//!
//! Without an adapter a test is skipped with a notice; under `CI` it fails instead, so a green run
//! means that pixels were checked.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixel arithmetic of the tests"
)]

use std::io::Write as _;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use cuboid_model::{MeshGeometry, ObjId};
use cuboid_scene::{
    AtlasUpdate, Camera, ChunkMesh, ChunkRange, DrawItem, DrawTarget, ElementInstance, FrameKey,
    GizmoKind, InstanceUpdate, MeshUpdate, OverlayItem, OverlayKind, RenderExtract, ViewDrawList,
};
use cuboid_voxel::{Rev, VoxelFace};
use egui_wgpu::{RenderState, RendererOptions, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use glam::{Mat4, Quat, Vec3};

use crate::look::{block_lit, item_lit, level_lights};
use crate::{
    AdapterCaps, DeviceWatch, ItemLight, PngJob, RenderSettings, Renderer, ShadingMode,
    UploadBudget, ViewInput, ViewParams, ViewTexture,
};

/// Creating an instance and a device on two threads at once crashes the process on Windows
/// (see the shell), and the tests run in parallel.
static GPU_INIT: Mutex<()> = Mutex::new(());

/// Size of the atlas pages in the tests.
const ATLAS: u32 = 256;

struct TestGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    caps: AdapterCaps,
}

fn software_first(
    adapters: &[wgpu::Adapter],
    _surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    // CUBOID_RENDER_HARDWARE=1 benchmarks on the first real GPU instead.
    let wanted: fn(wgpu::DeviceType) -> bool =
        if std::env::var_os("CUBOID_RENDER_HARDWARE").is_some() {
            |t| t != wgpu::DeviceType::Cpu
        } else {
            |t| t == wgpu::DeviceType::Cpu
        };
    adapters
        .iter()
        .find(|adapter| wanted(adapter.get_info().device_type))
        .or_else(|| adapters.first())
        .cloned()
        .ok_or_else(|| "no adapter found".to_owned())
}

/// A device like the shell's headless one, or `None` with a notice when there is no adapter.
fn test_gpu() -> Option<TestGpu> {
    let ci = std::env::var_os("CI").is_some();
    let setup = WgpuSetupCreateNew {
        native_adapter_selector: Some(Arc::new(software_first)),
        ..WgpuSetupCreateNew::without_display_handle()
    };
    let config = WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..WgpuConfiguration::default()
    };
    let options = RendererOptions {
        msaa_samples: 1,
        depth_stencil_format: None,
        dithering: false,
        predictable_texture_filtering: false,
    };
    let state = {
        let _turn = GPU_INIT.lock().unwrap_or_else(PoisonError::into_inner);
        let instance = pollster::block_on(config.wgpu_setup.new_instance());
        pollster::block_on(RenderState::create(&config, &instance, None, options))
    };
    match state {
        Ok(state) => {
            let info = state.adapter.get_info();
            let _ = writeln!(
                std::io::stderr(),
                "render test adapter: {} ({:?}, {:?})",
                info.name,
                info.device_type,
                info.backend
            );
            let caps = AdapterCaps::detect(&state.adapter, &state.device);
            Some(TestGpu {
                device: state.device.clone(),
                queue: state.queue.clone(),
                caps,
            })
        }
        Err(err) => {
            assert!(!ci, "no wgpu adapter under CI: {err}");
            let _ = writeln!(std::io::stderr(), "SKIPPED (no wgpu adapter): {err}");
            None
        }
    }
}

/// The settings of the tests that check Cubik's lighting.
fn cubik() -> RenderSettings {
    RenderSettings {
        shading: ShadingMode::Cubik,
        ..RenderSettings::default()
    }
}

/// A renderer with small atlas pages on a test device, in the Cubik shading mode.
fn setup() -> Option<(TestGpu, Renderer)> {
    setup_with(cubik())
}

fn setup_with(settings: RenderSettings) -> Option<(TestGpu, Renderer)> {
    let gpu = test_gpu()?;
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue, &gpu.caps);
    renderer.set_settings(settings);
    Some((gpu, renderer))
}

/// An element instance with the same colour on all faces unless `colors` says otherwise.
struct Cube {
    pos: [f32; 3],
    size: [f32; 3],
    colors: [u32; 6],
    uv: [f32; 4],
    flags: u32,
    rot: Quat,
}

const WHITE: u32 = 0xFFFF_FFFF;

/// Colour words are RGBA8 in byte order.
const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([r, g, b, a])
}

/// The atlas-space rectangle (V up, page rows run bottom to top) of the solid 32 x 32 block at the
/// origin of page 0, kept one texel away from its border.
const SOLID: [f32; 4] = [1.0 / 256.0, 1.0 / 256.0, 31.0 / 256.0, 31.0 / 256.0];

impl Cube {
    fn new(pos: [f32; 3], size: [f32; 3]) -> Cube {
        Cube {
            pos,
            size,
            colors: [WHITE; 6],
            uv: SOLID,
            flags: 0xF,
            rot: Quat::IDENTITY,
        }
    }

    fn colored(mut self, color: u32) -> Cube {
        self.colors = [color; 6];
        self
    }

    fn rotated(mut self, rot: Quat) -> Cube {
        self.rot = rot;
        self
    }

    /// The element has `shade` false (bit 7 of the flags word).
    fn unshaded(mut self) -> Cube {
        self.flags |= 0x80;
        self
    }

    fn record(&self) -> ElementInstance {
        let mut w = [0u32; 44];
        for i in 0..3 {
            w[i] = self.pos[i].to_bits();
            w[4 + i] = self.size[i].to_bits();
        }
        w[3] = self.flags;
        for (i, v) in self.rot.to_array().into_iter().enumerate() {
            w[8 + i] = v.to_bits();
        }
        for face in 0..6 {
            for (k, v) in self.uv.iter().enumerate() {
                w[12 + face * 4 + k] = v.to_bits();
            }
            w[36 + face] = self.colors[face];
        }
        w[42] = 0x0101_0101;
        w[43] = 0x0101;
        ElementInstance(w)
    }
}

/// The atlas update that paints the solid block.
fn solid_block(rgba: [u8; 4]) -> AtlasUpdate {
    let pixels: Vec<u8> = rgba.iter().copied().cycle().take(32 * 32 * 4).collect();
    AtlasUpdate {
        page: 0,
        x: 0,
        y: 0,
        w: 32,
        h: 32,
        rgba: pixels.into(),
    }
}

fn extract(cubes: &[Cube]) -> RenderExtract {
    RenderExtract {
        instance_count: cubes.len() as u32,
        instances: cubes
            .iter()
            .enumerate()
            .map(|(i, c)| InstanceUpdate {
                slot: i as u32,
                instance: c.record(),
            })
            .collect(),
        atlas: vec![solid_block([255, 255, 255, 255])],
        atlas_pages: 1,
        atlas_page_size: ATLAS,
        ..RenderExtract::default()
    }
}

fn run(first: u32, count: u32, translucent: bool) -> DrawItem {
    DrawItem {
        target: DrawTarget::Elements { first, count },
        translucent,
    }
}

fn list(scene: u64, items: Vec<DrawItem>) -> ViewDrawList {
    ViewDrawList {
        key: FrameKey {
            scene,
            ..FrameKey::default()
        },
        voxel_ranges: Default::default(),
        items,
        overlays: Vec::new(),
        outline: Vec::new(),
    }
}

const SIZE: (u32, u32) = (96, 96);

/// A perspective camera at `pos` looking at `target`.
fn camera(pos: Vec3, target: Vec3) -> ViewParams {
    camera_sized(pos, target, SIZE)
}

fn camera_sized(pos: Vec3, target: Vec3, size: (u32, u32)) -> ViewParams {
    let rot = Quat::from_rotation_arc(Vec3::Z, (target - pos).normalize());
    let cam = Camera {
        pos,
        rot,
        ortho: false,
        ortho_size: 8.0,
        fov_deg: 64.0,
        near: 0.3,
        far: 128.0,
    };
    ViewParams::from_camera(&cam, 3, size)
}

/// The pixel (column, row from the top) a world point lands on.
fn project(params: &ViewParams, p: Vec3) -> (u32, u32) {
    let clip = params.view_proj * p.extend(1.0);
    let (x, y) = (clip.x / clip.w, clip.y / clip.w);
    let px = ((x * 0.5 + 0.5) * params.size.0 as f32) as u32;
    let py = ((0.5 - y * 0.5) * params.size.1 as f32) as u32;
    (px, py)
}

/// The tight RGBA bytes of a texture, rows top to bottom.
fn read_texture(gpu: &TestGpu, texture: &wgpu::Texture, size: (u32, u32)) -> Vec<u8> {
    let padded = wgpu::util::align_to(size.0 * 4, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test_readback"),
        size: u64::from(padded) * u64::from(size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    enc.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    rx.recv().expect("map").expect("mapped");
    let data = buffer.slice(..).get_mapped_range().expect("range").to_vec();
    let row = size.0 as usize * 4;
    let mut out = Vec::with_capacity(row * size.1 as usize);
    for y in 0..size.1 as usize {
        out.extend_from_slice(&data[y * padded as usize..y * padded as usize + row]);
    }
    out
}

/// Renders the Main view and returns its pixels.
fn render(
    gpu: &TestGpu,
    renderer: &mut Renderer,
    params: ViewParams,
    list: &ViewDrawList,
) -> (Vec<u8>, [ViewTexture; 4]) {
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let views = [None, None, None, Some(ViewInput { params, list })];
    let textures = renderer.render_views(&mut enc, &views);
    gpu.queue.submit([enc.finish()]);
    let pixels = read_texture(gpu, &textures[3].texture, textures[3].size);
    (pixels, textures)
}

fn px(pixels: &[u8], at: (u32, u32)) -> [u8; 4] {
    let i = (at.1 * SIZE.0 + at.0) as usize * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn close(got: [u8; 4], want: [u8; 4], tolerance: u8) {
    for (g, w) in got.iter().zip(want) {
        assert!(g.abs_diff(w) <= tolerance, "got {got:?}, wanted {want:?}");
    }
}

fn byte(v: f32) -> u8 {
    (v * 255.0).round() as u8
}

/// An opaque grey of the given brightness.
fn gray(v: f32) -> [u8; 4] {
    [byte(v), byte(v), byte(v), 255]
}

#[test]
fn one_cube_shows_its_three_visible_faces_lit() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let mut cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    // Faces down, up, north, south, west, east.
    cube.colors = [
        rgba(255, 255, 0, 255),
        rgba(255, 0, 0, 255),
        rgba(0, 255, 255, 255),
        rgba(0, 255, 0, 255),
        rgba(0, 0, 255, 255),
        rgba(255, 0, 255, 255),
    ];
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    // From the south-west above: up, south and west are visible.
    let params = camera(Vec3::new(-5.0, 5.0, -7.0), Vec3::ZERO);
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));

    let up = project(&params, Vec3::new(0.0, 1.0, 0.0));
    let south = project(&params, Vec3::new(0.0, 0.0, -1.0));
    let west = project(&params, Vec3::new(-1.0, 0.0, 0.0));
    close(px(&pixels, up), [byte(0.961), 0, 0, 255], 2);
    close(px(&pixels, south), [0, byte(0.859), 0, 255], 2);
    close(px(&pixels, west), [0, 0, byte(0.720), 255], 2);
    // The background is the clear colour, alpha 1.
    close(px(&pixels, (0, 0)), [52, 52, 52, 255], 0);
}

/// The extract the scene extractor makes of one white cube whose faces carry the tints 0 to 5
/// (down, up, north, south, west, east).
fn tinted_cube(p: Vec3, s: Vec3) -> RenderExtract {
    let mut buf = cuboid_image::RgbaBuf::new(16, 16);
    buf.pixels.fill(255);
    let mut textures = cuboid_model::TextureList::default();
    textures.entries.push(cuboid_model::TextureEntry {
        label: cuboid_model::Label::from("white"),
        texture: cuboid_model::Texture::new(Arc::new(cuboid_image::Image::from_rgba(buf))),
        animation: None,
    });
    let mut doc = cuboid_model::Document::new(textures);
    let mut el = cuboid_model::Element::new(
        cuboid_model::ElementType::Cube,
        &doc.blueprint,
        1,
        [1.0; 4],
        &cuboid_model::Label::from("white"),
    );
    el.base.p = p;
    el.base.s = s;
    for (face, tint) in el.faces.iter_mut().zip(0..) {
        face.tint = tint;
    }
    doc.elements.push(el);
    cuboid_scene::Extractor::default().extract(
        &mut doc,
        &cuboid_model::RenderPrefs::default(),
        &cuboid_scene::ExtractBudget::UNLIMITED,
    )
}

/// A document with one cube whose faces use a strip of two frames of 16 x 16 texels: the first
/// is red, the second green.
fn strip_cube() -> cuboid_model::Document {
    let mut buf = cuboid_image::RgbaBuf::new(16, 32);
    // Rows run bottom to top, so the first frame is the upper half of the data.
    for (i, texel) in buf.pixels.chunks_exact_mut(4).enumerate() {
        let first = i >= 16 * 16;
        texel.copy_from_slice(if first {
            &[255, 0, 0, 255]
        } else {
            &[0, 255, 0, 255]
        });
    }
    let mut textures = cuboid_model::TextureList::default();
    let mut animation = cuboid_model::TextureAnimation::new(16, 16);
    animation.frametime = 5;
    textures.entries.push(cuboid_model::TextureEntry {
        label: cuboid_model::Label::from("water"),
        texture: cuboid_model::Texture::new(Arc::new(cuboid_image::Image::from_rgba(buf))),
        animation: Some(animation),
    });
    let mut doc = cuboid_model::Document::new(textures);
    let mut el = cuboid_model::Element::new(
        cuboid_model::ElementType::Cube,
        &doc.blueprint,
        1,
        [1.0; 4],
        &cuboid_model::Label::from("water"),
    );
    el.base.p = Vec3::new(-1.0, -1.0, 1.0);
    el.base.s = Vec3::splat(2.0);
    doc.elements.push(el);
    doc
}

#[test]
fn a_face_on_a_strip_shows_the_tile_of_the_tick_and_only_the_frame_is_uploaded() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let mut doc = strip_cube();
    let mut extractor = cuboid_scene::Extractor::default();
    let prefs = cuboid_model::RenderPrefs::default();
    let budget = cuboid_scene::ExtractBudget::UNLIMITED;
    let first = extractor.extract(&mut doc, &prefs, &budget);
    // The error texture, the white texel for faces without a texture and one frame of 16 x 16
    // texels with its ring, not the strip.
    let sizes: Vec<(u32, u32)> = first.atlas.iter().map(|u| (u.w, u.h)).collect();
    assert_eq!(sizes, [(18, 18), (3, 3), (18, 18)]);
    r.sync(&gpu.queue, &first, UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let (before, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));
    let lit = 0.859;
    close(px(&before, (48, 48)), [byte(lit), 0, 0, 255], 2);

    // Tick 5 starts the second step: one rectangle, the size of a frame, is sent.
    extractor.set_texture_tick(5);
    let next = extractor.extract(&mut doc, &prefs, &budget);
    assert_eq!(next.atlas.len(), 1);
    assert_eq!((next.atlas[0].w, next.atlas[0].h), (18, 18));
    assert!(next.instances.is_empty());
    r.sync(&gpu.queue, &next, UploadBudget::default());
    let (after, _) = render(&gpu, &mut r, params, &list(2, vec![run(0, 1, false)]));
    close(px(&after, (48, 48)), [0, byte(lit), 0, 255], 2);
}

/// How many pixels of the picture show each of the tints 0 to 5. Lighting scales a colour's
/// channels together, so a pixel counts for the tint whose channels it is closest to in ratio.
fn tint_pixels(pixels: &[u8]) -> [usize; 6] {
    const TINTS: [[f32; 3]; 6] = [
        [106.0, 170.0, 64.0],
        [255.0, 105.0, 180.0],
        [100.0, 149.0, 237.0],
        [255.0, 215.0, 0.0],
        [210.0, 105.0, 30.0],
        [72.0, 209.0, 204.0],
    ];
    let unit = |c: [f32; 3]| {
        let n = c.iter().map(|v| v * v).sum::<f32>().sqrt();
        c.map(|v| v / n)
    };
    let mut counts = [0; 6];
    for texel in pixels.chunks_exact(4) {
        if texel[..3] == [52, 52, 52] {
            continue;
        }
        let c = unit([texel[0] as f32, texel[1] as f32, texel[2] as f32]);
        let distance = |tint: &[f32; 3]| {
            let t = unit(*tint);
            (0..3).map(|i| (c[i] - t[i]).powi(2)).sum::<f32>()
        };
        let nearest = (0..6).min_by(|&a, &b| distance(&TINTS[a]).total_cmp(&distance(&TINTS[b])));
        if let Some(tint) = nearest {
            counts[tint] += 1;
        }
    }
    counts
}

#[test]
fn an_inverted_cube_shows_its_far_inner_faces_and_hides_the_near_ones() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // The same box [-1, 1]^3 twice: a normal cube, and one whose x size is negative, so its
    // position is the high end of x. From the south-west above, a normal cube shows up, south and
    // west. The mirror swaps the west and east slots between the sides and reverses the winding
    // of every face, so culling hides the three near sides and shows the inside of the far ones:
    // the down and north slots and the west slot, now at the east side.
    let params = camera(Vec3::new(-5.0, 5.0, -7.0), Vec3::ZERO);
    let mut scene = 0;
    let mut seen = |p: Vec3, s: Vec3| {
        scene += 1;
        r.sync(&gpu.queue, &tinted_cube(p, s), UploadBudget::default());
        let view = list(scene, vec![run(0, 1, false)]);
        tint_pixels(&render(&gpu, &mut r, params, &view).0)
    };
    let normal = seen(Vec3::new(-1.0, -1.0, 1.0), Vec3::splat(2.0));
    let (down, up, north, south, west, east) = (0, 1, 2, 3, 4, 5);
    for tint in [up, south, west] {
        assert!(normal[tint] > 100, "a normal cube shows {tint}: {normal:?}");
    }
    for tint in [down, north, east] {
        assert_eq!(normal[tint], 0, "a normal cube hides {tint}: {normal:?}");
    }
    let inverted = seen(Vec3::new(1.0, -1.0, 1.0), Vec3::new(-2.0, 2.0, 2.0));
    for tint in [down, north, west] {
        assert!(
            inverted[tint] > 100,
            "an inverted cube shows {tint}: {inverted:?}"
        );
    }
    for tint in [up, south, east] {
        assert_eq!(
            inverted[tint], 0,
            "an inverted cube hides {tint}: {inverted:?}"
        );
    }
}

#[test]
fn the_picture_is_upright_and_the_world_is_left_handed() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // A cube above and to the right (+X) of the view axis.
    let cube = Cube::new([2.0, 2.0, 5.0], [1.0, 1.0, 1.0]).colored(rgba(255, 255, 255, 255));
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -4.0), Vec3::new(0.0, 0.0, 0.0));
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));
    let white = |p: [u8; 4]| p[0] > 150;
    let mut seen = None;
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            if white(px(&pixels, (x, y))) {
                seen = Some((x, y));
            }
        }
    }
    let (x, y) = seen.expect("the cube is visible");
    assert!(
        y < SIZE.1 / 2,
        "above the axis means the upper half, row {y}"
    );
    assert!(x > SIZE.0 / 2, "+X is on the right, column {x}");
}

#[test]
fn an_atlas_update_repaints_without_touching_the_instances() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let (before, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));
    close(px(&before, (48, 48)), gray(0.859), 2);

    // A painted 4 x 4 rectangle changes the block; the cube uses its top-left corner.
    let patch = AtlasUpdate {
        page: 0,
        x: 0,
        y: 0,
        w: 32,
        h: 32,
        rgba: [10u8, 20, 30, 255].repeat(32 * 32).into(),
    };
    let update = RenderExtract {
        instance_count: 1,
        atlas: vec![patch],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &update, UploadBudget::default());
    let (after, _) = render(&gpu, &mut r, params, &list(2, vec![run(0, 1, false)]));
    let lit = 0.859;
    close(
        px(&after, (48, 48)),
        [
            byte(10.0 / 255.0 * lit),
            byte(20.0 / 255.0 * lit),
            byte(30.0 / 255.0 * lit),
            255,
        ],
        2,
    );
}

#[test]
fn a_voxel_face_is_lit_and_shaded_by_its_ao_codes() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // Two faces toward +Z in chunk 0: a plain one and one with AO code 2 on all corners.
    let plain = VoxelFace::new([1, 1, 0], [0, 0, 0], 0x00FF00, [0; 4]);
    let dark = VoxelFace::new([3, 1, 0], [0, 0, 0], 0x00FF00, [2; 4]);
    let mut faces: [Vec<VoxelFace>; 6] = Default::default();
    faces[2] = vec![plain, dark];
    let ex = RenderExtract {
        chunks: vec![ChunkMesh {
            slot: 0,
            rev: Rev::next(),
            offsets: [0; 6],
            faces,
        }],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let mut view = list(1, Vec::new());
    view.voxel_ranges[2] = vec![ChunkRange {
        slot: 0,
        first: 0,
        count: 2,
    }];
    // The faces sit at z = 16 with x in [-15, -14] and [-13, -12]; look at them from +Z.
    let params = camera(Vec3::new(-13.5, -14.5, 21.0), Vec3::new(-13.5, -14.5, 16.0));
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    let lit = 0.805;
    let plain_at = project(&params, Vec3::new(-14.5, -14.5, 16.0));
    let dark_at = project(&params, Vec3::new(-12.5, -14.5, 16.0));
    close(px(&pixels, plain_at), [0, byte(lit), 0, 255], 2);
    close(px(&pixels, dark_at), [0, byte(lit * 0.8), 0, 255], 2);
}

#[test]
fn translucent_cubes_blend_in_the_given_order() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // Alpha 0.4 writes no depth in the pre-pass, so the order of the queue shows.
    let far = Cube::new([-1.0, -1.0, 3.0], [2.0, 2.0, 2.0]).colored(rgba(255, 0, 0, 102));
    let near = Cube::new([-1.0, -1.0, 0.0], [2.0, 2.0, 1.0]).colored(rgba(0, 0, 255, 102));
    r.sync(&gpu.queue, &extract(&[far, near]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -6.0), Vec3::ZERO);
    let centre = (48, 48);

    let (sorted, _) = render(
        &gpu,
        &mut r,
        params,
        &list(1, vec![run(0, 1, true), run(1, 1, true)]),
    );
    let (reversed, _) = render(
        &gpu,
        &mut r,
        params,
        &list(2, vec![run(1, 1, true), run(0, 1, true)]),
    );

    // Back to front: red over the clear colour, then blue over that. 0.4 is 102/255.
    let a = 102.0 / 255.0;
    let lit = 0.859;
    let over = |dst: [f32; 3], src: [f32; 3]| [0, 1, 2].map(|i| src[i] * a + dst[i] * (1.0 - a));
    let clear = [52.0 / 255.0; 3];
    let red = over(clear, [lit, 0.0, 0.0]);
    let both = over(red, [0.0, 0.0, lit]);
    let want = [byte(both[0]), byte(both[1]), byte(both[2]), 255];
    close(px(&sorted, centre), want, 3);
    // The wrong order puts red over blue: a visibly different mix.
    let blue = over(clear, [0.0, 0.0, lit]);
    let wrong = over(blue, [lit, 0.0, 0.0]);
    close(
        px(&reversed, centre),
        [byte(wrong[0]), byte(wrong[1]), byte(wrong[2]), 255],
        3,
    );
    assert!(px(&sorted, centre)[0].abs_diff(px(&reversed, centre)[0]) > 20);
}

#[test]
fn a_selected_cube_gets_an_outline_band_outside_its_silhouette() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let cube = Cube::new([-0.5, -0.5, 0.5], [1.0, 1.0, 1.0]);
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -4.0), Vec3::ZERO);
    let mut view = list(1, vec![run(0, 1, false)]);
    view.outline = vec![run(0, 1, false)];
    let (pixels, _) = render(&gpu, &mut r, params, &view);

    // Find the left edge of the cube along the middle row.
    let row = 48;
    let edge = (0..SIZE.0)
        .find(|&x| px(&pixels, (x, row))[0] > 150 && px(&pixels, (x, row))[1] > 150)
        .expect("the cube");
    // Inside the silhouette the cube is unchanged, a pixel or two outside it is orange.
    close(px(&pixels, (edge + 2, row)), gray(0.859), 3);
    let outside = px(&pixels, (edge - 2, row));
    assert!(
        outside[0] > 200 && outside[1] < 150 && outside[2] < 60,
        "{outside:?}"
    );
    // Far from the cube nothing changes.
    close(px(&pixels, (2, 2)), [52, 52, 52, 255], 0);
}

#[test]
fn the_png_render_reads_back_transparent_background_bottom_up() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // A cube in the upper half of the picture.
    let cube = Cube::new([-0.5, 0.5, 0.5], [1.0, 1.0, 1.0]).colored(rgba(255, 0, 0, 255));
    let ex = extract(&[cube]);
    let params = {
        let cam = Camera {
            pos: Vec3::new(0.0, 0.0, -4.0),
            rot: Quat::IDENTITY,
            ortho: false,
            ortho_size: 8.0,
            fov_deg: 64.0,
            near: 0.3,
            far: 128.0,
        };
        ViewParams::from_camera(&cam, 3, (64, 64))
    };
    for antialias in [false, true] {
        let job = PngJob {
            extract: ex.clone(),
            list: list(1, vec![run(0, 1, false)]),
            camera: params,
            antialias,
        };
        let mut enc = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let id = r.start_png_render(&mut enc, &job);
        gpu.queue.submit([enc.finish()]);
        let started = Instant::now();
        let image = loop {
            let done = r.poll_readbacks(&gpu.device);
            if let Some((got, result)) = done.into_iter().next() {
                assert_eq!(got, id);
                break result.expect("the render succeeds");
            }
            assert!(
                started.elapsed().as_secs() < 30,
                "the read-back never finished"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!((image.w, image.h), (64, 64));
        // Rows run bottom to top: the cube is up, so it sits in the high rows.
        let top = image.get(32, 43).expect("pixel");
        let bottom = image.get(32, 12).expect("pixel");
        assert!(top[0] > 150 && top[3] == 255, "cube at the top: {top:?}");
        assert_eq!(bottom, [0, 0, 0, 0], "transparent background below");
        assert_eq!(image.get(2, 2), Some([0, 0, 0, 0]));
    }
}

#[test]
fn a_rebuilt_device_renders_the_same_picture_from_the_shadows() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let (before, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));

    let Some(second) = test_gpu() else {
        return;
    };
    r.rebuild(
        &second.device,
        &second.queue,
        &second.caps,
        &DeviceWatch::new(&second.device),
    );
    // The extract is not sent again: only `sync` drains the queue of the shadows.
    let nothing_new = RenderExtract {
        instance_count: 1,
        ..RenderExtract::default()
    };
    r.sync(&second.queue, &nothing_new, UploadBudget::default());
    assert!(!r.device_lost());
    let (after, _) = render(&second, &mut r, params, &list(1, vec![run(0, 1, false)]));
    let differing = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert_eq!(differing, 0, "{differing} bytes differ");
}

#[test]
fn a_small_upload_budget_spreads_the_chunks_over_several_syncs() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let face = VoxelFace::new([0, 0, 0], [0, 0, 0], 0xFFFFFF, [0; 4]);
    let chunks: Vec<ChunkMesh> = (0..8)
        .map(|slot| {
            let mut faces: [Vec<VoxelFace>; 6] = Default::default();
            faces[2] = vec![face; 100];
            ChunkMesh {
                slot,
                rev: Rev::next(),
                offsets: [0, 0, slot as u32 * 100, 0, 0, 0],
                faces,
            }
        })
        .collect();
    let ex = RenderExtract {
        chunks,
        ..RenderExtract::default()
    };
    let budget = UploadBudget {
        bytes: 1000,
        millis: 1000.0,
    };
    r.sync(&gpu.queue, &ex, budget);
    let after_first = r.pending_upload();
    assert!(after_first > 0, "the leftovers carry over");
    let mut rounds = 0;
    while r.pending_upload() > 0 {
        r.sync(&gpu.queue, &RenderExtract::default(), budget);
        rounds += 1;
        assert!(rounds < 20, "the queue drains");
    }
    assert_eq!(r.health().voxel_faces, 800);
}

#[test]
fn a_tight_memory_budget_drops_multisampling_and_still_renders() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    r.set_settings(RenderSettings {
        msaa: true,
        memory_budget: 1_000_000,
        ..cubik()
    });
    let cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));
    close(px(&pixels, (48, 48)), gray(0.859), 2);
    let health = r.health();
    assert!(health.degraded, "{health:?}");
    assert!(!r.settings().msaa);
}

#[test]
fn multisampled_rendering_resolves_to_the_same_flat_colours() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    r.set_settings(RenderSettings {
        msaa: true,
        ..cubik()
    });
    let cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));
    close(px(&pixels, (48, 48)), gray(0.859), 2);
    assert!(!r.health().degraded);
}

#[test]
fn overlays_draw_lines_and_the_sub_grid_adds_light() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    r.sync(
        &gpu.queue,
        &RenderExtract::default(),
        UploadBudget::default(),
    );
    let params = camera(Vec3::new(0.5, 6.0, -0.5), Vec3::new(0.5, 0.0, -0.5));
    let mut view = list(
        1,
        vec![DrawItem {
            target: DrawTarget::SubGrid(cuboid_scene::GridPlane::Floor),
            translucent: true,
        }],
    );
    view.overlays = vec![OverlayItem {
        kind: OverlayKind::Gizmo(GizmoKind::Move),
        transform: Mat4::from_translation(Vec3::new(0.5, 0.0, -0.5)),
    }];
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    // Somewhere the gizmo or the grid changed the clear colour.
    let changed = pixels
        .chunks_exact(4)
        .filter(|p| p[0] != 52 || p[1] != 52 || p[2] != 52)
        .count();
    assert!(changed > 20, "{changed} pixels changed");
}

#[test]
fn views_with_an_unchanged_key_keep_their_picture_and_hidden_views_stay_empty() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let view = list(7, vec![run(0, 1, false)]);
    let (_, first) = render(&gpu, &mut r, params, &view);
    let (_, second) = render(&gpu, &mut r, params, &view);
    assert_eq!(first[3].generation, second[3].generation);
    assert_eq!(
        first[0].size,
        (1, 1),
        "a view that was never shown is a placeholder"
    );
}

#[test]
fn a_mesh_draws_with_its_model_matrix_and_tint() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // A quad in the XY plane, wound to face -Z where the camera is, moved up by the model.
    let geometry = MeshGeometry {
        vertices: vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
        ],
        normals: None,
        uvs: Vec::new(),
        triangles: vec![0, 2, 1, 0, 3, 2],
    };
    let id = ObjId::next();
    let ex = RenderExtract {
        meshes: vec![MeshUpdate {
            id,
            geometry: Arc::new(geometry),
            texture: None,
            model: Mat4::from_translation(Vec3::new(0.0, 3.0, 0.0)),
            tint: [1.0, 0.5, 0.25, 1.0],
            hidden: false,
            submeshes: Arc::from(Vec::new()),
        }],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let params = camera(Vec3::new(0.0, 3.0, -5.0), Vec3::new(0.0, 3.0, 0.0));
    let item = DrawItem {
        target: DrawTarget::Mesh { id, submesh: 0 },
        translucent: false,
    };
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![item]));
    let lit = 0.859;
    close(
        px(&pixels, (48, 48)),
        [byte(lit), byte(0.5 * lit), byte(0.25 * lit), 255],
        3,
    );
    // Removing the mesh takes it out of the picture.
    let gone = RenderExtract {
        removed_meshes: vec![id],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &gone, UploadBudget::default());
    let (pixels, _) = render(&gpu, &mut r, params, &list(2, vec![item]));
    close(px(&pixels, (48, 48)), [52, 52, 52, 255], 0);
}

#[test]
fn a_hovered_cube_is_outlined_in_translucent_white() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let mut cube = Cube::new([-0.5, -0.5, 0.5], [1.0, 1.0, 1.0]);
    cube.flags |= 1 << 4;
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -4.0), Vec3::ZERO);
    let mut view = list(1, vec![run(0, 1, false)]);
    view.outline = vec![run(0, 1, false)];
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    let row = 48;
    let edge = (0..SIZE.0)
        .find(|&x| px(&pixels, (x, row))[0] > 150)
        .expect("the cube");
    // The first bright pixel is the outline band: two taps of alpha 0.5 give 1.0 of white.
    close(px(&pixels, (edge, row)), [255, 255, 255, 255], 3);
    close(px(&pixels, (edge - 1, row)), [52, 52, 52, 255], 0);
}

#[test]
fn a_destroyed_device_is_reported_and_a_new_one_recovers() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    gpu.device.destroy();
    let _ = gpu.device.poll(wgpu::PollType::Poll);
    assert!(r.device_lost());
    // Nothing panics while the device is gone, and the shadows keep following the scene.
    let keep = RenderExtract {
        instance_count: 1,
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &keep, UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let views = [
        None,
        None,
        None,
        Some(ViewInput {
            params,
            list: &list(1, vec![run(0, 1, false)]),
        }),
    ];
    let _ = r.render_views(&mut enc, &views);
    assert!(r.health().device_lost);

    let Some(second) = test_gpu() else {
        return;
    };
    r.rebuild(
        &second.device,
        &second.queue,
        &second.caps,
        &DeviceWatch::new(&second.device),
    );
    assert!(!r.device_lost());
    r.sync(&second.queue, &keep, UploadBudget::default());
    let (pixels, _) = render(&second, &mut r, params, &list(1, vec![run(0, 1, false)]));
    close(px(&pixels, (48, 48)), gray(0.859), 2);
}

#[test]
fn two_renderers_on_one_device_both_report_its_loss() {
    let Some(gpu) = test_gpu() else {
        return;
    };
    // The callbacks of a device are installed once; a renderer that installed its own would
    // replace the other's.
    let watch = DeviceWatch::new(&gpu.device);
    let first = Renderer::with_watch(&gpu.device, &gpu.queue, &gpu.caps, &watch);
    let second = Renderer::with_watch(&gpu.device, &gpu.queue, &gpu.caps, &watch);
    assert!(!first.device_lost() && !second.device_lost() && !watch.lost());

    gpu.device.destroy();
    let _ = gpu.device.poll(wgpu::PollType::Poll);
    assert!(first.device_lost(), "the first renderer");
    assert!(second.device_lost(), "the second renderer");
    assert!(watch.lost(), "the watch of the device");
    for renderer in [&first, &second] {
        let health = renderer.health();
        assert!(health.device_lost);
        assert!(
            health.last_error.is_some_and(|text| text.contains("lost")),
            "the loss is the last error"
        );
    }
}

#[test]
fn a_mesh_texture_above_the_device_limit_is_cut_down_and_the_mesh_still_draws() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let side = gpu.device.limits().max_texture_dimension_2d + 1;
    let mut buf = cuboid_image::RgbaBuf::new(side, 1);
    buf.pixels.fill(255);
    let (geometry, _) = quad_mesh();
    let source = (
        geometry,
        Some(Arc::new(cuboid_image::Image::from_rgba(buf))),
    );
    let id = ObjId::next();
    let ex = RenderExtract {
        meshes: vec![mesh_at(id, &source, Mat4::IDENTITY)],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![mesh_item(id)]));
    assert_eq!(r.health().last_error, None);
    close(px(&pixels, (48, 48)), gray(0.859), 3);
}

/// Renders up to four views in one submit and returns the view textures.
fn render_many(
    gpu: &TestGpu,
    renderer: &mut Renderer,
    views: &[Option<ViewInput<'_>>; 4],
) -> [ViewTexture; 4] {
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let textures = renderer.render_views(&mut enc, views);
    gpu.queue.submit([enc.finish()]);
    textures
}

const CLEAR: [u8; 4] = [52, 52, 52, 255];

fn is_clear(pixel: [u8; 4]) -> bool {
    pixel[..3]
        .iter()
        .zip(CLEAR)
        .all(|(a, b)| a.abs_diff(b) <= 1)
}

/// A voxel face in the pool of direction `dir` at `local` in chunk 0: its centre and normal.
fn voxel_quad(dir: usize, local: [u32; 3]) -> (Vec3, Vec3) {
    let quad = cuboid_voxel::expand_face(
        VoxelFace::new(local, [0; 3], 0, [0; 4]),
        cuboid_voxel::Dir::ALL[dir],
        1.0,
    );
    let centre: Vec3 = quad.iter().map(|v| v.pos).sum::<Vec3>() / 4.0;
    (centre, quad[0].normal)
}

#[test]
fn uploads_that_finish_in_later_frames_reach_an_unchanged_view() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // Two chunks in the +Z pool; a one-byte budget uploads one range per call.
    let at = |x: u32| VoxelFace::new([x, 1, 0], [0, 0, 0], 0x00FF00, [0; 4]);
    let chunk = |slot: usize, first: u32, x: u32| {
        let mut faces: [Vec<VoxelFace>; 6] = Default::default();
        faces[2] = vec![at(x)];
        ChunkMesh {
            slot,
            rev: Rev::next(),
            offsets: [0, 0, first, 0, 0, 0],
            faces,
        }
    };
    let ex = RenderExtract {
        chunks: vec![chunk(0, 0, 1), chunk(1, 1, 3)],
        ..RenderExtract::default()
    };
    let small = UploadBudget {
        bytes: 1,
        millis: 1000.0,
    };
    r.sync(&gpu.queue, &ex, small);
    assert!(r.needs_frame(), "the second chunk is still waiting");

    let mut view = list(1, Vec::new());
    view.voxel_ranges[2] = vec![
        ChunkRange {
            slot: 0,
            first: 0,
            count: 1,
        },
        ChunkRange {
            slot: 1,
            first: 1,
            count: 1,
        },
    ];
    let params = camera(Vec3::new(-13.5, -14.5, 21.0), Vec3::new(-13.5, -14.5, 16.0));
    let first_at = project(&params, Vec3::new(-14.5, -14.5, 16.0));
    let second_at = project(&params, Vec3::new(-12.5, -14.5, 16.0));
    let (before, _) = render(&gpu, &mut r, params, &view);
    assert!(!is_clear(px(&before, first_at)));
    assert!(is_clear(px(&before, second_at)));

    // Nothing new from the scene, the same key: the leftover still reaches the picture.
    let keep = RenderExtract::default();
    r.sync(&gpu.queue, &keep, small);
    assert!(!r.needs_frame());
    let (after, _) = render(&gpu, &mut r, params, &view);
    assert!(!is_clear(px(&after, first_at)));
    assert!(!is_clear(px(&after, second_at)));
}

#[test]
fn a_rebuilt_device_shows_all_six_voxel_directions_once_the_uploads_drained() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    // One face per direction side by side, and a textured cube above them.
    let mut faces: [Vec<VoxelFace>; 6] = Default::default();
    let mut ranges: [Vec<ChunkRange>; 6] = Default::default();
    let mut quads = Vec::new();
    for (dir, (pool, range)) in faces.iter_mut().zip(&mut ranges).enumerate() {
        let local = [2 * dir as u32, 5, 5];
        pool.push(VoxelFace::new(local, [0; 3], 0xFF8040, [0; 4]));
        range.push(ChunkRange {
            slot: 0,
            first: 0,
            count: 1,
        });
        quads.push(voxel_quad(dir, local));
    }
    let centre = quads.iter().map(|q| q.0).sum::<Vec3>() / 6.0;
    let cube_centre = centre + Vec3::new(0.0, 4.75, 0.0);
    let cube = Cube::new(
        [
            cube_centre.x - 0.75,
            cube_centre.y - 0.75,
            cube_centre.z + 0.75,
        ],
        [1.5, 1.5, 1.5],
    );
    let mut ex = extract(&[cube]);
    ex.chunks = vec![ChunkMesh {
        slot: 0,
        rev: Rev::next(),
        offsets: [0; 6],
        faces,
    }];
    r.sync(&gpu.queue, &ex, UploadBudget::default());

    let Some(second) = test_gpu() else {
        return;
    };
    r.rebuild(
        &second.device,
        &second.queue,
        &second.caps,
        &DeviceWatch::new(&second.device),
    );

    // Two cameras on opposite sides see three directions each.
    let offset = Vec3::new(8.0, 8.0, -8.0);
    let cams = [
        camera(centre + offset, centre),
        camera(centre - offset, centre),
    ];
    let lists = [
        {
            let mut l = list(1, vec![run(0, 1, false)]);
            l.voxel_ranges = ranges.clone();
            l
        },
        {
            let mut l = list(2, vec![run(0, 1, false)]);
            l.voxel_ranges = ranges.clone();
            l
        },
    ];
    let views = [
        Some(ViewInput {
            params: cams[0],
            list: &lists[0],
        }),
        Some(ViewInput {
            params: cams[1],
            list: &lists[1],
        }),
        None,
        None,
    ];
    // The scene sends nothing again; the shadows refill the new device a little at a time.
    let keep = RenderExtract {
        instance_count: 1,
        ..RenderExtract::default()
    };
    let small = UploadBudget {
        bytes: 1,
        millis: 1000.0,
    };
    let mut frames = 0;
    let textures = loop {
        r.sync(&second.queue, &keep, small);
        let textures = render_many(&second, &mut r, &views);
        frames += 1;
        if !r.needs_frame() {
            break textures;
        }
        assert!(frames < 40, "the uploads never drain");
    };
    assert!(frames > 1, "the rebuild takes several frames");
    for (i, cam) in cams.iter().enumerate() {
        let pixels = read_texture(&second, &textures[i].texture, textures[i].size);
        for (c, n) in &quads {
            let eye = if i == 0 {
                centre + offset
            } else {
                centre - offset
            };
            if n.dot(eye - *c) > 0.0 {
                assert!(
                    !is_clear(px(&pixels, project(cam, *c))),
                    "a face toward {n:?} is missing in view {i}"
                );
            }
        }
        assert!(
            !is_clear(px(&pixels, project(cam, cube_centre))),
            "the textured cube is missing in view {i}"
        );
    }
}

/// A 32 x 32 atlas block with a transparent ring around a solid 30 x 30 entry, as the scene
/// uploads a texture with its padding.
fn padded_block(rgba: [u8; 4]) -> AtlasUpdate {
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 1..31 {
        for x in 1..31 {
            let at = (y * 32 + x) * 4;
            pixels[at..at + 4].copy_from_slice(&rgba);
        }
    }
    AtlasUpdate {
        page: 0,
        x: 0,
        y: 0,
        w: 32,
        h: 32,
        rgba: pixels.into(),
    }
}

#[test]
fn multisampling_leaves_no_seam_at_the_edges_of_exact_entry_rectangles() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    r.set_settings(RenderSettings {
        msaa: true,
        ..cubik()
    });
    // The face rectangle is exactly the entry (texels 1 to 30), the ring around it transparent.
    let mut cube = Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0]);
    cube.uv = [1.0 / 256.0, 1.0 / 256.0, 31.0 / 256.0, 31.0 / 256.0];
    let mut ex = extract(&[cube]);
    ex.atlas = vec![padded_block([255, 255, 255, 255])];
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let params = camera(Vec3::new(-5.0, 5.0, -7.0), Vec3::ZERO);
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));

    // Along the crease between the south and the west face every pixel is lit: a discarded
    // sample would let the clear colour through.
    for step in 0..9 {
        let y = -0.5 + 0.15 * step as f32;
        let (x, row) = project(&params, Vec3::new(-1.0, y, -1.0));
        for col in x - 1..=x + 1 {
            let pixel = px(&pixels, (col, row));
            assert!(
                pixel[..3].iter().all(|&c| c > 170),
                "seam at ({col}, {row}): {pixel:?}"
            );
        }
    }
    // The outer edge of the silhouette is covered like any other multisampled edge.
    let top = project(&params, Vec3::new(0.0, 1.0, 0.0));
    assert!(!is_clear(px(&pixels, top)));
}

#[test]
fn the_gutter_copies_the_edge_texels_into_the_ring() {
    let mut page = vec![0u8; 8 * 8 * 4];
    // A 4 x 4 rectangle at (2, 2) with a distinct interior.
    for y in 3..5usize {
        for x in 3..5usize {
            let at = (y * 8 + x) * 4;
            page[at..at + 4].copy_from_slice(&[10 * x as u8, 10 * y as u8, 1, 255]);
        }
    }
    crate::atlas::fill_gutter(&mut page, 8, (2, 2), (4, 4));
    let texel = |x: usize, y: usize| {
        let at = (y * 8 + x) * 4;
        [page[at], page[at + 1], page[at + 2], page[at + 3]]
    };
    assert_eq!(texel(2, 3), texel(3, 3), "left side");
    assert_eq!(texel(5, 4), texel(4, 4), "right side");
    assert_eq!(texel(3, 2), texel(3, 3), "top side");
    assert_eq!(texel(4, 5), texel(4, 4), "bottom side");
    assert_eq!(texel(2, 2), texel(3, 3), "corner");
    assert_eq!(texel(5, 5), texel(4, 4), "corner");
    assert_eq!(texel(1, 3), [0; 4], "outside the rectangle");
}

/// A unit quad in the XY plane wound to face -Z, with a 2 x 2 white texture.
fn quad_mesh() -> (Arc<MeshGeometry>, Option<Arc<cuboid_image::Image>>) {
    let geometry = MeshGeometry {
        vertices: vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
        ],
        normals: None,
        uvs: vec![
            glam::Vec2::new(0.0, 0.0),
            glam::Vec2::new(1.0, 0.0),
            glam::Vec2::new(1.0, 1.0),
            glam::Vec2::new(0.0, 1.0),
        ],
        triangles: vec![0, 2, 1, 0, 3, 2],
    };
    let mut buf = cuboid_image::RgbaBuf::new(2, 2);
    buf.pixels.fill(255);
    (
        Arc::new(geometry),
        Some(Arc::new(cuboid_image::Image::from_rgba(buf))),
    )
}

fn mesh_at(
    id: ObjId,
    (geometry, texture): &(Arc<MeshGeometry>, Option<Arc<cuboid_image::Image>>),
    model: Mat4,
) -> MeshUpdate {
    MeshUpdate {
        id,
        geometry: Arc::clone(geometry),
        texture: texture.clone(),
        model,
        tint: [1.0; 4],
        hidden: false,
        submeshes: Arc::from(Vec::new()),
    }
}

fn mesh_item(id: ObjId) -> DrawItem {
    DrawItem {
        target: DrawTarget::Mesh { id, submesh: 0 },
        translucent: false,
    }
}

#[test]
fn selected_and_hovered_meshes_get_the_outline() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let id = ObjId::next();
    let ex = RenderExtract {
        meshes: vec![mesh_at(id, &quad_mesh(), Mat4::IDENTITY)],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let mut view = list(1, vec![mesh_item(id)]);
    view.outline = vec![mesh_item(id)];
    let (pixels, _) = render(&gpu, &mut r, params, &view);

    let row = 48;
    let edge = (0..SIZE.0)
        .find(|&x| !is_clear(px(&pixels, (x, row))))
        .expect("the outline or the mesh");
    // Selected: the orange band lies outside the silhouette, the mesh itself is unchanged.
    let outside = px(&pixels, (edge, row));
    assert!(
        outside[0] > 200 && outside[1] < 150 && outside[2] < 60,
        "{outside:?}"
    );
    close(px(&pixels, (48, 48)), gray(0.859), 3);
    close(px(&pixels, (2, 2)), CLEAR, 0);

    // Hovered: the same silhouette in translucent white.
    r.set_hovered_mesh(Some(id));
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    close(px(&pixels, (edge, row)), [255, 255, 255, 255], 3);
    close(px(&pixels, (48, 48)), gray(0.859), 3);
}

#[test]
fn moving_meshes_uploads_no_geometry_or_texture_and_never_drops_one_for_a_frame() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let source = quad_mesh();
    let (left, right) = (ObjId::next(), ObjId::next());
    let place = |x: f32| Mat4::from_translation(Vec3::new(x, 0.0, 0.0));
    let ex = RenderExtract {
        meshes: vec![
            mesh_at(left, &source, place(-3.0)),
            mesh_at(right, &source, place(3.0)),
        ],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -8.0), Vec3::ZERO);
    let view = list(1, vec![mesh_item(left), mesh_item(right)]);
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    let at = |x: f32| project(&params, Vec3::new(x, 0.0, 0.0));
    assert!(!is_clear(px(&pixels, at(-3.0))));
    assert!(!is_clear(px(&pixels, at(3.0))));
    let created = r.gpu.as_ref().map(|g| g.meshes.uploaded);
    assert!(created.is_some_and(|n| n > 0));

    // Both meshes move in one frame under a budget of one byte; the key stays the same.
    let moved = RenderExtract {
        meshes: vec![
            mesh_at(left, &source, place(-1.5)),
            mesh_at(right, &source, place(1.5)),
        ],
        ..RenderExtract::default()
    };
    let tiny = UploadBudget {
        bytes: 1,
        millis: 1000.0,
    };
    r.sync(&gpu.queue, &moved, tiny);
    assert_eq!(r.pending_upload(), 0, "a move queues no upload");
    assert_eq!(r.gpu.as_ref().map(|g| g.meshes.uploaded), created);
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    assert!(is_clear(px(&pixels, at(-3.0))));
    assert!(is_clear(px(&pixels, at(3.0))));
    assert!(!is_clear(px(&pixels, at(-1.5))));
    assert!(!is_clear(px(&pixels, at(1.5))));

    // A tint change is a move as well.
    let mut tinted = mesh_at(left, &source, place(-1.5));
    tinted.tint = [1.0, 0.0, 0.0, 1.0];
    let ex = RenderExtract {
        meshes: vec![tinted],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, tiny);
    assert_eq!(r.gpu.as_ref().map(|g| g.meshes.uploaded), created);
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    let pixel = px(&pixels, at(-1.5));
    assert!(pixel[0] > 150 && pixel[1] < 20, "{pixel:?}");
}

#[test]
fn a_changed_geometry_is_built_once_and_the_mesh_stays_visible() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let id = ObjId::next();
    let first = quad_mesh();
    r.sync(
        &gpu.queue,
        &RenderExtract {
            meshes: vec![mesh_at(id, &first, Mat4::IDENTITY)],
            ..RenderExtract::default()
        },
        UploadBudget::default(),
    );
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let view = list(1, vec![mesh_item(id)]);
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    assert!(!is_clear(px(&pixels, (48, 48))));

    // A different geometry is built next to the old one; two updates for the id build it once.
    let built = r.gpu.as_ref().map_or(0, |g| g.meshes.uploaded);
    let second = quad_mesh();
    let ex = RenderExtract {
        meshes: vec![
            mesh_at(id, &second, Mat4::IDENTITY),
            mesh_at(id, &second, Mat4::IDENTITY),
        ],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    assert_eq!(r.pending_upload(), 0);
    assert_eq!(r.gpu.as_ref().map(|g| g.meshes.uploaded), Some(2 * built));
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    assert!(!is_clear(px(&pixels, (48, 48))));
}

/// The scene of the benchmark: 10 000 cuboids on a 100 x 100 grid and 2 M voxel faces in 4096
/// chunks of 85 faces per direction. Returns the extract and the ranges of `every`-th chunk.
fn bench_scene(every: usize) -> (RenderExtract, [Vec<ChunkRange>; 6]) {
    let cubes: Vec<Cube> = (0..10_000)
        .map(|i| {
            let (x, z) = ((i % 100) as f32 * 1.5, (i / 100) as f32 * 1.5);
            Cube::new([x, 0.0, z + 1.0], [1.0, 1.0, 1.0])
        })
        .collect();
    let mut ex = extract(&cubes);
    let mut ranges: [Vec<ChunkRange>; 6] = Default::default();
    for slot in 0..4096usize {
        let mut faces: [Vec<VoxelFace>; 6] = Default::default();
        for (dir, list) in faces.iter_mut().enumerate() {
            list.extend((0..85u32).map(|i| {
                VoxelFace::new(
                    [i % 16, (i / 16) % 16, 0],
                    [slot as u32 % 64, (slot as u32 / 64) % 64, 0],
                    0x80A0C0,
                    [0; 4],
                )
            }));
            if slot % every == 0 {
                ranges[dir].push(ChunkRange {
                    slot: slot as u32,
                    first: slot as u32 * 85,
                    count: 85,
                });
            }
        }
        ex.chunks.push(ChunkMesh {
            slot,
            rev: Rev::next(),
            offsets: [slot as u32 * 85; 6],
            faces,
        });
    }
    ex.voxel_pools = [4096 * 85; 6];
    (ex, ranges)
}

/// The roadmap target: 16 ms for 10 k cuboids and 2 M voxel faces. Run it with
/// `CUBOID_RENDER_HARDWARE=1 cargo test -p cuboid-render --release -- --ignored --nocapture`.
/// It renders the scene at 1920 x 1080 with and without multisampling, with the chunks
/// contiguous and scattered, and as four 960 x 540 views.
#[test]
#[ignore = "benchmark: run in release mode on a real GPU"]
fn benchmark_ten_thousand_cuboids_and_two_million_voxel_faces() {
    let Some((gpu, mut r)) = setup() else {
        return;
    };
    let (ex, contiguous) = bench_scene(1);
    let started = Instant::now();
    r.sync(
        &gpu.queue,
        &ex,
        UploadBudget {
            bytes: usize::MAX,
            millis: f32::INFINITY,
        },
    );
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let upload = started.elapsed();
    let health = r.health();
    let _ = writeln!(
        std::io::stderr(),
        "benchmark scene: {} elements, {} voxel faces, upload {upload:?}",
        health.elements,
        health.voxel_faces,
    );
    let scattered: [Vec<ChunkRange>; 6] =
        std::array::from_fn(|dir| contiguous[dir].iter().step_by(2).cloned().collect());
    for (name, size, views, ranges, msaa) in [
        ("1080p contiguous", (1920, 1080), 1, &contiguous, false),
        ("1080p contiguous", (1920, 1080), 1, &contiguous, true),
        ("1080p scattered", (1920, 1080), 1, &scattered, false),
        ("1080p scattered", (1920, 1080), 1, &scattered, true),
        ("4 x 960x540 contiguous", (960, 540), 4, &contiguous, false),
        ("4 x 960x540 contiguous", (960, 540), 4, &contiguous, true),
    ] {
        r.set_settings(RenderSettings { msaa, ..cubik() });
        let params = camera_sized(
            Vec3::new(75.0, 60.0, -40.0),
            Vec3::new(75.0, 0.0, 75.0),
            size,
        );
        let mut view = list(1, vec![run(0, 10_000, false)]);
        view.voxel_ranges = ranges.clone();
        let mut times = Vec::new();
        let mut cpu: Vec<std::time::Duration> = Vec::new();
        for i in 0..14u64 {
            view.key.scene = i + 1;
            let t = Instant::now();
            let mut enc = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let input = Some(ViewInput {
                params,
                list: &view,
            });
            let mut slots: [Option<ViewInput<'_>>; 4] = [None, None, None, None];
            for slot in slots.iter_mut().take(views) {
                *slot = input;
            }
            let _ = r.render_views(&mut enc, &slots);
            let commands = enc.finish();
            cpu.push(t.elapsed());
            gpu.queue.submit([commands]);
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            // The first frames create the attachments and warm up the driver.
            if i >= 2 {
                times.push(t.elapsed());
            }
        }
        times.sort();
        cpu.sort();
        let _ = writeln!(
            std::io::stderr(),
            "benchmark {name}, MSAA {}: {} MiB on the device, frame median {:?}, worst {:?}, CPU encode median {:?}",
            if r.settings().msaa { "4x" } else { "off" },
            r.health().gpu_bytes >> 20,
            times[times.len() / 2],
            times[times.len() - 1],
            cpu[cpu.len() / 2],
        );
    }
}

// Shading modes. A white cube of edge 2 at the origin; the face colours are white unless a test
// says otherwise, so a pixel shows the light of its face directly.

/// The cube of the shading tests: [-1, 1] on every axis.
fn unit_cube() -> Cube {
    Cube::new([-1.0, -1.0, 1.0], [2.0, 2.0, 2.0])
}

/// The level lights in the internal axes (z flipped from Minecraft's).
fn internal_level_lights() -> [Vec3; 2] {
    level_lights().map(|l| Vec3::new(l.x, l.y, -l.z))
}

/// Renders `cube` once and returns the pixels.
fn shade_scene(
    gpu: &TestGpu,
    r: &mut Renderer,
    cube: Cube,
    params: ViewParams,
    scene: u64,
) -> Vec<u8> {
    r.sync(&gpu.queue, &extract(&[cube]), UploadBudget::default());
    render(gpu, r, params, &list(scene, vec![run(0, 1, false)])).0
}

#[test]
fn the_block_mode_lights_each_face_by_its_axis() {
    let Some((gpu, mut r)) = setup_with(RenderSettings::default()) else {
        return;
    };
    assert_eq!(r.settings().shading, ShadingMode::MinecraftBlock);
    // From the south-west above: up, south and west are visible.
    let mut cube = unit_cube();
    cube.colors = [
        rgba(255, 255, 0, 255),
        rgba(255, 0, 0, 255),
        rgba(0, 255, 255, 255),
        rgba(0, 255, 0, 255),
        rgba(0, 0, 255, 255),
        rgba(255, 0, 255, 255),
    ];
    let params = camera(Vec3::new(-5.0, 5.0, -7.0), Vec3::ZERO);
    let pixels = shade_scene(&gpu, &mut r, cube, params, 1);
    let at = |p: Vec3| px(&pixels, project(&params, p));
    close(at(Vec3::new(0.0, 1.0, 0.0)), [255, 0, 0, 255], 2);
    close(at(Vec3::new(0.0, 0.0, -1.0)), [0, byte(0.8), 0, 255], 2);
    close(at(Vec3::new(-1.0, 0.0, 0.0)), [0, 0, byte(0.6), 255], 2);

    // From the north-east below: down, north and east are visible.
    let params = camera(Vec3::new(5.0, -5.0, 7.0), Vec3::ZERO);
    let pixels = shade_scene(&gpu, &mut r, unit_cube(), params, 2);
    let at = |p: Vec3| px(&pixels, project(&params, p));
    close(at(Vec3::new(0.0, -1.0, 0.0)), gray(0.5), 2);
    close(at(Vec3::new(0.0, 0.0, 1.0)), gray(0.8), 2);
    close(at(Vec3::new(1.0, 0.0, 0.0)), gray(0.6), 2);
}

#[test]
fn a_face_turned_22_degrees_steps_in_the_block_mode_and_follows_the_normal_when_smooth() {
    let turn = Quat::from_rotation_x(22.5f32.to_radians());
    // The front face of the cube turned about the corner at its position.
    let centre = Vec3::new(-1.0, -1.0, 1.0) + turn * Vec3::new(1.0, 1.0, -2.0);
    let normal = turn * Vec3::NEG_Z;
    let params = camera(Vec3::new(centre.x, centre.y, -8.0), centre);
    let at = project(&params, centre);

    let Some((gpu, mut r)) = setup_with(RenderSettings::default()) else {
        return;
    };
    // The nearest axis of the normal is z: 0.8, exactly as an unturned face.
    let stepped = px(
        &shade_scene(&gpu, &mut r, unit_cube().rotated(turn), params, 1),
        at,
    );
    close(stepped, gray(0.8), 2);

    r.set_settings(RenderSettings {
        smooth_block_light: true,
        ..RenderSettings::default()
    });
    let smooth = px(
        &shade_scene(&gpu, &mut r, unit_cube().rotated(turn), params, 2),
        at,
    );
    let want = block_lit(normal, true);
    assert!(
        (want - 0.8).abs() > 0.05,
        "the smooth light moves off the step: {want}"
    );
    close(smooth, gray(want), 2);
}

#[test]
fn shade_false_is_unlit_in_the_minecraft_modes_and_ignored_by_cubik() {
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    // The south face has the internal normal -Z.
    let item = item_lit(Vec3::NEG_Z, internal_level_lights());
    for (shading, shaded, unshaded) in [
        (ShadingMode::MinecraftBlock, 0.8, 1.0),
        (ShadingMode::MinecraftItem, item, 1.0),
        (ShadingMode::Cubik, 0.859, 0.859),
        (ShadingMode::Flat, 1.0, 1.0),
    ] {
        let Some((gpu, mut r)) = setup_with(RenderSettings {
            shading,
            ..RenderSettings::default()
        }) else {
            return;
        };
        let on = shade_scene(&gpu, &mut r, unit_cube(), params, 1);
        close(px(&on, (48, 48)), gray(shaded), 2);
        let off = shade_scene(&gpu, &mut r, unit_cube().unshaded(), params, 2);
        close(px(&off, (48, 48)), gray(unshaded), 2);
    }
}

/// The centre pixel of a cube whose texture has the given alpha, through the cutout pass.
fn cutout_pixel(shading: ShadingMode, alpha: u8) -> Option<[u8; 4]> {
    let (gpu, mut r) = setup_with(RenderSettings {
        shading,
        ..RenderSettings::default()
    })?;
    let mut ex = extract(&[unit_cube()]);
    ex.atlas = vec![solid_block([255, 255, 255, alpha])];
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let (pixels, _) = render(&gpu, &mut r, params, &list(1, vec![run(0, 1, false)]));
    Some(px(&pixels, (48, 48)))
}

#[test]
fn the_cutout_threshold_is_a_half_for_blocks_and_cubik_and_a_tenth_for_items_and_flat() {
    let clear = [52, 52, 52, 255];
    for shading in [ShadingMode::MinecraftItem, ShadingMode::Flat] {
        // Alpha 0.05 is discarded; 0.2 passes and blends the lit face over the clear colour.
        let Some(dropped) = cutout_pixel(shading, 13) else {
            return;
        };
        close(dropped, clear, 0);
        let kept = cutout_pixel(shading, 51).unwrap_or_default();
        assert!(kept[0] > 60, "{shading:?} keeps alpha 0.2: {kept:?}");
    }
    // Alpha 0.2 is below the half of the terrain cutout and of Cubik; 0.7 passes.
    for shading in [ShadingMode::MinecraftBlock, ShadingMode::Cubik] {
        for alpha in [13, 51] {
            let Some(dropped) = cutout_pixel(shading, alpha) else {
                return;
            };
            close(dropped, clear, 0);
        }
        let kept = cutout_pixel(shading, 179).unwrap_or_default();
        assert!(kept[0] > 60, "{shading:?} keeps alpha 0.7: {kept:?}");
    }
}

#[test]
fn item_lighting_uses_the_level_lights_and_the_gui_setups_follow_the_camera() {
    let Some((gpu, mut r)) = setup_with(RenderSettings {
        shading: ShadingMode::MinecraftItem,
        ..RenderSettings::default()
    }) else {
        return;
    };
    // The west face seen from the west has the internal normal -X. Only the second level light
    // reaches it: 0.6 * 0.1617 + 0.4.
    let west = camera(Vec3::new(-5.0, 0.0, 0.0), Vec3::ZERO);
    let level = px(&shade_scene(&gpu, &mut r, unit_cube(), west, 1), (48, 48));
    close(level, gray(0.497), 2);
    close(
        level,
        gray(item_lit(Vec3::NEG_X, internal_level_lights())),
        2,
    );

    // The GUI setups turn with the camera, so the face that looks into the camera gets the same
    // light from any side. Side: the second light has 0.1884 toward the viewer, 0.6 * 0.1884 +
    // 0.4. Front: both lights reach the face and it is fully lit.
    let south = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    for (scene, params) in [(10, south), (20, west)] {
        let side = params.with_item_light(ItemLight::GuiSide);
        let pixels = shade_scene(&gpu, &mut r, unit_cube(), side, scene);
        close(px(&pixels, (48, 48)), gray(0.513), 2);
        let front = params.with_item_light(ItemLight::GuiFront);
        let pixels = shade_scene(&gpu, &mut r, unit_cube(), front, scene + 1);
        close(px(&pixels, (48, 48)), gray(1.0), 2);
    }
}

#[test]
fn a_display_preview_is_lit_as_an_item_in_the_block_mode_only() {
    let params = camera(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO).with_item_light(ItemLight::GuiSide);
    for (shading, want) in [
        (ShadingMode::MinecraftBlock, 0.513),
        (ShadingMode::Cubik, 0.859),
        (ShadingMode::Flat, 1.0),
    ] {
        let Some((gpu, mut r)) = setup_with(RenderSettings {
            shading,
            ..RenderSettings::default()
        }) else {
            return;
        };
        let pixels = shade_scene(&gpu, &mut r, unit_cube(), params, 1);
        close(px(&pixels, (48, 48)), gray(want), 2);
    }
}

/// Two voxel faces toward +Z: a plain one and one with AO code 2 on every corner.
fn voxel_pair(gpu: &TestGpu, r: &mut Renderer) -> (ViewParams, ViewDrawList) {
    let plain = VoxelFace::new([1, 1, 0], [0, 0, 0], 0x00FF00, [0; 4]);
    let dark = VoxelFace::new([3, 1, 0], [0, 0, 0], 0x00FF00, [2; 4]);
    let mut faces: [Vec<VoxelFace>; 6] = Default::default();
    faces[2] = vec![plain, dark];
    let ex = RenderExtract {
        chunks: vec![ChunkMesh {
            slot: 0,
            rev: Rev::next(),
            offsets: [0; 6],
            faces,
        }],
        ..RenderExtract::default()
    };
    r.sync(&gpu.queue, &ex, UploadBudget::default());
    let mut view = list(1, Vec::new());
    view.voxel_ranges[2] = vec![ChunkRange {
        slot: 0,
        first: 0,
        count: 2,
    }];
    let params = camera(Vec3::new(-13.5, -14.5, 21.0), Vec3::new(-13.5, -14.5, 16.0));
    (params, view)
}

#[test]
fn voxel_faces_take_the_block_constants_and_ao_is_an_option() {
    let mut settings = RenderSettings::default();
    let Some((gpu, mut r)) = setup_with(settings) else {
        return;
    };
    let (params, view) = voxel_pair(&gpu, &mut r);
    let plain_at = project(&params, Vec3::new(-14.5, -14.5, 16.0));
    let dark_at = project(&params, Vec3::new(-12.5, -14.5, 16.0));
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    close(px(&pixels, plain_at), [0, byte(0.8), 0, 255], 2);
    close(px(&pixels, dark_at), [0, byte(0.8 * 0.8), 0, 255], 2);

    settings.voxel_ao = false;
    r.set_settings(settings);
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    close(px(&pixels, dark_at), [0, byte(0.8), 0, 255], 2);

    settings.shading = ShadingMode::MinecraftItem;
    r.set_settings(settings);
    let (pixels, _) = render(&gpu, &mut r, params, &view);
    let item = item_lit(Vec3::Z, internal_level_lights());
    close(px(&pixels, plain_at), [0, byte(item), 0, 255], 2);
}
