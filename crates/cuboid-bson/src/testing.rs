//! Helpers of the unit tests.

use crate::{DocWriter, Limits, RawDoc, RawElem};

/// Bytes from hexadecimal text; white space separates the bytes.
pub fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
        .collect()
}

/// A document around `body`, the encoded elements.
pub fn wrap(body: &[u8]) -> Vec<u8> {
    let size = i32::try_from(body.len() + 5).expect("small document");
    let mut doc = size.to_le_bytes().to_vec();
    doc.extend_from_slice(body);
    doc.push(0);
    doc
}

/// The payload of a root document with the elements that `build` writes.
pub fn root(build: impl FnOnce(&mut DocWriter)) -> Vec<u8> {
    let mut w = DocWriter::new();
    let root = w.begin_root();
    build(&mut w);
    w.end(root);
    w.finish().expect("small document")
}

/// Hands the first element of the document that `build` writes to `read`.
pub fn first<T>(build: impl FnOnce(&mut DocWriter), read: impl FnOnce(RawElem<'_>) -> T) -> T {
    let bytes = root(build);
    let doc = RawDoc::parse(&bytes, &Limits::DEFAULT).expect("well-formed");
    read(
        doc.iter()
            .next()
            .expect("one element")
            .expect("well-formed"),
    )
}
