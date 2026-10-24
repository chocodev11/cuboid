//! Start queue: default UV for created and pasted elements (spec: tools 4.7; arch 9.4).

use cuboid_model::{EditCtx, ElementMut, ObjRef};

/// Elements that were created or pasted and still have to "start": they run
/// `default_uv(false)` once, so an element whose UV snap flag is set takes its UVs from its
/// position. Opened files and undo restores do not start their elements: they keep the stored
/// UVs, which keeps a `.cubik` payload as it was read.
///
/// spec: tools 4.7, arch 9.4, arch 6 (cuboid-core)
#[derive(Clone, Debug, Default)]
pub struct StartQueue {
    pending: Vec<ObjRef>,
}

impl StartQueue {
    /// Queues objects, in order.
    pub fn push(&mut self, objects: impl IntoIterator<Item = ObjRef>) {
        self.pending.extend(objects);
    }

    /// Starts every queued element that still exists and empties the queue.
    pub fn run(&mut self, cx: &mut EditCtx<'_>) {
        for obj in self.pending.drain(..) {
            if let ObjRef::Element(id) = obj
                && let Some(mut guard) = ElementMut::new(cx, id)
            {
                guard.default_uv(false);
            }
        }
    }
}
