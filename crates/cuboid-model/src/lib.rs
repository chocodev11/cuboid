#![forbid(unsafe_code)]
//! Layer L2. Document, `ObjList`, objects, setters, UV, change journal, selection, apply and
//! capture, snapshots and invariants.
//!
//! Owns document state and the setter semantics of tools 2-6 and the apply/capture rules of
//! native 5-6. Must not perform I/O, spawn threads or know codecs or UI.

#![deny(clippy::cast_possible_truncation)]

pub mod animation;
pub mod apply;
pub mod capture;
pub mod contract;
pub mod groups;
pub mod ids;
pub mod invariants;
pub mod journal;
pub mod list;
pub mod mc;
pub mod normal_form;
pub mod objects;
pub mod pose;
#[cfg(any(test, feature = "reference"))]
pub mod reference;
pub mod selection;
pub mod setters;
pub mod snapshot;
pub mod uv;

// The sibling crates whose types appear in the public fields of the document, so crates above
// the model can name them without a dependency of their own.
pub use cuboid_image as image;
pub use cuboid_voxel as voxel;

pub use animation::{
    Animation, AnimationError, AnimationId, Animations, BoneTrack, Channel, DEFAULT_SNAPPING,
    Handles, Interp, Keyframe, LoopMode, MAX_ANIMATIONS, MAX_EXPRESSION_BYTES, MAX_EXTRA_BYTES,
    MAX_KEYFRAMES, MAX_LENGTH, SNAPPING_RANGE, TIME_EPSILON, Value,
};
pub use apply::{ApplyError, ApplyMode, ApplyReport, ApplySource, LoadedDoc, apply_document};
pub use capture::{CaptureScope, capture};
pub use contract::*;
pub use groups::{
    Group, GroupError, GroupFrame, GroupId, Groups, GroupsBuilder, MAX_DEPTH, MAX_GROUPS,
};
pub use ids::{ObjId, ObjRef, Rev};
pub use invariants::{InvariantError, check_invariants};
pub use journal::{ChangeFlags, ChangeJournal, DocFlags};
pub use list::{ListSnap, ObjList};
pub use objects::*;
pub use pose::{BoneValues, Pose, Rig, animation_time, bone_values, group_values, pose, sample};
pub use selection::Selection;
pub use setters::{BlueprintMut, ElementMut, MeshMut, RotOpts, RotResult};
pub use snapshot::{Categories, ElementsSnap, GroupsSnap, MeshesSnap, Snapshot, VoxelSnap};
pub use uv::UvRect;
