#![forbid(unsafe_code)]
//! Layer L3. Format registry, pipeline texts, session traits, profiles, text dialects and
//! emitters, `.lnk` parsing and atomic writes.
//!
//! Owns the shared machinery of interop 1-6, the text dialects (native 9.4; interop 3, 8.6,
//! 10.1) and `.lnk` (interop 28). Must not write files itself; the I/O thread writes artifacts.

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

mod atomic;
pub mod contract;
pub mod lnk;
pub mod names;
pub mod pipeline;
mod registry;
#[cfg(test)]
mod testgen;
pub mod text;
pub mod texture_files;

pub use atomic::write_atomic;
pub use contract::*;
pub use registry::Registry;
