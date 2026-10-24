//! Mesh creation and the geometry of primitive meshes (spec: tools 6.1, 6.2, 6.5). The mesh tools
//! and the mesh texture and colour commands (tools 6.3, 10.14) are not built yet.

use std::sync::Arc;

use cuboid_image::{Image, RgbaBuf};
use cuboid_model::{
    ChangeFlags, DocFlags, Document, EditCtx, Label, MIN_SIZE, Mesh, MeshBounds, MeshKind, ObjId,
    ObjRef, ObjectBase, Perms, Rev, Texture,
};
use cuboid_num::pack::to_byte;
use glam::Vec3;

use crate::primitives::shapes;

/// Side of the square paint texture of a primitive, in texels.
const PAINT_SIZE: u32 = 128;

/// Creates a primitive mesh of `kind` at the blueprint's position and size (its rotation is
/// ignored), with generated geometry and a paint texture filled with `color`, and selects it
/// exclusively. Returns `None` for [`MeshKind::Custom`], which has no generator.
///
/// spec: tools 6.1, tools 6.2
pub fn add_primitive(cx: &mut EditCtx<'_>, kind: MeshKind, color: [f32; 4]) -> Option<ObjId> {
    let geometry = shapes::geometry(kind)?;
    let label = shapes::label(kind);
    let blueprint = &cx.doc.blueprint.base;
    let (p, mut s) = (blueprint.p, blueprint.s);
    if kind == MeshKind::Sprite {
        s.y = MIN_SIZE;
    }
    let offset = match kind {
        MeshKind::Sphere | MeshKind::Torus => Vec3::new(s.x / 2.0, s.y / 2.0, -s.z / 2.0),
        MeshKind::Cylinder | MeshKind::Cone => Vec3::new(s.x / 2.0, 0.0, -s.z / 2.0),
        _ => Vec3::ZERO,
    };
    let mut base = ObjectBase::new(label, p, s, Perms::PRIMITIVE);
    base.o = p + offset;

    let texel = color.map(to_byte);
    let mut paint = RgbaBuf::new(PAINT_SIZE, PAINT_SIZE);
    for chunk in paint.pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&texel);
    }
    let mut texture = Texture::new(Arc::new(Image::from_rgba(paint)));
    texture.name = Label::from(label);

    let id = cx.doc.meshes.push(Mesh {
        base,
        kind,
        geometry,
        texture: Some(texture),
        submeshes: Arc::from([]),
        bounds: MeshBounds::default(),
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    });
    cx.doc.journal.mark_object(id, ChangeFlags::all());
    cx.doc
        .journal
        .mark_doc(DocFlags::LISTS | DocFlags::SELECTION | DocFlags::HIGHLIGHT);
    cx.doc.revision = Rev::next();
    cx.sel.clear();
    cx.sel.add(ObjRef::Mesh(id));
    Some(id)
}

/// Fills the geometry of every primitive mesh that has none, which is how a `.cubik` file
/// delivers them. Meshes that have geometry, and imported meshes, are not touched. Returns how
/// many meshes were filled.
///
/// spec: tools 6.5
pub fn ensure_geometry(doc: &mut Document) -> usize {
    let empty: Vec<_> = doc
        .meshes
        .ids()
        .iter()
        .zip(doc.meshes.items())
        .filter(|(_, mesh)| mesh.kind != MeshKind::Custom && mesh.geometry.vertices.is_empty())
        .map(|(&id, _)| id)
        .collect();
    let mut filled = 0;
    for id in empty {
        let Some(mesh) = doc.meshes.edit(id, &mut doc.journal) else {
            continue;
        };
        if let Some(geometry) = shapes::geometry(mesh.kind) {
            mesh.geometry = geometry;
            filled += 1;
        }
    }
    if filled > 0 {
        doc.revision = Rev::next();
    }
    filled
}

#[cfg(test)]
mod tests;
