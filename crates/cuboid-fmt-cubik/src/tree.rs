//! The group tree in the `cuboid.json` entry (spec: hierarchy A1 5).
//!
//! The payload of a `.cubik` file keeps the world transforms of the elements and knows nothing of
//! groups, so Cubik shows the same model. The groups and the links of the elements to them go
//! into the entry, which Cubik ignores and drops when it saves the file again. A reference names
//! an element or a mesh by its place in the payload and by a short signature of its transform; a
//! digest of all signatures says whether the payload is the one the references were made for.
//! When it is not (Cubik rewrote the file), a reference is kept if the element at its place still
//! has the signature or exactly one other element has it, and dropped otherwise.

use std::collections::HashMap;

use cuboid_model::{
    ElementsSnap, Group, GroupId, GroupsBuilder, GroupsSnap, MAX_GROUPS, MeshesSnap, ObjRef,
    Selection, Snapshot,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The longest uuid kept, in bytes.
const UUID_BYTES: usize = 64;

fn yes() -> bool {
    true
}

/// The `groups` member of the entry.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Tree {
    /// Digest of the signatures of all elements and meshes of the payload.
    records: String,
    /// The groups in list order.
    list: Vec<Node>,
    /// The top level in outliner order, as references.
    roots: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Node {
    name: String,
    origin: [f32; 3],
    #[serde(default)]
    rotation: [f32; 3],
    #[serde(default = "yes")]
    visible: bool,
    #[serde(default)]
    locked: bool,
    #[serde(default = "yes")]
    export: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uuid: Option<String>,
    /// References in outliner order.
    #[serde(default)]
    children: Vec<String>,
}

/// The signature of an object: a hash of the fields that place it.
fn signature(kind: u8, fields: [glam::Vec3; 4]) -> u32 {
    let mut hash = Sha256::new();
    hash.update([kind]);
    for v in fields {
        for c in v.to_array() {
            hash.update(c.to_bits().to_le_bytes());
        }
    }
    let digest = hash.finalize();
    digest
        .first_chunk::<4>()
        .map_or(0, |bytes| u32::from_be_bytes(*bytes))
}

/// The signatures of the elements, then of the meshes, in list order.
fn signatures(
    elements: Option<&ElementsSnap>,
    meshes: Option<&MeshesSnap>,
) -> (Vec<u32>, Vec<u32>) {
    let element = |e: &std::sync::Arc<cuboid_model::Element>| {
        let b = &e.base;
        signature(e.kind as u8, [b.p, b.s, b.e, b.o])
    };
    let mesh = |m: &std::sync::Arc<cuboid_model::Mesh>| {
        let b = &m.base;
        signature(0x10 | m.kind as u8, [b.p, b.s, b.e, b.o])
    };
    (
        elements.map_or_else(Vec::new, |s| s.list.items.iter().map(element).collect()),
        meshes.map_or_else(Vec::new, |s| s.list.items.iter().map(mesh).collect()),
    )
}

fn digest(elements: &[u32], meshes: &[u32]) -> String {
    let mut hash = Sha256::new();
    hash.update((elements.len() as u64).to_le_bytes());
    for sig in elements {
        hash.update(sig.to_le_bytes());
    }
    hash.update((meshes.len() as u64).to_le_bytes());
    for sig in meshes {
        hash.update(sig.to_le_bytes());
    }
    hash.finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The tree of a snapshot as the entry writes it; `None` for a model without groups.
pub(crate) fn encode(snapshot: &Snapshot) -> Option<Tree> {
    let groups = snapshot
        .groups
        .as_ref()
        .filter(|g| !g.list.ids.is_empty())?;
    let (el_sigs, me_sigs) = signatures(snapshot.elements.as_ref(), snapshot.meshes.as_ref());
    let place = |ids: Option<&[cuboid_model::ObjId]>| -> HashMap<cuboid_model::ObjId, usize> {
        ids.unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(at, id)| (*id, at))
            .collect()
    };
    let element_at = place(snapshot.elements.as_ref().map(|s| s.list.ids.as_slice()));
    let mesh_at = place(snapshot.meshes.as_ref().map(|s| s.list.ids.as_slice()));
    let group_at: HashMap<_, _> = groups
        .list
        .ids
        .iter()
        .enumerate()
        .map(|(at, id)| (*id, at))
        .collect();
    let reference = |node: ObjRef| -> Option<String> {
        match node {
            ObjRef::Group(id) => group_at.get(&id).map(|at| format!("g{at}")),
            ObjRef::Element(id) => element_at
                .get(&id)
                .and_then(|&at| Some(format!("e{at}:{:08x}", el_sigs.get(at)?))),
            ObjRef::Mesh(id) => mesh_at
                .get(&id)
                .and_then(|&at| Some(format!("m{at}:{:08x}", me_sigs.get(at)?))),
            ObjRef::Blueprint | ObjRef::ModelOrigin => None,
        }
    };
    let list = groups
        .list
        .items
        .iter()
        .map(|g| Node {
            name: g.name.to_string(),
            origin: g.origin.to_array(),
            rotation: g.rotation.to_array(),
            visible: g.visible,
            locked: g.locked,
            export: g.export,
            uuid: g.uuid.clone(),
            children: g.children().iter().filter_map(|&c| reference(c)).collect(),
        })
        .collect();
    Some(Tree {
        records: digest(&el_sigs, &me_sigs),
        list,
        roots: groups.roots.iter().filter_map(|&n| reference(n)).collect(),
    })
}

/// What reading the member gave.
pub(crate) struct Read {
    pub groups: Option<GroupsSnap>,
    /// The id each position of the member's list became, `None` for a group that was not placed.
    pub group_ids: Vec<Option<GroupId>>,
    pub warnings: Vec<String>,
}

/// A reference as parsed: the kind, the place and the signature of an object.
enum Reference {
    Group(usize),
    Object(char, usize, u32),
}

fn parse(text: &str) -> Option<Reference> {
    let mut chars = text.chars();
    let kind = chars.next()?;
    let rest = chars.as_str();
    match kind {
        'g' => rest.parse().ok().map(Reference::Group),
        'e' | 'm' => {
            let (at, sig) = rest.split_once(':')?;
            Some(Reference::Object(
                kind,
                at.parse().ok()?,
                u32::from_str_radix(sig, 16).ok()?,
            ))
        }
        _ => None,
    }
}

/// Reads the member against the elements and meshes of the loaded sections and builds the tree.
/// A damaged member gives no groups and a warning.
pub(crate) fn decode(value: serde_json::Value, sections: &Snapshot) -> Read {
    let mut warnings = Vec::new();
    let tree: Tree = match serde_json::from_value(value) {
        Ok(tree) => tree,
        Err(error) => {
            warnings.push(format!(
                "the groups in the `cuboid.json` entry are damaged and ignored ({error})"
            ));
            return Read {
                groups: None,
                group_ids: Vec::new(),
                warnings,
            };
        }
    };
    if tree.list.len() > MAX_GROUPS {
        warnings.push("the `cuboid.json` entry holds more groups than a model can have".to_owned());
        return Read {
            groups: None,
            group_ids: Vec::new(),
            warnings,
        };
    }
    let (el_sigs, me_sigs) = signatures(sections.elements.as_ref(), sections.meshes.as_ref());
    let current = tree.records == digest(&el_sigs, &me_sigs);
    let by_signature = |sigs: &[u32]| {
        let mut map: HashMap<u32, Option<usize>> = HashMap::new();
        for (at, sig) in sigs.iter().enumerate() {
            map.entry(*sig)
                .and_modify(|found| *found = None)
                .or_insert(Some(at));
        }
        map
    };
    let (el_by_sig, me_by_sig) = (by_signature(&el_sigs), by_signature(&me_sigs));

    let ids = |s: Option<&[cuboid_model::ObjId]>| s.unwrap_or_default().to_vec();
    let el_ids = ids(sections.elements.as_ref().map(|s| s.list.ids.as_slice()));
    let me_ids = ids(sections.meshes.as_ref().map(|s| s.list.ids.as_slice()));

    let mut builder = GroupsBuilder::new();
    let mut state = Build {
        tree: &tree,
        current,
        el_sigs: &el_sigs,
        me_sigs: &me_sigs,
        el_ids: &el_ids,
        me_ids: &me_ids,
        el_by_sig: &el_by_sig,
        me_by_sig: &me_by_sig,
        placed_groups: vec![false; tree.list.len()],
        group_ids: vec![None; tree.list.len()],
        placed: std::collections::HashSet::new(),
        lost: 0,
    };
    for text in &tree.roots {
        state.add(&mut builder, text, None, 0);
    }
    let Build {
        placed_groups,
        group_ids,
        lost,
        ..
    } = state;
    let unreachable = placed_groups.iter().filter(|p| !**p).count();
    if !current {
        warnings.push(format!(
            "the elements of the file were changed outside Cuboid: {lost} links to groups \
             could not be matched and {unreachable} groups were left out"
        ));
    } else if unreachable > 0 {
        warnings.push(format!(
            "{unreachable} groups in the `cuboid.json` entry are not in its tree and were left out"
        ));
    }
    let groups = (!builder.is_empty()).then(|| builder.finish().snapshot(&Selection::default()));
    Read {
        groups,
        group_ids,
        warnings,
    }
}

/// The state of building a tree from references.
struct Build<'a> {
    tree: &'a Tree,
    current: bool,
    el_sigs: &'a [u32],
    me_sigs: &'a [u32],
    el_ids: &'a [cuboid_model::ObjId],
    me_ids: &'a [cuboid_model::ObjId],
    el_by_sig: &'a HashMap<u32, Option<usize>>,
    me_by_sig: &'a HashMap<u32, Option<usize>>,
    placed_groups: Vec<bool>,
    /// The id of each group that was added to the builder.
    group_ids: Vec<Option<GroupId>>,
    placed: std::collections::HashSet<ObjRef>,
    /// References that named nothing the file still has.
    lost: usize,
}

impl Build<'_> {
    /// The object a reference stands for, if the file still has it.
    fn object(&self, kind: char, at: usize, sig: u32) -> Option<ObjRef> {
        let (sigs, ids, by_sig): (&[u32], &[cuboid_model::ObjId], _) = if kind == 'e' {
            (self.el_sigs, self.el_ids, self.el_by_sig)
        } else {
            (self.me_sigs, self.me_ids, self.me_by_sig)
        };
        let found = if sigs.get(at) == Some(&sig) || (self.current && at < sigs.len()) {
            Some(at)
        } else {
            by_sig.get(&sig).copied().flatten()
        }?;
        let id = *ids.get(found)?;
        Some(if kind == 'e' {
            ObjRef::Element(id)
        } else {
            ObjRef::Mesh(id)
        })
    }

    /// Adds the node a reference names to `parent`, with everything below a group.
    fn add(
        &mut self,
        builder: &mut GroupsBuilder,
        text: &str,
        parent: Option<cuboid_model::GroupId>,
        depth: usize,
    ) {
        if depth > cuboid_model::MAX_DEPTH {
            self.lost += 1;
            return;
        }
        match parse(text) {
            Some(Reference::Group(at)) => {
                let Some(node) = self.tree.list.get(at) else {
                    self.lost += 1;
                    return;
                };
                let Some(slot) = self.placed_groups.get_mut(at) else {
                    self.lost += 1;
                    return;
                };
                if std::mem::replace(slot, true) {
                    self.lost += 1;
                    return;
                }
                let mut group = Group::new(node.name.as_str(), glam::Vec3::from_array(node.origin));
                group.rotation = glam::Vec3::from_array(node.rotation);
                group.visible = node.visible;
                group.locked = node.locked;
                group.export = node.export;
                group.uuid = node
                    .uuid
                    .clone()
                    .filter(|u| u.len() <= UUID_BYTES && !u.chars().any(char::is_control));
                let Ok(id) = builder.add_group(group, parent) else {
                    self.lost += 1;
                    return;
                };
                if let Some(slot) = self.group_ids.get_mut(at) {
                    *slot = Some(id);
                }
                for child in &node.children {
                    self.add(builder, child, Some(id), depth + 1);
                }
            }
            Some(Reference::Object(kind, at, sig)) => {
                let Some(obj) = self.object(kind, at, sig) else {
                    self.lost += 1;
                    return;
                };
                if !self.placed.insert(obj) || builder.add_object(obj, parent).is_err() {
                    self.lost += 1;
                }
            }
            None => self.lost += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cuboid_model::{
        Blueprint, Categories, Element, ElementType, ElementsSnap, Groups, Label, ListSnap, ObjId,
    };
    use glam::Vec3;

    use super::*;

    fn element(x: f32) -> Element {
        let mut el = Element::new(
            ElementType::Cube,
            &Blueprint::default(),
            1,
            [1.0; 4],
            &Label::from("texture"),
        );
        el.base.p = Vec3::new(x, 0.0, 0.0);
        el
    }

    /// Four cubes and a snapshot with group A holding cubes 0 and 1 and group B inside A
    /// holding cube 2; cube 3 is loose.
    fn sample() -> Snapshot {
        let list = ListSnap::fresh((0..4).map(|i| element(i as f32 * 3.0)));
        let ids = list.ids.clone();
        let mut b = GroupsBuilder::new();
        let a = b.add_group(Group::new("A", Vec3::ONE), None).unwrap();
        b.add_object(ObjRef::Element(ids[0]), Some(a)).unwrap();
        let mut inner = Group::new("B", Vec3::X);
        inner.rotation = Vec3::new(0.0, 30.0, 0.0);
        inner.locked = true;
        inner.uuid = Some("bone-b".to_owned());
        let inner = b.add_group(inner, Some(a)).unwrap();
        b.add_object(ObjRef::Element(ids[2]), Some(inner)).unwrap();
        b.add_object(ObjRef::Element(ids[1]), Some(a)).unwrap();
        let selected = vec![false; list.ids.len()];
        Snapshot {
            categories: Categories::ELEMENTS | Categories::HIERARCHY,
            props: None,
            view: None,
            elements: Some(ElementsSnap {
                blueprint: Blueprint::default(),
                list,
                selected,
            }),
            textures: None,
            voxels: None,
            meshes: None,
            display: None,
            groups: Some(b.finish().snapshot(&Selection::default())),
            animations: None,
        }
    }

    /// The sections a loader makes of the same elements: new ids, no groups.
    fn loaded_like(snapshot: &Snapshot, edit: impl FnOnce(&mut Vec<Arc<Element>>)) -> Snapshot {
        let mut items: Vec<Arc<Element>> = snapshot
            .elements
            .as_ref()
            .unwrap()
            .list
            .items
            .iter()
            .map(|e| Arc::new((**e).clone()))
            .collect();
        edit(&mut items);
        let ids = items.iter().map(|_| ObjId::next()).collect();
        let selected = vec![false; items.len()];
        Snapshot {
            groups: None,
            elements: Some(ElementsSnap {
                blueprint: Blueprint::default(),
                list: ListSnap { ids, items },
                selected,
            }),
            ..snapshot.clone()
        }
    }

    fn names(groups: &GroupsSnap) -> Vec<String> {
        groups
            .list
            .items
            .iter()
            .map(|g| g.name.to_string())
            .collect()
    }

    #[test]
    fn the_tree_comes_back_with_its_order_flags_and_links() {
        let saved = sample();
        let tree = encode(&saved).expect("a tree");
        let value = serde_json::to_value(&tree).unwrap();
        let loaded = loaded_like(&saved, |_| {});
        let read = decode(value, &loaded);
        assert!(read.warnings.is_empty(), "{:?}", read.warnings);
        let groups = read.groups.expect("groups");
        assert_eq!(names(&groups), ["A", "B"]);
        let (a, b) = (&groups.list.items[0], &groups.list.items[1]);
        let ids = &loaded.elements.as_ref().unwrap().list.ids;
        assert_eq!(
            a.children(),
            [
                ObjRef::Element(ids[0]),
                ObjRef::Group(groups.list.ids[1]),
                ObjRef::Element(ids[1])
            ]
        );
        assert_eq!(b.children(), [ObjRef::Element(ids[2])]);
        assert!(b.locked && b.visible && b.export);
        assert_eq!(b.rotation, Vec3::new(0.0, 30.0, 0.0));
        assert_eq!(b.uuid.as_deref(), Some("bone-b"));
        assert_eq!(groups.roots, [ObjRef::Group(groups.list.ids[0])]);
    }

    #[test]
    fn a_model_without_groups_writes_no_member() {
        let mut saved = sample();
        saved.groups = None;
        assert!(encode(&saved).is_none());
        let mut empty = sample();
        empty.groups = Some(Groups::default().snapshot(&Selection::default()));
        assert!(encode(&empty).is_none());
    }

    #[test]
    fn links_that_still_resolve_are_kept_when_the_payload_changed() {
        let saved = sample();
        let value = serde_json::to_value(encode(&saved).unwrap()).unwrap();
        // Another program moved cube 1, deleted cube 3 and put a new cube first.
        let loaded = loaded_like(&saved, |items| {
            Arc::make_mut(&mut items[1]).base.p.y = 9.0;
            items.pop();
            items.insert(0, Arc::new(element(100.0)));
        });
        let read = decode(value, &loaded);
        assert_eq!(read.warnings.len(), 1, "{:?}", read.warnings);
        assert!(
            read.warnings[0].contains("changed outside"),
            "{:?}",
            read.warnings
        );
        let groups = read.groups.expect("groups");
        let ids = &loaded.elements.as_ref().unwrap().list.ids;
        // Cube 0 moved to place 1 and is found by its signature; cube 1 changed and is lost;
        // cube 2 moved to place 3 and is found.
        let a = &groups.list.items[0];
        assert_eq!(
            a.children(),
            [ObjRef::Element(ids[1]), ObjRef::Group(groups.list.ids[1])]
        );
        assert_eq!(groups.list.items[1].children(), [ObjRef::Element(ids[3])]);
        assert!(
            !read_has(&groups, ObjRef::Element(ids[0])),
            "the new cube is loose"
        );
    }

    fn read_has(groups: &GroupsSnap, obj: ObjRef) -> bool {
        groups
            .list
            .items
            .iter()
            .any(|g| g.children().contains(&obj))
            || groups.roots.contains(&obj)
    }

    #[test]
    fn two_elements_with_the_same_signature_are_never_guessed() {
        let saved = sample();
        let value = serde_json::to_value(encode(&saved).unwrap()).unwrap();
        // Cube 0 is gone and two copies of cube 2 take other places.
        let loaded = loaded_like(&saved, |items| {
            let twin = items[2].clone();
            items.remove(0);
            items.push(twin);
        });
        let read = decode(value, &loaded);
        let groups = read.groups.expect("groups");
        let ids = &loaded.elements.as_ref().unwrap().list.ids;
        // Cube 1 is found by its signature, cube 0 is gone and the two twins of cube 2 are
        // neither of them taken for it.
        assert!(read_has(&groups, ObjRef::Element(ids[0])));
        assert!(!read_has(&groups, ObjRef::Element(ids[1])));
        assert!(!read_has(&groups, ObjRef::Element(ids[3])));
        assert!(groups.list.items[1].children().is_empty());
    }

    #[test]
    fn a_damaged_member_gives_a_warning_and_no_groups() {
        let saved = sample();
        let loaded = loaded_like(&saved, |_| {});
        let read = decode(serde_json::json!({"records": 7}), &loaded);
        assert!(read.groups.is_none());
        assert!(read.warnings[0].contains("damaged"));

        // A cycle and a reference outside the file are dropped, not followed.
        let mut tree = serde_json::to_value(encode(&saved).unwrap()).unwrap();
        tree["list"][1]["children"] = serde_json::json!(["g0", "e99:00000000", "x1", "g1"]);
        let read = decode(tree, &loaded);
        let groups = read.groups.expect("groups");
        assert_eq!(names(&groups), ["A", "B"]);
        assert!(groups.list.items[1].children().is_empty());
    }

    #[test]
    fn a_tree_deeper_than_the_limit_is_cut() {
        let saved = sample();
        let loaded = loaded_like(&saved, |_| {});
        let depth = cuboid_model::MAX_DEPTH + 5;
        let list: Vec<serde_json::Value> = (0..depth)
            .map(|i| {
                let children = if i + 1 < depth {
                    vec![format!("g{}", i + 1)]
                } else {
                    vec![]
                };
                serde_json::json!({"name": "g", "origin": [0, 0, 0], "children": children})
            })
            .collect();
        let tree = serde_json::json!({"records": "x", "list": list, "roots": ["g0"]});
        let groups = decode(tree, &loaded).groups.expect("groups");
        assert_eq!(groups.list.ids.len(), cuboid_model::MAX_DEPTH);
    }
}
