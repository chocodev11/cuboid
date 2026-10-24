//! Random documents built with the `bson` crate, the independent implementation of the
//! differential tests.

use bson::oid::ObjectId;
use bson::raw::CString;
use bson::spec::BinarySubtype;
use bson::{
    Binary, Bson, DateTime, Decimal128, Document, JavaScriptCodeWithScope, Regex, Timestamp,
};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::strategy::Union;

/// A key: any characters but NUL, empty keys and keys with separators included.
fn key() -> impl Strategy<Value = String> {
    "[^\0]{0,6}"
}

/// A string value: any characters, NUL included.
fn text() -> impl Strategy<Value = String> {
    "(?s).{0,12}"
}

fn c_string() -> impl Strategy<Value = CString> {
    "[^\0]{0,5}".prop_map(|s| CString::try_from(s).expect("no NUL"))
}

fn bytes() -> impl Strategy<Value = Vec<u8>> {
    vec(any::<u8>(), 0..24)
}

/// What Cubik writes (native 3.1) and int64: every double bit pattern, binary of subtype 0 only.
fn subset_leaf() -> Vec<BoxedStrategy<Bson>> {
    vec![
        any::<u64>()
            .prop_map(|b| Bson::Double(f64::from_bits(b)))
            .boxed(),
        text().prop_map(Bson::String).boxed(),
        any::<bool>().prop_map(Bson::Boolean).boxed(),
        Just(Bson::Null).boxed(),
        any::<i32>().prop_map(Bson::Int32).boxed(),
        any::<i64>().prop_map(Bson::Int64).boxed(),
        bytes()
            .prop_map(|bytes| {
                Bson::Binary(Binary {
                    subtype: BinarySubtype::Generic,
                    bytes,
                })
            })
            .boxed(),
    ]
}

/// The leaves of the subset and the other types of the BSON specification (a DbPointer cannot
/// be built with the `bson` crate), binary values of every subtype but the old one.
fn wide_leaf() -> Vec<BoxedStrategy<Bson>> {
    let mut leaf = subset_leaf();
    leaf.extend([
        (
            any::<u8>().prop_filter("subtype 2 has an inner length", |&t| t != 2),
            bytes(),
        )
            .prop_map(|(subtype, bytes)| {
                Bson::Binary(Binary {
                    subtype: BinarySubtype::from(subtype),
                    bytes,
                })
            })
            .boxed(),
        any::<[u8; 12]>()
            .prop_map(|b| Bson::ObjectId(ObjectId::from_bytes(b)))
            .boxed(),
        any::<i64>()
            .prop_map(|ms| Bson::DateTime(DateTime::from_millis(ms)))
            .boxed(),
        (any::<u32>(), any::<u32>())
            .prop_map(|(time, increment)| Bson::Timestamp(Timestamp { time, increment }))
            .boxed(),
        (c_string(), c_string())
            .prop_map(|(pattern, options)| Bson::RegularExpression(Regex { pattern, options }))
            .boxed(),
        text().prop_map(Bson::JavaScriptCode).boxed(),
        text().prop_map(Bson::Symbol).boxed(),
        any::<[u8; 16]>()
            .prop_map(|b| Bson::Decimal128(Decimal128::from_bytes(b)))
            .boxed(),
        (text(), members(Union::new(subset_leaf())))
            .prop_map(|(code, scope)| {
                Bson::JavaScriptCodeWithScope(JavaScriptCodeWithScope { code, scope })
            })
            .boxed(),
        Just(Bson::Undefined).boxed(),
        Just(Bson::MaxKey).boxed(),
        Just(Bson::MinKey).boxed(),
    ]);
    leaf
}

/// The members of a document; a repeated key keeps the position of its first occurrence.
fn members(value: impl Strategy<Value = Bson>) -> impl Strategy<Value = Document> {
    vec((key(), value), 0..6).prop_map(|pairs| pairs.into_iter().collect())
}

fn doc(leaf: Vec<BoxedStrategy<Bson>>) -> impl Strategy<Value = Document> {
    let value = Union::new(leaf).prop_recursive(4, 64, 6, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..6).prop_map(Bson::Array),
            members(inner).prop_map(Bson::Document),
        ]
    });
    members(value)
}

/// Documents of the subset of native 3.1 plus int64.
pub fn subset_doc() -> impl Strategy<Value = Document> {
    doc(subset_leaf())
}

/// Documents with every type of the BSON specification the `bson` crate can build.
pub fn wide_doc() -> impl Strategy<Value = Document> {
    doc(wide_leaf())
}
