#![forbid(unsafe_code)]
//! Layer L1. `Image` (bottom row first), stored pixel formats, BC1/BC3, file codecs, the texture
//! signature, raster primitives, packers and alpha statistics.
//!
//! Owns `Image`, the stored formats of native 4.5.1, codecs, the signature (native 4.5), raster
//! primitives (tools 13.2), packers and alpha statistics. Must not know elements or faces.
//!
//! Rows of every pixel buffer run bottom to top, the original's image orientation; the flip to
//! top-first happens only at the PNG edge ([`decode_file`], [`encode_png`]).

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

mod alpha;
mod bc;
pub mod contract;
mod file;
pub mod format;
pub mod pack;
pub mod raster;
mod signature;

pub use alpha::AlphaStats;
pub use contract::*;
pub use file::{decode_file, encode_png};
pub use signature::signature;
