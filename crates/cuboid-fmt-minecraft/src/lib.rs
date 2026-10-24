#![forbid(unsafe_code)]
//! Layer L4. Minecraft formats: Java JSON in and out, resource pack, display presets, `.mcpe`,
//! `.js`, `.java`, `.jpm` and the entity atlas.
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

pub mod bedrock_js;
pub mod entity_atlas;
pub mod geo;
pub mod java;
pub mod jem;
pub mod jpm;
pub mod json_export;
pub mod json_import;
pub mod mcpe;
pub mod modern;
pub mod presets;
pub mod resource_pack;
