# Data model and editing operations (model_and_tools)

Functional specification for **cuboid**, a clean-room reimplementation of the editing core of
Cubik Studio 2.9.480 Beta. This document describes *behaviour*: data structures, coordinate
conventions, every editing operation and tool, and undo/redo. It is written to drive the
implementation and the parity test suite. File byte layouts, menus/panels layout, import/export
formats and display settings are covered by their own specifications; this document only repeats
what is needed to define editing behaviour.

This document is also the **texture specification** that the other specifications refer to (texture list,
labels, image loading and saving, external editing: 5.6–5.15) and the specification of the **built-in
geometry** (primitive meshes and the preview player rig: 6.5–6.8).

Notation and labels used throughout:

| Term | Meaning |
|---|---|
| internal space | the coordinate frame the original stores in memory and in `.cubik` files (left-handed, +Y up) |
| MC space / UI space | the Minecraft-style frame shown to the user: `(x, y, -z)` of internal space |
| unit | one model unit = one Minecraft texel ("pixel"); 16 units = 1 block |
| `round(v)` | round half to even (banker's rounding), the rounding of the original's math library |
| `trunc(v)` | truncation toward zero (integer cast) |
| `⊙` | component-wise product |
| QUIRK | original behaviour that looks unintended; the port must reproduce it unless a port decision says otherwise |
| UNVERIFIED | not confirmed at runtime, or inferred from incomplete evidence |
| "text" | user-visible string; must match exactly (typos included) |

All arithmetic is IEEE single precision (f32) in the original; parity-sensitive code paths (snapping,
quantisation, auto-UV) must use f32 and the same operation order.

---

## 1. Coordinate systems, units and numeric rules

### 1.1 Units and the editing volume

- 16 units = 1 Minecraft block. Element positions and sizes are in units.
- The **editing volume** (also the default *model box*, see 2.4) is a 48 x 48 x 48 unit cube:
  internal `x ∈ [-16, 32]`, `y ∈ [-16, 32]`, `z ∈ [-32, 16]`; centre `(8, 8, -8)`.
  In MC space this is `[-16, 32]` on all three axes.
- The **central block** ("middle zone") is internal `x ∈ [0,16]`, `y ∈ [0,16]`, `z ∈ [-16, 0]`
  (MC `[0,16]³`).
- Ground/axis grid planes are 48 units wide and centred on `(8, 8, -8)`.

### 1.2 Internal space versus MC/UI space

Internal space is left-handed, +Y up, +Z "forward". The UI and all Minecraft-facing values negate Z:
`ui = (x, y, -z)`. Sizes are never negated.

Face directions (the six faces of every element, voxel and box):

| Face | Internal outward normal | MC outward normal | Face flag value |
|---|---|---|---|
| down | -Y | -Y | 1 |
| up | +Y | +Y | 2 |
| north | +Z | -Z | 4 |
| south | -Z | +Z | 8 |
| west | -X | -X | 16 |
| east | +X | +X | 32 |

`none = 0`, `any = all = 63`. The same bit values are used for a face's cull direction.

### 1.3 Element box convention

- **position** `p` (internal) is the corner with minimum x, minimum y and **maximum** z of the box,
  *after* the element's rotation has been applied about its origin.
- **size** `s` (all components > 0, see 3.3) is the box extent; the unrotated box occupies
  `[px, px+sx] × [py, py+sy] × [pz−sz, pz]` (the box extends toward −Z internally).
- **origin** `o` (internal) is the rotation pivot; it defaults to the creation position.
- **rotation** `e` is an Euler triple in degrees, stored wrapped and quantised (3.4); a stored component lies
  in `[0, 360]` (360 itself can result from quantisation).
- World placement: a local box vertex `v ∈ {0,1}×{0,1}×{−1,0}` maps to `p + R · (v ⊙ s)`, where `R` is
  the rotation built from `e` with the engine convention *rotate about Z, then X, then Y*
  (quaternion `q = qY · qX · qZ`).
- The unrotated min corner `u` satisfies `p = o + R·(u − o)`, i.e. `u = o + R⁻¹·(p − o)`.
- MC values: `from = (px, py, −pz)` (only meaningful when unrotated), `to = from + s`,
  rotation origin `= (ox, oy, −oz)`.
- MC angle mapping used by Minecraft-oriented helpers: each stored angle `a ∈ [0,360)` becomes
  `a' = a − 360` if `a > 180`, else `a`; the MC rotation is `(−x', −y', +z')`. The "rotation axis" of an
  element is the first non-zero of x, y, z (letter `x`, `y`, `z`, or `-`), its angle is that component
  rounded to 0.1°.

### 1.4 Block-space helpers

Used by the UV auto-mapping (4.7): `bpos = (px/16, py/16, −pz/16)`, `bsize = s/16`.

### 1.5 Voxel index space

Defined in 7.2. In short: index `(i, j, k)`; `i` grows with +X, `j` with +Y, `k` grows toward **−Z**
internally (i.e. toward MC +Z).

### 1.6 Numeric rules

- The original forces the invariant culture at start-up: numbers use `.` as decimal separator, no
  grouping. Text fields parse with "try parse, 0 on failure" (floats and ints alike); empty text is 0.
- Floats are displayed with the platform default general format (up to 7 significant digits, trailing
  zeros removed). Exact digit-for-digit output: UNVERIFIED (see open questions).
- `round` is half-to-even for both float rounding and float-to-int rounding.
- After any drag ends on an object whose rotation is zero, its world position and origin are cleaned to
  4 decimals: `v ← round(v·10000)·0.0001` per component.
- Float-to-byte colour conversion (`Color` → 8-bit channels) uses `trunc(clamp01(c)·255)` (UNVERIFIED for
  the engine's implicit conversion; the explicit ARGB packer in 7.3 truncates).

---

## 2. Document model

### 2.1 Aggregate

A model (document) is **flat**: there is no grouping, bone or parent/child relation between objects.
It consists of:

| Part | Cardinality | Notes |
|---|---|---|
| model properties | 1 | general settings, model box, origin, per-format export groups |
| view properties | 1 | grid/snap settings and per-view camera parameters |
| blueprint | 1 | the placement box (2.3) |
| elements | ordered list | cuboids and planes (section 4) |
| textures | ordered list | named images used by element faces (section 5) |
| voxel world | 0 or 1 | sparse chunked grid (section 7) |
| meshes | ordered list | primitives and imported meshes (section 6) |
| model display | 1 | display transforms (display-settings specification) |

List order matters: the element list order is the list-panel order, the file order and the render order
used for transparency sorting. The mesh list is ordered the same way.

**Display ids.** Each element and each mesh gets an integer id from a per-kind counter when it is created
(first id 1). Ids are shown in the list panels and used for Shift-range selection. They are never stored in
files. Clearing all elements (new model, load, undo/redo restore) resets the counter to 0, so ids are
re-issued in list order after every load, undo or redo.

### 2.2 Model properties relevant to editing

(Key names are the persisted keys; full list in the file-format specification.)

| Key | Meaning | Default after "new model" |
|---|---|---|
| `g.o` | model origin (internal coordinates) | (0,0,0) |
| `g.mb` | model box `{min, max}` | absent (the live box is reset to the editing volume) |
| `g.rer` | **restrict rotations** ("Restrict rotations (required for Minecraft block/item model)") | **true** |
| `g.n` | model name | "Cubik Model" (when unset) |
| `g.d` | signature/description | from the user signature template when unset |
| `g.ee`, `g.ev`, `g.em` | exclude elements / voxels / meshes from export | false |
| `g.ehe` | exclude hidden elements from export | false |
| `g.pbt`, `g.pzf`, `g.me` | prevent bleeding textures, prevent z-fighting, minified export | false |
| `mc.t` | model kind: none 0, block 1, item 2, entity 3, optifine 4, map 5 | block |
| `mc.esb` | entity: same bone (also toggles the entity bone origin marker) | true |
| `obj.ao` | ambient occlusion used by the voxel chunk meshes (7.6) | true |

Double quote characters typed into the name or signature are replaced by `”` (U+201D).

### 2.3 Special scene objects

All share the common object base (section 3).

**Blueprint.** A box-shaped placement helper (label "Blueprint"). Its origin is always its position
(reading the origin returns the position). It is not deletable. Flags: canMove, canRotate, canScale,
restrictable and grid-snap-on-scale all true (scene values). Scene default: position `(-2, 0, 2)`, size `(1,1,1)`,
rotation 0. "Reset Blueprint to default" (Recover panel, undo "Reset blueprint") restores exactly these
values. A new model does *not* reset the blueprint. The blueprint's state is saved with the model and
with every element snapshot (key `bp`). Uses: new elements and primitives are created at its transform;
voxel copy/cut/paste, fill/clear/flip in blueprint and the voxel "Wrap" tool use its box.

**Model origin.** A diamond marker (label "Model Origin"), visible only while selected (selecting it
activates it; deselecting hides it). Only movable (no rotate, no scale, not deletable). Whenever its
position changes it writes its position into `g.o`; when (re)enabled it moves itself to `g.o`. Selected
via "Set Model Origin" (shortcut Ctrl+K). The model origin is the export pivot (export specifications).

**Model box.** An axis-aligned box `[min, max]` with six face handles (2.4).

**Entity bone origin marker.** Shown only when the model kind is entity and `mc.esb` is set. Display only.

### 2.4 Model box

- Stored as six floats `xmin…zmax`. Default (reset) = the editing volume `(-16,-16,-32)…(32,32,16)`.
- Setters clamp: `xmin ∈ [-16, xmax]`, `ymin ∈ [-16, ymax]`, `zmin ∈ [-32, zmax]`,
  `xmax ∈ [xmin, 32]`, `ymax ∈ [ymin, 32]`, `zmax ∈ [zmin, 16]`.
- Six handles sit at the face centres: x-min handle at `(xmin, ymid, zmid)` and so on (`mid = (min+max)/2`).
  Handles are shown only when "show handles" is on (Gizmos tools toggle, default off); the box outline when
  "show box" is on (default on).
- Handle drag (9.6) commits all six handle coordinates through the clamped setters in the order xmin, ymin,
  zmin, xmax, ymax, zmax, repositions the handles, notifies the voxel world (new active box, 7.2) and writes
  the box into `g.mb`. Model box edits are **not** recorded in undo (QUIRK).
- Loading or restoring model properties re-applies `g.mb` to the box (a missing box leaves the current box).
- Voxel flips/rotations transform the box together with the voxels (7.9).

### 2.5 Change categories

Every undo step snapshots a set of categories (bit flags):

| Category | Value | Content snapshotted |
|---|---|---|
| modelProperties | 1 | model properties (incl. model box and origin) **except** the display transforms and the item overrides |
| viewProperties | 2 | grid settings, cameras |
| elements | 4 | blueprint + all elements (or only the selection, 10.2 and 11.2) |
| textures | 8 | the texture list |
| voxels | 16 | the whole voxel world |
| meshes | 32 | all meshes (or the selection) |
| modelDisplay | 64 | display transforms (the eight display slots) |

Named combinations: texturesAndMeshes 40, elementsAndTextures 12, elementsAndVoxels 20, objects 36
(elements+meshes), objectsAndVoxels 52, all 255.

Display transforms are captured only by modelDisplay (used by "Edit display settings"); the item overrides
(predicates and override models) are captured by **no** category, so no undo step restores them (QUIRK).

---

## 3. Object base

Common to elements, meshes, blueprint and model origin.

### 3.1 Fields

| Field | Type | Default | Notes |
|---|---|---|---|
| label | string | "Cube" | display name |
| id | int | assigned on creation | display only (2.1) |
| position `p` | vec3 | creation value | internal; see 1.3 |
| size `s` | vec3 | creation value | min 0.001 per component when set through the size setter |
| rotation `e` | vec3 deg | 0 | normalised and quantised (3.4) |
| origin `o` | vec3 | = position at creation | pivot |
| hidden | bool | false | not rendered, not pickable in views |
| locked | bool | false | blocks mouse drags (not keyboard nudges, QUIRK 9.4) |
| canMove / canRotate / canScale | bool | true | gate gizmos and drags (model origin: move only) |
| restrictable | bool | per kind (below) | participates in restricted-rotation mode (3.4) |
| gridSnapOnScale | bool | per kind (below) | scale drags snap only if set |
| deletable | bool | true | blueprint and model origin: false |
| dragSensitivity | float | 1 | multiplies scale-drag deltas |
| gizmoSizeFactor | vec3 | (1,1,1) | scale gizmo extent multiplier (meshes) |
| gizmoPivotOffset | vec3 | (0,0,0) | scale gizmo offset (meshes: import offset) |
| markedForConversion | bool | false | transient, used by voxelization (14.1) |

Flag values per object kind (prefab and scene values; they are never changed at run time except where noted):

| Kind | restrictable | gridSnapOnScale | deletable | canMove / canRotate / canScale |
|---|---|---|---|---|
| element | **true** | true | true | all true |
| blueprint | **true** | true | false | all true |
| primitive mesh (all ten types, sprite included) | **false** | true | true | all true |
| imported mesh (OBJ import, mesh records adopted on load, undo or paste) | **false** (set at import) | false | true | all true |
| model origin | false | false | false | move only |

Consequently restricted-rotation mode (3.4) only ever applies to elements and the blueprint; every mesh uses the
5° grid.

Derived predicates:
- **rotated** = rotation ≠ (0,0,0). A stored component of 360 counts as non-zero here and in the per-axis
  tests of 3.4 (QUIRK).
- **invalid Minecraft rotation** = rotated on two or more axes, or any component in the open interval
  `(45, 315)`, or any component not a multiple of 22.5. Counted by "Recalculate elements rotation" (3.5).
- **selected** = contained in the selection list (section 8).

### 3.2 Position and origin setters

- **set position (propagating)**: if the new position equals the old one, nothing happens. Otherwise, if
  propagation is requested and this object is the primary selection, every *other* selected object is
  translated by the same delta (both its position and its origin). Then the position is written, gizmos are
  refreshed if selected, and a position-changed notification is raised (elements recompute their auto-UV
  then, 4.7).
- **set origin and move** (`setOriginMove(target, propagate)`): `offset = p − o`; `o ← target`;
  set position (propagating) to `target + offset`. Used by drags, nudges and the move dialogs.
- **move keeping origin offset** (`moveTo(target)`): `o ← target + (o − p)`, then set position to `target`.
- The settings panel's Position field writes `p` directly (no propagation, no notification); the Origin
  field writes `o` directly (element does not move).

### 3.3 Size setter

Given a requested size `n` (only acts if `n ≠ s`):
1. If this object is the primary selection, every other selected object receives `its size + (n − s)`
   through its own size setter.
2. Pivot-relative rescale: `ratio = n ⊘ s` (component-wise division using the *unclamped* request);
   `p ← o + (p − o) ⊙ ratio`.
3. `s ← (max(nx, 0.001), max(ny, 0.001), max(nz, 0.001))`.
4. Refresh gizmos if selected; raise size-changed (elements recompute auto-UV, 4.7).

Several dialogs write the transform scale directly, bypassing steps 1–3 (no clamp; QUIRK, see 10.13).

### 3.4 Rotation setter (normalisation, quantisation, restriction)

`setRotation(r, propagate, force, aboutPivot)`; does nothing if `r` equals the stored rotation.

1. **Propagation**: if `propagate` and this object is the primary selection, every other selected object
   first gets its origin **overwritten with this object's origin**, then receives `its rotation + (r − e)`
   through its own (propagating) setter. Net effect: a multi-selection rotates rigidly about the primary's
   pivot, and the other objects' pivots are permanently moved there (QUIRK).
2. **Wrap**: each component `c ← (c + 360) mod 360` (remainder keeps the sign, so inputs below −360 stay
   negative; QUIRK).
3. **Quantise**:
   - If restricted mode applies (`g.rer` and the object is restrictable, i.e. an element or the blueprint,
     3.1): unless `force`, the change is **rejected silently** when the *current* stored rotation is non-zero on
     some axis and the new (wrapped, not yet rounded) value is non-zero on another axis. Each component is then
     rounded to a multiple of **22.5°**.
   - Otherwise (every mesh, and elements/blueprint while `g.rer` is off) each component is rounded to a
     multiple of **5°**; there is no rejection.
   (Rounding is `round(c/step)·step`, half to even. The result can be exactly 360: on the 5° grid every value in
   `[357.5, 360)` becomes 360, on the 22.5° grid every value in `[348.75, 360)`; 360 is stored as is.)
4. Store the rotation. If `aboutPivot`: rotate the transform about the origin
   (`p ← o + R_new·R_old⁻¹·(p − o)`, rotation ← `R_new`); otherwise only the orientation is replaced
   (position untouched).
5. Raise rotation-changed. For elements this also **turns UV snap off** (4.7).

Consequences: rotations loaded from files or restored by undo, redo or paste are re-quantised (forced, so
never rejected): to 22.5° for elements and the blueprint while `g.rer` is on, else to 5°. Meshes are always
re-quantised to 5°, so a stored mesh angle of 22.5 loads as 20 (half to even) and 67.5 as 70.

### 3.5 Restrict rotation of one element ("bake")

Used after a rotation drag in restricted mode, on all elements when the restriction is switched on, and by
"Force" in the model-properties confirmation.

1. If this element is the primary selection, apply the same operation to every other selected object first.
2. Let `v = e`. If `v.y ≠ 0` and (`v.x ≠ 0` or `v.z ≠ 0`): `v = (0, v.y, 0)`. Then if `v.x ≠ 0` and
   `v.z ≠ 0`: `v = (v.x, 0, 0)`. (Priority Y, then X, then Z.)
3. Round each component to a multiple of 22.5°.
4. For axis Y, then X, then Z: if the component `c` lies in the open interval `(45, 315)`:
   `k = trunc(c − 45) div 90 + 1`, `q = (k·90) mod 360`, `c ← c − q`, and apply a **quarter-turn bake** of
   `q` degrees about that axis (4.9) without moving the element's pivot (see step 6).
5. If the rotation changed, set it with `force = true`, about the pivot, without propagation.
6. If the origin changed during the bake, restore it with *set origin and move* (the element moves by the
   origin's displacement), which turns each bake into a rotation about the element's own origin.

Result: at most one rotated axis, angle in `[0,45] ∪ [315,360)` on the 22.5° grid, extra quarter turns
expressed by permuting faces and swapping size axes.

---

## 4. Elements

### 4.1 Fields

In addition to the object base:

| Field | Type | Default | Key |
|---|---|---|---|
| type | Cube 0, PlaneX 1, PlaneY 2, PlaneZ 3 | Cube | `t` |
| colour | RGBA float | random, see 10.1 | `c` (ARGB int, alpha forced 255, 0 if alpha 0) |
| faces | 6 × Face (4.4) | see 4.4 | `f` |
| uvSnap | bool | false (but every creation command sets it true) | `suv` |
| uvSnapMirror ("UV Snap Symmetry") | bool | false | `suvm` |
| rescale ("Rescale element based on rotation") | bool | false | `rs` (stored, used by exporters only) |
| shade | bool | true | `sh` (UI "Disable shading" is the inverse) |
| invertNormals ("Invert faces normals", "Inner normals") | bool | false | `in` |
| selected-at-save | bool | — | `is` (re-selects the element on load/undo/redo) |

Persisted element record keys: `p, s, r, o, c, is, ih, il, l, f, rs, suv, suvm, sh, in, t`
(position, size, rotation, origin, colour, selected, hidden, locked, label, faces, rescale, uvSnap,
uvSnapMirror, shade, invertNormals, type).

### 4.2 Element types

| Type | Enabled faces at creation | Size at creation | Default label | List icon |
|---|---|---|---|---|
| Cube | all six | blueprint size | "Box"+id | primitive_cube |
| PlaneX | west, east | x component set to 0 → clamped 0.001 | "PlaneX"+id | element_planex |
| PlaneY | down, up | y → 0.001 | "PlaneY"+id | element_planey |
| PlaneZ | north, south | z → 0.001 | "PlaneZ"+id | element_planez |

Planes are ordinary boxes with a 0.001-unit thickness and four disabled faces; the user may re-enable
faces or resize them like any element.

### 4.3 Element mesh

Each element owns a 24-vertex mesh (4 vertices per face, faces in the order down, up, north, south,
west, east) in its local unit box, scaled by the transform.

Unit-box corners: `c0 (0,0,0)`, `c1 (1,0,0)`, `c2 (1,0,−1)`, `c3 (0,0,−1)`, `c4 (0,1,0)`, `c5 (1,1,0)`,
`c6 (1,1,−1)`, `c7 (0,1,−1)`.

| Face slot | Vertices (outer normals) | Vertices (inverted normals) | Normal (both modes) |
|---|---|---|---|
| 0 down | c2 c3 c0 c1 | c6 c7 c4 c5 | (0,−1,0) |
| 1 up | c7 c6 c5 c4 | c3 c2 c1 c0 | (0,+1,0) |
| 2 north | c4 c5 c1 c0 | c7 c6 c2 c3 | (0,0,+1) |
| 3 south | c6 c7 c3 c2 | c5 c4 c0 c1 | (0,0,−1) |
| 4 west | c7 c4 c0 c3 | c6 c5 c1 c2 | (−1,0,0) |
| 5 east | c5 c6 c2 c1 | c4 c7 c3 c0 | (+1,0,0) |

With inverted normals each slot is drawn on the *opposite* plane with reversed orientation (visible from
inside the box) while keeping its own normal vector, UVs, texture and tint.

- **Triangles**: for every *active* face with base index `b = 4·slot`: `(b+3, b+1, b)` and
  `(b+3, b+2, b+1)`. Inactive faces contribute no triangles. Rebuilding the triangle list from the active
  flags is cheap and is done after any face-activity change.
- **Rebuild with all faces**: changing invertNormals rebuilds the mesh with *all six faces active*
  (it also sets every face's active flag to true; QUIRK — disabled faces come back).
- **UVs** per vertex, from the face's rotated UV corners (4.5) mapped into the runtime atlas (5.2):
  - down, north, south, west, east: vertices 0..3 of the slot get corners `C2, C3, C0, C1`;
  - up: vertices 0..3 get `C0, C1, C2, C3`.
- **Vertex colours**:
  - with "Emulate elements tint colors" on (default): each face's four vertices get the tint colour of its
    tint index: −1 → element colour if "Use elements color" is on, else white; 0 (106,170,64);
    1 (255,105,180); 2 (100,149,237); 3 (255,215,0); 4 (210,105,30); 5 (72,209,204); any other index
    (147,112,219). All opaque.
  - with emulation off: all vertices get the element colour (if "Use elements color", else white) with the
    element colour's alpha.
  - Toggling either preference recolours all elements immediately.
- A picking collider uses the same mesh.

### 4.4 Faces

| Field | Type | Default | Key |
|---|---|---|---|
| name | string | "down", "up", "north", "south", "west", "east" (set by the face set) | `n` |
| active | bool | true | `a` |
| tint index | int | −1 (none) | `ti` |
| cull direction | face flag (1.2) | none (0) | `c` |
| texture label | string | see below | `t` |
| uv | UV rectangle (4.5) | `(0,0)…(1,1)`, rotation 0 | `uv` |

Face set keys: `d, u, n, s, w, e`. A face count = number of active faces.

Default texture label of a *new element's* faces: if the texture list is empty → "texture"; else if the
first list entry is not "particle" → that entry's label; else if a second entry exists → its label; else
"particle". (With the default list `[particle, texture]` this is "texture".) A face record read from a file
without a label uses "texture".

Face copy semantics: copying a face copies active, tint, cull, texture and uv (rotation and both corners),
and the name unless "keep name" is requested (all internal face moves keep names).

### 4.5 UV rectangle

Stored per face, in **normalised texture coordinates of the face's own texture** (1 = full width/height,
V axis up, i.e. engine texture space with the origin bottom-left).

| Field | Meaning |
|---|---|
| rotation `r` | int degrees; setter stores `(round(r/90)·90 + 360) mod 360` (values ≤ −405 can stay negative; QUIRK) |
| `tl` ("min" corner) | raw stored value clamped to `[-1, 2]` per component on write |
| `br` ("max" corner) | raw stored value clamped to `[-1, 2]` per component on write |

Despite the key names, `tl` holds the corner with the *smaller V* for an unmirrored rectangle; mirroring is
expressed by swapping components (`tl.x > br.x` = horizontally mirrored, etc.).

**Read-back wrapping.** Reading `tl`/`br` folds rectangles that were dragged outside `[0,1]` back in, per
axis, using both raw values `a` (tl) and `b` (br):
- `tl'`: if `a < 0` and `b > 0` → 0; else if `a < 0` → `a + 1`; else if `a > 1` or (`a = 1` and `b > 1`) →
  `a − 1`; else `a`.
- `br'`: if `a < 1` and `b > 1` → 1; else if `b > 1` → `b − 1`; else if `b < 0` or (`b = 0` and `a < 0`) →
  `b + 1`; else `b`.
All consumers (mesh UVs, files, copy) use the folded values.

**Rotated corners.** Let `P00 = (tl.x, tl.y)`, `P10 = (br.x, tl.y)`, `P11 = (br.x, br.y)`,
`P01 = (tl.x, br.y)` and the cycle `Q = [P00, P10, P11, P01]`, `k = r/90`. The rotated corners are
`C0 = Q[k]`, `C1 = Q[k+1]`, `C2 = Q[k+2]`, `C3 = Q[k+3]` (indices mod 4).

**Pixel view** (Minecraft `uv` convention, 16-px face space, Y down):
`[x1, y1, x2, y2] = [tl.x·16, (1−br.y)·16, br.x·16, (1−tl.y)·16]`.

**Setters from rectangles** (used by tools):
- from a 0..1 rect `(x, y, w, h)` with Y down: `tl = (x, 1−(y+h))`, `br = (x+w, 1−y)`;
- from a 16-px rect: same after dividing by 16;
- from an atlas rect `(xmin, ymin, xmax, ymax)` scaled by `(sx, sy)` without flip:
  `tl = (xmin·sx, ymin·sy)`, `br = (xmax·sx, ymax·sy)` (a rect with negative width/height keeps its corner
  order, which is how mirrored atlas rects are expressed).
- **Full** reset: `tl = (0,0)`, `br = (1,1)`; rotation untouched.

### 4.6 Face UV to runtime-atlas UV

For a face with texture label `L` and a rotated corner `c`: if `L` is empty use "error"; if `L` is not in
the atlas use the fallback label "error"; with the entry's atlas rect `R` (normalised):
`u = clamp(R.x + c.x·R.w, R.x, R.x + R.w)`, `v = clamp(R.y + c.y·R.h, R.y, R.y + R.h)`.
If "error" itself is missing, an error is logged and `(0,0)` is used. Clamping means UVs outside a texture
never bleed into neighbouring atlas entries (no tiling).

### 4.7 UV snap (automatic default mapping)

`defaultUV(force)` runs only if `uvSnap` or `force`. All six face rotations become 0 and, with
`(x, y, z) = bpos`, `(w, h, d) = bsize` (1.4; computed from the *current position*, which for a rotated
element is the rotated corner, not the unrotated one):

| Face | tl | br |
|---|---|---|
| down | (x, z) | (x+w, z+d) |
| up | (x, 1−z−d) | (x+w, 1−z) |
| south | (x, y) | (x+w, y+h) |
| west | (z, y) | (z+d, y+h) |
| north, mirror off | (1−x−w, y) | (1−x, y+h) |
| north, mirror on | (x+w, y) | (x, y+h) |
| east, mirror off | (1−z−d, y) | (1−z, y+h) |
| east, mirror on | (z+d, y) | (z, y+h) |

Then the mesh UVs are refreshed.

Triggers: every position-changed and size-changed notification of an element runs `defaultUV(false)`,
so a snapped element re-maps continuously while moved or resized. Setting `uvSnap` or `uvSnapMirror`
refreshes mesh UVs when the value changes; the menu commands "Enable UV-snap"/"Enable UV-snap mirror" also
call `defaultUV(false)`. "Apply Default UV"/"Redefine UV Data" call `defaultUV(true)`.

Snap is turned **off** by: any rotation change (3.4), quarter-turn bakes, every UV-tab operation except
flip/rotate/swap/copy (10.17), every UV view write-back (drag, arrow nudge, double click; 10.16), "Paste texture
labels", "Inflate", the atlas builder, and voxel-to-element conversions. All creation commands turn it **on**.

Every element runs `defaultUV(false)` once when it starts (the frame after it is created, pasted, loaded or
restored by undo), so an element whose stored `suv` is true re-derives its UVs from its position instead of
keeping the stored rectangles.

### 4.8 Element predicates

- **Far element**: the stored position `p` or the point `(px+sx, py+sy, pz−sz)` is outside the editing
  volume (box centre (8,8,−8), size 48, bounds inclusive). Rotation is ignored. Used by the element list
  (error colour for block/item models) and by "Get back far elements" (10.10).
- **Transparent element**: any face's texture crop (5.5) contains a pixel with alpha < 1 (UNVERIFIED exact
  threshold). Used by "Sort transparent elements".
- **Invalid Minecraft rotation**: 3.1.

### 4.9 Quarter-turn bakes (rotate 90° about a world axis)

A quarter turn transforms an element as if rotated by 90° about the **block centre** `C = (8, 8, −8)`,
keeping it axis-aligned by permuting faces and size axes. It first resets the element rotation to zero
(about its pivot, forced, no propagation) — the element's own rotation is **lost** (QUIRK) except inside
the restriction bake (3.5), which re-applies the residual angle afterwards.

Common geometry with `Rθ` the engine rotation for +90° about the axis:
`p' = Rθ·(p − C) + C`, `o' = Rθ·(o − C) + C`, then the box corner/size fix-ups below.

| Axis | Rotation (internal) | Position fix-up | New size | Face cycle (new ← old) | UV changes |
|---|---|---|---|---|---|
| X | `(x, y, z) → (x, −z, y)` | `p' += (0, 0, sy)` | `(sx, sz, sy)` | south←down, down←north, north←up, up←south | west r −= 90, east r += 90; the faces arriving at down and north have tl/br swapped (180° flip) |
| Y | `(x, y, z) → (z, y, −x)` | `p' += (−sz, 0, 0)` | `(sz, sy, sx)` | north←west, west←south, south←east, east←north | down r −= 90, up r += 90 |
| Z | composed: X(+90), Y(+90), X(−90) = `(x, y, z) → (y, −x, z)` | via the composition | via the composition | via the composition | via the composition |

Angles are applied as multiples of 90: `n = ((a div 90)·90 + 360) mod 360` quarter turns (`a div 90`
truncates); Z for 180 = X(180) then Y(180); Z for 270 = X(−90), Y(+90), X(+90). Every quarter turn turns UV
snap off and refreshes face mask, UVs and gizmos. A "faces only" variant (used by UV projections, 10.17)
cycles the faces and UV rotations without touching geometry.

MC-space equivalents: X turn `(x, y, z_mc) → (x, z_mc, −y)`; Y turn `(x, y, z_mc) → (−z_mc, y, x)`;
Z turn `(x, y, z_mc) → (y, −x, z_mc)` (all about the block centre).

---

## 5. Textures, materials and the atlas

### 5.1 Texture entry

| Field | Meaning | Persisted key |
|---|---|---|
| label | unique key; labels entered in the UI keep only `A–Z a–z 0–9 - _` (5.9); labels coming from files are not re-validated (UNVERIFIED for every importer) | `lb` |
| image | the entry's own image (Point filtered). It carries an internal **image name** (5.14) | `tx` (image name `tx.n`) |
| source path | file the image was loaded from, `/` separators; empty for generated, placeholder, emptied, shortcut and painted entries | `fp` |
| file name | the source file name without extension while a source path exists, otherwise a stored name (null after "Empty texture") | `fn` |
| Minecraft path | a Minecraft texture id (for example `block/stone`) or a shortcut `#label` (5.7). When empty and a source path exists it is derived on first read and then kept: the text after the last `/textures/` (found at an index above 0) without its extension, else the file name without extension (QUIRK: a `/textures/` path without any `.` after it throws) | `fm` |
| file size | byte length of the source file (display only; 0 after a reset) | — |
| content signature | MD5 of the source file bytes (5.12); null for images set from memory | `cs` |
| atlas rect | normalised rect inside the runtime atlas, rewritten by every repack | — |

A derived **type** text exists but is not shown by the preview panel: "Shortcut" when the Minecraft path starts
with `#`, "-" without a source path, else the upper-cased source extension (QUIRK: a source path without an
extension throws).

**Special entries.**
- **"error"**: an atlas entry that is never listed, using the built-in error image (16×16). After every clear it
  is the first atlas entry. UV and image lookups of unknown or empty labels fall back to it (4.6, 5.5).
- **"particle"** and every other default name: listed entries holding a copy of the built-in particle image
  (label "particle") or of the built-in face image (any other label), 5.7.
- **"atlas"**: the only listed entry after the atlas generator (5.4).
- **Shortcut placeholder**: a built-in image shown by a shortcut whose target label does not exist (5.7).

Default lists: **new model** = `[particle, texture]`; **clear** = empty list plus the hidden "error" entry.
The texture list, its popups and every texture command are specified in 5.6–5.15; their undo labels are listed
in 15.6.

Behaviour that matters for editing:
- "Edit texture label" renames the atlas key only; faces using the old label then resolve to "error"
  (QUIRK). "Rename texture" rewrites the label on every face of every element first, even when the new
  label is then rejected (5.9).
- Deleting a texture leaves faces naming it; they resolve to "error".
- Changing any texture image repacks the atlas and refreshes the UVs of all elements.

### 5.2 Runtime atlas (editor rendering)

All textures are packed into one runtime atlas used as the main texture of the shared element material;
element vertices carry atlas UVs (4.6).

- Slot limit: `max(512, deviceMaxTextureSize / 2)`, read once at start-up; a loaded image whose *width*
  exceeds it is down-scaled (aspect kept) before insertion (5.10; QUIRK: height is not tested). The slot limit
  is not applied by the packer itself, and nothing limits the number or the bytes of entries.
- Adding an entry appends it and **repacks all entries** with 1-pixel padding into a fresh ARGB atlas
  (Point filter, clamp, maximum size = the device limit), then each entry's normalised rect is updated.
  Replacing an entry's image (load, reload, shortcut, "Empty texture") also repacks. Removing an entry does
  **not** repack (stale pixels remain until the next repack). What happens when the packed set does not fit
  the device limit is UNVERIFIED.
- The label dictionary is rebuilt from the ordered entry list at every repack, which is how a rename takes
  effect. Adding an entry whose label already exists stores nothing, still repacks, and returns the
  candidate as if it had been added (only reachable when the callers' existence checks are bypassed). An
  entry without an image would shift the packed rects of the following entries (QUIRK, not reached by normal
  flows).
- The atlas refuses an entry (returns none) only when the candidate has no image: an empty label, a null
  image or an image file that produced no image (5.10). There is **no** count limit.
- Packing algorithm: the engine's built-in rectangle packer. A port may use any deterministic packer: the
  runtime atlas is never persisted, so its layout only has to produce identical *rendering*.
- Lookups by label use a dictionary; a missing label falls back to "error" (4.6).

### 5.3 Paint write-back

Painting on elements writes into the runtime atlas image (13.2). When a stroke ends, every atlas entry's
image (the hidden "error" entry included) is replaced by its sub-rectangle cut out of the atlas (pixel rect =
`trunc(rect·atlasSize)`).

Side effects of the replacement (QUIRK, reproduce): each new image is unnamed (the export then falls back to
the label, 5.14) and every entry loses its source path and its signature, so after any paint stroke no texture
is reloaded from disk any more (5.12) and every entry counts as generated (type "-"). Minecraft paths are kept.
Undoing the stroke restores the previous entries with their paths (15.3).

### 5.4 Atlas generation ("Create textures atlas", "Create blank atlas")

Both are confirmed in a dialog (titles "Create textures atlas" / "Create blank atlas"; buttons "Continue",
"Cancel"; texts in 5.8) and recorded as "Create full atlas" / "Create blank atlas"
(category elementsAndTextures).

1. Status "Calculating Atlas...". Padding = 2 px if the preference "advanced atlas padding" is on, else 0;
   square = preference "advanced atlas square" (default on).
2. For each element in list order: UV snap off; for each face in order down, up, north, south, west, east:
   - **full**: the face image is the face's texture crop (5.5); a face whose crop is null (inactive face or
     rect out of bounds) is set inactive and skipped;
   - **blank**: only active faces; each gets its own solid image of a random opaque colour (each channel
     uniform in `[0.5, 1]`) sized `max(trunc(a),1) × max(trunc(b),1)` pixels where `(a, b)` = `(sx, sz)` for
     down/up, `(sx, sy)` for north/south, `(sz, sy)` for west/east (1 unit = 1 px).
   Every 100 elements: status "Preparing texture of each element face...", progress, yield.
3. Pack all face images (padding as above, max size = device limit) into a new ARGB atlas. If padding > 0:
   status "Bleeding pixels in textures padding..." and fill each fully transparent pixel outside every
   packed rect with the colour of its first non-transparent (alpha > 0) neighbour, checked left, right, below,
   above in the flattened bottom-up pixel array (QUIRK: left/right wrap across row ends, the pixel at index 0 is
   never used as a source). One pass reading the unmodified image, so only a 1-pixel ring is filled. This
   happens before the square step.
4. Square mode: if the atlas is not square, copy it into the bottom-left corner of a square canvas of the
   longer side and remember the ratio (shorter/longer) for that axis.
5. If the packed rect count differs from the face count: error "Number of packed UVs mismatch number of
   element faces" (thrown; the faces disabled in step 2 stay disabled, nothing is rolled back).
6. Each included face: texture label ← "atlas"; uv ← packed rect scaled by the square ratios (4.5);
   rotation is **kept** (QUIRK: rotated faces may end up re-rotated, UNVERIFIED visually).
7. **Clear the texture list** (the "clear" state of 5.1; the list is really emptied, as the confirmation text
   says), then add the atlas as "atlas", listed and selected, and refresh all elements. Status "Done...".

Hidden elements are included. The atlas image is unnamed (its export name is the label "atlas", 5.14).

The packed layout is not reproducible bit-for-bit; parity tests compare rendered results.

### 5.5 Face texture crop

Crop of a face's own texture (not the atlas): take the rotated corners `C0` and `C2`; pixel rect
`x0 = trunc(min(C0.x, C2.x)·W)`, `y0 = trunc(min(C0.y, C2.y)·H)`, `x1 = trunc(max(..)·W)`,
`y1 = trunc(max(..)·H)`, width/height at least 1. Returns null when the face is inactive or the rect exceeds
the texture. The pixels are mirrored horizontally if `C0.x > C2.x`, vertically if `C0.y > C2.y`, both if
both (no rotation is applied to pixels). A face whose label has no entry is cropped from the "error" image
(it is not treated as missing). The crop is a new ARGB image.

### 5.6 Texture list, selection, preview

- The **texture list** (Elements Material panel) is an ordered list of rows; each row references one atlas
  entry and carries a selected flag. The hidden "error" entry is never listed. Row height 22 px; a row shows the
  label, a selected row uses the pressed button colour. New rows are appended.
- The atlas keeps its own order (index 0 = "error"); a renamed entry moves to the end of it. That order only
  affects packing.
- **Selection**: one entry or none. Selecting a row selects its entry and refreshes the preview. Every add made
  by a texture command selects the new (last) row. After any removal the selection moves to list index 0 (none
  when the list is empty). An empty list clears the selection and the preview.
- List index 0 is special: "Remove unused textures" never removes it.
- Notifications: *texture changed* (label) when an entry's image is replaced; *list changed* (label, or none
  after a removal) on add, remove and rename. Every list refresh also rebuilds the **texture list dropdown**
  (one item per label, list order); which control consumes its choice is UNVERIFIED.

**Preview panel** (selected entry):

| Field | Text |
|---|---|
| label | the label |
| file name | the file name (5.1) |
| path | shortcut: its Minecraft path (`#name`); otherwise the folder part of `<Minecraft path>.png` followed by `/` (a generated entry shows `/`) |
| size | `W` + `x` + `H` of the image, no spaces |
| weight | shortcut: "Shortcut"; otherwise the file size divided by 1024 (integer division) while ≥ 1024, with unit "B", "KB", "MB" or "GB" (1536 bytes → `1KB`; never decimals) |
| extension | never written (stays empty) |
| image | the entry's image; for a shortcut whose target exists, the target's image |

Previewing a shortcut whose target exists copies the target's image object into the shortcut again and repacks
the atlas (elements are not refreshed). Changes to a target therefore reach its shortcuts only when they are
previewed (QUIRK).

### 5.7 Texture commands

**Menus** (item order as shown):
- **Texture popup** (row options gesture, 5.13): "Load texture...", "Save texture...", separator, "Set as
  shortcut", "Edit label...", "Rename texture...", separator, "Apply to selection", separator, "Edit
  texture...", "Empty texture", "Delete texture".
- **Material popup**: "Add missing textures", "Remove unused textures", "Delete all textures", separator,
  "Replace textures list" (submenu), separator, "Create atlas, blank textures", "Create atlas, keep textures".
- **"Replace textures list"**: "Texture (default)", "Side + Up + Down", "Side + Cap", "Front + Side + Cap",
  "Down + Up + North + South + West + East".
- The panel's **"+"** button adds a new texture.
- Item enable rules are those of the generic submenu (every item has the "always active" flag off; which rule
  greys them out is UNVERIFIED).

**Default add** (used by "+", "Add missing textures" and the list presets): does nothing when the label already
exists in the atlas. Otherwise it appends a listed entry holding a **copy** of the built-in particle image (label
"particle") or of the built-in face image (any other label), selects it, and refreshes elements and the
dropdown. If the atlas refuses it (empty label, 5.2): Error box "Too many textures".

"Throws" below means the original stops with an unhandled error at that point; an undo entry pushed earlier stays
(see 5.15 for the proposed port behaviour). Undo category is textures unless noted; "before" = pushed before any
dialog, prompt or question, so cancelling still leaves the entry.

| Command | Behaviour | No selected entry | Undo | Messages |
|---|---|---|---|---|
| Load texture... | open the texture dialog (5.13); on accept load the file into the selected entry (5.10; label kept, Minecraft path re-derived), remember the folder | throws | "Load texture", before | status `Material Manager: New texture loaded: <path>` (written before loading, also on failure); cancel: status `Material Manager: New texture cancelled`; load errors 5.10 |
| Save texture... | save dialog (5.13), then write the entry's current image (5.11) | throws | none | 5.11 |
| Set as shortcut | prompt "Set as shortcut", default = the Minecraft path without its `#` when it starts with `#`, else empty. On accept: prefix `#` when missing; target = the text with **every** `#` removed. Empty target: shortcut removed (image ← built-in face image, shared, not copied; Minecraft path empty). Existing target: image ← the target's image object (shared), Minecraft path `#<target>`. Missing target: image ← shortcut placeholder, Minecraft path `#<target>`. Source path and signature are cleared; repack, element refresh, texture changed | throws | "Set as shortcut", before | none (names are not sanitised; self-reference and cycles are allowed) |
| Edit label... | prompt "Edit texture label" (default = label); on accept apply the label rule (5.9); faces are not changed | throws | "Edit texture label", before | "Label already exists" |
| Rename texture... | prompt "Rename texture" (default = label); on accept **first** set every element face whose label equals the old label to the raw input, **then** apply the label rule (5.9) | throws | "Rename texture", before | "Label already exists" (shown after the faces were rewritten) |
| Apply to selection | every selected element: all six face labels ← the selected label (existence not checked), UVs refreshed; other objects are ignored | throws only if an element is selected | "Apply texture to selection" (elements), before | none |
| Edit texture... | external editor round trip (5.12) | throws | none | "Missing Texture editor" |
| Empty texture | image ← built-in particle image if the label is "particle", else the built-in face image (shared, not copied); source path, signature, Minecraft path (a shortcut is removed), file name and size cleared; label kept; repack, element refresh | throws | "Empty texture", before | none, no confirmation |
| Delete texture | Question box; Yes: remove the entry from atlas and list (faces keep the label and render "error"), selection ← list index 0, refresh elements, list and dropdown | throws | "Delete texture", **before** the question | "Delete Texture" |
| "+" | new name (5.9), default add of that name | — | "Add new texture", before | "Too many unnamed textures" when the add raises any error |
| Add missing textures | collect the labels of all faces of all elements; if neither the faces nor the list use "particle", default-add "particle"; default-add every face label the list lacks (each add selects its row) | — | "Add missing textures", before | "Too many textures" for an empty face label |
| Remove unused textures | collect the labels of all faces; walk the list from the last row down to index 1 and remove every row (and its atlas entry) that no face uses, without refreshing elements; selection ← index 0 | — | "Remove unused textures", before | none (the dropdown is not rebuilt, QUIRK) |
| Delete all textures | **clear**: empty atlas and list, re-add the hidden "error" entry, clear selection and preview, refresh elements (face labels stay and render "error") | — | "Clear textures", before | none, **no confirmation** |
| Texture (default) | clear, then default-add "particle", "texture" | — | "New textures", before | none, no confirmation |
| Side + Up + Down | clear, then "particle", "down", "up", "side" | — | "New textures" | as above |
| Side + Cap | clear, then "particle", "cap", "side" | — | "New textures" | as above |
| Front + Side + Cap | clear, then "particle", "cap", "side", "front" | — | "New textures" | as above |
| Down + Up + North + South + West + East | clear, then "particle", "down", "up", "north", "south", "west", "east" | — | "New textures" | as above |
| Create atlas, blank textures | Warning box "Create blank atlas"; Continue → 5.4 blank variant | — | "Create blank atlas" (elementsAndTextures), after Continue | 5.8 |
| Create atlas, keep textures | Warning box "Create textures atlas"; Continue → 5.4 full variant | — | "Create full atlas" (elementsAndTextures), after Continue | 5.8 |

### 5.8 Texture message boxes

Box type in brackets; boxes with a single button show "OK".

- "Delete Texture" [Question, "Yes", "No"]: `Are you sure you want to delete texture named "<label>"?`
- "Label already exists" [Warning]: `A texture with the label "<sanitised input>" already exists, please try
  another.`
- "Too many textures" [Error]: `You reached the maximum number of textures.` + blank line + `Check your model to
  see if one or more textures can be deleted or re-used.` Shown whenever the atlas refuses an entry (5.2): an
  empty label, or an image file that produced no image. There is no count limit, so the text is misleading
  (QUIRK).
- "Too many unnamed textures" [Warning]: `Rename or delete one or more textures to add new ones.` Shown only when
  the "+" command raises an error (for example a missing built-in image).
- "Missing Texture editor" [Information]: `A Texture Editor is not setup.` + blank line + `Please set the path to
  your favorite Texture Editor in Preferences`
- "Error while loading texture" [Error]: `An error occured while loading "<file name>".` + blank line +
  `<error message>`; status `Failed loading texture: <path>`.
- "Error while saving file" [Error]: `An error occured while saving texture "<file name>".` + blank line +
  `<error message>`; status `Failed saving texture: <path>`.
- "Create blank atlas" / "Create textures atlas" [Warning, "Continue", "Cancel"]: `This will create a unique
  blank texture with random color per element face and will clear the Elements Material textures list.` /
  `This will create a unique texture assembling all textures shown on elements faces and will clear the
  Elements Material textures list.`, each followed by a blank line and `Do you wish to continue?`.

### 5.9 Labels, renames and new names

**Label rule** (Edit label..., and the second half of Rename texture...):
1. Remove every character other than ASCII letters, digits, `-` and `_` (case kept, nothing trimmed, no length
   limit).
2. If the result exists in the atlas (exact, case-sensitive): "Label already exists" with the sanitised text,
   stop. Submitting the current label again counts as existing (QUIRK).
3. Remove the entry from the atlas, set its label, add it again (repack), refresh list, dropdown and elements,
   raise list changed with the new label.
4. QUIRK: an input that sanitises to the empty string passes step 2; step 3 then removes the entry from the
   atlas, empties its label and fails on the re-add. The texture is gone from the atlas while the list keeps a
   row for it; only undo brings it back.

| Command | Element faces | When the rule rejects the input |
|---|---|---|
| Rename texture... | every face equal to the old label gets the **raw, unsanitised** input before the rule runs | the faces stay rewritten: they now use the existing texture of that name, or (when sanitising changed the text) a label without an entry (QUIRK) |
| Edit label... | unchanged; after a successful edit they keep the old label and render "error" (QUIRK) | nothing changes |

**New names** ("+"): the first of `particle`, `texture`, `texture1`, `texture2`, …, `texture100` that is not a
label in the atlas (atlas keys, not list rows, are checked). When all of them exist the code requests
`texture100`, the default add does nothing and no message is shown.

### 5.10 Loading images

**Entry load** (Load texture..., reload from disk, external-editor reload, and adding from a file during
imports):
1. Normalise the path (`\` → `/`). If the file does not exist: log and stop, nothing changes.
2. Normal mode: source path ← path, Minecraft path ← empty (derived again on next read, 5.1). Keep-metadata mode
   (external-editor reload only): the source path is **cleared** and the Minecraft path kept. Both modes: file
   size ← file length.
3. Decode (below). No image: log "Unable to load" and stop; step 2 has already changed path, size and Minecraft
   path while the old image stays (QUIRK).
4. Slot downscale (below).
5. Animated-texture crop (below).
6. Image name ← file name without extension (keep-metadata mode: the previous image name, or "texture" when there
   was none). Point filter; signature ← digest of the file (5.12).
7. The caller repacks the atlas, refreshes the preview and all elements and raises texture changed.

Adding from a file that does not exist creates the listed entry with a **copy of the error image** instead (log
`Texture file <label> not found: <path>`); the "error" entry itself is not used.

**Decoders** (chosen by extension, case-insensitive):
- **`.dds`**: the bytes must start with `DDS` (the fourth magic byte is not checked) and byte 4 (low byte of the
  header size) must be 124, otherwise an error. Height = 16-bit little-endian value at bytes 12–13, width = bytes
  16–17 (the high halves are ignored). Byte 87 (last FourCC character) equal to `5` selects DXT5, **any other
  value DXT1**, so DXT2–DXT4 decode wrongly and uncompressed or DX10 files are not supported. Every byte from
  offset 128 on is given to the decoder as one level (mip levels are not skipped; behaviour with surplus or
  missing bytes UNVERIFIED). The result is converted to an uncompressed 32-bit ARGB image.
- **`.tga`**: the ID field is skipped. A file with a colour map (colour-map type ≠ 0) logs "TGA with indexed
  colors not supported" and yields a blank image of the header size (no error box). Image types 2 (raw true
  colour) and 10 (RLE true colour; packet header bit 7 = run, count = low 7 bits + 1) are decoded; any other type
  logs and yields a blank image. Depth 32 reads B, G, R, A; depth 24 reads B, G, R with alpha 255; any other
  depth logs and yields transparent pixels **without consuming input**, so the rest is misread (QUIRK).
  Pixels are stored in file order starting at the image's bottom-left and the descriptor's origin bit is
  ignored, so top-left-origin files come out flipped vertically (QUIRK). Corrupt RLE data raises an error. The result is converted to
  32-bit ARGB. The content of a "blank" image is whatever the engine puts in a new image (UNVERIFIED).
- **any other extension** (PNG, JPG, but also BMP, GIF and so on): the engine's image decoder runs on the file
  bytes into a 1×1 RGBA image. Its success result is **not** checked, so an undecodable file silently becomes a
  1×1 image (QUIRK). The resulting pixel format is the engine's choice (UNVERIFIED; typically 24-bit RGB
  without alpha and 32-bit RGBA with alpha).
- Any read or decode error: Error box "Error while loading texture" (5.8), status
  `Failed loading texture: <path>`, no image.
- Every decoded image gets Point filtering.

**Slot downscale.** With the slot size `S` (5.2), only when `width > S`: `w' = S`, `h' = trunc(S·h/w)`; if
`h' > S` then `h' = S` and `w' = trunc(S·w/h)`. A tall image whose width fits is never scaled, even when its
height exceeds `S` (QUIRK). The image is drawn with **bilinear** sampling into a `w' × h'` render target cleared
to opaque 50 % grey (so translucent pixels blend over grey; exact blend UNVERIFIED), then read back in place
(same pixel format). Log `Texture too big to fit in atlas slot, scaled down to <w'>x<h'>`. QUIRK: the temporary
render target is never released and stays the active target.

**Animated textures.** When `<image path>.mcmeta` exists, the image is replaced by a square of side = width taken
from its **bottom** rows (the engine's bottom-up origin). Animation strips normally store frame 0 at the top, so
the square is usually the last frame (UNVERIFIED visually). The crop keeps the pixel format.

### 5.11 Saving textures as PNG

- The entry's **current** image (after downscale, crop, painting or an external edit) is encoded as PNG; the
  source file bytes are never copied, so DDS and TGA sources are saved as 32-bit PNG.
- ".png" is appended unless the name already ends with ".png" (case-insensitive): `a.jpg` → `a.jpg.png`.
- Success: status `Successfuly saved texture: <path>` (spelling as in the original). Failure: Error box "Error
  while saving file" (5.8). Overwrite confirmation is left to the file dialog (UNVERIFIED).
- The same writer produces the external-editor temp file (5.12), the export PNGs (5.14) and mesh texture saves
  (10.14).

### 5.12 Signatures, reload from disk, external editor

**Signature.** MD5 of the file bytes, turned into text by decoding the 16 digest bytes with the platform's
default text encoding (not hex, lossy), stored as `cs` and only compared for equality. Entries whose image was
set from memory (generated, placeholder, emptied, shortcut, painted) have none and are never reported as
changed. Port decision (proposed): compute raw digests and treat stored legacy strings as opaque; a legacy value
that cannot be reproduced only causes one reload.

**On every gain of application focus**, in this order:
1. **Reload changed files**: every listed entry whose source file exists, whose signature is not empty and whose
   file digest differs from it is reloaded through the entry load (5.10, normal mode) with its source path and
   Minecraft path. The first such reload pushes one undo entry "Reload textures from HDD" (textures). Every focus
   gain hashes every file-backed entry.
2. **External-edit check** (step 5 below).

**External editor** ("Edit texture..." and the UV tool "Texture edit", 10.17):
1. The editor-path preference must name an existing file or folder; otherwise Information box "Missing Texture
   editor" and stop.
2. Temp file: `cubik_texture.png` in the application's temporary cache folder. There is one shared file and only
   the most recently edited entry is tracked.
3. If the temp file does not exist, or the entry's signature differs from the temp file's digest: write the
   entry's image to the temp file (5.11) and remember the temp file's digest as the **synced digest**. (A
   file-loaded entry's signature is its source file's digest, so the temp file is rewritten on practically every
   edit; when the digests match nothing is written and the synced digest is not updated.)
4. Remember the edited entry, start the editor with the temp path as one quoted argument (backslash separators).
   No waiting, no error handling beyond the start error, no undo entry.
5. On focus: if an edited entry, the temp path and a synced digest exist, hash the temp file (a deleted temp file
   makes this check fail with an error; how the engine reports it is UNVERIFIED). If the digest differs from the
   synced digest: reload the edited entry from the temp file in **keep-metadata** mode (source path cleared;
   label, Minecraft path and image name kept; size and signature from the temp file), then synced digest ← the
   entry's new signature. No undo entry.

After an external edit the entry has no source path any more, so it is never reloaded from its original file
again (QUIRK).

### 5.13 Texture dialogs, start folders, row gestures

- **Open dialog** "Select texture file": filters "Image" (png), "Photo" (jpg), "DirectX texture" (dds), "Targa
  texture" (tga); default extension png. With the native-browser preference the OS dialog is used with an extra
  "All files" filter, otherwise the in-app browser (differences UNVERIFIED). Cancel: status
  `Material Manager: New texture cancelled`.
- **Save dialog**: filter "PNG Texture (*.png)", title "Chose a path to save selected texture", default name =
  the label (mesh textures: `<mesh label>_<mesh id>`), default extension png. Cancel only logs.
- **Start folders** (the first candidate that exists as a folder wins; remembered folders live for the session
  and are updated by every accepted choice):

| Dialog | Candidates |
|---|---|
| texture load | folder of the selected entry's source path → last texture-load folder → preference "textures folder" (default Desktop) → Desktop → `/` |
| texture save | folder of the selected entry's source path → last texture-save folder → textures folder → Desktop → `/` |
| mesh texture load | last mesh-texture-load folder → textures folder → Desktop → `/` |
| mesh texture save | last mesh-texture-save folder → textures folder → Desktop → `/` |

- **Row gestures**: a left click selects the row's entry; when the same row was activated at most 0.25 s
  before, it also opens the texture load dialog for the selected entry **without** an undo entry (unlike the
  popup command). Every activation updates the time stamp. The options gesture (right click or options button)
  runs the same logic, then shows the texture popup, so a right click within 0.25 s of a left click opens the load
  dialog too (QUIRK). How the engine's pointer events reach these handlers is not in the scene dump (UNVERIFIED).

### 5.14 Texture image name and export names

Every entry's image carries an internal name, persisted with the image (`tx.n`):

| Image origin | Image name |
|---|---|
| loaded from a file | the file name without extension |
| reloaded after an external edit | the previous name (or "texture" when there was none) |
| default add (5.7) | the built-in image's name: "e_particle" or "e_texture" |
| added from a missing file | "e_error" |
| "Empty texture" | the shared built-in image's name ("e_particle" for "particle", else "e_texture") |
| removed shortcut | "e_texture" (always the face image) |
| shortcut | the target image's name, or "e_shortcut" for the placeholder |
| voxel atlas conversion (14.3) | its label, or empty when the square step replaced the image |
| atlas generator, palette image, voxel cube textures, every entry after a paint stroke (5.3) | empty |

**Export name** (Minecraft JSON export, `formats_interop.md` 7.4): the image name with all whitespace removed
and lower-cased (invariant culture); when that is empty, the label transformed the same way. Nothing else is
removed, so dots, brackets or `#` survive. The PNG is written as `<export folder>/<export name>.png` with the
writer of 5.11. Example: an untouched default "texture" entry is exported as `e_texture.png`.

### 5.15 Texture defects and port decisions

| # | Original behaviour | Port |
|---|---|---|
| T1 | A rejected rename has already re-pointed faces (5.9) | proposed: sanitise and validate first, rewrite faces only after a successful rename |
| T2 | A label that sanitises to empty removes the texture from the atlas (5.9) | proposed: refuse the empty result and change nothing (the original has no message for this case; wording to be decided) |
| T3 | Texture-popup commands without a selected entry throw after pushing their undo entry (5.7) | proposed: do nothing and push nothing |
| T4 | A deleted temp file makes the focus check fail (5.12) | proposed: skip the check |
| T5 | "Too many textures" for decode failures and empty labels (5.8) | reproduce the box; the port may add the decoder's reason to the log |
| T6 | Undecodable images become 1×1, failed loads keep the new path and size (5.10) | reproduce |
| T7 | DDS FourCC, TGA origin and depth handling (5.10) | reproduce by default; standard-conforming decoding is an optional improvement that only affects newly loaded files (stored images are raw pixels) |
| T8 | Shortcuts share images and refresh only when previewed (5.6) | reproduce |
| T9 | External edits drop the source path (5.12) | reproduce |
| T10 | Undo entries pushed before dialogs, prompts and questions (5.7) | reproduce |

---

## 6. Meshes

### 6.1 Fields

Object base plus:

| Field | Meaning |
|---|---|
| primitive type | mesh 0 (imported), cube 1, sphere 2, cylinder 3, cone 4, pyramid 5, slope 6, corner-in 7, corner-out 8, torus 9, sprite 10 |
| geometry | triangle mesh with optional submeshes (imported) or the primitive's built-in mesh |
| main texture | 128×128 RGBA image for primitives (Point filter, clamp, image name = the type label, filled with the left colour at creation); its pixels are placed by the primitive's UV layout (6.6) |
| material colour (tint) | multiplies the texture; **not persisted** for primitives (QUIRK); per-submesh colours are persisted for imported meshes |
| model bounds | bounds captured at import (persisted) |
| normalisation factor | `1 / mean(import collider size)` (used by "block size"); it is stored in the object's drag-sensitivity field, so it also scales scale-drag deltas of imported meshes (3.1, 9.3) |
| import offset | `(minX − px, minY − py, maxZ − pz)` of the import bounds; also the scale-gizmo pivot offset |

No mesh is restrictable (3.1): primitives keep their prefab value false, and imported meshes (and mesh records
adopted on load, undo or paste) get false at import, so every mesh rotation uses the 5° grid and "Restrict
rotations" never affects meshes. Primitives snap on scale; imported meshes do not. Primitive objects carry drag
sensitivity 1, gizmo size factor (1,1,1), gizmo pivot offset (0,0,0) and zero model bounds. Persisted keys:
`p, s, r, o, is, ih, il, l, tp, tx, mb` and, for imported meshes, `me` (vertices, normals, triangles, uvs) and
`sm` (per-submesh triangles, colour, texture). Primitive geometry is never persisted (6.5).

### 6.2 Creation

- **Primitive** (`Create <Name> primitive`, category meshes): placed at the blueprint position with the
  blueprint size (the blueprint rotation is ignored); origin = position + offset (sphere, torus:
  `(sx/2, sy/2, −sz/2)`; cylinder, cone: `(sx/2, 0, −sz/2)`; others 0); sprite forces size y to 0.001; the
  paint texture is created (6.1); the object is selected exclusively and appended to the mesh list. Labels:
  "Cube", "Sphere", "Cylinder", "Cone", "Pyramid", "Slope", "Corner-in", "Corner-out", "Torus", "Sprite". No menu
  command creates a sprite. Restoring a mesh record of a primitive type (load, undo, redo, paste) runs the same
  builder (rotation ignored), then applies the record: position and origin (plus the paste offset), size,
  rotation (forced, 3.4), hidden, locked, label, type, paint texture, bounds.
- **Imported mesh** (OBJ, "Add custom mesh"): OBJ parsing and post-processing (scale heuristic, centring)
  are specified in the import specification. Each imported renderer becomes one mesh object, selected.
- Ids, list rows, next/previous and move to top/bottom behave as for elements (mesh variants).

### 6.3 Mesh tools (mesh tools panel)

| Tool (tooltip) | Behaviour | Undo |
|---|---|---|
| "Add custom mesh" | open the OBJ file dialog | (import spec) |
| "Resize mesh to fit middle zone" | target `T = (16,16,16)·norm`; if the import offset is non-zero: scale ← T, then *set origin and move* to `−(16,16,16) ⊙ (offset·norm)`; else: position ← 0, origin ← `(o − p_old) ⊙ (T ⊘ s_old)`, scale ← T | "Set mesh block size" |
| "Resize mesh to stay proportional" | `m = max(sx, sy, sz)`; origin ← `p + (o − p) ⊙ ((m,m,m) ⊘ s)`; scale ← (m,m,m) | "Set mesh proportional size" |
| "Apply left color to selected meshes material" | every material colour ← left colour | "Set mesh tint" |
| "Set material color of selected meshes" | colour dialog ("Mesh Color"/"Meshes Color"), result channels clamped to ≥ 0.25 | "Set color" |

These loop over selected meshes only and register their undo step even when nothing is selected (QUIRK).
The mesh texture commands of the Meshes popup (load/save/clear/remove alpha, copy/paste texture or colour) are
listed in 10.14.

### 6.4 Mesh quarter turns

Meshes rotate about **`(8, 0, −8)`** (block bottom centre) — not the block centre used by elements and
voxels (QUIRK: "Rotate model" on X or Z misaligns meshes relative to elements by up to 8 units). The engine
rotates the transform about that point by ±90° (X: +X axis, Y: +Y axis, Z: −Z axis, matching the element
directions in 4.9), then stores the resulting world Euler angles through the rotation setter (forced, not
about pivot, i.e. re-quantised to the 5° grid; 90° steps are exact) and rotates the origin about the same point.

### 6.5 Built-in primitive geometry: conventions

Primitive geometry is not stored in `.cubik` files: load, undo, redo and paste rebuild it from the type (6.2).
The meshes, and above all their UV layouts (which place the pixels of the paint texture), are therefore part of
the file contract: a painted primitive saved by the original must show the same picture in the port.

- Mesh-local frame = internal space; the unit box is x ∈ [0,1], y ∈ [0,1], z ∈ [−1,0] (MC space [0,1]³). The
  object transform scales it by the size and places it at the position (1.3).
- Every triangle is front-facing by the rule `(v1 − v0) × (v2 − v0)` points along the stored outward normal
  (holds for all primitive triangles). A conversion that negates z must reverse the winding.
- One triangle list per mesh (no submeshes). Vertex data: position, normal, one UV set; the torus also stores a
  tangent (w = −1). No vertex colours.
- Each primitive object renders its mesh with its own instance of the shared primitive material (vertex-colour,
  transparency-capable diffuse shading, white main colour, black emission) whose main texture is the paint
  texture; it casts no shadow. A non-convex mesh collider on the same mesh is its pick target.

| Type (code) | Label | Vertices (unique positions) | Triangles | Local bounds | Origin offset at creation |
|---|---|---|---|---|---|
| cube (1) | Cube | 24 (8) | 12 | unit box | 0 |
| sphere (2) | Sphere | 1734 (1538) | 3072 | sphere, centre (0.5, 0.5, −0.5), radius 0.498416 | (sx/2, sy/2, −sz/2) |
| cylinder (3) | Cylinder | 132 (66) | 128 | unit box | (sx/2, 0, −sz/2) |
| cone (4) | Cone | 194 (66) | 128 | unit box | (sx/2, 0, −sz/2) |
| pyramid (5) | Pyramid | 17 (6) | 8 | unit box | 0 |
| slope (6) | Slope | 18 (6) | 8 | unit box | 0 |
| corner-in (7) | Corner-in | 24 (7) | 10 | unit box | 0 |
| corner-out (8) | Corner-out | 16 (5) | 6 | unit box | 0 |
| torus (9) | Torus | 2145 (2048) | 4096 | unit box | (sx/2, sy/2, −sz/2) |
| sprite (10) | Sprite | 8 (4) | 4 | unit square at y = 0 (no thickness) | 0 (size y forced to 0.001) |

Type 0 is an imported mesh. The origin offset puts the pivot at the box centre (sphere, torus), at the base
centre (cylinder, cone) or at the position corner of 1.3 (all others); scaling keeps the pivot fixed (3.3).

### 6.6 Primitive face tables

Values are rounded to 4 decimals (a texel-exact parity test needs the exact reference values, which are kept as
private test data). Each face lists its corners as `position → uv` in mesh order; quads use the triangles
`(0,1,2), (1,0,3)` of their own corners, triangles `(0,1,2)`. Face names use internal directions (1.2: +Z = north).

**UV cells of the boxy primitives.** The cube divides the paint texture into a 3 × 3 grid of cells about 0.311
wide (columns ≈ u [0.0005, 0.3117], [0.3132, 0.6245], [0.6257, 0.9367]; rows ≈ v [0.0634, 0.3746],
[0.3754, 0.6870], [0.6883, 0.9996]): east = column 0 row 0, down = column 2 row 0, north = column 0 row 1,
west = column 1 row 1, south = column 2 row 1, up = column 0 row 2; column 1 rows 0 and 2 are unused (about 40 px
per cell on the 128-px texture). Corner-in, corner-out and slope reuse the same cells for the same faces.

**Cube** (all faces quads):

| Face | Corner 0 | Corner 1 | Corner 2 | Corner 3 |
|---|---|---|---|---|
| down (0,−1,0) | (1,0,0) → (0.9364, 0.0637) | (0,0,−1) → (0.6260, 0.3739) | (1,0,−1) → (0.9364, 0.3739) | (0,0,0) → (0.6260, 0.0637) |
| up (0,1,0) | (1,1,0) → (0.0005, 0.6883) | (0,1,−1) → (0.3117, 0.9996) | (0,1,0) → (0.3117, 0.6883) | (1,1,−1) → (0.0005, 0.9996) |
| north (0,0,1) | (1,0,0) → (0.0008, 0.3757) | (0,1,0) → (0.3117, 0.6870) | (0,0,0) → (0.3117, 0.3757) | (1,1,0) → (0.0008, 0.6870) |
| west (−1,0,0) | (0,0,0) → (0.3132, 0.3754) | (0,1,−1) → (0.6245, 0.6870) | (0,0,−1) → (0.6245, 0.3754) | (0,1,0) → (0.3132, 0.6870) |
| south (0,0,−1) | (0,0,−1) → (0.6257, 0.3758) | (1,1,−1) → (0.9367, 0.6870) | (1,0,−1) → (0.9367, 0.3758) | (0,1,−1) → (0.6257, 0.6870) |
| east (1,0,0) | (1,0,−1) → (0.0005, 0.0634) | (1,1,0) → (0.3114, 0.3746) | (1,0,0) → (0.3114, 0.0634) | (1,1,−1) → (0.0005, 0.3746) |

**Corner-in** (the cube without its corner vertex (1,1,−1): the top quad is gone and the opening is closed by two
sloped triangles that share the edge (0,1,0)–(1,0,−1)): down = the cube's down quad; north = the cube's north
quad; west = the cube's west quad; then the triangles

| Face | Corner 0 | Corner 1 | Corner 2 |
|---|---|---|---|
| sloped (0.707, 0.707, 0) | (1,0,−1) → (0.0005, 0.9996) | (0,1,−1) → (0.3117, 0.9996) | (0,1,0) → (0.3117, 0.6883) |
| sloped (0, 0.707, −0.707) | (0,1,0) → (0.3117, 0.6883) | (1,1,0) → (0.0005, 0.6883) | (1,0,−1) → (0.0005, 0.9996) |
| south (0,0,−1) | (1,0,−1) → (0.9367, 0.3758) | (0,0,−1) → (0.6257, 0.3758) | (0,1,−1) → (0.6257, 0.6870) |
| east (1,0,0) | (1,0,−1) → (0.0005, 0.0634) | (1,1,0) → (0.3114, 0.3746) | (1,0,0) → (0.3114, 0.0634) |

Mesh order: down, sloped (0.707, 0.707, 0), sloped (0, 0.707, −0.707), north, west, south, east.

**Corner-out** (square base on y = 0, apex (0,1,0) above a base corner): down = the cube's down quad, then

| Face | Corner 0 | Corner 1 | Corner 2 |
|---|---|---|---|
| sloped (0.707, 0.707, 0) | (0,1,0) → (0.3117, 0.6883) | (1,0,0) → (0.0005, 0.6883) | (1,0,−1) → (0.0005, 0.9996) |
| north (0,0,1) | (1,0,0) → (0.0008, 0.3757) | (0,1,0) → (0.3117, 0.6870) | (0,0,0) → (0.3117, 0.3757) |
| west (−1,0,0) | (0,0,−1) → (0.6245, 0.3754) | (0,0,0) → (0.3132, 0.3754) | (0,1,0) → (0.3132, 0.6870) |
| sloped (0, 0.707, −0.707) | (1,0,−1) → (0.9367, 0.3758) | (0,0,−1) → (0.6257, 0.3758) | (0,1,0) → (0.6257, 0.6870) |

**Slope** (full wall at x = 0, slope from the bottom edge at x = 1 to the top edge at x = 0): down = the cube's
down quad; west = the cube's west quad; then

| Face | Corners |
|---|---|
| sloped (0.707, 0.707, 0), quad | (1,0,0) → (0.0005, 0.6883); (0,1,−1) → (0.3117, 0.9996); (0,1,0) → (0.3117, 0.6883); (1,0,−1) → (0.0005, 0.9996) |
| north (0,0,1), triangle | (1,0,0) → (0.0008, 0.3757); (0,1,0) → (0.3117, 0.6870); (0,0,0) → (0.3117, 0.3757) |
| south (0,0,−1), triangle | (1,0,−1) → (0.9367, 0.3758); (0,0,−1) → (0.6257, 0.3758); (0,1,−1) → (0.6257, 0.6870) |

Mesh order: down, sloped, north, west, south.

**Pyramid** (apex A = (0.5, 1, −0.5), base centre C = (0.5, 0, −0.5)):

| Face | Corners | Triangles |
|---|---|---|
| side (0, 0.447, 0.894) | A → (0.4684, 0.9982); (0,0,0) → (0.6221, 0.6895); (1,0,0) → (0.3146, 0.6895) | (0,1,2) |
| side (−0.894, 0.447, 0) | A → (0.7817, 1.0); (0,0,−1) → (0.9367, 0.6887); (0,0,0) → (0.6267, 0.6887) | (0,1,2) |
| side (0, 0.447, −0.894) | A → (0.1563, 0.6852); (1,0,−1) → (0.3100, 0.3765); (0,0,−1) → (0.0025, 0.3765) | (0,1,2) |
| side (0.894, 0.447, 0) | A → (0.1551, 1.0); (1,0,0) → (0.3103, 0.6896); (1,0,−1) → (0.0, 0.6896) | (0,1,2) |
| base (0,−1,0) | (1,0,0) → (0.6284, 0.6853); (0,0,0) → (0.9356, 0.6853); C → (0.7820, 0.5308); (0,0,−1) → (0.9356, 0.3764); (1,0,−1) → (0.6284, 0.3764) | (0,1,2), (1,3,2), (3,4,2), (4,0,2) |

**Sprite** (two coincident unit squares on y = 0, x ∈ [0,1], z ∈ [−1,0]):

| Face | Corners |
|---|---|
| down (0,−1,0) | (0,0,−1) → (0.0003, 0.0003); (1,0,0) → (0.9995, 0.9998); (0,0,0) → (0.0003, 0.9998); (1,0,−1) → (0.9995, 0.0003) |
| up (0,1,0) | (0,0,−1) → (0.0003, 0.0003); (1,0,0) → (0.9995, 0.9998); (1,0,−1) → (0.9995, 0.0003); (0,0,0) → (0.0003, 0.9998) |

The 0.001 thickness exists only through the forced size.

**Cylinder** (axis along y, centre (0.5, y, −0.5), radius 0.5, height 1, 32 segments; θ = atan2(z + 0.5, x − 0.5);
ring angles θ_k = π + 2πk/32):
- vertices: 0 = bottom centre; 1–32 = bottom ring k = 0..31; 33–98 = side pairs, for i = 0..32 vertex 33 + 2i is
  the bottom ring point at θ_i and 34 + 2i the top point (i = 32 repeats θ_0); 99 = top centre; 100–131 = top
  ring.
- triangles: 32 bottom fan, then 64 side, then 32 top fan (the exact index lists are reference data).
- normals: caps (0,∓1,0), side (cos θ, 0, sin θ).
- UVs (fitted, residual ≈ 0.002 rad; UNVERIFIED to the last digit): bottom cap `(0.1871, 0.1874) +
  0.1862·(cos a, sin a)` with `a = 230.51° − θ`; top cap `(0.8122, 0.1885) + 0.1849·(cos a, sin a)` with
  `a = 230.51° + θ`; the centres take the centre value; side `u_i = frac(0.749 + 0.99683·i/32)` for both points
  of pair i, `v = 0.37653` (bottom) and `0.99795` (top).

**Cone** (base on y = 0, centre (0.5, 0, −0.5), radius 0.5, apex (0.5, 1, −0.5), 64 segments, θ_k = π + 2πk/64):
- vertices: 0 = base centre; 1–64 = base ring; ring points of the side at 65 + 2i (i = 0..64, i = 64 copies
  i = 0) and one apex copy per segment at 66 + 2i (i = 0..63).
- triangles: base fan (the last one closes on ring vertex 1), then one side triangle per segment made of a ring
  edge and its own apex copy.
- normals: base (0,−1,0); side ≈ (0.894·cos θ, 0.447, 0.894·sin θ).
- UVs: base cap centre (0.7816, 0.2180), radius 0.2174, `a = 109.80° − θ` (fitted, UNVERIFIED to the last digit);
  side ring points `v = 0.4385`, `u_k = frac(0.7651 + k/64)`; the apex copy of segment i has `v = 0.9991` and the
  u of ring point i + 1.

**Torus** (exact): centre (0.5, 0.5, −0.5); the tube section is an ellipse of radial half-width 0.15 and vertical
half-height 0.5 around the major radius 0.35. Grid i = 0..32 around the tube, j = 0..64 around the ring:
`φ_i = 2πi/32`, `θ_j = −π/2 − 2πj/64`, `R = 0.35 + 0.15·cos φ`; position
`(0.5 + R·cos θ, 0.5 + 0.5·sin φ, −0.5 + R·sin θ)` (max error 8e−7); `uv = (i/32, j/64)`. Two triangles per grid
cell (32 × 64 cells); the vertex order zig-zags, only the topology matters. Normals are smooth and close to
`normalize(0.5·cos φ·cos θ, 0.15·sin φ, 0.5·cos φ·sin θ)` (within 2.2°; exact construction UNVERIFIED).

**Sphere** (a quad sphere): six patches of 17 × 17 vertices (16 × 16 quads, two triangles each; patch edges are
duplicated), every vertex on the sphere of radius 0.498416 around (0.5, 0.5, −0.5), normals radial (within
0.38°). The lattice distribution (neither a plain nor an equal-angle cube projection) and the UV layout (one
skewed parallelogram per patch inside u ∈ [0.0004, 0.9369], v ∈ [0.0631, 0.9995]) are UNVERIFIED (6.8).

### 6.7 Preview player rig geometry

The display-settings preview (`app_behavior.md` §9: modes, anchors, presets and skin loading) shows a player
rig made of six parts, each with an outer-layer child.

- **Placement**: rig root at (8, 0, −8), rotated 180° about the axis (0, 1, 1)/√2, uniform scale 2. Rig-local
  +Z becomes world up (+Y), rig-local +X becomes world −X, rig-local +Y becomes world +Z, so the figure faces
  south (rig −Y). One rig unit = 2 world units = 2 skin pixels; the figure is 32 world units tall.
- **Part pivots** (rig-local): head (0, 0, 12), torso (0, 0, 6), left arm (−2, 0, 11), right arm (2, 0, 11),
  left leg (−1, 0, 6), right leg (1, 0, 6).
- **Faces** are named by the mesh-local normal: −Y = front, +Y = back, +Z = top, −Z = bottom, +X = the figure's
  right side, −X = its left side.
- **Skin rectangles** are in pixels of a 64 × 64 skin with the origin at the top-left, written
  `(x0, y0)–(x1, y1)` (the stored UVs are inset by about 0.05 px). Skin UVs are normalised, so any square skin
  works; the rectangles scale by size/64.

| Part | Mesh box (local) | Rig box | −X | +X | −Y | +Y | −Z | +Z |
|---|---|---|---|---|---|---|---|---|
| head | x[−2,2] y[−2,2] z[0,4] | z[12,16] | (16,8)–(24,16) | (0,8)–(8,16) | (8,8)–(16,16) | (24,8)–(32,16) | (16,0)–(24,8) | (8,0)–(16,8) |
| torso | x[−2,2] y[−1,1] z[0,6] | z[6,12] | (28,20)–(32,32) | (16,20)–(20,32) | (20,20)–(28,32) | (32,20)–(40,32) | (28,16)–(36,20) | (20,16)–(28,20) |
| left arm | x[−2,0] y[−1,1] z[−5,1] | x[−4,−2] z[6,12] | (40,52)–(44,64) | (32,52)–(36,64) | (36,52)–(40,64) | (44,52)–(48,64) | (40,48)–(44,52) | (36,48)–(40,52) |
| right arm | x[0,2] y[−1,1] z[−5,1] | x[2,4] z[6,12] | (48,20)–(52,32) | (40,20)–(44,32) | (44,20)–(48,32) | (52,20)–(56,32) | (48,16)–(52,20) | (44,16)–(48,20) |
| left leg | x[−1,1] y[−1,1] z[−6,0] | x[−2,0] z[0,6] | (24,52)–(28,64) | (16,52)–(20,64) | (20,52)–(24,64) | (28,52)–(32,64) | (24,48)–(28,52) | (20,48)–(24,52) |
| right leg | x[−1,1] y[−1,1] z[−6,0] | x[0,2] z[0,6] | (8,20)–(12,32) | (0,20)–(4,32) | (4,20)–(8,32) | (12,20)–(16,32) | (8,16)–(12,20) | (4,16)–(8,20) |

**Outer layers** (same face structure as their part; the layer's scale applies about the parent pivot and its
mesh is shifted by a small offset; "effective" = after the scale, before the pivot):

| Layer | Scale | Mesh offset | Effective box | −X | +X | −Y | +Y | −Z | +Z |
|---|---|---|---|---|---|---|---|---|---|
| head | (1.08, 1.08, 1.08) | z −0.139 | x,y ±2.16, z[−0.15, 4.17] | (48,8)–(56,16) | (32,8)–(40,16) | (40,8)–(48,16) | (56,8)–(64,16) | (48,0)–(56,8) | (40,0)–(48,8) |
| torso | (1.08, 1.08, 1.03) | z −0.0971 | x ±2.16, y ±1.08, z[−0.10, 6.08] | (28,36)–(32,48) | (16,36)–(20,48) | (20,36)–(28,48) | (32,36)–(40,48) | (28,32)–(36,36) | (20,32)–(28,36) |
| left arm | (1.1, 1.1, 1.03) | (+0.0909, 0, +0.0485) | x[−2.1, 0.1], y ±1.1, z[−5.10, 1.08] | (56,52)–(60,64) | (48,52)–(52,64) | (52,52)–(56,64) | (60,52)–(64,64) | (56,48)–(60,52) | (52,48)–(56,52) |
| right arm | (1.1, 1.1, 1.03) | (−0.0909, 0, +0.0485) | x[−0.1, 2.1], y ±1.1, z[−5.10, 1.08] | (48,36)–(52,48) | (40,36)–(44,48) | (44,36)–(48,48) | (52,36)–(56,48) | (48,32)–(52,36) | (44,32)–(48,36) |
| left leg | (1.1, 1.1, 1.03) | 0 | x,y ±1.1, z[−6.10, 0.08] | (8,52)–(12,64) | (0,52)–(4,64) | (4,52)–(8,64) | (12,52)–(16,64) | (8,48)–(12,52) | (4,48)–(8,52) |
| right leg | (1.1, 1.1, 1.03) | 0 | x,y ±1.1, z[−6.10, 0.08] | (8,36)–(12,48) | (0,36)–(4,48) | (4,36)–(8,48) | (12,36)–(16,48) | (8,32)–(12,36) | (4,32)–(8,36) |

The rectangles follow the public 64 × 64 player-skin layout. The skin is applied to every renderer of the active
preview scene that is not an editable object, each with its own material instance (`app_behavior.md` 9.7).

### 6.8 Art and data policy for built-in geometry

1. Ship **no** asset from the original install: no serialized engine data, meshes, textures, sprites, the
   bundled default skin or preset files. Extracted dumps (per-vertex and per-triangle data) stay private and
   serve only as test oracles; they are not committed to the public repository.
2. Primitives are generated in code from 6.5–6.6 (cube, corners, pyramid, slope, sprite from the face tables;
   cylinder, cone and torus from their parameters). Constants are taken from this specification, not from
   copied dumps.
3. Sphere: until the exact lattice and UV atlas are reproduced, the port uses an original cube sphere with
   16 × 16 quads per face and its own UV layout, and documents the paint-layout difference for painted spheres as
   a known compatibility gap; exact reproduction is a later parity task. Cylinder and cone caps use the fitted
   formulas of 6.6; their small residuals can move single texels at the cap rims (parity tests use a tolerance
   there).
4. Preview rig: the 64 × 64 skin regions are a public convention and are kept as a layout requirement; the
   boxes are re-authored generically (no reuse of the original model asset). The default skin is replaced by a
   generated neutral placeholder; user skins (square images) are supported as before.
5. The online skin lookup (legacy hosts) is not part of the default build; see `app_behavior.md` (D2, 9.7) for
   the opt-in replacement.
6. Display presets keep their functional values but are shipped as the port's own files.

---

## 7. Voxel world

### 7.1 Resolution and grid

| Quantity | Definition |
|---|---|
| resolution `R` | voxels per block edge; setup range 2..128, default 16 |
| grid size `N` | `3R` voxels per axis (the 48-unit editing volume) |
| cell size `c` | `16 / R` units |
| chunk edge | 16 voxels |
| chunk count per axis `n` | `ceil(N / 16)`; chunk-grid size `M = 16n ≥ N` |
| initialised | false until setup; most voxel commands require it ("Setup Voxels first", 12.7) |
| ambient occlusion | from `obj.ao` (2.2), applied when meshing |

Chunks are allocated lazily (including by *reads*), each holding 16³ cells (`index = x + 16y + 256z`), a
dirty flag and an "empty" flag (true at creation, cleared by any non-zero write, set again only by a full
clear).

### 7.2 Index and world coordinates; active box

- **Index → world corner** (minimum x, minimum y, *maximum* z of the cell, i.e. the element convention):
  `x = (i − R)·c`, `y = (j − R)·c`, `z = (R − k)·c`; cell `(i,j,k)` spans
  `[x, x+c] × [y, y+c] × [z−c, z]`.
- **World → index, rounded** (box corners, blueprint boxes, copy/paste):
  `i = round((x + 16)/c)`, `j = round((y + 16)/c)`, `k = round((16 − z)/c)`.
- **World → index, truncated** (ray hits, voxel tools): same expressions with `trunc`.
- **Active box** (half-open `[min, max)` per axis) = the model box converted with the rounded mapping
  (min/max taken per axis because Z flips). Default = the whole grid. Reads outside the active box return 0
  (empty); full and bounded writes outside it are ignored silently; the unchecked writer (used by
  up/downscale and some tools) ignores the box. The world corners used at setup are remembered and are what
  up/downscale restore (QUIRK: later model-box edits are not remembered there).
- When the model box changes, chunks along the first changed side are marked dirty (QUIRK: only one side is
  processed per change) and the world is flagged dirty.

### 7.3 Voxel value and colour packing

A voxel is a signed 32-bit `0xAARRGGBB`; **0 = empty**.

- Packing from 8-bit channels: 0 if alpha is 0, else `(a<<24)|(r<<16)|(g<<8)|b`.
- Packing from a float colour: channels `trunc(ch·255)`, then as above (alpha kept).
- **Tool colour packing** (all voxel tools, fills, presets): the colour is converted to 8 bits and then
  packed with alpha forced to 255 (opaque) unless its alpha is 0, which yields 0 = erase.
- Decoding for display: RGB from the value, alpha forced 255.
- Hex text form: "#RRGGBBAA" (uppercase). Hex parsing keeps only 0-9 A-F (uppercased), pads to 6 digits with
  zeros if shorter, uses digits 7-8 as alpha only when exactly 8 digits remain, else alpha 255; empty → fully
  transparent.

### 7.4 Palette

The world keeps an ordered list of distinct non-transparent values, starting `[0]`. Every write of a value
with non-zero alpha appends it if absent; values are never removed except by reset (back to `[0]`),
"Color all" (`[0, colour]`) or global colour replacement (entry renamed). Some palette imports add values in a
forced mode that skips both tests, so duplicates (and transparent values) can exist; lookups always use the first
occurrence. Used by palette conversion (14.3) and exporters (export specifications).

### 7.5 Write and read primitives

| Primitive | Box check | Palette update | Neighbour-chunk dirtying |
|---|---|---|---|
| full write | yes | yes | yes: a write at local 0 or 15 on an axis dirties the adjacent chunk on that side |
| bounded raw write | yes | non-zero only | no |
| unchecked raw write | no (grid only) | non-zero only | no |

All writes dirty their chunk and the world. Reads: "read" (0 outside the box), "read solid outside"
(returns 1 outside the box; used by some helpers). Port note: dirty all chunks within one voxel of any write
(face, edge and corner neighbours) — this yields the same final picture without the original's stale-seam
artefacts (raw writers and diagonal AO changes).

### 7.6 Chunk meshing

For every non-zero voxel of a chunk (local `(x, y, z)`, global `(i, j, k)`):
- emit a face when the neighbour in that direction is empty, where a neighbour outside the active box
  counts as empty: −Y uses `j−1`, +Y `j+1`, +Z (internal) uses `k−1`, −Z uses `k+1`, −X `i−1`, +X `i+1`;
- one quad (4 vertices, 2 triangles `(q, q+1, q+2)`, `(q, q+2, q+3)` with `q = 4·quadIndex`), no vertex
  sharing, no greedy merging; flat normal = face normal;
- vertex colour = decoded opaque colour × AO factor (all four channels multiplied).

Quad vertex positions in chunk-local units (`s = c`; local cell spans `x∈[x,x+1]s`, `y∈[y,y+1]s`,
`z∈[−(z+1), −z]s`) and the AO neighbours `A, B` (sides) and `D` (corner) of each vertex, given as global
index offsets:

| Face | v0 | v1 | v2 | v3 |
|---|---|---|---|---|
| −Y (plane y) | (x, y, −z−1) A=(0,−1,+1) B=(−1,−1,0) D=(−1,−1,+1) | (x+1, y, −z−1) A=(+1,−1,0) B=(0,−1,+1) D=(+1,−1,+1) | (x+1, y, −z) A=(0,−1,−1) B=(+1,−1,0) D=(+1,−1,−1) | (x, y, −z) A=(−1,−1,0) B=(0,−1,−1) D=(−1,−1,−1) |
| +Y (plane y+1) | (x, y+1, −z) A=(−1,+1,0) B=(0,+1,−1) D=(−1,+1,−1) | (x+1, y+1, −z) A=(0,+1,−1) B=(+1,+1,0) D=(+1,+1,−1) | (x+1, y+1, −z−1) A=(+1,+1,0) B=(0,+1,+1) D=(+1,+1,+1) | (x, y+1, −z−1) A=(0,+1,+1) B=(−1,+1,0) D=(−1,+1,+1) |
| +Z (plane −z) | (x+1, y, −z) A=(+1,0,−1) B=(0,−1,−1) D=(+1,−1,−1) | (x+1, y+1, −z) A=(0,+1,−1) B=(+1,0,−1) D=(+1,+1,−1) | (x, y+1, −z) A=(−1,0,−1) B=(0,+1,−1) D=(−1,+1,−1) | (x, y, −z) A=(0,−1,−1) B=(−1,0,−1) D=(−1,−1,−1) |
| −Z (plane −z−1) | (x, y, −z−1) A=(0,−1,+1) B=(−1,0,+1) D=(−1,−1,+1) | (x, y+1, −z−1) A=(−1,0,+1) B=(0,+1,+1) D=(−1,+1,+1) | (x+1, y+1, −z−1) A=(0,+1,+1) B=(+1,0,+1) D=(+1,+1,+1) | (x+1, y, −z−1) A=(+1,0,+1) B=(0,−1,+1) D=(+1,−1,+1) |
| −X (plane x) | (x, y, −z) A=(−1,−1,0) B=(−1,0,−1) D=(−1,−1,−1) | (x, y+1, −z) A=(−1,0,−1) B=(−1,+1,0) D=(−1,+1,−1) | (x, y+1, −z−1) A=(−1,+1,0) B=(−1,0,+1) D=(−1,+1,+1) | (x, y, −z−1) A=(−1,0,+1) B=(−1,−1,0) D=(−1,−1,+1) |
| +X (plane x+1) | (x+1, y, −z−1) A=(+1,0,+1) B=(+1,−1,0) D=(+1,−1,+1) | (x+1, y+1, −z−1) A=(+1,+1,0) B=(+1,0,+1) D=(+1,+1,+1) | (x+1, y+1, −z) A=(+1,0,−1) B=(+1,+1,0) D=(+1,+1,−1) | (x+1, y, −z) A=(+1,−1,0) B=(+1,0,−1) D=(+1,−1,−1) |

(Index offsets are `(Δi, Δj, Δk)`; remember `+Δk` is internal −Z.)

AO factor (occupancy `o(·) ∈ {0,1}` read with the box rule): AO off → 1.0; if `o(A)=1` and `o(B)=1` → 0.8;
else `1 − 0.1·(o(A) + o(B) + o(D))` (values 1.0, 0.9, 0.8).

Chunk objects are placed at internal `(16i'·c − 16, 16j'·c − 16, 16 − 16k'·c)` for chunk `(i', j', k')`.

**Scheduling.** At every fixed (physics) step, if auto-rebuild is enabled and the world is dirty, a single
rebuild job (never two at once) clears the world-dirty flag, walks chunks in `(i', j', k')` order and
rebuilds each dirty chunk, yielding to the frame loop after every 64 rebuilt chunks. Picking colliders are
refreshed with the mesh except during a stroke; at stroke end all chunk colliders are refreshed.
Long jobs (hollow/fill volume, conversions, voxelization) disable auto-rebuild while running.

Port notes: use 32-bit indices (a checkerboard chunk exceeds 65 535 vertices); mesh dirty chunks in parallel
but upload in a deterministic order; colliders can be replaced by a voxel DDA ray cast (the hit face/normal
semantics of 12.2 must be kept).

### 7.7 Setup and reset

**Setup dialog** ("Voxel Model Setup"; "Number of voxels per axis per zone:"; toggles "Voxel at center",
"Floor", "Full block" (default off) and an adapt-grid toggle (default on, label UNVERIFIED)). The
resolution field clamps to 2..128 on end-edit and on OK (unparsable → 0 → 2). When a world already exists,
all controls are disabled (settings locked) and OK does nothing but close. OK on an unlocked dialog:
1. undo "Setup voxels" (voxels);
2. create the grid at `R` with the AO flag; set the active box from the model box;
3. adapt grid: mouse snap step ← `16/R` (one voxel); display grid step ← `16/R'` where `R'` is `R` divided
   repeatedly by its smallest prime factor while `R' > 64`;
4. enable the voxel tools;
5. presets (white): "Full block" → fill `[R, 2R)³` and stop; else "Voxel at center" → one voxel at
   `(N/2, N/2, N/2)`; "Floor" → plate `y = R`, `x, z ∈ [R, 2R)` (center and floor may both apply).

**Reset** (Voxel menu "Reset", confirm "Confirm reset of Voxels" / "Are you sure you want to delete the
current voxel model?" / "Reset", "Cancel"; undo "Reset voxels"): drop all chunks and chunk objects, palette
`[0]`, uninitialised, settings back to defaults, tools disabled.

**Clear voxels** (Voxel menu): sets every cell to 0 in place, palette `[0]`; **no undo step and no
confirmation** (QUIRK).

### 7.8 Whole-world operations

| Command | Behaviour | Undo (category) |
|---|---|---|
| "Upscale x2" | refused when `R ≥ 128` ("Can't upscale above x128" / "Your model is already at the maximum allowed voxel size and can't be upscaled."); snapshot every cell readable through the box, reset, create at `2R`, write each old voxel into the 2×2×2 block at `(2i,2j,2k)` (unchecked), restore the setup-time box | "Upscale voxels x2" (voxels) |
| "Downscale /2" | refused when `R ≤ 2` ("Can't downscale below x2" / "Your model would be a simple cube after dowscale..."); sample old cell `(2i,2j,2k)` for `i,j,k < N div 2` (no averaging), reset, create at `R div 2`, write unchecked, restore box | "Downscale voxels x2" (voxels) |
| "Color all" | palette `[0, colour]`; every non-zero cell ← left colour (opaque) | "Reset voxels color" (voxels) |
| "Crop voxels outside model box" | confirm "Confirm clear Voxels outside" ("Are you sure you want to clear all voxels outside the model box?\n\nThis will take effect immediately." / "Clear", "Cancel"); every grid cell outside the active box ← 0 (chunk-level write: chunks are dirtied but the world-dirty flag is not raised, so the rebuild waits for the next voxel change — QUIRK; port: raise it) | "Clear voxels outside" (voxels) |
| "Fill volume" / "Hollow volume" | 12.5 | "Fill voxels volume" / "Hollow voxels" |
| "Flip on X/Y/Z axis" | 7.9, also flips the model box | "Flip voxels on X/Y/Z" (modelProperties+voxels) |
| "Rotate 90° around X/Y/Z axis" | 7.9, also rotates the model box | "Rotate voxels around X/Y/Z" (modelProperties+voxels) |

Upscale may reach `R = 254` (from 127), contradicting its message (QUIRK; the code behaviour is the
compatibility target). Voxels outside the active box are lost by up/downscale.

### 7.9 Voxel flips and rotations

Implemented on whole chunks (swap/permute chunk positions, then permute cell contents), which is
equivalent to the following global index maps with `M = 16n` (chunk-grid size, 7.1):

| Operation | Map `(i, j, k) →` |
|---|---|
| flip X | `(M−1−i, j, k)` |
| flip Y | `(i, M−1−j, k)` |
| flip Z | `(i, j, M−1−k)` |
| rotate X | `(i, k, M−1−j)` |
| rotate Y | `(M−1−k, j, i)` |
| rotate Z | `(j, M−1−i, k)` |

In internal world space these are the same rotations as the element quarter turns (rotate X:
`(x,y,z)→(x,−z,y)`, Y: `(z,y,−x)`, Z: `(y,−x,z)`) about the centre of the chunk grid. **QUIRK:** when
`3R` is not a multiple of 16 (e.g. R = 2, 4, 8, 24), `M > N`, the centre is not the block centre and voxels
shift (and may land outside the grid, becoming unreachable). The model box is mirrored about 8 (x, y) or
−8 (z), or rotated about (8, 8, −8), to stay in sync. All touched chunks are marked dirty.

"Rotate model +90° on X/Y/Z" (Edit menu, 10.11) rotates the voxel world the same way but does **not**
rotate the model box.

### 7.10 Persistence shape

Voxel data is stored as `vx.w = [ { r: R, c: [ { cx, cy, cz, v: [ {x, y, z, c} ] } ] } ]` (one world;
chunk indices; local cell coordinates; signed ARGB). Chunks whose empty flag is set are skipped; within a
chunk cells are listed x-outer, then y, then z, non-zero only. On load or undo restore: if the snapshot has
no world, an initialised world is reset; else the world is (re)created at the stored resolution when needed,
cleared, and every listed cell is written with the full writer (so the active box applies) and added to the
palette. Byte layout: file-format specification.

---

## 8. Selection model and interaction state

### 8.1 Selection state

| State | Meaning |
|---|---|
| selection list | ordered list of objects (elements, meshes, blueprint, model origin) |
| primary | gizmo target; first added object, or explicitly set; may be null |
| hovered | object under the cursor (8.4) |
| active axis | axis flags currently grabbed/hovered (none, x 1, y 2, z 4, xy 3, xz 5, yz 6, xyz 7) |
| default axis | per mode: the view's planar axes (move/scale/clone/selection) or its direction axis (rotate) |
| busy | a drag is in progress |
| grabbed | a gizmo handle was pressed this click (suppresses the click-select once) |
| clone guard | prevents re-entering clone mode until the mouse is released |

Operations (all raise **SelectionChanged**):

| Operation | Behaviour |
|---|---|
| select exclusive(obj) | if *grabbed* is set: clear it and do nothing; else clear the selection, then add `obj` (if non-null) and make it primary |
| add(obj) | append if absent; becomes primary if there is none |
| toggle(obj) | remove if present, else add |
| remove(obj) | remove; if it was primary, the first remaining object (or none) becomes primary |
| clear | deselect all, primary none |
| select all | all elements, then all meshes; primary = first if none |
| set selection(list) | replace the list (used to restore originals after menu Clone); primary = first (without primary events) |
| select by name(pattern) | prompt "Select by name (wildcards supported)" (default `*`); the input `p` (untrimmed) becomes the glob `*p*`, matched **case-insensitively against the whole label** (anchored at both ends); `*` = any run of characters, `?` = exactly one, every other character literal. Element labels first (all elements, hidden included), then the meshes found under the mesh container (inactive objects are skipped, so hidden meshes do not match unless the preference of 10.6 keeps hidden objects active, QUIRK); the matches replace the selection list. Undo "Select " + trimmed input (objects). QUIRK (also for select all): the list is emptied without un-highlighting the previous objects, and an existing primary is kept even if it no longer matches |
| select inverse | toggle every element, then every mesh |
| box apply | 8.6 |

Events: SelectionChanged; PrimarySelected (new non-null primary — also switches paint, colour and voxel
tools off); PrimaryDeselected; HoverEnter/HoverExit; PositionChanged / RotationChanged / SizeChanged (raised by
selected objects); GeometryChanged (element UV refresh, and with no object at the end of every paint stroke).
Highlights: hover colour `objectHover`, selection colour `objectSelect` (theme).

Selecting the blueprint or model origin is possible (menu "Set Model Blueprint" Ctrl+B / "Set Model Origin"
Ctrl+K, clicks, or tools); box selection only considers elements and meshes.

### 8.2 Input bindings (defaults)

| Action | Keys |
|---|---|
| ModeRotate (hold) | 1, Keypad 1 |
| ModeScale (hold) | 2, Keypad 2 (also: right mouse button held over a view) |
| ModeClone (hold) | 3, Keypad 3, Left Alt, Right Alt |
| PanView | middle mouse, Space |
| ModifySelection | Left/Right Ctrl, Left/Right Cmd |
| ModifySelectionMultiple | Left/Right Shift |
| Focus | F |
| MoveLeft/Right/Up/Down | arrow keys |

Any of an action's keys triggers it. User overrides (preferences) are comma-separated key names.

### 8.3 Per-frame state machine

Runs every frame while the application has focus, except while PanView is held or the display-settings
preview owns the view.

1. **Gizmo press/hover** (pointer inside a view zone only): ray of length 100 from the view camera against
   the gizmo layers visible in that view. If a handle is held and the left button is up → release it. If the
   ray hits: with no held handle and left button pressed this frame → press the handle under the ray (if any;
   set *grabbed*); otherwise, if no mouse button/PanView is held → if the hit collider carries axis flags,
   *active axis* ← those flags.
2. **Mode** (unless a tool has put the machine in custom mode): start from *move*. With a primary:
   - ModeRotate held and no text field focused → *rotate*: show the rotate gizmo (if canRotate), hide the
     others, default axis ← view direction axis;
   - else ModeScale held, or (right button held, pointer in a view, no text field focused) → *scale*: show
     the scale gizmo (if canScale), default axis ← planar axes;
   - else ModeClone held and clone guard clear → *clone*, default ← planar;
   - else *move*: show the move gizmo (if canMove), default ← planar.
   If none of rotate/scale/clone was chosen: ModifySelection held → *selection*; else ModifySelectionMultiple
   held → *selectionMultiple* (rotate/scale gizmos hidden, default ← planar).
3. **Dispatch**: rotate → rotate update (9.3), end-drag; scale → scale update (9.3), end-drag; clone → hide
   the select box, clone start (10.2); move → move update (9.3), click-select (8.5), end-drag; custom → tool
   callbacks (8.7); selection modes → selection input (8.6), end-drag.
4. While busy: hide the select box and clear *grabbed*.
5. When a rotation drag has ended and restricted mode is on: bake the primary (3.5).

**End-drag**: on left or right button release with a primary: drag cleanup (1.6 rounding), refresh the view
zone, clear clone guard and busy.

At every fixed step: hover pick (8.4); apply a pending box selection (8.6); arrow-repeat counter
(increments while an arrow is held, resets otherwise).

### 8.4 Hover picking

When the pointer is in a view and no mouse button/PanView is held: ray of length 1000 from the view camera
through the cursor against the default (model) layer, cached while the cursor does not move. Hidden objects
are inactive and cannot be hit.

- Hit: Focus pressed → move the view focus point to the hit and aim the 3D camera at it. If a voxel tool is
  active (or custom mode over a voxel chunk): show the voxel pointer (12.2). Otherwise (not custom mode):
  hovered ← the hit object (or none); show the 3D pointer marker at the hit point facing the hit normal;
  **if hovered = primary → active axis ← default axis**.
- Whenever hovered ≠ primary → active axis ← none.
- No hit: hovered ← none, active axis ← none, pointers hidden.

Consequently an object is dragged either by a gizmo axis (1. above) or by pressing on the primary itself,
which uses the view's planar axes (orthographic views) — in the 3D view the planar axes are none, so only
gizmo axes drag there.

### 8.5 Click selection (move mode)

In a view: left press arms a click. On left release with the click armed, not busy and neither modifier held:
if the select box is visible and not *grabbed* → hide the box; else select exclusive(hovered) — a click on
empty space clears the selection.

### 8.6 Selection modes and box selection

In a view, on left press: arm the click; if ModifySelection or ModifySelectionMultiple is held: toggle the
hovered object and start the select box at the drag-plane point (9.2), both corners there. Left release
(armed, not busy, no modifier) → as 8.5. While ModifySelectionMultiple and the left button are held and the
view camera is orthographic: move the box's second corner to the drag-plane point; if the box is not empty
(any extent > 0.1) show it and extend it through the view depth to the full editing volume along the
dominant camera-forward axis (|component| ≥ 0.1, checked x, then y, then z).

Any non-empty box change schedules a **box apply** at the next fixed step: for every element and mesh
(hidden and locked included), origin inside the box → add (if not selected); else, if selected → remove.
Primary ← first selected or none. (The box is a rubber band only; its handles are never shown.)

### 8.7 Custom mode (tools)

Paint, colour-picker and voxel tools switch the machine to custom mode, clear the selection and register a
left and a right stroke callback. In custom mode, inside a view: a left (or right) press arms the
corresponding button; every frame while armed and the hover ray hits something, the callback is called with
`(down = true, hit)`. On release the callback is called once with `(false, lastHit)`. Choosing "none" in the
tool group returns to move mode.

### 8.8 Edit modes

Five mutually exclusive edit-mode toggles (element, voxel, mesh, paint, preview) select which panels are
shown; with none on, a custom panel set from the user's "open panel" preferences is used. Mode → panels:
element = element create, model side, element tools, elements list, elements material; voxel = model side,
voxel tools, paint tools, colour tools, palette, gizmos tools; mesh = mesh create, model side, mesh tools,
meshes list, paint tools, colour tools; paint = model side, paint tools, colour tools, palette;
preview = model side + display settings. A new model selects the custom set. Edit modes do not change data
behaviour. The element, voxel, paint and colour-picker tool toggles share one exclusive toggle group; switching
rules and auto-off rules are in 8.10.

### 8.9 List panels

Element list rows (row height 26): label, id, icon (hidden → lightbulb_off; else locked → lock; else type
icon tinted with the element RGB), pressed background when selected, label in the error colour for far
elements of block/item models (QUIRK: the colour is not reset for recycled rows in that case).
Row click: Shift (ModifySelectionMultiple) with a non-empty selection → add every element whose id lies in
`[min, max]` of the selection's ids and the row's id (QUIRK: throws if the selection contains a mesh);
Ctrl (ModifySelection) → toggle; else select exclusive. A double click (≤ 0.25 s) has no effect. The options
button/right click opens the element popup when the row is selected. Mesh rows behave the same (range by
mesh id; row height 28; type icons per primitive). Texture rows: 5.13.

### 8.10 Tool toggle groups

**Membership.** One exclusive toggle group (switching off allowed) holds 22 toggles: the three element tools
(settings, faces, UV), the nine voxel tools, the nine paint tools and the colour picker. The two gizmo toggles
(model box, handles) are independent. Mesh tools and the element tools' six one-shot buttons are plain buttons.
The edit-mode toggles form a separate group.

**Switching.** Turning a toggle on turns the group's previously active toggle off first; its class receives
"off" before the new class receives "on". Clicking the active toggle turns it off (that class becomes "none").
Each class calls its handler exactly once per switch with its resulting tool, so a switch A → B within a class
runs B's handler once, and across classes runs A's none branch, then B's handler. When several toggles of one
class read as on during a transition, the class takes the first in its priority order (12.1, 13.1; element
tools: settings, faces, UV; colour: picker).

| Class | Switching a tool on | Switching the class to none |
|---|---|---|
| element tools | open Element Settings on its tab (settings → General, faces → Faces, UV → Textures UV, which also refreshes the UV view), hide Display Settings; select mode and selection are unchanged (element commands check for a primary element themselves) | hide Element Settings (tab kept) |
| voxel tools | store the tool, enter custom mode (8.7: the selection is cleared and the primary removed), bind the left (add, left colour) and right (remove/subtract, right colour) stroke handlers; show the size field of Sphere or Cube only while that tool is on | back to move mode (the selection is not restored), reset the voxel stroke state, hide the size fields |
| paint tools | as voxel tools with the paint handlers; size fields: Line → line size, round brush → brush size, square brush → square size, sprayer → sprayer size, noise → noise amount | move mode, reset the paint stroke state, hide the size fields |
| colour picker | custom mode, selection cleared; left click samples into the left colour, right click into the right colour | move mode only |

Stroke handlers are skipped while a gizmo handle is dragged; Left Alt turns any stroke into a colour sample for
the pressed button (13.4). "Switch left and right colors" is a plain button and changes no mode.

**Auto-off rules.**
1. Group exclusivity (above): an element tool switched off by a voxel, paint or picker tool closes Element
   Settings. An Element Settings panel opened by a menu command (no element toggle on) stays open.
2. Hiding a tool panel clears its own class (element, voxel, paint, colour tools); the gizmo and mesh panels clear
   nothing.
3. An edit-mode change hides the helper panels, the side panels and all ten tool and list panels (so rule 2
   clears every tool), then shows the new mode's panels (8.8).
4. A new non-null primary selection clears the voxel, paint and colour classes; element tools and gizmo toggles
   are unaffected; clearing the selection clears nothing. Consequence: the voxel Wrap tool switches itself off
   when it selects the blueprint (12.3).
5. Voxel reset (also reached through model reset and "Reset voxels") clears the voxel toggles and makes them
   non-interactable; setup or a resolution change re-enables them. All nine voxel toggles start
   non-interactable.
6. The paint class is cleared by the application reset. No dialog clears tool toggles directly.
7. QUIRK: the "Display Settings" menu command hides Element Settings without clearing an active element toggle;
   the next click on that toggle only switches it off.

**Gizmo toggles.** "Bounding box" and "Handles" show or hide the model box outline and its handles (2.4); no mode,
selection or panel effect, no auto-off. At start both toggles are off while the box outline defaults to shown and
nothing syncs them (UNVERIFIED whether the outline is visible with its toggle off).

**Size fields** (voxel sphere and cube diameters; paint line, brush, square and sprayer sizes; noise amount) are
decimal-number input fields whose value is read as an **integer** each time it is used: "try parse integer,
0 on failure", so empty text, "7.5", "7." and "7,5" all read 0; surrounding spaces and a leading sign are
accepted, "-7" reads −7. Typing is limited to 2 characters (3 for the noise amount); there is no clamp. The mouse
wheel adds or subtracts 1.0 to the float value of the text and writes it back as a float (so "7.5" becomes "8.5",
which then reads 0); whether wheel-written text can exceed the character limit is UNVERIFIED. Defaults and uses:

| Field | Default | Use |
|---|---|---|
| voxel sphere diameter | 7 | radius = value / 2 |
| voxel cube diameter | 7 | radius = value / 2 |
| paint line size | 1 | line thickness = value |
| paint round brush, square brush, sprayer diameters | 7 | radius = value / 2 |
| paint noise amount | 32 | tolerance and jitter = value (tooltip says 0–255, nothing clamps) |

Voxel edge cases (12.3): sphere diameter 0 or 1 → one voxel; cube diameter 0 → nothing, 1 → one voxel; negative
values → nothing. The sphere stamp tests about `(2r + 1)³` cells per frame, about one million at the two-digit
maximum 99.

### 8.11 Tool cursor

A 16 × 16 symbol drawn on the topmost UI layer (above panels, menus and dialogs; only tooltips are above it),
always following the pointer; it is never deactivated, "hidden" means fully transparent.

- **Position** (every rendered frame): its centre = pointer + (16, −16) × canvas scale factor, in screen pixels
  (below right of the hotspot, never covering it). The canvas scale factor is 1 in the scene and set at start-up
  by the display code (range and conditions UNVERIFIED).
- **Sprite and tint** (every fixed step), by the select mode (8.3):

| Mode | Sprite | Tint |
|---|---|---|
| custom (any colour-picker, paint or voxel tool) | ring | the custom colour = the left colour (written whenever either colour changes; transparent until the first change, UNVERIFIED whether that happens at start-up) |
| move | move arrows | by active axis |
| rotate | ring (same picture as custom) | by active axis |
| scale (also while the right button is held in a view) | scale corners | by active axis |
| clone, with a primary | plus | white |
| selection, selectionMultiple | square | white (also with nothing selected) |

  By active axis (8.1): x → (1, 0.3, 0.3), y → (0.05, 0.5, 1), z → (0.5, 0.9, 0.1), a plane axis or xyz → white,
  none → fully transparent. With no primary the axis counts as none. The ring and arrow pictures are mostly black
  with white parts, so only the white pixels take the tint.
- **When it shows** (move, rotate, scale): only when a drag would act. Over the primary itself in the top, side
  or front view: move and scale show white (planar drag), rotate shows the view's direction colour (top Y blue,
  side X red, front Z green). Over an arrow handle: that axis colour; over a plane handle: white. In the 3D view
  over the primary, over other objects or empty space: hidden.
- **Freezing**: the active axis is only updated while the pointer is in a view zone and no mouse button or
  PanView is held, so the tint keeps its last value outside the views, over dialogs and during drags. The mode
  (and so the sprite) is still recomputed every frame, except while PanView is held or the display preview owns
  the views; mode keys work over the UI too, the right-button scale switch only inside a view. A focused text
  field blocks the rotate key and the right-button scale. While the application has no focus nothing updates.
- Port note: the 3D hover markers and the voxel pointer (12.2) are separate scene objects, not this symbol.

---

## 9. Gizmos, drags, snapping and nudges

### 9.1 Gizmo set and placement

| Gizmo | Placement | Size factor |
|---|---|---|
| move (arrows + plane squares) | at the primary's origin; one variant per view, each drawn only in its view (front: x, y, xy; top: x, z, xz; side: y, z, yz — plus the arrow along the view direction, drawn end-on and not pickable; 3D: x, y, z, xy, xz, yz) | 0.6 × the factor of that variant's own view camera |
| rotate (three rings + axis triad) | at the primary's origin; rings follow the object's yaw/pitch; visible in every view | 0.7 × the factor of the view the pointer is in (shared by all views) |
| scale (six face handles) | root = primary transform position + R·(pivotOffset ⊙ size), rotation = object rotation, root scale = size ⊙ gizmoSizeFactor; handle scale compensates the root scale; the handles along a view's direction are hidden in that view | 0.3 × the factor of the view the pointer is in |

View factor: orthographic views `clamp(orthoSize/4, 0.2, 4)` (3 for the default orthographic size 12);
perspective `clamp(distance(camera, primary origin)/8, 0.5, 4)` (1 with no primary). With no primary all
gizmos and the pointer are hidden. Geometry, hit areas, colours and visibility details: 9.7.

Per-view axes: top view = planar xz, direction y; side view = planar yz, direction x; front view = planar xy,
direction z; 3D view = none. Gizmo colours: X red, Y blue, Z green (9.7); the cursor symbol uses its own tint
constants (8.11).

### 9.2 Drag plane

All mouse drags read the intersection of the cursor ray (length 200, view camera latched while busy) with a
large camera-facing plane through the primary's **transform position** (or the pressed handle when nothing
is selected). The hit is computed once per frame and shared. Drags are component-wise in world axes:
`Δ = hit − startHit`, masked by the axis flags (`x→(1,0,0)`, `y→(0,1,0)`, `z→(0,0,1)`, combinations).

### 9.3 Mouse drags

All drags are refused for locked or hidden objects or when the matching permission is off. The undo step is
pushed on the first drag frame: "Drag selection", "Rotate selection", "Scale selection" (objects).

- **Move**: start = origin. Target origin `= start + Δ ⊙ mask`. If grid snap is on, **every** component of
  the target is snapped: `round(t/stepMouse)·stepMouse` (absolute snap; unmasked axes snap too — QUIRK).
  Apply *set origin and move* with propagation (other selected objects move by the same delta). Moving
  requires the pointer to be inside a view until the drag has started.
- **Scale**: start = size, `Δ = (hit − startHit)·dragSensitivity` with Δz negated; target size
  `= start + Δ ⊙ mask`; if grid snap and gridSnapOnScale: each component snapped to stepMouse; apply the size
  setter (pivot-relative, propagating, clamped at 0.001). Both left and right buttons drag in scale mode.
- **Rotate**: start = the object's current world Euler angles (engine decomposition) and the start sum
  `mouseX + mouseY` (pixels). `d = (mouseX + mouseY) − startSum`; target `= start + mask ⊙ (d, −d, −d)`
  (1 pixel = 1°; X positive, Y and Z negated); apply the rotation setter (propagating, about pivot), which
  quantises to 5° or 22.5° and applies the restricted-mode rejection. Gizmos refresh every frame.

Snap step defaults: mouse 0.5 (grid dialog "Mouse snap:" N → step `16/N`, default N = 32), keyboard 0.125,
UV 0.5 (UV view, 10.16), display grid 1. Grid snap and grid visibility default on (not persisted).

### 9.4 Keyboard nudges

Arrow gate: only with the pointer inside a view and no text field focused. A press edge fires; while any
arrow stays held the fixed-step counter grows, and once it exceeds 15 the gate fires every frame
(≈ 0.32 s delay at the default 50 Hz fixed step; UNVERIFIED project setting).

Plane vector `v` from the arrows (`h` = +1 Right / −1 Left, Right wins; `w` = +1 Up / −1 Down, Up wins):
front (xy) `(h, w, 0)`; side (yz) `(0, w, −h)`; top (xz) `(h, 0, w)`; 3D `(0,0,0)`.

| Mode | Effect per gate fire |
|---|---|
| move | `setOriginMove(origin + v·stepKeyboard)` with propagation |
| scale | size setter with `size + (vx, vy, −vz)·stepKeyboard` (propagating) |
| rotate | axis: X if (unrotated and view direction = x) or rotated on X; else Y if (unrotated and direction = y) or rotated on Y; else Z if (unrotated and direction = z) or rotated on Z. Step `±22.5` with sign `sgn(vy+vz)` (X), `sgn(vx−vz)` (Y), `sgn(−vx+vy)` (Z), where `sgn(0) = +1`. The new rotation is that single component (the other two set to 0), applied with propagation about the pivot |

Keyboard nudges ignore the locked flag (QUIRK) and push **no** undo step (QUIRK). In the 3D view an already
rotated object rotates by +22.5° on any arrow (QUIRK of `sgn(0)`).

The rotate request goes through the rotation setter (3.4, not forced). In restricted mode (elements and the
blueprint with `g.rer` on) the step is exactly 22.5° and the single-axis rejection applies; keyboard rotations are
never baked (only drags are, 8.3). On the 5° grid (every mesh; elements and the blueprint while `g.rer` is off)
the request is re-quantised half to even: from a multiple of 10 the step becomes ±20, from an odd multiple of 5
it becomes ±25, so after the first press the angle stays on multiples of 10 and never shows 22.5. Because the
request has a single non-zero component, a multi-axis rotation (possible on the 5° grid, e.g. after two quarter
turns) collapses to the nudged axis.

### 9.5 Rotation constraints summary

- Unrestricted: any axes, 5° grid. This always holds for meshes (primitives, sprites and imported meshes are
  not restrictable, 3.1), whatever `g.rer` says.
- Restricted (`g.rer`, default on; elements and the blueprint only): 22.5° grid; while an object is rotated on
  one axis, rotating another axis is rejected; after each rotation drag the primary (and selection) is baked
  (3.5) so the result is a single axis within ±45° plus quarter-turn face permutations. Only elements bake: a
  blueprint (or mesh) primary is left as it is. File loads, undo, redo, paste and quarter turns pass `force` and
  are never rejected.
- Turning the restriction on in Model Properties: if any element has an invalid Minecraft rotation, ask
  "Recalculate elements rotation" — "Your model contains N element(s) with un-restricted rotation
  angles.\n\nDo you want to force restricted rotation?" ("Force", "Cancel"); Force (or no such element)
  sets the flag and bakes every element (undo "Restrict elements rotation", modelProperties+elements).
  QUIRK: confirming the dialog while the flag is already on clears it (decompiled logic; UNVERIFIED).

### 9.6 Model box handles

A handle is pressed through 8.3 step 1 (sets *grabbed*). While held: first frame stores the handle position
and the drag-plane hit; target `= start + Δ ⊙ handleAxis`; if grid snap is on every component is snapped to
stepMouse; the handle moves and the box is committed (2.4). Releasing the button outside every view does not
release the handle until the cursor returns (UNVERIFIED).

### 9.7 Gizmo geometry, hit areas, colours and per-view visibility

**Visibility per view.** Each view draws and hit-tests the same set of gizmo parts (the original assigns parts to
render layers; each view camera's culling mask equals its gizmo pick mask):

| Gizmo part | Top | Side | Front | 3D |
|---|---|---|---|---|
| move gizmo variant of that view | own | own | own | own |
| rotate rings and axis triad | yes | yes | yes | yes |
| scale arrows and model-box/select-box handles on ±X | yes | — | yes | yes |
| scale arrows and handles on ±Y | — | yes | yes | yes |
| scale arrows and handles on ±Z | yes | yes | — | yes |
| north pointer (markers below) | yes | yes | — | yes |

The blueprint and the model origin are on the model layer, so the hover ray (8.4) can pick them in every view.
Objects kept active while hidden by the preference of 10.6 are moved to a 3D-only layer: drawn only in the 3D view
and not hit by the hover ray.

**Shapes** (gizmo-local units, before the size factor; directions in internal space):
- **Move arrow**: a thin shaft from the origin to 1.0 (triangular section, circumradius 0.05) ending in a
  square-based head (base half-width 0.15 at 1.0, tip at 1.3), base closed. X points +X, Y +Y, Z −Z (MC +Z).
- **Plane square**: 0.5 × 0.5 in the quadrant of its two axes, starting at the origin (inset 0.01): xz in
  (+X, −Z), yz in (+Y, −Z), xy in (+X, +Y).
- **Rotation ring**: 64 segments with a V-shaped section: inner edges at radius 1.8 and ±0.25 along the ring
  axis, outer ridge at radius 2.0 in the ring plane; the inner side is open. Ring axes: X ring +X, Y ring −Y,
  Z ring +Z.
- **Axis triad** (with the rings): three thin spikes of length 1 along +X, +Y, −Z.
- **Scale arrow / box handle**: an open square pyramid with its apex on the anchor and a 0.5 × 0.5 mouth 0.5
  out along its direction.
- **Markers**: centre marker = three perpendicular 1 × 1 squares through the point; model origin = octahedron of
  radius 0.4; north pointer = a flat scale-arrow pyramid (scale (2, 1, 2), length 1) at (8, 0, 2), just outside
  the north side of the central block, apex pointing north and opening toward the block; voxel pointer box = the
  unit voxel cell inset by 0.01 (no collider).

**Scale-handle positions** (scale root frame, before the object scale; pivot on a face centre of the unit box
x ∈ [0,1], y ∈ [0,1], z ∈ [−1,0], pyramid pointing outward):

| Handle | Pivot | Direction | Pick sphere centre |
|---|---|---|---|
| −X | (0, 0.5, −0.5) | −X | (−0.33, 0.5, −0.5) |
| +X | (1, 0.5, −0.5) | +X | (1.33, 0.5, −0.5) |
| −Y | (0.5, 0, −0.5) | −Y | (0.5, −0.33, −0.5) |
| +Y | (0.5, 1, −0.5) | +Y | (0.5, 1.33, −0.5) |
| +Z (north face) | (0.5, 0.5, 0) | +Z | (0.5, 0.5, 0.33) |
| −Z (south face) | (0.5, 0.5, −1) | −Z | (0.5, 0.5, −1.33) |

The Y and Z scale arrows are modelled along the other axis and turned −90° about X; the scale root's Y/Z swap of
the compensation (9.1) is therefore intentional. The six model-box handles sit on the face centres of the model
box (defaults: −X (−16, 8, −8), +X (32, 8, −8), −Y (8, −16, −8), +Y (8, 32, −8), −Z (8, 8, −32), +Z (8, 8, 16)),
pointing outward, at the fixed handle scale 1.5 (not view dependent); the select-box handles use the same shapes
on the select box (never shown, 8.6).

**Hit areas** (no collider is a trigger; ray lengths: gizmo ray 100, hover ray 1000, drag plane 200):

| Part | Collider | Axis flags |
|---|---|---|
| move arrow (not end-on) | sphere r 0.3 centred 1.0 along the arrow | x 1, y 2, z 4 |
| move plane square | box 0.5 × 0.5, zero thickness, centred on the square | xy 3, xz 5, yz 6 |
| rotation ring | the ring mesh (non-convex) | 1 / 2 / 4 |
| scale arrow | sphere r 0.33 centred 0.33 along the arrow | 1 / 2 / 4 |
| model-box / select-box handle | sphere r 0.5 centred 0.33 along the arrow (world radius 0.75 at scale 1.5) | 1 / 2 / 4, pressed as a handle (9.6) |
| blueprint | its box mesh (model layer) | — |
| model origin | sphere r 0.4 | — |

Only the box handles start a handle press; arrows, planes, rings and scale arrows set the active axis (8.3) that
the mode drags use. Whether the zero-thickness plane boxes are hit reliably is engine dependent (UNVERIFIED).

**Colours.** Arrows, planes, rings, triads and scale arrows take their colour from a small colour-key texture: X
red #FF4242, Y blue #0072FF, Z green #82F215, all opaque in the sampled areas of arrows, scale arrows and
triads. Plane squares and rings sample partly transparent areas (about 60 % of a plane square and 86 % of a ring
is fully transparent), so planes show as partial two-colour squares and rings as thin bands (visual result
UNVERIFIED). Model-box handles: orange #FFC800 at 25 % alpha; select-box handles: red at 25 %; centre markers
and model origin: white at 25 %; north and 3D pointers: opaque white; voxel pointer: #E11A1A at 50 %. No gizmo
colour or material changes on hover or drag.

**Draw order.** The move, rotate and scale gizmos, the triads, the select-box handles and the centre markers are
drawn in an overlay group after all other geometry; the model-box handles, blueprint, pointers, voxel pointer,
model origin, grids and backdrops are drawn in the transparent group before it. Whether the overlay group ignores
depth (always on top) is UNVERIFIED; its order suggests so. No gizmo casts or receives shadows.

---

## 10. Element and object operations

Guards: commands marked **E** require the primary to be an element or the selection to contain elements
("No element selected" / "You must select at least one element to proceed." or, for a mesh-only selection,
"This feature doesn't work on meshes.\n\nSelect at least one element to proceed."); element-tool buttons use
a stricter guard (primary must be an element: "No element selected" / "You must select at least one element
to proceed..."). **O** = any element or mesh selected ("No object selected" / "You must select at least one
element, mesh or primitive to proceed."). The undo category of O-commands follows the selection: elements,
meshes, or objects for a mix.

### 10.1 Create

| Command | Behaviour | Undo (elements) |
|---|---|---|
| "Add Cube", "Add Plane on X/Y/Z axis" | new element of that type at the blueprint's local position and scale; its transform orientation is copied from the blueprint **without** updating its stored rotation (QUIRK, normally 0); random colour (each RGB channel uniform in [0.6, 1], alpha 1); faces per type (4.2); selected exclusively; appended to the list; then UV snap ← true (the default mapping is applied when the element starts, 4.7) | "Create Cube element", "Create PlaneX element", "Create PlaneY element", "Create PlaneZ element" |

Creation details: id ← ++counter, label per type (4.2), origin = position, faces default (4.4), rotation 0.

### 10.2 Duplicate / clone

- **Element popup "Clone"** (E, undo "Clone element"): for each selected element create a copy at the same
  place (same size, colour) and copy every property (permissions, type, colour, label — clones keep the
  source label —, uvSnap, mirror, rescale, shade, faces, position, size, rotation, origin); afterwards the
  **originals** are re-selected (QUIRK).
- **Object menu "Clone"** (O, undo "Clone selection" with the selection's category): same as clone mode
  below without a drag.
- **Clone mode** (hold ModeClone, 8.3): on left drag with an active axis: undo "Clone selection" (category
  *elements* only — mesh clones are not undoable, QUIRK); snapshot the selected elements and meshes, rebuild
  them as new objects at the same place, select the copies (and only them), set the clone guard, switch to
  move: the copies are dragged.

### 10.3 Delete

"Delete" (O; Ctrl+Del; undo "Delete", objects): remove every selected object (elements and meshes; the
blueprint and model origin are not deletable and are skipped). The Del key opens a confirmation first:
title "Confirm deletion of N object" + ("s" if N > 1); text "Are you sure you want to delete N objects?" or
(N = 1) "Are you sure you want to delete \"LABEL\"?" (silently aborted if that single object is not
deletable), followed by "\n\nClick \"Delete\" to confirm."; buttons "Delete", "Cancel".

### 10.4 Rename

- Element popup "Rename..." (E; F2): dialog "Rename selected element" prefilled with the primary label;
  OK pushes "Rename " + old label (elements) and renames every selected object (elements and meshes): the
  first gets the name, the following get name + "1", name + "2", …
- Object menu "Rename" (Shift+N): dialog "Rename elements" / "Rename meshes" / "Rename selection" by
  selection kind; undo "Rename elements to X" / "Rename meshes to X" / "Rename selection to X"; every
  selected element/mesh gets exactly the new name.
- Element label field (settings panel): double quotes are stripped while typing; no undo.

### 10.5 Colour

- "Apply random color to selected elements" (undo "Randomized color"): each selected element gets a random
  colour (RGB in [0.6, 1], alpha 1).
- "Set color of selected elements" (colour dialog "Element Color" / "Elements Color", initial = primary
  colour): result channels clamped to ≥ 0.25 (alpha untouched), applied to all selected elements; undo
  "Set color".

### 10.6 Visibility and lock

| Command | Effect | Undo |
|---|---|---|
| element popup Hide / Show / Toggle visibility (E) | hidden ← true / false / flipped, elements only | "Hide" / "Show" / "Toggle visibility" (elements) |
| element popup Lock / Unlock / Toggle lock (E) | locked ← true / false / flipped | "Lock" / "Unlock" / "Toggle lock" |
| element popup Focus lock (E) | lock every element, unlock the primary | "Focus lock" |
| object menu Hide/Show/Toggle visibility/Lock/Unlock/Toggle lock (O) | same on selected elements and meshes | same labels, category by selection |
| object menu Focus lock (O) | lock all elements (or all meshes, by primary kind / edit mode), unlock the primary | "Focus lock" |
| "Show all" (Ctrl+Shift+S) | unhide every element and mesh | "Show all" (objects) |
| "Unlock all" (Ctrl+Shift+U) | unlock every element and mesh | "Unlock all" (objects) |

Hidden objects are deactivated (not drawn, not pickable). With the preference "render hidden elements in
main view" on, hidden objects stay active but are drawn only in the 3D view layer.

### 10.7 Order

"Move to top" / "Move to bottom" (element popup, E): stable partition of the element list with the selected
elements first/last; undo "Move elements to top/bottom". Mesh equivalents in the object menu use "Move
meshes to top/bottom" with category *elements* (QUIRK: mesh order is not captured). "Next"/"Previous"
(PgUp/PgDown) select exclusively the element (or mesh, by primary kind/edit mode) after/before the first
selected one in list order, wrapping; with nothing selected Next selects index 1 and Previous the last
(QUIRK).

### 10.8 Faces, textures, tint, culling, shading (element popup; E; all elements in the selection)

| Command | Effect | Undo |
|---|---|---|
| Faces → Top/Down/North/South/West/East/All (enable or disable) | set the face active flag(s); rebuild triangles | "Enable Up face" … "Enable all faces" / "Disable … face" / "Disable all faces" (face names Down, Up, North, South, West, East) |
| Texture → "texture (default)" | all six labels "texture" | "Set faces texture to default" |
| Texture → "side + up + down" | down "down", up "up", sides "side" | "Change faces texture" |
| Texture → "side + cap" | down/up "cap", sides "side" | "Change faces texture" |
| Texture → "front + side + cap" | down/up "cap", south "front", north/west/east "side" | "Change faces texture" |
| Texture → "down + up + north + south + west + east" | each face its own name | "Change faces texture" |
| Texture → "Customize..." | six-field dialog prefilled from the primary (order down, up, north, south, west, east); OK writes the six labels to every selected element | "Custom faces texture" |
| Tint → Top / Down / Sides / All / None | tint index 0 on those faces / −1 on all | "Tint Up face", "Tint Down face", "Tint Sides faces", "Tint all faces", "Tint no face" |
| Culling → Top / Down / Sides / All / None | each named face's cull = its own direction / none | "Cull Up face", "Cull Down face", "Cull Sides faces", "Cull All face", "Cull no face" |
| Shading → "Enable shading" / "Disable shading" | shade flag | "Enable shade" / "Disable shade" |
| Shading → "Outer normals" / "Inner normals" | invertNormals false / true (rebuild with all faces active, 4.3) | "Outer normals" / "Inner normals" |
| UV → "Enable Snap" / "Disable Snap" | uvSnap (enable also re-maps) | "Enable UV-snap" / "Disable UV-snap" |
| UV → "Enable Symmetry" / "Disable Symmetry" | uvSnapMirror (enable re-maps only when snap is on) | "Enable UV-snap mirror" / "Disable UV-snap mirror" |
| UV → "Apply Default UV" | the scene item is wired to a handler that does not exist (likely no-op; UNVERIFIED) | — |
| "Apply UV mapping with world coordinates to selected elements" | confirm "Redefine UV Data": "This will replace UV Mapping of N selected Elements by new UV Mapping calculated with real coordinates (without rotation).\n\nDo you wish to continue?" (Yes/No); Yes → default mapping forced on each selected element | "Apply default UV" |

Copy/paste of face properties (element popup Copy/Paste submenus; one shared face-set clipboard plus a shade
clipboard; copy reads the primary element, paste writes every selected element):

| Item | Copies | Paste effect | Paste undo |
|---|---|---|---|
| "Faces visibility" | active flags | rebuild triangles | "Paste faces" |
| "Textures" | texture labels | uvSnap ← false, refresh UVs | "Paste texture labels" |
| "UV Mapping" | uv rect + rotation | refresh UVs | "Paste UVs" |
| "Tint" | tint indices | (paste item has no handler: no-op; UNVERIFIED) | — |
| "Culling" | cull directions | — | "Paste culling" |
| "Shading" | shade flag | — | "Paste shade" |

Copying never pushes undo. Tint paste is unavailable because the menu item references a missing handler.

### 10.9 Optimisation and sorting (element tools; all elements of the model)

**"Automatically removes hidden faces of all elements"** (undo "Optimize elements faces"): for every ordered
pair of distinct elements `(A, B)` (hidden and locked included): for each *active* face of A, if B is not
rotated and all four corners of A's face (A's rotated box corners, 4.3 order down `[c0..c3]`, up `[c4..c7]`,
north `[c0,c1,c5,c4]`, south `[c3,c2,c6,c7]`, west `[c0,c3,c7,c4]`, east `[c1,c2,c6,c5]`, corners taken from
the transform) lie inside B's render AABB expanded by 0.1 on every side → deactivate the face. Status
"Optimization j/count, removed N face(s)..." (yields every 200 000 pair tests), log "Optimization: N faces
removed". Then, if some elements have no active face: "Empty elements detected" — "N elements don't have any
face.\n\nDo you want to remove them?" ("Remove"/"Keep"); Remove deletes them. Finally "Model faces
optimization done" with "No face has been removed...", "1 face has been removed:" or "N faces have been
removed (P%):" (P = removed·100 div before), then "Before: X faces" and "After: Y faces". Result is
independent of pair order.

**"Automatically apply new culling properties to all elements"** (undo "Cull elements faces"; tolerance
`t = 0.1`; block = internal `x,y ∈ [0,16]`, `z ∈ [−16,0]`). Classify a point: *inside* (none) if strictly
inside the block; *down* if `y < −t` with x, z strictly inside; *up* if `y > 16+t` with x, z inside; *north*
if `z > t`; *south* if `z < −16−t`; *west* if `x < −t`; *east* if `x > 16+t` (each with the other two
coordinates strictly inside); otherwise *any*. Per element:
1. For each direction f in order down, up, north, south, west, east: if **any** of the element's six faces has
   all four corners classified as either inside or f, with at least one f → set **all six** faces' cull to f
   (last matching f wins) and count 6 (QUIRK).
2. Otherwise, for each face whose corners are all within the block expanded by t (QUIRK: the lower Z bound is
   not checked) and lie on that face's block plane within t (down `y≈0`, up `y≈16`, north `z≈0`, west `x≈0`,
   east `x≈16`, south `z ≤ −16+t` with a buggy lower bound) → cull = own direction, count 1.
Message "Model culling optimization done": "N faces have been culled." (N > 1) or "N face has been
culled...".

**"Automatically sort transparent elements"** (undo "Sort transparent elements"): stable partition with
transparent elements (4.8) moved to the end; status "Sorting element i/count...", log "Sorted N transparent
elements"; message "Transparent elements sort": "No element with transparent texture to sort...", "1 element
with transparent texture has been sorted." or "N elements have been sorted.".

### 10.10 Recover panel

| Button | Behaviour | Undo |
|---|---|---|
| Show all / Unlock all | 10.6 | 10.6 |
| "Reset Blueprint to default" | blueprint → (-2,0,2), rotation 0, size 1 | "Reset blueprint" (elements) |
| Reset camera | view reset (views specification) | "Reset camera" (viewProperties) |
| "Constrain all elements inside available 3D space" | for every far element: per axis, if its render AABB min < volume min, shift by the difference; else if max > volume max, shift back; move keeping origin offset | "Get back far elements" (elements) |

### 10.11 Model rotation (Edit menu "Rotate model +90° on X/Y/Z axis")

Undo "Rotate model on X/Y/Z" (elementsAndVoxels — mesh changes are not captured, QUIRK). Every **unlocked**
element gets a quarter turn (4.9) about the block centre; the voxel world rotates (7.9, model box unchanged);
every unlocked mesh turns about (8,0,−8) (6.4).

### 10.12 Object menu: rotation and alignment (O)

| Command | Behaviour | Undo label |
|---|---|---|
| "Rotate 90° around X/Y/Z" | selected elements: quarter turn about the block centre; meshes: 6.4 | "Rotate around X axis" / "… Y axis" / "… Z axis" |
| Reset rotation | each selected object: rotation 0 (forced, about pivot); for elements the origin ← position | "Reset selected objects" |
| "Apply rescale" | for objects whose single rotation angle is ±45: multiply the size of the two non-rotated axes by 1.4142; ±22.5: by 1.08264; refresh UVs | "Apply rescale" |
| Center on X / Y / Z | `c` = primary centre on that axis (`px + sx/2`, `py + sy/2`, or `pz − sz/2`); every selected object is moved (keeping its origin offset) so its centre is `c` on that axis | "Center on X" / "Center on Y" / "Center on Z" |

### 10.13 Object menu: transform dialogs (O)

Vector dialogs prefill X/Y/Z; an empty field means "unchanged" (NaN); invalid text parses to 0.

| Command | Dialog (title / prompt / prefill) | Behaviour | Undo |
|---|---|---|---|
| Set position | "Set position" / "New position (leave empty to not modify)" / primary origin in UI space | `offset = (vx − ox, vy − oy, −(vz − oz_ui))` per non-empty field; primary `setOriginMove(origin + offset)` with propagation — i.e. it positions the **origin** | "Set position" |
| Move (Shift+M) | "Move" / "Offset (leave empty to not modify)" / last value | offset `(x, y, −z)` in UI space, applied as above | "Move" |
| Set size | "Set size" / "New size (leave empty to not modify)" / primary size | write each selected object's scale directly (no clamp, no pivot compensation); refresh UVs | "Set size" |
| Scale | "Scale" / "Scale (leave empty to not modify)" / last (default 1) | about the primary's origin `O`: `s ← s ⊙ f`, `p ← (p − O) ⊙ f + O`, `o ← (o − O) ⊙ f + O` for every selected object (no clamp) | "Scale" |
| Inflate (elements only) | "Inflate" / "Inflate (leave empty to not modify)" / last (default 1) | UV snap off; about each element's own centre `C = (px + sx/2, py + sy/2, pz − sz/2)`: size, position and origin scaled by `f` (no clamp) | "Inflate" (elements) |
| Randomize | "Randomize" / "Random position (leave empty to not modify)" / 0 | add an independent random offset in `[−v/2, v/2]` per axis (internal axes, Z not negated) to each object's position and origin | "Randomize position" |

Dialog-based scale changes bypass auto-UV (only the mesh UVs are refreshed) and allow zero/negative sizes
(QUIRK).

### 10.14 Meshes popup: mesh texture and colour

Submenus of the Meshes popup: "Texture" ("Load texture...", "Save texture...", separator, "Remove alpha",
separator, "Clear texture"), "Copy" ("Texture", "Mesh color") and "Paste" ("Texture", "Mesh color"). The **mesh
guard** passes when the selection holds at least one mesh (mixed selections pass); otherwise Information box "No
mesh selected" with `You must select at least one mesh or primitive to proceed.` (empty selection) or `This
feature doesn't work on elements.` + blank line + `Select at least one mesh or primitive to proceed.` (elements
only).

| Command | Targets | Guard | Behaviour | Undo |
|---|---|---|---|---|
| Load texture... | every **selected** mesh that already has a main texture | none (with nothing suitable selected the dialog opens and then nothing happens) | open dialog "Select texture file" (5.13, mesh start folders); on accept: remember the folder, status `Material Manager: New texture loaded: <path>`, decode with the decoders of 5.10 **without** slot downscale or `.mcmeta` crop; on success the **same** image object becomes the main texture of every target (later in-place edits affect all of them) | none |
| Save texture... | the **primary** mesh | none: the default name `<label>_<id>` is built from the primary before the dialog, so without a primary mesh it throws (QUIRK) | save dialog (5.13); writes the primary's main texture as PNG (5.11); without a texture: Error box "Error while saving texture" / `An error occured while saving texture.` + blank line + `Selected mesh don't have a valid texture to save.` | none |
| Remove alpha | every selected non-element object with a main texture | none | alpha ← 255 on every pixel, in place; shared images are processed repeatedly (idempotent); no message | none (QUIRK) |
| Clear texture | every selected non-element object with a main texture | mesh guard, then Question box "Clear meshes textures" / `Are you sure you want to clear textures of selected meshes?` ("Clear", "Cancel"); on Clear the guard runs again | fill every pixel with the current left colour, in place | "Clear meshes textures" (meshes), after Clear |
| Copy → Texture | the primary, if it is a mesh with a main texture | mesh guard | store a **reference** to that image in the session clipboard (no pixel copy: later in-place edits change the clipboard too) | none |
| Copy → Mesh color | the primary mesh | mesh guard | store its material colour | none |
| Paste → Texture | the primary mesh, if it has a main texture | clipboard not empty (no message otherwise) | main texture ← a copy of the clipboard image (size, format, filter and image name kept); the previous image is not released (UNVERIFIED whether that leaks) | none (QUIRK) |
| Paste → Mesh color | the primary mesh | clipboard set | material colour ← the stored colour | none (QUIRK) |

Load, Clear and Remove alpha act on the whole selection; Copy, Paste and Save use the primary only. Set Color and
Apply Color (mesh tint) are in 6.3.

### 10.15 Element settings panel (tabs "General", "Faces", "Textures UV")

(The Objects-menu item that opens the third tab is worded "Element UV Mapping...".)

Shows the primary element; when none, an overlay "Select an element to access its settings" fades in.
**No edit made in this panel pushes an undo step** (QUIRK). Fields that have keyboard focus are not
overwritten by refreshes.

- General: "Label" (writes the label, strips `"`), "Position" (UI space → position, 3.2), "Size" (size
  setter, 3.3), "Origin" (UI space → origin only), toggles "UV Snap", "UV Snap Symmetry", "Disable shading"
  (inverse of shade), "Rescale element based on rotation", "Invert faces normals". Any label/toggle change
  rewrites all five flags from the toggles, runs the default mapping if snap is on, refreshes UVs and the
  list.
- Faces (rows Down, Up, North, South, West, East): "Active", "Tint index:" (int), "Culling:" (none, down, up,
  north, south, west, east), "Texture:" (label list). Any change writes all rows back into the faces and
  refreshes colours and UVs; Active also rebuilds triangles.

### 10.16 Textures UV tab and the UV view

Face selector (Down, Up, North, South, West, East) chooses the *current face*; the UV view shows the face's
own texture and rectangle.

**Layout.** The UV view is a clipped scroll view (54 px below the tab's top). Its content holds the **zone** (the
texture area) with a 32 px margin on every side, so content size = zone size + 64 on both axes. The content scale
starts at 0.5.

**Zone and texture.** The zone keeps the texture's aspect ratio: when width ≥ height it is 512 wide (height
`512·h/w`), otherwise 768 tall (width `768·w/h`; QUIRK: portrait textures get a larger zone). The texture is the
face label's entry image ("error" for an unknown label), drawn with point filtering (the entry image's filter is
set to point). Outline and markers are tinted by face: down/up (0.5, 0.5, 1), north/south (0.2, 0.9, 0.2),
west/east (1, 0.3, 0.3).

**Coordinates.** Zone units with the origin at the zone's top-left, x to the right, y up (the zone spans
`y ∈ [−H, 0]`, `W × H` = zone size). The rectangle's pivot is its top-left corner. Its normalised form is
`(x, y, w, h) = (left/W, −top/H, width/W, height/H)` with y growing downward; a negative width or height means a
mirrored rectangle.

**Face → view** (on face selection and after face edits; never writes back): with the pixel view
`[x1, y1, x2, y2]` of 4.5, top-left = `(x1/16·W, −y1/16·H)`, size = `((x2 − x1)/16·W, (y2 − y1)/16·H)`; the
rotation marker follows the face rotation.

**View → face** (write-back): `tl = (x, 1 − (y + h))`, `br = (x + w, 1 − y)` (4.5 setter from a 0..1 rect;
negative sizes store reversed corners); rotation and all other face fields are untouched. Then the element's UV
snap is turned off, the General tab is refreshed (its snap toggle shows off) and the element's UVs are refreshed.
Writes happen during the gesture (every drag frame, every arrow press, each snap step of a double click), not on
release, and push no undo entry (10.15).

**Grid step** `g = gridStepUv · 32` zone units (default gridStepUv 0.5, so `g` = 16 zone units = half a face
pixel for a 512-wide zone). Rounding is half to even.

| Gesture | Condition | Effect |
|---|---|---|
| mouse wheel | pointer over the view | content scale x and y += 0.5 × the frame's "Zoom" axis value, each clamped to `[0.5, 4]` (axis sensitivity 0.1, so 0.05 per raw wheel unit; raw units per notch UNVERIFIED); scroll inertia stops and both normalised scroll positions are multiplied by new scale / old scale (proportional, not anchored at the cursor). The wheel never scrolls the view and never reaches the 3D views |
| pan | PanView held (middle button or Space) | dragging pans the scroll view (clamped movement, elasticity 0.1, inertia on); a drag that starts on the rectangle is forwarded to the scroll view event by event while PanView is held (releasing PanView mid-drag can leave the scroll view in its dragging state; visible effect UNVERIFIED). Without PanView a drag on the background does nothing. Ctrl does **not** pan |
| left drag on the rectangle | PanView not held | **move**: new top-left = top-left at press + pointer displacement in zone units (correct at any zoom); each coordinate rounded to `g`, then clamped inside the zone: `x ∈ [0, W − w]`, `y ∈ [h − H, 0]` (mirrored: `x ∈ [|w|, W]`, `y ∈ [−H, −|h|]`); clamping after rounding can leave the rectangle off-grid at an off-grid zone edge |
| right drag on the rectangle | PanView not held | **resize**: width = width at press + dx, height = height at press − dy (dragging right or down enlarges); top-left fixed; right edge = `round(min(left + width, W)/g)·g`, bottom edge = `round(max(top − height, −H)/g)·g`. Only these two edges are clamped, so a mirrored rectangle can push its far edge out of the zone (QUIRK; the face corners are clamped to `[−1, 2]` on write, 4.5) |
| arrow keys | pointer over the view; key-down edge only, no repeat in code | Right/Left move by `+g`/`−g` in x, Up/Down by `+g`/`−g` in zone y (Up = toward the top). The step is added in **screen pixels** and then snapped in zone units, so with content scale `s` (canvas scale 1 assumed): `s < 0.667` → 2g per press, `0.667 ≤ s < 2` → g, `s = 2` → 0 or g (half-to-even), `s > 2` → no move. The face is written (and UV snap turned off) on every press, even without a move; an off-grid rectangle snaps on its first press |
| double click | two clicks within 0.3 s, any button, anywhere in the view | the rectangle becomes 32 × 32 zone units with its top-left at pointer + (−16, +16) screen pixels, then the move snap and the resize snap run (each writes back when it changes something). 32 zone units are one face pixel for square or landscape textures (`512/W` UV units in general); the square is centred on the pointer only at screen scale 1 |

**Markers.** The outline fills the rectangle (minimum corner, `|w| × |h|`). The rotation marker sits at the corner
given by the face rotation (0 → top-left, 90 → top-right, 180 → bottom-right, 270 → bottom-left, computed from the
signed size, so a mirrored rectangle shows it on the visually opposite side); the origin marker stays at the
top-left (pivot) corner. Both 24 × 24 markers are scaled by 0.5 / content scale every frame, keeping a constant
on-screen size of 12 px.

Only the wheel and the arrow keys require the pointer over the view; drags, pan and clicks do not.

### 10.17 UV tools

Every tool below ends by redrawing the view, refreshing the General fields and writing the Faces rows back
(so texture labels shown in the Faces tab overwrite any label set by the tool — see projections).

| Tool | Behaviour |
|---|---|
| "Rotate 90" clockwise / anticlockwise | current face rotation += 90 / −= 90 (normalised); UV snap unchanged |
| "Flip H" / "Flip V" | swap the x (resp. y) components of tl and br; UV snap unchanged |
| "Swap" | exchange all face data (except names) between the current face and its opposite (down↔up, north↔south, west↔east); UV snap unchanged |
| "Copy" | copy the current face's uv (rotation + corners) to the UV clipboard |
| "Paste" | Ctrl held: "Paste UV" — "Do you want to paste copied UV on all faces of selected element?" ("All faces", "<Face> face", "Cancel"); otherwise current face only; UV snap off |
| "Full" | confirm (below) "Apply Full Mapping"; all faces or current: corners ← (0,0)/(1,1), rotation kept; UV snap off |
| "Projection Up>Down", "West>East", "South>North" | confirm (below); UV snap off; see table |
| "Texture edit" | open the current face's texture entry in the external editor (5.12) |

Confirmation for Full/projections: titles "Apply Full Mapping", "Apply Projection Mapping Up>Down",
"Apply Projection Mapping West>East", "Apply Projection Mapping South>North"; texts "Do you want to apply full
mapping on all faces or only current face?" / "Do you want to apply a projection mapping from Up to Down on
all faces or only current face?" (West to East, South to North likewise), each followed by
"\n\nNote: This will disable UV Snap option"; buttons "All", "Current (<Face>)", "Cancel".

**Projections.** The source face S (up, west or south) first has its rotation set to 0 (remembered as `rS`).
With S's corners `P00, P10, P11, P01` (4.5, rotation 0) and the nudge `ε = (1/W, 1/H)` of the **up face's**
texture (0.01 if not found; QUIRK), each target face T (all faces except S, or only the current face) gets
rotation 0, the corners below, then the nudge (if `br.x − tl.x = 0` add/subtract `ε.x` to `br.x`; if
`br.y − tl.y = 0` add/subtract `ε.y` to `br.y`; "+" or "−" as listed), the rotation listed, and S's texture
label:

| Projection | Target | tl | br | nudge | final rotation |
|---|---|---|---|---|---|
| Up>Down | down | P01 | P10 | none | 0 |
| | north | P11 | P01 | − | 0 |
| | south | P00 | P10 | + | 0 |
| | west | P00 | P01 | +x, −y | 270 |
| | east | P10 | P11 | −x, +y | 90 |
| West>East | down | P00 | P10 | + | 270 |
| | up | P01 | P11 | − | 90 |
| | north | P00 | P01 | + | 0 |
| | south | P10 | P11 | − | 0 |
| | east | P10 | P01 | none | 0 |
| South>North | down | P00 | P10 | + | 0 |
| | up | P01 | P11 | − | 0 |
| | north | P10 | P01 | none | 0 |
| | west | P00 | P01 | + | 0 |
| | east | P10 | P11 | − | 0 |

Afterwards the faces (not the geometry) are quarter-turned by `rS` about the source axis (Up>Down: Y by
`rS`; West>East: X by `−rS`; South>North: Z by `rS`) — a no-op in the common case `rS = 0` (QUIRK otherwise).
Because the Faces rows are written back at the end, the texture-label copy is overwritten by the labels shown
in the Faces tab (net effect: labels unchanged; UNVERIFIED at runtime).

---

## 11. Clipboard, cut, copy, paste

### 11.1 State

In-memory only (never the OS clipboard, never saved): either an **object clipboard** (a snapshot of selected
elements and meshes plus the blueprint, in the same shape as an undo snapshot) with a **copy anchor** (the
primary's position at copy time), or a **voxel clipboard** (list of `(dx, dy, dz, value)` non-zero cells
relative to the copy box minimum). Copy and cut clear the previous clipboard first. New model clears it. The
Edit menu Paste item is enabled iff the clipboard is non-empty and labelled "Paste objects", "Paste voxels" or
"Paste"; it is refreshed after copy/cut/clear only.

### 11.2 Copy / Cut (Ctrl+C / Ctrl+X)

- **Voxel branch** — voxel world initialised, exactly one object selected and it is the blueprint: box =
  rounded indices of the blueprint position and of `position + (sx, sy, −sz)`; for `i, j, k` in
  `[min, max)` (x outer, z inner): store non-zero cells relative to min; cut also writes 0 to every visited
  cell (bounded raw writer).
- **Object branch** — otherwise, with a non-empty selection: anchor = primary position; snapshot (category
  objects, selection only): selected elements, selected meshes and the blueprint record; cut then deletes the
  selected elements and meshes.
- Cut menu rules: if the primary is the blueprint → with voxels "Cut voxels" (voxels) and cut; without voxels
  message "No voxel to cut" — "When only blueprint is selected, voxels are cut.\n\nYour model doesn't have any
  voxel currently."; otherwise by selection kind "Cut elements" (elements), "Cut meshes" (meshes), "Cut
  selection" (objects); empty selection → "No selection" — "You must select at least one element, mesh,
  primitive or blueprint to proceed.". Copy pushes no undo. QUIRK: a blueprint selected together with other
  objects takes the voxel message path in Cut but executes the object branch.
- Voxel menu "Copy voxels"/"Paste voxels" select the blueprint exclusively first, then run the plain copy or
  paste. "Paste voxels" pushes **no** undo entry (QUIRK; only Edit > Paste records "Paste") and pastes whatever
  the clipboard holds, objects included (11.3).

### 11.3 Paste (Ctrl+V; undo "Paste", objectsAndVoxels)

- **Voxels**: origin = rounded index of the blueprint's *current* position; every stored cell is written with
  the bounded raw writer at origin + offset (merge: cells not in the clipboard are kept; cells outside the
  active box are dropped). Selection unchanged.
- **Objects**: offset = blueprint position (now) − copy anchor. Clear the selection; restore the stored
  blueprint record onto the blueprint (the blueprint jumps back to its copy-time transform; QUIRK); for every
  stored element create a new element and apply the record with the offset (position and origin + offset; size,
  rotation (re-quantised), hidden, locked, label verbatim; when the offset is non-zero the stored uvSnap is
  cleared first); the same for meshes; every pasted object is selected. Existing objects are never replaced.

### 11.4 Shared-record aliasing (port decision)

The original applies stored face sets by reference: two elements pasted from the same clipboard (or an
element restored by undo) share face objects with the clipboard/snapshot, so later face edits leak into the
stored copy and into sibling pastes (QUIRK, almost certainly unintended). **Port decision: deep-copy on every
restore/paste.** This only differs from the original in cases that are bugs there.

---

## 12. Voxel tools

### 12.1 Toolbar

Nine tools (one active at a time; priority when several are on: Dot, Line, Cuboid, Sphere, Cube, Extrusion,
Shell, Laser, Wrap). Tools are disabled until the voxel world is set up. Size inputs (visible only with their
tool): "Sphere brush diameter" (default 7), "Cube brush diameter" (default 7); radius = value/2, the value being
read as an integer (8.10: empty or decimal text reads 0).

| Button tooltip | Left button (left colour) | Right button (right colour) |
|---|---|---|
| "Dot: Add/Remove one voxel" | add one voxel, "Voxel add" | recolour one existing voxel, "Voxel sub" |
| "Line: Add/Remove line of voxels" | add line, "Voxel add line" | recolour existing along a line, "Voxel sub line" |
| "Cuboid: Add/Remove cuboid of voxels (like a selection)" | add box, "Voxel add cuboid" | recolour existing in box, "Voxel sub cuboid" |
| "Sphere brush: Add/Remove sphere of voxels" | add sphere, "Voxel add sphere" | recolour existing in sphere, "Voxel sub sphere" |
| "Cube brush: Add/Remove cube of voxels" | add cube, "Voxel add cube" | recolour existing in cube, "Voxel sub cube" |
| "Laser: Paint or cut voxels in depth" | recolour along the depth line, "Voxel laser paint" | erase along the depth line, "Voxel laser cut" |
| "Extrusion: Add/Remove thickness to voxels face" | extrude exposed region, "Voxel face extrusion" | remove exposed layer, "Voxel face intrusion" |
| "Shell: Add/Remove layer around collateral voxels" | add a layer around the component, "Voxel shell" | recolour the component's surface, "Voxel shell" |
| "Wrap: Set blueprint around group of collateral voxels" | fit the blueprint to the connected component and select it | — |

"Remove" works through the right colour: its default is fully transparent ("#00000000"), so right-button
tools erase by default; with an opaque right colour they recolour (existing voxels only). Every tool's undo
category is voxels. The default left colour is "#808080".

### 12.2 Stroke lifecycle and hit mapping

- First callback of a press: begin flag set, chunk collider updates suspended, undo pushed (label above),
  hit normal captured.
- Left Alt held during a callback: eyedropper instead (picked colour → that button's colour; 13.4).
- Release: collider updates resumed, all chunk colliders refreshed.
- **Inward cell** = truncated index of `hit − n·c/2`; **outward cell** = truncated index of `hit + n·c/2`
  (n = hit normal).
- **Plane pin** (tools marked "pinned"): on begin, remember the cell coordinate on each axis where
  `round(n)` is non-zero (others 0); afterwards a stored coordinate that is non-zero replaces the computed one
  (QUIRK: a pinned plane at index 0 is ignored).
- **Same-face gate** (Dot add, Laser): later frames act only if the hit normal equals the begin normal.
- **Dedupe**: act only when the target cell key (`i + N·j + N²·k`) differs from the last processed key, or
  on begin.
- **Preview cache** (Line, Cuboid): begin clears it and stores the anchor cell; on later frames whose target
  changed, every cell recorded so far is restored to its original value (the cache is emptied only if it holds
  more than 4096 entries; QUIRK) before redrawing from the anchor, giving a rubber-band preview. Sphere, Cube
  and Extrusion record originals without rollback.
- Voxel pointer (hover): a 0.01-thick plate drawn on the hit face of the inward cell (12.2 mapping), size
  `c × c`.

### 12.3 Tool algorithms

- **Dot add**: same-face gate; outward cell (pinned); full write of the left colour.
- **Dot sub**: inward cell (not pinned); write the right colour only if the cell is non-empty.
- **Line** (add: outward cells; sub: inward cells, existing only; the plane pin is re-taken on every frame, so
  lines are effectively unpinned): integer 3D Bresenham from anchor to target: major = max(|Δ|), each minor
  error starts at `floor(major/2)`, `major + 1` cells; radius `≤ 1` → single cells, else a sphere stamp per
  cell (the voxel tool's sub line uses radius 1/2 → single cells; the paint tool's line uses
  thickness/2).
- **Cuboid** (add: outward cells; sub: inward cells, existing only; effectively unpinned): inclusive box
  between anchor and target cells.
- **Sphere stamp** (centre = inward cell; pinned for add, unpinned for sub; radius r): cells in
  `[round(c−r), round(c+r)]` inclusive per axis with `Δ² ≤ trunc(r²)`; sub = existing only.
- **Cube stamp** (centre = inward cell; pinned for add, unpinned for sub; radius r): cells in
  `[round(c+0.5−r), round(c+0.5+r))` per axis; sub = existing only.
- **Laser**: same-face gate; inward cell; walk the column through the whole grid in the direction into the
  solid (decreasing index for normals +x, +y and internal −z; increasing for −x, −y and internal +z) and
  replace every **non-empty** cell (gaps are skipped, the walk continues); decreasing walks stop at index 1
  (QUIRK: index 0 is never reached). Right button uses fully transparent → erase.
- **Extrusion / intrusion** (begin frame only): axis from the hit normal (x if non-zero, else y, else z);
  direction `d = round(n)` (z negated for the index axis); seed = inward cell. Depth-first search over cells of
  the seed's layer: a cell is *exposed* when non-empty and the cell at `+d` is empty; exposed cells push their
  four in-plane non-empty unvisited neighbours. Extrude writes each exposed cell's value into `cell + d`;
  intrude writes 0 into the exposed cell.
- **Shell add** (begin only): breadth-first search over the 6-connected non-empty component of the inward
  seed; every empty 6-neighbour of the component receives the left colour (bounded raw writer).
- **Shell recolour** (right, begin only): every component cell with at least one empty 6-neighbour receives
  the right colour.
- **Wrap**: 6-connected component bounds of the inward seed (inclusive; the seed itself counts even when
  empty); the blueprint is moved (origin and position) to the world corner of the min cell and resized to the
  box size; it is selected 0.1 s later (becoming the primary selection, which switches the voxel tool off,
  8.10). QUIRK: an out-of-range seed yields an inverted box.

### 12.4 Blueprint voxel commands (Voxel menu → Blueprint)

Box = rounded indices of the blueprint position and of `position + (sx, sy, −sz)`.

| Command | Behaviour | Undo |
|---|---|---|
| "Create voxels" | fill the half-open box between the two rounded corner indices (per-axis min/max) with the left colour (opaque), full writer | "Fill blueprint with voxels" |
| "Delete voxels" | fill with 0 | "Fill blueprint with voxels" |
| "Copy voxels" / "Paste voxels" | select the blueprint exclusively, then copy (11.2) / paste whatever the clipboard holds (11.3) | none / none (QUIRK: unlike Edit > Paste) |
| "Flip voxels on X/Y/Z axis" | mirror the box `[min, max)` along the axis by swapping cell `k` with `n−1−k` (n = box extent) using the unchecked writer (neighbour chunks not refreshed) | "Flip voxels in blueprint on X/Y/Z" |

### 12.5 Hollow and fill volume

Scan the whole grid; a cell is *interior* when all six face neighbours are inside the grid and non-empty
(reads use the box rule). Status "Calculating shell..." with progress, yielding every 262 144 cells. Then each
recorded cell that is non-empty is set to 0 (hollow) or to the left colour (fill) — enclosed empty cavities are
**not** filled. Status "Filling shell i/n...". Auto-rebuild is off during the write pass. Completion: "Hollow
voxels success!" + dialog "Volume hollowed" / "Voxels volume has been hollowed successfuly"; or "Fill voxels
success!" + "Volume filled" / "Voxels volume has been filled with color successfuly".

### 12.6 Paint tools on voxels

See 13.3. Painting never adds voxels.

### 12.7 Guard

Commands that need a world show "Setup Voxels first" — "<reason>, be sure to setup Voxels first.\n\nDo you want
to setup now?" ("Setup", "Cancel"). Reasons: "No voxel to convert to elements", "No voxel to clear", "No voxel to
crop", "Nothing to reset", default "To use this function".

---

## 13. Paint and colour tools

### 13.1 Toolbar

| Tooltip | Size field (default) | Left/right behaviour |
|---|---|---|
| "Pen: Set color of one pixel/voxel" | — | one texel |
| "Line: Draw a line of pixels/voxels" | "Line diameter" (1) | rubber-band line |
| "Rectangle: Create a rectangle of pixels or voxels" | — | rubber-band filled rectangle |
| "Round brush: Change color in round area" | "Round brush diameter" (7) | disc stamp |
| "Square brush: Change color in square area" | "Square brush diameter" (7) | square stamp (right button: no effect, QUIRK) |
| "Bucket: Flood zone with color" | — | flood fill, tolerance 0 |
| "Sprayer: Change first clicked color in sprayer area" | "Sprayer diameter" (7) | disc; on voxels match-colour mode |
| "Magic: Replace clicked color with new color" | — | global colour replace |
| "Noise: Flood zone with random shades of color" | "Noise amount (0-255)" (32) | flood fill with jitter |

Priority when several are on: pen, line, rectangle, brush, square, sprayer, fill, magic, noise. Initial state:
all off. Sizes are read as integers (8.10); radius = diameter/2 (as a float, so 7 gives 3.5); the line thickness
is the line size itself (the line routine then uses thickness/2 as its disc radius).

### 13.2 Texture path (elements and meshes)

Target: the hit renderer's main texture (elements: the runtime atlas). Requirements: a renderer with a
material and a mesh collider; the **first hit on a newly hovered object is ignored** (the stroke restarts on
the next frame). Texel `(trunc(u·W), trunc(v·H))` from the hit UV (no wrap). Clip rect: the atlas entry that
contains the hit UV when painting the atlas, else the whole texture. Each operation applies the texture
immediately; errors are logged ("DrawPixel error: ", "DrawLine error: ", "DrawRectangle error: ", "DrawBrush
error: ", "DrawBrushSquare error: ", "DrawFill error: ", "DrawMagic error: ") and end the stroke. Undo
(category texturesAndMeshes) is pushed on the first effective frame: "Paint pixel", "Paint pixel line",
"Paint pixel rectangle", "Paint pixel disc" (brush and sprayer), "Paint pixel square", "Fill pixels" (fill and
noise), "Replace pixels color".

| Tool | Algorithm |
|---|---|
| Pen | skip if `(x, y, colour)` equals the last painted texel; set the texel (no clip) |
| Line | begin: remember start texel/UV and copy the texture; later frames: restore the copy, draw an integer Bresenham line start→current (`err = dx + dy` form); thickness ≤ 1 → single texels, else a disc of radius thickness/2 per point; clipped to the start's rect |
| Rectangle | same copy/restore; fill the inclusive axis-aligned rectangle start→current (not clipped, QUIRK) |
| Brush / Sprayer | texels with x in `[trunc(cx−r), max(1, cx+r))` (same for y), inside the clip rect, excluding those with `Δ² ≥ r²−1` when `r²−1 > 1`; the sprayer's "first clicked color" sampling is not used on textures (sprayer = brush, QUIRK) |
| Square | texels in `[round(c+0.5−r), round(c+0.5+r))` per axis; the clip rect is computed but **not applied** (QUIRK: can paint across atlas entries) |
| Fill / Noise | scanline flood fill from the texel, within the clip rect: a texel joins when it is similar to the clicked colour (each RGB channel within T/255, alpha equal; T = 0 for Fill, noise amount for Noise) and not already similar to the fill colour (within T/510); with T > 0 each painted texel gets the fill colour plus an independent uniform offset in `±T/2` levels per RGB channel (clamped) |
| Magic | every texel inside the clip rect equal to the clicked colour becomes the paint colour |

Stroke end (release): clear the dedupe memory, write the atlas back into the textures (5.3), refresh the
texture preview, raise GeometryChanged, refresh voxel colliders if a world exists.

### 13.3 Voxel path (paint tools over voxel chunks)

Labels (category voxels): "Paint voxel" (pen → recolour the inward cell if non-empty), "Paint voxel line" (line
of existing cells, thickness = line size), "Paint voxel cuboid" (rectangle → recolour existing in the box),
"Paint voxel sphere" (brush/sprayer → recolour existing in the sphere; the sprayer additionally requires the
cell to equal the colour sampled at the centre on begin), "Paint voxel cube" (square), "Fill voxels" (fill and
noise → flood fill over the connected non-empty cells similar to the seed colour with tolerance T; jitter as
13.2 when T > 0; unchecked writer), "Replace voxels color" (magic → replace the seed colour everywhere in the
world, palette entry renamed).

### 13.4 Colour tools

- Two colours: **left** (default "#808080") used by left strokes, mesh tint, voxel fills; **right** (default
  "#00000000") used by right strokes. Hex fields accept "#RRGGBB" or "#RRGGBBAA" (7.3 parsing).
- "Color Picker: Get clicked color" (toggle): custom mode; left click sets the left colour, right click the
  right colour, from the hit (voxel value, or texel colour including alpha — a transparent texel yields a
  transparent colour); "nothing hit" (sentinel 1) is ignored. The texture picker also returns "nothing" when the
  texel equals the last texel remembered by the paint dedupe memory (QUIRK). Left Alt during any paint/voxel
  stroke does the same for that button.
- "Switch left and right colors": swaps them.
- Palette panel: left/right click on a swatch sets the left/right colour. Presets rgb256, hsl256, jmp16,
  generic16, pico8, edg32, edg16; "load palette" accepts PNG/JPG reduced to at most 16×32 px; the current
  palette image is kept as `palette.png` in the user data folder. Palette details: UI specification.

---

## 14. Conversions

### 14.1 Meshes → voxels ("Voxelization")

**Dialog** "Voxelization" ("Convert to voxels..." in the Meshes popup): "Resolution:" slider 4..128 (default 16,
then the last value) with the label "x<N>", description "Number of voxels on one axis per zone", toggle "Keep
inside empty" (the empty-hull option), buttons "OK" and "Close", plus the window's close button. Not modal; Escape
= Close; Enter does nothing. Opening is refused only when no mesh exists ("No mesh to voxelize" — "Please add
primitives or import meshes to start voxelization."); a running conversion is not checked, so the dialog can be
reopened during a run. Opening changes nothing in the scene.

There is no separate "voxelization mode": the state is a status (not started, in progress, done, aborted) and a
busy flag. **OK**: when the status is "in progress" nothing starts; otherwise, if a world exists with another
resolution: "Confirm replace of voxels" — "Selected resolution xNEW is different from existing voxels
xOLD.\nVoxelization will erase current voxels.\n\nAre you sure you want to voxelize the model?." ("Erase and
Start", "Cancel") → reset and set up at the new resolution; no world → set up; same resolution → merge into the
existing voxels (no clear). Then the run starts. The dialog hides right after OK in every case.

0. Busy gate: if the busy flag is set the start does nothing, silently; otherwise it is set.
1. Undo "Voxelization" (voxels+meshes); status "in progress". Sources: the selected meshes, or all meshes when
   nothing is selected (elements are never sources); each is marked and moved, with all its children, to a
   dedicated probe layer that every view still draws, but that the hover ray (8.4) does not hit. Selection
   cleared; nothing re-selects later.
2. Bounds: the union AABB of **all** meshes (QUIRK); index range = truncated indices of
   `(minX, minY, maxZ+0.1)` … `(maxX, maxY, minZ−0.1)`, inclusive. Writes are clipped by the voxel world's active
   box (7.2), so the model box is the effective limit; nothing previews the scan box or the resolution. Chunk
   auto-rebuild and collider refresh are paused for the run (7.6).
3. For `i`, then `j` (outer), then `k` (inner): probe box = the cell (7.2); if it overlaps any source triangle
   (surface overlap, not volume): sample the colour (below); if non-zero: write (bounded) and remember it as the
   last colour. Unless empty hull: if not inside-run and a ray from the cell's +Z face centre (internal) going −Z
   for one cell hits a source → push a run start `(k, colour)`; if a ray from the −Z face centre going +Z for one
   cell hits → run end at `k` (consecutive ends collapse to the last). Yield every 110 592 cells (a fixed count,
   48³, at every resolution) with the status "Voxelization in progress: N voxels..." and progress = percent / 100,
   where the percent divides by the product of the per-axis index spans without the +1 of the inclusive loops (so
   it can pass 100 % before the end; a model flat on one axis divides by zero; QUIRK). Small models show no
   progress at all.
4. After each column (unless empty hull): pop pairs (end, start) while both stacks have items; if start > end,
   keep the end; else fill cells strictly between with the start's colour.
5. Finish: resume auto-rebuild and collider refresh; status and log "Voxelized: X/Y voxels" (X = voxels written,
   interior fill included; Y = cells visited); progress 1.0; status "done"; delete every marked mesh; clear the
   busy flag.

**Close** (button "Close", the window's close button or Escape):
- Not running (not started, done or aborted): status ← "not started" and the run counters are reset; the window
  hides. Nothing else changes (no layer, status-line or progress change). File > New does the same reset.
- Running: status ← "aborted", the run stops at its current yield and **no** finish step runs: the marked meshes
  stay (marked, on the probe layer: visible but no longer pickable in the views until a hide/show of the object
  moves it back to the model layer), partial voxels stay, auto-rebuild and collider refresh stay paused (the voxel
  display stays stale until another flow resumes them, UNVERIFIED how soon), status line and progress keep their
  last values, the undo entry stays, and the **busy flag stays set**, so every later voxelization in the session
  silently does nothing (QUIRK). Voxel-to-element conversions have their own flag and are not blocked.
- QUIRK consequences: with the busy flag stuck, OK still runs the resolution handling first, so "Erase and
  Start" erases the existing voxels and then converts nothing. Loading a file sets the status to "in progress"
  without a run, so the first OK after a load does nothing (a Close, which then aborts, makes OK work again).

**Status and progress afterwards**: nothing restores them explicitly. The status line returns to "Ready" 10 s
after its last message (`app_behavior.md`); the progress value stays where the run left it
(1.0 after completion). The progress dialog that progress values below 50 % bring up (`ui_dialogs_menus.md`)
closes itself at 1.0; after an abort it stays open until the user closes it. Port decisions (proposed): clear the
busy flag, resume rebuilding and restore the source layer on abort, and do not let a file load mark a run as in
progress.

Colour sample: six rays of length `2c`, each starting one cell away from the cell centre and pointing toward
it, tried in this order: start on the −Y side pointing +Y; +Y side pointing −Y; +Z side pointing −Z; −Z side
pointing +Z; +X side pointing −X; −X side pointing +X (internal axes); the first hit wins. No hit → 0.
The hit triangle's submesh material: no texture → its colour (opaque); texture → texel at `trunc(uv·size)`
times the material colour (alpha kept, `trunc(·255)`).

Port notes: implement with a BVH over source triangles (triangle/AABB overlap + ray tests with single-sided,
front-face-only semantics as in the original physics queries); keep the scan order and run pairing exactly.
Voxelizing elements is not available (the dialog only voxelizes meshes).

### 14.2 Voxels → elements: one cube per voxel

Voxel menu "1 element per voxel (cubes)". Refused when `R > 32`: "Voxel resolution too high" — "This method to
convert voxels to elements is limited to resolution x32 or lower.\n\nPlease select another conversion method or
decrease voxel resolution.". Confirm "Confirm conversion to elements (cubes)" — "This conversion will create N
elements and create M textures.\n\nDo you want to continue?" (N = non-zero cells in the box, M = distinct
colours; "Continue", "Cancel"). Undo "Convert voxels to elements (cubes)" (elements+textures+voxels).
For each non-zero cell of the occupied-chunk bounds (inclusive loops, x outer): a texture "Voxel-RRGGBB" is
created once per colour (16×16, border pixels = colour × 0.8 RGB, interior = colour); a Cube element at the
cell's world corner with size `(c, c, c)`, white colour, label "Voxel-RRGGBB", UV snap off, all faces using that
texture. Afterwards all voxels are cleared. Status "Converting voxels to elements...", "Conversion to elements
in progress: N elements...", "Converted to elements: N elements".

### 14.3 Voxels → elements: palette and atlas (greedy boxes)

"1+ elements per color (palette)" and "1+ elements per surface (atlas)": confirm "Confirm conversion to elements
(palette)" / "(atlas)" — "Are you sure you want to convert all voxels to elements?\n\nThis will delete voxels."
("Convert", "Cancel"); undo "Convert voxels to elements (palette)" / "(atlas)". Base name = imported file name or
"voxels".

Scan the occupied bounds (inclusive, x outer, z inner); for each non-zero cell `(i,j,k)` with value `v`, grow a
box (exclusive maxima `x1, y1, z1`, initially the bounds max + 1):
1. along +x from `i+1`: stop at the first cell that is a *stop cell*;
2. along +y: for each row `j'` from `j+1`, test cells `[i, x1)` at `(·, j', k)`; stop at the first row containing
   a stop cell;
3. along +z: for each slab `k'` from `k+1`, test `[i,x1) × [j,y1)`; stop likewise.
Stop cell: palette mode — value ≠ `v` and the cell is not interior (12.5 interior test; interior cells of other
colours are absorbed); atlas mode — value = 0.
Create a Cube element at the world corner of `(i,j,k)` with size `((x1−i), (y1−j), (z1−k))·c`, then clear the
box (all cells set to 0).
- **Palette**: label "Voxels-RRGGBB" (the first six hex digits of the colour, upper case, alpha dropped); random
  colour; all six faces textured with "<base>_palette" and given the same uv, the centre of the palette cell of
  `v` (below); rotation 0; UV snap off.
- **Atlas**: label "Voxels"; for each element six face images are captured from the voxels (down = layer `j`
  over x×z, up = layer `y1−1`, north = layer `k`, south = layer `z1−1` over x×y, west = layer `i`, east = layer
  `x1−1` over z×y; colours decoded opaque); after the scan they are packed (padding 2 if "advanced atlas
  padding", square mode per preference) into a texture named "<base>_atlas" (+ smallest free numeric suffix);
  each face uv = its packed rect (up mirrored vertically, north and east mirrored horizontally), scaled by the
  square ratios. Status "Creating texture atlas...".

**Palette image** ("<base>_palette"). Built from the world palette (7.4), all `N` entries in list order including
entry 0 (the empty sentinel, normally code 0, so it paints opaque black):
- **Size**: square, side `S` = the smallest power of two ≥ `max(4, ceil(sqrt(N)))` (N ≤ 16 → 4, ≤ 64 → 8,
  ≤ 256 → 16, ≤ 1024 → 32, ≤ 4096 → 64). RGBA, no mip maps, point filter, repeat wrap. The image object is cached
  by the palette and reallocated only when `S` must grow (it never shrinks); every call rewrites all `S × S`
  texels.
- **Texels**: one texel per entry, no border. Entry `k` → column `k mod S`, row `floor(k / S)` counted from the
  **top** (bottom-up texel row `S − 1 − floor(k / S)`). RGB from the code, alpha forced to 255 (codes that differ
  only in alpha look the same). Texels from index `N` on are opaque black. Duplicate codes (7.4) give identical
  cells.
- **UV of colour `v`**: `idx` = position of the first entry equal to `v` (an absent value uses `idx = 0`); cell
  `c = idx mod S`, `r = floor(idx / S)`; the top-down rect is `x = (c + 0.24)/S`, `y = (r + 0.24)/S`,
  `w = h = 0.52/S` (the central 52 % of the cell), stored with the 0..1 setter of 4.5 (`tl = (x, 1 − (y + h))`,
  `br = (x + w, 1 − y)`), whose V flip cancels the top-down row order. Example: `S = 8`, `idx = 9` → tl
  (0.155, 0.780), br (0.220, 0.845). Corners are relative to the palette texture; the atlas mapping of 4.6 places
  them at run time.
- **Registration**: once per palette conversion, after the confirmation and before the element-creation run
  (also when there is no voxel), with the palette image as it is at that moment; the cube and atlas conversions do
  not register it. `<base>` = the stem of the last loaded voxel file, else "voxels". The registered image is the
  palette's cached object, so a later rebuild of the palette image rewrites it in place (seen in the atlas at the
  next repack). The element run computes the UVs from the live palette, so palette edits during a run would make
  image and UVs disagree (UNVERIFIED whether the UI allows that).
- **Label clash**: there is no uniqueness check (unlike the atlas conversion's numeric suffix). An absent label
  is added as a listed, selected entry (log "MaterialManager: Texture <label> did not exist and has been added");
  an existing label (an earlier palette run or a user texture of that name) gets its image replaced in place and
  its Minecraft path cleared, then repack, element refresh and texture changed. Faces from an earlier run keep
  their palette-relative corners and now sample the new image, so they change colour unless the palette kept its
  size and order (QUIRK, no warning).

---

## 15. Undo and redo

### 15.1 Model

Snapshot-based, linear history.

- An **entry** = `{label, snapshot, optional callback}`; the snapshot holds a copy of the categories given at
  push time (2.5), taken **before** the mutation (every command pushes first, then mutates).
- **Undo list** with a cursor (= number of applied entries) and a **redo cache** with a redo count.
- **Push(label, categories)**: drop undo entries at and after the cursor; clear the redo cache; if the list
  holds more than 10 entries remove the oldest; append; cursor = length; redo count = 0; mark the document
  dirty; refresh the Edit menu. **Maximum depth: 11 entries.** No coalescing: one entry per push.
- **Undo** (Ctrl+Z) when cursor > 0: cursor −= 1, redo count += 1; take entry E at the cursor; if the redo cache
  is shorter than `length − cursor`, append `{E.label, snapshot of the current state for E's categories,
  E.callback}`; restore E.snapshot (15.3); run the callback.
- **Redo** (Ctrl+Y) when redo count > 0: redo count −= 1, cursor += 1; restore `redoCache[redoCount]`; run its
  callback. Redo never creates entries.
- Undo/redo do **not** change the dirty flag.
- Menu text: "Undo" / "Undo " + label of the entry before the cursor; "Redo" / "Redo " + label of the next redo
  entry. Items are enabled only when available; text is refreshed after each push/undo/redo.
- New model and load clear the history and the dirty flag; saving clears the dirty flag. History is never
  persisted.

### 15.2 Snapshot content per category

| Category | Captured |
|---|---|
| modelProperties | all model properties including model box and origin, **excluding** the display transforms and the item overrides (2.5) |
| viewProperties | grid steps, snap flags, cameras |
| elements | the blueprint record + every element record (or only the selected ones for the selection-only snapshots used by copy and clone) |
| textures | every texture entry with its image |
| voxels | the whole world (if initialised) |
| meshes | every mesh record (geometry included for imported meshes) |
| modelDisplay | display transforms |

### 15.3 Restore semantics

For each category present:
1. If elements or meshes are present: clear the selection.
2. modelProperties: copy back and re-apply the model box.
3. textures: clear the texture list, then re-add every entry.
4. elements: delete every element (ids restart at 1), restore the blueprint transform, re-create each element in
   snapshot order (rotation re-quantised), re-select those whose `is` flag was set.
5. voxels: as 7.10 (an absent world resets an existing one).
6. meshes: delete every mesh, re-create from records (primitives rebuilt from their type; imported meshes from
   stored geometry), re-select flagged ones.
7. modelDisplay: copy back.
Errors are reported as an exception dialog.

### 15.4 Granularity

| Interaction | Entries |
|---|---|
| any menu/tool command | one, pushed before the change |
| mouse drag (move/rotate/scale/clone) | one, on the first drag frame |
| paint/voxel stroke | one per stroke (first effective frame) |
| keyboard nudge | none |
| element settings panel edits | none |
| model box handle drag | none |
| texture reload from disk on focus | one ("Reload textures from HDD", once per focus) |
| reload after an external texture edit | none |
| UV view gestures | none (10.16) |

### 15.5 Known capture gaps (QUIRKS to reproduce)

- "Rotate model on X/Y/Z": category excludes meshes.
- Clone mode: category elements only.
- "Move meshes to top/bottom": category elements.
- "Clear voxels", "Remove alpha", mesh texture load and texture/colour paste, the voxel popup's "Paste voxels",
  external texture edits (5.12): no undo.
- Item overrides: captured by no category (2.5).
- Commands with a selection guard push their entry even when nothing changes (e.g. mesh tools with no mesh).
- Texture-popup commands push before their dialog, prompt or question (a cancel leaves an entry); a double click
  on a texture row loads a file without any entry (5.13).
- An aborted voxelization leaves its entry (14.1).

### 15.6 Undo label catalogue

Elements: "Create Cube element", "Create PlaneX element", "Create PlaneY element", "Create PlaneZ element", "Clone
element", "Randomized color", "Set color", "Apply default UV", "Move elements to top", "Move elements to bottom",
"Optimize elements faces", "Cull elements faces", "Sort transparent elements", "Restrict elements rotation",
"Rename " + old, "Hide", "Show", "Toggle visibility", "Lock", "Unlock", "Toggle lock", "Focus lock",
"Enable/Disable Down|Up|North|South|West|East face", "Enable all faces", "Disable all faces", "Set faces texture
to default", "Change faces texture", "Custom faces texture", "Tint Down face", "Tint Up face", "Tint Sides
faces", "Tint all faces", "Tint no face", "Cull Down face", "Cull Up face", "Cull Sides faces", "Cull All face",
"Cull no face", "Enable shade", "Disable shade", "Outer normals", "Inner normals", "Enable UV-snap", "Disable
UV-snap", "Enable UV-snap mirror", "Disable UV-snap mirror", "Paste faces", "Paste texture labels", "Paste UVs",
"Paste culling", "Paste shade", "Reset blueprint", "Get back far elements", "Inflate", "Apply texture to
selection".
Objects/selection: "Drag selection", "Rotate selection", "Scale selection", "Clone selection", "Delete", "Paste",
"Cut elements", "Cut meshes", "Cut selection", "Cut voxels", "Select " + pattern, "Show all", "Unlock all",
"Rename elements to X", "Rename meshes to X", "Rename selection to X", "Set position", "Move", "Set size",
"Scale", "Randomize position", "Apply rescale", "Reset selected objects", "Rotate around X axis", "Rotate around
Y axis", "Rotate around Z axis", "Center on X", "Center on Y", "Center on Z", "Rotate model on X", "Rotate model
on Y", "Rotate model on Z".
Meshes: "Create Cube primitive", "Create Sphere primitive", "Create Cylinder primitive", "Create Cone primitive",
"Create Pyramid primitive", "Create Slope primitive", "Create Corner-in primitive", "Create Corner-out
primitive", "Create Torus primitive", "Set mesh block size", "Set mesh proportional size", "Set mesh tint", "Set
color", "Move meshes to top", "Move meshes to bottom", "Clear meshes textures".
Textures: "Load texture", "Set as shortcut", "Rename texture", "Edit texture label", "Empty texture", "Delete
texture", "Add new texture", "New textures", "Clear textures", "Add missing textures", "Remove unused textures",
"Create blank atlas", "Create full atlas", "Reload textures from HDD".
Paint: "Paint pixel", "Paint pixel line", "Paint pixel rectangle", "Paint pixel disc", "Paint pixel square",
"Fill pixels", "Replace pixels color", "Paint voxel", "Paint voxel line", "Paint voxel cuboid", "Paint voxel
sphere", "Paint voxel cube", "Fill voxels", "Replace voxels color".
Voxels: "Setup voxels", "Reset voxels", "Reset voxels color", "Clear voxels outside", "Hollow voxels", "Fill voxels
volume", "Upscale voxels x2", "Downscale voxels x2", "Rotate voxels around X|Y|Z", "Flip voxels on X|Y|Z", "Flip
voxels in blueprint on X|Y|Z", "Fill blueprint with voxels", "Voxel add", "Voxel add line", "Voxel add cuboid",
"Voxel add sphere", "Voxel add cube", "Voxel sub", "Voxel sub line", "Voxel sub cuboid", "Voxel sub sphere",
"Voxel sub cube", "Voxel face extrusion", "Voxel face intrusion", "Voxel shell", "Voxel laser paint", "Voxel
laser cut", "Voxelization", "Convert voxels to elements (cubes)", "Convert voxels to elements (palette)",
"Convert voxels to elements (atlas)".
Other: "Reset camera", "Edit display settings".

### 15.7 Unsaved-changes prompt

When the document is dirty and the user closes or replaces it: "Unsaved modifications" — "Do you want to save the
changes you made to model NAME?" ("Yes", "No", "Cancel"). Yes saves (then quits when quitting); No discards
(clears the dirty flag) and continues; Cancel aborts.

---

## 16. Rust implementation notes

### 16.1 Types

- `glam::Vec3`/`Quat` in **f32** for the document (parity of snapping); f64 only for derived, non-persisted
  computations that do not feed back into stored values.
- `bitflags` for `FaceFlags` (1..32, any 63), `AxisFlags` (1, 2, 4), `ChangeCategory` (1..64).
- Enums: `ElementType`, `PrimitiveType`, `ModelKind`, `SelectMode {Move, Rotate, Scale, Clone, Custom,
  Selection, SelectionMultiple}`, `EditMode`, `VoxelTool`, `PaintTool`, `ColorTool`.
- `Element { id, label, pos, size, rot, origin, flags: ObjectFlags, kind, color: Rgba, faces: [Face; 6],
  uv_snap, uv_mirror, rescale, shade, invert }`; `Face { name, active, tint: i32, cull: FaceFlags,
  texture: SmolStr, uv: UvRect }`; `UvRect { rot: i32, tl_raw: Vec2, br_raw: Vec2 }` with folding getters.
  Keep raw (clamped) and folded values distinct: files and copies use folded values.
- Voxel value `u32` with the ARGB convention (persist as `i32`).

### 16.2 Ownership and invariants

- `Document` owns ordered `Vec<ElementKey>` / `Vec<MeshKey>` over `slotmap` arenas (stable keys for selection,
  gizmo target and undo; display ids are a separate per-kind counter rebuilt on load/restore).
- All mutations go through command functions returning `Result` and an undo descriptor (label + categories); the
  command layer pushes the snapshot before mutating, exactly like the original.
- Invariants to assert in tests: size components ≥ 0.001 after the size setter; rotation components in
  `[0,360]` and on the 5° or 22.5° grid (meshes always on the 5° grid); face count = number of active faces; element ids strictly increasing in
  creation order; voxel cells outside the grid are unreachable; active box half-open; palette starts with 0.

### 16.3 Undo storage

Observable semantics (11 entries, category snapshots, before-state, redo cache) must match. Storage may share
structure: `Arc<ElementRecord>` copy-on-write per element, `Arc<Chunk>` per voxel chunk (a voxel undo then
costs only the touched chunks), texture images by `Arc<[u8]>`. Deep-copy on restore (11.4).

### 16.4 Performance-critical paths

- Voxel chunk meshing (7.6): per-chunk jobs on a thread pool (`rayon`), 32-bit indices, upload in a
  deterministic order; keep the non-greedy quad layout for parity of the editor view (greedy meshing is an
  *export* concern, export specification).
- Voxel tools allocate visited sets per click (extrusion uses an N³ bit set in the original): use a dense
  bitset sized N³ (N ≤ 384 → 7 MB as bits) or a hash set for sparse searches.
- Face optimisation is O(n²): add a spatial grid/BVH over expanded AABBs; results must stay identical (order
  independent).
- Runtime atlas: repack incrementally or with a shelf/guillotine packer (`etagere`, `guillotiere`); layout is not
  persisted.
- Focus reload (5.12): the original hashes every file-backed texture on every focus gain; compare file size and
  modification time first and hash only candidates (same observable result except for content changes that keep
  both).
- Primitive meshes (6.5–6.6) are generated once and shared by all objects of a type; only the paint texture is
  per object.
- Voxelization: BVH over triangles; parallelise per (i, j) column (each column is independent); keep the
  per-column run pairing.
- Picking: ray vs element OBBs and voxel DDA instead of physics colliders.

### 16.5 Determinism

Random colours (creation, blank atlas), "Randomize", noise jitter and paint/voxel noise use an injectable RNG;
parity tests seed it or mask the affected values.

---

## 17. Parity test checklist

1. Element creation at the default blueprint: position (-2,0,2), size 1, label "Box1", uv snapped mapping.
2. Size setter: pivot-relative rescale and 0.001 clamp; propagation to the selection.
3. Rotation setter: wrap, 5°/22.5° quantisation (half-to-even ties), restricted rejection, multi-selection pivot
   overwrite.
4. Restriction bake for angles 67.5, 90, 180, 292.5, 315 on each axis (faces, sizes, positions, origin).
5. Quarter turns X/Y/Z of a non-cubic element about (8,8,−8): positions, sizes, face cycles, UV rotations.
6. Auto-UV table for mirrored and unmirrored, including rotated elements (uses the rotated corner).
7. UV folding getters for rectangles in [−1,0], [1,2] and straddling 0/1.
8. Projections (all three, all faces) with and without degenerate source edges.
9. Mouse move with grid snap (absolute snap of unmasked axes), scale snap, rotate 1 px = 1°.
10. Keyboard nudges per view plane, repeat timing, rotation key sign rules (incl. sgn(0)).
11. Box selection by origin; Shift range selection by id; next/previous wrap quirks.
12. Clipboard: object paste offset and blueprint restore; voxel copy/paste merge and clipping.
13. Undo: 11-entry cap, redo cache creation, labels, re-selection via `is`, id renumbering.
14. Voxel: index↔world both mappings, box half-openness, AO factors, flips/rotations for R = 16 and R = 8 (shift
    quirk), up/downscale, hollow/fill.
15. Voxel tools: each tool's footprint for fixed hits and normals, preview rollback, laser index-0 quirk.
16. Voxel→element conversions: greedy box decomposition and face-capture orientation on a two-colour model.
17. Face optimisation and culling on canonical layouts (touching cubes, cubes on block boundaries, protruding
    cubes).
18. Paint: disc/square footprints, flood fill tolerance, line/rectangle preview restore, atlas clip rect.
19. Texture commands: label rule (sanitising, clash, same label, empty result), new-name sequence up to
    `texture100`, rename face rewrite before validation, list presets, remove-unused keeping index 0, undo timing.
20. Image loading: DDS header rules (DXT5 only for `5`), TGA types 2/10 and depths 24/32, width-only slot
    downscale, `.mcmeta` bottom crop, PNG name suffix rule, weight text (`1536` → `1KB`).
21. Primitive meshes: vertex/triangle counts, face tables (6.6), front-face rule, paint texture of a restored
    primitive at the same texels.
22. UV view: move/resize snap and clamp (mirrored included), arrow step versus content scale, double-click
    square, write-back corners, UV snap cleared.
23. Palette image: side for N = 1, 16, 17, 64, 65; cell rects; label clash replacing an earlier palette.
24. Rotation grids: mesh keyboard nudges (0 → 20 → 40, 5 → 30), 22.5 loading as 20 on meshes, 358 → 360.

---

## 18. Unverified / open questions

1. Unity f32 text formatting digits (exact output of 7-significant-digit formatting for values such as 0.125,
   22.5, 0.3333333) — affects only displayed text.
2. The engine's float-to-8-bit colour conversion (truncation assumed) for element/tool colours and hex strings.
3. Fixed-step rate (assumed 50 Hz) → arrow auto-repeat delay (> 15 fixed steps).
4. Model Properties OK handler: decompiled logic clears the restriction flag when it was already on; needs a
   runtime check before reproducing.
5. (Resolved: object flag values per kind are in 3.1.)
6. "Apply Default UV" (UV submenu) and "Paste → Tint" are wired to handler names that do not exist: assumed
   no-ops.
7. Projection tools: the final Faces-row write-back is believed to revert the copied texture labels; confirm.
8. Atlas build with rotated faces: crops are mirrored but not rotated while the face keeps its rotation; visual
   result unconfirmed.
9. Transparency predicate threshold for "Sort transparent elements" (any alpha < 1 assumed).
10. (Resolved: select-by-name glob semantics, 8.1.)
11. Releasing a model-box handle outside every view (handle stays armed until the cursor re-enters).
12. (Resolved: one exclusive group for element, voxel, paint and picker toggles, 8.10.)
13. (Resolved: the blueprint is on the model layer and pickable in every view, 9.7.)
14. Voxel up/downscale restore the *setup-time* model box corners, not later edits.
15. (Resolved: scale-handle positions, 9.7.)
16. Unity's rectangle packer output (power-of-two sizing, ordering) and its behaviour when the entries exceed the
    device limit — only rendering parity is targeted.
17. Mono `(int)` casts of negative world coordinates in the truncated world→index mapping (toward zero) for hits
    just outside the grid.
18. (Resolved: hidden objects kept active by the preference are on a 3D-only layer and not pickable, 9.7.)
19. Textures: the engine decoder's pixel format for PNG/JPG; DDS files with surplus or missing bytes; the content
    of "blank" TGA results; the exact blend of the grey-cleared downscale; native versus in-app file dialog
    differences; the wiring of texture-row pointer events; the consumer of the texture list dropdown; the menu
    enable rules; how the original reports a deleted external-edit temp file; overwrite prompts of the save
    dialog; the text encoding the original runtime uses for signatures; whether UI-entered labels are the only
    ones sanitised (importers).
20. Primitive geometry: the sphere's vertex lattice and UV layout; the last digits of the cylinder and cone cap
    and side UV fits; the torus normal construction.
21. Gizmos: whether the overlay group ignores depth; whether zero-thickness plane boxes are hit reliably; the
    visible look of the partly transparent ring and plane colours.
22. UV view: raw wheel units per notch; the canvas scale factor (affects the arrow step and the double-click
    square); the scroll view state after releasing PanView mid-drag; OS key repeat for the arrows.
23. Tool cursor: the canvas scale at start-up; whether the colour selectors report a change at start-up (custom
    tint); the fixed-step rate (tint update rate).
24. Tool toggles: whether the model-box outline shows at start while its toggle is off; whether wheel-written size
    text can exceed the character limit; paint behaviour for radius 0 and negative sizes.
25. Voxelization: how soon chunk rebuilding resumes after an aborted run; visibility of the progress dialog's
    buttons after an abort; undo of an aborted run.
26. Palette conversion: whether the UI allows palette edits while a conversion runs.
