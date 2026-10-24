#![forbid(unsafe_code)]
//! Layer L0. BSON subset: zero-copy reader, size-patching writer, lenient getters, spans and
//! canonical dump.
//!
//! Owns the BSON wire format of native 3.1-3.6, element spans and limits. Must not know about
//! Cubik records.
//!
//! [`RawDoc`] reads a payload without copying it: [`RawDoc::iter`] yields every element in
//! document order with its key, type and byte [`Span`]s, and the getters of [`RawElem`] convert
//! a value the way Cubik's reader does. [`DocWriter`] writes elements in call order and patches
//! the size of each document when it closes. [`dump()`] prints a document as the canonical text
//! of the golden files.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod convert;
mod dump;
mod error;
mod reader;
mod writer;

#[cfg(test)]
mod testing;

pub use dump::dump;
pub use error::BsonError;
pub use reader::{ElemType, Limits, RawDoc, RawElem, RawIter, Span, Value};
pub use writer::{DocWriter, Key, Mark};
