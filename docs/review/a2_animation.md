# Review: A2 keyframe animation, formats and playback

Reviewer: Opus, 2026-10-26. Scope: docs/design/animation_a2.md as built. Gates and the seven private oracles
were run again after the agent.

Verdict: passes.

## Checked

- Undo: `Categories` is a `u16`; the old bits keep their values, `ANIMATION` is bit 256. Delete, cut and
  ungroup snapshot `ANIMATION` too, so a group's tracks go in the same step; a restore prunes tracks of groups
  that are gone; merge leaves animations out like the hierarchy.
- Formats: `.bbmodel` animations are data now and an unchanged file writes the same `animations` JSON
  values (fixture and a 200-case proptest); controllers and placeholders stay verbatim. `.cubik` keeps the
  payload untouched; a too-large extension entry fails the save instead of dropping data.
- Playback: `Extractor::set_pose` rewrites only records that differ; picking and the gizmo stay on the rest
  pose; the document never changes during playback.
- Private data: the test fixtures are synthetic (bone names such as root, arm, tail); no sample content is in
  the repository.
- Process: two small shell edits (`sed -i` in `fmt-cubik/src/tree.rs`, `cat >>` in `ui/tests/panels.rs`)
  instead of the Edit tool; both diffs are correct.

## Decisions on the open questions

- A3 lets `SetKeyframe` take Molang text as well as numbers (kept as written, evaluated as today).
- Selecting an animation in the panel previews it at the playhead (t = 0 when new), as Blockbench does.
- No full matrix in the instance record for now: shear under non-uniform bone scale is rare; revisit when a
  user model needs it (it touches the renderer shader).

## Known limits

`rotation_global`, quaternion interpolation, loop-wrapped Catmull-Rom neighbours and the `override` flag are
kept and written back but not played. Rest Euler angles are recovered from the stored group rotation, so
unusual rest angles can differ from Blockbench while animating. One `Vec` per `RenderExtract` is allocated per
frame, as before A2.
