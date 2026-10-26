# Review: A3 Animate mode, timeline and keyframe editing

Reviewer: Opus, 2026-10-26. Scope: docs/design/animate_a3.md as built. The core, ops, model and scene were read
by the reviewer, the UI and the app wiring by a second Opus reader. Gates and the seven private oracles were run
again after the fixes.

Verdict: passes after the fixes below.

## Checked

- Auto-key: a gizmo drag in Animate never touches the rest transforms. It keys the channel at the playhead
  rounded to the frame grid; one `ANIMATION` snapshot is pushed on the first changed frame ("Key position",
  "Key rotation", "Key scale"). Move goes through the inverse of the parent's posed frame and the Z mirror,
  rotate adds (-d, d, -d) in the project's convention where Model adds (d, -d, -d), scale multiplies the channel
  value. Locked and hidden bones are not keyed.
- The pose of the core: `Core::set_animating` keeps a `Rig` and a `Pose`, refreshed after commands, undo, redo,
  document swaps and every drag frame.
- Picking: carried objects are tested in rest space through the inverse motion (the ray parameter is kept,
  normals go through the inverse transpose); the BVH keeps the rest boxes and skips them; box selection and
  framing use the posed boxes.
- Commands: `KeyValue` (the old `[1, 2, 3]` JSON still reads; empty text or text over 1 KiB is refused),
  `SetKeyframeInterp`, `DuplicateAnimation` (bounds of keyframes and of the kept JSON), `CopyKeyframes` and
  `PasteKeyframes` (whole keyframes, relative times, a landing key is replaced, bounds).
- Private data: the fixtures are synthetic; no sample content is in the repository.
- Line endings: no changed file has a CR byte. The agent's note that the tree is CRLF is wrong (`core.autocrlf`
  is on, `.gitattributes` keeps LF).
- Screenshot `editor_animate_mocha_100_1920.png`: the tab, the timeline in frames with the playhead, one mark
  shape per interpolation, the Animations panel and the Animate tab.

## Accepted deviations of the agent

Unselected marks are hollow (the contrast is tested on the fills of 24 theme variants); a move snaps its delta,
not the position; the scale box stands in the posed axes of the bone; "Add an animation to key the pose" is a
core status message; posed picking is used in Animate only; F frames what a selected group holds (in Model
too, where it framed nothing); a press on the ruler seeks at once; text that is a number becomes a number.

## Fixed in review

1. The Time field of the Animate tab sent a move on every drag frame and shifted the selection before the
   timeline checked it against the view, which did not have the move yet: the selection was lost after one
   step, the drag gesture stayed open, and a key dragged across another one replaced it. The field now sends
   one move when the edit ends. The timeline does not check its selection against a view of the revision a
   move was sent with (egui may draw a frame twice), which covers the release of a mark drag as well.
2. A new document reset the revision of the timeline view to 0, so the timeline could keep the rows of the
   old document and index past the bones of the new one (a panic). The revision now keeps growing.
3. Keyframes of an animation that was deleted, or whose creation was undone, stayed selected and took Delete,
   Ctrl+C and Ctrl+V from the objects. The selection is cleared when there is no animation.
4. The timeline height was saved on any click while the window clamped the panel; a clamped height is not
   saved.
5. A drag of a pose field while playing keyed a trail of frames; the player now pauses on a frame first.
6. A rename asked for with no animation chosen took the focus of the next one; it is dropped.
7. Outside A3, found by the gates: the A1 property `a_tree_of_rotated_groups_survives_read_write_read` failed
   on a chain of two rotated groups. The Z rotation of the element read 0.0001 after one save and 0.0002 after
   the next, so a file drifted on every save. The importer composed the chain in f32 and from the angles of the
   JSON, while the writer undid it from the stored angles. `mc::Rigid` and the conversions of `place`,
   `place_group` and `unplace` now run in f64, and the importer composes from the stored group angles
   (`Rigid::about_stored`), so the writer undoes what the reader baked. The failing case stays in
   `tests/groups.proptest-regressions`; 30 000 further random cases passed.
8. Outside A3: four test fixtures of cuboid-app held their temporary folder in the first field, so it was
   dropped before the app, whose IO thread then wrote its last files (`ui.json`, settings, recovery) and made
   the folder again: every test run left about a dozen folders in the system temp folder. The folder is now the
   last field, and a run leaves none.

## Decisions

- F2 in Animate renames the animation (design 8); a bone is renamed from the outliner there.
- The A2 limits stand: `rotation_global`, quaternion interpolation and the `override` flag are kept, not played.

## Known limits

- While an animation plays in Animate, the picking index finds the carried objects again on every frame
  (about 1 ms for 5 000 elements).
- Undoing a move leaves the selection at the times the keys moved to; if the move had replaced a key there,
  the selection lands on it.
- The rotation signs are tested for drags on one axis. A drag on a bone with rest angles on several axes adds
  to the Euler components, as Blockbench does.

## Process

The agent used `cat >>` once (a probe test, replaced at once) and `sed -i` once (two type paths); both diffs
are small and correct.
