//! Change journal and the document dirty flag (spec: tools 15.1; arch 8.6).

use bitflags::bitflags;
use rustc_hash::FxHashMap;

use crate::ids::ObjId;

bitflags! {
    /// What changed on one object since the journal was last cleared.
    ///
    /// spec: arch 8.6
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub struct ChangeFlags: u8 {
        /// Position, size, rotation or origin.
        const TRANSFORM = 1;
        /// Mesh geometry or the face mask.
        const GEOMETRY = 2;
        /// Face activity, textures, tint or cull.
        const FACES = 4;
        /// Face UV rectangles.
        const UV = 8;
        /// Element colour or mesh tint.
        const COLOR = 16;
        /// Hidden or locked.
        const VISIBILITY = 32;
        /// Position in its list.
        const ORDER = 64;
        /// Label.
        const LABEL = 128;
    }
}

bitflags! {
    /// What changed in the document outside the objects since the journal was last cleared.
    ///
    /// spec: arch 8.6
    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
    pub struct DocFlags: u16 {
        /// The texture list or a texture image.
        const TEXTURES = 1;
        /// The runtime atlas needs a re-layout.
        const ATLAS = 2;
        /// Voxel chunks are dirty; the bit set is `VoxelWorld::take_dirty`.
        const VOXEL_CHUNKS = 4;
        /// The model box.
        const MODEL_BOX = 8;
        /// The display slots.
        const DISPLAY = 16;
        /// Grid steps or cameras.
        const VIEW = 32;
        /// The selection list or the primary object.
        const SELECTION = 64;
        /// The highlight set.
        const HIGHLIGHT = 128;
        /// The membership of an object list.
        const LISTS = 256;
        /// The model properties, including the model origin `g.o`.
        const PROPS = 512;
        /// The groups: their records, their children or their order.
        const GROUPS = 1024;
        /// The animations: their records, their tracks or their order.
        const ANIMATIONS = 2048;
    }
}

/// Collects what changed, so scene sync, the list panels, the status bar and the title do work
/// only for that. The core clears it once per frame after all of them ran.
///
/// The dirty flag of the document ("unsaved changes") is not derived from the journal: it
/// follows the undo pushes literally (tools 15.1) and lives in the core.
///
/// spec: arch 6 (cuboid-model), arch 8.6
#[derive(Clone, Debug, Default)]
pub struct ChangeJournal {
    objects: FxHashMap<ObjId, ChangeFlags>,
    doc: DocFlags,
}

impl ChangeJournal {
    /// Records changes of one object.
    pub fn mark_object(&mut self, id: ObjId, flags: ChangeFlags) {
        *self.objects.entry(id).or_insert(ChangeFlags::empty()) |= flags;
    }

    /// Records changes outside the objects.
    pub fn mark_doc(&mut self, flags: DocFlags) {
        self.doc |= flags;
    }

    /// The changed objects with what changed on each; iteration order is unspecified.
    pub fn objects(&self) -> &FxHashMap<ObjId, ChangeFlags> {
        &self.objects
    }

    /// The document-level changes.
    pub fn doc(&self) -> DocFlags {
        self.doc
    }

    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.doc.is_empty()
    }

    /// Forgets all changes.
    pub fn clear(&mut self) {
        self.objects.clear();
        self.doc = DocFlags::empty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_of_one_object_accumulate() {
        let mut j = ChangeJournal::default();
        let id = ObjId::next();
        j.mark_object(id, ChangeFlags::TRANSFORM);
        j.mark_object(id, ChangeFlags::UV);
        assert_eq!(j.objects().len(), 1);
        assert_eq!(j.objects()[&id], ChangeFlags::TRANSFORM | ChangeFlags::UV);
    }

    #[test]
    fn document_marks_accumulate_and_clear_empties_the_journal() {
        let mut j = ChangeJournal::default();
        assert!(j.is_empty());
        j.mark_doc(DocFlags::MODEL_BOX);
        j.mark_doc(DocFlags::LISTS);
        assert_eq!(j.doc(), DocFlags::MODEL_BOX | DocFlags::LISTS);
        assert!(!j.is_empty());
        j.mark_object(ObjId::next(), ChangeFlags::LABEL);
        j.clear();
        assert!(j.is_empty());
        assert!(j.objects().is_empty());
        assert_eq!(j.doc(), DocFlags::empty());
    }
}
