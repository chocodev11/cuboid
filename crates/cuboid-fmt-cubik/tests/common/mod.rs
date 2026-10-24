//! Helpers shared by the integration tests: records to build files from, and a document to
//! open them in.

#![allow(clippy::field_reassign_with_default)]
#![allow(dead_code)]

use std::sync::Arc;

use cuboid_fmt_cubik::records::{
    BoundsRecord, CameraRecord, ChunkRecord, ElementRecord, FACE_NAMES, FaceRecord, FaceSetRecord,
    FaceUvRecord, MeshDataRecord, MeshRecord, QuatRecord, RawTexture, Root, SubmeshRecord,
    TextureRecord, ViewRecord, VoxelData, VoxelRecord, WorldRecord,
};
use cuboid_fmt_cubik::{Limits, Loaded, SaveOptions, encode, load, save};
use cuboid_model::{
    ApplyMode, ApplySource, CaptureScope, Categories, DisplaySlots, Document, EditCtx, EditEnv,
    ModelProps, RenderPrefs, Selection, TextureList, apply_document, capture,
};
use cuboid_num::{EngineRng, euler_to_quat, quat_to_euler};
use glam::{Quat, Vec2, Vec3};

/// A document with the loaded file applied as File > Open does.
pub fn open(loaded: Loaded) -> (Document, Selection) {
    let mut doc = Document::new(TextureList::default());
    let mut sel = Selection::default();
    let env = EditEnv {
        restrict_rotations: true,
        render: RenderPrefs::default(),
        code_page: 1252,
    };
    let mut rng = EngineRng::seeded(1);
    apply_document(
        &mut EditCtx {
            doc: &mut doc,
            sel: &mut sel,
            env: &env,
            rng: &mut rng,
        },
        ApplySource::Loaded(Box::new(loaded.doc)),
        ApplyMode::Open,
    )
    .expect("the file applies");
    (doc, sel)
}

pub fn snapshot(doc: &Document, sel: &Selection) -> cuboid_model::Snapshot {
    capture(
        doc,
        sel,
        CaptureScope {
            categories: Categories::ALL,
            selection_only: false,
        },
    )
}

/// The payload that a save of the document writes.
pub fn save_payload(doc: &Document, sel: &Selection, options: &SaveOptions) -> Vec<u8> {
    save::save_payload(&snapshot(doc, sel), doc.retained.as_deref(), options)
        .expect("the document saves")
}

pub fn without_retention() -> SaveOptions {
    SaveOptions {
        use_retention: false,
        ..SaveOptions::default()
    }
}

pub fn load_bytes(payload: &[u8]) -> Loaded {
    load::load_payload(payload.to_vec(), &Limits::default()).expect("the payload loads")
}

pub fn decode(payload: &[u8]) -> Root {
    cuboid_fmt_cubik::decode::decode(payload, &Limits::default())
        .expect("decodes")
        .root
}

// --- Records --------------------------------------------------------------------------------------

pub fn quat_record(euler: Vec3) -> QuatRecord {
    let q = euler_to_quat(euler);
    let e = quat_to_euler(q);
    QuatRecord {
        x: q.x,
        y: q.y,
        z: q.z,
        w: q.w,
        euler: Some(e),
        rotation: euler_to_quat(e),
    }
}

pub fn camera(pos: Vec3, euler: Vec3, ortho: bool) -> CameraRecord {
    CameraRecord {
        p: pos,
        r: quat_record(euler),
        o: ortho,
        os: 12.0,
        fv: if ortho { 60.0 } else { 64.0 },
    }
}

pub fn view_record() -> ViewRecord {
    ViewRecord {
        cameras: [
            Some(camera(
                Vec3::new(8.0, 72.0, -8.0),
                Vec3::new(90.0, 0.0, 0.0),
                true,
            )),
            Some(camera(
                Vec3::new(-54.0, 8.0, -8.0),
                Vec3::new(0.0, 90.0, 0.0),
                true,
            )),
            Some(camera(Vec3::new(8.0, 8.0, -54.0), Vec3::ZERO, true)),
            Some(camera(
                Vec3::new(-4.0, 24.0, -20.0),
                Vec3::new(45.0, 45.0, 0.0),
                false,
            )),
        ],
        gm: 0.25,
        gk: 0.125,
        gu: 0.5,
        gd: 2.0,
        gl: false,
    }
}

pub fn face(slot: usize, uv: FaceUvRecord, texture: &str) -> FaceRecord {
    FaceRecord {
        n: Some(FACE_NAMES[slot].to_owned()),
        a: true,
        ti: -1,
        c: 0,
        t: Some(texture.to_owned()),
        uv,
    }
}

pub fn element_record(label: &str, p: Vec3, s: Vec3) -> ElementRecord {
    let uv = FaceUvRecord {
        r: 0,
        tl: Vec2::new(0.0, 0.25),
        br: Vec2::new(0.5, 0.75),
    };
    ElementRecord {
        p,
        s,
        r: Vec3::ZERO,
        o: p,
        c: 0xFF33_6699_u32.cast_signed(),
        is: false,
        ih: false,
        il: false,
        l: Some(label.to_owned()),
        f: Some(FaceSetRecord {
            faces: std::array::from_fn(|slot| face(slot, uv, "texture")),
        }),
        rs: false,
        suv: true,
        suvm: false,
        sh: true,
        inv: false,
        t: 0,
    }
}

pub fn blueprint_record() -> ElementRecord {
    ElementRecord {
        p: Vec3::new(-2.0, 0.0, 2.0),
        s: Vec3::ONE,
        o: Vec3::new(-2.0, 0.0, 2.0),
        l: Some("Blueprint".to_owned()),
        ..ElementRecord::default()
    }
}

pub fn rgba_texture(name: &str, w: i32, h: i32) -> RawTexture {
    let len = usize::try_from(w * h * 4).unwrap();
    RawTexture {
        n: name.to_owned(),
        w,
        h,
        f: 4,
        r: (0..len)
            .map(|i| u8::try_from(i * 7 % 256).unwrap())
            .collect(),
    }
}

/// 16 x 16 RGB24 with a mip chain behind level 0, as Cubik writes the default textures.
pub fn rgb_texture_with_mips(name: &str) -> RawTexture {
    RawTexture {
        n: name.to_owned(),
        w: 16,
        h: 16,
        f: 3,
        r: (0..1023).map(|i| u8::try_from(i % 253).unwrap()).collect(),
    }
}

pub fn texture_record(label: &str, tx: RawTexture, fp: Option<&str>) -> TextureRecord {
    TextureRecord {
        lb: label.to_owned(),
        fp: fp.map(str::to_owned),
        fname: fp.map(|_| "name".to_owned()),
        fm: String::new(),
        tx,
        cs: fp.map(|_| "sig".to_owned()),
    }
}

pub fn voxel_world(res: i32) -> VoxelData {
    let voxel = |x: u8, y: u8, z: u8, c: u32| VoxelRecord {
        x,
        y,
        z,
        c: c.cast_signed(),
    };
    VoxelData {
        w: vec![WorldRecord {
            r: res,
            c: vec![
                ChunkRecord {
                    cx: 0,
                    cy: 0,
                    cz: 0,
                    v: vec![
                        voxel(0, 0, 0, 0xFF5A_9643),
                        voxel(0, 1, 2, 0xFFA4_6939),
                        voxel(3, 0, 0, 0xFF5A_9643),
                    ],
                },
                ChunkRecord {
                    cx: 0,
                    cy: 1,
                    cz: 0,
                    v: Vec::new(),
                },
                ChunkRecord {
                    cx: 1,
                    cy: 0,
                    cz: 0,
                    v: vec![voxel(7, 0, 0, 0xFF00_00FF)],
                },
            ],
        }],
    }
}

pub fn primitive_record(kind: i32, label: &str) -> MeshRecord {
    MeshRecord {
        p: Vec3::new(1.0, 2.0, 3.0),
        s: Vec3::new(2.0, 2.0, 2.0),
        r: Vec3::new(0.0, 45.0, 0.0),
        o: Vec3::new(2.0, 3.0, 2.0),
        l: Some(label.to_owned()),
        tp: kind,
        tx: Some(rgba_texture("Cube", 2, 2)),
        mb: Some(BoundsRecord::default()),
        ..MeshRecord::default()
    }
}

pub fn custom_record(label: &str) -> MeshRecord {
    MeshRecord {
        tp: 0,
        tx: Some(rgba_texture("custom", 2, 2)),
        mb: Some(BoundsRecord {
            c: Vec3::new(0.5, 0.5, 0.0),
            s: Vec3::new(1.0, 1.0, 0.0),
        }),
        me: Some(MeshDataRecord {
            v: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            n: Some(vec![Vec3::Z; 3]),
            t: vec![0, 1, 2],
            u: Some(vec![Vec2::ZERO, Vec2::X, Vec2::Y]),
        }),
        sm: Some(vec![SubmeshRecord {
            t: vec![0, 1, 2],
            c: 0xFF80_4020_u32.cast_signed(),
            tx: Some(rgba_texture("sub", 1, 1)),
        }]),
        ..primitive_record(0, label)
    }
}

/// A file with every section, in the form Cubik writes it.
pub fn full_root() -> Root {
    let mut selected = element_record(
        "second",
        Vec3::new(4.0, 0.0, -4.0),
        Vec3::new(2.0, 2.0, 2.0),
    );
    selected.is = true;
    selected.r = Vec3::new(22.5, 0.0, 0.0);
    let mut props = ModelProps::default();
    props.save_path = "C:/models/a.cubik".to_owned();
    props.general.name = "Test model".to_owned();
    props.general.description = "Designed by  with Cubik Studio".to_owned();
    Root {
        v: 1,
        mp: Some(props),
        vp: Some(view_record()),
        bp: Some(blueprint_record()),
        el: Some(vec![
            element_record("first", Vec3::ZERO, Vec3::ONE),
            selected,
            element_record("third", Vec3::new(8.0, 8.0, -8.0), Vec3::new(1.0, 2.0, 3.0)),
        ]),
        tx: Some(vec![
            texture_record("particle", rgb_texture_with_mips("e_particle"), None),
            texture_record("texture", rgba_texture("", 3, 2), Some("C:/pics/t.png")),
        ]),
        vx: Some(voxel_world(16)),
        me: Some(vec![
            primitive_record(1, "Cube"),
            custom_record("Imported"),
            primitive_record(10, "Sprite"),
        ]),
        md: Some(DisplaySlots::default()),
    }
}

pub fn full_payload() -> Vec<u8> {
    encode::encode(&full_root()).expect("encodes")
}

pub fn arc<T>(value: T) -> Arc<T> {
    Arc::new(value)
}

// --- Comparison ---------------------------------------------------------------------------------

/// The root as a file lists it after a load and a save without retention: the documented
/// normalisations of native 6.3 applied to the records.
pub fn normalised(root: &Root) -> Root {
    let mut out = root.clone();
    for tx in out.tx.iter_mut().flatten() {
        // A null path becomes an empty one.
        tx.fp.get_or_insert_with(String::new);
    }
    if let Some(vx) = &mut out.vx {
        for world in &mut vx.w {
            world.c.retain(|chunk| !chunk.v.is_empty());
        }
    }
    out
}

/// The cameras compared as rotations, the rest of the view as it is; then the cameras are
/// cleared so that the rest of the roots can be compared with `==`.
pub fn same_cameras(a: &mut Root, b: &mut Root) {
    let (Some(va), Some(vb)) = (&mut a.vp, &mut b.vp) else {
        return;
    };
    for (ca, cb) in va.cameras.iter().zip(&vb.cameras) {
        match (ca, cb) {
            (None, None) => {}
            (Some(x), Some(y)) => {
                assert_eq!(
                    (x.p, x.o, x.os.to_bits(), x.fv.to_bits()),
                    (y.p, y.o, y.os.to_bits(), y.fv.to_bits())
                );
                let (qa, qb): (Quat, Quat) = (x.r.rotation, y.r.rotation);
                assert!(qa.dot(qb).abs() > 1.0 - 1e-6, "{qa:?} against {qb:?}");
            }
            _ => panic!("a camera appeared or vanished"),
        }
    }
    va.cameras = [None; 4];
    vb.cameras = [None; 4];
}

/// The first section or record that differs.
pub fn first_difference(got: &Root, want: &Root) -> Option<String> {
    macro_rules! items {
        ($name:literal, $a:expr, $b:expr) => {
            let (a, b) = (
                $a.as_deref().unwrap_or_default(),
                $b.as_deref().unwrap_or_default(),
            );
            if a.len() != b.len() {
                return Some(format!(
                    "{}: {} records against {}",
                    $name,
                    a.len(),
                    b.len()
                ));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                if x != y {
                    return Some(format!("{}[{i}]: {x:?} against {y:?}", $name));
                }
            }
        };
    }
    if got.mp != want.mp {
        return Some(format!("mp: {:?} against {:?}", got.mp, want.mp));
    }
    if got.vp != want.vp {
        return Some(format!("vp: {:?} against {:?}", got.vp, want.vp));
    }
    if got.bp != want.bp {
        return Some(format!("bp: {:?} against {:?}", got.bp, want.bp));
    }
    items!("el", got.el, want.el);
    items!("tx", got.tx, want.tx);
    items!("me", got.me, want.me);
    if got.vx != want.vx {
        return Some(format!("vx: {:?} against {:?}", got.vx, want.vx));
    }
    if got.md != want.md {
        return Some("md".to_owned());
    }
    None
}
