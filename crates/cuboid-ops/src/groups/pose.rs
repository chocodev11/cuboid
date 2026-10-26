//! Moving, turning and scaling whole subtrees (spec: hierarchy A1 4).
//!
//! A [`Pose`] holds the transforms of the objects and groups of some subtrees. Every change is
//! computed from it, never from the document as the last change left it, so a drag does not
//! pile up rounding from frame to frame.

use cuboid_model::{
    ChangeFlags, DocFlags, Document, EditCtx, ElementFlags, GroupId, MIN_SIZE, ObjRef, Rev,
};
use cuboid_num::{engine_eq, euler_to_quat, quat_to_euler, round4, snap_angle, wrap360};
use glam::{Quat, Vec3};

use super::{independent, leaves};
use crate::transform::{base_mut, refresh_uv, snap_to};

/// Restricted rotation refused the turn: it would leave an object off the 22.5 degree grid or on
/// a second axis.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RotationRefused;

#[derive(Debug)]
struct LeafPose {
    obj: ObjRef,
    p: Vec3,
    o: Vec3,
    s: Vec3,
    e: Vec3,
    /// Takes part in restricted rotation (elements, not meshes).
    restrictable: bool,
}

#[derive(Debug)]
struct GroupPose {
    id: GroupId,
    origin: Vec3,
    rotation: Vec3,
    /// For the groups the pose was asked for: the rotation of the groups above. The groups
    /// below them keep their own rotation relative to their parent.
    above: Option<Quat>,
}

/// The transforms of some subtrees at one moment.
#[derive(Debug)]
pub struct Pose {
    leaves: Vec<LeafPose>,
    groups: Vec<GroupPose>,
}

impl Pose {
    /// The pose of `nodes` and everything below the groups among them. Nodes with a selected
    /// group above them are left out, as the group already holds them.
    pub fn capture(doc: &Document, nodes: &[ObjRef]) -> Pose {
        let nodes = independent(doc, nodes);
        let tops: Vec<GroupId> = nodes
            .iter()
            .filter_map(|n| match n {
                ObjRef::Group(id) => Some(*id),
                _ => None,
            })
            .collect();
        let leaves = leaves(doc, &nodes)
            .into_iter()
            .filter_map(|obj| {
                let (base, restrictable) = match obj {
                    ObjRef::Element(id) => (&doc.elements.get(id)?.base, true),
                    ObjRef::Mesh(id) => (&doc.meshes.get(id)?.base, false),
                    _ => return None,
                };
                Some(LeafPose {
                    obj,
                    p: base.p,
                    o: base.o,
                    s: base.s,
                    e: base.e,
                    restrictable,
                })
            })
            .collect();
        let groups = doc
            .groups
            .subtree_groups(&tops)
            .into_iter()
            .filter_map(|id| {
                let group = doc.groups.get(id)?;
                let above = tops.contains(&id).then(|| {
                    doc.groups
                        .world_rotation(doc.groups.parent_of(ObjRef::Group(id)))
                });
                Some(GroupPose {
                    id,
                    origin: group.origin,
                    rotation: group.rotation,
                    above,
                })
            })
            .collect();
        Pose { leaves, groups }
    }

    /// The box around the objects of the pose: the corners of every box, turned. `None` when
    /// the pose holds no object.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for leaf in &self.leaves {
            let turn = euler_to_quat(leaf.e);
            for corner in 0..8_u8 {
                let unit = Vec3::new(
                    f32::from(corner & 1),
                    f32::from(corner >> 1 & 1),
                    -f32::from(corner >> 2 & 1),
                );
                let point = leaf.p + turn * (unit * leaf.s);
                lo = lo.min(point);
                hi = hi.max(point);
            }
        }
        lo.is_finite().then_some((lo, hi))
    }

    /// Number of objects and groups in the pose.
    pub fn len(&self) -> usize {
        self.leaves.len() + self.groups.len()
    }

    /// Whether the pose holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn write_leaf(
        cx: &mut EditCtx<'_>,
        leaf: &LeafPose,
        next: (Vec3, Vec3, Vec3, Vec3),
        turned: bool,
    ) -> bool {
        let (p, o, s, e) = next;
        let Some(base) = crate::transform::base_of(cx.doc, leaf.obj) else {
            return false;
        };
        if engine_eq(base.p, p) && engine_eq(base.o, o) && engine_eq(base.s, s) && base.e == e {
            return false;
        }
        let Some(base) = base_mut(cx, leaf.obj, ChangeFlags::TRANSFORM) else {
            return false;
        };
        (base.p, base.o, base.s, base.e) = (p, o, s, e);
        if let ObjRef::Element(id) = leaf.obj {
            if turned {
                // The default mapping follows the position of an unturned element only.
                let doc = &mut *cx.doc;
                if doc
                    .elements
                    .get(id)
                    .is_some_and(|el| el.flags.contains(ElementFlags::UV_SNAP))
                    && let Some(el) =
                        doc.elements
                            .edit_marked(id, ChangeFlags::TRANSFORM, &mut doc.journal)
                {
                    el.flags.remove(ElementFlags::UV_SNAP);
                }
            } else {
                refresh_uv(cx, leaf.obj);
            }
        }
        true
    }

    fn write_group(cx: &mut EditCtx<'_>, group: &GroupPose, origin: Vec3, rotation: Vec3) -> bool {
        let doc = &mut *cx.doc;
        let Some(current) = doc.groups.get(group.id) else {
            return false;
        };
        if engine_eq(current.origin, origin) && current.rotation == rotation {
            return false;
        }
        if let Some(record) = doc
            .groups
            .edit(group.id, ChangeFlags::TRANSFORM, &mut doc.journal)
        {
            record.origin = origin;
            record.rotation = rotation;
        }
        doc.journal.mark_doc(DocFlags::GROUPS);
        doc.revision = Rev::next();
        true
    }

    /// Moves everything by `delta`. Returns how many objects and groups changed.
    pub fn translate(&self, cx: &mut EditCtx<'_>, delta: Vec3) -> usize {
        let mut changed = 0;
        for leaf in &self.leaves {
            let next = (leaf.p + delta, leaf.o + delta, leaf.s, leaf.e);
            changed += usize::from(Self::write_leaf(cx, leaf, next, false));
        }
        for group in &self.groups {
            changed += usize::from(Self::write_group(
                cx,
                group,
                group.origin + delta,
                group.rotation,
            ));
        }
        changed
    }

    /// Turns everything by the world rotation `turn` about `pivot`. A group of the pose gets
    /// the local rotation that carries its frame along; the groups below it keep theirs.
    /// Nothing changes when restricted rotation refuses an object.
    pub fn rotate(
        &self,
        cx: &mut EditCtx<'_>,
        pivot: Vec3,
        turn: Quat,
    ) -> Result<usize, RotationRefused> {
        let restricted = cx.env.restrict_rotations;
        let identity = turn.abs_diff_eq(Quat::IDENTITY, 1e-7);
        let mut next = Vec::with_capacity(self.leaves.len());
        for leaf in &self.leaves {
            let moved = |v: Vec3| (pivot + turn * (v - pivot)).map(round4);
            let mut e = if identity {
                leaf.e
            } else {
                quat_to_euler(turn * euler_to_quat(leaf.e)).map(round4)
            };
            if restricted && !identity {
                e = restrict(e, leaf.restrictable).ok_or(RotationRefused)?;
            }
            let (p, o) = if identity {
                (leaf.p, leaf.o)
            } else {
                (moved(leaf.p), moved(leaf.o))
            };
            next.push((p, o, leaf.s, e));
        }
        let mut changed = 0;
        for (leaf, next) in self.leaves.iter().zip(next) {
            changed += usize::from(Self::write_leaf(cx, leaf, next, true));
        }
        for group in &self.groups {
            let origin = if identity {
                group.origin
            } else {
                (pivot + turn * (group.origin - pivot)).map(round4)
            };
            let rotation = match group.above {
                Some(above) if !identity => {
                    let local = above.inverse() * turn * above * euler_to_quat(group.rotation);
                    quat_to_euler(local).map(round4)
                }
                _ => group.rotation,
            };
            changed += usize::from(Self::write_group(cx, group, origin, rotation));
        }
        Ok(changed)
    }

    /// Scales everything about `pivot` by `factor` along the world axes: sizes, positions,
    /// pivots and the origins of the groups. With `snap` the new sizes are rounded to that grid.
    pub fn scale(
        &self,
        cx: &mut EditCtx<'_>,
        pivot: Vec3,
        factor: Vec3,
        snap: Option<f32>,
    ) -> usize {
        let about = |v: Vec3| (v - pivot) * factor + pivot;
        let mut changed = 0;
        for leaf in &self.leaves {
            let mut s = leaf.s * factor;
            if let Some(step) = snap {
                s = s.map(|c| snap_to(c, step));
            }
            let next = (
                about(leaf.p),
                about(leaf.o),
                s.max(Vec3::splat(MIN_SIZE)),
                leaf.e,
            );
            changed += usize::from(Self::write_leaf(cx, leaf, next, false));
        }
        for group in &self.groups {
            changed += usize::from(Self::write_group(
                cx,
                group,
                about(group.origin),
                group.rotation,
            ));
        }
        changed
    }
}

/// The rotation restricted mode keeps: on the 22.5 degree grid of elements or the 5 degree grid of
/// meshes, and one axis only for an element. `None` when the rotation is further than a
/// rounding error from such a rotation.
fn restrict(e: Vec3, restrictable: bool) -> Option<Vec3> {
    let snapped = e.map(|c| snap_angle(wrap360(c), restrictable));
    let near = |a: f32, b: f32| ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs() < 0.05;
    if !e
        .to_array()
        .into_iter()
        .zip(snapped.to_array())
        .all(|(a, b)| near(a, b))
    {
        return None;
    }
    let snapped = snapped.map(|c| c.rem_euclid(360.0));
    let axes = snapped.to_array().iter().filter(|c| **c != 0.0).count();
    (!restrictable || axes <= 1).then_some(snapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_rotation_keeps_the_grid_and_one_axis() {
        let tolerated = |e: Vec3| restrict(e, true);
        assert_eq!(
            tolerated(Vec3::new(0.0, 22.500_02, 0.0)),
            Some(Vec3::new(0.0, 22.5, 0.0))
        );
        assert_eq!(
            tolerated(Vec3::new(0.0, 359.999, 0.0)),
            Some(Vec3::ZERO),
            "a full turn is no rotation"
        );
        assert_eq!(tolerated(Vec3::new(0.0, 15.0, 0.0)), None);
        assert_eq!(tolerated(Vec3::new(22.5, 22.5, 0.0)), None, "two axes");
        // A mesh may use any axes, on the 5 degree grid.
        assert_eq!(
            restrict(Vec3::new(10.0, 15.0, 0.0), false),
            Some(Vec3::new(10.0, 15.0, 0.0))
        );
    }
}
