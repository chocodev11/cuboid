#![forbid(unsafe_code)]
//! Layer L3. Every editing operation and tool algorithm as pure functions over an edit context,
//! primitive generators and reference twins.
//!
//! Owns the operations of tools 4-14. Must not push undo, show messages or know cameras.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod animation;
pub mod convert;
pub mod elements;
pub mod groups;
pub mod mesh;
pub mod paint;
pub mod primitives;
pub mod textures;
pub mod transform;
pub mod uv;
pub mod voxel;

/// Why an operation refused. The document is untouched in every case.
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    /// A texture label has no usable characters (only `A-Z a-z 0-9 - _` are kept).
    #[error("`{0}` has no usable characters for a texture label")]
    BadLabel(String),
    /// The label is already in the texture list.
    #[error("the texture `{0}` already exists")]
    TextureExists(String),
    /// The label is not in the texture list.
    #[error("there is no texture `{0}`")]
    NoTexture(String),
    /// The requested texture size is zero or over the image limits.
    #[error("a texture of {w} x {h} texels is not allowed")]
    TextureSize {
        /// Requested width.
        w: u32,
        /// Requested height.
        h: u32,
    },
    /// A paste or duplicate could not be applied.
    #[error(transparent)]
    Apply(#[from] cuboid_model::ApplyError),
    /// A change of the group tree was refused.
    #[error(transparent)]
    Group(#[from] cuboid_model::GroupError),
}
