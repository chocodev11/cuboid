//! Copy, paste and duplicate through the apply path of the model.

use cuboid_model::{
    ApplyMode, ApplySource, CaptureScope, Categories, EditCtx, ObjRef, Selection, Snapshot,
    apply_document, capture,
};
use glam::Vec3;

use crate::OpError;

/// A selection-only snapshot of `objs` (elements and meshes; the blueprint record rides along
/// as in every object snapshot). Pointer copies only.
///
/// spec: tools 11.2
pub fn copy(cx: &EditCtx<'_>, objs: &[ObjRef]) -> Snapshot {
    let mut chosen = Selection::default();
    for &obj in objs {
        if matches!(obj, ObjRef::Element(_) | ObjRef::Mesh(_)) {
            chosen.add(obj);
        }
    }
    capture(
        cx.doc,
        &chosen,
        CaptureScope {
            categories: Categories::OBJECTS,
            selection_only: true,
        },
    )
}

/// Creates new objects from a snapshot, moved by `offset`, and selects them (the old selection
/// is cleared). The blueprint stays where it is, whatever the snapshot recorded. Returns the new
/// objects in list order, elements first; they still need the start step.
///
/// spec: tools 11.3, tools 11.4
pub fn paste(
    cx: &mut EditCtx<'_>,
    snapshot: &Snapshot,
    offset: Vec3,
) -> Result<Vec<ObjRef>, OpError> {
    let mut snapshot = snapshot.clone();
    if let Some(elements) = &mut snapshot.elements {
        elements.blueprint = cx.doc.blueprint.clone();
    }
    let report = apply_document(
        cx,
        ApplySource::Snapshot(&snapshot),
        ApplyMode::Paste { offset },
    )?;
    Ok(report.created)
}

/// Copies `objs` in place and selects the copies. Returns them.
///
/// spec: tools 10.2
pub fn duplicate(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> Result<Vec<ObjRef>, OpError> {
    let snapshot = copy(cx, objs);
    paste(cx, &snapshot, Vec3::ZERO)
}
