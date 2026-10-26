//! Undo and redo over snapshots, with a step cap, a memory cap and merging of continuous drags
//! (spec: tools 15.1, 15.3-15.4, 16.3).

use std::mem::size_of;
use std::sync::Arc;

use cuboid_model::voxel::{Chunk, ChunkSlot};
use cuboid_model::{Categories, Label, ListSnap, Snapshot};

use crate::commands::Effects;

/// The most undo entries kept.
pub const MAX_ENTRIES: usize = 256;

/// The most memory the undo entries may hold exclusively, in bytes. Snapshots share what did not
/// change with the document and with each other, so only the parts an entry owns alone count.
pub const MAX_BYTES: usize = 256 << 20;

/// What runs after the entry was restored by undo or redo: UI requests, as data.
pub type UndoCallback = Effects;

/// One undo entry: its label, the categories it snapshots, the snapshot of the state to go back
/// to, and an optional follow-up.
///
/// spec: tools 15.1
#[derive(Clone, Debug)]
pub struct Entry<S> {
    /// The label of the Edit menu row ("Undo" plus the label).
    pub label: Label,
    /// What the snapshot covers.
    pub categories: Categories,
    /// The state to restore.
    pub snap: S,
    /// Runs after a restore of this entry.
    pub callback: Option<UndoCallback>,
    /// The drag this entry was pushed for; later pushes of the same drag merge into it.
    gesture: Option<u64>,
}

/// How much memory a snapshot owns alone.
pub trait Weigh {
    /// An estimate in bytes of the parts nothing else shares.
    fn weight(&self) -> usize;
}

/// What an undo or redo restored.
#[derive(Clone, Debug)]
pub struct Applied {
    /// The label of the entry.
    pub label: Label,
    /// The follow-up of the entry.
    pub callback: Option<UndoCallback>,
}

/// Linear snapshot history. Push drops the redo entries and trims the oldest steps; undo swaps
/// the entry with a snapshot of the present state, which becomes the redo entry.
///
/// Steps pushed with the same gesture id in a row are one step: the first push holds the state
/// before the drag, later ones are skipped by [`History::continues`].
///
/// spec: tools 15.1, arch 9.2, arch 6 (cuboid-core)
#[derive(Clone, Debug)]
pub struct History<S> {
    undo: Vec<Entry<S>>,
    redo: Vec<Entry<S>>,
    max_entries: usize,
    max_bytes: usize,
}

impl<S> Default for History<S> {
    /// An empty history with the default caps.
    fn default() -> Self {
        Self::with_limits(MAX_ENTRIES, MAX_BYTES)
    }
}

impl<S> History<S> {
    /// An empty history that keeps at most `max_entries` steps and `max_bytes` of owned memory.
    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_entries: max_entries.max(1),
            max_bytes,
        }
    }

    /// The label of the entry the next undo restores.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| &*e.label)
    }

    /// The label of the entry the next redo restores.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| &*e.label)
    }

    /// The labels of the undo entries, oldest first (an undo history panel).
    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().map(|e| &*e.label)
    }

    /// The labels of the redo entries, the next redo first.
    pub fn redo_labels(&self) -> impl Iterator<Item = &str> {
        self.redo.iter().rev().map(|e| &*e.label)
    }

    /// Number of undo entries.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Number of redo entries.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Whether the newest entry was pushed for this drag, so the next push of the drag is
    /// already covered.
    pub fn continues(&self, gesture: u64) -> bool {
        self.redo.is_empty() && self.undo.last().is_some_and(|e| e.gesture == Some(gesture))
    }

    /// Forgets everything: new model and load.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Steps back: `swap` receives the entry to restore, restores it and returns a snapshot of
    /// the state it replaced, which becomes the redo entry. On `Err` nothing moves. `None` when
    /// there is nothing to undo.
    pub fn undo<E>(
        &mut self,
        swap: impl FnOnce(&Entry<S>) -> Result<S, E>,
    ) -> Option<Result<Applied, E>> {
        Self::step(&mut self.undo, &mut self.redo, swap)
    }

    /// Steps forward; the mirror of [`History::undo`]. Redo never creates new steps.
    pub fn redo<E>(
        &mut self,
        swap: impl FnOnce(&Entry<S>) -> Result<S, E>,
    ) -> Option<Result<Applied, E>> {
        Self::step(&mut self.redo, &mut self.undo, swap)
    }

    fn step<E>(
        from: &mut Vec<Entry<S>>,
        to: &mut Vec<Entry<S>>,
        swap: impl FnOnce(&Entry<S>) -> Result<S, E>,
    ) -> Option<Result<Applied, E>> {
        let entry = from.last()?;
        let current = match swap(entry) {
            Ok(current) => current,
            Err(e) => return Some(Err(e)),
        };
        let entry = from.pop()?;
        let applied = Applied {
            label: entry.label.clone(),
            callback: entry.callback.clone(),
        };
        to.push(Entry {
            snap: current,
            gesture: None,
            ..entry
        });
        Some(Ok(applied))
    }
}

impl<S: Weigh> History<S> {
    /// Records the state before a change. The redo entries are dropped and the oldest steps go
    /// when a cap is exceeded; the newest step always stays.
    pub fn push(
        &mut self,
        label: Label,
        categories: Categories,
        snap: S,
        callback: Option<UndoCallback>,
    ) {
        self.push_gesture(label, categories, snap, callback, None);
    }

    /// As [`History::push`], tagging the step with the drag it belongs to.
    pub fn push_gesture(
        &mut self,
        label: Label,
        categories: Categories,
        snap: S,
        callback: Option<UndoCallback>,
        gesture: Option<u64>,
    ) {
        self.redo.clear();
        self.undo.push(Entry {
            label,
            categories,
            snap,
            callback,
            gesture,
        });
        self.trim();
    }

    /// Changes the caps and drops the oldest steps that no longer fit.
    pub fn set_limits(&mut self, max_entries: usize, max_bytes: usize) {
        self.max_entries = max_entries.max(1);
        self.max_bytes = max_bytes;
        self.trim();
    }

    /// The memory the undo entries own alone, in bytes.
    pub fn bytes(&self) -> usize {
        self.undo.iter().map(|e| e.snap.weight()).sum()
    }

    fn trim(&mut self) {
        let excess = self.undo.len().saturating_sub(self.max_entries);
        self.undo.drain(..excess);
        let mut weights: Vec<usize> = self.undo.iter().map(|e| e.snap.weight()).collect();
        let mut total: usize = weights.iter().sum();
        let mut drop = 0;
        while total > self.max_bytes && drop + 1 < weights.len() {
            total -= weights[drop];
            weights[drop] = 0;
            drop += 1;
        }
        self.undo.drain(..drop);
    }
}

fn list_weight<T>(list: &ListSnap<T>) -> usize {
    let owned = list
        .items
        .iter()
        .filter(|item| Arc::strong_count(item) == 1)
        .count();
    owned * size_of::<T>() + list.ids.len() * size_of::<(u64, usize)>()
}

impl Weigh for Snapshot {
    fn weight(&self) -> usize {
        let mut bytes = 0;
        if let Some(elements) = &self.elements {
            bytes += list_weight(&elements.list);
        }
        if let Some(meshes) = &self.meshes {
            bytes += list_weight(&meshes.list);
        }
        if let Some(groups) = &self.groups {
            bytes += list_weight(&groups.list);
        }
        if let Some(textures) = &self.textures {
            bytes += textures
                .entries
                .iter()
                .filter(|e| Arc::strong_count(&e.texture.image) == 1)
                .map(|e| e.texture.image.w as usize * e.texture.image.h as usize * 4)
                .sum::<usize>();
        }
        if let Some(Some(world)) = &self.voxels {
            if Arc::strong_count(&world.slots) == 1 {
                bytes += world.slots.len() * size_of::<ChunkSlot>();
            }
            let owned = world
                .slots
                .iter()
                .filter(
                    |slot| matches!(slot, ChunkSlot::Data(chunk) if Arc::strong_count(chunk) == 1),
                )
                .count();
            bytes += owned * size_of::<Chunk>();
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Fake(usize);

    impl Weigh for Fake {
        fn weight(&self) -> usize {
            self.0
        }
    }

    fn label(text: &str) -> Label {
        Label::from(text)
    }

    fn push(h: &mut History<Fake>, name: &str, weight: usize) {
        h.push(label(name), Categories::OBJECTS, Fake(weight), None);
    }

    fn undo(h: &mut History<Fake>, present: usize) -> Option<Fake> {
        let mut restored = None;
        h.undo(|entry| {
            restored = Some(entry.snap.clone());
            Ok::<_, ()>(Fake(present))
        })?
        .ok()?;
        restored
    }

    #[test]
    fn undo_and_redo_swap_the_present_state_with_the_entry() {
        let mut h = History::default();
        push(&mut h, "one", 1);
        push(&mut h, "two", 2);
        assert_eq!(h.undo_label(), Some("two"));
        assert_eq!(undo(&mut h, 20), Some(Fake(2)));
        assert_eq!(h.undo_label(), Some("one"));
        assert_eq!(h.redo_label(), Some("two"));

        let mut redone = None;
        let applied = h
            .redo(|entry| {
                redone = Some(entry.snap.clone());
                Ok::<_, ()>(Fake(2))
            })
            .and_then(Result::ok)
            .expect("a redo entry");
        assert_eq!(
            redone,
            Some(Fake(20)),
            "redo restores the state undo replaced"
        );
        assert_eq!(&*applied.label, "two");
        assert_eq!(h.undo_label(), Some("two"));
        assert_eq!(h.redo_label(), None);
    }

    #[test]
    fn a_push_after_an_undo_drops_the_redo_entries() {
        let mut h = History::default();
        push(&mut h, "one", 1);
        push(&mut h, "two", 1);
        undo(&mut h, 0);
        push(&mut h, "three", 1);
        assert_eq!(h.redo_len(), 0);
        assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["one", "three"]);
    }

    #[test]
    fn nothing_to_undo_or_redo_gives_none() {
        let mut h: History<Fake> = History::default();
        assert!(undo(&mut h, 0).is_none());
        assert!(h.redo(|_| Ok::<_, ()>(Fake(0))).is_none());
    }

    #[test]
    fn a_failed_restore_leaves_the_history_alone() {
        let mut h = History::default();
        push(&mut h, "one", 1);
        let result = h.undo(|_| Err::<Fake, _>("broken"));
        assert!(matches!(result, Some(Err("broken"))));
        assert_eq!((h.undo_len(), h.redo_len()), (1, 0));
    }

    #[test]
    fn the_step_cap_drops_the_oldest_entries() {
        let mut h = History::with_limits(3, usize::MAX);
        for n in 0..5 {
            push(&mut h, &format!("step{n}"), 1);
        }
        assert_eq!(
            h.undo_labels().collect::<Vec<_>>(),
            ["step2", "step3", "step4"]
        );
    }

    #[test]
    fn the_memory_cap_drops_the_oldest_but_keeps_the_newest() {
        let mut h = History::with_limits(100, 10);
        push(&mut h, "a", 4);
        push(&mut h, "b", 4);
        push(&mut h, "c", 4);
        assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["b", "c"]);
        push(&mut h, "huge", 1000);
        assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["huge"]);
        assert_eq!(h.bytes(), 1000);
    }

    #[test]
    fn a_gesture_continues_until_something_else_is_pushed_or_undone() {
        let mut h = History::default();
        h.push_gesture(label("drag"), Categories::OBJECTS, Fake(1), None, Some(7));
        assert!(h.continues(7));
        assert!(!h.continues(8));
        undo(&mut h, 0);
        assert!(
            !h.continues(7),
            "an undone entry is not the newest any more"
        );
        h.push_gesture(label("drag"), Categories::OBJECTS, Fake(1), None, Some(7));
        push(&mut h, "other", 1);
        assert!(!h.continues(7));
    }
}
