//! What the Animate mode shows and the pose the core works on while it does (spec: animate A3 5).
//!
//! The app says which animation and which time the views show. The core computes the pose of
//! that animation itself, with a [`Rig`] and a [`Pose`] it keeps between calls, so the gizmo,
//! the clicks and the drags have the posed bones without the core knowing the player.

use cuboid_model::{
    Animation, AnimationId, Document, GroupId, ObjRef, Pose, Rev, Rig, group_values,
};
use cuboid_ops::transform::{base_of, world_center};
use cuboid_scene::{Frame, GizmoKind};
use glam::{Affine3A, Mat3, Quat, Vec3};

use crate::core::Core;

/// The smallest size the box of a bone has for the scale gizmo.
const MIN_BOX: f32 = 0.001;

/// What the Animate mode shows: the animation and the time of the playhead in seconds.
///
/// spec: animate A3 5
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Playhead {
    /// The animation, `None` when the model has none to key.
    pub animation: Option<AnimationId>,
    /// Seconds from the start of the animation.
    pub time: f32,
}

/// The pose of the animation at the playhead and the revisions it was computed for.
#[derive(Debug)]
pub(crate) struct Posed {
    shown: Playhead,
    rig: Rig,
    pose: Pose,
    /// The revisions of the groups and of the animations the pose belongs to.
    key: Option<(Rev, Rev)>,
}

impl Posed {
    pub(crate) fn new(shown: Playhead, doc: &Document) -> Posed {
        Posed {
            shown,
            rig: Rig::new(&doc.groups),
            pose: Pose::new(),
            key: None,
        }
    }

    pub(crate) fn shown(&self) -> Playhead {
        self.shown
    }

    /// Shows another animation or time; the pose is computed again at the next refresh.
    pub(crate) fn show(&mut self, shown: Playhead) {
        self.shown = shown;
        self.key = None;
    }

    /// The animation being keyed, when the model has it.
    pub(crate) fn animation<'a>(&self, doc: &'a Document) -> Option<(AnimationId, &'a Animation)> {
        let id = self.shown.animation?;
        Some((id, doc.animations.get(id)?))
    }

    /// Computes the pose again when the groups or the animations changed.
    pub(crate) fn refresh(&mut self, doc: &Document) {
        let key = (doc.groups.rev(), doc.animations.rev());
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        if self.rig.groups_rev() != key.0 {
            self.rig = Rig::new(&doc.groups);
        }
        match self.animation(doc) {
            Some((_, animation)) => self.rig.pose(animation, self.shown.time, &mut self.pose),
            None => self.pose.clear(),
        }
    }

    /// The motion the pose gives a bone, `None` when it stays at rest.
    pub(crate) fn delta_of(&self, group: GroupId) -> Option<Affine3A> {
        self.pose.delta_of(group).copied()
    }

    /// The frame of a bone in the pose: its pivot, turned as the pose turns it.
    pub(crate) fn frame_of(&self, group: GroupId) -> Option<Affine3A> {
        let rest = self.rig.rest_frame(group)?;
        Some(self.delta_of(group).map_or(rest, |delta| delta * rest))
    }

    /// The pivot of a bone in the pose and its rotation, without the scale and shear that the
    /// motion of the bones above may have put in.
    pub(crate) fn pivot_of(&self, group: GroupId) -> Option<(Vec3, Quat)> {
        let unit = Frame {
            pos: Vec3::ZERO,
            rot: Quat::IDENTITY,
            inv_rot: Quat::IDENTITY,
            size: Vec3::ONE,
        };
        let posed = unit.posed(&self.frame_of(group)?);
        Some((posed.pos, posed.rot))
    }

    /// The linear part of the frame of the bone above `group`: what turns and stretches the
    /// position channel of the bone into the world. The identity for a bone at the top.
    pub(crate) fn above(&self, doc: &Document, group: GroupId) -> Mat3 {
        doc.groups
            .parent_of(ObjRef::Group(group))
            .and_then(|parent| self.frame_of(parent))
            .map_or(Mat3::IDENTITY, |frame| Mat3::from(frame.matrix3))
    }

    /// Where the pose carries a point of the rest pose that belongs to `obj`.
    pub(crate) fn carry(&self, doc: &Document, obj: ObjRef, point: Vec3) -> Vec3 {
        doc.groups
            .parent_of(obj)
            .and_then(|group| self.delta_of(group))
            .map_or(point, |delta| delta.transform_point3(point))
    }

    /// The box around everything inside a bone in the pose, in the axes of the bone: its
    /// smallest corner and its largest, from the pivot. `None` when the bone holds no object.
    fn box_of(
        &self,
        doc: &Document,
        group: GroupId,
        pivot: Vec3,
        rot: Quat,
    ) -> Option<(Vec3, Vec3)> {
        let turn = rot.inverse();
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for obj in doc.groups.descendants(&[group]) {
            let Some(base) = base_of(doc, obj) else {
                continue;
            };
            let at_rest = cuboid_num::euler_to_quat(base.e);
            for corner in 0..8_u8 {
                let unit = Vec3::new(
                    f32::from(corner & 1),
                    f32::from(corner >> 1 & 1),
                    -f32::from(corner >> 2 & 1),
                );
                let point = self.carry(doc, obj, base.p + at_rest * (unit * base.s));
                let local = turn * (point - pivot);
                lo = lo.min(local);
                hi = hi.max(local);
            }
        }
        lo.is_finite().then_some((lo, hi))
    }

    /// The corner of the box of a bone (smallest X and Y, largest Z) and its size, in the world.
    /// The scale gizmo stands on it.
    pub(crate) fn scale_box(&self, doc: &Document, group: GroupId) -> Option<(Vec3, Vec3)> {
        let (pivot, rot) = self.pivot_of(group)?;
        let (lo, hi) = self.box_of(doc, group, pivot, rot)?;
        let corner = pivot + rot * Vec3::new(lo.x, lo.y, hi.z);
        Some((corner, (hi - lo).max(Vec3::splat(MIN_BOX))))
    }

    /// The world centre of an object in the pose, for a box selection.
    pub(crate) fn centre_of(&self, doc: &Document, obj: ObjRef) -> Option<Vec3> {
        Some(self.carry(doc, obj, world_center(base_of(doc, obj)?)))
    }
}

/// The values of the channel a gizmo of this kind keys, at `time`: what a drag adds to.
pub(crate) fn channel_value(
    animation: &Animation,
    group: GroupId,
    kind: GizmoKind,
    time: f32,
) -> Vec3 {
    let values = group_values(animation, group, time);
    match kind {
        GizmoKind::Move => values.position,
        GizmoKind::Rotate => values.rotation,
        GizmoKind::Scale => values.scale,
    }
}

impl Core {
    /// Tells the core what the Animate mode shows, or `None` outside it. In Animate the gizmo
    /// stands on the posed bone and a drag writes keyframes at the playhead; clicks select bones.
    ///
    /// spec: animate A3 5
    pub fn set_animating(&mut self, shown: Option<Playhead>) {
        let Some(shown) = shown else {
            if self.animate.take().is_some() {
                self.interact.cancel();
            }
            return;
        };
        match &mut self.animate {
            // The pose follows the commands and the history on its own.
            Some(posed) if posed.shown() == shown => return,
            Some(posed) => {
                if posed.shown().animation != shown.animation {
                    self.interact.cancel();
                }
                posed.show(shown);
            }
            None => {
                self.interact.cancel();
                self.animate = Some(Posed::new(shown, &self.doc));
                if shown.animation.is_none() {
                    self.status
                        .set("Add an animation to key the pose", self.clock.now);
                }
            }
        }
        self.refresh_pose();
    }

    /// What the Animate mode shows now; `None` outside it.
    pub fn animating(&self) -> Option<Playhead> {
        self.animate.as_ref().map(Posed::shown)
    }

    /// Brings the pose up to date after the groups or the animations changed.
    pub(crate) fn refresh_pose(&mut self) {
        if let Some(posed) = &mut self.animate {
            posed.refresh(&self.doc);
        }
    }
}
