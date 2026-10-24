//! Public contract stubs of this crate: cross-crate types and traits with their spec references
//! (W0.2).
//!
//! This file holds the picking contract that `cuboid-core` consumes. Views, cameras, gizmo
//! state, draw lists and extracts live in their modules next to the stubs W3.1 to W3.4 replace;
//! `lib.rs` re-exports everything at the crate root.
//!
//! Changes by L3-A (roadmap_lean 4), all additive for the consumers of the picking and gizmo
//! types, which are unchanged:
//!
//! - `RenderExtract` gained `atlas_pages`, `atlas_page_size`, `voxel_pools` and `removed_meshes`;
//!   `ChunkMesh` gained `offsets` (the scene owns the layout of the voxel pools, so
//!   `ChunkRange.first` is valid without asking the renderer); `MeshUpdate` gained `model`,
//!   `tint`, `hidden` and `submeshes`. The word layout of `ElementInstance` is on
//!   `INSTANCE_WORDS`.
//! - `ViewSet` gained `layout` and `secondary` (`ViewLayout::Single` is the default; `Split2` and
//!   `Split4` show more views). `ViewDrawList.outline` lists the hovered object after the selected
//!   ones; the hovered element also carries the hover bit in its instance record.
//! - `navigation`: the pan of app 5.5 (absolute from a start record) is replaced by an
//!   incremental `pan`; `begin_pan` and `PanStart` are gone. Orbit, zoom and pan are the
//!   pivot-based functions of the two input presets (`NavPreset`).
//! - `SceneIndex` needs `set_cameras` once per frame for the screen-constant gizmo size and the
//!   camera-facing drag plane, and `set_origin_pickable` for the model-origin marker.

use cuboid_model::{FaceName, ObjId};
use cuboid_voxel::Ray;
use glam::{Vec2, Vec3};

use crate::gizmo::{GizmoHit, GizmoState};
use crate::views::ViewId;

/// What a model ray hit.
///
/// spec: tools 8.4, arch 13.7
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum HitTarget {
    /// An element.
    Element(ObjId),
    /// A mesh.
    Mesh(ObjId),
    /// The blueprint, on the model layer in every view.
    Blueprint,
    /// The model-origin marker.
    ModelOrigin,
    /// A voxel, through the collider snapshot.
    Voxel,
}

/// A model ray hit.
///
/// spec: tools 8.4, tools 12.2, arch 6 (cuboid-scene)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Hit {
    /// What was hit.
    pub target: HitTarget,
    /// The hit point.
    pub point: Vec3,
    /// The outward unit normal of the face hit.
    pub normal: Vec3,
    /// The element face slot, for element hits.
    pub face: Option<FaceName>,
    /// The texture coordinate at the hit, for painting on elements and meshes.
    pub uv: Option<Vec2>,
}

/// The queries the interaction state machine makes: the hover pick, the gizmo pick and the
/// drag plane. The scene index implements them; interaction tests use a mock until W3.2 lands.
///
/// spec: arch 10.5, arch 13.7, arch 6 (cuboid-scene)
pub trait Raycaster {
    /// The nearest front-facing hit within `max_len` against the model layer: active faces of
    /// elements and meshes, the blueprint, the model origin and the voxel colliders. Hidden
    /// objects are excluded. The hover ray is 1000 long.
    fn pick_model(&self, ray: Ray, max_len: f32) -> Option<Hit>;

    /// The nearest hit on a gizmo part visible in `view`. The gizmo ray is 100 long.
    fn pick_gizmo(&self, ray: Ray, view: ViewId, gizmo: &GizmoState) -> Option<GizmoHit>;

    /// The intersection of the ray with a large camera-facing plane through `through`. The drag
    /// ray is 200 long.
    fn drag_plane(&self, ray: Ray, through: Vec3, max_len: f32) -> Option<Vec3>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interaction state machine takes the ray caster as a trait object.
    fn _raycaster_is_object_safe(_: &dyn Raycaster) {}
}
