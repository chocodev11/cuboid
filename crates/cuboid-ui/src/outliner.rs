//! The outliner: a virtualised object tree with selection, rename, drag to reparent, visibility
//! and lock toggles.

use std::collections::{HashMap, HashSet};

use cuboid_actions::ActionId;
use cuboid_ui_kit::{
    DropHint, EditOutcome, Icon, RADIUS, RowToggle, Tokens, TreeRow, search_field, tree_row,
};
use egui::{Align2, CursorIcon, FontId, Id, LayerId, Order, ScrollArea, Ui, vec2};

use crate::common::Sink;
use crate::model::{DocView, NodeId, NodeKind, NodeView};
use crate::request::{Edit, SelectOp};

const ROW_HEIGHT: f32 = 24.0;

/// One visible row of the flattened tree.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// Index into the node list.
    pub index: usize,
    /// Nesting level.
    pub depth: usize,
    /// The node has children.
    pub has_children: bool,
}

/// Parent links and sibling lists of a node list.
#[derive(Default)]
pub(crate) struct Tree {
    index: HashMap<NodeId, usize>,
    /// Child node indices by parent; `None` holds the roots.
    children: HashMap<Option<NodeId>, Vec<usize>>,
}

impl Tree {
    pub fn build(nodes: &[NodeView]) -> Tree {
        let index: HashMap<NodeId, usize> =
            nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let mut children: HashMap<Option<NodeId>, Vec<usize>> = HashMap::new();
        for (i, node) in nodes.iter().enumerate() {
            let parent = node
                .parent
                .filter(|p| *p != node.id && index.contains_key(p));
            children.entry(parent).or_default().push(i);
        }
        Tree { index, children }
    }

    /// The parent that counts: a missing or self parent makes a root.
    fn parent_of(&self, nodes: &[NodeView], id: NodeId) -> Option<NodeId> {
        let node = &nodes[*self.index.get(&id)?];
        node.parent
            .filter(|p| *p != id && self.index.contains_key(p))
    }

    /// `ancestor` is `id` or one of its ancestors. Stops after the number of nodes, which also
    /// ends a cycle in bad data.
    pub fn is_within(&self, nodes: &[NodeView], id: NodeId, ancestor: NodeId) -> bool {
        let mut current = Some(id);
        for _ in 0..=nodes.len() {
            match current {
                Some(c) if c == ancestor => return true,
                Some(c) => current = self.parent_of(nodes, c),
                None => return false,
            }
        }
        false
    }

    /// Flattens the tree in display order. Nodes in `collapsed` hide their children unless a
    /// filter is active; with a filter only matching nodes and their ancestors show.
    pub fn rows(&self, nodes: &[NodeView], collapsed: &HashSet<NodeId>, filter: &str) -> Vec<Row> {
        let needle = filter.trim().to_lowercase();
        let keep = (!needle.is_empty()).then(|| self.matching(nodes, &needle));
        let mut rows = Vec::with_capacity(nodes.len().min(4096));
        // An explicit stack keeps deep trees off the call stack.
        let mut stack: Vec<(usize, usize)> = Vec::new();
        if let Some(roots) = self.children.get(&None) {
            stack.extend(roots.iter().rev().map(|&i| (i, 0)));
        }
        let mut guard = nodes.len().saturating_mul(2).saturating_add(8);
        while let Some((i, depth)) = stack.pop() {
            guard = guard.saturating_sub(1);
            if guard == 0 {
                break;
            }
            if keep.as_ref().is_some_and(|keep| !keep[i]) {
                continue;
            }
            let id = nodes[i].id;
            let kids = self.children.get(&Some(id));
            let has_children =
                kids.is_some_and(|k| keep.as_ref().is_none_or(|keep| k.iter().any(|&c| keep[c])));
            rows.push(Row {
                index: i,
                depth,
                has_children,
            });
            let open = keep.is_some() || !collapsed.contains(&id);
            if open && let Some(kids) = kids {
                stack.extend(kids.iter().rev().map(|&c| (c, depth + 1)));
            }
        }
        rows
    }

    fn matching(&self, nodes: &[NodeView], needle: &str) -> Vec<bool> {
        let mut keep = vec![false; nodes.len()];
        for (i, node) in nodes.iter().enumerate() {
            if !node.name.to_lowercase().contains(needle) {
                continue;
            }
            keep[i] = true;
            let mut parent = self.parent_of(nodes, node.id);
            let mut steps = 0;
            while let Some(p) = parent {
                let Some(&pi) = self.index.get(&p) else { break };
                if keep[pi] || steps > nodes.len() {
                    break;
                }
                keep[pi] = true;
                parent = self.parent_of(nodes, p);
                steps += 1;
            }
        }
        keep
    }

    /// The sibling list a node is in.
    fn siblings(&self, nodes: &[NodeView], id: NodeId) -> &[usize] {
        let parent = self.parent_of(nodes, id);
        self.children.get(&parent).map_or(&[], Vec::as_slice)
    }
}

/// Parent and insertion point for dropping `dragged` next to or into `target`; `None` when the
/// drop is not allowed (into itself or its own subtree).
pub(crate) fn drop_target(
    tree: &Tree,
    nodes: &[NodeView],
    dragged: &[NodeId],
    target: NodeId,
    hint: DropHint,
) -> Option<(Option<NodeId>, Option<NodeId>)> {
    if dragged.iter().any(|d| tree.is_within(nodes, target, *d)) {
        return None;
    }
    match hint {
        DropHint::Inside => Some((Some(target), None)),
        DropHint::Before => Some((tree.parent_of(nodes, target), Some(target))),
        DropHint::After => {
            let siblings = tree.siblings(nodes, target);
            let pos = siblings.iter().position(|&i| nodes[i].id == target)?;
            let next = siblings[pos + 1..]
                .iter()
                .map(|&i| nodes[i].id)
                .find(|id| !dragged.contains(id));
            Some((tree.parent_of(nodes, target), next))
        }
    }
}

struct DragState {
    ids: Vec<NodeId>,
    target: Option<(NodeId, DropHint)>,
}

struct Cache {
    nodes_rev: u64,
    version: u64,
    filter: String,
    tree: Tree,
    rows: Vec<Row>,
}

/// State of the outliner between frames.
#[derive(Default)]
pub(crate) struct OutlinerState {
    filter: String,
    collapsed: HashSet<NodeId>,
    version: u64,
    cache: Option<Cache>,
    rename: Option<(NodeId, String)>,
    drag: Option<DragState>,
    anchor: Option<NodeId>,
}

impl OutlinerState {
    /// Starts renaming a node, for F2 and the context menu.
    pub fn begin_rename(&mut self, id: NodeId, current: &str) {
        self.rename = Some((id, current.to_owned()));
    }

    pub fn show(&mut self, ui: &mut Ui, doc: &DocView<'_>, sink: &mut Sink) {
        let tokens = Tokens::of(ui.ctx());
        search_field(ui, "outliner_filter", &mut self.filter, "Filter objects");
        ui.add_space(4.0);

        let stale = self.cache.as_ref().is_none_or(|c| {
            c.nodes_rev != doc.nodes_rev || c.version != self.version || c.filter != self.filter
        });
        if stale {
            let tree = Tree::build(doc.nodes);
            let rows = tree.rows(doc.nodes, &self.collapsed, &self.filter);
            self.cache = Some(Cache {
                nodes_rev: doc.nodes_rev,
                version: self.version,
                filter: self.filter.clone(),
                tree,
                rows,
            });
        }
        let Some(cache) = self.cache.as_ref() else {
            return;
        };

        if doc.nodes.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Nothing here yet").color(tokens.text_weak));
                ui.label(
                    egui::RichText::new("Add a cube from the Create menu.").color(tokens.text_weak),
                );
            });
            return;
        }
        if cache.rows.is_empty() {
            ui.label(egui::RichText::new("No object matches").color(tokens.text_weak));
            return;
        }

        let pointer = ui.input(|i| i.pointer.hover_pos());
        let released = ui.input(|i| i.pointer.any_released());
        let modifiers = ui.input(|i| i.modifiers);
        let mut new_target: Option<(NodeId, DropHint)> = None;
        let mut toggle_collapse: Option<NodeId> = None;
        let mut clicked: Option<(NodeId, usize)> = None;
        let mut started_drag: Option<NodeId> = None;
        let mut rename_result: Option<(NodeId, String, bool)> = None;
        let mut double_clicked: Option<NodeId> = None;

        ui.spacing_mut().item_spacing.y = 0.0;
        let area = ScrollArea::vertical().auto_shrink([false, false]);
        let output = area.show_rows(ui, ROW_HEIGHT, cache.rows.len(), |ui, range| {
            for pos in range {
                let Some(row) = cache.rows.get(pos).copied() else {
                    continue;
                };
                let node = &doc.nodes[row.index];
                let selected = doc.selection.contains(&node.id);
                let toggles = [
                    RowToggle {
                        on: Icon::Eye,
                        off: Icon::EyeOff,
                        state: node.visible,
                        tip: if node.visible { "Hide" } else { "Show" },
                    },
                    RowToggle {
                        on: Icon::Lock,
                        off: Icon::Unlock,
                        state: node.locked,
                        tip: if node.locked { "Unlock" } else { "Lock" },
                    },
                ];
                let drop = self
                    .drag
                    .as_ref()
                    .and_then(|d| d.target)
                    .filter(|(id, _)| *id == node.id)
                    .map(|(_, hint)| hint);
                let editing = match &mut self.rename {
                    Some((id, buffer)) if *id == node.id => Some(buffer),
                    _ => None,
                };
                let icon = match node.kind {
                    NodeKind::Group => Icon::Group,
                    NodeKind::Element => Icon::Cube,
                    NodeKind::Mesh => Icon::Mesh,
                    NodeKind::Voxels => Icon::Voxel,
                };
                let resp = tree_row(
                    ui,
                    ("outliner_row", node.id.0),
                    TreeRow {
                        depth: row.depth,
                        label: &node.name,
                        icon,
                        expanded: row.has_children.then(|| !self.collapsed.contains(&node.id)),
                        selected,
                        primary: doc.primary == Some(node.id),
                        dimmed: !node.visible,
                        toggles: &toggles,
                        drop,
                        editing,
                    },
                );
                if resp.arrow {
                    toggle_collapse = Some(node.id);
                }
                if resp.toggled[0] {
                    sink.edit(Edit::SetVisible {
                        id: node.id,
                        visible: !node.visible,
                    });
                }
                if resp.toggled[1] {
                    sink.edit(Edit::SetLocked {
                        id: node.id,
                        locked: !node.locked,
                    });
                }
                match resp.edit {
                    EditOutcome::Commit => {
                        if let Some((id, buffer)) = &self.rename {
                            rename_result = Some((*id, buffer.clone(), true));
                        }
                    }
                    EditOutcome::Cancel => rename_result = Some((node.id, String::new(), false)),
                    EditOutcome::None => {}
                }
                let rename_active = matches!(&self.rename, Some((id, _)) if *id == node.id);
                if !rename_active && !resp.arrow && !resp.toggled.iter().any(|t| *t) {
                    if resp.row.clicked() {
                        clicked = Some((node.id, pos));
                    }
                    if resp.row.double_clicked() {
                        double_clicked = Some(node.id);
                    }
                    if resp.row.drag_started() {
                        started_drag = Some(node.id);
                    }
                    if resp.row.secondary_clicked() && !selected {
                        sink.edit(Edit::Select {
                            ids: vec![node.id],
                            op: SelectOp::Replace,
                        });
                    }
                    resp.row
                        .context_menu(|ui| context_menu(ui, doc, node, sink, &mut self.rename));
                }
                if self.drag.is_some()
                    && let Some(p) = pointer.filter(|p| resp.row.rect.contains(*p))
                {
                    let can_nest = node.kind == NodeKind::Group;
                    new_target = Some((node.id, DropHint::at(resp.row.rect, p.y, can_nest)));
                }
            }
        });

        if let Some(id) = toggle_collapse {
            if !self.collapsed.remove(&id) {
                self.collapsed.insert(id);
            }
            self.version += 1;
        }
        if let Some((id, name, commit)) = rename_result {
            self.rename = None;
            if commit && !name.trim().is_empty() {
                sink.edit(Edit::Rename { id, name });
            }
        }
        if let Some(id) = double_clicked
            && let Some(node) = doc.nodes.iter().find(|n| n.id == id)
        {
            self.rename = Some((id, node.name.to_string()));
        }
        if let Some((id, pos)) = clicked {
            self.click(id, pos, modifiers, doc, sink);
        }
        if let Some(id) = started_drag {
            let ids = if doc.selection.contains(&id) {
                doc.selection.to_vec()
            } else {
                vec![id]
            };
            self.drag = Some(DragState { ids, target: None });
        }
        self.drive_drag(
            ui,
            doc,
            &output.inner_rect,
            new_target,
            pointer,
            released,
            sink,
        );
        ui.painter(); // keep the painter borrowed for the ghost below
        let _ = tokens;
    }

    fn click(
        &mut self,
        id: NodeId,
        pos: usize,
        mods: egui::Modifiers,
        doc: &DocView<'_>,
        sink: &mut Sink,
    ) {
        if mods.shift {
            let anchor_pos = self.anchor.and_then(|a| {
                self.cache
                    .as_ref()?
                    .rows
                    .iter()
                    .position(|r| doc.nodes[r.index].id == a)
            });
            if let (Some(from), Some(cache)) = (anchor_pos, self.cache.as_ref()) {
                let (lo, hi) = (from.min(pos), from.max(pos));
                let ids = cache.rows[lo..=hi.min(cache.rows.len() - 1)]
                    .iter()
                    .map(|r| doc.nodes[r.index].id)
                    .collect();
                sink.edit(Edit::Select {
                    ids,
                    op: SelectOp::Replace,
                });
                return;
            }
        }
        self.anchor = Some(id);
        let op = if mods.command {
            SelectOp::Toggle
        } else {
            SelectOp::Replace
        };
        sink.edit(Edit::Select { ids: vec![id], op });
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_drag(
        &mut self,
        ui: &Ui,
        doc: &DocView<'_>,
        list_rect: &egui::Rect,
        target: Option<(NodeId, DropHint)>,
        pointer: Option<egui::Pos2>,
        released: bool,
        sink: &mut Sink,
    ) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        let Some(cache) = self.cache.as_ref() else {
            self.drag = None;
            return;
        };
        drag.target = target.filter(|(id, hint)| {
            drop_target(&cache.tree, doc.nodes, &drag.ids, *id, *hint).is_some()
        });
        ui.ctx().set_cursor_icon(if drag.target.is_some() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::NoDrop
        });
        if let Some(p) = pointer {
            let tokens = Tokens::of(ui.ctx());
            let painter = ui.ctx().layer_painter(LayerId::new(
                Order::Tooltip,
                Id::unique("outliner_drag_ghost"),
            ));
            let text = match drag.ids.as_slice() {
                [one] => doc
                    .nodes
                    .iter()
                    .find(|n| n.id == *one)
                    .map_or_else(String::new, |n| n.name.to_string()),
                many => format!("{} objects", many.len()),
            };
            let galley =
                painter.layout_no_wrap(text, FontId::proportional(13.0), tokens.text_strong);
            let rect =
                egui::Rect::from_min_size(p + vec2(16.0, 12.0), galley.size() + vec2(16.0, 8.0));
            painter.rect_filled(rect, RADIUS, tokens.bg_raised);
            painter.galley(rect.min + vec2(8.0, 4.0), galley, tokens.text_strong);
            let _ = Align2::LEFT_TOP;
        }
        if !released {
            return;
        }
        let drag = self.drag.take();
        let Some(drag) = drag else { return };
        let outcome = match drag.target {
            Some((id, hint)) => drop_target(&cache.tree, doc.nodes, &drag.ids, id, hint),
            None if pointer.is_some_and(|p| list_rect.contains(p)) && !doc.nodes.is_empty() => {
                // Dropped on empty space below the last row: move to the end of the root.
                Some((None, None))
            }
            None => None,
        };
        if let Some((parent, before)) = outcome {
            let mut ids = drag.ids;
            ids.sort_by_key(|id| cache.tree.index.get(id).copied().unwrap_or(usize::MAX));
            sink.edit(Edit::Reparent {
                ids,
                parent,
                before,
            });
        }
    }
}

fn context_menu(
    ui: &mut Ui,
    doc: &DocView<'_>,
    node: &NodeView,
    sink: &mut Sink,
    rename: &mut Option<(NodeId, String)>,
) {
    let run = |ui: &mut Ui, label: &str, f: &mut dyn FnMut()| {
        if ui.button(label).clicked() {
            f();
            ui.close();
        }
    };
    run(ui, "Rename", &mut || {
        *rename = Some((node.id, node.name.to_string()))
    });
    run(ui, "Duplicate", &mut || sink.action(ActionId::Clone));
    ui.separator();
    run(ui, "Group selection", &mut || {
        sink.edit(Edit::GroupSelection)
    });
    if node.kind == NodeKind::Group {
        run(ui, "Ungroup", &mut || sink.edit(Edit::Ungroup));
    }
    ui.separator();
    let visible_label = if node.visible { "Hide" } else { "Show" };
    run(ui, visible_label, &mut || {
        sink.edit(Edit::SetVisible {
            id: node.id,
            visible: !node.visible,
        });
    });
    let lock_label = if node.locked { "Unlock" } else { "Lock" };
    run(ui, lock_label, &mut || {
        sink.edit(Edit::SetLocked {
            id: node.id,
            locked: !node.locked,
        });
    });
    ui.separator();
    let _ = doc;
    run(ui, "Delete", &mut || sink.action(ActionId::DeleteConfirm));
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn node(id: u64, parent: Option<u64>, name: &str, kind: NodeKind) -> NodeView {
        NodeView {
            id: NodeId(id),
            parent: parent.map(NodeId),
            name: Arc::from(name),
            kind,
            visible: true,
            locked: false,
        }
    }

    /// root group 1 { cube 2, group 3 { cube 4 } }, cube 5
    fn sample() -> Vec<NodeView> {
        vec![
            node(1, None, "Body", NodeKind::Group),
            node(2, Some(1), "Head", NodeKind::Element),
            node(3, Some(1), "Arm", NodeKind::Group),
            node(4, Some(3), "Hand", NodeKind::Element),
            node(5, None, "Base", NodeKind::Element),
        ]
    }

    fn ids(nodes: &[NodeView], rows: &[Row]) -> Vec<(u64, usize)> {
        rows.iter()
            .map(|r| (nodes[r.index].id.0, r.depth))
            .collect()
    }

    #[test]
    fn rows_follow_the_tree_in_sibling_order() {
        let nodes = sample();
        let tree = Tree::build(&nodes);
        let rows = tree.rows(&nodes, &HashSet::new(), "");
        assert_eq!(ids(&nodes, &rows), [(1, 0), (2, 1), (3, 1), (4, 2), (5, 0)]);
        assert!(rows[0].has_children && !rows[1].has_children);
    }

    #[test]
    fn collapsed_groups_hide_their_children() {
        let nodes = sample();
        let tree = Tree::build(&nodes);
        let collapsed = HashSet::from([NodeId(1)]);
        let rows = tree.rows(&nodes, &collapsed, "");
        assert_eq!(ids(&nodes, &rows), [(1, 0), (5, 0)]);
    }

    #[test]
    fn a_filter_keeps_matches_and_their_ancestors_and_ignores_collapsing() {
        let nodes = sample();
        let tree = Tree::build(&nodes);
        let collapsed = HashSet::from([NodeId(1), NodeId(3)]);
        let rows = tree.rows(&nodes, &collapsed, "hand");
        assert_eq!(ids(&nodes, &rows), [(1, 0), (3, 1), (4, 2)]);
        assert!(tree.rows(&nodes, &collapsed, "zzz").is_empty());
    }

    #[test]
    fn a_missing_parent_makes_a_root_and_a_cycle_does_not_hang() {
        let nodes = vec![
            node(1, Some(99), "Lost", NodeKind::Element),
            node(2, Some(3), "A", NodeKind::Group),
            node(3, Some(2), "B", NodeKind::Group),
            node(4, Some(4), "Self", NodeKind::Group),
        ];
        let tree = Tree::build(&nodes);
        let rows = tree.rows(&nodes, &HashSet::new(), "");
        assert_eq!(ids(&nodes, &rows), [(1, 0), (4, 0)]);
        assert!(!tree.is_within(&nodes, NodeId(2), NodeId(1)));
        // The cycle 2 -> 3 -> 2 ends instead of looping.
        assert!(!tree.is_within(&nodes, NodeId(2), NodeId(77)));
    }

    #[test]
    fn dropping_into_the_own_subtree_is_refused() {
        let nodes = sample();
        let tree = Tree::build(&nodes);
        let dragged = [NodeId(1)];
        assert!(drop_target(&tree, &nodes, &dragged, NodeId(4), DropHint::Inside).is_none());
        assert!(drop_target(&tree, &nodes, &dragged, NodeId(1), DropHint::After).is_none());
        assert!(drop_target(&tree, &nodes, &dragged, NodeId(5), DropHint::After).is_some());
    }

    #[test]
    fn drop_positions_resolve_to_parent_and_sibling() {
        let nodes = sample();
        let tree = Tree::build(&nodes);
        let d = [NodeId(5)];
        assert_eq!(
            drop_target(&tree, &nodes, &d, NodeId(1), DropHint::Inside),
            Some((Some(NodeId(1)), None))
        );
        assert_eq!(
            drop_target(&tree, &nodes, &d, NodeId(2), DropHint::Before),
            Some((Some(NodeId(1)), Some(NodeId(2))))
        );
        // After the first child: inserted before its next sibling.
        assert_eq!(
            drop_target(&tree, &nodes, &d, NodeId(2), DropHint::After),
            Some((Some(NodeId(1)), Some(NodeId(3))))
        );
        // After the last child: appended.
        assert_eq!(
            drop_target(&tree, &nodes, &d, NodeId(3), DropHint::After),
            Some((Some(NodeId(1)), None))
        );
        // The next sibling is skipped when it is dragged too.
        let both = [NodeId(5), NodeId(3)];
        assert_eq!(
            drop_target(&tree, &nodes, &both, NodeId(2), DropHint::After),
            Some((Some(NodeId(1)), None))
        );
        assert_eq!(
            drop_target(&tree, &nodes, &d, NodeId(5), DropHint::Before),
            None
        );
    }
}
