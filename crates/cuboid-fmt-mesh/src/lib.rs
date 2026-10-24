#![forbid(unsafe_code)]
//! Layer L4. Mesh and image formats: OBJ/MTL in and out, STL, PNG sprite import, the PNG render
//! export driver and glTF (M8).
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

pub mod gltf;
pub mod obj_export;
pub mod obj_import;
pub mod png_render;
pub mod png_sprite;
pub mod stl;
