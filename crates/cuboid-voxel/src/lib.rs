#![forbid(unsafe_code)]
//! Layer L1. Voxel storage and algorithms: `VoxelWorld`, chunk table, palette, writers,
//! mappings, transforms, searches, face mesher and DDA.
//!
//! Owns the storage and algorithms of tools 7 and the canonical face mesher (tools 7.6). Must
//! not allocate chunks on reads.

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

pub mod chunk;
pub mod contract;
pub mod dda;
pub mod mapping;
pub mod mesh;
pub mod ops;
pub mod palette;
pub mod search;
pub mod world;
pub mod writers;

pub use chunk::{CHUNK_CELLS, CHUNK_EDGE, Chunk};
pub use contract::*;
pub use dda::dda;
pub use mesh::{expand_face, mesh_chunk};
pub use palette::Palette;
pub use world::{VoxelSnap, VoxelWorld};
