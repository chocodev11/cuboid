//! Payload identity and the semantic round trip on generated documents (native 10.2, arch 19).
//!
//! The documents are in the form Cubik writes: rotations on the grid of the restriction setting,
//! colours with full alpha, UV corners inside [0, 1], voxel records in scan order.

#![allow(clippy::field_reassign_with_default)]

mod common;

use cuboid_fmt_cubik::records::{
    ChunkRecord, ElementRecord, FaceRecord, FaceSetRecord, FaceUvRecord, MeshDataRecord,
    MeshRecord, RawTexture, Root, SubmeshRecord, TextureRecord, VoxelData, VoxelRecord,
    WorldRecord,
};
use cuboid_fmt_cubik::{decode, encode};
use cuboid_model::{DisplaySlot, DisplaySlots, ModelKind, ModelProps};
use glam::{Vec2, Vec3};
use proptest::prelude::*;

use common::*;

fn quarter() -> impl Strategy<Value = f32> {
    (-64i32..64).prop_map(|n| n as f32 * 0.25)
}

fn position() -> impl Strategy<Value = Vec3> {
    prop::array::uniform3(quarter()).prop_map(Vec3::from_array)
}

fn size() -> impl Strategy<Value = Vec3> {
    prop::array::uniform3(1i32..32).prop_map(|n| Vec3::from_array(n.map(|n| n as f32 * 0.25)))
}

fn rotation(step: f32, count: i32) -> impl Strategy<Value = Vec3> {
    prop::array::uniform3(0..count).prop_map(move |n| Vec3::from_array(n.map(|n| n as f32 * step)))
}

fn uv() -> impl Strategy<Value = FaceUvRecord> {
    let c = (0..=4).prop_map(|n| n as f32 * 0.25);
    (0..4i32, prop::array::uniform4(c)).prop_map(|(quarter_turns, [a, b, c, d])| FaceUvRecord {
        r: quarter_turns * 90,
        tl: Vec2::new(a, b),
        br: Vec2::new(c, d),
    })
}

fn label() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["texture", "particle", "a b", "caf\u{e9}", ""])
        .prop_map(str::to_owned)
}

fn element() -> impl Strategy<Value = ElementRecord> {
    (
        (position(), size(), rotation(45.0, 8), position()),
        (0u32..0x0100_0000, any::<[bool; 7]>(), 0..4i32),
        prop::collection::vec(
            (
                uv(),
                label(),
                -1..4i32,
                prop::sample::select(vec![0, 1, 2, 4, 8, 16, 32]),
                any::<bool>(),
            ),
            6..=6,
        ),
        label(),
    )
        .prop_map(|((p, s, r, pivot), (rgb, flags, kind), faces, name)| {
            let faces = std::array::from_fn(|slot| {
                let (uv, texture, tint, cull, active) = faces[slot].clone();
                FaceRecord {
                    a: active,
                    ti: tint,
                    c: cull,
                    ..face(slot, uv, &texture)
                }
            });
            ElementRecord {
                p,
                s,
                r,
                o: p + pivot,
                c: (0xFF00_0000 | rgb).cast_signed(),
                is: flags[0],
                ih: flags[1],
                il: flags[2],
                l: Some(name),
                f: Some(FaceSetRecord { faces }),
                rs: flags[3],
                suv: flags[4],
                suvm: flags[5],
                sh: flags[6],
                inv: false,
                t: kind,
            }
        })
}

fn pixels() -> impl Strategy<Value = RawTexture> {
    (
        1..4i32,
        1..4i32,
        any::<bool>(),
        prop::collection::vec(any::<u8>(), 64),
        prop::sample::select(vec!["", "e_texture", "image"]),
    )
        .prop_map(|(w, h, rgb, noise, name)| {
            let texels = usize::try_from(w * h).unwrap();
            let (f, bytes) = if rgb {
                // Level 0 and a tail that is not a whole chain.
                (3, texels * 3 + 5)
            } else {
                (4, texels * 4)
            };
            RawTexture {
                n: name.to_owned(),
                w,
                h,
                f,
                r: noise.iter().cycle().take(bytes).copied().collect(),
            }
        })
}

fn textures() -> impl Strategy<Value = Vec<TextureRecord>> {
    prop::sample::subsequence(vec!["particle", "texture", "a", "b"], 0..=4).prop_flat_map(
        |labels| {
            let count = labels.len();
            (
                Just(labels),
                prop::collection::vec(
                    (
                        pixels(),
                        prop::sample::select(vec![None, Some(""), Some("C:/pics/x.png")]),
                    ),
                    count..=count,
                ),
            )
                .prop_map(|(labels, items)| {
                    labels
                        .into_iter()
                        .zip(items)
                        .map(|(label, (tx, fp))| texture_record(label, tx, fp))
                        .collect()
                })
        },
    )
}

fn voxels() -> impl Strategy<Value = Option<VoxelData>> {
    let cells =
        prop::collection::btree_set((0u32..48, 0u32..48, 0u32..48, 1u32..0x0100_0000), 0..40);
    prop::option::of((
        prop::sample::select(vec![1i32, 2, 4, 16]),
        cells,
        any::<bool>(),
    ))
    .prop_map(|found| {
        found.map(|(res, cells, with_empty)| {
            let limit = u32::try_from(3 * res).unwrap();
            let n = (3 * res + 15) / 16;
            let mut chunks: std::collections::BTreeMap<(i32, i32, i32), Vec<VoxelRecord>> =
                std::collections::BTreeMap::new();
            let mut seen = std::collections::BTreeSet::new();
            for (x, y, z, rgb) in cells {
                let (x, y, z) = (x % limit, y % limit, z % limit);
                if !seen.insert((x, y, z)) {
                    continue;
                }
                let key = (
                    i32::try_from(x / 16).unwrap(),
                    i32::try_from(y / 16).unwrap(),
                    i32::try_from(z / 16).unwrap(),
                );
                chunks.entry(key).or_default().push(VoxelRecord {
                    x: u8::try_from(x % 16).unwrap(),
                    y: u8::try_from(y % 16).unwrap(),
                    z: u8::try_from(z % 16).unwrap(),
                    c: (0xFF00_0000 | rgb).cast_signed(),
                });
            }
            if with_empty {
                chunks.entry((n - 1, n - 1, n - 1)).or_default();
            }
            let c = chunks
                .into_iter()
                .map(|((cx, cy, cz), mut v)| {
                    v.sort_by_key(|v| (v.x, v.y, v.z));
                    ChunkRecord { cx, cy, cz, v }
                })
                .collect();
            VoxelData {
                w: vec![WorldRecord { r: res, c }],
            }
        })
    })
}

fn mesh() -> impl Strategy<Value = MeshRecord> {
    (
        (position(), size(), rotation(5.0, 72), position()),
        (0..11i32, any::<[bool; 3]>(), label()),
        prop::option::of(pixels()),
        prop::sample::select(vec![0, 1, 2]),
    )
        .prop_map(|((p, s, r, o), (kind, flags, name), tx, normals)| {
            let base = MeshRecord {
                p,
                s,
                r,
                o,
                is: flags[0],
                ih: flags[1],
                il: flags[2],
                l: Some(name.clone()),
                tp: kind,
                tx,
                mb: Some(Default::default()),
                ..MeshRecord::default()
            };
            if kind != 0 {
                return base;
            }
            let v = vec![p, p + Vec3::X, p + Vec3::Y];
            MeshRecord {
                me: Some(MeshDataRecord {
                    n: match normals {
                        0 => Some(Vec::new()),
                        1 => Some(vec![Vec3::Z; 3]),
                        _ => Some(vec![Vec3::Y; 3]),
                    },
                    t: vec![0, 1, 2],
                    u: Some(vec![Vec2::ZERO, Vec2::X, Vec2::Y]),
                    v,
                }),
                sm: (name.len() % 2 == 0).then(|| {
                    vec![SubmeshRecord {
                        t: vec![2, 1, 0],
                        c: 0xFF80_4020_u32.cast_signed(),
                        tx: base.tx.clone(),
                    }]
                }),
                ..base
            }
        })
}

fn display() -> impl Strategy<Value = DisplaySlots> {
    let slot = (
        prop::sample::select(vec!["Default", "Custom"]),
        any::<[bool; 2]>(),
        position(),
        position(),
        prop::array::uniform3(1i32..16).prop_map(|n| Vec3::from_array(n.map(|n| n as f32 * 0.25))),
    )
        .prop_map(
            |(preset, [removable, enabled], rotation, translation, scale)| DisplaySlot {
                preset: preset.to_owned(),
                removable,
                enabled,
                rotation,
                translation,
                scale,
            },
        );
    prop::collection::vec(slot, 8..=8).prop_map(|slots| DisplaySlots {
        slots: std::array::from_fn(|i| slots[i].clone()),
    })
}

fn props() -> impl Strategy<Value = ModelProps> {
    (
        prop::sample::select(vec!["Cubik Model", "my model", "caf\u{e9} \u{1d11e}"]),
        any::<[bool; 6]>(),
        prop::sample::select(vec![ModelKind::Block, ModelKind::Item, ModelKind::Entity]),
        prop::sample::select(vec!["", "C:/models/a.cubik", "D:/x y/b.cubik"]),
    )
        .prop_map(|(name, flags, kind, path)| {
            let mut p = ModelProps::default();
            p.save_path = path.to_owned();
            p.general.name = name.to_owned();
            p.general.exclude_elements = flags[0];
            p.general.minify = flags[1];
            p.general.restrict_rotations = flags[2];
            p.minecraft.ambient_occlusion = flags[3];
            p.mesh_export.weld_vertices = flags[4];
            p.png.antialias = flags[5];
            p.minecraft.kind = kind;
            p
        })
}

fn view() -> impl Strategy<Value = cuboid_fmt_cubik::records::ViewRecord> {
    let cam = prop::option::of((position(), rotation(15.0, 24), any::<bool>()))
        .prop_map(|c| c.map(|(pos, euler, ortho)| camera(pos, euler, ortho)));
    (
        prop::array::uniform4(cam),
        prop::sample::select(vec![0.25f32, 0.5, 1.0]),
    )
        .prop_map(|(cameras, step)| cuboid_fmt_cubik::records::ViewRecord {
            cameras,
            gm: step,
            gk: step / 2.0,
            gu: 0.5,
            gd: 1.0,
            gl: false,
        })
}

fn root() -> impl Strategy<Value = Root> {
    (
        (props(), view(), position(), rotation(45.0, 8)),
        prop::collection::vec(element(), 0..4),
        textures(),
        voxels(),
        prop::collection::vec(mesh(), 0..4),
        display(),
    )
        .prop_map(|((mp, vp, bp_pos, bp_rot), el, tx, vx, me, md)| {
            let mut bp = blueprint_record();
            bp.p = bp_pos;
            bp.o = bp_pos;
            bp.r = bp_rot;
            Root {
                v: 1,
                mp: Some(mp),
                vp: Some(vp),
                bp: Some(bp),
                el: Some(el),
                tx: Some(tx),
                vx,
                me: Some(me),
                md: Some(md),
            }
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn the_record_codec_reproduces_generated_payloads(root in root()) {
        let bytes = encode::encode(&root).unwrap();
        let decoded = decode::decode(&bytes, &Default::default()).unwrap();
        prop_assert_eq!(&decoded.root, &root);
        prop_assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
        prop_assert_eq!(encode::encode(&decoded.root).unwrap(), bytes);
    }

    #[test]
    fn an_unchanged_generated_document_saves_the_identical_payload(root in root()) {
        let bytes = encode::encode(&root).unwrap();
        let (doc, sel) = open(load_bytes(&bytes));
        let saved = save_payload(&doc, &sel, &Default::default());
        prop_assert!(saved == bytes, "payload identity");
    }

    #[test]
    fn the_semantic_round_trip_keeps_generated_documents(root in root()) {
        let bytes = encode::encode(&root).unwrap();
        let (doc, sel) = open(load_bytes(&bytes));
        let saved = save_payload(&doc, &sel, &without_retention());
        let mut got = decode(&saved);
        let mut want = normalised(&root);
        same_cameras(&mut want, &mut got);
        prop_assert!(first_difference(&got, &want).is_none(), "{:?}", first_difference(&got, &want));
    }

    #[test]
    fn a_saved_document_is_stable_under_another_load_and_save(root in root()) {
        let bytes = encode::encode(&root).unwrap();
        let (doc, sel) = open(load_bytes(&bytes));
        let first = save_payload(&doc, &sel, &without_retention());
        let (doc, sel) = open(load_bytes(&first));
        prop_assert!(save_payload(&doc, &sel, &Default::default()) == first);
        // Only the Euler copies of the cameras may move by a rounding step (native 4.3.2).
        let again = save_payload(&doc, &sel, &without_retention());
        let (mut a, mut b) = (decode(&again), decode(&first));
        same_cameras(&mut a, &mut b);
        prop_assert!(first_difference(&a, &b).is_none());
    }
}
