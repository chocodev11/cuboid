//! Mirror and align.

use cuboid_model::{ChangeFlags, EditCtx, ElementMut, FaceFlags, FaceName, ObjRef};
use cuboid_num::{engine_eq, euler_to_quat, wrap360};
use glam::Vec3;

use super::{Axis, base_of, changed, extent, pivot_of, set_origin_move};
use crate::OpError;
use crate::elements::duplicate;

/// Which edge of the reference box the other boxes line up with.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum AlignEdge {
    /// The low side of the axis.
    Min,
    /// The middle.
    Center,
    /// The high side of the axis.
    Max,
}

/// What a mirror did.
#[derive(Clone, Debug, Default)]
pub struct Mirrored {
    /// Elements that were mirrored.
    pub mirrored: usize,
    /// Objects that were skipped because only elements can be mirrored.
    pub skipped: usize,
    /// The copies, when the mirror was asked to work on copies; they are selected.
    pub copies: Vec<ObjRef>,
}

/// Mirrors the elements among `objs` across the plane through the centre of their combined
/// bounds, perpendicular to `axis`. The face pair of the axis swaps, so the textures end up on
/// the mirrored sides; UV rectangles of elements without UV snap are not flipped. With `copy`
/// the mirror is applied to copies of the elements and the copies end up selected.
pub fn mirror(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    axis: Axis,
    copy: bool,
) -> Result<Mirrored, OpError> {
    let elements: Vec<ObjRef> = objs
        .iter()
        .copied()
        .filter(|o| matches!(o, ObjRef::Element(_)))
        .collect();
    let mut out = Mirrored {
        skipped: objs.len() - elements.len(),
        ..Mirrored::default()
    };
    let Some(plane) = plane_of(cx, &elements, axis) else {
        return Ok(out);
    };
    let targets = if copy {
        out.copies = duplicate(cx, &elements)?;
        out.copies.clone()
    } else {
        elements
    };
    for &obj in &targets {
        out.mirrored += usize::from(mirror_one(cx, obj, axis, plane));
    }
    Ok(out)
}

/// The middle of the combined bounds of the objects on one axis.
fn plane_of(cx: &EditCtx<'_>, objs: &[ObjRef], axis: Axis) -> Option<f32> {
    let (lo, hi) = objs.iter().filter_map(|&o| extent(cx.doc, o, axis)).fold(
        None,
        |acc: Option<(f32, f32)>, (lo, hi)| {
            Some(acc.map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))))
        },
    )?;
    Some((lo + hi) * 0.5)
}

/// The sides of a cull flag swap along the mirrored axis.
fn mirror_cull(flags: FaceFlags, axis: Axis) -> FaceFlags {
    let (a, b) = match axis {
        Axis::X => (FaceFlags::WEST, FaceFlags::EAST),
        Axis::Y => (FaceFlags::DOWN, FaceFlags::UP),
        Axis::Z => (FaceFlags::NORTH, FaceFlags::SOUTH),
    };
    let mut out = flags - a - b;
    out.set(b, flags.contains(a));
    out.set(a, flags.contains(b));
    out
}

fn mirror_one(cx: &mut EditCtx<'_>, obj: ObjRef, axis: Axis, plane: f32) -> bool {
    let ObjRef::Element(id) = obj else {
        return false;
    };
    let Some(base) = base_of(cx.doc, obj) else {
        return false;
    };
    let i = axis.index();
    // A reflection turns the rotation about the other two axes the opposite way.
    let mut e = base.e;
    for k in (0..3).filter(|&k| k != i) {
        e[k] = wrap360(-e[k]);
    }
    let rot = euler_to_quat(e);
    let mut p = base.p;
    p[i] = 2.0 * plane - p[i];
    // The local vertex (0, 0, 0) of the mirrored box is the far side of the old one.
    p += match axis {
        Axis::Z => rot * (Vec3::Z * base.s.z),
        _ => rot * (-axis.unit() * base.s[i]),
    };
    let mut o = base.o;
    o[i] = 2.0 * plane - o[i];

    let (first, second) = match axis {
        Axis::X => (FaceName::West, FaceName::East),
        Axis::Y => (FaceName::Down, FaceName::Up),
        Axis::Z => (FaceName::North, FaceName::South),
    };
    let Some(el) = cx.doc.elements.edit_marked(
        id,
        ChangeFlags::TRANSFORM | ChangeFlags::FACES | ChangeFlags::UV,
        &mut cx.doc.journal,
    ) else {
        return false;
    };
    el.base.e = e;
    el.base.p = p;
    el.base.o = o;
    let (a, b) = (
        el.faces[first as usize].clone(),
        el.faces[second as usize].clone(),
    );
    a.copy_to(&mut el.faces[second as usize], true);
    b.copy_to(&mut el.faces[first as usize], true);
    for face in &mut el.faces {
        face.cull = mirror_cull(face.cull, axis);
    }
    cx.doc.revision = cuboid_model::Rev::next();
    if let Some(mut guard) = ElementMut::new(cx, id) {
        guard.default_uv(false);
    }
    true
}

/// Lines every object up with the first one along `axis`: the chosen edge of each box is moved
/// onto the same edge of the reference box (rotation is ignored). Returns how many objects
/// moved.
///
/// spec: tools 10.12 (Center on X, Y, Z)
pub fn align(cx: &mut EditCtx<'_>, objs: &[ObjRef], axis: Axis, edge: AlignEdge) -> usize {
    let pick = |(lo, hi): (f32, f32)| match edge {
        AlignEdge::Min => lo,
        AlignEdge::Center => (lo + hi) * 0.5,
        AlignEdge::Max => hi,
    };
    let Some(reference) = objs
        .first()
        .and_then(|&o| extent(cx.doc, o, axis))
        .map(pick)
    else {
        return 0;
    };
    let mut moved = 0;
    for &obj in objs.iter().skip(1) {
        let (Some(own), Some(pivot)) = (extent(cx.doc, obj, axis), pivot_of(cx.doc, obj)) else {
            continue;
        };
        let shift = reference - pick(own);
        if engine_eq(Vec3::splat(shift), Vec3::ZERO) {
            continue;
        }
        let target = pivot + axis.unit() * shift;
        moved += usize::from(changed(cx, |cx| set_origin_move(cx, obj, target, false)));
    }
    moved
}
