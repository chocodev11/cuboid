//! In-process clipboards: objects, voxels, face properties, UV and mesh textures (spec: tools 11;
//! app 14).

use std::sync::Arc;

use cuboid_model::image::Image;
use cuboid_model::{Face, Snapshot, UvRect};
use glam::{IVec3, Vec3};

/// What the Edit clipboard holds; it decides the label of the Paste row.
///
/// spec: tools 11.1, menus 1.7
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ClipboardKind {
    /// Nothing copied: "Paste", disabled.
    #[default]
    Empty,
    /// Objects copied: "Paste objects".
    Objects,
    /// Voxels copied: "Paste voxels".
    Voxels,
}

/// Copied objects: a snapshot of the selected elements and meshes plus the blueprint, in the
/// shape of an undo snapshot, with the copy anchor. Paste always deep-copies.
///
/// spec: tools 11.1, tools 11.3, tools 11.4
#[derive(Clone, Debug)]
pub struct ObjectClipboard {
    /// The selection-only snapshot (category objects).
    pub snapshot: Snapshot,
    /// The primary's position at copy time.
    pub anchor: Vec3,
}

/// All clipboards of the session. They live in memory only, never on the OS clipboard, and are
/// never saved.
///
/// Copy and cut clear the Edit clipboard first (objects and voxels exclude each other); the face,
/// UV, mesh texture and mesh colour clipboards are separate. New model clears the ones the specs
/// name.
///
/// spec: arch 9.3, tools 11, tools 5.5, tools 10.14, app 14
#[derive(Clone, Debug, Default)]
pub struct Clipboards {
    /// The object clipboard.
    pub objects: Option<ObjectClipboard>,
    /// The voxel clipboard: non-zero cells as `(dx, dy, dz)` relative to the copy box minimum
    /// with their values.
    pub voxels: Option<Vec<(IVec3, u32)>>,
    /// The six faces of the primary element, for the face-property pastes (faces, textures,
    /// UV, culling).
    pub faces: Option<[Face; 6]>,
    /// The shading flag of the primary element.
    pub shade: Option<bool>,
    /// The UV rectangle copied in the UV tab.
    pub uv: Option<UvRect>,
    /// The main texture copied from a mesh.
    pub mesh_texture: Option<Arc<Image>>,
    /// The material colour copied from a mesh, RGBA in 0..=1.
    pub mesh_color: Option<[f32; 4]>,
}

impl Clipboards {
    /// What the Edit clipboard holds.
    pub fn kind(&self) -> ClipboardKind {
        if self.objects.is_some() {
            ClipboardKind::Objects
        } else if self.voxels.is_some() {
            ClipboardKind::Voxels
        } else {
            ClipboardKind::Empty
        }
    }
}

#[cfg(test)]
mod tests {
    use cuboid_model::Categories;

    use super::*;

    fn empty_snapshot() -> Snapshot {
        Snapshot {
            categories: Categories::empty(),
            props: None,
            view: None,
            elements: None,
            textures: None,
            voxels: None,
            meshes: None,
            display: None,
            groups: None,
            animations: None,
        }
    }

    #[test]
    fn the_paste_row_follows_what_was_copied() {
        let mut clip = Clipboards::default();
        assert_eq!(clip.kind(), ClipboardKind::Empty);
        clip.voxels = Some(vec![(IVec3::ZERO, 1)]);
        assert_eq!(clip.kind(), ClipboardKind::Voxels);
        clip.voxels = None;
        clip.objects = Some(ObjectClipboard {
            snapshot: empty_snapshot(),
            anchor: Vec3::ZERO,
        });
        assert_eq!(clip.kind(), ClipboardKind::Objects);
    }
}
