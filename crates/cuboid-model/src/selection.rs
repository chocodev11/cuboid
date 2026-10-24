//! Selection list, primary object and the separate highlight set (spec: tools 8.1; arch 8.5).

use indexmap::IndexSet;

use crate::ids::ObjRef;

/// The selected objects in selection order, the gizmo target, and the highlighted objects.
///
/// The highlight set is separate state because select-all and select-by-name empty the list
/// without un-highlighting the previous objects (tools 8.1 quirk); every other operation keeps it
/// equal to the list. Hover, active axis, busy,
/// grabbed and the clone guard are interaction state in `cuboid-core`.
///
/// spec: tools 8.1, arch 6 (cuboid-model), arch 8.5
#[derive(Clone, Debug, Default)]
pub struct Selection {
    /// Selected objects in selection order.
    pub list: IndexSet<ObjRef>,
    /// The gizmo target: the first added object or the explicitly set one; may be `None`.
    pub primary: Option<ObjRef>,
    /// Objects drawn with the selection outline: the selected objects, plus the leftovers of
    /// [`Selection::replace_list`].
    pub highlighted: IndexSet<ObjRef>,
}

impl Selection {
    /// Whether `obj` is selected.
    pub fn contains(&self, obj: ObjRef) -> bool {
        self.list.contains(&obj)
    }

    /// Appends `obj` when absent and highlights it; it becomes primary when there is none.
    pub fn add(&mut self, obj: ObjRef) {
        self.list.insert(obj);
        self.highlighted.insert(obj);
        self.primary.get_or_insert(obj);
    }

    /// Removes `obj` when selected, else adds it.
    pub fn toggle(&mut self, obj: ObjRef) {
        if self.contains(obj) {
            self.remove(obj);
        } else {
            self.add(obj);
        }
    }

    /// Removes and un-highlights `obj`; when it was primary, the first remaining object (or
    /// none) becomes primary.
    pub fn remove(&mut self, obj: ObjRef) {
        if !self.list.shift_remove(&obj) {
            return;
        }
        self.highlighted.shift_remove(&obj);
        if self.primary == Some(obj) {
            self.primary = self.list.first().copied();
        }
    }

    /// Deselects everything and clears the primary. The selected objects lose their highlight;
    /// highlighted objects that are not in the list (left over from [`Selection::replace_list`])
    /// keep it.
    pub fn clear(&mut self) {
        for obj in self.list.drain(..) {
            self.highlighted.shift_remove(&obj);
        }
        self.primary = None;
    }

    /// Select all and select by name: the list is emptied without un-highlighting the previous
    /// objects, an existing primary is kept even if it is not in `objs`, and `objs` are added.
    ///
    /// spec: tools 8.1 (quirk)
    pub fn replace_list(&mut self, objs: impl IntoIterator<Item = ObjRef>) {
        self.list.clear();
        for obj in objs {
            self.add(obj);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ObjId;

    fn element() -> ObjRef {
        ObjRef::Element(ObjId::next())
    }

    #[test]
    fn the_first_added_object_becomes_primary() {
        let (a, b) = (element(), element());
        let mut sel = Selection::default();
        sel.add(a);
        sel.add(b);
        sel.add(a);
        assert_eq!(sel.list.iter().copied().collect::<Vec<_>>(), [a, b]);
        assert_eq!(sel.primary, Some(a));
        assert!(sel.contains(b));
    }

    #[test]
    fn toggle_removes_a_selected_object_and_adds_an_absent_one() {
        let (a, b) = (element(), element());
        let mut sel = Selection::default();
        sel.toggle(a);
        sel.toggle(b);
        assert_eq!(sel.list.len(), 2);
        sel.toggle(a);
        assert!(!sel.contains(a));
        assert_eq!(sel.primary, Some(b), "the next object becomes primary");
        sel.toggle(a);
        assert_eq!(sel.list.iter().copied().collect::<Vec<_>>(), [b, a]);
        assert_eq!(sel.primary, Some(b));
    }

    #[test]
    fn removing_the_primary_promotes_the_first_remaining_object() {
        let (a, b, c) = (element(), element(), element());
        let mut sel = Selection::default();
        for obj in [a, b, c] {
            sel.add(obj);
        }
        sel.remove(b);
        assert_eq!(
            sel.primary,
            Some(a),
            "a removed non-primary changes nothing"
        );
        sel.remove(a);
        assert_eq!(sel.primary, Some(c));
        sel.remove(c);
        assert_eq!(sel.primary, None);
        sel.remove(ObjRef::Blueprint);
        assert!(sel.list.is_empty());
    }

    fn highlighted(sel: &Selection) -> Vec<ObjRef> {
        sel.highlighted.iter().copied().collect()
    }

    #[test]
    fn add_toggle_and_remove_keep_the_highlight_in_step_with_the_list() {
        let (a, b) = (element(), element());
        let mut sel = Selection::default();
        sel.add(a);
        sel.toggle(b);
        assert_eq!(highlighted(&sel), [a, b]);
        sel.toggle(a);
        assert_eq!(highlighted(&sel), [b]);
        sel.remove(b);
        assert!(sel.highlighted.is_empty());
        sel.remove(b);
        assert!(sel.highlighted.is_empty());
    }

    #[test]
    fn clear_un_highlights_the_selected_objects_and_keeps_other_highlights() {
        let (a, b, leftover) = (element(), element(), element());
        let mut sel = Selection::default();
        sel.add(a);
        sel.add(b);
        sel.highlighted.insert(leftover);
        sel.clear();
        assert!(sel.list.is_empty());
        assert_eq!(sel.primary, None);
        assert_eq!(highlighted(&sel), [leftover]);
    }

    #[test]
    fn select_all_empties_the_list_without_un_highlighting() {
        let (a, b, c) = (element(), element(), element());
        let mut sel = Selection::default();
        sel.add(a);
        sel.add(b);

        sel.replace_list([c]);

        assert_eq!(sel.list.iter().copied().collect::<Vec<_>>(), [c]);
        assert_eq!(sel.primary, Some(a), "the old primary is kept");
        assert_eq!(highlighted(&sel), [a, b, c]);

        // The next clear only un-highlights what is selected.
        sel.clear();
        assert_eq!(highlighted(&sel), [a, b]);

        let mut sel = Selection::default();
        sel.replace_list([a, b]);
        assert_eq!(sel.primary, Some(a), "the first match becomes primary");
    }
}
