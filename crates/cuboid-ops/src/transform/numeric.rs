//! Numeric transforms of a list of objects. Each object is edited on its own: the setters never
//! propagate to the rest of the selection here.

use cuboid_model::{ChangeFlags, EditCtx, ElementMut, MIN_SIZE, ObjRef, RotOpts, RotResult};
use cuboid_num::engine_eq;
use glam::Vec3;

use super::{
    Axis, base_mut, base_of, changed, pivot_of, position_of, refresh_uv, set_origin_move,
    set_rotation, set_size, snap_to, world_center,
};

/// What a rotation request did to a list of objects.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Rotated {
    /// Objects whose rotation changed.
    pub changed: usize,
    /// Objects the restricted-rotation rule refused.
    pub rejected: usize,
}

fn merge(old: Vec3, value: [Option<f32>; 3]) -> Vec3 {
    Vec3::new(
        value[0].unwrap_or(old.x),
        value[1].unwrap_or(old.y),
        value[2].unwrap_or(old.z),
    )
}

fn snap_vec(v: Vec3, step: Option<f32>) -> Vec3 {
    step.map_or(v, |step| {
        Vec3::new(snap_to(v.x, step), snap_to(v.y, step), snap_to(v.z, step))
    })
}

/// Moves the pivot (and so the position) of every object by `delta`; with `snap` the new pivot
/// is rounded to the grid. Returns how many objects moved.
///
/// spec: tools 10.13 (Move)
pub fn translate(cx: &mut EditCtx<'_>, objs: &[ObjRef], delta: Vec3, snap: Option<f32>) -> usize {
    let mut moved = 0;
    for &obj in objs {
        let Some(pivot) = pivot_of(cx.doc, obj) else {
            continue;
        };
        let target = snap_vec(pivot + delta, snap);
        moved += usize::from(changed(cx, |cx| set_origin_move(cx, obj, target, false)));
    }
    moved
}

/// Sets the position component-wise, `None` leaving a component alone; the pivot moves with it.
/// Returns how many objects moved.
///
/// spec: tools 10.13 (Set position)
pub fn set_position(cx: &mut EditCtx<'_>, objs: &[ObjRef], value: [Option<f32>; 3]) -> usize {
    let mut moved = 0;
    for &obj in objs {
        let (Some(position), Some(pivot)) = (position_of(cx.doc, obj), pivot_of(cx.doc, obj))
        else {
            continue;
        };
        let target = pivot + (merge(position, value) - position);
        moved += usize::from(changed(cx, |cx| set_origin_move(cx, obj, target, false)));
    }
    moved
}

/// Sets the size component-wise through the size setter (pivot-relative, clamped). Returns how
/// many objects changed.
///
/// spec: tools 3.3
pub fn resize(cx: &mut EditCtx<'_>, objs: &[ObjRef], value: [Option<f32>; 3]) -> usize {
    let mut resized = 0;
    for &obj in objs {
        let Some(size) = base_of(cx.doc, obj).map(|b| b.s) else {
            continue;
        };
        let target = merge(size, value);
        resized += usize::from(changed(cx, |cx| set_size(cx, obj, target, false)));
    }
    resized
}

/// Scales every object about `pivot`: sizes, positions and pivots scale by `factor`, sizes are
/// then rounded to the grid when `snap` is set and kept above the minimum. Returns how many
/// objects changed.
///
/// spec: tools 10.13 (Scale)
pub fn scale_about(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    pivot: Vec3,
    factor: Vec3,
    snap: Option<f32>,
) -> usize {
    let mut scaled = 0;
    for &obj in objs {
        let Some(base) = base_of(cx.doc, obj) else {
            continue;
        };
        let size = snap_vec(base.s * factor, snap).max(Vec3::splat(MIN_SIZE));
        let position = (base.p - pivot) * factor + pivot;
        let origin = (base.o - pivot) * factor + pivot;
        if engine_eq(size, base.s) && engine_eq(position, base.p) && engine_eq(origin, base.o) {
            continue;
        }
        if let Some(base) = base_mut(cx, obj, ChangeFlags::TRANSFORM) {
            base.s = size;
            base.p = position;
            base.o = if obj == ObjRef::Blueprint {
                position
            } else {
                origin
            };
            scaled += 1;
        }
        refresh_uv(cx, obj);
    }
    scaled
}

fn rotate_each(cx: &mut EditCtx<'_>, objs: &[ObjRef], target: impl Fn(Vec3) -> Vec3) -> Rotated {
    let opts = RotOpts {
        propagate: false,
        force: false,
        about_pivot: true,
    };
    let mut out = Rotated::default();
    for &obj in objs {
        let Some(current) = base_of(cx.doc, obj).map(|b| b.e) else {
            continue;
        };
        let before = cx.doc.revision;
        match set_rotation(cx, obj, target(current), opts) {
            RotResult::Rejected => out.rejected += 1,
            RotResult::Applied | RotResult::Unchanged => {
                out.changed += usize::from(cx.doc.revision != before);
            }
        }
    }
    out
}

/// Adds `delta` degrees to the rotation of every object, about its pivot. With `snap` the delta
/// is first rounded to that many degrees. The rotation setter then applies its own grid and the
/// restricted-rotation rule.
///
/// spec: tools 3.4
pub fn rotate_by(cx: &mut EditCtx<'_>, objs: &[ObjRef], delta: Vec3, snap: Option<f32>) -> Rotated {
    let delta = snap_vec(delta, snap);
    rotate_each(cx, objs, |current| current + delta)
}

/// Sets the rotation component-wise (degrees), `None` leaving a component alone.
///
/// spec: tools 3.4
pub fn set_rotation_to(cx: &mut EditCtx<'_>, objs: &[ObjRef], value: [Option<f32>; 3]) -> Rotated {
    rotate_each(cx, objs, |current| merge(current, value))
}

/// Resets the rotation of every object to zero about its pivot, and the pivot of an element to
/// its position. Returns how many objects changed.
///
/// spec: tools 10.12 (Reset rotation)
pub fn reset_rotation(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> usize {
    let force = RotOpts {
        propagate: false,
        force: true,
        about_pivot: true,
    };
    let mut reset = 0;
    for &obj in objs {
        if base_of(cx.doc, obj).is_none() {
            continue;
        }
        let before = cx.doc.revision;
        set_rotation(cx, obj, Vec3::ZERO, force);
        if let ObjRef::Element(_) = obj
            && base_of(cx.doc, obj).is_some_and(|b| !engine_eq(b.o, b.p))
            && let Some(base) = base_mut(cx, obj, ChangeFlags::TRANSFORM)
        {
            base.o = base.p;
        }
        reset += usize::from(cx.doc.revision != before);
    }
    reset
}

/// Moves the pivot of every element and mesh to the world centre of its box. The geometry does
/// not move: the stored position is a world position and the pivot only decides what later
/// rotations turn about. Returns how many objects changed.
pub fn reset_pivot(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> usize {
    let mut reset = 0;
    for &obj in objs {
        if !matches!(obj, ObjRef::Element(_) | ObjRef::Mesh(_)) {
            continue;
        }
        let Some(base) = base_of(cx.doc, obj) else {
            continue;
        };
        let centre = world_center(base);
        if engine_eq(centre, base.o) {
            continue;
        }
        if let Some(base) = base_mut(cx, obj, ChangeFlags::TRANSFORM) {
            base.o = centre;
            reset += 1;
        }
    }
    reset
}

/// Turns every element a quarter turn (+90 degrees) about a world axis of the block centre,
/// keeping it axis-aligned. Meshes are left alone. Returns how many elements turned.
///
/// spec: tools 4.9, tools 10.12
pub fn quarter_turn(cx: &mut EditCtx<'_>, objs: &[ObjRef], axis: Axis) -> usize {
    let model_axis = match axis {
        Axis::X => cuboid_model::setters::Axis::X,
        Axis::Y => cuboid_model::setters::Axis::Y,
        Axis::Z => cuboid_model::setters::Axis::Z,
    };
    let mut turned = 0;
    for &obj in objs {
        if let ObjRef::Element(id) = obj
            && let Some(mut guard) = ElementMut::new(cx, id)
        {
            guard.quarter_turn(model_axis, 90);
            turned += 1;
        }
    }
    turned
}
