#![no_main]

use cuboid_fmt_cubik::{Limits, decode, dump, encode, load, load_bytes};
use libfuzzer_sys::fuzz_target;

const TIGHT: Limits = Limits {
    max_payload: 1 << 20,
    max_text: 1 << 20,
    max_depth: 8,
    max_array: 256,
    max_objects: 64,
    max_mesh_items: 256,
    max_textures: 8,
    max_texels: 1 << 16,
    voxel_budget: 1 << 22,
    retain_cap: 1 << 20,
};

/// A document around `body`, the encoded elements.
fn wrap_body(body: &[u8]) -> Vec<u8> {
    let size = i32::try_from(body.len() + 5).unwrap_or(i32::MAX);
    let mut doc = size.to_le_bytes().to_vec();
    doc.extend_from_slice(body);
    doc.push(0);
    doc
}

// The first byte picks the input: bit 0 reads the rest as a whole file, bit 1 puts a valid
// document header around it so that the fuzzer reaches the schema without first having to guess
// a size field. Bit 2 picks tight limits. A payload that decodes is encoded again, and every payload
// is dumped and loaded.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let limits = if mode & 4 == 0 { Limits::default() } else { TIGHT };
    if mode & 1 == 0 {
        let _ = load_bytes(rest, &limits);
        return;
    }
    let payload = if mode & 2 == 0 {
        rest.to_vec()
    } else {
        wrap_body(rest)
    };
    let _ = dump::payload(&payload);
    if let Ok(decoded) = decode::decode(&payload, &limits) {
        let _ = encode::encode(&decoded.root);
    }
    let _ = load::load_payload(payload, &limits);
});
