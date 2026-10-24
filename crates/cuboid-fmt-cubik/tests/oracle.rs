//! Tests against private samples of the original program (oracle O1). They read every
//! `.cubik` file of the folder that `CUBOID_PARITY_REFS` names and print nothing about its
//! content. Run them with `cargo test -p cuboid-fmt-cubik -- --include-ignored`.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use cuboid_fmt_cubik::{Limits, container, decode, encode};
use cuboid_testkit::oracle::{self, Oracle};

/// The payloads of the samples, with a number instead of a name for the messages.
fn payloads() -> Option<Vec<(usize, Vec<u8>)>> {
    let dir = oracle::require(Oracle::ParityRefs)?;
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "cubik"))
        .collect();
    files.sort();
    let payloads = files
        .iter()
        .enumerate()
        .map(|(n, path)| {
            let bytes = fs::read(path).expect("readable sample");
            let limits = Limits::default();
            let container = container::read(Cursor::new(bytes), &limits)
                .unwrap_or_else(|err| panic!("sample {n}: {err}"));
            (n, container.payload)
        })
        .collect();
    Some(payloads)
}

#[test]
#[ignore = "oracle O1: CUBOID_PARITY_REFS"]
fn the_record_codec_reproduces_the_payload_of_every_sample_byte_for_byte() {
    let Some(samples) = payloads() else { return };
    assert!(
        !samples.is_empty(),
        "the oracle folder holds no .cubik file"
    );
    for (n, payload) in samples {
        let decoded = decode::decode(&payload, &Limits::default())
            .unwrap_or_else(|err| panic!("sample {n}: {err}"));
        let again = encode::encode(&decoded.root).expect("encodes");
        assert_eq!(again.len(), payload.len(), "sample {n}: payload length");
        let first_diff = again.iter().zip(&payload).position(|(a, b)| a != b);
        assert_eq!(first_diff, None, "sample {n}: first differing offset");
    }
}

mod common;

#[test]
#[ignore = "oracle O1: CUBOID_PARITY_REFS"]
fn a_loaded_and_unchanged_sample_saves_the_identical_payload() {
    let Some(samples) = payloads() else { return };
    for (n, payload) in samples {
        let loaded = cuboid_fmt_cubik::load::load_payload(payload.clone(), &Limits::default())
            .unwrap_or_else(|err| panic!("sample {n}: {err}"));
        let (doc, sel) = common::open(loaded);
        let again = common::save_payload(&doc, &sel, &cuboid_fmt_cubik::SaveOptions::default());
        assert_eq!(again.len(), payload.len(), "sample {n}: payload length");
        let first_diff = again.iter().zip(&payload).position(|(a, b)| a != b);
        assert_eq!(first_diff, None, "sample {n}: first differing offset");
    }
}

#[test]
#[ignore = "oracle O1: CUBOID_PARITY_REFS"]
fn the_semantic_round_trip_of_every_sample_keeps_its_content() {
    let Some(samples) = payloads() else { return };
    for (n, payload) in samples {
        let original = common::decode(&payload);
        let loaded = common::load_bytes(&payload);
        let (doc, sel) = common::open(loaded);
        let saved = common::save_payload(&doc, &sel, &common::without_retention());
        let mut got = common::decode(&saved);
        let mut want = common::normalised(&original);
        common::same_cameras(&mut want, &mut got);
        assert_eq!(common::first_difference(&got, &want), None, "sample {n}");
    }
}

#[test]
#[ignore = "oracle O1: CUBOID_PARITY_REFS"]
fn the_canonical_dump_of_every_sample_lists_the_raw_texture_levels() {
    let Some(samples) = payloads() else { return };
    for (n, payload) in samples {
        let text = cuboid_fmt_cubik::dump::payload(&payload).expect("dumps");
        assert!(text.contains(".r.level0: bytes = len="), "sample {n}");
        assert!(text.contains(".r.tail: i32 = "), "sample {n}");
    }
}
