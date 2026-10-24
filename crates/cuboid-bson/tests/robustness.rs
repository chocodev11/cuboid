//! Malformed input is an error, never a panic (architecture 16, native 8.1 item 4, 10.4).
//!
//! The reader walks every document it can reach, calls every getter on every element and dumps
//! the document; damaged documents, random bytes and random limits must only produce errors.

mod support;

use cuboid_bson::{Limits, RawDoc};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::Index;
use support::strategies::wide_doc;
use support::walk::{read_everything, walk, wrap_body};

fn limits() -> impl Strategy<Value = Limits> {
    prop_oneof![
        Just(Limits::DEFAULT),
        (1..600_u32, 0..6_u8, 0..8_u32).prop_map(|(max_doc, max_depth, max_array)| Limits {
            max_doc,
            max_depth,
            max_array,
        }),
    ]
}

#[derive(Debug, Clone)]
enum Edit {
    Xor(Index, u8),
    Size(Index, i32),
    Cut(Index),
    Insert(Index, Vec<u8>),
    Remove(Index, usize),
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (any::<Index>(), 1..=u8::MAX).prop_map(|(at, bits)| Edit::Xor(at, bits)),
        (any::<Index>(), prop_oneof![-8..64_i32, any::<i32>()])
            .prop_map(|(at, size)| Edit::Size(at, size)),
        any::<Index>().prop_map(Edit::Cut),
        (any::<Index>(), vec(any::<u8>(), 1..8)).prop_map(|(at, data)| Edit::Insert(at, data)),
        (any::<Index>(), 1..8_usize).prop_map(|(at, len)| Edit::Remove(at, len)),
    ]
}

fn apply(bytes: &mut Vec<u8>, edit: Edit) {
    if bytes.is_empty() {
        return;
    }
    match edit {
        Edit::Xor(at, bits) => {
            let at = at.index(bytes.len());
            bytes[at] ^= bits;
        }
        Edit::Size(at, size) => {
            let at = at.index(bytes.len());
            let end = (at + 4).min(bytes.len());
            let size = size.to_le_bytes();
            bytes[at..end].copy_from_slice(&size[..end - at]);
        }
        Edit::Cut(at) => bytes.truncate(at.index(bytes.len())),
        Edit::Insert(at, data) => {
            let at = at.index(bytes.len());
            bytes.splice(at..at, data);
        }
        Edit::Remove(at, len) => {
            let at = at.index(bytes.len());
            bytes.drain(at..(at + len).min(bytes.len()));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 10_000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in vec(any::<u8>(), 0..512), limits in limits()) {
        read_everything(&bytes, &limits);
    }

    #[test]
    fn arbitrary_elements_in_a_valid_document_never_panic(
        body in vec(any::<u8>(), 0..256),
        limits in limits(),
    ) {
        read_everything(&wrap_body(&body), &limits);
    }

    #[test]
    fn damaged_documents_never_panic(
        doc in wide_doc(),
        edits in vec(edit(), 1..4),
        limits in limits(),
    ) {
        let mut bytes = doc.to_vec().unwrap();
        for edit in edits {
            apply(&mut bytes, edit);
        }
        read_everything(&bytes, &limits);
    }

    #[test]
    fn every_proper_prefix_of_a_document_is_rejected(doc in wide_doc()) {
        let bytes = doc.to_vec().unwrap();
        for len in 0..bytes.len() {
            prop_assert!(RawDoc::parse(&bytes[..len], &Limits::DEFAULT).is_err(), "prefix of {len} bytes");
        }
    }

    #[test]
    fn an_intact_document_walks_without_error(doc in wide_doc()) {
        let bytes = doc.to_vec().unwrap();
        let parsed = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
        prop_assert_eq!(walk(parsed, &bytes), Ok(()));
    }
}
