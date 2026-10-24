#![forbid(unsafe_code)]
//! Layer L4. The wgpu renderer: targets, pipelines, uploads, transparency runs, voxel pools,
//! overlays and PNG render.
//!
//! Owns the GPU resources and passes of app 8 and 15. Must not read the document directly; it
//! consumes scene extracts.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod atlas;
pub mod backgrounds;
pub mod composite;
pub mod contract;
pub mod elements;
pub mod gpu;
pub mod grid;
pub mod look;
pub mod meshes;
pub mod offscreen;
pub mod outline;
pub mod overlays;
pub mod pipelines;
pub mod preview;
pub mod readback;
pub mod shadows;
pub mod sort;
pub mod targets;
pub mod util;
pub mod voxels;
pub mod wireframe;

pub use atlas::{DEFAULT_ATLAS_SIZE, MAX_ATLAS_PAGES};
pub use contract::{
    AdapterCaps, Health, PngJob, ReadbackId, RenderError, RenderSettings, UploadBudget, ViewInput,
    ViewParams,
};
pub use cuboid_scene::FrameKey;
pub use gpu::Renderer;
pub use targets::ViewTexture;

#[cfg(test)]
mod tests;
