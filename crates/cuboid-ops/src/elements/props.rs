//! Colour, shading and the per-face switches of elements: enabled, tint index and cull face.

use cuboid_model::{
    ChangeFlags, EditCtx, Element, ElementFlags, Face, FaceFlags, FaceName, ObjRef, Rev,
};
use cuboid_num::pack;

/// The darkest a channel of an element colour gets from "Set color".
const MIN_CHANNEL: f32 = 0.25;

/// Whether `name` is among the faces a call names; no faces means all six.
pub(crate) fn wanted(faces: &[FaceName], name: FaceName) -> bool {
    faces.is_empty() || faces.contains(&name)
}

/// Runs `edit` on a copy of every element among `objs` and stores the copy, marked `flags` in the
/// journal, when it differs. Returns how many elements changed.
pub(crate) fn edit_elements(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    flags: ChangeFlags,
    mut edit: impl FnMut(&mut Element),
) -> usize {
    let mut changed = 0;
    for &obj in objs {
        let ObjRef::Element(id) = obj else {
            continue;
        };
        let Some(current) = cx.doc.elements.get(id) else {
            continue;
        };
        let mut next = current.clone();
        edit(&mut next);
        if next == *current {
            continue;
        }
        let doc = &mut *cx.doc;
        if let Some(slot) = doc.elements.edit_marked(id, flags, &mut doc.journal) {
            *slot = next;
            changed += 1;
        }
    }
    if changed > 0 {
        cx.doc.revision = Rev::next();
    }
    changed
}

fn edit_faces(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    faces: &[FaceName],
    mut edit: impl FnMut(&mut Face),
) -> usize {
    edit_elements(cx, objs, ChangeFlags::FACES, |el| {
        el.faces
            .iter_mut()
            .filter(|f| wanted(faces, f.name))
            .for_each(&mut edit);
    })
}

/// Gives every element the colour `rgb`, each channel at least 0.25. The alpha stays opaque, and
/// the stored colour is the one a record holds (bytes), so undo, redo and a save give it back
/// unchanged. Returns how many elements changed.
///
/// spec: tools 10.5 ("Set color"), native 4.10
pub fn set_color(cx: &mut EditCtx<'_>, objs: &[ObjRef], rgb: [f32; 3]) -> usize {
    let [r, g, b] = rgb.map(|c| c.clamp(MIN_CHANNEL, 1.0));
    let color = pack::unpack_element(pack::element([r, g, b, 1.0]));
    edit_elements(cx, objs, ChangeFlags::COLOR, |el| el.color = color)
}

/// Turns the directional shading of the elements on or off. Returns how many changed.
///
/// spec: tools 10.8 ("Shading")
pub fn set_shade(cx: &mut EditCtx<'_>, objs: &[ObjRef], on: bool) -> usize {
    edit_elements(cx, objs, ChangeFlags::COLOR, |el| {
        el.flags.set(ElementFlags::SHADE, on);
    })
}

/// Switches the faces (all six when `faces` is empty) on or off. Returns how many elements
/// changed.
///
/// spec: tools 10.8 ("Faces")
pub fn set_face_active(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    faces: &[FaceName],
    on: bool,
) -> usize {
    edit_faces(cx, objs, faces, |face| face.active = on)
}

/// Sets the tint index of the faces, -1 for none. Returns how many elements changed.
///
/// spec: tools 4.4, tools 10.8 ("Tint")
pub fn set_face_tint(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    faces: &[FaceName],
    tint: i32,
) -> usize {
    edit_faces(cx, objs, faces, |face| face.tint = tint)
}

/// Sets the cull direction of the faces: one neighbour, or none. Returns how many elements
/// changed.
///
/// spec: tools 4.4, tools 10.8 ("Culling")
pub fn set_face_cull(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    faces: &[FaceName],
    cull: Option<FaceName>,
) -> usize {
    let flags = cull.map_or(FaceFlags::empty(), |name| {
        FaceFlags::from_bits_retain(1 << (name as u8))
    });
    edit_faces(cx, objs, faces, |face| face.cull = flags)
}
