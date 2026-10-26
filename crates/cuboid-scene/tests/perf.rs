//! Timed checks of the budgets of roadmap_lean 8 for the scene side: 10 k elements and a dense
//! voxel world. They measure wall-clock time, so they are ignored in CI; run them with
//! `cargo test -p cuboid-scene --release --test perf -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use cuboid_model::{
    ChangeJournal, Document, Element, ElementType, Label, RenderPrefs, Texture, TextureEntry,
    TextureList,
};
use cuboid_num::EngineRng;
use cuboid_scene::{
    Camera, ExtractBudget, Extractor, Ray, Raycaster, SceneIndex, View, ViewId, ViewSet,
    compile_draw_list,
};
use cuboid_voxel::{VoxelWorld, mapping::grid_box};
use glam::{IVec3, Vec3};

/// One frame at 60 Hz.
const FRAME: Duration = Duration::from_millis(16);

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = f();
    let took = start.elapsed();
    println!("{label}: {:.3} ms", took.as_secs_f64() * 1000.0);
    (value, took)
}

fn solid_texture() -> Texture {
    let mut buf = cuboid_image::RgbaBuf::new(16, 16);
    buf.pixels.fill(255);
    Texture::new(std::sync::Arc::new(cuboid_image::Image::from_rgba(buf)))
}

/// A document with `n` randomly placed and rotated cubes in a 300 unit cube.
fn element_doc(n: usize) -> Document {
    let mut textures = TextureList::default();
    textures.entries.push(TextureEntry {
        label: Label::from("texture"),
        texture: solid_texture(),
        animation: None,
    });
    let mut doc = Document::new(textures);
    let mut rng = EngineRng::seeded(7);
    let label = Label::from("texture");
    for _ in 0..n {
        let mut el = Element::new(ElementType::Cube, &doc.blueprint, 1, [1.0; 4], &label);
        el.base.p = Vec3::new(
            rng.range_f32(-150.0, 150.0),
            rng.range_f32(-150.0, 150.0),
            rng.range_f32(-150.0, 150.0),
        );
        el.base.s = Vec3::new(
            rng.range_f32(0.5, 3.0),
            rng.range_f32(0.5, 3.0),
            rng.range_f32(0.5, 3.0),
        );
        el.base.e = Vec3::new(
            (rng.unit_f32() * 16.0).floor() * 22.5,
            (rng.unit_f32() * 16.0).floor() * 22.5,
            (rng.unit_f32() * 16.0).floor() * 22.5,
        );
        doc.elements.push(el);
    }
    doc
}

fn random_rays(count: usize, spread: f32) -> Vec<Ray> {
    let mut rng = EngineRng::seeded(11);
    (0..count)
        .map(|_| {
            let origin = Vec3::new(
                rng.range_f32(-spread, spread),
                rng.range_f32(-spread, spread),
                -400.0,
            );
            let target = Vec3::new(
                rng.range_f32(-spread, spread),
                rng.range_f32(-spread, spread),
                0.0,
            );
            Ray {
                origin,
                dir: (target - origin).normalize(),
            }
        })
        .collect()
}

#[test]
#[ignore = "timing, run in release mode"]
fn ten_thousand_elements_stay_within_a_frame() {
    let mut doc = element_doc(10_000);
    let journal = ChangeJournal::default();

    let mut index = SceneIndex::default();
    let (_, build) = timed("index build, 10 k elements", || index.sync(&doc, &journal));
    let rays = random_rays(1000, 150.0);
    let (hits, picks) = timed("1000 pick rays", || {
        rays.iter()
            .filter(|r| index.pick_model(**r, 1000.0).is_some())
            .count()
    });
    println!("  {hits} of 1000 rays hit");

    let id = doc.elements.ids()[5000];
    let mut j = ChangeJournal::default();
    doc.elements.edit(id, &mut j).unwrap().base.p.x += 1.0;
    let (_, refit) = timed("index refit after one move", || index.sync(&doc, &j));

    let mut extractor = Extractor::default();
    let prefs = RenderPrefs::default();
    let (first, first_extract) = timed("first extract", || {
        extractor.extract(&mut doc, &prefs, &ExtractBudget::UNLIMITED)
    });
    assert_eq!(first.instances.len(), 10_000);
    let (idle, idle_extract) = timed("extract, nothing changed", || {
        extractor.extract(&mut doc, &prefs, &ExtractBudget::default())
    });
    assert!(idle.is_empty());
    doc.elements.edit(id, &mut j).unwrap().base.p.x += 1.0;
    let (edit, edit_extract) = timed("extract after one edit", || {
        extractor.extract(&mut doc, &prefs, &ExtractBudget::default())
    });
    assert_eq!(edit.instances.len(), 1);

    let mut views = ViewSet::default();
    views.arrange(cuboid_scene::Rect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 720.0,
    });
    let main: &View = views.view(ViewId::Main);
    let (_, compile) = timed("draw list, 10 k elements", || {
        compile_draw_list(main, extractor.cache(), &prefs)
    });

    assert!(build < FRAME * 4, "index build {build:?}");
    assert!(picks < FRAME, "1000 picks {picks:?}");
    assert!(refit < FRAME, "refit {refit:?}");
    assert!(first_extract < FRAME * 8, "first extract {first_extract:?}");
    assert!(
        idle_extract < Duration::from_millis(2),
        "idle extract {idle_extract:?}"
    );
    assert!(
        edit_extract < Duration::from_millis(2),
        "one-edit extract {edit_extract:?}"
    );
    assert!(compile < FRAME, "draw list {compile:?}");
}

/// A checkerboard of voxels over the whole grid: every voxel shows all six faces.
fn checkerboard_world(res: u32) -> VoxelWorld {
    let mut world = VoxelWorld::new(res, true, grid_box(res));
    let n = i32::try_from(3 * res).unwrap();
    let cells =
        (0..n).flat_map(|i| (0..n).flat_map(move |j| (0..n).map(move |k| IVec3::new(i, j, k))));
    let mut cells = cells
        .filter(|c| (c.x + c.y + c.z) % 2 == 0)
        .map(|c| (c, 0xFF40_80C0_u32));
    world.write_many(&mut cells, cuboid_voxel::WriteKind::Full);
    world
}

#[test]
#[ignore = "timing, run in release mode"]
fn a_dense_voxel_world_meshes_and_picks_within_a_frame() {
    let mut doc = Document::new(TextureList::default());
    let (world, write) = timed("write the checkerboard, res 32", || checkerboard_world(32));
    doc.voxels = Some(world);
    let _ = write;

    let mut extractor = Extractor::default();
    let prefs = RenderPrefs::default();
    let (out, mesh_all) = timed("mesh every chunk", || {
        extractor.extract(&mut doc, &prefs, &ExtractBudget::UNLIMITED)
    });
    let faces: usize = out
        .chunks
        .iter()
        .flat_map(|c| c.faces.iter())
        .map(Vec::len)
        .sum();
    println!("  {} chunks, {faces} faces", out.chunks.len());
    assert!(
        faces >= 2_000_000,
        "the board should show about 2.6 M faces"
    );

    let mut index = SceneIndex::default();
    index.sync(&doc, &ChangeJournal::default());
    let rays = random_rays(1000, 24.0);
    let (hits, picks) = timed("1000 voxel pick rays", || {
        rays.iter()
            .filter(|r| index.pick_model(**r, 1000.0).is_some())
            .count()
    });
    println!("  {hits} of 1000 rays hit");

    // A voxel edit: flip one voxel and mesh the dirty chunks.
    doc.voxels
        .as_mut()
        .unwrap()
        .write_full(IVec3::new(40, 40, 40), 0xFFFF_0000);
    let (edit, edit_time) = timed("one voxel edit, extract", || {
        extractor.extract(&mut doc, &prefs, &ExtractBudget::default())
    });
    println!("  {} chunks remeshed", edit.chunks.len());

    let mut views = ViewSet::default();
    views.arrange(cuboid_scene::Rect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 720.0,
    });
    let (_, compile) = timed("draw list, dense world", || {
        compile_draw_list(views.view(ViewId::Main), extractor.cache(), &prefs)
    });
    let _ = Camera::default_for(ViewId::Main);

    assert!(mesh_all < Duration::from_secs(2), "full mesh {mesh_all:?}");
    assert!(picks < FRAME, "1000 voxel picks {picks:?}");
    assert!(edit_time < FRAME, "voxel edit {edit_time:?}");
    assert!(compile < FRAME, "draw list {compile:?}");
}
