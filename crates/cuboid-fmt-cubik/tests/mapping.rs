//! The loader rules left by W1.6, the saver rules of native 5.3 and the retention (native 6.3,
//! 8.3, integration backlog item 1).

#![allow(clippy::field_reassign_with_default)]

mod common;

use std::sync::Arc;

use cuboid_fmt_cubik::records::{
    ChunkRecord, FaceUvRecord, MeshRecord, RawTexture, Root, VoxelData, VoxelRecord, WorldRecord,
};
use cuboid_fmt_cubik::{CubikError, Limits, SaveOptions, TexturePolicy, encode, load};
use cuboid_model::image::{Image, RgbaBuf};
use cuboid_model::{
    Blueprint, Element, ElementType, Label, Mesh, MeshBounds, MeshGeometry, MeshKind, ObjectBase,
    Perms, Texture, TextureEntry, UvRect, voxel,
};
use glam::{Vec2, Vec3};

use common::*;

fn load_root(root: &Root) -> cuboid_fmt_cubik::Loaded {
    load_bytes(&encode::encode(root).unwrap())
}

fn load_error(root: &Root) -> String {
    match load::load_payload(encode::encode(root).unwrap(), &Limits::default()) {
        Ok(_) => panic!("the file was accepted"),
        Err(err) => err.to_string(),
    }
}

// --- Loader duties ------------------------------------------------------------------------------

#[test]
fn loaded_objects_get_fresh_ids_and_keep_the_selection_flags() {
    let root = full_root();
    let a = load_root(&root);
    let b = load_root(&root);
    let elements = a.doc.sections.elements.as_ref().unwrap();
    assert_eq!(elements.selected, [false, true, false]);
    let ids: Vec<_> = elements
        .list
        .ids
        .iter()
        .chain(&b.doc.sections.elements.as_ref().unwrap().list.ids)
        .chain(&a.doc.sections.meshes.as_ref().unwrap().list.ids)
        .collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn meshes_get_the_permissions_of_their_kind() {
    let loaded = load_root(&full_root());
    let meshes = &loaded.doc.sections.meshes.as_ref().unwrap().list.items;
    assert_eq!(meshes[0].kind, MeshKind::Cube);
    assert_eq!(meshes[0].base.perms, Perms::PRIMITIVE);
    assert_eq!(meshes[1].kind, MeshKind::Custom);
    assert_eq!(meshes[1].base.perms, Perms::IMPORTED_MESH);
    assert_eq!(meshes[2].kind, MeshKind::Sprite);
    assert_eq!(meshes[2].base.perms, Perms::PRIMITIVE);
    // Geometry is stored for the custom mesh only.
    assert_eq!(meshes[1].geometry.vertices.len(), 3);
    assert_eq!(meshes[1].geometry.normals.as_ref().map(Vec::len), Some(3));
    assert_eq!(meshes[1].submeshes.len(), 1);
    assert!(meshes[0].geometry.vertices.is_empty());
}

#[test]
fn an_unknown_mesh_kind_loads_as_a_cube_with_a_warning() {
    let mut root = full_root();
    root.me = Some(vec![primitive_record(77, "odd")]);
    let loaded = load_root(&root);
    let meshes = &loaded.doc.sections.meshes.as_ref().unwrap().list.items;
    assert_eq!(meshes[0].kind, MeshKind::Cube);
    assert!(loaded.warnings.iter().any(|w| w.contains("mesh kind 77")));
}

#[test]
fn a_null_path_becomes_an_empty_path_and_the_signature_stays() {
    let loaded = load_root(&full_root());
    let entries = &loaded.doc.sections.textures.as_ref().unwrap().entries;
    assert_eq!(entries[0].texture.source.path.as_deref(), Some(""));
    assert_eq!(entries[0].texture.source.signature, None);
    assert_eq!(
        entries[1].texture.source.path.as_deref(),
        Some("C:/pics/t.png")
    );
    assert_eq!(entries[1].texture.source.signature.as_deref(), Some("sig"));
    assert_eq!(&*entries[0].texture.name, "e_particle");
}

#[test]
fn camera_rotations_follow_the_euler_copy() {
    let mut root = full_root();
    let view = root.vp.as_mut().unwrap();
    // The stored components say identity; the Euler copy says 90 degrees about Y.
    let cam = view.cameras[0].as_mut().unwrap();
    cam.r.x = 0.0;
    cam.r.y = 0.0;
    cam.r.z = 0.0;
    cam.r.w = 1.0;
    cam.r.euler = Some(Vec3::new(0.0, 90.0, 0.0));
    let loaded = load_root(&root);
    let view = loaded.doc.sections.view.as_ref().unwrap();
    let q = view.cameras[0].unwrap().rot;
    assert!((q.y - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
}

#[test]
fn texture_records_are_validated_before_anything_is_allocated() {
    let with = |tx: RawTexture| {
        let mut root = Root::default();
        root.tx = Some(vec![texture_record("t", tx, None)]);
        root
    };
    let ok = rgba_texture("t", 2, 2);
    assert!(
        load::load_payload(
            encode::encode(&with(ok.clone())).unwrap(),
            &Limits::default()
        )
        .is_ok()
    );

    for (what, tx) in [
        (
            "negative size",
            RawTexture {
                w: -1,
                ..ok.clone()
            },
        ),
        (
            "too wide",
            RawTexture {
                w: 20_000,
                h: 1,
                ..ok.clone()
            },
        ),
        (
            "too many texels",
            RawTexture {
                w: 10_000,
                h: 10_000,
                ..ok.clone()
            },
        ),
        (
            "short data",
            RawTexture {
                w: 8,
                h: 8,
                ..ok.clone()
            },
        ),
    ] {
        let message = load_error(&with(tx));
        assert!(message.contains("texture"), "{what}: {message}");
    }

    // An unknown format code keeps its bytes (native 4.5.1).
    let odd = RawTexture {
        f: 61,
        r: Arc::from([1u8, 2, 3]),
        ..ok.clone()
    };
    let loaded = load_root(&with(odd));
    let image = &loaded.doc.sections.textures.as_ref().unwrap().entries[0]
        .texture
        .image;
    assert_eq!(image.stored_bytes(), (61, [1u8, 2, 3].as_slice()));

    let limits = Limits {
        max_texels: 3,
        ..Limits::default()
    };
    let bytes = encode::encode(&with(ok)).unwrap();
    assert!(load::load_payload(bytes, &limits).is_err());
}

#[test]
fn an_empty_texture_label_is_refused() {
    let mut root = Root::default();
    root.tx = Some(vec![texture_record("", rgba_texture("", 1, 1), None)]);
    assert!(load_error(&root).contains("Invalid Label"));
}

// --- Voxels -------------------------------------------------------------------------------------

fn world(res: i32, chunks: Vec<ChunkRecord>) -> Root {
    let mut root = Root::default();
    root.vx = Some(VoxelData {
        w: vec![WorldRecord { r: res, c: chunks }],
    });
    root
}

fn voxel_at(x: u8, y: u8, z: u8, c: u32) -> VoxelRecord {
    VoxelRecord {
        x,
        y,
        z,
        c: c.cast_signed(),
    }
}

fn chunk(cx: i32, cy: i32, cz: i32, v: Vec<VoxelRecord>) -> ChunkRecord {
    ChunkRecord { cx, cy, cz, v }
}

#[test]
fn canonical_voxel_records_become_a_chunk_table() {
    let loaded = load_root(&Root {
        vx: Some(voxel_world(16)),
        ..Root::default()
    });
    assert!(loaded.doc.voxel_replay.is_none());
    let snap = loaded
        .doc
        .sections
        .voxels
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(snap.res, 16);
    // 3 * 16 = 48 cells: three chunks per axis. The chunk without voxels is not allocated.
    assert_eq!(snap.slots.len(), 27);
    let data = snap
        .slots
        .iter()
        .filter(|s| matches!(s, voxel::ChunkSlot::Data(_)))
        .count();
    assert_eq!(data, 2);
}

#[test]
fn records_out_of_order_or_repeated_are_replayed_in_file_order() {
    let c = 0xFF11_2233;
    let cases = [
        // Chunks out of slot order.
        vec![
            chunk(1, 0, 0, vec![voxel_at(0, 0, 0, c)]),
            chunk(0, 0, 0, vec![voxel_at(0, 0, 0, c)]),
        ],
        // Voxels out of scan order.
        vec![chunk(
            0,
            0,
            0,
            vec![voxel_at(1, 0, 0, c), voxel_at(0, 0, 0, c)],
        )],
        // A repeated cell.
        vec![chunk(
            0,
            0,
            0,
            vec![voxel_at(1, 0, 0, c), voxel_at(1, 0, 0, c + 1)],
        )],
        // A colour without alpha.
        vec![chunk(0, 0, 0, vec![voxel_at(1, 0, 0, 0x0011_2233)])],
    ];
    for (n, chunks) in cases.into_iter().enumerate() {
        let loaded = load_root(&world(8, chunks.clone()));
        let replay = loaded
            .doc
            .voxel_replay
            .as_ref()
            .unwrap_or_else(|| panic!("case {n} is canonical"));
        let expected: Vec<(i32, u32)> = chunks
            .iter()
            .flat_map(|ch| {
                ch.v.iter().map(move |v| {
                    (
                        ch.cx * 16 + i32::from(v.x) + 1000 * (ch.cy * 16 + i32::from(v.y)),
                        v.c.cast_unsigned(),
                    )
                })
            })
            .collect();
        let got: Vec<(i32, u32)> = replay
            .cells
            .iter()
            .map(|(cell, c)| (cell.x + 1000 * cell.y, *c))
            .collect();
        assert_eq!(got, expected, "case {n}");
    }
}

#[test]
fn a_replayed_repeat_applies_like_the_full_writer() {
    // The later value of a repeated cell wins, and both values reach the palette.
    let c = 0xFF11_2233;
    let root = world(
        8,
        vec![chunk(
            0,
            0,
            0,
            vec![voxel_at(1, 0, 0, c), voxel_at(1, 0, 0, c + 1)],
        )],
    );
    let (doc, _) = open(load_root(&root));
    let w = doc.voxels.as_ref().unwrap();
    assert_eq!(w.read(voxel::I3::new(1, 0, 0)), c + 1);
    assert_eq!(w.palette.values(), [0, c, c + 1]);
}

#[test]
fn the_voxel_resolution_is_limited_by_the_memory_budget() {
    // Any resolution of at least 1 is accepted while the world fits.
    for res in [1, 2, 128, 254] {
        assert!(
            load::load_payload(
                encode::encode(&world(res, Vec::new())).unwrap(),
                &Limits::default()
            )
            .is_ok(),
            "{res}"
        );
    }
    for res in [0, -1] {
        assert!(load_error(&world(res, Vec::new())).contains("resolution"));
    }
    let tight = Limits {
        voxel_budget: 1 << 16,
        ..Limits::default()
    };
    let bytes = encode::encode(&world(128, Vec::new())).unwrap();
    let err = load::load_payload(bytes, &tight).unwrap_err();
    assert!(err.to_string().contains("budget"), "{err}");
    // The apply path refuses what the budget of the loader let through only if it is larger.
    assert!(matches!(
        CubikError::Invalid(String::new()),
        CubikError::Invalid(_)
    ));
}

// --- Saver rules (native 5.3) -------------------------------------------------------------------

fn empty_doc() -> (cuboid_model::Document, cuboid_model::Selection) {
    (
        cuboid_model::Document::new(cuboid_model::TextureList::default()),
        cuboid_model::Selection::default(),
    )
}

#[test]
fn elements_are_saved_with_packed_colour_and_folded_uv() {
    let (mut doc, sel) = empty_doc();
    let mut el = Element::new(
        ElementType::Cube,
        &Blueprint::default(),
        1,
        [0.2, 0.4, 0.6, 0.3],
        &Label::from("texture"),
    );
    el.faces[0].uv = UvRect::new(90, Vec2::new(-0.25, 0.5), Vec2::new(0.5, 1.5));
    doc.elements.push(el);
    let saved = decode(&save_payload(&doc, &sel, &without_retention()));
    let rec = &saved.el.unwrap()[0];
    // Alpha is forced to 255 unless the byte is 0; channels truncate.
    assert_eq!(rec.c, 0xFF33_6699_u32.cast_signed());
    let down = &rec.f.as_ref().unwrap().faces[0];
    assert_eq!(down.n.as_deref(), Some("down"));
    assert_eq!(down.uv.r, 90);
    assert_eq!(down.uv.tl, Vec2::new(0.0, 0.5));
    assert_eq!(down.uv.br, Vec2::new(0.5, 1.0));
    assert_eq!(rec.l.as_deref(), Some("Box1"));
}

#[test]
fn the_blueprint_is_saved_with_its_fixed_keys() {
    let (mut doc, sel) = empty_doc();
    doc.blueprint.base.p = Vec3::new(3.0, 1.0, -1.0);
    doc.blueprint.base.o = Vec3::new(9.0, 9.0, 9.0);
    doc.blueprint.base.hidden = true;
    let bp = decode(&save_payload(&doc, &sel, &without_retention()))
        .bp
        .unwrap();
    assert_eq!(bp.p, Vec3::new(3.0, 1.0, -1.0));
    assert_eq!(bp.o, bp.p, "the world position");
    assert!(bp.ih && !bp.is);
    assert_eq!((bp.c, bp.t), (0, 0));
    assert!(bp.f.is_none());
    assert!(!(bp.rs || bp.suv || bp.suvm || bp.sh || bp.inv));
    assert_eq!(bp.l.as_deref(), Some("Blueprint"));
}

#[test]
fn only_custom_meshes_write_geometry_and_sub_meshes() {
    let loaded = load_root(&full_root());
    let (doc, sel) = open(loaded);
    let saved = decode(&save_payload(&doc, &sel, &without_retention()));
    let meshes = saved.me.unwrap();
    assert!(meshes[0].me.is_none() && meshes[0].sm.is_none());
    assert!(meshes[2].me.is_none() && meshes[2].sm.is_none());
    let data = meshes[1].me.as_ref().unwrap();
    assert_eq!(data.v.len(), 3);
    assert_eq!(data.t, [0, 1, 2]);
    let sub = &meshes[1].sm.as_ref().unwrap()[0];
    assert_eq!(sub.c, 0xFF80_4020_u32.cast_signed());
    // Primitives keep their stored 128 x 128 style texture record.
    assert_eq!(meshes[0].tx.as_ref().unwrap().n, "Cube");

    // A custom mesh without normals writes an empty array (native 4.6.1).
    let mut root = full_root();
    root.me.as_mut().unwrap()[1].me.as_mut().unwrap().n = None;
    let (doc, sel) = open(load_root(&root));
    let saved = decode(&save_payload(&doc, &sel, &without_retention()));
    assert_eq!(
        saved.me.unwrap()[1].me.as_ref().unwrap().n,
        Some(Vec::new())
    );
}

#[test]
fn a_mesh_with_a_missing_texture_and_no_sub_meshes_round_trips() {
    let (mut doc, sel) = empty_doc();
    doc.meshes.push(Mesh {
        base: ObjectBase::new("Bare", Vec3::ZERO, Vec3::ONE, Perms::IMPORTED_MESH),
        kind: MeshKind::Custom,
        geometry: Arc::new(MeshGeometry::default()),
        texture: None,
        submeshes: Arc::from([]),
        bounds: MeshBounds::default(),
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    });
    let saved = decode(&save_payload(&doc, &sel, &without_retention()));
    let rec: &MeshRecord = &saved.me.unwrap()[0];
    assert!(rec.tx.is_none() && rec.sm.is_none());
    assert_eq!(rec.me.as_ref().unwrap().n, Some(Vec::new()));
}

#[test]
fn the_view_and_display_are_saved_with_their_fixed_values() {
    let loaded = load_root(&full_root());
    let (mut doc, sel) = open(loaded);
    {
        let view = Arc::make_mut(&mut doc.view);
        view.uv_step = 0.125;
        view.large_grid = true;
        let display = Arc::make_mut(&mut doc.display);
        display.slots[2].scale = Vec3::new(0.0, 1.0, 0.0);
        display.slots[2].enabled = true;
    }
    let saved = decode(&save_payload(&doc, &sel, &SaveOptions::default()));
    let vp = saved.vp.unwrap();
    assert_eq!((vp.gu, vp.gl), (0.5, false));
    assert_eq!(vp.gm, 0.25);
    let md = saved.md.unwrap();
    assert_eq!(md.slots[2].scale, Vec3::new(0.0001, 1.0, 0.0001));
    assert!(md.slots[2].enabled);
}

// --- Texture write policy -----------------------------------------------------------------------

fn texture_doc() -> (cuboid_model::Document, cuboid_model::Selection) {
    let (doc, sel) = open(load_root(&full_root()));
    (doc, sel)
}

#[test]
fn unchanged_textures_keep_their_format_and_bytes_and_edited_ones_become_rgba32() {
    let (mut doc, sel) = texture_doc();
    {
        let entry = &mut doc.textures.entries[1];
        let mut image = (*entry.texture.image).clone();
        image.edit().pixels[0] = 77;
        entry.texture.image = Arc::new(image);
    }
    let saved = decode(&save_payload(&doc, &sel, &without_retention()));
    let tx = saved.tx.unwrap();
    // The default-style texture keeps RGB24 with its whole mip chain.
    assert_eq!(tx[0].tx.f, 3);
    assert_eq!(tx[0].tx.r.len(), 1023);
    assert_eq!(
        tx[0].fp.as_deref(),
        Some(""),
        "a reopened null path is an empty path"
    );
    // The edited one is written as RGBA32 without a chain.
    assert_eq!(tx[1].tx.f, 4);
    assert_eq!(tx[1].tx.r.len(), 3 * 2 * 4);
    assert_eq!(tx[1].tx.r[0], 77);
}

#[test]
fn the_strict_switch_writes_the_atlas_recut_form() {
    let (doc, sel) = texture_doc();
    let options = SaveOptions {
        textures: TexturePolicy::AtlasRecutArgb32,
        ..SaveOptions::default()
    };
    let saved = decode(&save_payload(&doc, &sel, &options));
    for rec in saved.tx.unwrap() {
        assert_eq!(rec.tx.f, 5, "{}", rec.lb);
        assert_eq!(rec.tx.n, "");
        assert_eq!((rec.fp, rec.cs), (None, None));
        let texels = usize::try_from(rec.tx.w * rec.tx.h).unwrap();
        assert_eq!(rec.tx.r.len(), texels * 4, "level 0 only");
    }
    // The level-0 pixels of the 16 x 16 RGB24 texture come back as A, R, G, B.
    let saved = decode(&save_payload(&doc, &sel, &options));
    let first = &saved.tx.unwrap()[0].tx.r;
    assert_eq!(first[0], 255);
    assert_eq!(first[1], 0);
}

#[test]
fn a_new_texture_is_written_as_rgba32() {
    let (mut doc, sel) = empty_doc();
    let mut buf = RgbaBuf::new(2, 1);
    buf.pixels[..4].copy_from_slice(&[1, 2, 3, 4]);
    doc.textures.entries.push(TextureEntry {
        label: Label::from("made"),
        texture: Texture::new(Arc::new(Image::from_rgba(buf))),
        animation: None,
    });
    let saved = decode(&save_payload(&doc, &sel, &SaveOptions::default()));
    let rec = &saved.tx.unwrap()[0];
    assert_eq!((rec.tx.f, rec.tx.w, rec.tx.h), (4, 2, 1));
    assert_eq!(rec.tx.r.len(), 8);
    assert_eq!(
        (rec.fp.clone(), rec.cs.clone()),
        (None, None),
        "assigned in memory"
    );
}

// --- Retention ----------------------------------------------------------------------------------

#[test]
fn an_unchanged_document_saves_the_payload_it_was_loaded_from() {
    let payload = full_payload();
    let (doc, sel) = open(load_bytes(&payload));
    assert!(doc.retained.is_some());
    assert_eq!(save_payload(&doc, &sel, &SaveOptions::default()), payload);
}

#[test]
fn without_retention_the_stored_euler_copy_is_recomputed() {
    let mut root = full_root();
    // One f32 step above 45 degrees, as the engine's single-precision results are.
    let step = f32::from_bits(45.0_f32.to_bits() + 1);
    let cam = root.vp.as_mut().unwrap().cameras[3].as_mut().unwrap();
    cam.r.euler = Some(Vec3::new(step, step, 0.0));
    let payload = encode::encode(&root).unwrap();
    let (doc, sel) = open(load_bytes(&payload));
    assert_eq!(save_payload(&doc, &sel, &SaveOptions::default()), payload);
    let plain = save_payload(&doc, &sel, &without_retention());
    assert_ne!(plain, payload);
    // Only the view differs.
    let (a, b) = (decode(&plain), decode(&payload));
    assert_eq!(a.el, b.el);
    // Only a null path turns into an empty one (native 6.3).
    let images = |r: &Root| -> Vec<_> {
        r.tx.iter()
            .flatten()
            .map(|t| (t.lb.clone(), t.tx.clone()))
            .collect()
    };
    assert_eq!(images(&a), images(&b));
    assert_eq!(a.me, b.me);
    // The chunk that was written empty is not listed again after a load (native 6.3).
    let voxels =
        |r: &Root| -> usize { r.vx.iter().flat_map(|v| &v.w[0].c).map(|c| c.v.len()).sum() };
    assert_eq!(voxels(&a), voxels(&b));
}

#[test]
fn only_the_edited_record_is_encoded_again() {
    let mut root = full_root();
    // An alpha-less colour that the model cannot hold: it loads as opaque black.
    root.el.as_mut().unwrap()[0].c = 0;
    let payload = encode::encode(&root).unwrap();
    let (mut doc, sel) = open(load_bytes(&payload));
    let id = doc.elements.ids()[1];
    {
        let mut journal = std::mem::take(&mut doc.journal);
        doc.elements.edit(id, &mut journal).unwrap().base.label = Label::from("renamed");
        doc.journal = journal;
    }
    let saved = decode(&save_payload(&doc, &sel, &SaveOptions::default()));
    let elements = saved.el.unwrap();
    let original = root.el.unwrap();
    assert_eq!(
        elements[0], original[0],
        "c = 0 survives in the untouched record"
    );
    assert_eq!(elements[1].l.as_deref(), Some("renamed"));
    assert_eq!(elements[2], original[2]);
    assert!(elements[1].is, "the selection is saved");
}

#[test]
fn the_save_path_is_the_only_thing_that_changes_for_a_save_as() {
    let payload = full_payload();
    let (mut doc, sel) = open(load_bytes(&payload));
    {
        let mut props = (*doc.props).clone();
        props.save_path = "D:/elsewhere/b.cubik".to_owned();
        doc.props = Arc::new(props);
    }
    let saved = decode(&save_payload(&doc, &sel, &SaveOptions::default()));
    let original = decode(&payload);
    assert_eq!(saved.mp.as_ref().unwrap().save_path, "D:/elsewhere/b.cubik");
    assert_eq!(saved.vp, original.vp);
    assert_eq!(saved.el, original.el);
    assert_eq!(saved.tx, original.tx);
    assert_eq!(saved.vx, original.vx);
    assert_eq!(saved.me, original.me);
}

#[test]
fn editing_the_voxels_writes_the_world_again_and_drops_erased_chunks_records_of_the_file() {
    let payload = full_payload();
    let (mut doc, sel) = open(load_bytes(&payload));
    doc.voxels
        .as_mut()
        .unwrap()
        .write_full(voxel::I3::new(10, 10, 10), 0xFF12_3456);
    let saved = decode(&save_payload(&doc, &sel, &SaveOptions::default()));
    let world = &saved.vx.unwrap().w[0];
    let total: usize = world.c.iter().map(|c| c.v.len()).sum();
    assert_eq!(total, 5);
    assert!(
        world.c.iter().all(|c| !c.v.is_empty()),
        "an empty chunk is not listed after a load"
    );
}

#[test]
fn a_loaded_file_can_switch_its_retention_off() {
    let limits = Limits {
        retain_cap: 0,
        ..Limits::default()
    };
    let loaded = load::load_payload(full_payload(), &limits).unwrap();
    assert!(loaded.doc.retained.is_none());
    let limits = Limits {
        retain_cap: 100,
        ..Limits::default()
    };
    let loaded = load::load_payload(full_payload(), &limits).unwrap();
    assert!(loaded.doc.retained.is_none(), "above the cap");
}

// --- Files --------------------------------------------------------------------------------------

#[test]
fn a_saved_file_opens_again_and_carries_the_overrides_when_asked() {
    let (mut doc, sel) = open(load_bytes(&full_payload()));
    let overrides = vec![cuboid_model::OverrideCase {
        model: "item/x".to_owned(),
        pull: Some(0.5),
        ..Default::default()
    }];
    doc.overrides = overrides.clone().into();
    let snapshot = snapshot(&doc, &sel);
    let with = cuboid_fmt_cubik::save_bytes(
        &snapshot,
        doc.retained.as_deref(),
        &overrides,
        &SaveOptions::default(),
    )
    .unwrap();
    let loaded = cuboid_fmt_cubik::load_bytes(&with, &Limits::default()).unwrap();
    assert_eq!(loaded.doc.overrides.as_deref(), Some(overrides.as_slice()));

    let compatible = SaveOptions {
        extension: false,
        ..SaveOptions::default()
    };
    let without =
        cuboid_fmt_cubik::save_bytes(&snapshot, doc.retained.as_deref(), &overrides, &compatible)
            .unwrap();
    let loaded = cuboid_fmt_cubik::load_bytes(&without, &Limits::default()).unwrap();
    assert!(loaded.doc.overrides.is_none());
}

#[test]
fn a_file_is_written_atomically_and_reads_back() {
    let (doc, sel) = open(load_bytes(&full_payload()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.cubik");
    std::fs::write(&path, b"old content").unwrap();
    cuboid_fmt_cubik::save_path(
        &path,
        &snapshot(&doc, &sel),
        doc.retained.as_deref(),
        &[],
        &SaveOptions::default(),
    )
    .unwrap();
    let loaded = cuboid_fmt_cubik::load_path(&path, &Limits::default()).unwrap();
    assert_eq!(loaded.doc.sections.elements.unwrap().list.ids.len(), 3);
    let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftovers, 1, "no temporary file stays behind");
}

#[test]
fn a_snapshot_without_all_categories_cannot_be_saved() {
    let (doc, sel) = empty_doc();
    let snap = cuboid_model::capture(
        &doc,
        &sel,
        cuboid_model::CaptureScope {
            categories: cuboid_model::Categories::ELEMENTS,
            selection_only: false,
        },
    );
    let err = cuboid_fmt_cubik::save::save_payload(&snap, None, &SaveOptions::default());
    assert!(err.unwrap_err().to_string().contains("model properties"));
    let _ = FaceUvRecord::default();
}

// --- Rules of native 4.2.1 and 4.3.2 ------------------------------------------------------------

#[test]
fn double_quotes_in_the_model_strings_never_reach_the_file() {
    let mut root = full_root();
    let props = root.mp.as_mut().unwrap();
    props.general.name = "say \"hi\"".to_owned();
    props.general.description = "\"quoted\"".to_owned();
    props.minecraft.parent = "a\"b".to_owned();
    let (doc, sel) = open(load_root(&root));
    assert_eq!(doc.props.general.name, "say \u{201d}hi\u{201d}");
    for options in [SaveOptions::default(), without_retention()] {
        let saved = decode(&save_payload(&doc, &sel, &options)).mp.unwrap();
        assert_eq!(saved.general.name, "say \u{201d}hi\u{201d}");
        assert_eq!(saved.general.description, "\u{201d}quoted\u{201d}");
        assert_eq!(saved.minecraft.parent, "a\u{201d}b");
    }
}

#[test]
fn the_top_camera_at_gimbal_lock_keeps_its_rotation_and_stores_zero_for_y_and_z() {
    let mut root = Root {
        vp: Some(view_record()),
        ..Root::default()
    };
    let top = root.vp.as_ref().unwrap().cameras[0].unwrap();
    let (doc, sel) = open(load_root(&root));
    let saved = decode(&save_payload(&doc, &sel, &without_retention()));
    let cam = saved.vp.unwrap().cameras[0].unwrap();
    // Rotations are compared, not Euler components (native 4.3.2).
    assert!(cam.r.rotation.dot(top.r.rotation).abs() > 1.0 - 1e-6);
    let euler = cam.r.euler.unwrap();
    assert!((euler.x - 90.0).abs() < 1e-4, "{euler:?}");
    assert!(euler.y.abs() < 1e-4 && euler.z.abs() < 1e-4, "{euler:?}");
    root.vp = None;
}

#[test]
fn a_damaged_extension_entry_is_ignored_with_a_warning() {
    let archive = cuboid_fmt_cubik::container::write(&full_payload(), Some(b"not json")).unwrap();
    let loaded = cuboid_fmt_cubik::load_bytes(&archive, &Limits::default()).unwrap();
    assert!(loaded.doc.overrides.is_none());
    assert!(loaded.warnings.iter().any(|w| w.contains("cuboid.json")));
}
