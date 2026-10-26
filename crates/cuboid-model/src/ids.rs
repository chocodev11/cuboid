//! Session-unique `ObjId`, revision stamps `Rev` and the `ObjRef` handle (spec: tools 2.1; arch
//! 8.2).

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

/// Revision stamps come from `cuboid-image`, whose `Image` carries one and which sits below this
/// crate; they are the same counter for images, objects and documents.
///
/// spec: arch 6 (`Rev`), arch 8.2
pub use cuboid_image::Rev;

/// Identity of an element or a mesh. Session-unique and never persisted. A record keeps its id
/// through undo, redo and restore, so renderer caches and list rows stay keyed; paste, clone and
/// import allocate fresh ids.
///
/// spec: arch 8.2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ObjId(NonZeroU64);

impl ObjId {
    /// Allocates an id that no earlier call returned.
    pub fn next() -> ObjId {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        ObjId(NonZeroU64::MIN.saturating_add(COUNTER.fetch_add(1, Ordering::Relaxed)))
    }

    /// The raw value, for logs and hashing.
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// What the selection, hover, highlight and the gizmos refer to: an element, a mesh, a group, or
/// one of the two scene objects that exist once per document.
///
/// spec: arch 8.2, tools 2.3, tools 8.1
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ObjRef {
    /// An element of the document.
    Element(ObjId),
    /// A mesh (primitive or imported) of the document.
    Mesh(ObjId),
    /// A group of the document (hierarchy A1).
    Group(ObjId),
    /// The placement box.
    Blueprint,
    /// The model-origin marker; its position is `g.o`.
    ModelOrigin,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_ids_are_unique_and_never_zero() {
        let a = ObjId::next();
        let b = ObjId::next();
        assert_ne!(a, b);
        assert_ne!(a.get(), 0);
        assert_ne!(b.get(), 0);
    }

    #[test]
    fn revision_stamps_increase_and_never_equal_none() {
        let a = Rev::next();
        let b = Rev::next();
        assert!(b > a);
        assert!(a > Rev::NONE);
    }
}
