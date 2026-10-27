//! UV operations on the faces of elements: automatic mapping, rotate, flip, a numeric rectangle,
//! a common move and UV snap (spec: tools 4.5, 4.7, 10.17).
//!
//! Every function takes the faces to change (empty means all six) and returns how many elements
//! changed; an element whose faces would stay the same is not touched, so the revision stays.

use cuboid_model::uv::default_uv_rects;
use cuboid_model::{ChangeFlags, EditCtx, ElementFlags, FaceName, ObjRef, uv::UvRect};
use glam::Vec2;

use crate::elements::{edit_elements, wanted};

/// How an automatic mapping lays the faces out.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum AutoUvStyle {
    /// Box style: the faces take the part of the texture their place and size in the block cover,
    /// the mapping UV snap keeps up to date (tools 4.7). UV snap stays as it is.
    #[default]
    Box,
    /// Per face: every face covers its whole texture, its rotation kept (tools 10.17, "Full").
    /// UV snap goes off, or the next move would map the face again.
    PerFace,
}

/// The axis of a flip.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum FlipAxis {
    /// Left to right: the U components swap.
    Horizontal,
    /// Top to bottom: the V components swap.
    Vertical,
}

/// Which of the two UV snap options a change sets.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum SnapOption {
    /// The mapping follows position and size.
    Snap,
    /// The mirrored variant of the mapping.
    Mirror,
}

/// What a UV edit marks in the journal.
const TOUCHED: ChangeFlags = ChangeFlags::UV.union(ChangeFlags::FACES);

/// Re-derives the UV mapping of the faces in the given style.
///
/// spec: tools 4.7 ("Apply Default UV"), tools 10.17 ("Full")
pub fn auto_uv(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    faces: &[FaceName],
    style: AutoUvStyle,
) -> usize {
    edit_elements(cx, objs, TOUCHED, |el| {
        let rects = default_uv_rects(
            el.base.p,
            el.base.s,
            el.flags.contains(ElementFlags::UV_SNAP_MIRROR),
        );
        for face in el.faces.iter_mut().filter(|f| wanted(faces, f.name)) {
            match style {
                AutoUvStyle::Box => face.uv = rects[face.name as usize],
                AutoUvStyle::PerFace => face.uv.set(Vec2::ZERO, Vec2::ONE),
            }
        }
        if style == AutoUvStyle::PerFace {
            el.flags.remove(ElementFlags::UV_SNAP);
        }
    })
}

/// Adds `degrees` to the rotation of the face UVs; a multiple of 90, negative turns back. UV
/// snap stays as it is.
///
/// spec: tools 10.17 ("Rotate 90")
pub fn rotate(cx: &mut EditCtx<'_>, objs: &[ObjRef], faces: &[FaceName], degrees: i32) -> usize {
    edit_elements(cx, objs, TOUCHED, |el| {
        for face in el.faces.iter_mut().filter(|f| wanted(faces, f.name)) {
            face.uv.set_rot(face.uv.rot().saturating_add(degrees));
        }
    })
}

/// Swaps the U (or V) components of the corners of the face UVs. UV snap stays as it is.
///
/// spec: tools 10.17 ("Flip H", "Flip V")
pub fn flip(cx: &mut EditCtx<'_>, objs: &[ObjRef], faces: &[FaceName], axis: FlipAxis) -> usize {
    edit_elements(cx, objs, TOUCHED, |el| {
        for face in el.faces.iter_mut().filter(|f| wanted(faces, f.name)) {
            let (tl, br) = (face.uv.tl_raw(), face.uv.br_raw());
            match axis {
                FlipAxis::Horizontal => face.uv.set(Vec2::new(br.x, tl.y), Vec2::new(tl.x, br.y)),
                FlipAxis::Vertical => face.uv.set(Vec2::new(tl.x, br.y), Vec2::new(br.x, tl.y)),
            }
        }
    })
}

/// Sets the UV rectangle of the faces: corners clamped to `[-1, 2]`, and the rotation when
/// given. UV snap goes off, as with every write of the UV view.
///
/// spec: tools 4.5, tools 4.7
pub fn set_rect(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    faces: &[FaceName],
    tl: Vec2,
    br: Vec2,
    rotation: Option<i32>,
) -> usize {
    edit_elements(cx, objs, TOUCHED, |el| {
        for face in el.faces.iter_mut().filter(|f| wanted(faces, f.name)) {
            face.uv = UvRect::new(rotation.unwrap_or(face.uv.rot()), tl, br);
        }
        el.flags.remove(ElementFlags::UV_SNAP);
    })
}

/// Moves the UV rectangles of the faces by `delta` (V up), all by the same offset. The offset is
/// cut down to what keeps every moved face on the texture, so none is folded or shrunk; each
/// keeps its size, its flips and its rotation. UV snap goes off, as with every write of the UV
/// view; an offset that comes to nothing touches nothing.
///
/// spec: uv editor v2 5
pub fn move_faces(cx: &mut EditCtx<'_>, objs: &[ObjRef], faces: &[FaceName], delta: Vec2) -> usize {
    let (mut lowest, mut highest) = (Vec2::NEG_INFINITY, Vec2::INFINITY);
    for &obj in objs {
        let ObjRef::Element(id) = obj else {
            continue;
        };
        let Some(el) = cx.doc.elements.get(id) else {
            continue;
        };
        for face in el.faces.iter().filter(|f| wanted(faces, f.name)) {
            let (a, b) = (face.uv.tl(), face.uv.br());
            lowest = lowest.max(-a.min(b));
            highest = highest.min(Vec2::ONE - a.max(b));
        }
    }
    let delta = delta.max(lowest).min(highest);
    if delta == Vec2::ZERO {
        return 0;
    }
    edit_elements(cx, objs, TOUCHED, |el| {
        for face in el.faces.iter_mut().filter(|f| wanted(faces, f.name)) {
            face.uv = UvRect::new(face.uv.rot(), face.uv.tl() + delta, face.uv.br() + delta);
        }
        el.flags.remove(ElementFlags::UV_SNAP);
    })
}

/// Turns UV snap (or its mirrored variant) on or off. While snap is on the faces take the
/// mapping of the element's place and size right away.
///
/// spec: tools 4.7
pub fn set_snap(cx: &mut EditCtx<'_>, objs: &[ObjRef], option: SnapOption, on: bool) -> usize {
    let flag = match option {
        SnapOption::Snap => ElementFlags::UV_SNAP,
        SnapOption::Mirror => ElementFlags::UV_SNAP_MIRROR,
    };
    edit_elements(cx, objs, TOUCHED, |el| {
        el.flags.set(flag, on);
        if el.flags.contains(ElementFlags::UV_SNAP) {
            let rects = default_uv_rects(
                el.base.p,
                el.base.s,
                el.flags.contains(ElementFlags::UV_SNAP_MIRROR),
            );
            for (face, rect) in el.faces.iter_mut().zip(rects) {
                face.uv = rect;
            }
        }
    })
}

#[cfg(test)]
mod tests;
