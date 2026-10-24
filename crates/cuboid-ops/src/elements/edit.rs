//! Delete, rename, hide and lock.

use cuboid_model::{ChangeFlags, DocFlags, EditCtx, Label, ObjRef, ObjectBase, Perms, Rev};
use rustc_hash::FxHashSet;

/// The base of an element or mesh for writing; the blueprint and the model origin are not
/// listed objects and none of these operations apply to them.
fn listed_mut<'a>(
    cx: &'a mut EditCtx<'_>,
    obj: ObjRef,
    flags: ChangeFlags,
) -> Option<&'a mut ObjectBase> {
    let doc = &mut *cx.doc;
    match obj {
        ObjRef::Element(id) => doc
            .elements
            .edit_marked(id, flags, &mut doc.journal)
            .map(|e| &mut e.base),
        ObjRef::Mesh(id) => doc
            .meshes
            .edit_marked(id, flags, &mut doc.journal)
            .map(|m| &mut m.base),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

fn listed<'a>(cx: &'a EditCtx<'_>, obj: ObjRef) -> Option<&'a ObjectBase> {
    match obj {
        ObjRef::Element(id) => cx.doc.elements.get(id).map(|e| &e.base),
        ObjRef::Mesh(id) => cx.doc.meshes.get(id).map(|m| &m.base),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

/// Removes the deletable elements and meshes among `objs` and deselects them. The blueprint and
/// the model origin are skipped. Returns how many objects were removed.
///
/// spec: tools 10.3
pub fn delete(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> usize {
    let mut elements = FxHashSet::default();
    let mut meshes = FxHashSet::default();
    for &obj in objs {
        if !listed(cx, obj).is_some_and(|b| b.perms.contains(Perms::DELETABLE)) {
            continue;
        }
        match obj {
            ObjRef::Element(id) => elements.insert(id),
            ObjRef::Mesh(id) => meshes.insert(id),
            ObjRef::Blueprint | ObjRef::ModelOrigin => continue,
        };
    }
    let removed = elements.len() + meshes.len();
    if removed == 0 {
        return 0;
    }
    cx.doc.elements.remove_set(&elements);
    cx.doc.meshes.remove_set(&meshes);
    for id in elements {
        cx.sel.remove(ObjRef::Element(id));
    }
    for id in meshes {
        cx.sel.remove(ObjRef::Mesh(id));
    }
    cx.doc
        .journal
        .mark_doc(DocFlags::LISTS | DocFlags::SELECTION | DocFlags::HIGHLIGHT);
    cx.doc.revision = Rev::next();
    removed
}

/// Gives every listed object among `objs` the same label; double quotes are dropped. Returns
/// how many labels changed.
///
/// spec: tools 10.4
pub fn rename(cx: &mut EditCtx<'_>, objs: &[ObjRef], name: &str) -> usize {
    let label = Label::from(name.replace('"', ""));
    let mut changed = 0;
    for &obj in objs {
        if listed(cx, obj).is_none_or(|b| b.label == label) {
            continue;
        }
        if let Some(base) = listed_mut(cx, obj, ChangeFlags::LABEL) {
            base.label = label.clone();
            changed += 1;
        }
    }
    if changed > 0 {
        cx.doc.revision = Rev::next();
    }
    changed
}

/// Sets the hidden flag of the listed objects among `objs`. Returns how many changed.
///
/// spec: tools 10.6
pub fn set_hidden(cx: &mut EditCtx<'_>, objs: &[ObjRef], hidden: bool) -> usize {
    set_flag(cx, objs, hidden, |b| b.hidden, |b, v| b.hidden = v)
}

/// Sets the locked flag of the listed objects among `objs`. Returns how many changed.
///
/// spec: tools 10.6
pub fn set_locked(cx: &mut EditCtx<'_>, objs: &[ObjRef], locked: bool) -> usize {
    set_flag(cx, objs, locked, |b| b.locked, |b, v| b.locked = v)
}

/// Unhides every element and mesh. Returns how many changed.
///
/// spec: tools 10.6
pub fn show_all(cx: &mut EditCtx<'_>) -> usize {
    let all = all_listed(cx);
    set_hidden(cx, &all, false)
}

/// Unlocks every element and mesh. Returns how many changed.
///
/// spec: tools 10.6
pub fn unlock_all(cx: &mut EditCtx<'_>) -> usize {
    let all = all_listed(cx);
    set_locked(cx, &all, false)
}

fn all_listed(cx: &EditCtx<'_>) -> Vec<ObjRef> {
    let elements = cx.doc.elements.ids().iter().map(|&id| ObjRef::Element(id));
    let meshes = cx.doc.meshes.ids().iter().map(|&id| ObjRef::Mesh(id));
    elements.chain(meshes).collect()
}

fn set_flag(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    value: bool,
    get: fn(&ObjectBase) -> bool,
    set: fn(&mut ObjectBase, bool),
) -> usize {
    let mut changed = 0;
    for &obj in objs {
        if listed(cx, obj).is_none_or(|b| get(b) == value) {
            continue;
        }
        if let Some(base) = listed_mut(cx, obj, ChangeFlags::VISIBILITY) {
            set(base, value);
            changed += 1;
        }
    }
    if changed > 0 {
        cx.doc.journal.mark_doc(DocFlags::LISTS);
        cx.doc.revision = Rev::next();
    }
    changed
}
