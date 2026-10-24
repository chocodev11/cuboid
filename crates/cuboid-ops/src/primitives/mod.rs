//! Creation of the cuboid and plane primitives at the blueprint (spec: tools 4.2, 10.1). The
//! primitive mesh generators are in [`shapes`]; the creation of the meshes is in `mesh`.

pub mod shapes;

use cuboid_model::{ChangeFlags, DocFlags, EditCtx, Element, ElementType, ObjId, ObjRef, Rev};
use cuboid_num::pack::{from_byte, to_byte};

/// Creates an element of `kind` at the blueprint's position and size, with a random light
/// colour and the default face texture, and selects it exclusively. The element starts with UV
/// snap on; its default UV mapping is applied when the caller runs the start step.
///
/// spec: tools 4.2, tools 10.1
pub fn add_element(cx: &mut EditCtx<'_>, kind: ElementType) -> ObjId {
    // Colours are kept at byte precision, the precision of the records an undo or a save writes.
    let mut channel = || from_byte(to_byte(cx.rng.range_f32(0.6, 1.0)));
    let color = [channel(), channel(), channel(), 1.0];
    let texture = cx.doc.textures.default_face_label();
    let element = Element::new(
        kind,
        &cx.doc.blueprint,
        cx.doc.elements.next_display_id(),
        color,
        &texture,
    );
    let id = cx.doc.elements.push(element);
    cx.doc.journal.mark_object(id, ChangeFlags::all());
    cx.doc
        .journal
        .mark_doc(DocFlags::LISTS | DocFlags::SELECTION | DocFlags::HIGHLIGHT);
    cx.doc.revision = Rev::next();
    cx.sel.clear();
    cx.sel.add(ObjRef::Element(id));
    id
}

#[cfg(test)]
mod tests {
    use cuboid_model::{Document, EditEnv, RenderPrefs};
    use cuboid_model::{FaceFlags, MIN_SIZE, Selection, TextureList, check_invariants};
    use cuboid_num::EngineRng;
    use glam::Vec3;

    use super::*;

    #[test]
    fn a_plane_has_two_faces_and_the_minimum_thickness() {
        let mut doc = Document::new(TextureList::default());
        let mut sel = Selection::default();
        let env = EditEnv {
            restrict_rotations: true,
            render: RenderPrefs::default(),
            code_page: 1252,
            allow_negative_size: false,
        };
        let mut rng = EngineRng::seeded(7);
        let mut cx = EditCtx {
            doc: &mut doc,
            sel: &mut sel,
            env: &env,
            rng: &mut rng,
        };
        let id = add_element(&mut cx, ElementType::PlaneY);
        let el = doc.elements.get(id).expect("created");
        assert_eq!(el.base.p, Vec3::new(-2.0, 0.0, 2.0));
        assert!((el.base.s.y - MIN_SIZE).abs() < 1e-9);
        let active: Vec<_> = el.faces.iter().filter(|f| f.active).collect();
        assert_eq!(active.len(), 2);
        assert!(el.faces.iter().all(|f| {
            f.active
                == FaceFlags::from_bits_truncate(3)
                    .intersects(FaceFlags::from_bits_truncate(1 << (f.name as u8)))
        }));
        assert!(el.color[..3].iter().all(|c| (0.6..=1.0).contains(c)));
        assert_eq!(sel.primary, Some(ObjRef::Element(id)));
        check_invariants(&doc, &sel).expect("valid document");
    }
}
