#![forbid(unsafe_code)]
//! Layer L4. Voxel formats: VOX, QB, QEF, binvox, schematic, NBT, ZOX, CSV, Stonehearth,
//! Staxel and block tables.
//!
//! One module per format with its fuzz target and goldens. Must not share mutable state, call
//! UI or touch the GPU.

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

pub mod binvox;
pub mod csv;
pub mod modern;
pub mod nbt;
pub mod qb;
pub mod qef;
pub mod schematic;
pub mod staxel;
pub mod stonehearth;
pub mod tables;
pub mod vox;
pub mod zox;
