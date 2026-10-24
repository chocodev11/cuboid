//! Public contract stubs of this crate: cross-crate types and traits with their spec references
//! (W0.2).
//!
//! This file holds the plain-data vocabulary of the voxel crate. The types with private state
//! live in their modules ([`crate::world`], [`crate::chunk`], [`crate::palette`]) next to the
//! method stubs W1.3 and W1.4 replace. Panics are denied in this crate, so a stub never panics:
//! it logs an error and returns the neutral value.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::{IVec3, Vec3};

use crate::chunk::Chunk;

/// A revision stamp drawn from a process-wide counter, so two stamps are never equal unless one
/// was copied from the other. Chunk jobs carry the stamp they started from and their results
/// are dropped when it is stale.
///
/// This is a twin of `cuboid_image::Rev`: both crates sit on layer L1 and share only
/// `cuboid-num`, which W0.3 owns. Moving the type into `cuboid-num` replaces both copies.
///
/// spec: arch 6 (`Rev`), arch 13.5 (Jobs)
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Rev(u64);

impl Rev {
    /// The stamp of "never"; no live object has it.
    pub const NONE: Rev = Rev(0);

    /// Draws a fresh stamp.
    pub fn next() -> Rev {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Rev(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw counter value.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A voxel index `(i, j, k)`: `i` grows with +X, `j` with +Y and `k` toward internal -Z.
///
/// spec: tools 1.5, tools 7.2
pub type I3 = IVec3;

/// A box of voxel indices, half-open `[min, max)` on every axis. The world's active box.
///
/// spec: tools 7.2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct IBox3 {
    /// First index inside the box.
    pub min: I3,
    /// First index past the box.
    pub max: I3,
}

/// A box in internal world units. The model box, and the corners remembered at voxel setup.
///
/// spec: tools 2.4, tools 7.2
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WBox3 {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl WBox3 {
    /// The 48-unit editing volume, the default model box: `(-16, -16, -32)` to `(32, 32, 16)`.
    ///
    /// spec: tools 1.1, tools 2.4
    pub const EDITING_VOLUME: WBox3 = WBox3 {
        min: Vec3::new(-16.0, -16.0, -32.0),
        max: Vec3::new(32.0, 32.0, 16.0),
    };
}

/// A ray in internal world space.
///
/// spec: arch 13.7, tools 8.4
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Ray {
    /// Start point.
    pub origin: Vec3,
    /// Unit direction.
    pub dir: Vec3,
}

/// The first exposed voxel face a ray enters.
///
/// spec: arch 13.7 (Voxels), tools 12.2
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct VoxelHit {
    /// Where the ray enters the face.
    pub point: Vec3,
    /// Outward unit normal of the face, an axis direction in internal space.
    pub normal: Vec3,
    /// Distance from the ray origin along the ray.
    pub distance: f32,
}

/// Which write primitive a batch uses.
///
/// spec: tools 7.5
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WriteKind {
    /// Box check, palette update and neighbour dirtying.
    Full,
    /// Box check and palette update of non-zero values; no neighbour dirtying.
    Bounded,
    /// Grid bounds only (no box check); palette update of non-zero values.
    Unchecked,
}

/// The six face directions of a voxel in the order of the mesher table and of the GPU pools.
/// Names are internal-space normals; `PosZ` faces toward internal +Z, which is the cell at
/// index `k - 1`.
///
/// spec: tools 7.6, arch 13.5
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Dir {
    /// -Y (down).
    NegY,
    /// +Y (up).
    PosY,
    /// +Z internal.
    PosZ,
    /// -Z internal.
    NegZ,
    /// -X.
    NegX,
    /// +X.
    PosX,
}

impl Dir {
    /// All directions in pool order.
    pub const ALL: [Dir; 6] = [
        Dir::NegY,
        Dir::PosY,
        Dir::PosZ,
        Dir::NegZ,
        Dir::NegX,
        Dir::PosX,
    ];
}

/// One exposed voxel face, 8 bytes; the direction is implied by the pool it sits in.
///
/// Word 0 holds the local x, y, z (4 bits each) and the chunk x, y, z (6 bits each). Word 1 holds
/// the RGB colour (24 bits) and the ambient-occlusion code of the four vertices (2 bits each).
/// The bit positions inside the words are fixed by W1.4 together with the shader of W3.5.
///
/// spec: arch 13.5 (Face records), tools 7.6
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct VoxelFace(pub [u32; 2]);

/// A vertex of a CPU-expanded face quad, for exports.
///
/// spec: arch 13.5, tools 7.6
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ExportVertex {
    /// Position in internal world units.
    pub pos: Vec3,
    /// Flat face normal.
    pub normal: Vec3,
    /// Voxel colour times the ambient-occlusion factor of the vertex, opaque.
    pub color: [f32; 4],
}

/// One slot of the chunk table, indexed `(cx * n + cy) * n + cz` as the file persists chunks.
///
/// spec: arch 8.4, native 4.7, tools 7.10
#[derive(Clone, Debug, Default)]
pub enum ChunkSlot {
    /// Never written; reads give 0 without allocating.
    #[default]
    Unallocated,
    /// Written once and erased again; the file still lists the chunk. Neither a load nor an
    /// undo restore keeps this state.
    EmptyWritten,
    /// Cells present. Shared between world clones and snapshots; a write copies on demand.
    Data(Arc<Chunk>),
}
