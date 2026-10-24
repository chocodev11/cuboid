//! Retention of a loaded file (native 8.3, arch 12.3).
//!
//! The loader keeps the decompressed payload together with the position of every section and
//! record and the model-side value each of them had after loading. The saver copies the bytes
//! of every section and record whose model value is still equal and encodes only the rest. This
//! keeps what the model cannot hold byte for byte: the Euler copy of the camera quaternions,
//! mip chains and the null form of `fp` (raw texture bytes live in the images themselves),
//! unknown keys, `c` = 0 of a transparent element and everything that the read-side
//! normalisations of native 6.3 change.
//!
//! Unchanged means equal to the value the apply path leaves in the document, so the loader
//! computes that value with the same record normal form (`cuboid_model::normal_form`).

use std::sync::Arc;

use cuboid_model::voxel::ChunkSlot;
use cuboid_model::{
    Blueprint, CameraState, DisplaySlots, Element, Label, Mesh, MeshKind, ModelProps, ObjId,
    Retained, Texture, ViewProps,
};
use rustc_hash::FxHashMap;

use crate::decode::Spans;
use crate::records::CameraRecord;

/// What a loaded file keeps for a byte-identical re-save. Lives behind `Retained::data`.
#[derive(Debug, Default)]
pub struct Kept {
    /// The decompressed payload that the spans refer to.
    pub payload: Vec<u8>,
    /// The format version `v`.
    pub v: i32,
    pub spans: Spans,
    pub props: Option<ModelProps>,
    /// The view as a save writes it (`gu` and `gl` with their defaults).
    pub view: Option<ViewProps>,
    /// The four cameras with the record they were read from.
    pub cameras: [Option<(CameraState, CameraRecord)>; 4],
    pub blueprint: Option<Blueprint>,
    pub display: Option<DisplaySlots>,
    pub elements: Vec<KeptElement>,
    /// Position in `elements` by object id.
    pub element_index: FxHashMap<ObjId, usize>,
    pub meshes: Vec<KeptMesh>,
    pub mesh_index: FxHashMap<ObjId, usize>,
    pub textures: Vec<KeptTexture>,
    /// Position in `textures` by label; the last record of a repeated label.
    pub texture_index: FxHashMap<Label, usize>,
    /// The chunk table of a canonical voxel section.
    pub voxels: Option<KeptVoxels>,
    /// The `cuboid.json` entry of the file when this build cannot read it (a newer version or
    /// damaged). A save writes it back unchanged. The payload fields above are unused when
    /// `payload` is empty.
    pub entry: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct KeptElement {
    pub id: ObjId,
    /// The element as the apply path leaves it.
    pub value: Arc<Element>,
    /// The `is` flag of the record.
    pub selected: bool,
}

#[derive(Debug)]
pub struct KeptMesh {
    pub id: ObjId,
    pub value: Arc<Mesh>,
    pub selected: bool,
}

#[derive(Debug)]
pub struct KeptTexture {
    pub label: Label,
    pub value: Texture,
}

#[derive(Debug)]
pub struct KeptVoxels {
    pub res: u32,
    pub slots: Arc<[ChunkSlot]>,
}

impl Kept {
    /// The retention data of a document for the payload, `None` when it does not come from this
    /// crate or holds no payload.
    pub fn of(retained: &Retained) -> Option<&Kept> {
        retained
            .data
            .downcast_ref::<Kept>()
            .filter(|kept| !kept.payload.is_empty())
    }

    /// The `cuboid.json` entry that a save writes back unchanged, if the loader kept one.
    pub fn entry_of(retained: &Retained) -> Option<&[u8]> {
        retained
            .data
            .downcast_ref::<Kept>()
            .and_then(|kept| kept.entry.as_deref())
    }

    /// Adds the unreadable entry to the retention of a freshly loaded file, which may hold no
    /// payload when the file is above the retention cap.
    pub(crate) fn keep_entry(retained: Option<Arc<Retained>>, entry: Vec<u8>) -> Arc<Retained> {
        let mut retained = retained.unwrap_or_else(|| Arc::new(Self::default().into_retained()));
        if let Some(kept) = Arc::get_mut(&mut retained).and_then(|r| r.data.downcast_mut::<Kept>())
        {
            kept.entry = Some(entry);
        }
        retained
    }

    pub(crate) fn into_retained(self) -> Retained {
        let bytes = self.payload.len() + 1024 * (self.elements.len() + self.meshes.len());
        Retained {
            bytes,
            data: Box::new(self),
        }
    }

    /// The bytes of a span of the payload.
    pub fn bytes(&self, span: cuboid_bson::Span) -> &[u8] {
        self.payload.get(span.range()).unwrap_or_default()
    }
}

/// Whether two textures are the same image with the same metadata.
pub fn same_texture(a: &Texture, b: &Texture) -> bool {
    Arc::ptr_eq(&a.image, &b.image) && a.name == b.name && a.source == b.source
}

fn same_optional_texture(a: Option<&Texture>, b: Option<&Texture>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same_texture(a, b),
        _ => false,
    }
}

/// Whether two meshes hold the same record data. A primitive's geometry is generated from its
/// kind and never written, so only a custom mesh compares it.
pub fn same_mesh(a: &Mesh, b: &Mesh) -> bool {
    if std::ptr::eq(a, b) {
        return true;
    }
    a.base == b.base
        && a.kind == b.kind
        && (a.kind != MeshKind::Custom
            || Arc::ptr_eq(&a.geometry, &b.geometry)
            || a.geometry == b.geometry)
        && same_optional_texture(a.texture.as_ref(), b.texture.as_ref())
        && a.submeshes.len() == b.submeshes.len()
        && a.submeshes.iter().zip(b.submeshes.iter()).all(|(x, y)| {
            x.triangles == y.triangles
                && x.color.map(f32::to_bits) == y.color.map(f32::to_bits)
                && same_optional_texture(x.texture.as_ref(), y.texture.as_ref())
        })
        && a.bounds == b.bounds
        && a.tint.map(f32::to_bits) == b.tint.map(f32::to_bits)
}

/// Whether the chunk table of a world is the one that was loaded.
pub fn same_slots(a: &[ChunkSlot], b: &[ChunkSlot]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (ChunkSlot::Unallocated, ChunkSlot::Unallocated) => true,
            (ChunkSlot::Data(x), ChunkSlot::Data(y)) => Arc::ptr_eq(x, y),
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use cuboid_model::{MeshBounds, MeshGeometry, ObjectBase, Perms};
    use glam::Vec3;

    use super::*;

    fn mesh(kind: MeshKind, geometry: MeshGeometry) -> Mesh {
        Mesh {
            base: ObjectBase::new("m", Vec3::ZERO, Vec3::ONE, Perms::IMPORTED_MESH),
            kind,
            geometry: Arc::new(geometry),
            texture: None,
            submeshes: Arc::from([]),
            bounds: MeshBounds::default(),
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        }
    }

    #[test]
    fn a_primitive_that_got_generated_geometry_still_matches_its_record() {
        let generated = MeshGeometry {
            vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            triangles: vec![0, 1, 2],
            ..MeshGeometry::default()
        };
        let kept = mesh(MeshKind::Cube, MeshGeometry::default());
        let loaded = mesh(MeshKind::Cube, generated.clone());
        assert!(same_mesh(&kept, &loaded));

        // A custom mesh stores its geometry, so a difference there is an edit.
        let kept = mesh(MeshKind::Custom, MeshGeometry::default());
        let edited = mesh(MeshKind::Custom, generated);
        assert!(!same_mesh(&kept, &edited));

        // The rest of a primitive is still compared.
        let mut moved = mesh(MeshKind::Cube, MeshGeometry::default());
        moved.tint = [0.5; 4];
        assert!(!same_mesh(
            &mesh(MeshKind::Cube, MeshGeometry::default()),
            &moved
        ));
    }
}
