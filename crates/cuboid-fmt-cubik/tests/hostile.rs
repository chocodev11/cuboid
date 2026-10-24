//! Hostile input never panics and never allocates without bound (native 8.1 item 4, 10.4). The
//! libFuzzer targets do the real work; these tests run the same entry points on damaged
//! versions of a valid file.

mod common;

use std::io::Cursor;
use std::path::Path;

use cuboid_fmt_cubik::{Limits, container, cub, load_bytes};

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

fn try_payload(bytes: Vec<u8>) {
    // Whether the result is an error or a document does not matter; the call must return.
    let _ = cuboid_fmt_cubik::load::load_payload(bytes, &Limits::default());
}

#[test]
fn every_truncation_of_a_valid_payload_returns() {
    let payload = common::full_payload();
    for len in (0..payload.len()).step_by(3) {
        try_payload(payload[..len].to_vec());
    }
}

#[test]
fn damaged_payloads_return() {
    let payload = common::full_payload();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for round in 0..3000 {
        let mut bytes = payload.clone();
        for _ in 0..=(round % 4) {
            let at = rng.below(bytes.len());
            bytes[at] = u8::try_from(rng.next() & 0xFF).unwrap();
        }
        try_payload(bytes);
    }
}

#[test]
fn damaged_size_fields_cannot_make_the_reader_allocate_without_bound() {
    let payload = common::full_payload();
    let mut rng = Rng(7);
    for _ in 0..2000 {
        let mut bytes = payload.clone();
        // Overwrite four bytes with a huge little-endian length.
        let at = rng.below(bytes.len() - 4);
        bytes[at..at + 4].copy_from_slice(&0x7FFF_FF00_u32.to_le_bytes());
        try_payload(bytes);
    }
}

#[test]
fn random_bytes_are_neither_a_container_nor_a_payload_nor_a_cub() {
    let mut rng = Rng(42);
    for _ in 0..500 {
        let len = rng.below(300);
        let bytes: Vec<u8> = (0..len)
            .map(|_| u8::try_from(rng.next() & 0xFF).unwrap())
            .collect();
        let _ = container::read(Cursor::new(&bytes), &Limits::default());
        let _ = load_bytes(&bytes, &Limits::default());
        let _ = cub::import(&bytes, Path::new("x.cub"), &cub::DiskFiles);
        try_payload(bytes);
    }
}

#[test]
fn damaged_archives_return() {
    let archive = container::write(&common::full_payload(), None).unwrap();
    let mut rng = Rng(99);
    for round in 0..1500 {
        let mut bytes = archive.clone();
        for _ in 0..=(round % 3) {
            let at = rng.below(bytes.len());
            bytes[at] ^= u8::try_from(rng.next() & 0xFF).unwrap() | 1;
        }
        let _ = load_bytes(&bytes, &Limits::default());
    }
}

#[test]
fn damaged_cub_files_return() {
    let text = r##"{"name":"m","textures_path":{"t":"t.png"},"elements":[{"from":[0,0,0],"to":[1,1,1],"faces":{"up":{"texture":"#t","uv":[0,0,4,4]}}}],"voxel":{"resolution":2,"voxels":[[0,0,0,5]]}}"##;
    let valid = cub::deobfuscate(text.as_bytes()).into_bytes();
    let mut rng = Rng(5);
    for round in 0..2000 {
        let mut bytes = valid.clone();
        for _ in 0..=(round % 3) {
            let at = rng.below(bytes.len());
            bytes[at] = u8::try_from(rng.next() & 0xFF).unwrap();
        }
        let _ = cub::import(&bytes, Path::new("m.cub"), &cub::DiskFiles);
    }
}

/// `depth` documents inside each other under the key `key`.
fn nested(key: u8, depth: usize) -> Vec<u8> {
    let mut inner = vec![5u8, 0, 0, 0, 0];
    for _ in 0..depth {
        let mut doc = vec![0u8; 4];
        doc.extend_from_slice(&[0x03, key, 0]);
        doc.extend(&inner);
        doc.push(0);
        let len = u32::try_from(doc.len()).unwrap();
        doc[..4].copy_from_slice(&len.to_le_bytes());
        inner = doc;
    }
    inner
}

#[test]
fn deeply_nested_documents_return() {
    // Unknown keys are skipped without descending into them.
    for key in [b'a', b'g', b'w'] {
        let payload = nested(key, 200);
        assert!(cuboid_fmt_cubik::decode::decode(&payload, &Limits::default()).is_ok());
    }
}
