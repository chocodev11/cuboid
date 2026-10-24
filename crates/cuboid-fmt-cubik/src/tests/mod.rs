//! Unit tests of the codec, the container, the mapping and `.cub`.

#![allow(clippy::float_cmp, clippy::excessive_precision)]

mod codec;
mod container;
mod cub;
mod mapping;

use cuboid_bson::DocWriter;

/// Bytes from hexadecimal text; white space separates the bytes.
pub(crate) fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
        .collect()
}

/// A payload around the elements that `build` writes.
pub(crate) fn root(build: impl FnOnce(&mut DocWriter)) -> Vec<u8> {
    let mut w = DocWriter::new();
    let top = w.begin_root();
    build(&mut w);
    w.end(top);
    w.finish().expect("small document")
}

/// The bytes of the one document that `build` writes under a one-letter key, without the
/// element header: the notation of the worked examples in native 3.5.
pub(crate) fn embedded(build: impl FnOnce(&mut DocWriter)) -> Vec<u8> {
    let bytes = root(build);
    // Root size field, type byte, one-letter key and its terminator; the root terminator.
    bytes[7..bytes.len() - 1].to_vec()
}
