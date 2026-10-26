#![forbid(unsafe_code)]
//! Layer L4. Mesh and image formats: OBJ/MTL in and out, STL, PNG sprite import, the PNG render
//! export driver and glTF (M8).
//!
//! One module per format with its fuzz target and goldens. Must not share mutable state, call
//! UI or touch the GPU.
//!
//! # Export frame
//!
//! The OBJ and the glTF exporter read the same [`gather`]ed scene, so both files hold the same
//! geometry. The model's internal frame (X right, Y up, Z toward the viewer's back, a
//! left-handed frame in which a block spans `x` 0..16, `y` 0..16, `z` -16..0) becomes the
//! right-handed, Y-up frame of the two formats by `(x, y, z) -> (-(x - ox), y - oy, z - oz) * s`:
//!
//! - `o` is the model origin (`g.o`, the export pivot), subtracted first, as the reference
//!   offset `-origin` does (interop 16.1);
//! - X is negated (interop 16.3), so the north face of the model, which faces internal +Z,
//!   faces +Z in the file, the way glTF viewers show an asset's front;
//! - `s` is `1/16` for [`UnitScale::Block`] (one unit is one block, one metre in glTF) and 1 for
//!   [`UnitScale::Pixel`] (sixteen units per block, the model's own units).
//!
//! The reflection turns the winding around, so each triangle of the renderer is written in
//! reverse order and still faces outward. A box with inverted normals (negative size) keeps the
//! corner tables the renderer uses for it, so it is drawn inside out in every tool.
//!
//! Texture coordinates are the model's (origin at the bottom left, V up) in OBJ and flipped
//! (`v -> 1 - v`) in glTF, which measures V from the top.

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

mod files;
pub mod gather;
pub mod gltf;
mod names;
pub mod obj_export;
pub mod obj_import;
pub mod png_render;
pub mod png_sprite;
pub mod stl;

use std::fmt;

use thiserror::Error;

pub use files::{ExportFile, MeshExport, export_for_path};
pub use gather::{Limits, MeshOptions, UnitScale};
pub use gltf::{GltfFormat, export_gltf};
pub use obj_export::export_obj;

/// Why a model is not exported.
#[derive(Debug, Error)]
pub enum MeshError {
    /// Nothing to write: no visible element, mesh or voxel.
    #[error("the model has nothing to export")]
    Empty,
    /// The model is larger than a limit allows, named here.
    #[error("the model exceeds the limit on {0}")]
    Limit(&'static str),
    /// A texture cannot be encoded as PNG.
    #[error("cannot encode the texture `{name}`: {reason}")]
    Texture {
        /// The name of the texture.
        name: String,
        /// Why.
        reason: String,
    },
    /// The target has an extension that no mesh exporter writes.
    #[error("no mesh format is written for the extension `{0}`")]
    Extension(String),
    /// The output is too large for the container of the format.
    #[error("the output is too large for the file format")]
    TooLarge,
}

/// Something the exporter left out or changed, for the caller to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Objects that were not exported because their numbers are not finite or they have no
    /// geometry.
    Skipped {
        /// The kind of object.
        what: &'static str,
        /// How many.
        count: usize,
    },
    /// A face names a texture that the model does not have; the face is drawn with the colour
    /// of its element only.
    TextureMissing {
        /// The label the face names.
        label: String,
    },
    /// Textures with an animation were written as their first frame, because the format has no
    /// animation.
    AnimationStill {
        /// How many textures.
        count: usize,
    },
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::Skipped { what, count } => write!(f, "skipped {count} {what}"),
            Notice::TextureMissing { label } => {
                write!(f, "the texture `{label}` is not in the model")
            }
            Notice::AnimationStill { count } => {
                write!(
                    f,
                    "{count} animated textures were written as their first frame"
                )
            }
        }
    }
}
