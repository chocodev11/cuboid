//! Snapshot shapes and change categories (spec: tools 2.5, 15.2).

use std::sync::Arc;

use bitflags::bitflags;
pub use cuboid_voxel::VoxelSnap;

use crate::animation::Animation;
use crate::contract::{DisplaySlots, ModelProps, ViewProps};
use crate::groups::Group;
use crate::ids::ObjRef;
use crate::list::ListSnap;
use crate::objects::{Blueprint, Element, Mesh, TextureList};

bitflags! {
    /// The change categories of an undo step: which parts of the document an entry snapshots.
    /// The values are the original's; the content of each category is tools 15.2.
    ///
    /// The item overrides are captured by no category, so no undo step restores them (QUIRK).
    ///
    /// spec: tools 2.5, tools 15.2, arch 9.2
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub struct Categories: u16 {
        /// Model properties including the model box and origin, excluding the display slots.
        const MODEL_PROPERTIES = 1;
        /// Grid settings and cameras.
        const VIEW_PROPERTIES = 2;
        /// The blueprint and all elements, or only the selected ones.
        const ELEMENTS = 4;
        /// The texture list.
        const TEXTURES = 8;
        /// The whole voxel world.
        const VOXELS = 16;
        /// All meshes, or only the selected ones.
        const MESHES = 32;
        /// The eight display slots.
        const MODEL_DISPLAY = 64;
        /// The groups and the order of the children (hierarchy A1). The parent links live in
        /// the groups, so this is the whole outliner tree.
        const HIERARCHY = 128;
        /// The animations and their keyframes (animation A2).
        const ANIMATION = 256;

        /// 40.
        const TEXTURES_AND_MESHES = Self::TEXTURES.bits() | Self::MESHES.bits();
        /// 12.
        const ELEMENTS_AND_TEXTURES = Self::ELEMENTS.bits() | Self::TEXTURES.bits();
        /// 20.
        const ELEMENTS_AND_VOXELS = Self::ELEMENTS.bits() | Self::VOXELS.bits();
        /// 36: elements and meshes.
        const OBJECTS = Self::ELEMENTS.bits() | Self::MESHES.bits();
        /// 52.
        const OBJECTS_AND_VOXELS = Self::OBJECTS.bits() | Self::VOXELS.bits();
        /// 511.
        const ALL = 511;
    }
}

/// The blueprint and the elements of a snapshot, with the selection state they were captured
/// with.
///
/// spec: tools 15.2, native 5.2
#[derive(Clone, Debug)]
pub struct ElementsSnap {
    /// The blueprint record.
    pub blueprint: Blueprint,
    /// The elements in list order.
    pub list: ListSnap<Element>,
    /// Parallel to `list`: the element was selected (`is`), so a restore selects it again.
    pub selected: Vec<bool>,
}

/// The meshes of a snapshot, with the selection state they were captured with.
///
/// spec: tools 15.2, native 4.6
#[derive(Clone, Debug)]
pub struct MeshesSnap {
    /// The meshes in list order.
    pub list: ListSnap<Mesh>,
    /// Parallel to `list`: the mesh was selected (`is`).
    pub selected: Vec<bool>,
}

/// The groups of a snapshot with the order of the top level, and the selection they were
/// captured with. A group lists its own children, so the links between the groups travel with
/// the records.
///
/// A command that only changes the tree (group, ungroup, reparent) snapshots no element list, so
/// the whole selection travels here: a restore puts it back.
///
/// spec: hierarchy A1 3
#[derive(Clone, Debug)]
pub struct GroupsSnap {
    /// The groups in list order.
    pub list: ListSnap<Group>,
    /// The top level in outliner order (see `Groups::roots`).
    pub roots: Vec<ObjRef>,
    /// The selection in selection order, the first object being the primary. A file has none.
    pub selection: Vec<ObjRef>,
}

/// A copy of the document parts named by `categories`, in the document's record shape: undo
/// entries, clipboards and loaded files all use it, so a restore or paste yields what a record
/// round trip yields. Parts are pointer copies; memory grows only with what changes afterwards.
///
/// spec: tools 15.2, arch 6 (cuboid-model), arch 9.1
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Which of the parts below are present.
    pub categories: Categories,
    /// Model properties.
    pub props: Option<Arc<ModelProps>>,
    /// Grid steps and cameras.
    pub view: Option<Arc<ViewProps>>,
    /// Blueprint and elements.
    pub elements: Option<ElementsSnap>,
    /// The texture list.
    pub textures: Option<Arc<TextureList>>,
    /// The voxel world: the outer `Option` says whether the category is present, the inner one
    /// whether a world exists (an absent world resets an existing one on restore).
    pub voxels: Option<Option<VoxelSnap>>,
    /// The meshes.
    pub meshes: Option<MeshesSnap>,
    /// The display slots.
    pub display: Option<Arc<DisplaySlots>>,
    /// The groups and the order of the top level.
    pub groups: Option<GroupsSnap>,
    /// The animations.
    pub animations: Option<ListSnap<Animation>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_values_follow_the_table_of_tools_2_5() {
        assert_eq!(Categories::MODEL_PROPERTIES.bits(), 1);
        assert_eq!(Categories::VIEW_PROPERTIES.bits(), 2);
        assert_eq!(Categories::ELEMENTS.bits(), 4);
        assert_eq!(Categories::TEXTURES.bits(), 8);
        assert_eq!(Categories::VOXELS.bits(), 16);
        assert_eq!(Categories::MESHES.bits(), 32);
        assert_eq!(Categories::MODEL_DISPLAY.bits(), 64);
        assert_eq!(Categories::HIERARCHY.bits(), 128);
        assert_eq!(Categories::ANIMATION.bits(), 256);
        assert_eq!(Categories::TEXTURES_AND_MESHES.bits(), 40);
        assert_eq!(Categories::ELEMENTS_AND_TEXTURES.bits(), 12);
        assert_eq!(Categories::ELEMENTS_AND_VOXELS.bits(), 20);
        assert_eq!(Categories::OBJECTS.bits(), 36);
        assert_eq!(Categories::OBJECTS_AND_VOXELS.bits(), 52);
        assert_eq!(Categories::ALL.bits(), 511);
    }
}
