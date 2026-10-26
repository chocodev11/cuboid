# A2: keyframe animation core, `.bbmodel` animations and playback

Status: 2026-10-26, refines `anim_texture_spec.md` 4 for the code after A1 (6d9c946). A2 makes animations
real data that is read, written, evaluated and played back; A3 adds the Animate mode with the timeline and
keyframe editing by hand. Owner decision D-N1: `.bbmodel` animations first (ModelEngine-style plugins read
them), then Bedrock and GeckoLib.

## 1. Today

- Groups are real (A1). `.bbmodel` animations, controllers and placeholders are kept as raw JSON in
  `Document.retained` and written back verbatim; nothing can read or play them.
- `Categories` is a `u8` and all eight bits are taken.

## 2. Data (cuboid-model)

- `Animation { id, name, length, loop_mode: Once | Loop | Hold, snapping (fps), override, extra }` where
  `extra` keeps the `.bbmodel` fields the model does not use (for example `anim_time_update`,
  `blend_weight`, `start_delay`, `loop_delay`) as JSON, written back unchanged.
- `BoneTrack { group: GroupId, rotation, position, scale: Vec<Keyframe> }` per animated group.
- `Keyframe { time, value: [Value; 3], post: Option<[Value; 3]>, interp, bezier: Option<Handles>, uuid }`.
  `Value` is a number or a Molang text kept as written; `post` holds the second data point of a
  discontinuous key; `interp` is Linear, CatmullRom, Bezier or Step.
- Animators that are not bones of the model (unknown uuids, `effects` with sounds, particles and timeline
  markers) are kept as raw JSON on the animation and written back unchanged.
- `Document.animations` with stable ids. Undo: `Categories` becomes a `u16` and gains `ANIMATION` (bit 256);
  the existing bits keep their values.
- Bounds: at most 4 096 animations, 100 000 keyframes in a document, length at most 3 600 s, the 64 MiB cap
  of the kept JSON stays.

## 3. Evaluation (pure, in cuboid-model or a small new module)

- `sample(track, t)` gives the channel value at time `t`: linear, Catmull-Rom (the neighbours of the segment,
  endpoints repeated), Bezier (the handles, solved for time), Step, with `post` values on the far side of a
  discontinuous key; a Molang value counts as the last numeric value before it (0 when none).
- `pose(doc, animation, t)` gives, per group, the motion that takes the rest pose to the pose at `t`:
  position offset in model units (1/16 block), rotation in degrees added to the rest rotation, scale as a
  factor about the group pivot, composed down the hierarchy. Rotation order and signs follow the `.bbmodel`
  convention the importer already uses for groups (`Rigid::about`); the Blockbench clone may be read to learn
  the format, never copied.
- Loop modes: Loop wraps `t`; Once returns to the rest pose after the end; Hold keeps the last pose.
- Tests: hand-computed values for every interpolation, discontinuous keys, Molang fallback, loop modes,
  a 3-level bone chain, and bounds.

## 4. Commands (cuboid-core), each one undo step in ANIMATION

`AddAnimation`, `RemoveAnimation`, `SetAnimation` (name, length, loop mode, snapping), `SetKeyframe` (insert
or replace at a time for a group and channel), `RemoveKeyframes`, `MoveKeyframes` (shift times, merge on
equal times). They are the API A3's timeline uses; A2 needs them for the tests and for the importers to stay
honest. Deleting a group removes its tracks in the same step.

## 5. Formats

- `.bbmodel` read: `animations` become `Animation`s (bone animators by group uuid); unknown animators,
  `effects` and the unused fields go to `extra`; `animation_controllers` and placeholders stay verbatim in the
  kept JSON (not modelled in A2).
- `.bbmodel` write: the animations from the data, with the original uuids, plus `extra`; an unchanged file
  writes the same `animations` as JSON values (property test).
- `.cubik`: animations in the `cuboid.json` entry, keyed by the hierarchy refs of A1, bounded; the payload
  stays untouched.
- Other formats: the existing note that animations are not written.

## 6. Playback

- The scene takes an optional pose (per-object world transform overrides) computed from `pose()` at the
  playhead; the extract writes changed instance records only (the existing `InstanceUpdate` path), meshes
  likewise; the document never changes during playback.
- An "Animations" panel (dockable, kit components): the list of animations with length and loop mode, play,
  pause, stop, a time scrubber with the playhead time, loop toggle and speed. Repaint only while playing.
- While a pose is shown, picking and the gizmo use the rest pose; the viewport header says "Previewing
  <name>" and Stop returns to the rest pose. Editing by hand is A3.

## 7. Tests and gates

Evaluation tests of 3; one scenario test per command of 4 with undo and redo; the `.bbmodel` property test on
fixtures made in the tests (several bones, every interpolation, Molang values, discontinuous keys, an
`effects` animator, unknown fields); `.cubik` round trip of animations; playback tests in cuboid-app
(headless: the pose reaches the instance records, Stop restores the rest pose, no document change); the
private oracles stay green (the two private `.bbmodel` samples import and export).
