//! Transform operations: numeric move, resize, scale, rotate, pivot reset, mirror and align, and
//! the helpers pointer drags use (spec: tools 3.2-3.5, 4.9, 10.12, 10.13).
//!
//! Vectors are in model (internal) space, the space of the stored fields: `ui = (x, y, -z)`
//! only for display (tools 1.2). An object's box is `[p, p + s]` on X and Y and `[p - s, p]` on
//! Z (tools 1.3).

mod arrange;
mod drag;
mod numeric;

use cuboid_model::normal_form::free_rotation;
use cuboid_model::setters::set_model_origin;
use cuboid_model::{
    BlueprintMut, ChangeFlags, DocFlags, Document, EditCtx, ElementMut, GroupId, MeshMut, ObjRef,
    ObjectBase, Rev, RotOpts, RotResult,
};
use cuboid_num::{engine_eq, euler_to_quat, round_he, snap_angle, wrap360};
use glam::Vec3;

use crate::groups::Pose;

pub use arrange::{AlignEdge, Mirrored, align, mirror};
pub use drag::{bake_restricted, clean_after_drag};
pub use numeric::{
    Rotated, quarter_turn, reset_pivot, reset_rotation, resize, rotate_by, scale_about, set_pivot,
    set_position, set_rotation_to, translate,
};

/// A coordinate axis of a command.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Axis {
    /// X.
    X,
    /// Y.
    Y,
    /// Z.
    Z,
}

impl Axis {
    /// The unit vector of the axis.
    pub fn unit(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }

    /// The component index, 0 to 2.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The shared fields of an element, mesh or the blueprint; the model origin and a group have
/// none.
pub fn base_of(doc: &Document, obj: ObjRef) -> Option<&ObjectBase> {
    match obj {
        ObjRef::Element(id) => doc.elements.get(id).map(|e| &e.base),
        ObjRef::Mesh(id) => doc.meshes.get(id).map(|m| &m.base),
        ObjRef::Blueprint => Some(&doc.blueprint.base),
        ObjRef::Group(_) | ObjRef::ModelOrigin => None,
    }
}

/// The position of an object: the box corner `p`, the marker position for the model origin, or
/// the pivot of a group.
pub fn position_of(doc: &Document, obj: ObjRef) -> Option<Vec3> {
    match obj {
        ObjRef::ModelOrigin => Some(doc.props.general.origin),
        ObjRef::Group(id) => doc.groups.get(id).map(|g| g.origin),
        _ => base_of(doc, obj).map(|b| b.p),
    }
}

/// The pivot of an object; the model origin and a group are their own pivot.
pub fn pivot_of(doc: &Document, obj: ObjRef) -> Option<Vec3> {
    match obj {
        ObjRef::ModelOrigin => Some(doc.props.general.origin),
        ObjRef::Group(id) => doc.groups.get(id).map(|g| g.origin),
        _ => base_of(doc, obj).map(|b| b.o),
    }
}

/// The rotation of an object as its field shows it; zero for the model origin.
pub fn rotation_of(doc: &Document, obj: ObjRef) -> Option<Vec3> {
    match obj {
        ObjRef::Group(id) => doc.groups.get(id).map(|g| g.rotation),
        ObjRef::ModelOrigin => None,
        _ => base_of(doc, obj).map(|b| b.e),
    }
}

/// The world centre of the box of an object, rotation included.
pub fn world_center(base: &ObjectBase) -> Vec3 {
    base.p + euler_to_quat(base.e) * (base.s * Vec3::new(0.5, 0.5, -0.5))
}

/// The bounds of the unrotated box on one axis; the model origin is a point.
pub fn extent(doc: &Document, obj: ObjRef, axis: Axis) -> Option<(f32, f32)> {
    let i = axis.index();
    if obj == ObjRef::ModelOrigin {
        let o = doc.props.general.origin[i];
        return Some((o, o));
    }
    let base = base_of(doc, obj)?;
    let (p, s) = (base.p[i], base.s[i]);
    Some(match axis {
        Axis::Z => (p - s, p),
        _ => (p, p + s),
    })
}

/// Rounds to the nearest multiple of `step`, half to even; a step that is not positive and
/// finite leaves the value alone.
pub fn snap_to(value: f32, step: f32) -> f32 {
    if step.is_finite() && step > 0.0 {
        round_he(value / step) * step
    } else {
        value
    }
}

/// The base of an object for writing, with its journal entry and a new document revision. The
/// blueprint has no journal entry; the model origin has no base.
pub(crate) fn base_mut<'a>(
    cx: &'a mut EditCtx<'_>,
    obj: ObjRef,
    flags: ChangeFlags,
) -> Option<&'a mut ObjectBase> {
    let doc = &mut *cx.doc;
    let base = match obj {
        ObjRef::Element(id) => doc
            .elements
            .edit_marked(id, flags, &mut doc.journal)
            .map(|e| &mut e.base),
        ObjRef::Mesh(id) => doc
            .meshes
            .edit_marked(id, flags, &mut doc.journal)
            .map(|m| &mut m.base),
        ObjRef::Blueprint => Some(&mut doc.blueprint.base),
        ObjRef::Group(_) | ObjRef::ModelOrigin => None,
    };
    if base.is_some() {
        doc.revision = Rev::next();
    }
    base
}

/// Re-derives the default UV mapping of an element whose UV snap is on.
pub(crate) fn refresh_uv(cx: &mut EditCtx<'_>, obj: ObjRef) {
    if let ObjRef::Element(id) = obj
        && let Some(mut guard) = ElementMut::new(cx, id)
    {
        guard.default_uv(false);
    }
}

/// Runs `edit` and tells whether the document changed.
pub(crate) fn changed(cx: &mut EditCtx<'_>, edit: impl FnOnce(&mut EditCtx<'_>)) -> bool {
    let before = cx.doc.revision;
    edit(cx);
    cx.doc.revision != before
}

/// "Set origin and move": the pivot becomes `target` and the position follows by the old offset
/// between them. With `propagate` and `obj` the primary, the other selected objects move by the
/// same delta. A group moves with everything below it and does not propagate.
///
/// spec: tools 3.2
pub fn set_origin_move(cx: &mut EditCtx<'_>, obj: ObjRef, target: Vec3, propagate: bool) {
    match obj {
        ObjRef::Group(id) => {
            if let Some(group) = cx.doc.groups.get(id) {
                let delta = target - group.origin;
                Pose::capture(cx.doc, &[obj]).translate(cx, delta);
            }
        }
        ObjRef::Element(id) => {
            if let Some(mut guard) = ElementMut::new(cx, id) {
                guard.set_origin_move(target, propagate);
            }
        }
        ObjRef::Mesh(id) => {
            if let Some(mut guard) = MeshMut::new(cx, id) {
                guard.set_origin_move(target, propagate);
            }
        }
        ObjRef::Blueprint => BlueprintMut::new(cx).set_origin_move(target, propagate),
        ObjRef::ModelOrigin => set_model_origin(cx.doc, target),
    }
}

/// The size setter: pivot-relative rescale, clamped to 0.001. The setter always resizes the
/// other selected objects when `obj` is the primary; `propagate` off hides the primary for the
/// call. The model origin has no size.
///
/// spec: tools 3.3
pub fn set_size(cx: &mut EditCtx<'_>, obj: ObjRef, size: Vec3, propagate: bool) {
    let primary = cx.sel.primary;
    if !propagate {
        cx.sel.primary = None;
    }
    match obj {
        ObjRef::Element(id) => {
            if let Some(mut guard) = ElementMut::new(cx, id) {
                guard.set_size(size);
            }
        }
        ObjRef::Mesh(id) => {
            if let Some(mut guard) = MeshMut::new(cx, id) {
                guard.set_size(size);
            }
        }
        ObjRef::Blueprint => BlueprintMut::new(cx).set_size(size),
        ObjRef::Group(_) | ObjRef::ModelOrigin => {}
    }
    cx.sel.primary = primary;
}

/// The rotation setter. `opts.propagate` rotates the other selected objects about the primary's
/// pivot when `obj` is the primary. A group turns everything below it about its pivot and does
/// not propagate.
///
/// spec: tools 3.4
pub fn set_rotation(cx: &mut EditCtx<'_>, obj: ObjRef, rotation: Vec3, opts: RotOpts) -> RotResult {
    match obj {
        ObjRef::Group(id) => rotate_group(cx, id, rotation),
        ObjRef::Element(id) => ElementMut::new(cx, id).map_or(RotResult::Unchanged, |mut guard| {
            guard.set_rotation(rotation, opts)
        }),
        ObjRef::Mesh(id) => MeshMut::new(cx, id).map_or(RotResult::Unchanged, |mut guard| {
            guard.set_rotation(rotation, opts)
        }),
        ObjRef::Blueprint => BlueprintMut::new(cx).set_rotation(rotation, opts),
        ObjRef::ModelOrigin => RotResult::Unchanged,
    }
}

/// Sets the local rotation of a group: everything below it turns about its pivot, and the
/// group keeps the value. Restricted rotation rounds the value to the 22.5 degree grid and
/// refuses a turn that would leave an object off its grid.
fn rotate_group(cx: &mut EditCtx<'_>, id: GroupId, rotation: Vec3) -> RotResult {
    let Some(group) = cx.doc.groups.get(id) else {
        return RotResult::Unchanged;
    };
    let target = if cx.env.restrict_rotations {
        rotation.map(|c| snap_angle(wrap360(c), true))
    } else {
        free_rotation(rotation)
    };
    if engine_eq(target, group.rotation) {
        return RotResult::Unchanged;
    }
    let above = cx
        .doc
        .groups
        .world_rotation(cx.doc.groups.parent_of(ObjRef::Group(id)));
    let turn =
        above * euler_to_quat(target) * euler_to_quat(group.rotation).inverse() * above.inverse();
    let pivot = group.origin;
    let pose = Pose::capture(cx.doc, &[ObjRef::Group(id)]);
    if pose.rotate(cx, pivot, turn).is_err() {
        return RotResult::Rejected;
    }
    // The turn computed an equal rotation; the field keeps the value it was given.
    let doc = &mut *cx.doc;
    if let Some(record) = doc
        .groups
        .edit(id, ChangeFlags::TRANSFORM, &mut doc.journal)
        && record.rotation != target
    {
        record.rotation = target;
        doc.journal.mark_doc(DocFlags::GROUPS);
        doc.revision = Rev::next();
    }
    RotResult::Applied
}
