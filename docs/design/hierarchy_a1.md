# A1: groups and hierarchy (implementation design)

Status: 2026-10-25, refines `anim_texture_spec.md` 3 for the code as it is after brief v2 (a8aedd1). Owner
order: brief v2 first, then animation; A1 is the prerequisite of A2 (keyframe animation of bones).

## 1. Today

- The model is flat. `.bbmodel` import walks the outliner and bakes each group's origin and rotation into its
  elements (`import.rs`, `Rigid::about`); group names survive only as label prefixes.
- `animations`, `animation_controllers` and the other unread sections are counted in a notice and dropped, so
  saving back to `.bbmodel` loses them. `Document.retained` exists for loader data but `.bbmodel` sets none.
- `Group`, `Ungroup` and `Reparent` commands return `Deferred`; the outliner can draw a tree and drop hints.

## 2. Decision H1: groups are a layer over world-space objects

Elements and meshes keep their world (rest pose) transforms exactly as today. A group adds a name, a pivot, a
rotation and flags, and objects point at their group. Nothing that reads element transforms changes: the
renderer, picking, gizmos, the validator, the Minecraft JSON, OBJ and glTF exporters and the `.cubik` payload
all keep working on world transforms. Only the commands that move a group, and the `.bbmodel` writer, compose
or invert the group chain.

This differs from `anim_texture_spec.md` 3 ("world matrices composed from the hierarchy in the extract"):
composing at extract time would touch every reader of element transforms for no gain in A1. A2 adds the
animated pose as a per-bone delta on top of the rest pose at draw time.

## 3. Model (cuboid-model)

- `Group { id, name, parent: Option<GroupId>, origin: Vec3, rotation: Vec3, visible, locked, export, uuid:
  Option<String> }`; rotation uses the `.bbmodel` Euler convention the importer already reads.
- `Element.parent` and `Mesh.parent`: `Option<GroupId>`. `ObjRef::Group(ObjId)` for selection and commands.
- `Document.groups` with stable ids (the W1.5 rule: never reused in a document) and the children order: the
  order of groups and objects inside one parent is stored, not derived.
- Effective flags: an object is drawn when it and every ancestor are visible, and is locked when it or any
  ancestor is locked.
- Invariants checked by `check_invariants`: parents exist, no cycle, depth at most 64, at most 65 536 groups.
- Undo: a new snapshot category `HIERARCHY` (bit 128, free today) for the groups and parent links.

## 4. Commands (cuboid-core), each one undo step

| Command | Effect | Categories |
|---|---|---|
| `Group { objects }` | new group, pivot at the centre of the objects' bounds, parent = their common parent, placed where the first object was | HIERARCHY |
| `Ungroup { group }` | children move to the group's parent at the group's place; the group goes | HIERARCHY |
| `Reparent { objects, parent, index }` | moves objects and groups; refuses a cycle | HIERARCHY |
| `SetGroup { group, name, origin, visible, locked, export }` | the given fields; moving the origin moves the pivot only | HIERARCHY |
| move, rotate and scale with groups selected | the group's pivot and rotation and every descendant's world transform change rigidly, about the group's pivot | HIERARCHY, OBJECTS |
| delete with a group selected | the group and its descendants | HIERARCHY, OBJECTS |
| duplicate with a group selected | the subtree with new ids and new uuids | HIERARCHY, OBJECTS |

The existing `Deferred` answers for group, ungroup and reparent go away.

## 5. Formats

- `.bbmodel` read: build the groups from the outliner (origin, rotation, name, visibility, export, uuid) and
  keep the element uuids, while elements still get their baked world transforms as today.
- `.bbmodel` write: an element's local transform is the inverse of its group chain applied to its world
  transform, the exact inverse of the reader; a property test checks that read, write and read again give
  the same world transforms (within 1e-4) and that an unchanged file writes the same groups and elements as
  JSON values. Original uuids are written back; new objects get new ones.
- Sections the model cannot hold yet (`animations`, `animation_controllers`,
  `animation_variable_placeholders` and unknown top-level keys) are kept as JSON in the document's retained
  data, bounded at 64 MiB, and written back unchanged by the `.bbmodel` writer. Writing to another format
  shows a notice that they are not included. A2 replaces this for animations.
- `.cubik`: the payload stays as it is (world transforms, Cubik-compatible, payload identity for unchanged
  files). The groups and parent links go into `cuboid.json` with a hash of the element records; when the hash
  does not match (Cubik rewrote the file), the links that still resolve are kept and the user is told.
- Minecraft JSON, OBJ and glTF: unchanged (world transforms); a glTF node tree is later.

## 6. UI

- Outliner: real groups with folder icons; drag onto a group to reparent, between rows to reorder; context
  menu Group (Ctrl+G), Ungroup, Select children; collapse state per document in the UI; visibility and lock
  toggles on a group act on the subtree.
- Element tab with a group selected: name, origin, rotation (rotates the subtree), visible, locked, export.
- The gizmo on a group moves, rotates and scales the subtree about the group's pivot.

## 7. Tests

Model invariants; one scenario test per command with undo and redo; the `.bbmodel` property test of 5 on
fixtures made in the tests (nested and rotated groups, a 3-level chain, an animation section kept verbatim);
`.cubik` sidecar round trip and the stale-hash case; the private oracles must stay green (payload identity);
outliner UI tests for drag to reparent and the group context menu.
