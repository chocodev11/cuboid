//! Codec unit tests: the worked byte examples of native 3.5, the sizes of 3.4, the lenient
//! forms of 3.6 and the record encodings (native 10.1).

use cuboid_bson::{DocWriter, Limits as BsonLimits, RawDoc};
use cuboid_model::{DisplaySlots, ModelKind, ModelProps};
use glam::{Vec2, Vec3};

use super::{embedded, hex, root};
use crate::decode::{self, PayloadErrorKind};
use crate::records::{
    ChunkRecord, ElementRecord, FaceRecord, FaceUvRecord, QuatRecord, RawTexture, Root,
    TextureRecord, ViewRecord, VoxelRecord,
};
use crate::{Limits, encode};

fn decoded(payload: &[u8]) -> decode::Decoded {
    decode::decode(payload, &Limits::default()).expect("decodes")
}

// --- The worked examples of native 3.5 ----------------------------------------------------------

#[test]
fn the_empty_document_is_five_bytes() {
    assert_eq!(root(|_| {}), hex("05 00 00 00 00"));
}

#[test]
fn vector3_of_one_two_three() {
    let bytes = embedded(|w| encode::vec3(w, b"v\x00", Vec3::new(1.0, 2.0, 3.0)));
    assert_eq!(
        bytes,
        hex("26 00 00 00
             01 78 00  00 00 00 00 00 00 F0 3F
             01 79 00  00 00 00 00 00 00 00 40
             01 7A 00  00 00 00 00 00 00 08 40
             00")
    );
}

#[test]
fn vector2_widens_the_f32_exactly() {
    let bytes = embedded(|w| encode::vec2(w, b"v\x00", Vec2::new(0.1, 0.5)));
    assert_eq!(
        bytes,
        hex("1B 00 00 00
             01 78 00  00 00 00 A0 99 99 B9 3F
             01 79 00  00 00 00 00 00 00 E0 3F
             00")
    );
}

#[test]
fn the_face_record_of_the_bottom_face() {
    let face = FaceRecord {
        n: Some("down".to_owned()),
        a: true,
        ti: -1,
        c: 0,
        t: Some("texture".to_owned()),
        uv: FaceUvRecord {
            r: 90,
            tl: Vec2::ZERO,
            br: Vec2::ONE,
        },
    };
    let bytes = embedded(|w| encode::face(w, b"v\x00", &face));
    assert_eq!(bytes.len(), 129);
    assert_eq!(
        bytes,
        hex("81 00 00 00
             02 6E 00 05 00 00 00 64 6F 77 6E 00
             08 61 00 01
             10 74 69 00 FF FF FF FF
             10 63 00 00 00 00 00
             02 74 00 08 00 00 00 74 65 78 74 75 72 65 00
             03 75 76 00 4A 00 00 00
                10 72 00 5A 00 00 00
                03 74 6C 00 1B 00 00 00 01 78 00 00 00 00 00 00 00 00 00
                                        01 79 00 00 00 00 00 00 00 00 00 00
                03 62 72 00 1B 00 00 00 01 78 00 00 00 00 00 00 00 F0 3F
                                        01 79 00 00 00 00 00 00 00 F0 3F 00
                00
             00")
    );
}

const VOXEL: &str = "21 00 00 00 10 78 00 01 00 00 00 10 79 00 02 00 00 00 10 7A 00 03 00 00 00 10 63 00 43 96 5A FF 00";

#[test]
fn the_voxel_record_is_33_bytes_and_reads_back_on_both_paths() {
    let rec = VoxelRecord {
        x: 1,
        y: 2,
        z: 3,
        c: 0xFF5A_9643_u32.cast_signed(),
    };
    assert_eq!(rec.c, -10_840_509);
    let mut w = DocWriter::new();
    let top = w.begin_root();
    encode::voxel(&mut w, 0, &rec);
    w.end(top);
    let bytes = w.finish().unwrap();
    assert_eq!(&bytes[4 + 3..bytes.len() - 1], hex(VOXEL).as_slice());

    // The same record with its members in another order takes the generic path.
    let shuffled = root(|w| {
        let vx = w.begin_doc(b"vx\x00");
        let worlds = w.begin_array(b"w\x00");
        let world = w.begin_doc(0usize);
        w.i32(b"r\x00", 1);
        let chunks = w.begin_array(b"c\x00");
        let chunk = w.begin_doc(0usize);
        w.i32(b"cx\x00", 0);
        w.i32(b"cy\x00", 0);
        w.i32(b"cz\x00", 0);
        let v = w.begin_array(b"v\x00");
        let item = w.begin_doc(0usize);
        w.i32(b"c\x00", -10_840_509);
        w.i32(b"z\x00", 3);
        w.i32(b"y\x00", 2);
        w.i32(b"x\x00", 1);
        w.end(item);
        encode::voxel(
            w,
            1,
            &VoxelRecord {
                x: 4,
                y: 5,
                z: 6,
                c: -1,
            },
        );
        w.end(v);
        w.end(chunk);
        w.end(chunks);
        w.end(world);
        w.end(worlds);
        w.end(vx);
    });
    let out = decoded(&shuffled).root.vx.unwrap();
    let chunk = &out.w[0].c[0];
    assert_eq!(chunk.v[0], rec);
    assert_eq!(
        chunk.v[1],
        VoxelRecord {
            x: 4,
            y: 5,
            z: 6,
            c: -1
        }
    );
}

#[test]
fn vector_and_quaternion_documents_have_the_documented_sizes() {
    assert_eq!(
        embedded(|w| encode::vec2(w, b"v\x00", Vec2::ZERO)).len(),
        27
    );
    assert_eq!(
        embedded(|w| encode::vec3(w, b"v\x00", Vec3::ZERO)).len(),
        38
    );
    let q = QuatRecord {
        euler: Some(Vec3::ZERO),
        ..QuatRecord::IDENTITY
    };
    assert_eq!(embedded(|w| encode::quaternion(w, b"v\x00", &q)).len(), 100);
}

#[test]
fn f32_values_survive_the_widening_exactly() {
    let samples = [
        0.0_f32,
        -0.0,
        0.1,
        1.0,
        -2.5,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::from_bits(0x007F_FFFF),
        f32::MAX,
        f32::MIN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        3.478_199_958_801_269_5,
    ];
    for x in samples {
        let bytes = root(|w| w.f32(b"x\x00", x));
        let doc = RawDoc::parse(&bytes, &BsonLimits::DEFAULT).unwrap();
        let elem = doc.iter().next().unwrap().unwrap();
        assert_eq!(elem.f32().unwrap().to_bits(), x.to_bits(), "{x:?}");
        assert_eq!(elem.value_bytes(), f64::from(x).to_le_bytes());
    }
    let nan = root(|w| w.f32(b"x\x00", f32::NAN));
    let doc = RawDoc::parse(&nan, &BsonLimits::DEFAULT).unwrap();
    assert!(doc.iter().next().unwrap().unwrap().f32().unwrap().is_nan());
}

// --- Record encodings ---------------------------------------------------------------------------

fn raw_texture() -> RawTexture {
    RawTexture {
        n: "a".to_owned(),
        w: 1,
        h: 1,
        f: 4,
        r: [1u8, 2, 3, 4].into(),
    }
}

const RAW_TEXTURE: &str = "2F 00 00 00
    02 6E 00 02 00 00 00 61 00
    10 77 00 01 00 00 00
    10 68 00 01 00 00 00
    10 66 00 04 00 00 00
    05 72 00 04 00 00 00 00 01 02 03 04
    00";

#[test]
fn a_raw_texture_record_matches_the_hand_computed_bytes() {
    let bytes = embedded(|w| encode::raw_texture(w, b"v\x00", &raw_texture()));
    assert_eq!(bytes, hex(RAW_TEXTURE));
}

#[test]
fn a_texture_record_writes_null_for_a_missing_path_name_and_signature() {
    let rec = TextureRecord {
        lb: "t".to_owned(),
        fp: None,
        fname: None,
        fm: String::new(),
        tx: raw_texture(),
        cs: None,
    };
    let bytes = embedded(|w| encode::texture(w, b"v\x00", &rec));
    let mut expected = hex("57 00 00 00
        02 6C 62 00 02 00 00 00 74 00
        0A 66 70 00
        0A 66 6E 00
        02 66 6D 00 01 00 00 00 00
        03 74 78 00");
    expected.extend(hex(RAW_TEXTURE));
    expected.extend(hex("0A 63 73 00 00"));
    assert_eq!(bytes, expected);
}

#[test]
fn a_chunk_without_voxels_is_written_with_an_empty_array() {
    let chunk = ChunkRecord {
        cx: 1,
        cy: 2,
        cz: 3,
        v: Vec::new(),
    };
    let fragment = encode::chunk_fragment(0, &chunk);
    let mut expected = hex("03 30 00");
    expected.extend(hex("25 00 00 00
        10 63 78 00 01 00 00 00
        10 63 79 00 02 00 00 00
        10 63 7A 00 03 00 00 00
        04 76 00 05 00 00 00 00
        00"));
    assert_eq!(fragment, expected);
}

#[test]
fn the_empty_option_groups_ht_and_tr_are_written() {
    let bytes = root(|w| encode::props(w, b"mp\x00", &ModelProps::default()));
    let empty = hex("05 00 00 00 00");
    for key in [b"ht", b"tr"] {
        let mut pattern = vec![0x03, key[0], key[1], 0];
        pattern.extend(&empty);
        assert!(
            bytes.windows(pattern.len()).any(|w| w == pattern),
            "{key:?}"
        );
    }
}

#[test]
fn the_root_writes_its_keys_in_the_order_of_the_schema() {
    let full = Root {
        mp: Some(ModelProps::default()),
        vp: Some(ViewRecord::default()),
        bp: Some(ElementRecord::default()),
        el: Some(Vec::new()),
        tx: Some(Vec::new()),
        me: Some(Vec::new()),
        md: Some(DisplaySlots::default()),
        ..Root::default()
    };
    let bytes = encode::encode(&full).unwrap();
    let doc = RawDoc::parse(&bytes, &BsonLimits::DEFAULT).unwrap();
    let keys: Vec<String> = doc
        .iter()
        .map(|e| String::from_utf8(e.unwrap().key.to_vec()).unwrap())
        .collect();
    assert_eq!(keys, ["v", "mp", "vp", "bp", "el", "tx", "vx", "me", "md"]);
    // A root without a voxel model writes `vx` as null.
    assert!(bytes.windows(4).any(|w| w == hex("0A 76 78 00").as_slice()));
}

// --- Lenient reading (native 3.6) ---------------------------------------------------------------

#[test]
fn numbers_booleans_and_enumerations_convert_like_cubiks_reader() {
    let payload = root(|w| {
        let mp = w.begin_doc(b"mp\x00");
        let g = w.begin_doc(b"g\x00");
        w.i32(b"ee\x00", 5); // int as bool
        w.f64(b"rer\x00", 0.0); // double as bool
        w.bool(b"ev\x00", true);
        let o = w.begin_doc(b"o\x00");
        w.i32(b"x\x00", 2); // int as float
        w.i64(b"y\x00", 3); // int64 as float
        w.f64(b"z\x00", 0.5);
        w.end(o);
        w.end(g);
        let mc = w.begin_doc(b"mc\x00");
        w.str(b"t\x00", "ITEM"); // enumeration by name, any case
        w.f64(b"etr\x00", 2.5); // double as int: half to even
        w.f64(b"etx\x00", 3.5);
        w.end(mc);
        w.end(mp);
    });
    let mp = decoded(&payload).root.mp.unwrap();
    assert!(mp.general.exclude_elements && mp.general.exclude_voxels);
    assert!(!mp.general.restrict_rotations);
    assert_eq!(mp.general.origin, Vec3::new(2.0, 3.0, 0.5));
    assert_eq!(mp.minecraft.kind, ModelKind::Item);
    assert_eq!(mp.minecraft.entity_resolution, 2);
    assert_eq!(mp.minecraft.entity_offset_x, 4);
}

#[test]
fn a_binary_value_may_be_a_base64_string() {
    let payload = root(|w| {
        let tx = w.begin_array(b"tx\x00");
        let item = w.begin_doc(0usize);
        w.str(b"lb\x00", "t");
        let image = w.begin_doc(b"tx\x00");
        w.i32(b"w\x00", 1);
        w.i32(b"h\x00", 1);
        w.i32(b"f\x00", 4);
        w.str(b"r\x00", "AQIDBA==");
        w.end(image);
        w.end(item);
        w.end(tx);
    });
    let tx = decoded(&payload).root.tx.unwrap();
    assert_eq!(&*tx[0].tx.r, [1u8, 2, 3, 4]);
}

#[test]
fn a_null_for_a_value_type_is_absent_with_a_warning() {
    let payload = root(|w| {
        let mp = w.begin_doc(b"mp\x00");
        let g = w.begin_doc(b"g\x00");
        w.null(b"ee\x00");
        w.null(b"o\x00");
        w.str(b"n\x00", "named");
        w.end(g);
        w.null(b"mc\x00");
        w.end(mp);
    });
    let out = decoded(&payload);
    let mp = out.root.mp.unwrap();
    assert!(!mp.general.exclude_elements);
    assert_eq!(mp.general.origin, Vec3::ZERO);
    assert_eq!(mp.general.name, "named");
    assert_eq!(mp.minecraft, ModelProps::default().minecraft);
    assert_eq!(out.warnings.len(), 3, "{:?}", out.warnings);
}

#[test]
fn repeated_keys_keep_the_last_value_and_unknown_keys_are_ignored() {
    let payload = root(|w| {
        w.i32(b"v\x00", 7);
        w.i32(b"v\x00", 9);
        w.str(b"zz\x00", "unknown");
        let mp = w.begin_doc(b"mp\x00");
        let g = w.begin_doc(b"g\x00");
        w.str(b"n\x00", "first");
        w.str(b"n\x00", "second");
        w.end(g);
        w.end(mp);
    });
    let out = decoded(&payload);
    assert_eq!(out.root.v, 9);
    assert!(out.warnings.iter().any(|w| w.contains("format version 9")));
    assert_eq!(out.root.mp.unwrap().general.name, "second");
}

#[test]
fn missing_keys_keep_the_value_of_a_fresh_record() {
    let payload = root(|w| {
        let bp = w.begin_doc(b"bp\x00");
        w.end(bp);
        let vp = w.begin_doc(b"vp\x00");
        w.end(vp);
    });
    let root = decoded(&payload).root;
    assert_eq!(root.bp.unwrap(), ElementRecord::default());
    assert_eq!(root.vp.unwrap(), ViewRecord::default());
    assert!(root.mp.is_none() && root.el.is_none() && root.vx.is_none());
}

#[test]
fn quaternion_members_apply_in_document_order_with_the_euler_override() {
    let quat = |order: &[&str]| {
        root(|w| {
            let vp = w.begin_doc(b"vp\x00");
            let cam = w.begin_doc(b"ctl\x00");
            let r = w.begin_doc(b"r\x00");
            for key in order {
                match *key {
                    "x" => w.f32(b"x\x00", 0.0),
                    "w" => w.f32(b"w\x00", 1.0),
                    "euler" => encode::vec3(w, b"eulerAngles\x00", Vec3::new(0.0, 90.0, 0.0)),
                    _ => {}
                }
            }
            w.end(r);
            w.end(cam);
            w.end(vp);
        })
    };
    let rotation = |order: &[&str]| {
        decoded(&quat(order)).root.vp.unwrap().cameras[0]
            .unwrap()
            .r
            .rotation
    };
    let half = std::f32::consts::FRAC_1_SQRT_2;
    // Members before `eulerAngles` are replaced by it.
    let by_euler = rotation(&["x", "w", "euler"]);
    assert!((by_euler.y - half).abs() < 1e-6);
    // A member after it changes one component of the Euler rotation.
    let after = rotation(&["euler", "w"]);
    assert!((after.w - 1.0).abs() < 1e-6 && (after.y - half).abs() < 1e-6);
    // Without it the stored components stand.
    assert_eq!(rotation(&["x", "w"]), glam::Quat::IDENTITY);
}

#[test]
fn a_malformed_payload_names_the_failing_key() {
    let payload = root(|w| {
        let el = w.begin_array(b"el\x00");
        let item = w.begin_doc(0usize);
        w.str(b"c\x00", "not a number");
        w.end(item);
        w.end(el);
    });
    let err = decode::decode(&payload, &Limits::default()).unwrap_err();
    assert_eq!(err.path, "el[0].c");
    assert!(matches!(err.kind, PayloadErrorKind::Bson(_)));
}

fn world_payload(res: i32, chunks: &[(i32, i32)]) -> Vec<u8> {
    root(|w| {
        let vx = w.begin_doc(b"vx\x00");
        let worlds = w.begin_array(b"w\x00");
        let world = w.begin_doc(0usize);
        w.i32(b"r\x00", res);
        let list = w.begin_array(b"c\x00");
        for (i, &(cx, x)) in chunks.iter().enumerate() {
            let chunk = w.begin_doc(i);
            w.i32(b"cx\x00", cx);
            w.i32(b"cy\x00", 0);
            w.i32(b"cz\x00", 0);
            let v = w.begin_array(b"v\x00");
            // A local coordinate of 16 is outside the chunk.
            let item = w.begin_doc(0usize);
            w.i32(b"x\x00", x);
            w.i32(b"y\x00", 0);
            w.i32(b"z\x00", 0);
            w.i32(b"c\x00", -1);
            w.end(item);
            w.end(v);
            w.end(chunk);
        }
        w.end(list);
        w.end(world);
        w.end(worlds);
        w.end(vx);
    })
}

#[test]
fn a_chunk_outside_the_grid_and_a_voxel_outside_the_chunk_are_skipped() {
    // R = 1: 3 cells per axis, one chunk. The second record has cx = 1.
    let out = decoded(&world_payload(1, &[(0, 15), (1, 0), (0, 16)]));
    let chunks = &out.root.vx.unwrap().w[0].c;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].v.len(), 1);
    assert!(chunks[1].v.is_empty());
    assert!(out.warnings.iter().any(|w| w.contains("skipped")));
}

#[test]
fn limits_are_checked_before_allocating() {
    let big_list = root(|w| {
        let el = w.begin_array(b"el\x00");
        for i in 0..5usize {
            let item = w.begin_doc(i);
            w.end(item);
        }
        w.end(el);
    });
    let limits = Limits {
        max_objects: 4,
        ..Limits::default()
    };
    assert!(decode::decode(&big_list, &limits).is_err());

    for res in [0, -3, i32::MAX] {
        assert!(
            decode::decode(&world_payload(res, &[]), &Limits::default()).is_err(),
            "{res}"
        );
    }
    assert!(decode::decode(&world_payload(254, &[]), &Limits::default()).is_ok());
    let small = Limits {
        voxel_budget: 1 << 10,
        ..Limits::default()
    };
    assert!(decode::decode(&world_payload(128, &[]), &small).is_err());
}

#[test]
fn the_nesting_depth_is_limited() {
    // vp > ctl > r > eulerAngles is five levels deep.
    let cam = crate::records::CameraRecord {
        p: Vec3::ZERO,
        r: QuatRecord {
            euler: Some(Vec3::ZERO),
            ..QuatRecord::IDENTITY
        },
        o: false,
        os: 1.0,
        fv: 60.0,
    };
    let view = ViewRecord {
        cameras: [Some(cam), None, None, None],
        ..ViewRecord::default()
    };
    let payload = root(|w| encode::view(w, b"vp\x00", &view));
    assert!(decode::decode(&payload, &Limits::default()).is_ok());
    let tight = Limits {
        max_depth: 4,
        ..Limits::default()
    };
    let err = decode::decode(&payload, &tight).unwrap_err();
    assert!(err.to_string().contains("nesting"), "{err}");
}
