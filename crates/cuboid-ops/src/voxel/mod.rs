//! Voxel tool algorithms and the voxel commands: setup, writing cells with mirror and brush,
//! flood fill, global colour replacement and palette (spec: tools 7.7, 12.2-12.3, 13.3).
//!
//! The functions work on an edit context and push no undo; the stroke state (dedupe, plane pin,
//! preview) belongs to the tools of `cuboid-core`.

mod edit;
mod shape;

pub use edit::{
    Brush, Put, fill_region, palette_add, replace_color, rollback, setup, world, world_mut,
    write_cells,
};
pub use shape::{FaceHit, box_cells, brush_span, cell_at, hit_cells, line_cells, mirror_images};

/// The most cells one write of a stroke may touch before the brush and the mirror multiply them:
/// a box or a line preview that is larger is refused for that frame.
pub const MAX_STROKE_CELLS: usize = 1 << 19;

/// The largest edge of a cube brush in voxels.
pub const MAX_BRUSH: i32 = 16;

/// The default most cells a flood fill may change; a larger region leaves the world untouched.
pub const FILL_LIMIT: usize = 1 << 18;

/// The smallest and the largest resolution the setup accepts, voxels per block edge.
///
/// spec: tools 7.7
pub const SETUP_RES: std::ops::RangeInclusive<u32> = 2..=128;

/// Why a voxel operation refused. The document is untouched in every case.
#[derive(Debug, thiserror::Error)]
pub enum VoxelError {
    /// The voxel world does not exist yet.
    #[error("set up the voxels first")]
    NoWorld,
    /// The world exists; its resolution is fixed.
    #[error("the voxel world is already set up")]
    AlreadySetUp,
    /// The setup resolution is outside 2 to 128.
    #[error("a resolution of {0} voxels per block is not allowed, use 2 to 128")]
    Resolution(u32),
    /// The world would need more memory than the budget allows.
    #[error("a voxel world of resolution {res} needs {bytes} bytes, over the budget of {budget}")]
    Budget {
        /// The requested resolution.
        res: u32,
        /// The memory a dense world of it needs.
        bytes: u64,
        /// The budget.
        budget: u64,
    },
    /// A fill region or a shape is larger than the limit.
    #[error("more than {0} voxels would change, nothing was changed")]
    TooLarge(usize),
    /// The colour has no opacity, so it cannot name a palette entry.
    #[error("the colour must not be transparent")]
    Transparent,
}

#[cfg(test)]
mod tests;
