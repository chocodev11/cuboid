//! Selection operations shared by commands, clicks and box selection.

use cuboid_model::{DocFlags, Document, ObjRef, Selection};

use crate::commands::SelectOp;

/// Every visible element, then every visible mesh.
pub(crate) fn visible_objects(doc: &Document) -> Vec<ObjRef> {
    let elements = doc
        .elements
        .ids()
        .iter()
        .zip(doc.elements.items())
        .filter(|(_, el)| !el.base.hidden)
        .map(|(&id, _)| ObjRef::Element(id));
    let meshes = doc
        .meshes
        .ids()
        .iter()
        .zip(doc.meshes.items())
        .filter(|(_, mesh)| !mesh.base.hidden)
        .map(|(&id, _)| ObjRef::Mesh(id));
    elements.chain(meshes).collect()
}

/// The selection as a list, in selection order.
pub(crate) fn list(sel: &Selection) -> Vec<ObjRef> {
    sel.list.iter().copied().collect()
}

/// Makes `objs` the selection, the first becoming the primary.
pub(crate) fn set(doc: &mut Document, sel: &mut Selection, objs: &[ObjRef]) {
    if sel.list.iter().eq(objs.iter()) {
        return;
    }
    sel.clear();
    for &obj in objs {
        sel.add(obj);
    }
    doc.journal
        .mark_doc(DocFlags::SELECTION | DocFlags::HIGHLIGHT);
}

/// Combines `objs` with the selection.
pub(crate) fn apply(doc: &mut Document, sel: &mut Selection, objs: &[ObjRef], op: SelectOp) {
    match op {
        SelectOp::Replace => set(doc, sel, objs),
        SelectOp::Add => {
            for &obj in objs {
                sel.add(obj);
            }
        }
        SelectOp::Toggle => {
            for &obj in objs {
                sel.toggle(obj);
            }
        }
        SelectOp::Remove => {
            for &obj in objs {
                sel.remove(obj);
            }
        }
    }
    if op != SelectOp::Replace {
        doc.journal
            .mark_doc(DocFlags::SELECTION | DocFlags::HIGHLIGHT);
    }
}

/// Selects the visible elements and meshes.
pub(crate) fn select_all(doc: &mut Document, sel: &mut Selection) {
    let all = visible_objects(doc);
    set(doc, sel, &all);
}

/// Selects the visible elements and meshes that are not selected.
pub(crate) fn select_inverse(doc: &mut Document, sel: &mut Selection) {
    let rest: Vec<ObjRef> = visible_objects(doc)
        .into_iter()
        .filter(|obj| !sel.contains(*obj))
        .collect();
    set(doc, sel, &rest);
}

/// Removes selected objects the document no longer has.
pub(crate) fn prune(doc: &Document, sel: &mut Selection) {
    let gone: Vec<ObjRef> = sel
        .list
        .iter()
        .copied()
        .filter(|obj| match *obj {
            ObjRef::Element(id) => doc.elements.get(id).is_none(),
            ObjRef::Mesh(id) => doc.meshes.get(id).is_none(),
            ObjRef::Blueprint | ObjRef::ModelOrigin => false,
        })
        .collect();
    for obj in gone {
        sel.remove(obj);
    }
}
