//! Open and save of a 150 MB payload (architecture 18: S2 saved, interactive within 2 s).
//!
//! Run with `cargo test -p cuboid-fmt-cubik --release --test perf -- --include-ignored --nocapture`.

mod common;

use std::io::Cursor;
use std::time::Instant;

use cuboid_fmt_cubik::records::{ChunkRecord, Root, VoxelData, VoxelRecord, WorldRecord};
use cuboid_fmt_cubik::{Limits, container, encode, load, save};
use cuboid_model::{
    ApplyMode, ApplySource, CaptureScope, Categories, Document, EditCtx, EditEnv, RenderPrefs,
    Selection, TextureList, apply_document, capture,
};
use cuboid_num::EngineRng;

/// R = 128, 1000 full chunks: 4.1 million voxels with 256 colours.
fn s2() -> Root {
    let mut chunks = Vec::new();
    for cx in 0..10 {
        for cy in 0..10 {
            for cz in 0..10 {
                let mut v = Vec::with_capacity(4096);
                for x in 0..16u8 {
                    for y in 0..16u8 {
                        for z in 0..16u8 {
                            let shade = (u32::from(x) * 16 + u32::from(y) + u32::from(z) * 3) % 256;
                            v.push(VoxelRecord {
                                x,
                                y,
                                z,
                                c: (0xFF00_0000 | shade << 16 | shade << 8 | 0x40).cast_signed(),
                            });
                        }
                    }
                }
                chunks.push(ChunkRecord { cx, cy, cz, v });
            }
        }
    }
    Root {
        vx: Some(VoxelData {
            w: vec![WorldRecord { r: 128, c: chunks }],
        }),
        ..common::full_root()
    }
}

fn lap(what: &str, since: &mut Instant) -> f64 {
    let seconds = since.elapsed().as_secs_f64();
    eprintln!("perf: {what}: {seconds:.3} s");
    *since = Instant::now();
    seconds
}

#[test]
#[ignore = "perf: a 150 MB payload; run with --release"]
fn a_150_mb_payload_opens_and_saves_within_the_budget() {
    let root = s2();
    let mut t = Instant::now();
    let payload = encode::encode(&root).unwrap();
    lap("encode records", &mut t);
    drop(root);
    eprintln!("perf: payload {:.1} MB", payload.len() as f64 / 1e6);
    assert!(payload.len() > 140_000_000, "the scene is S2 sized");

    let archive = container::write(&payload, None).unwrap();
    lap("compress container", &mut t);
    eprintln!("perf: container {:.1} MB", archive.len() as f64 / 1e6);

    let read = container::read(Cursor::new(&archive), &Limits::default()).unwrap();
    let inflate = lap("inflate container", &mut t);
    assert_eq!(read.payload.len(), payload.len());

    let loaded = load::load_payload(read.payload, &Limits::default()).unwrap();
    let decode = lap("decode and map to the model sections", &mut t);

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
    .unwrap();
    let apply = lap("apply (cuboid-model)", &mut t);

    let snapshot = capture(
        &doc,
        &sel,
        CaptureScope {
            categories: Categories::ALL,
            selection_only: false,
        },
    );
    lap("capture", &mut t);
    let kept = save::save_payload(&snapshot, doc.retained.as_deref(), &Default::default()).unwrap();
    lap("save, unchanged (copies the retained spans)", &mut t);
    assert_eq!(kept.len(), payload.len());
    let diff = kept.iter().zip(&payload).position(|(a, b)| a != b);
    assert_eq!(diff, None, "first differing offset");

    let plain = save::save_payload(&snapshot, None, &Default::default()).unwrap();
    let encode_all = lap("save, every record encoded again", &mut t);
    // A null path in the small texture records becomes an empty one.
    assert!(plain.len().abs_diff(payload.len()) < 64);

    if !cfg!(debug_assertions) {
        assert!(
            inflate + decode + apply < 2.0,
            "open took {:.2} s",
            inflate + decode + apply
        );
        assert!(encode_all < 2.0, "encoding took {encode_all:.2} s");
    }
}
