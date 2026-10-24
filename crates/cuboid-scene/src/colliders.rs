//! Voxel collider snapshot, frozen while a stroke runs (spec: tools 7.6, 12.2; arch 10.5).

use std::sync::Arc;

use cuboid_voxel::VoxelWorld;

/// The voxel world that ray hits use. It is refreshed when a rebuild pass completes, frozen when
/// a stroke begins and refreshed when the stroke ends, so a stroke lands where the world looked
/// when it began.
///
/// spec: tools 7.6, tools 12.2, arch 10.5
#[derive(Debug, Default)]
pub struct VoxelColliders {
    /// The world hits are tested against; `None` while the model has no voxels.
    pub world: Option<Arc<VoxelWorld>>,
    /// Refreshes are suspended until the stroke ends.
    pub frozen: bool,
}
