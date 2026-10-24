//! Random documents through `cuboid-bson` and the `bson` crate (3.1.0), in both directions
//! (architecture 12.3, 19.3): what the `bson` crate writes is read back by the reader, and what
//! the writer produces is byte for byte what the `bson` crate produces and is read back by it.

mod support;

use bson::{Bson, Document};
use cuboid_bson::{DocWriter, Key, Limits, RawDoc, RawElem, Value};
use cuboid_testkit::dump::{At, Dump};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use support::strategies::{subset_doc, wide_doc};

const CASES: u32 = 10_000;
const MANY_CASES: u32 = 100_000;

/// A key taken from a generated document.
struct Name<'a>(&'a str);

impl Key for Name<'_> {
    fn put(self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.0.as_bytes());
        out.push(0);
    }
}

fn run<S: Strategy>(cases: u32, strategy: S, check: impl Fn(S::Value)) {
    let config = Config {
        cases,
        failure_persistence: None,
        ..Config::default()
    };
    TestRunner::new(config)
        .run(&strategy, |value| {
            check(value);
            Ok(())
        })
        .unwrap();
}

// Direction 1: the reader against the `bson` encoder.

fn reads_what_bson_wrote(doc: &Document) {
    let bytes = doc.to_vec().unwrap();
    let parsed = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
    assert_eq!(parsed.as_bytes(), bytes);
    check_members(doc.iter().map(|(k, v)| (k.clone(), v)), parsed, &bytes);
}

/// The elements of `doc` are `expected`, in order, with spans that tile the document.
fn check_members<'a>(
    expected: impl Iterator<Item = (String, &'a Bson)>,
    doc: RawDoc<'_>,
    bytes: &[u8],
) {
    let mut expected = expected;
    let mut cursor = doc.span().start as usize + 4;
    for elem in doc.iter() {
        let elem = elem.unwrap();
        let (key, value) = expected.next().expect("more elements than expected");
        assert_eq!(elem.key, key.as_bytes());
        assert_eq!(elem.whole.start as usize, cursor);
        cursor = elem.whole.end as usize;
        assert_eq!(bytes[elem.whole.start as usize], value.element_type() as u8);
        assert_eq!(elem.value.end, elem.whole.end);
        assert_eq!(elem.value_bytes(), &bytes[elem.value.range()]);
        check_value(value, &elem, bytes);
    }
    assert!(expected.next().is_none(), "fewer elements than expected");
    assert_eq!(cursor, doc.span().end as usize - 1);
}

fn check_value(expected: &Bson, elem: &RawElem<'_>, bytes: &[u8]) {
    match (expected, elem.value().unwrap()) {
        (Bson::Double(want), Value::Double(got)) => {
            assert_eq!(got.to_bits(), want.to_bits());
            assert_eq!(elem.f32().unwrap().to_bits(), (*want as f32).to_bits());
        }
        (Bson::String(want), Value::String(got)) => {
            assert_eq!(got, want);
            assert_eq!(elem.str_opt().unwrap(), Some(want.as_str()));
        }
        (Bson::Document(want), Value::Doc(got)) => {
            check_members(want.iter().map(|(k, v)| (k.clone(), v)), got, bytes);
        }
        (Bson::Array(want), Value::Array(got)) => {
            check_members(
                want.iter().enumerate().map(|(i, v)| (i.to_string(), v)),
                got,
                bytes,
            );
        }
        (Bson::Boolean(want), Value::Bool(got)) => {
            assert_eq!(got, *want);
            assert_eq!(elem.bool().unwrap(), *want);
        }
        (Bson::Null, Value::Null) => assert_eq!(elem.str_opt().unwrap(), None),
        (Bson::Int32(want), Value::Int32(got)) => {
            assert_eq!(got, *want);
            assert_eq!(elem.i32().unwrap(), *want);
        }
        (Bson::Int64(want), Value::Int64(got)) => assert_eq!(got, *want),
        (Bson::Binary(want), Value::Binary { subtype, bytes }) => {
            assert_eq!(subtype, u8::from(want.subtype));
            assert_eq!(bytes, want.bytes);
        }
        (
            Bson::ObjectId(_)
            | Bson::DateTime(_)
            | Bson::Timestamp(_)
            | Bson::RegularExpression(_)
            | Bson::JavaScriptCode(_)
            | Bson::JavaScriptCodeWithScope(_)
            | Bson::Symbol(_)
            | Bson::Decimal128(_)
            | Bson::Undefined
            | Bson::MaxKey
            | Bson::MinKey,
            Value::Other(ty),
        ) => assert_eq!(ty as u8, expected.element_type() as u8),
        (want, got) => panic!("{want:?} was read as {got:?}"),
    }
}

// Direction 2: the writer against the `bson` encoder and decoder.

fn write_doc(w: &mut DocWriter, doc: &Document) {
    for (key, value) in doc {
        write_value(w, Name(key), value);
    }
}

fn write_value(w: &mut DocWriter, key: impl Key, value: &Bson) {
    match value {
        Bson::Double(v) => w.f64(key, *v),
        Bson::String(v) => w.str(key, v),
        Bson::Document(doc) => {
            let mark = w.begin_doc(key);
            write_doc(w, doc);
            w.end(mark);
        }
        Bson::Array(items) => {
            let mark = w.begin_array(key);
            for (i, item) in items.iter().enumerate() {
                write_value(w, i, item);
            }
            w.end(mark);
        }
        Bson::Boolean(v) => w.bool(key, *v),
        Bson::Null => w.null(key),
        Bson::Int32(v) => w.i32(key, *v),
        Bson::Int64(v) => w.i64(key, *v),
        Bson::Binary(v) => w.bin(key, &v.bytes),
        other => panic!("{other:?} is outside the subset"),
    }
}

fn writes_what_bson_writes(doc: &Document) {
    let expected = doc.to_vec().unwrap();
    let mut w = DocWriter::new();
    let root = w.begin_root();
    write_doc(&mut w, doc);
    w.end(root);
    let written = w.finish().unwrap();
    assert_eq!(written, expected);
    let decoded = Document::from_reader(&written[..]).unwrap();
    assert_eq!(decoded.to_vec().unwrap(), expected);
}

// The canonical dump against the test kit's.

fn dump_value(d: &mut Dump, at: impl At, value: &Bson) {
    match value {
        Bson::Double(v) => d.f64(at, *v),
        Bson::String(v) => d.str(at, v),
        Bson::Document(doc) => d.doc(at, |d| {
            for (key, value) in doc {
                dump_value(d, key.as_str(), value);
            }
        }),
        Bson::Array(items) => d.array(at, |d| {
            for (i, item) in items.iter().enumerate() {
                dump_value(d, i, item);
            }
        }),
        Bson::Boolean(v) => d.bool(at, *v),
        Bson::Null => d.null(at),
        Bson::Int32(v) => d.i32(at, *v),
        Bson::Int64(v) => d.i64(at, *v),
        Bson::Binary(v) => d.bytes(at, &v.bytes),
        other => panic!("{other:?} is outside the subset"),
    }
}

fn dumps_like_the_test_kit(doc: &Document) {
    let mut expected = Dump::new();
    for (key, value) in doc {
        dump_value(&mut expected, key.as_str(), value);
    }
    let bytes = doc.to_vec().unwrap();
    let parsed = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
    assert_eq!(cuboid_bson::dump(&parsed).unwrap(), expected.finish());
}

fn subset(cases: u32) {
    run(cases, subset_doc(), |doc| {
        reads_what_bson_wrote(&doc);
        writes_what_bson_writes(&doc);
        dumps_like_the_test_kit(&doc);
    });
}

fn wide(cases: u32) {
    run(cases, wide_doc(), |doc| reads_what_bson_wrote(&doc));
}

#[test]
fn subset_documents_agree_with_the_bson_crate_in_both_directions() {
    subset(CASES);
}

#[test]
fn documents_with_every_type_are_read_like_the_bson_crate_reads_them() {
    wide(CASES);
}

#[test]
#[ignore = "10^5 cases: cargo test -p cuboid-bson --release -- --ignored"]
fn subset_documents_agree_with_the_bson_crate_in_both_directions_100k() {
    subset(MANY_CASES);
}

#[test]
#[ignore = "10^5 cases: cargo test -p cuboid-bson --release -- --ignored"]
fn documents_with_every_type_are_read_like_the_bson_crate_reads_them_100k() {
    wide(MANY_CASES);
}

#[test]
fn old_binary_values_read_like_the_bson_crate_reads_them() {
    // `Bson::Binary` leaves the inner length of subtype 2 to the caller; the raw reader of the
    // `bson` crate strips it, like the lenient getter.
    let element = [0x05, b'b', 0, 7, 0, 0, 0, 2, 3, 0, 0, 0, 0xAA, 0xBB, 0xCC];
    let mut bytes = (element.len() as i32 + 5).to_le_bytes().to_vec();
    bytes.extend(element);
    bytes.push(0);

    let theirs = bson::RawDocument::from_bytes(&bytes).unwrap();
    let bson::RawBsonRef::Binary(binary) = theirs.get("b").unwrap().unwrap() else {
        panic!("not binary");
    };
    assert_eq!(u8::from(binary.subtype), 2);
    assert_eq!(binary.bytes, [0xAA, 0xBB, 0xCC]);

    let ours = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
    let elem = ours.last(b"b").unwrap().unwrap();
    assert_eq!(elem.bytes_opt().unwrap().unwrap().as_ref(), binary.bytes);
}
