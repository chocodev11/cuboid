//! Canonical text dump of a document.
//!
//! The lines have the format of `cuboid_testkit::dump::Dump`, which the golden files use: one
//! line `path: type = value` per element in document order, a header line `path: doc len=3` or
//! `path: array len=0` before the entries of a container, doubles as hex bit patterns, binary
//! values as length and SHA-256. A BSON double is `f64`, an int32 `i32` and an int64 `i64`.
//! The root document has no header line. Array entries take their index, not their key.
//!
//! Two additions cover what Cubik never writes: a binary value of a subtype other than `0x00`
//! ends with ` subtype=0x..`, and a value of a type outside the subset prints as
//! `raw = type=0x.. len=.. sha256=..`.
//!
//! spec: native 10.2

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::{BsonError, RawDoc, RawElem, Value};

/// Dumps the whole document, or returns the first error found on the way.
pub fn dump(doc: &RawDoc<'_>) -> Result<String, BsonError> {
    let mut out = String::new();
    entries(&mut out, &mut String::new(), doc, false)?;
    Ok(out)
}

/// The entries of `doc` below `path`.
fn entries(
    out: &mut String,
    path: &mut String,
    doc: &RawDoc<'_>,
    array: bool,
) -> Result<(), BsonError> {
    for (index, elem) in doc.iter().enumerate() {
        let elem = elem?;
        let mark = path.len();
        if array {
            let _ = write!(path, "[{index}]");
        } else {
            path.push('.');
            let key = String::from_utf8_lossy(elem.key);
            escape(path, &key, &['.', '[', ']', ' ']);
        }
        entry(out, path, &elem)?;
        path.truncate(mark);
    }
    Ok(())
}

fn entry(out: &mut String, path: &mut String, elem: &RawElem<'_>) -> Result<(), BsonError> {
    let name = shown(path).to_owned();
    match elem.value()? {
        Value::Doc(doc) => return container(out, path, &name, "doc", &doc, false),
        Value::Array(doc) => return container(out, path, &name, "array", &doc, true),
        Value::Double(v) => {
            let _ = write!(out, "{name}: f64 = 0x{:016x} ({v:?})", v.to_bits());
        }
        Value::String(v) => {
            let _ = write!(out, "{name}: str = \"");
            escape(out, v, &['"']);
            out.push('"');
        }
        Value::Binary { subtype, bytes } => {
            let (len, hash) = (bytes.len(), sha256(bytes));
            let _ = write!(out, "{name}: bytes = len={len} sha256={hash}");
            if subtype != 0 {
                let _ = write!(out, " subtype=0x{subtype:02x}");
            }
        }
        Value::Bool(v) => {
            let _ = write!(out, "{name}: bool = {v}");
        }
        Value::Null => {
            let _ = write!(out, "{name}: null");
        }
        Value::Int32(v) => {
            let _ = write!(out, "{name}: i32 = {v}");
        }
        Value::Int64(v) => {
            let _ = write!(out, "{name}: i64 = {v}");
        }
        Value::Other(ty) => {
            let bytes = elem.value_bytes();
            let (len, hash) = (bytes.len(), sha256(bytes));
            let _ = write!(
                out,
                "{name}: raw = type=0x{:02x} len={len} sha256={hash}",
                ty as u8
            );
        }
    }
    out.push('\n');
    Ok(())
}

fn container(
    out: &mut String,
    path: &mut String,
    name: &str,
    kind: &str,
    doc: &RawDoc<'_>,
    array: bool,
) -> Result<(), BsonError> {
    let len = doc.iter().try_fold(0usize, |n, elem| elem.map(|_| n + 1))?;
    let _ = writeln!(out, "{name}: {kind} len={len}");
    entries(out, path, doc, array)
}

/// A path as printed: without the dot of its first key. The dot of an empty first key stays, so
/// that it is not mistaken for the root or for an index.
fn shown(path: &str) -> &str {
    match path.strip_prefix('.') {
        Some(key) if !key.is_empty() && !key.starts_with(['.', '[']) => key,
        _ => path,
    }
}

/// Appends `text` with a backslash before `\` and the characters of `special`, and control
/// characters as `\n`, `\r`, `\t` or `\u{..}`.
fn escape(out: &mut String, text: &str, special: &[char]) {
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c if c == '\\' || special.contains(&c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
}

fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use cuboid_testkit::dump::{Dump, sha256_hex};

    use super::*;
    use crate::testing::{hex, root, wrap};
    use crate::{DocWriter, Limits};

    fn dumped(bytes: &[u8]) -> Result<String, BsonError> {
        dump(&RawDoc::parse(bytes, &Limits::DEFAULT)?)
    }

    #[test]
    fn the_lines_equal_those_of_the_test_kit() {
        let bytes = root(|w| {
            let mp = w.begin_doc(b"mp\0");
            let g = w.begin_doc(b"g\0");
            w.str(b"n\0", "Untitled \"quoted\"\n");
            w.i32(b"o\0", -3);
            w.end(g);
            w.end(mp);
            w.f32(b"zoom\0", 1.5);
            w.f64(b"neg zero\0", -0.0);
            w.f64(b"nan\0", f64::from_bits(0x7ff8_0000_0000_0001));
            let el = w.begin_array(b"el\0");
            let item = w.begin_doc(0usize);
            w.bool(b"hidden\0", false);
            let f = w.begin_array(b"f\0");
            let face = w.begin_doc(0usize);
            w.null(b"t\0");
            w.end(face);
            w.end(f);
            w.end(item);
            w.end(el);
            let tx = w.begin_array(b"tx\0");
            let texture = w.begin_doc(0usize);
            w.bin(b"r\0", b"abc");
            w.end(texture);
            w.end(tx);
            let m = w.begin_array(b"m\0");
            let inner = w.begin_array(0usize);
            w.i64(0usize, i64::MIN);
            w.end(inner);
            w.end(m);
            w.i32(b"x: str = \"y\"\0", 1);
            w.i32(b"a.b\0", 2);
            let a = w.begin_doc(b"a\0");
            w.i32(b"b\0", 3);
            w.end(a);
            w.i32(b"[3]\0", 4);
            w.i32(b"two\nlines\\\0", 5);
            let empty = w.begin_doc(b"\0");
            w.i32(b"x\0", 7);
            let v = w.begin_array(b"v\0");
            w.i32(0usize, 8);
            w.end(v);
            w.end(empty);
            let empty_array = w.begin_array(b"empty\0");
            w.end(empty_array);
        });

        let mut d = Dump::new();
        d.doc("mp", |d| {
            d.doc("g", |d| {
                d.str("n", "Untitled \"quoted\"\n");
                d.i32("o", -3);
            });
        });
        d.f64("zoom", 1.5);
        d.f64("neg zero", -0.0);
        d.f64("nan", f64::from_bits(0x7ff8_0000_0000_0001));
        d.array("el", |d| {
            d.doc(0, |d| {
                d.bool("hidden", false);
                d.array("f", |d| d.doc(0, |d| d.null("t")));
            });
        });
        d.array("tx", |d| d.doc(0, |d| d.bytes("r", b"abc")));
        d.array("m", |d| d.array(0, |d| d.i64(0, i64::MIN)));
        d.i32("x: str = \"y\"", 1);
        d.i32("a.b", 2);
        d.doc("a", |d| d.i32("b", 3));
        d.i32("[3]", 4);
        d.i32("two\nlines\\", 5);
        d.doc("", |d| {
            d.i32("x", 7);
            d.array("v", |d| d.i32(0, 8));
        });
        d.array("empty", |_| {});

        assert_eq!(dumped(&bytes).unwrap(), d.finish());
    }

    #[test]
    fn the_root_has_no_header_line_and_an_empty_document_prints_nothing() {
        assert_eq!(dumped(&hex("05 00 00 00 00")).unwrap(), "");
    }

    #[test]
    fn doubles_print_their_bit_pattern() {
        let bytes = root(|w| w.f32(b"x\0", 0.1));
        assert_eq!(
            dumped(&bytes).unwrap(),
            "x: f64 = 0x3fb99999a0000000 (0.10000000149011612)\n"
        );
    }

    #[test]
    fn binary_subtypes_and_unknown_types_print_what_the_test_kit_cannot() {
        let mut body = hex("05 62 00 02 00 00 00 05 01 02");
        body.extend(hex("07 6F 00 00 01 02 03 04 05 06 07 08 09 0A 0B"));
        let sum = |data: &[u8]| sha256_hex(data);
        let expected = format!(
            "b: bytes = len=2 sha256={} subtype=0x05\no: raw = type=0x07 len=12 sha256={}\n",
            sum(&[1, 2]),
            sum(&hex("00 01 02 03 04 05 06 07 08 09 0A 0B")),
        );
        assert_eq!(dumped(&wrap(&body)).unwrap(), expected);
    }

    #[test]
    fn a_key_that_is_not_utf8_prints_with_the_replacement_character() {
        let bytes = wrap(&hex("10 FF 00 01 00 00 00"));
        assert_eq!(dumped(&bytes).unwrap(), "\u{fffd}: i32 = 1\n");
    }

    #[test]
    fn an_error_below_the_root_is_returned() {
        let bytes = wrap(&hex("03 61 00 07 00 00 00 20 00 00"));
        assert!(matches!(
            dumped(&bytes),
            Err(BsonError::UnknownType { ty: 0x20, .. })
        ));
        let mut deep = DocWriter::new();
        let root = deep.begin_root();
        deep.bool(b"b\0", true);
        deep.end(root);
        let mut bytes = deep.finish().unwrap();
        *bytes.get_mut(7).unwrap() = 2;
        assert!(matches!(
            dumped(&bytes),
            Err(BsonError::BadBool { byte: 2, .. })
        ));
    }
}
