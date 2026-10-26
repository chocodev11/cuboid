//! Groups: the outliner tree over the world-space objects (spec: hierarchy A1 2-3).
//!
//! Elements and meshes keep their world transforms, so nothing that reads them changes. A group
//! adds a name, a pivot, a rotation and flags, and lists its children in outliner order; the
//! parent of a node is the group that lists it. The top level is the explicit prefix `roots`
//! plus the elements and meshes that no group lists, in list order, so an object created by a
//! command lands at the end of the top level without a write to the groups.
//!
//! Group ids come from the same counter as object ids, so an id is never issued twice in a
//! process and a removed group's id stays dead.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use cuboid_num::{dquat_to_euler, euler_to_dquat};
use glam::{DQuat, DVec3, Quat, Vec3};
use rustc_hash::{FxHashMap, FxHashSet};
use thiserror::Error;

use crate::contract::Document;
use crate::ids::{ObjId, ObjRef, Rev};
use crate::journal::{ChangeFlags, ChangeJournal, DocFlags};
use crate::list::ObjList;
use crate::objects::{Element, Label, Mesh};
use crate::selection::Selection;
use crate::snapshot::GroupsSnap;

/// Identity of a group. It is an [`ObjId`], so `ObjRef::Group(id)` names it.
pub type GroupId = ObjId;

/// The most groups a document holds.
pub const MAX_GROUPS: usize = 65_536;

/// The deepest nesting: a top-level group is level 1.
pub const MAX_DEPTH: usize = 64;

/// Why a change of the tree was refused. The document is untouched in every case.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Error)]
pub enum GroupError {
    /// The document would hold more groups than [`MAX_GROUPS`].
    #[error("a model holds at most 65536 groups")]
    TooMany,
    /// Groups would nest deeper than [`MAX_DEPTH`].
    #[error("groups nest at most 64 levels deep")]
    TooDeep,
    /// A group would go into itself or into one of its own groups.
    #[error("a group cannot go into itself or into one of its own groups")]
    Cycle,
    /// A group or an object is not in the model.
    #[error("the group or object is not in the model")]
    Missing,
    /// A node is listed by two groups.
    #[error("an object is in two groups")]
    Attached,
}

/// One group: a named pivot with a rotation, a flag set and an ordered list of children.
///
/// `origin` is the pivot at rest in world (model) space, the space of the object positions.
/// `rotation` is relative to the parent group, in the Euler convention of an element's rotation.
/// The `.bbmodel` writer turns both into the local numbers of its file; the world transforms of
/// the objects are never derived from them.
///
/// spec: hierarchy A1 3
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// Display name.
    pub name: Label,
    /// The pivot at rest, in world space.
    pub origin: Vec3,
    /// The rotation relative to the parent group, Euler degrees.
    pub rotation: Vec3,
    /// Shown in the views; a hidden group hides everything below it.
    pub visible: bool,
    /// Protected from drags; a locked group locks everything below it.
    pub locked: bool,
    /// Included in exports that know groups.
    pub export: bool,
    /// The identity a `.bbmodel` project gave the group; animations refer to it.
    pub uuid: Option<String>,
    children: Vec<ObjRef>,
}

impl Group {
    /// A visible, unlocked, exported group without a rotation and without children.
    pub fn new(name: impl Into<Label>, origin: Vec3) -> Self {
        Self {
            name: name.into(),
            origin,
            rotation: Vec3::ZERO,
            visible: true,
            locked: false,
            export: true,
            uuid: None,
            children: Vec::new(),
        }
    }

    /// The same group listing `children`, for a group that [`Document::graft`] adds.
    pub fn with_children(mut self, children: Vec<ObjRef>) -> Self {
        self.children = children;
        self
    }

    /// The children in outliner order: groups, elements and meshes.
    pub fn children(&self) -> &[ObjRef] {
        &self.children
    }
}

/// What the lookups of a tree need, built on first use after a change.
#[derive(Clone, Debug, Default)]
struct Index {
    parent: FxHashMap<ObjRef, GroupId>,
    /// Nodes with a hidden group above them.
    hidden: FxHashSet<ObjRef>,
    /// Nodes with a locked group above them.
    locked: FxHashSet<ObjRef>,
}

/// The groups of a document with the order of the top level.
///
/// spec: hierarchy A1 3
#[derive(Clone, Debug, Default)]
pub struct Groups {
    list: ObjList<Group>,
    roots: Vec<ObjRef>,
    index: OnceLock<Index>,
}

/// The accumulated frame of a group, for turning world data into the local numbers of a file. It
/// is kept in f64: a chain of rotations must not turn a rotation that is none into a small one.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GroupFrame {
    /// The rotation of the group and all its ancestors.
    pub rot: DQuat,
    /// The pivot in world space.
    pub origin: DVec3,
    /// The pivot in the coordinates the group's own children are written in.
    pub local_origin: DVec3,
}

impl GroupFrame {
    fn local_point(&self, world: DVec3) -> DVec3 {
        self.local_origin + self.rot.inverse() * (world - self.origin)
    }

    /// A world point in the coordinates of the group's children.
    pub fn to_local(&self, world: Vec3) -> Vec3 {
        self.local_point(world.as_dvec3()).as_vec3()
    }

    /// The position, pivot and Euler rotation of an object in the coordinates of the group's
    /// children. Without a rotation in the groups above, the angles are not touched.
    pub fn local_fields(&self, p: Vec3, o: Vec3, e: Vec3) -> (Vec3, Vec3, Vec3) {
        let rotation = if self.rot == DQuat::IDENTITY {
            e
        } else {
            dquat_to_euler(self.rot.inverse() * euler_to_dquat(e))
        };
        (self.to_local(p), self.to_local(o), rotation)
    }
}

fn rotation_of(euler: Vec3) -> DQuat {
    if euler == Vec3::ZERO {
        DQuat::IDENTITY
    } else {
        euler_to_dquat(euler)
    }
}

impl Groups {
    /// Number of groups.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether the document has no group.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The group ids in list order.
    pub fn ids(&self) -> &[GroupId] {
        self.list.ids()
    }

    /// The group records in list order, parallel to [`Groups::ids`].
    pub fn items(&self) -> &[Arc<Group>] {
        self.list.items()
    }

    /// The group with `id`.
    pub fn get(&self, id: GroupId) -> Option<&Group> {
        self.list.get(id)
    }

    /// The position of `id` in list order.
    pub fn position(&self, id: GroupId) -> Option<usize> {
        self.list.position(id)
    }

    /// Changes whenever a group is added, removed, edited or moved in the tree.
    pub fn rev(&self) -> Rev {
        self.list.rev()
    }

    /// Changes whenever a group is added or removed.
    pub fn structure_rev(&self) -> Rev {
        self.list.structure_rev()
    }

    /// The explicit top level: groups and objects in outliner order. Objects that no group lists
    /// and that are missing here follow in list order; see [`Document::root_nodes`].
    pub fn roots(&self) -> &[ObjRef] {
        &self.roots
    }

    /// Mutable access to a group, with a journal entry. The children cannot be reached this
    /// way: they change through the tree operations.
    pub fn edit(
        &mut self,
        id: GroupId,
        flags: ChangeFlags,
        journal: &mut ChangeJournal,
    ) -> Option<&mut Group> {
        self.index = OnceLock::new();
        self.list.edit_marked(id, flags, journal)
    }

    fn index(&self) -> &Index {
        self.index.get_or_init(|| {
            let mut parent: FxHashMap<ObjRef, GroupId> = FxHashMap::default();
            for (&id, group) in self.list.ids().iter().zip(self.list.items()) {
                for &child in &group.children {
                    parent.insert(child, id);
                }
            }
            let mut hidden = FxHashSet::default();
            let mut locked = FxHashSet::default();
            let mut seen: FxHashSet<GroupId> = FxHashSet::default();
            let mut stack: Vec<(GroupId, bool, bool)> = self
                .list
                .ids()
                .iter()
                .filter(|&&id| !parent.contains_key(&ObjRef::Group(id)))
                .map(|&id| (id, false, false))
                .collect();
            while let Some((id, above_hidden, above_locked)) = stack.pop() {
                let Some(group) = self.list.get(id).filter(|_| seen.insert(id)) else {
                    continue;
                };
                let hide = above_hidden || !group.visible;
                let lock = above_locked || group.locked;
                for &child in &group.children {
                    if hide {
                        hidden.insert(child);
                    }
                    if lock {
                        locked.insert(child);
                    }
                    if let ObjRef::Group(inner) = child {
                        stack.push((inner, hide, lock));
                    }
                }
            }
            Index {
                parent,
                hidden,
                locked,
            }
        })
    }

    /// The group that lists `obj`.
    pub fn parent_of(&self, obj: ObjRef) -> Option<GroupId> {
        if self.list.is_empty() {
            return None;
        }
        self.index().parent.get(&obj).copied()
    }

    /// The groups above `obj`, the nearest first. Stops after more than [`MAX_DEPTH`] levels,
    /// which also ends a cycle in damaged data.
    pub fn ancestors(&self, obj: ObjRef) -> impl Iterator<Item = GroupId> + '_ {
        let mut current = obj;
        let mut steps = 0;
        std::iter::from_fn(move || {
            if steps > MAX_DEPTH {
                return None;
            }
            let parent = self.parent_of(current)?;
            steps += 1;
            current = ObjRef::Group(parent);
            Some(parent)
        })
    }

    /// The level of a group: 1 for a top-level group.
    pub fn depth(&self, group: GroupId) -> usize {
        1 + self.ancestors(ObjRef::Group(group)).count()
    }

    /// The levels from `group` down to its deepest descendant group: 1 for a group without
    /// groups inside.
    pub fn height(&self, group: GroupId) -> usize {
        let mut deepest = 0;
        let mut budget = self.list.len() + 1;
        let mut stack = vec![(group, 1_usize)];
        while let Some((id, level)) = stack.pop() {
            budget = budget.saturating_sub(1);
            if budget == 0 {
                break;
            }
            deepest = deepest.max(level);
            if let Some(g) = self.get(id) {
                for &child in &g.children {
                    if let ObjRef::Group(inner) = child {
                        stack.push((inner, level + 1));
                    }
                }
            }
        }
        deepest
    }

    /// Whether a group above `obj` is hidden.
    pub fn hidden_by_group(&self, obj: ObjRef) -> bool {
        !self.list.is_empty() && self.index().hidden.contains(&obj)
    }

    /// Whether a group above `obj` is locked.
    pub fn locked_by_group(&self, obj: ObjRef) -> bool {
        !self.list.is_empty() && self.index().locked.contains(&obj)
    }

    /// Everything below the groups: the children of a group together, a group before the
    /// groups inside it. The groups themselves are not in the result unless another group
    /// lists them, and a node reached twice is listed once.
    pub fn descendants(&self, groups: &[GroupId]) -> Vec<ObjRef> {
        let mut out = Vec::new();
        let mut seen: FxHashSet<ObjRef> = FxHashSet::default();
        let mut stack: Vec<GroupId> = groups.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(group) = self.get(id) else {
                continue;
            };
            for &child in &group.children {
                if seen.insert(child) {
                    out.push(child);
                }
            }
            for &child in group.children.iter().rev() {
                if let ObjRef::Group(inner) = child {
                    stack.push(inner);
                }
            }
        }
        out
    }

    /// The group ids among `groups` and below them, each group before the groups inside it.
    pub fn subtree_groups(&self, groups: &[GroupId]) -> Vec<GroupId> {
        let mut out: Vec<GroupId> = groups.to_vec();
        out.extend(
            self.descendants(groups)
                .into_iter()
                .filter_map(|obj| match obj {
                    ObjRef::Group(id) => Some(id),
                    _ => None,
                }),
        );
        let mut seen = FxHashSet::default();
        out.retain(|id| seen.insert(*id));
        out
    }

    /// The rotation of `group` and all its ancestors; the identity for the top level.
    pub fn world_rotation(&self, group: Option<GroupId>) -> Quat {
        self.world_drotation(group).as_quat()
    }

    fn world_drotation(&self, group: Option<GroupId>) -> DQuat {
        let Some(group) = group else {
            return DQuat::IDENTITY;
        };
        let chain: Vec<GroupId> = std::iter::once(group)
            .chain(self.ancestors(ObjRef::Group(group)))
            .collect();
        chain.iter().rev().fold(DQuat::IDENTITY, |acc, id| {
            acc * self
                .get(*id)
                .map_or(DQuat::IDENTITY, |g| rotation_of(g.rotation))
        })
    }

    /// The frame of every group, parents before children.
    pub fn frames(&self) -> HashMap<GroupId, GroupFrame> {
        let mut frames: HashMap<GroupId, GroupFrame> = HashMap::new();
        let mut stack: Vec<(GroupId, Option<GroupId>)> = self
            .list
            .ids()
            .iter()
            .filter(|&&id| self.parent_of(ObjRef::Group(id)).is_none())
            .map(|&id| (id, None))
            .collect();
        while let Some((id, parent)) = stack.pop() {
            let Some(group) = self.get(id) else {
                continue;
            };
            let own = rotation_of(group.rotation);
            let origin = group.origin.as_dvec3();
            let frame = match parent.and_then(|p| frames.get(&p)) {
                Some(above) => GroupFrame {
                    rot: above.rot * own,
                    origin,
                    local_origin: above.local_point(origin),
                },
                None => GroupFrame {
                    rot: own,
                    origin,
                    local_origin: origin,
                },
            };
            if frames.insert(id, frame).is_some() {
                continue;
            }
            for &child in &group.children {
                if let ObjRef::Group(inner) = child {
                    stack.push((inner, Some(id)));
                }
            }
        }
        frames
    }

    /// The captured groups with the selection.
    pub fn snapshot(&self, sel: &Selection) -> GroupsSnap {
        GroupsSnap {
            list: self.list.snapshot(),
            roots: self.roots.clone(),
            selection: sel.list.iter().copied().collect(),
        }
    }

    /// The groups of a snapshot, keeping their ids.
    pub fn from_snap(snap: &GroupsSnap) -> Groups {
        let mut groups = Groups::default();
        groups.restore(snap);
        groups
    }

    /// Replaces the groups by a snapshot, keeping its ids.
    pub fn restore(&mut self, snap: &GroupsSnap) {
        self.list.restore(&snap.list);
        self.roots.clone_from(&snap.roots);
        self.index = OnceLock::new();
    }

    /// Checks the shape of the tree: at most [`MAX_GROUPS`] groups, every group and every
    /// listed node in exactly one list, groups only below existing groups, no cycle and at most
    /// [`MAX_DEPTH`] levels. Whether the listed elements and meshes exist is a question for the
    /// document ([`Document::check_groups`]).
    pub fn validate(&self) -> Result<(), String> {
        if self.list.len() > MAX_GROUPS {
            return Err(format!("the model has more than {MAX_GROUPS} groups"));
        }
        let mut listed: FxHashSet<ObjRef> = FxHashSet::default();
        let lists = std::iter::once(self.roots.as_slice())
            .chain(self.list.items().iter().map(|g| g.children.as_slice()));
        for nodes in lists {
            for &node in nodes {
                if !listed.insert(node) {
                    return Err("a group or object is listed twice in the tree".to_owned());
                }
                match node {
                    ObjRef::Group(id) if self.list.get(id).is_none() => {
                        return Err("a group lists a group that does not exist".to_owned());
                    }
                    ObjRef::Blueprint | ObjRef::ModelOrigin => {
                        return Err("the blueprint or the model origin is in a group".to_owned());
                    }
                    _ => {}
                }
            }
        }
        if self
            .list
            .ids()
            .iter()
            .any(|&id| !listed.contains(&ObjRef::Group(id)))
        {
            return Err("a group is not in the tree".to_owned());
        }
        let mut reached = 0_usize;
        let mut stack: Vec<(ObjRef, usize)> = self.roots.iter().map(|&n| (n, 1)).collect();
        while let Some((node, level)) = stack.pop() {
            let ObjRef::Group(id) = node else {
                continue;
            };
            if level > MAX_DEPTH {
                return Err(format!("groups nest deeper than {MAX_DEPTH} levels"));
            }
            reached += 1;
            if let Some(group) = self.list.get(id) {
                stack.extend(group.children.iter().map(|&n| (n, level + 1)));
            }
        }
        if reached != self.list.len() {
            return Err("the groups form a cycle".to_owned());
        }
        Ok(())
    }

    /// Drops the elements and meshes that no longer exist from every list, and the groups
    /// that no longer exist. Returns whether anything changed.
    pub(crate) fn retain_live(
        &mut self,
        elements: &ObjList<Element>,
        meshes: &ObjList<Mesh>,
    ) -> bool {
        let live = |node: &ObjRef| match *node {
            ObjRef::Element(id) => elements.position(id).is_some(),
            ObjRef::Mesh(id) => meshes.position(id).is_some(),
            ObjRef::Group(id) => self.list.position(id).is_some(),
            ObjRef::Blueprint | ObjRef::ModelOrigin => false,
        };
        let dead: Vec<GroupId> = self
            .list
            .ids()
            .iter()
            .zip(self.list.items())
            .filter(|(_, group)| group.children.iter().any(|c| !live(c)))
            .map(|(&id, _)| id)
            .collect();
        let roots_dead = self.roots.iter().any(|n| !live(n));
        if dead.is_empty() && !roots_dead {
            return false;
        }
        let alive: FxHashSet<ObjRef> = self
            .list
            .items()
            .iter()
            .flat_map(|g| g.children.iter().copied())
            .chain(self.roots.iter().copied())
            .filter(|n| live(n))
            .collect();
        let mut scratch = ChangeJournal::default();
        for id in dead {
            if let Some(group) = self.list.edit_marked(id, ChangeFlags::ORDER, &mut scratch) {
                group.children.retain(|c| alive.contains(c));
            }
        }
        self.roots.retain(|n| alive.contains(n));
        self.index = OnceLock::new();
        true
    }

    fn children_mut(&mut self, id: GroupId) -> Option<&mut Vec<ObjRef>> {
        self.index = OnceLock::new();
        let mut scratch = ChangeJournal::default();
        self.list
            .edit_marked(id, ChangeFlags::ORDER, &mut scratch)
            .map(|g| &mut g.children)
    }

    /// Replaces the child list of a group, or the explicit top level for `None`.
    fn set_children(&mut self, parent: Option<GroupId>, nodes: Vec<ObjRef>) {
        match parent {
            Some(id) => {
                if let Some(children) = self.children_mut(id) {
                    *children = nodes;
                }
            }
            None => {
                self.index = OnceLock::new();
                self.roots = nodes;
            }
        }
    }

    /// Takes `nodes` out of the lists that hold them.
    fn detach(&mut self, nodes: &FxHashSet<ObjRef>) {
        let mut parents: Vec<Option<GroupId>> = nodes.iter().map(|&n| self.parent_of(n)).collect();
        parents.sort_unstable();
        parents.dedup();
        for parent in parents {
            match parent {
                Some(id) => {
                    if let Some(children) = self.children_mut(id) {
                        children.retain(|c| !nodes.contains(c));
                    }
                }
                None => self.roots.retain(|c| !nodes.contains(c)),
            }
        }
        self.index = OnceLock::new();
    }

    /// Drops the records of the groups and takes them out of the lists of the groups that stay.
    /// Whatever the removed groups listed is not touched: the caller removes or moves it.
    pub fn remove(&mut self, ids: &FxHashSet<GroupId>) {
        if ids.is_empty() {
            return;
        }
        let nodes: FxHashSet<ObjRef> = ids.iter().map(|&id| ObjRef::Group(id)).collect();
        self.detach(&nodes);
        self.list.remove_set(ids);
        self.index = OnceLock::new();
    }
}

/// Builds the tree of a loader node by node, with the bounds of the model, without the cost of
/// a lookup rebuild per node.
#[derive(Default)]
pub struct GroupsBuilder {
    groups: Groups,
    attached: FxHashSet<ObjRef>,
    depth: FxHashMap<GroupId, usize>,
    scratch: ChangeJournal,
}

impl GroupsBuilder {
    /// An empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a group to the children of `parent` (the top level for `None`). The children
    /// field of `group` is ignored.
    pub fn add_group(
        &mut self,
        mut group: Group,
        parent: Option<GroupId>,
    ) -> Result<GroupId, GroupError> {
        if self.groups.list.len() >= MAX_GROUPS {
            return Err(GroupError::TooMany);
        }
        let depth = match parent {
            Some(p) => self.depth.get(&p).ok_or(GroupError::Missing)? + 1,
            None => 1,
        };
        if depth > MAX_DEPTH {
            return Err(GroupError::TooDeep);
        }
        group.children = Vec::new();
        let id = self.groups.list.push(group);
        self.depth.insert(id, depth);
        self.attached.insert(ObjRef::Group(id));
        self.link(ObjRef::Group(id), parent)?;
        Ok(id)
    }

    /// Appends an element or a mesh to the children of `parent`.
    pub fn add_object(&mut self, obj: ObjRef, parent: Option<GroupId>) -> Result<(), GroupError> {
        if !matches!(obj, ObjRef::Element(_) | ObjRef::Mesh(_)) {
            return Err(GroupError::Missing);
        }
        if let Some(p) = parent
            && !self.depth.contains_key(&p)
        {
            return Err(GroupError::Missing);
        }
        if !self.attached.insert(obj) {
            return Err(GroupError::Attached);
        }
        self.link(obj, parent)
    }

    fn link(&mut self, obj: ObjRef, parent: Option<GroupId>) -> Result<(), GroupError> {
        match parent {
            Some(id) => self
                .groups
                .list
                .edit_marked(id, ChangeFlags::ORDER, &mut self.scratch)
                .ok_or(GroupError::Missing)?
                .children
                .push(obj),
            None => self.groups.roots.push(obj),
        }
        Ok(())
    }

    /// Number of groups so far.
    pub fn len(&self) -> usize {
        self.groups.list.len()
    }

    /// Whether no group was added.
    pub fn is_empty(&self) -> bool {
        self.groups.list.is_empty()
    }

    /// The finished tree.
    pub fn finish(self) -> Groups {
        self.groups
    }
}

impl Document {
    /// Whether the element, mesh or group exists.
    pub fn has_node(&self, obj: ObjRef) -> bool {
        match obj {
            ObjRef::Element(id) => self.elements.position(id).is_some(),
            ObjRef::Mesh(id) => self.meshes.position(id).is_some(),
            ObjRef::Group(id) => self.groups.position(id).is_some(),
            ObjRef::Blueprint | ObjRef::ModelOrigin => true,
        }
    }

    /// The top level of the outliner: the explicit prefix, then the groups that are not in it,
    /// then the elements and meshes without a group that are not in it, in list order.
    pub fn root_nodes(&self) -> Vec<ObjRef> {
        let mut out: Vec<ObjRef> = self
            .groups
            .roots
            .iter()
            .copied()
            .filter(|&n| self.has_node(n))
            .collect();
        if self.groups.is_empty() && out.is_empty() {
            out.extend(self.elements.ids().iter().map(|&id| ObjRef::Element(id)));
            out.extend(self.meshes.ids().iter().map(|&id| ObjRef::Mesh(id)));
            return out;
        }
        let listed: FxHashSet<ObjRef> = out.iter().copied().collect();
        let top = |n: &ObjRef| !listed.contains(n) && self.groups.parent_of(*n).is_none();
        let rest: Vec<ObjRef> = self
            .groups
            .ids()
            .iter()
            .map(|&id| ObjRef::Group(id))
            .chain(self.elements.ids().iter().map(|&id| ObjRef::Element(id)))
            .chain(self.meshes.ids().iter().map(|&id| ObjRef::Mesh(id)))
            .filter(top)
            .collect();
        out.extend(rest);
        out
    }

    /// The children of a group in outliner order, or the top level for `None`.
    pub fn child_nodes(&self, parent: Option<GroupId>) -> Vec<ObjRef> {
        match parent {
            Some(id) => self.groups.get(id).map_or_else(Vec::new, |g| {
                g.children
                    .iter()
                    .copied()
                    .filter(|&n| self.has_node(n))
                    .collect()
            }),
            None => self.root_nodes(),
        }
    }

    /// Every node with the group that lists it, parents before children, in outliner order.
    pub fn outline(&self) -> Vec<(ObjRef, Option<GroupId>)> {
        let total = self.groups.len() + self.elements.len() + self.meshes.len();
        let mut out = Vec::with_capacity(total);
        let mut stack: Vec<(ObjRef, Option<GroupId>)> = self
            .root_nodes()
            .into_iter()
            .rev()
            .map(|n| (n, None))
            .collect();
        while let Some((node, parent)) = stack.pop() {
            if out.len() >= total {
                break;
            }
            out.push((node, parent));
            if let ObjRef::Group(id) = node {
                stack.extend(
                    self.child_nodes(Some(id))
                        .into_iter()
                        .rev()
                        .map(|n| (n, Some(id))),
                );
            }
        }
        out
    }

    /// Whether the node is hidden: by its own flag or by a hidden group above it.
    pub fn is_hidden(&self, obj: ObjRef) -> bool {
        let own = match obj {
            ObjRef::Element(id) => self.elements.get(id).is_some_and(|e| e.base.hidden),
            ObjRef::Mesh(id) => self.meshes.get(id).is_some_and(|m| m.base.hidden),
            ObjRef::Group(id) => self.groups.get(id).is_some_and(|g| !g.visible),
            ObjRef::Blueprint => self.blueprint.base.hidden,
            ObjRef::ModelOrigin => false,
        };
        own || self.groups.hidden_by_group(obj)
    }

    /// Whether the node is locked: by its own flag or by a locked group above it.
    pub fn is_locked(&self, obj: ObjRef) -> bool {
        let own = match obj {
            ObjRef::Element(id) => self.elements.get(id).is_some_and(|e| e.base.locked),
            ObjRef::Mesh(id) => self.meshes.get(id).is_some_and(|m| m.base.locked),
            ObjRef::Group(id) => self.groups.get(id).is_some_and(|g| g.locked),
            ObjRef::Blueprint => self.blueprint.base.locked,
            ObjRef::ModelOrigin => false,
        };
        own || self.groups.locked_by_group(obj)
    }

    /// Checks the tree and that every listed element and mesh exists.
    pub fn check_groups(&self) -> Result<(), String> {
        self.groups.validate()?;
        let dead = std::iter::once(self.groups.roots.as_slice())
            .chain(self.groups.items().iter().map(|g| g.children.as_slice()))
            .flatten()
            .any(|&n| !self.has_node(n));
        if dead {
            return Err("a group lists an object that does not exist".to_owned());
        }
        Ok(())
    }

    /// Drops what the groups list but the document no longer has (after objects were deleted).
    pub fn prune_groups(&mut self) {
        if self.groups.is_empty() && self.groups.roots.is_empty() {
            return;
        }
        if self.groups.retain_live(&self.elements, &self.meshes) {
            self.journal.mark_doc(DocFlags::GROUPS | DocFlags::LISTS);
            self.revision = Rev::next();
        }
    }

    /// Keeps the accumulated rotation of moved groups: a group that changes its parent chain
    /// gets the local rotation that leaves its frame where it was in the world.
    fn keep_world_rotations(&mut self, moved: &[ObjRef], new_parent: Option<GroupId>) {
        let target = self.groups.world_drotation(new_parent);
        let mut turned: Vec<(GroupId, Vec3)> = Vec::new();
        for &node in moved {
            let ObjRef::Group(id) = node else {
                continue;
            };
            let old_parent = self.groups.parent_of(node);
            if old_parent == new_parent {
                continue;
            }
            let before = self.groups.world_drotation(old_parent);
            if before.abs_diff_eq(target, 1e-9) {
                continue;
            }
            let Some(group) = self.groups.get(id) else {
                continue;
            };
            let local = target.inverse() * before * rotation_of(group.rotation);
            turned.push((id, dquat_to_euler(local.normalize())));
        }
        for (id, rotation) in turned {
            if let Some(group) = self
                .groups
                .edit(id, ChangeFlags::TRANSFORM, &mut self.journal)
            {
                group.rotation = rotation;
            }
        }
    }

    fn touched(&mut self) {
        self.journal.mark_doc(DocFlags::GROUPS | DocFlags::LISTS);
        self.revision = Rev::next();
    }

    /// Moves nodes into a group, or to the top level for `None`, at `index` among the children
    /// that stay (the end when `None`). The nodes keep their relative order. A group keeps its
    /// place in the world: its local rotation changes when the groups above it do. Returns how
    /// many nodes changed their place.
    pub fn move_nodes(
        &mut self,
        nodes: &[ObjRef],
        parent: Option<GroupId>,
        index: Option<usize>,
    ) -> Result<usize, GroupError> {
        let mut seen = FxHashSet::default();
        let moved: Vec<ObjRef> = nodes
            .iter()
            .copied()
            .filter(|&n| matches!(n, ObjRef::Element(_) | ObjRef::Mesh(_) | ObjRef::Group(_)))
            .filter(|&n| seen.insert(n))
            .collect();
        if moved.iter().any(|&n| !self.has_node(n)) {
            return Err(GroupError::Missing);
        }
        if parent.is_some_and(|p| self.groups.get(p).is_none()) {
            return Err(GroupError::Missing);
        }
        let base_depth = parent.map_or(0, |p| self.groups.depth(p));
        for &node in &moved {
            let ObjRef::Group(group) = node else {
                continue;
            };
            let inside = parent == Some(group)
                || parent
                    .is_some_and(|p| self.groups.ancestors(ObjRef::Group(p)).any(|a| a == group));
            if inside {
                return Err(GroupError::Cycle);
            }
            if base_depth + self.groups.height(group) > MAX_DEPTH {
                return Err(GroupError::TooDeep);
            }
        }

        let moved_set: FxHashSet<ObjRef> = moved.iter().copied().collect();
        let old_order = self.child_nodes(parent);
        let mut order = old_order.clone();
        order.retain(|n| !moved_set.contains(n));
        let at = index.map_or(order.len(), |i| i.min(order.len()));
        order.splice(at..at, moved.iter().copied());
        let in_place =
            order == old_order && moved.iter().all(|&n| self.groups.parent_of(n) == parent);
        if in_place {
            return Ok(0);
        }

        self.keep_world_rotations(&moved, parent);
        self.groups.detach(&moved_set);
        self.groups.set_children(parent, order);
        self.touched();
        Ok(moved.len())
    }

    /// Adds `group` where the first of `nodes` (in outliner order) is and moves the nodes into
    /// it. The nodes must not contain one another.
    pub fn insert_group(&mut self, group: Group, nodes: &[ObjRef]) -> Result<GroupId, GroupError> {
        if self.groups.len() >= MAX_GROUPS {
            return Err(GroupError::TooMany);
        }
        let mut seen = FxHashSet::default();
        let picked: Vec<ObjRef> = nodes.iter().copied().filter(|&n| seen.insert(n)).collect();
        if picked.is_empty() || picked.iter().any(|&n| !self.has_node(n)) {
            return Err(GroupError::Missing);
        }
        let rank: FxHashMap<ObjRef, usize> = self
            .outline()
            .into_iter()
            .enumerate()
            .map(|(at, (node, _))| (node, at))
            .collect();
        let mut ordered = picked;
        ordered.sort_by_key(|n| rank.get(n).copied().unwrap_or(usize::MAX));
        let first = ordered[0];
        let parent = self.groups.parent_of(first);
        let level = parent.map_or(0, |p| self.groups.depth(p)) + 1;
        let deepest = ordered
            .iter()
            .filter_map(|&n| match n {
                ObjRef::Group(id) => Some(self.groups.height(id)),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        if level + deepest > MAX_DEPTH {
            return Err(GroupError::TooDeep);
        }

        let mut order = self.child_nodes(parent);
        let at = order
            .iter()
            .position(|&n| n == first)
            .unwrap_or(order.len());
        let id = self.groups.list.push(Group {
            children: Vec::new(),
            ..group
        });
        order.insert(at, ObjRef::Group(id));
        self.groups.set_children(parent, order);
        match self.move_nodes(&ordered, Some(id), None) {
            Ok(_) => Ok(id),
            Err(error) => {
                self.groups.remove(&FxHashSet::from_iter([id]));
                Err(error)
            }
        }
    }

    /// Removes a group and puts its children where it was, in order. Child groups keep their
    /// place in the world. Returns the children.
    pub fn dissolve_group(&mut self, id: GroupId) -> Result<Vec<ObjRef>, GroupError> {
        let group = self.groups.get(id).ok_or(GroupError::Missing)?;
        let children = group.children.clone();
        let parent = self.groups.parent_of(ObjRef::Group(id));
        self.keep_world_rotations(&children, parent);
        let mut order = self.child_nodes(parent);
        let at = order
            .iter()
            .position(|&n| n == ObjRef::Group(id))
            .unwrap_or(order.len());
        order.splice(at..(at + 1).min(order.len()), children.iter().copied());
        self.groups.set_children(parent, order);
        self.groups.list.remove_set(&FxHashSet::from_iter([id]));
        self.groups.index = OnceLock::new();
        self.touched();
        Ok(children)
    }

    /// Adds ready-made groups with the ids they were given and lists `top` (nodes among them,
    /// or objects that no group lists) at `index` among the children of `parent`. A duplicate of
    /// a subtree is added this way. The tree is checked afterwards and left as it was when it
    /// would be malformed.
    pub fn graft(
        &mut self,
        groups: Vec<(GroupId, Group)>,
        top: &[ObjRef],
        parent: Option<GroupId>,
        index: Option<usize>,
    ) -> Result<(), GroupError> {
        if self.groups.len() + groups.len() > MAX_GROUPS {
            return Err(GroupError::TooMany);
        }
        if parent.is_some_and(|p| self.groups.get(p).is_none()) {
            return Err(GroupError::Missing);
        }
        let saved = self.groups.clone();
        let mut snap = self.groups.list.snapshot();
        for (id, group) in groups {
            snap.ids.push(id);
            snap.items.push(Arc::new(group));
        }
        self.groups.list.restore(&snap);
        self.groups.index = OnceLock::new();
        // The new nodes are listed by their groups now, or are the nodes to place.
        let mut order = self.child_nodes(parent);
        order.retain(|n| !top.contains(n));
        let at = index.map_or(order.len(), |i| i.min(order.len()));
        order.splice(at..at, top.iter().copied());
        self.groups.set_children(parent, order);
        if self.check_groups().is_err() {
            self.groups = saved;
            return Err(GroupError::Attached);
        }
        self.touched();
        Ok(())
    }

    /// Drops group records. See [`Groups::remove`].
    pub fn remove_groups(&mut self, ids: &FxHashSet<GroupId>) {
        if ids.is_empty() {
            return;
        }
        self.groups.remove(ids);
        self.touched();
    }
}

/// The groups of a snapshot with their records, for the apply path.
pub(crate) fn snap_groups(snap: &GroupsSnap) -> Result<Groups, String> {
    if snap.list.ids.len() != snap.list.items.len() {
        return Err(format!(
            "the group section has {} ids and {} records",
            snap.list.ids.len(),
            snap.list.items.len()
        ));
    }
    let mut ids = FxHashSet::default();
    if !snap.list.ids.iter().all(|id| ids.insert(*id)) {
        return Err("the group section repeats a group id".to_owned());
    }
    let groups = Groups::from_snap(snap);
    groups.validate()?;
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setters::fixture::World;

    fn el(id: ObjId) -> ObjRef {
        ObjRef::Element(id)
    }

    fn g(id: GroupId) -> ObjRef {
        ObjRef::Group(id)
    }

    /// Four cubes, group A with cubes 0 and 1, group B (inside A) with cube 2, cube 3 loose.
    fn tree() -> (World, Vec<ObjId>, GroupId, GroupId) {
        let mut w = World::new();
        let ids: Vec<ObjId> = (0..4)
            .map(|i| w.add_element(Vec3::new(i as f32 * 4.0, 0.0, 0.0), Vec3::ONE))
            .collect();
        let mut b = GroupsBuilder::new();
        let a = b.add_group(Group::new("A", Vec3::ZERO), None).unwrap();
        b.add_object(el(ids[0]), Some(a)).unwrap();
        let inner = b.add_group(Group::new("B", Vec3::X), Some(a)).unwrap();
        b.add_object(el(ids[2]), Some(inner)).unwrap();
        b.add_object(el(ids[1]), Some(a)).unwrap();
        w.doc.groups = b.finish();
        (w, ids, a, inner)
    }

    #[test]
    fn the_builder_makes_a_valid_tree_with_parent_links_and_order() {
        let (w, ids, a, inner) = tree();
        assert!(w.doc.check_groups().is_ok());
        assert_eq!(w.doc.groups.parent_of(el(ids[2])), Some(inner));
        assert_eq!(w.doc.groups.parent_of(g(inner)), Some(a));
        assert_eq!(w.doc.groups.parent_of(g(a)), None);
        assert_eq!(
            w.doc.child_nodes(Some(a)),
            [el(ids[0]), g(inner), el(ids[1])]
        );
        // The group is a top-level node, the loose cube follows it.
        assert_eq!(w.doc.root_nodes(), [g(a), el(ids[3])]);
        let outline: Vec<ObjRef> = w.doc.outline().into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            outline,
            [
                g(a),
                el(ids[0]),
                g(inner),
                el(ids[2]),
                el(ids[1]),
                el(ids[3])
            ]
        );
        assert_eq!(w.doc.groups.depth(inner), 2);
        assert_eq!(w.doc.groups.height(a), 2);
    }

    #[test]
    fn hidden_and_locked_groups_reach_everything_below() {
        let (mut w, ids, a, inner) = tree();
        assert!(!w.doc.is_hidden(el(ids[2])));
        let doc = &mut w.doc;
        doc.groups
            .edit(inner, ChangeFlags::VISIBILITY, &mut doc.journal)
            .unwrap()
            .visible = false;
        assert!(w.doc.is_hidden(el(ids[2])));
        assert!(!w.doc.is_hidden(el(ids[0])), "the sibling is not below B");
        assert!(w.doc.is_hidden(g(inner)));
        let doc = &mut w.doc;
        doc.groups
            .edit(a, ChangeFlags::VISIBILITY, &mut doc.journal)
            .unwrap()
            .locked = true;
        assert!(w.doc.is_locked(el(ids[2])) && w.doc.is_locked(g(inner)));
        assert!(!w.doc.is_locked(el(ids[3])));
        assert!(w.doc.groups.locked_by_group(el(ids[0])));
        assert!(
            !w.doc.groups.locked_by_group(g(a)),
            "a group's own flag is not 'above'"
        );
    }

    #[test]
    fn moving_nodes_orders_them_and_refuses_a_cycle() {
        let (mut w, ids, a, inner) = tree();
        // Cube 3 goes into A before cube 1.
        assert_eq!(w.doc.move_nodes(&[el(ids[3])], Some(a), Some(2)), Ok(1));
        assert_eq!(
            w.doc.child_nodes(Some(a)),
            [el(ids[0]), g(inner), el(ids[3]), el(ids[1])]
        );
        assert!(w.doc.check_groups().is_ok());
        // The same move again changes nothing.
        let rev = w.doc.revision;
        assert_eq!(w.doc.move_nodes(&[el(ids[3])], Some(a), Some(2)), Ok(0));
        assert_eq!(w.doc.revision, rev);
        // A into B, A into itself.
        assert_eq!(
            w.doc.move_nodes(&[g(a)], Some(inner), None),
            Err(GroupError::Cycle)
        );
        assert_eq!(
            w.doc.move_nodes(&[g(a)], Some(a), None),
            Err(GroupError::Cycle)
        );
        // B to the top level, first.
        assert_eq!(w.doc.move_nodes(&[g(inner)], None, Some(0)), Ok(1));
        assert_eq!(w.doc.root_nodes(), [g(inner), g(a)]);
        assert_eq!(w.doc.groups.parent_of(el(ids[2])), Some(inner));
        assert!(w.doc.check_groups().is_ok());
        assert_eq!(
            w.doc.move_nodes(&[el(ObjId::next())], None, None),
            Err(GroupError::Missing)
        );
    }

    #[test]
    fn grouping_puts_the_group_where_the_first_node_was() {
        let (mut w, ids, a, _) = tree();
        let made = w
            .doc
            .insert_group(Group::new("C", Vec3::ZERO), &[el(ids[1]), el(ids[0])])
            .unwrap();
        // Cube 0 comes first in the outliner, so the group takes its place in A.
        let kids = w.doc.child_nodes(Some(a));
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0], g(made));
        assert_eq!(w.doc.child_nodes(Some(made)), [el(ids[0]), el(ids[1])]);
        assert!(w.doc.check_groups().is_ok());
        // Grouping loose objects works at the top level.
        let top = w
            .doc
            .insert_group(Group::new("D", Vec3::ZERO), &[el(ids[3])])
            .unwrap();
        assert_eq!(w.doc.root_nodes(), [g(a), g(top)]);
    }

    #[test]
    fn dissolving_a_group_puts_its_children_in_its_place() {
        let (mut w, ids, a, inner) = tree();
        let kids = w.doc.dissolve_group(inner).unwrap();
        assert_eq!(kids, [el(ids[2])]);
        assert_eq!(
            w.doc.child_nodes(Some(a)),
            [el(ids[0]), el(ids[2]), el(ids[1])]
        );
        assert!(w.doc.groups.get(inner).is_none());
        assert!(w.doc.check_groups().is_ok());
        assert_eq!(w.doc.dissolve_group(inner), Err(GroupError::Missing));
    }

    #[test]
    fn a_moved_group_keeps_its_accumulated_rotation() {
        let (mut w, _, a, inner) = tree();
        let doc = &mut w.doc;
        doc.groups
            .edit(a, ChangeFlags::TRANSFORM, &mut doc.journal)
            .unwrap()
            .rotation = Vec3::new(0.0, 90.0, 0.0);
        doc.groups
            .edit(inner, ChangeFlags::TRANSFORM, &mut doc.journal)
            .unwrap()
            .rotation = Vec3::new(0.0, 30.0, 0.0);
        let before = w.doc.groups.world_rotation(Some(inner));
        w.doc.move_nodes(&[g(inner)], None, None).unwrap();
        let after = w.doc.groups.world_rotation(Some(inner));
        assert!(before.abs_diff_eq(after, 1e-4) || before.abs_diff_eq(-after, 1e-4));
        // The local rotation now holds the whole turn.
        let local = w.doc.groups.get(inner).unwrap().rotation;
        assert!((local.y - 120.0).abs() < 1e-2, "{local:?}");
    }

    #[test]
    fn the_tree_is_bounded_in_depth_and_count() {
        let mut b = GroupsBuilder::new();
        let mut parent = None;
        for level in 1..=MAX_DEPTH {
            parent = Some(b.add_group(Group::new("g", Vec3::ZERO), parent).unwrap());
            assert_eq!(b.len(), level);
        }
        assert_eq!(
            b.add_group(Group::new("deep", Vec3::ZERO), parent),
            Err(GroupError::TooDeep)
        );
        let mut many = GroupsBuilder::new();
        for _ in 0..MAX_GROUPS {
            many.add_group(Group::new("g", Vec3::ZERO), None).unwrap();
        }
        assert_eq!(
            many.add_group(Group::new("one more", Vec3::ZERO), None),
            Err(GroupError::TooMany)
        );
        assert!(many.finish().validate().is_ok());
    }

    #[test]
    fn validate_catches_duplicates_missing_groups_and_cycles() {
        let (w, ids, a, inner) = tree();
        let mut twice = w.doc.groups.clone();
        twice.roots.push(el(ids[0]));
        assert!(twice.validate().is_err(), "cube 0 is in A already");

        let mut lost = w.doc.groups.clone();
        lost.roots.retain(|n| *n != g(a));
        assert!(lost.validate().is_err(), "A is in no list");

        let mut ghost = w.doc.groups.clone();
        ghost.roots.push(g(ObjId::next()));
        assert!(ghost.validate().is_err());

        // A cycle: A lists B, B lists A, and neither is on the top level.
        let mut cycle = w.doc.groups.clone();
        cycle.roots.retain(|n| *n != g(a));
        cycle.children_mut(inner).unwrap().push(g(a));
        assert!(cycle.validate().is_err());
    }

    #[test]
    fn dead_objects_are_pruned_from_the_lists() {
        let (mut w, ids, a, _) = tree();
        w.doc
            .elements
            .remove_set(&FxHashSet::from_iter([ids[0], ids[3]]));
        assert!(w.doc.check_groups().is_err());
        w.doc.prune_groups();
        assert!(w.doc.check_groups().is_ok());
        assert!(!w.doc.child_nodes(Some(a)).contains(&el(ids[0])));
    }

    #[test]
    fn frames_turn_world_data_into_the_local_numbers_of_a_file() {
        let mut b = GroupsBuilder::new();
        let mut outer = Group::new("outer", Vec3::new(10.0, 0.0, 0.0));
        outer.rotation = Vec3::new(0.0, 90.0, 0.0);
        let outer = b.add_group(outer, None).unwrap();
        let inner = b
            .add_group(Group::new("inner", Vec3::new(10.0, 5.0, 0.0)), Some(outer))
            .unwrap();
        let groups = b.finish();
        let frames = groups.frames();
        let top = frames[&outer];
        assert_eq!(top.local_origin, DVec3::new(10.0, 0.0, 0.0));
        // A point one unit along +X of the world, seen from the group, lies on the group's
        // own axes turned by the inverse of its rotation.
        let local = top.to_local(Vec3::new(11.0, 0.0, 0.0));
        let turn = euler_to_dquat(Vec3::new(0.0, 90.0, 0.0)).inverse();
        let expected = (DVec3::new(10.0, 0.0, 0.0) + turn * DVec3::X).as_vec3();
        assert!((local - expected).length() < 1e-4);
        // The child's pivot is the child's local origin as seen from the parent.
        let child = frames[&inner];
        let pivot = top.to_local(Vec3::new(10.0, 5.0, 0.0));
        assert!((child.local_origin.as_vec3() - pivot).length() < 1e-4);
        assert!(
            child.rot.abs_diff_eq(top.rot, 1e-9),
            "the child has no rotation of its own"
        );
    }

    #[test]
    fn snapshots_restore_ids_order_and_the_selection() {
        let (mut w, ids, a, inner) = tree();
        w.select(&[g(inner), el(ids[0])]);
        let snap = w.doc.groups.snapshot(&w.sel);
        assert_eq!(snap.selection, [g(inner), el(ids[0])]);
        w.doc.move_nodes(&[g(inner)], None, Some(0)).unwrap();
        w.doc.groups.restore(&snap);
        assert_eq!(w.doc.groups.ids(), [a, inner]);
        assert_eq!(w.doc.groups.parent_of(g(inner)), Some(a));
        assert!(w.doc.check_groups().is_ok());
        assert!(snap_groups(&snap).is_ok());
        let mut short = snap.clone();
        short.list.ids.pop();
        assert!(snap_groups(&short).is_err());
    }
}
