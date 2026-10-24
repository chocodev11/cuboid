# Animation and Minecraft texture tools (spec for the next round)

Status: draft 2026-10-24, not reviewed, not scheduled. Owner decisions are in 8. It extends
`docs/design/roadmap_lean.md` (whose priorities still apply: stability, then performance and RAM, then features)
and moves the animation item of its cut list into a planned milestone.

## 1. What exists today

| Area | State |
|---|---|
| Hierarchy | none: the model is a flat list, like Cubik; `.bbmodel` import flattens groups and bakes their transforms; Group, Ungroup and Reparent return `NotYet` |
| Animation | none |
| Texture painting | pencil, eraser, fill with a limit, picker, brush size, alpha; only touched regions update |
| UV | per-face rectangles with drag in the UV editor, auto UV (box and per-face), rotate, flip, numeric rectangle, UV snap |
| Minecraft textures | no `.mcmeta` handling, no layers, no selection or shape tools, no vanilla texture library |

## 2. Who it is for

- Resource-pack authors: animated textures (`.mcmeta`), item states through item definitions (already planned in
  `agent_toolkit.md` 6.5), clean pixel-art tools.
- Server developers (Paper): animated models driven by plugins that read `.bbmodel` files with animations (for
  example ModelEngine) or by display-entity exporters (Animated Java style).
- Mod and Bedrock authors: Bedrock and GeckoLib geometry and animation JSON.

## 3. Hierarchy (prerequisite for animation)

- Model: `Group { id, name, parent, pivot, rotation, visible, locked, export }`; every element and mesh gets an
  optional parent. Ids follow the W1.5 rule (stable for the life of a document, never reused).
- Commands: group, ungroup, reparent, set pivot, group transform (move, rotate, scale with the children), each one
  undo step; undo snapshots gain a hierarchy category (W1.6 categories).
- Outliner: real tree with drag to reparent (the UI already draws drop hints), collapse state, visibility and lock
  that propagate to children.
- Scene: world matrices composed from the hierarchy in the extract; picking and gizmos work in world space; only the
  changed subtree is updated.
- `.cubik`: the payload stays Cubik-compatible. Elements keep their baked world transforms, so Cubik shows the same
  model. The hierarchy goes into the extra ZIP entry `cuboid.json`, which Cubik ignores (W1.7 review: native 2.2
  item 4 and 8.4). `cuboid.json` stores a hash of the payload; if Cubik later rewrote the payload, the sidecar is
  treated as stale: cuboid re-validates it against the elements, keeps what still matches and tells the user.
- `.bbmodel`: groups are read and written as an outliner tree (no more flattening).

## 4. Animation

### 4.1 Data

- `Animation { id, name, length, loop: Once | Loop | Hold, fps_snap, override }`.
- Per bone (group) channels: rotation, position, scale. `Keyframe { time, value: [f32; 3], interp }` with
  `interp = Linear | CatmullRom | Bezier { in, out } | Step`.
- Molang expressions from imported files are kept as text, shown read-only and exported unchanged; v1 does not
  evaluate them (the bone uses the last numeric value and the timeline marks the key).
- Sound, particle and timeline-marker keys are out of v1; imported ones are kept and exported unchanged.

### 4.2 Evaluation

- `pose(animation, t) -> local transforms` is a pure function: rest pose plus channel values, rotations composed in
  the order the target format uses (recorded per format profile), deterministic.
- Tests: hand-computed values for every interpolation (Catmull-Rom end conditions, Bezier handles, step), loop and
  hold behaviour, many bones.
- Cost per frame: bones x channels x log(keys); playback updates only instance transforms (the renderer already
  updates per-instance and per-mesh transforms without re-uploading geometry or textures).

### 4.3 Editing

- New mode tab Animate (beside Model, Voxel, Paint, Display).
- Timeline: one row per bone and channel, keyframe markers, drag to retime, box select, copy, paste, delete, snap
  to the fps grid; play, pause, loop, scrub, speed; length and loop settings.
- Auto-key: in Animate mode, gizmo and numeric transforms of a bone write keys at the playhead (one undo step per
  drag).
- Graph editor (curves) and onion skin: second step of the milestone.
- Voxel worlds stay static in v1 (one object, no bones); documented in the UI.

### 4.4 Storage and exchange

- `.cubik`: animations in `cuboid.json` (same staleness rule as 3).
- `.bbmodel`: animations read and written (animators per bone, keyframe channels, interpolation, loop, length).
- Bedrock `.animation.json` and GeckoLib (same JSON family) export; import if time allows.
- Display-entity datapack export (Animated Java style): later; needs its own spec because of the server
  constraints (interpolation ticks, transformation limits, entity counts).

### 4.5 Validation by target profile

Profiles such as "ModelEngine (.bbmodel)", "GeckoLib", "Bedrock" declare bone count limits, allowed channels,
rotation order and unsupported features; `validate` (agent_toolkit 6.6) reports breaches with fixes.

## 5. Minecraft texture tools

### 5.1 Animated textures (`.mcmeta`)

- Read a vertical frame strip with its `.mcmeta` (frametime, interpolate, frames list with indices and per-frame
  times, width and height).
- Flipbook panel: add, remove, duplicate and reorder frames, per-frame time, onion skin, playback in the viewport
  and in the UV editor.
- Export: PNG strip plus `.mcmeta` in resource packs and `.bbmodel`.
- `.cubik`: the RawTexture keeps one frame for Cubik; the full strip and its timing go into extra ZIP entries
  (`cuboid/tex/<id>.png`) described in `cuboid.json`.

### 5.2 Paint tools, second set

Selection (rectangle, magic wand) with move, copy and paste; line, rectangle and ellipse; gradient with an
optional ordered dither for pixel art; colour replace; shade brush (lighten or darken one palette step); noise
brush with a seed; mirror painting on the model's X, Y and Z; alpha lock; pixel-perfect lines; painting across
neighbouring faces on the model. Hard pixels only (no soft brushes).

### 5.3 Layers

Per-texture layers with opacity, visibility, lock and the blend modes normal, multiply, add and overlay; merge down
and flatten. Saved in the sidecar; `.cubik`, resource packs and exports receive the flattened image.

### 5.4 Texture operations

Resize (nearest), crop, flip and rotate, hue, saturation, brightness and contrast, invert, quantize to a palette,
import and export PNG, duplicate.

### 5.5 UV editor, second set

Multi-face selection, rotate and mirror handles, pixel snapping, UV islands drawn over the texture, texel-density
readout, and "generate template": a box-UV layout texture with one colour per face, as entity templates use.

### 5.6 Minecraft specifics

- Resource-pack checks: square frames for animated textures, sizes, missing textures, transparency on cutout
  faces (with `validate`).
- Tint-index preview with the vanilla grass and foliage colour maps when the user's game files are available.
- Vanilla texture library: the user points cuboid at their own Minecraft client jar or a resource-pack folder;
  cuboid browses its textures read-only and never copies them into projects it exports unless the user does so
  explicitly (Mojang's assets are not redistributable).
- Palettes: import and export `.gpl` and `.hex`, a project palette, a palette taken from a texture.

## 6. Formats touched

| Format | Change |
|---|---|
| `.cubik` | payload unchanged; new ZIP entries `cuboid.json` and `cuboid/tex/*.png` for hierarchy, animations, layers and frame strips |
| `.bbmodel` | groups, animations, frame strips with timing, layers where the format version supports them |
| Resource pack | textures with `.mcmeta`; item definitions as in agent_toolkit 6.5 |
| Bedrock, GeckoLib | geometry with bones; animation JSON export |

## 7. Work packages

| WP | Scope | Needs |
|---|---|---|
| A1 Hierarchy | model, commands, undo category, outliner tree, scene world matrices, `cuboid.json` sidecar, `.bbmodel` groups | - |
| A2 Animation core | data, evaluation, commands, undo, storage | A1 |
| A3 Animate mode | timeline, playback, auto-key | A2 |
| A4 Animation formats | `.bbmodel` animations, Bedrock and GeckoLib export, profiles | A2 |
| T1 Animated textures | `.mcmeta` read and write, flipbook panel, storage | - |
| T2 Paint tools | 5.2 | - |
| T3 Layers and operations | 5.3, 5.4 | T2 |
| T4 UV editor and templates | 5.5 | - |
| T5 Minecraft library and palettes | 5.6 | T1 |

A1 runs first; T1, T2 and T4 run beside A1 and A2; A3 and A4 follow A2. Opus reviews (high risk): A1 (data model and
file compatibility), A2 (evaluation), T1 (format). Every package: automated gates of roadmap_lean 5.

## 8. Owner decisions

| Id | Question | Recommendation |
|---|---|---|
| D-N1 | Main animation target | decided 2026-10-24 by the owner: `.bbmodel` with animations first (plugins such as ModelEngine read it), then GeckoLib/Bedrock; display-entity datapacks later |
| D-N2 | Where the hierarchy lives in `.cubik` files | the `cuboid.json` sidecar entry, with baked transforms in the payload |
| D-N3 | Vanilla texture library | read-only from the user's own game files, never bundled |
| D-N4 | Animating voxel models | not in this milestone |
