//! Group operations: build and take apart the tree, change a group, delete and duplicate whole
//! subtrees, and the transforms that move, turn or scale one (spec: hierarchy A1 4).

mod pose;

use cuboid_model::{
    ChangeFlags, DocFlags, Document, EditCtx, Group, GroupError, GroupId, Label, ObjId, ObjRef, Rev,
};
use cuboid_num::engine_eq;
use glam::Vec3;
use rustc_hash::{FxHashMap, FxHashSet};

pub use pose::{Pose, RotationRefused};

use crate::OpError;

/// The fields of a group that one call changes; `None` keeps a field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupFields {
    /// The name; double quotes are dropped.
    pub name: Option<String>,
    /// The pivot. Only the pivot moves: the objects stay where they are.
    pub origin: Option<Vec3>,
    /// Shown in the views, with everything below.
    pub visible: Option<bool>,
    /// Protected from drags, with everything below.
    pub locked: Option<bool>,
    /// Included in exports that know groups.
    pub export: Option<bool>,
}

/// The objects among `objs`, each once, without those that have a group of `objs` above them: a
/// selected group holds its children already, so a transform of the selection moves them once.
pub fn uncovered(doc: &Document, objs: &[ObjRef]) -> Vec<ObjRef> {
    let chosen: FxHashSet<ObjRef> = objs.iter().copied().collect();
    let mut seen = FxHashSet::default();
    objs.iter()
        .copied()
        .filter(|&o| seen.insert(o))
        .filter(|&o| {
            !doc.groups
                .ancestors(o)
                .any(|a| chosen.contains(&ObjRef::Group(a)))
        })
        .collect()
}

/// The elements, meshes and groups of [`uncovered`]: the nodes of the tree an operation on a
/// selection acts on.
pub fn independent(doc: &Document, objs: &[ObjRef]) -> Vec<ObjRef> {
    let mut nodes = uncovered(doc, objs);
    nodes.retain(|o| {
        matches!(o, ObjRef::Element(_) | ObjRef::Mesh(_) | ObjRef::Group(_)) && doc.has_node(*o)
    });
    nodes
}

/// The groups among `objs`.
pub fn groups_of(objs: &[ObjRef]) -> Vec<GroupId> {
    objs.iter()
        .filter_map(|o| match o {
            ObjRef::Group(id) => Some(*id),
            _ => None,
        })
        .collect()
}

/// The elements and meshes among `objs` and below the groups among them, each once.
pub fn leaves(doc: &Document, objs: &[ObjRef]) -> Vec<ObjRef> {
    let below = doc.groups.descendants(&groups_of(objs));
    let mut seen = FxHashSet::default();
    objs.iter()
        .copied()
        .chain(below)
        .filter(|o| matches!(o, ObjRef::Element(_) | ObjRef::Mesh(_)) && doc.has_node(*o))
        .filter(|&o| seen.insert(o))
        .collect()
}

/// The centre of the box around the objects below `nodes`, or the mean of the group pivots when
/// there is no object.
pub(crate) fn center_of(doc: &Document, nodes: &[ObjRef]) -> Vec3 {
    let pose = Pose::capture(doc, nodes);
    if let Some((lo, hi)) = pose.bounds() {
        return (lo + hi) * 0.5;
    }
    let pivots: Vec<Vec3> = groups_of(nodes)
        .iter()
        .filter_map(|id| doc.groups.get(*id).map(|g| g.origin))
        .collect();
    if pivots.is_empty() {
        return Vec3::ZERO;
    }
    pivots.iter().sum::<Vec3>() / pivots.len() as f32
}

fn clean_name(name: &str, fallback: &str) -> Label {
    let name = name.replace('"', "");
    Label::from(if name.trim().is_empty() {
        fallback
    } else {
        name.as_str()
    })
}

/// Makes a group of the nodes among `objs`. Its pivot is the centre of their bounds, it sits
/// where the first of them was and its parent is that node's parent. Returns the new group.
///
/// spec: hierarchy A1 4
pub fn group(cx: &mut EditCtx<'_>, objs: &[ObjRef], name: &str) -> Result<GroupId, GroupError> {
    let nodes = independent(cx.doc, objs);
    if nodes.is_empty() {
        return Err(GroupError::Missing);
    }
    let origin = center_of(cx.doc, &nodes);
    cx.doc
        .insert_group(Group::new(clean_name(name, "Group"), origin), &nodes)
}

/// Dissolves the groups among `objs`: their children take their place. Returns the children that
/// were freed.
///
/// spec: hierarchy A1 4
pub fn ungroup(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> Vec<ObjRef> {
    let mut freed = Vec::new();
    for id in groups_of(objs) {
        if let Ok(children) = cx.doc.dissolve_group(id) {
            cx.sel.remove(ObjRef::Group(id));
            freed.extend(children);
        }
    }
    let gone: FxHashSet<ObjRef> = groups_of(objs).into_iter().map(ObjRef::Group).collect();
    freed.retain(|n| !gone.contains(n));
    cx.doc
        .journal
        .mark_doc(DocFlags::SELECTION | DocFlags::HIGHLIGHT);
    freed
}

/// Moves the nodes among `objs` into a group, or to the top level for `None`, at `index` among
/// the children that stay (the end when `None`). The nodes keep the order they have in the
/// outliner. Returns how many moved.
///
/// spec: hierarchy A1 4
pub fn reparent(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    parent: Option<GroupId>,
    index: Option<usize>,
) -> Result<usize, GroupError> {
    let mut nodes = independent(cx.doc, objs);
    if nodes.len() > 1 {
        let rank: FxHashMap<ObjRef, usize> = cx
            .doc
            .outline()
            .into_iter()
            .enumerate()
            .map(|(at, (node, _))| (node, at))
            .collect();
        nodes.sort_by_key(|n| rank.get(n).copied().unwrap_or(usize::MAX));
    }
    cx.doc.move_nodes(&nodes, parent, index)
}

/// Changes the fields of a group. Hiding or locking marks everything below it in the journal.
/// Returns whether anything changed.
///
/// spec: hierarchy A1 4
pub fn set_fields(cx: &mut EditCtx<'_>, id: GroupId, fields: &GroupFields) -> bool {
    let Some(old) = cx.doc.groups.get(id) else {
        return false;
    };
    let name = fields.name.as_deref().map(|n| clean_name(n, &old.name));
    let flags_changed = fields.visible.is_some_and(|v| v != old.visible)
        || fields.locked.is_some_and(|l| l != old.locked);
    let changed = flags_changed
        || name.as_ref().is_some_and(|n| *n != old.name)
        || fields.origin.is_some_and(|o| !engine_eq(o, old.origin))
        || fields.export.is_some_and(|x| x != old.export);
    if !changed {
        return false;
    }
    {
        let doc = &mut *cx.doc;
        let Some(group) = doc.groups.edit(id, ChangeFlags::all(), &mut doc.journal) else {
            return false;
        };
        if let Some(name) = name {
            group.name = name;
        }
        if let Some(origin) = fields.origin {
            group.origin = origin;
        }
        if let Some(visible) = fields.visible {
            group.visible = visible;
        }
        if let Some(locked) = fields.locked {
            group.locked = locked;
        }
        if let Some(export) = fields.export {
            group.export = export;
        }
    }
    if flags_changed {
        let below = cx.doc.groups.descendants(&[id]);
        for obj in below {
            if let ObjRef::Element(oid) | ObjRef::Mesh(oid) | ObjRef::Group(oid) = obj {
                cx.doc.journal.mark_object(oid, ChangeFlags::VISIBILITY);
            }
        }
    }
    cx.doc.journal.mark_doc(DocFlags::GROUPS | DocFlags::LISTS);
    cx.doc.revision = Rev::next();
    true
}

/// Moves the pivot of a group; the objects stay where they are. Returns whether it moved.
pub fn set_origin(cx: &mut EditCtx<'_>, id: GroupId, origin: Vec3) -> bool {
    let fields = GroupFields {
        origin: Some(origin),
        ..GroupFields::default()
    };
    set_fields(cx, id, &fields)
}

/// Removes the nodes among `objs` with everything below the groups among them, and deselects
/// them. Returns how many objects and groups were removed.
///
/// spec: hierarchy A1 4
pub fn delete(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> usize {
    let nodes = independent(cx.doc, objs);
    let doomed_leaves = leaves(cx.doc, &nodes);
    let doomed_groups = cx.doc.groups.subtree_groups(&groups_of(&nodes));
    for &id in &doomed_groups {
        cx.sel.remove(ObjRef::Group(id));
    }
    let set: FxHashSet<GroupId> = doomed_groups.iter().copied().collect();
    cx.doc.remove_groups(&set);
    let removed = crate::elements::delete(cx, &doomed_leaves);
    if !doomed_groups.is_empty() {
        cx.doc
            .journal
            .mark_doc(DocFlags::SELECTION | DocFlags::HIGHLIGHT);
    }
    removed + doomed_groups.len()
}

/// What a duplicate made.
#[derive(Clone, Debug, Default)]
pub struct Duplicated {
    /// Every new element and mesh, elements first, for the start step.
    pub created: Vec<ObjRef>,
    /// The copies of the nodes that were asked for: the new groups and the loose copies.
    pub chosen: Vec<ObjRef>,
}

/// Copies the nodes among `objs` in place and selects the copies. A group is copied with its
/// whole subtree under new ids and without uuids, and sits right behind the original; an
/// object in a group is copied into that group.
///
/// spec: hierarchy A1 4
pub fn duplicate(cx: &mut EditCtx<'_>, objs: &[ObjRef]) -> Result<Duplicated, OpError> {
    let nodes = independent(cx.doc, objs);
    let picked = leaves(cx.doc, &nodes);
    let wanted: FxHashSet<ObjRef> = picked.iter().copied().collect();
    // The copy lists the objects in list order, elements first.
    let old: Vec<ObjRef> = cx
        .doc
        .elements
        .ids()
        .iter()
        .map(|&id| ObjRef::Element(id))
        .chain(cx.doc.meshes.ids().iter().map(|&id| ObjRef::Mesh(id)))
        .filter(|o| wanted.contains(o))
        .collect();
    let created = crate::elements::duplicate(cx, &picked)?;
    let copy_of: FxHashMap<ObjRef, ObjRef> = old.into_iter().zip(created.iter().copied()).collect();

    let mut chosen = Vec::new();
    for node in nodes {
        let parent = cx.doc.groups.parent_of(node);
        let behind = cx
            .doc
            .child_nodes(parent)
            .iter()
            .position(|n| *n == node)
            .map(|at| at + 1);
        let copy = match node {
            ObjRef::Group(id) => copy_subtree(cx.doc, id, &copy_of, parent, behind)?,
            leaf => {
                let Some(&copy) = copy_of.get(&leaf) else {
                    continue;
                };
                if parent.is_some() {
                    cx.doc.graft(Vec::new(), &[copy], parent, behind)?;
                }
                copy
            }
        };
        chosen.push(copy);
    }
    cx.sel.clear();
    for &obj in &chosen {
        cx.sel.add(obj);
    }
    cx.doc
        .journal
        .mark_doc(DocFlags::SELECTION | DocFlags::HIGHLIGHT);
    Ok(Duplicated { created, chosen })
}

/// Adds a copy of the subtree of `src` whose objects are the copies in `copy_of`.
fn copy_subtree(
    doc: &mut Document,
    src: GroupId,
    copy_of: &FxHashMap<ObjRef, ObjRef>,
    parent: Option<GroupId>,
    index: Option<usize>,
) -> Result<ObjRef, GroupError> {
    let all = doc.groups.subtree_groups(&[src]);
    let new_ids: FxHashMap<GroupId, GroupId> = all.iter().map(|&id| (id, ObjId::next())).collect();
    let mut made = Vec::with_capacity(all.len());
    for &id in &all {
        let Some(old) = doc.groups.get(id) else {
            continue;
        };
        let children = old
            .children()
            .iter()
            .filter_map(|child| match *child {
                ObjRef::Group(inner) => new_ids.get(&inner).map(|&n| ObjRef::Group(n)),
                other => copy_of.get(&other).copied(),
            })
            .collect();
        let mut copy = Group::new(old.name.clone(), old.origin).with_children(children);
        copy.rotation = old.rotation;
        copy.visible = old.visible;
        copy.locked = old.locked;
        copy.export = old.export;
        if let Some(&new) = new_ids.get(&id) {
            made.push((new, copy));
        }
    }
    let top = new_ids.get(&src).copied().ok_or(GroupError::Missing)?;
    doc.graft(made, &[ObjRef::Group(top)], parent, index)?;
    Ok(ObjRef::Group(top))
}

#[cfg(test)]
mod tests;
