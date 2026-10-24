//! Clean-up after a pointer drag.

use cuboid_model::{ChangeFlags, EditCtx, ElementMut, ObjRef};
use cuboid_num::{engine_eq, round4};
use glam::Vec3;

use super::{base_mut, base_of, refresh_uv};

/// Cleans the position and pivot of unrotated objects to four decimals, as a drag end does, so
/// floating-point noise from the drag plane does not reach the file. Returns how many objects
/// changed.
///
/// spec: tools 1.6
pub fn clean_after_drag(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> usize {
    let mut cleaned = 0;
    for &obj in objs {
        let Some(base) = base_of(cx.doc, obj) else {
            continue;
        };
        if !engine_eq(base.e, Vec3::ZERO) {
            continue;
        }
        let (p, o) = (base.p.map(round4), base.o.map(round4));
        if p == base.p && o == base.o {
            continue;
        }
        if let Some(base) = base_mut(cx, obj, ChangeFlags::TRANSFORM) {
            base.p = p;
            base.o = if obj == ObjRef::Blueprint { p } else { o };
            cleaned += 1;
        }
        refresh_uv(cx, obj);
    }
    cleaned
}

/// Bakes the restricted rotation of the primary element (and the other selected elements) after
/// a rotation drag: one rotated axis within 45 degrees and quarter-turn face permutations.
///
/// spec: tools 3.5
pub fn bake_restricted(cx: &mut EditCtx<'_>, primary: ObjRef) {
    if let ObjRef::Element(id) = primary
        && let Some(mut guard) = ElementMut::new(cx, id)
    {
        guard.bake_restricted_rotation();
    }
}
