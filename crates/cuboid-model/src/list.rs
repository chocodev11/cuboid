//! Identity-preserving object list with display ids and copy-on-write items (spec: tools 2.1; arch
//! 8.2).

use std::sync::{Arc, OnceLock};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ids::{ObjId, Rev};
use crate::journal::{ChangeFlags, ChangeJournal};

/// The ordered elements or meshes of a document. List order is panel, file, export and render
/// sort tie order.
///
/// Items are `Arc`s edited through `Arc::make_mut`, so a clone of the list or a snapshot costs
/// one pointer copy per object. Display ids (the numbers shown in the list panels) live in a
/// parallel vector and are issued from a per-kind counter, so renumbering never copies a record.
///
/// An `ObjId` is never issued twice, so a removed object's id stays dead for the life of the
/// document (ids come from a process-wide counter).
///
/// spec: arch 6 (cuboid-model), arch 8.2, tools 2.1
#[derive(Clone, Debug)]
pub struct ObjList<T> {
    ids: Vec<ObjId>,
    items: Vec<Arc<T>>,
    display_ids: Vec<u32>,
    next_display_id: u32,
    /// `ObjId` to position, built on first lookup after a structural change.
    index: OnceLock<FxHashMap<ObjId, usize>>,
    /// Changes with every structural change (push, remove, reorder, restore, clear).
    structure_rev: Rev,
    /// Changes with every structural change and every edit.
    rev: Rev,
}

/// A list captured for undo, clipboard or loading: ids and item pointers in list order.
/// Display ids are not stored; a restore re-issues them from 1 in list order.
///
/// spec: arch 8.2, tools 2.1, tools 15.3
#[derive(Clone, Debug)]
pub struct ListSnap<T> {
    /// One id per item.
    pub ids: Vec<ObjId>,
    /// The items, parallel to `ids`.
    pub items: Vec<Arc<T>>,
}

impl<T> ListSnap<T> {
    /// A snapshot over new items, each with a fresh id: what a loader hands to the apply path.
    pub fn fresh(items: impl IntoIterator<Item = T>) -> Self {
        let items: Vec<Arc<T>> = items.into_iter().map(Arc::new).collect();
        let ids = items.iter().map(|_| ObjId::next()).collect();
        Self { ids, items }
    }
}

impl<T> Default for ObjList<T> {
    /// An empty list whose display-id counter is 0.
    fn default() -> Self {
        Self {
            ids: Vec::new(),
            items: Vec::new(),
            display_ids: Vec::new(),
            next_display_id: 0,
            index: OnceLock::new(),
            structure_rev: Rev::next(),
            rev: Rev::next(),
        }
    }
}

impl<T: Clone> ObjList<T> {
    /// Number of items.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the list has no items.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The position of `id` in list order.
    pub fn position(&self, id: ObjId) -> Option<usize> {
        let index = self.index.get_or_init(|| {
            self.ids
                .iter()
                .enumerate()
                .map(|(pos, &id)| (id, pos))
                .collect()
        });
        index.get(&id).copied()
    }

    /// The item with `id`.
    pub fn get(&self, id: ObjId) -> Option<&T> {
        let pos = self.position(id)?;
        self.items.get(pos).map(|item| &**item)
    }

    /// The ids in list order.
    pub fn ids(&self) -> &[ObjId] {
        &self.ids
    }

    /// The item pointers in list order, parallel to [`ObjList::ids`]; cloning one shares it.
    pub fn items(&self) -> &[Arc<T>] {
        &self.items
    }

    /// The numbers shown in the list panel, parallel to [`ObjList::ids`].
    pub fn display_ids(&self) -> &[u32] {
        &self.display_ids
    }

    /// The number shown in the list panel for `id`.
    pub fn display_id(&self, id: ObjId) -> Option<u32> {
        let pos = self.position(id)?;
        self.display_ids.get(pos).copied()
    }

    /// The display id the next [`ObjList::push`] issues; creation commands build the default
    /// label ("Box" + id) from it.
    pub fn next_display_id(&self) -> u32 {
        self.next_display_id + 1
    }

    /// Changes whenever the membership or the order changes.
    pub fn structure_rev(&self) -> Rev {
        self.structure_rev
    }

    /// Changes whenever the membership, the order or an item changes.
    pub fn rev(&self) -> Rev {
        self.rev
    }

    /// Mutable access to an item: `Arc::make_mut`, a fresh stamp and a journal entry that marks
    /// every change flag, for edits that touch an unknown part of the object. Setters that
    /// know what they change use [`ObjList::edit_marked`].
    pub fn edit(&mut self, id: ObjId, j: &mut ChangeJournal) -> Option<&mut T> {
        self.edit_marked(id, ChangeFlags::all(), j)
    }

    /// As [`ObjList::edit`], marking only `flags`.
    pub fn edit_marked(
        &mut self,
        id: ObjId,
        flags: ChangeFlags,
        j: &mut ChangeJournal,
    ) -> Option<&mut T> {
        let pos = self.position(id)?;
        self.rev = Rev::next();
        j.mark_object(id, flags);
        self.items.get_mut(pos).map(Arc::make_mut)
    }

    /// Appends an item with the next display id and a fresh `ObjId`.
    pub fn push(&mut self, item: T) -> ObjId {
        let id = ObjId::next();
        self.next_display_id += 1;
        if let Some(index) = self.index.get_mut() {
            index.insert(id, self.ids.len());
        }
        self.ids.push(id);
        self.items.push(Arc::new(item));
        self.display_ids.push(self.next_display_id);
        // The lookup index (if built) already has the new id, so only the stamps change.
        self.stamp_structure();
        id
    }

    /// Removes the items with these ids; the display ids of the rest are kept.
    pub fn remove_set(&mut self, ids: &FxHashSet<ObjId>) {
        let keep: Vec<bool> = self.ids.iter().map(|id| !ids.contains(id)).collect();
        if keep.iter().all(|&k| k) {
            return;
        }
        let mut at = 0;
        self.items.retain(|_| {
            at += 1;
            keep[at - 1]
        });
        let mut at = 0;
        self.display_ids.retain(|_| {
            at += 1;
            keep[at - 1]
        });
        self.ids.retain(|id| !ids.contains(id));
        self.touch_structure();
    }

    /// Moves the items for which `front` is true to the head, keeping the relative order of
    /// both groups ("move to top" and "move to bottom"). `front` gets the position, the id and
    /// the item, so a caller can decide by identity (selection membership) or by index. Display
    /// ids travel with their items.
    pub fn stable_partition(&mut self, mut front: impl FnMut(usize, ObjId, &T) -> bool) {
        let (head, tail): (Vec<usize>, Vec<usize>) =
            (0..self.items.len()).partition(|&pos| front(pos, self.ids[pos], &self.items[pos]));
        let order: Vec<usize> = head.into_iter().chain(tail).collect();
        if order.iter().enumerate().all(|(at, &pos)| at == pos) {
            return;
        }
        self.ids = order.iter().map(|&pos| self.ids[pos]).collect();
        self.items = order.iter().map(|&pos| self.items[pos].clone()).collect();
        self.display_ids = order.iter().map(|&pos| self.display_ids[pos]).collect();
        self.touch_structure();
    }

    /// Pointer copies of the list for undo and clipboards.
    pub fn snapshot(&self) -> ListSnap<T> {
        ListSnap {
            ids: self.ids.clone(),
            items: self.items.clone(),
        }
    }

    /// Replaces the contents by a snapshot, keeping its `ObjId`s and renumbering display ids
    /// from 1 in list order.
    pub fn restore(&mut self, s: &ListSnap<T>) {
        self.ids.clone_from(&s.ids);
        self.items.clone_from(&s.items);
        self.next_display_id = u32::try_from(self.ids.len()).unwrap_or(u32::MAX);
        self.display_ids = (1..=self.next_display_id).collect();
        self.touch_structure();
    }

    /// Removes every item and sets the display-id counter back to 0.
    pub fn clear(&mut self) {
        self.ids.clear();
        self.items.clear();
        self.display_ids.clear();
        self.next_display_id = 0;
        self.touch_structure();
    }

    /// The number of display ids issued since the last clear, which the invariant checker
    /// compares with the ids in use.
    pub(crate) fn issued_display_ids(&self) -> u32 {
        self.next_display_id
    }

    fn touch_structure(&mut self) {
        self.index = OnceLock::new();
        self.stamp_structure();
    }

    fn stamp_structure(&mut self) {
        self.structure_rev = Rev::next();
        self.rev = self.structure_rev;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_of(values: &[i32]) -> (ObjList<i32>, Vec<ObjId>) {
        let mut list = ObjList::default();
        let ids = values.iter().map(|&v| list.push(v)).collect();
        (list, ids)
    }

    fn values(list: &ObjList<i32>) -> Vec<i32> {
        list.items().iter().map(|item| **item).collect()
    }

    #[test]
    fn a_fresh_snapshot_gives_every_item_its_own_id() {
        let snap = ListSnap::fresh([10, 20, 30]);
        assert_eq!(snap.ids.len(), 3);
        assert_eq!(snap.items.len(), 3);
        assert_ne!(snap.ids[0], snap.ids[1]);
        assert_ne!(snap.ids[1], snap.ids[2]);
        assert_eq!(*snap.items[2], 30);
    }

    #[test]
    fn display_ids_count_from_one_in_creation_order() {
        let (list, ids) = list_of(&[10, 20, 30]);
        assert_eq!(list.display_ids(), [1, 2, 3]);
        assert_eq!(list.display_id(ids[1]), Some(2));
        assert_eq!(list.next_display_id(), 4);
        assert_eq!(list.get(ids[2]), Some(&30));
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn removal_keeps_display_ids_and_lookups_and_never_reuses_numbers() {
        let (mut list, ids) = list_of(&[10, 20, 30, 40]);
        assert_eq!(list.get(ids[3]), Some(&40));
        list.remove_set(&FxHashSet::from_iter([ids[1], ids[2]]));
        assert_eq!(values(&list), [10, 40]);
        assert_eq!(list.display_ids(), [1, 4]);
        assert_eq!(list.ids(), [ids[0], ids[3]]);
        assert_eq!(list.get(ids[2]), None);
        assert_eq!(list.get(ids[3]), Some(&40));
        assert_eq!(list.position(ids[3]), Some(1));

        let fresh = list.push(50);
        assert_eq!(list.display_id(fresh), Some(5));
        assert!(!ids.contains(&fresh));
        assert_eq!(list.get(fresh), Some(&50));
    }

    #[test]
    fn a_push_after_a_lookup_keeps_the_index_valid() {
        let (mut list, ids) = list_of(&[1, 2]);
        assert_eq!(list.get(ids[0]), Some(&1));
        let id = list.push(3);
        assert_eq!(list.get(id), Some(&3));
        assert_eq!(list.position(id), Some(2));
    }

    #[test]
    fn a_push_does_not_throw_the_lookup_index_away() {
        let (mut list, ids) = list_of(&[1]);
        assert_eq!(list.position(ids[0]), Some(0));
        let structure = list.structure_rev();
        let mut last = ids[0];
        for v in 2..50 {
            last = list.push(v);
            assert!(list.index.get().is_some(), "push {v} dropped the index");
            assert_eq!(list.position(last), Some(v as usize - 1));
        }
        assert_ne!(list.structure_rev(), structure);
        assert_eq!(list.get(ids[0]), Some(&1));
        assert_eq!(list.get(last), Some(&49));
    }

    #[test]
    fn removing_nothing_changes_no_stamp() {
        let (mut list, _) = list_of(&[1, 2]);
        let (structure, rev) = (list.structure_rev(), list.rev());
        list.remove_set(&FxHashSet::from_iter([ObjId::next()]));
        assert_eq!((list.structure_rev(), list.rev()), (structure, rev));
    }

    #[test]
    fn edit_copies_a_shared_item_and_journals_it() {
        let (mut list, ids) = list_of(&[10, 20]);
        let snap = list.snapshot();
        let mut journal = ChangeJournal::default();
        let rev = list.rev();

        *list.edit(ids[0], &mut journal).unwrap() = 11;

        assert_eq!(values(&list), [11, 20]);
        assert_eq!(*snap.items[0], 10, "the snapshot keeps the old record");
        assert!(Arc::ptr_eq(&snap.items[1], &list.items()[1]));
        assert_eq!(journal.objects()[&ids[0]], ChangeFlags::all());
        assert_ne!(list.rev(), rev);
    }

    #[test]
    fn edit_of_a_missing_id_does_nothing() {
        let (mut list, _) = list_of(&[10]);
        let mut journal = ChangeJournal::default();
        let rev = list.rev();
        assert!(list.edit(ObjId::next(), &mut journal).is_none());
        assert!(journal.is_empty());
        assert_eq!(list.rev(), rev);
    }

    #[test]
    fn edit_marked_journals_only_the_given_flags() {
        let (mut list, ids) = list_of(&[10]);
        let mut journal = ChangeJournal::default();
        list.edit_marked(ids[0], ChangeFlags::LABEL, &mut journal);
        assert_eq!(journal.objects()[&ids[0]], ChangeFlags::LABEL);
    }

    #[test]
    fn stable_partition_keeps_the_order_inside_both_groups() {
        let (mut list, ids) = list_of(&[1, 2, 3, 4, 5, 6]);
        list.stable_partition(|_, _, v| v % 2 == 0);
        assert_eq!(values(&list), [2, 4, 6, 1, 3, 5]);
        assert_eq!(list.display_ids(), [2, 4, 6, 1, 3, 5]);
        assert_eq!(list.ids()[0], ids[1]);
        assert_eq!(list.position(ids[0]), Some(3));
        assert_eq!(list.get(ids[4]), Some(&5));

        list.stable_partition(|_, _, v| v % 2 == 1);
        assert_eq!(values(&list), [1, 3, 5, 2, 4, 6]);
    }

    #[test]
    fn stable_partition_can_decide_by_identity_or_by_index() {
        let (mut list, ids) = list_of(&[10, 10, 10, 10]);
        let selected = FxHashSet::from_iter([ids[1], ids[3]]);
        list.stable_partition(|_, id, _| selected.contains(&id));
        assert_eq!(list.ids(), [ids[1], ids[3], ids[0], ids[2]]);
        assert_eq!(list.display_ids(), [2, 4, 1, 3]);
        assert_eq!(list.position(ids[3]), Some(1));

        // Move to bottom: everything but the selection goes first.
        list.stable_partition(|_, id, _| !selected.contains(&id));
        assert_eq!(list.ids(), [ids[0], ids[2], ids[1], ids[3]]);

        list.stable_partition(|pos, _, _| pos >= 2);
        assert_eq!(list.ids(), [ids[1], ids[3], ids[0], ids[2]]);
    }

    #[test]
    fn stable_partition_of_an_ordered_list_changes_no_stamp() {
        let (mut list, _) = list_of(&[2, 4, 1]);
        let structure = list.structure_rev();
        list.stable_partition(|_, _, v| v % 2 == 0);
        assert_eq!(list.structure_rev(), structure);
    }

    #[test]
    fn restore_keeps_the_object_ids_and_renumbers_from_one() {
        let (mut list, ids) = list_of(&[10, 20, 30]);
        let snap = list.snapshot();
        list.remove_set(&FxHashSet::from_iter([ids[0]]));
        list.push(40);
        assert_eq!(list.display_ids(), [2, 3, 4]);

        list.restore(&snap);

        assert_eq!(values(&list), [10, 20, 30]);
        assert_eq!(list.ids(), ids.as_slice());
        assert_eq!(list.display_ids(), [1, 2, 3]);
        assert_eq!(list.get(ids[0]), Some(&10));
        let next = list.push(50);
        assert_eq!(list.display_id(next), Some(4));
    }

    #[test]
    fn restore_and_snapshot_share_the_item_pointers() {
        let (mut list, _) = list_of(&[10, 20]);
        let snap = list.snapshot();
        list.clear();
        list.restore(&snap);
        assert!(Arc::ptr_eq(&snap.items[0], &list.items()[0]));
        assert!(Arc::ptr_eq(&snap.items[1], &list.items()[1]));
    }

    #[test]
    fn clear_sets_the_display_counter_back_to_zero() {
        let (mut list, ids) = list_of(&[10, 20]);
        list.clear();
        assert!(list.is_empty());
        assert_eq!(list.get(ids[0]), None);
        assert_eq!(list.next_display_id(), 1);
        let id = list.push(30);
        assert_eq!(list.display_id(id), Some(1));
        assert!(
            !ids.contains(&id),
            "object ids are not reused after a clear"
        );
    }

    #[test]
    fn a_restore_from_fresh_items_numbers_them_in_list_order() {
        let mut list = ObjList::default();
        list.restore(&ListSnap::fresh([7, 8, 9]));
        assert_eq!(list.display_ids(), [1, 2, 3]);
        assert_eq!(values(&list), [7, 8, 9]);
    }

    #[test]
    fn display_ids_stay_valid_through_every_list_operation() {
        use crate::invariants::{InvariantError, check_display_ids};

        let (mut list, ids) = list_of(&[1, 2, 3, 4]);
        list.remove_set(&FxHashSet::from_iter([ids[1]]));
        list.stable_partition(|_, _, v| *v == 4);
        list.push(5);
        assert!(check_display_ids(&list, "test").is_ok());
        let snap = list.snapshot();
        list.restore(&snap);
        assert!(check_display_ids(&list, "test").is_ok());

        list.display_ids[1] = list.display_ids[0];
        assert!(matches!(
            check_display_ids(&list, "test"),
            Err(InvariantError::DisplayIdOrder("test"))
        ));
        list.display_ids[1] = list.next_display_id + 1;
        assert!(check_display_ids(&list, "test").is_err());
        list.display_ids[1] = 0;
        assert!(check_display_ids(&list, "test").is_err());
    }
}
