#![forbid(unsafe_code)]
//! Layer L3. Views, cameras, navigation, preview anchors, picking (BVH, DDA, colliders), gizmo
//! math, atlas layout and draw lists.
//!
//! Owns the math of app 5 and 7-9, picking, gizmo geometry and hit volumes, the render extract
//! (instance records, atlas layout, voxel chunk meshes) and draw-list compilation. Must not
//! depend on wgpu.
//!
//! A frame runs in this order: [`SceneIndex::sync`] and [`SceneIndex::set_cameras`] for the
//! picking of the pointer, then [`Extractor::extract`] for the renderer, then
//! [`compile_draw_list`] per shown view.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod atlas_layout;
pub mod bvh;
pub mod camera;
pub mod colliders;
pub mod contract;
pub mod drawlist;
pub mod extract;
pub mod frame;
pub mod geom;
pub mod gizmo;
pub mod navigation;
pub mod pick;
pub mod preview;
pub mod views;

pub use bvh::Bvh;
pub use camera::Camera;
pub use colliders::VoxelColliders;
pub use contract::{Hit, HitTarget, Raycaster};
pub use cuboid_voxel::Ray;
pub use drawlist::{
    ChunkRange, DrawItem, DrawTarget, FrameKey, GridPlane, OverlayItem, OverlayKind, OverlayState,
    SceneCache, ViewDrawList, compile_draw_list,
};
pub use extract::{
    AtlasUpdate, ChunkMesh, ElementInstance, ExtractBudget, Extractor, INSTANCE_WORDS,
    InstanceUpdate, MeshUpdate, RenderExtract,
};
pub use frame::Frame;
pub use geom::{Aabb, Frustum, Obb};
pub use gizmo::{
    Axes, Constraint, Drag, DragDelta, GizmoHit, GizmoKind, GizmoPart, GizmoState, Snap,
    gizmo_scale, ring_axes, snap_value,
};
pub use navigation::{
    Buttons, Modifiers, NavAction, NavInput, NavPreset, ORBIT_DEG_PER_PIXEL, StandardView, aim_at,
    focus, frame_bounds, frame_sphere, navigate, orbit, orbit_degrees, pan, set_ortho, snap_view,
    zoom,
};
pub use pick::SceneIndex;
pub use preview::{DisplayMode, PreviewState, anchor_transform};
pub use views::{Rect, View, ViewId, ViewLayout, ViewSet};
