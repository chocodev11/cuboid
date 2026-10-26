# A3: the Animate mode, the timeline and keyframe editing

Status: 2026-10-26, refines `anim_texture_spec.md` 4.3 for the code after A2 (f4de281). A2 made animations data
that is read, written, evaluated and played; A3 makes them editable by hand. The graph editor, onion skin, Bezier
handle editing and Molang evaluation stay out (second step of the milestone).

## 1. Today

- `Mode` has Model, Voxel, Paint and Display. The Animations panel lists animations and drives the `Player`
  (`cuboid-app/src/frontend/playback.rs`); playing shows a pose through `Extractor::set_pose`, while picking and
  the gizmo stay on the rest pose.
- Core commands exist for animations and keyframes (`AddAnimation`, `RemoveAnimation`, `SetAnimation`,
  `SetKeyframe` with numbers only, `RemoveKeyframes`, `MoveKeyframes`), all in the `ANIMATION` undo category.
- The gizmo drag (`cuboid-core/src/interact/gizmo.rs`) moves, turns and scales the rest transforms; a selection
  with a group in it moves as a whole through `cuboid_ops::groups::Pose`.

## 2. The Animate mode

- `Mode::Animate`, last in tab order (Model, Voxel, Paint, Display, Animate); Tab cycles through it and
  Ctrl+Alt+5 opens it. Its tools are Select, Move, Rotate and Scale; their keys of each preset work in Animate as
  in Model.
- Entering Animate selects the first animation when none is chosen and shows its pose at the playhead. Leaving
  Animate pauses and shows the rest pose; the chosen animation and the playhead stay for the return. The A2
  preview from the Animations panel keeps working in the other modes as it does today.
- `arrange_for(Animate)` brings the Animations panel forward and opens the Animate tab of the properties (4).
  The timeline (3) shows below the views in this mode only.
- Bones: in Animate a click on an object selects its innermost group, and a box selection selects the groups of
  the objects it catches. An object outside every group cannot be animated; clicking one clears the selection and
  the status bar says "Only groups are animated: put the object into a group". A model without groups shows that
  sentence in the timeline. Voxel models stay static (one object, no bones).
- Rest editing stays possible in Animate (the other property tabs, menus, undo); nothing blocks it.

## 3. The timeline

A bottom strip of the centre column, between the left and right areas, shown in Animate only: resizable, default
220 points, at least 120, at most 60 % of the window; the height is a preference.

- Header row: play or pause, stop, loop, the playhead time as a drag field ("1.25 s", frame number beside it at
  the animation's frame rate), and a fit button that fits the length of the animation to the width.
- Left column (200 points): one row per bone with a disclosure arrow, its name indented by depth, then one row
  per channel (Rotation, Position, Scale) with a "+" button that keys the channel at the playhead with its value
  there. The rows are the bones that have a track in the animation plus the selected bones, in outliner order.
  Bones start expanded; the collapsed set is UI state, not saved. A collapsed bone shows one summary row with the
  keyframes of all its channels.
- Time area: a ruler with labels whose step follows the zoom (frames once a frame is at least 6 points wide), the
  part after the length shaded, the playhead as an accent line across all rows, and keyframes as marks: diamond
  for linear, circle for Catmull-Rom, square for step, a diamond with a bar for Bezier. A keyframe with an
  expression value has a warning outline; one with a jump (`post`) is drawn split. Selected marks use the accent.
  Colours come from the theme tokens, with the contrast rules of `ui_v2_spec.md` 4 (a selected mark at least 3:1
  against an unselected one).
- Interaction:
  - Click or drag on the ruler scrubs: the playhead moves and the pose follows. Times snap to the animation's
    frame grid (`snapping`, frames per second); Alt held turns snapping off.
  - Click a mark selects it; Ctrl toggles, Shift adds. A click on empty space clears the selection, a drag there
    box-selects across rows and time.
  - Dragging selected marks retimes them: the marks follow the pointer, snapped, and one `MoveKeyframes` runs on
    release (one undo step). A move the command refuses (out of range) snaps back.
  - Double-click on a mark puts the playhead on it.
  - Right-click on a mark: Interpolation (Linear, Catmull-Rom, Bezier, Step), Copy, Paste, Delete, Go to keyframe.
  - Wheel scrolls the rows, Shift+wheel scrolls time, Ctrl+wheel zooms time about the pointer (10 to 2 000 points
    per second), middle drag pans both.
- Selection identity: the timeline keeps selected keyframes as (bone, channel, time). The app turns them into
  positions when it runs a command; after a move the selection shifts by the delta; keys that are gone drop out.
- Cost: only visible rows are built (as the outliner does); per channel only the keys inside the visible time
  range are drawn (`partition_point` on the times), and marks closer than 2 points merge into one, so a row draws
  at most its width in points over 2 marks. The row data is rebuilt only when the animations, the groups or the
  bone selection change, never per frame. No repaint while idle; while playing the repaint is the player's.

## 4. Editing values: the Animate tab of the properties

- Pose section, for the primary bone: Rotation (degrees added), Position (model units added) and Scale (factor)
  as the channels say at the playhead. Editing a field keys that channel at the playhead; a drag of a field is one
  undo step (the `commit` flag pattern of `Edit::Transform`).
- Keyframe section, when one keyframe is selected: time (drag field, moves it), X, Y and Z as text fields that
  take a number or a Molang expression (kept as written; playing uses the last number before it, as in A2),
  interpolation. With several selected: interpolation only, applied to all. A keyframe with a jump shows "This
  keyframe jumps; a new value removes the jump" (`SetKeyframe` drops `post`, as today).

## 5. Auto-key and the posed gizmo (cuboid-core)

- The app tells the core what Animate shows: the chosen animation and the playhead time, or nothing outside
  Animate. The core computes the pose it needs itself (a `Rig` and a `Pose` kept between calls, rebuilt when the
  groups, the animations or the time changed), so the core stays independent of the player.
- In Animate the gizmo of a bone sits on the posed pivot with the posed rotation; nothing shows without a chosen
  animation, and the status bar says "Add an animation to key the pose".
- A gizmo drag on a bone in Animate never touches the rest transforms. It writes keyframes at the playhead,
  updated every frame of the drag and recorded as one undo step in `ANIMATION` only:
  - Move: the pointer motion in the world, brought into the posed frame of the parent bone (its inverse rotation
    and scale) and mirrored into project space, is added to the position the channel has at the playhead.
  - Rotate: the degrees of the drag on the masked axes are added to the rotation the channel has at the
    playhead, in the project's sign convention. The test that fixes the signs: on a bone without rest rotation,
    under a parent without rotation, a rotate drag in Animate gives the same posed world rotation as the same
    drag gives the rest pose in Model.
  - Scale: the channel value at the playhead times the drag factor, masked axes only.
  - A channel value that is an expression counts as its last number (A2 rule); keying replaces it with a number.
- Keys land on the frame grid: the playhead time rounded to `1 / snapping`. Pause leaves the playhead on the
  nearest frame, so the pose shown is the pose keyed. A drag in the views or in the timeline pauses the player.
- Restricted rotations (Cubik's project rule for the rest pose) do not apply to keyframes. Locked or hidden bones
  are not keyed, as they are not dragged.

## 6. Picking in the pose (cuboid-scene)

- `SceneIndex` takes the pose shown (or none) and the bone of every object. In Animate a ray tests an object of
  a moving bone in rest space, through the inverse of the bone's motion; the hit point and distance come back in
  the world. Objects whose bones do not move go through the BVH as today; the BVH is not rebuilt per frame.
- Hover, click, box selection and framing (F) use the posed boxes in Animate. Selection outlines, the hover
  highlight and the gizmo draw at the pose; check that the outline follows `set_pose` and fix it if not.

## 7. Commands (cuboid-core)

- `SetKeyframe` takes `value: [KeyValue; 3]`, a number or a text per axis (serde untagged: `[1, 2, 3]` still
  reads). A text that is a number is a number; any other text is kept as an expression.
- `SetKeyframeInterp { animation, keys, interp }`: one undo step for many keys.
- `DuplicateAnimation { animation }`: a deep copy inserted after it, named `<name>_copy`, without uuids.
- `CopyKeyframes { animation, keys }` puts full keyframes (values, post, interpolation, handles) on a keyframe
  clipboard beside the other clipboards of `clipboard.rs`; no undo step. `PasteKeyframes { animation, time,
  group }` inserts them with their relative times, the first at `time`, into their own bones, or into `group`
  when they all come from one bone; keys that land on keys replace them; one undo step.
- Each new command: serde with docs, undo label, guard, validation of finite and in-range values, and the bounds
  of A2 (100 000 keyframes, one hour).

## 8. Animations panel and the player

- The list gets New (named `animation`, then `animation2`, ...; 1 s, 24 frames per second, Once), Duplicate,
  Rename (F2 or double-click) and Delete (undoable, no question). Below it, the fields of the chosen animation:
  name, length, loop mode, frame rate (`SetAnimation`, a drag is one undo step). The empty state offers New
  instead of the A2 text.
- Selecting an animation shows its pose at the playhead, clamped to its length; a new animation starts at 0
  (decision of the A2 review).
- Player requests (`AnimationRequest`) stay free of document changes; document edits from the timeline and the
  panel go as a new `UiRequest` variant. `cuboid-ui` gets plain view data, as `AnimationsView` does
  (`cargo xtask layering`).

## 9. Shortcuts (Animate only)

| Action | Blender, Blockbench presets | Cubik preset |
| --- | --- | --- |
| Play or pause | Space, unless the navigation preset is Cubik (Space pans there) | a free chord of the Cubik table |
| Previous or next frame | Left, Right | Left, Right |
| Start, end | Shift+Left, Shift+Right | Shift+Left, Shift+Right |
| Key the selected bones (all channels) | I (Blender), none (Blockbench) | none |
| Delete, copy, paste keyframes | Delete, Ctrl+C, Ctrl+V while keyframes are selected | same |

The keymap conflict test covers the new rows. Palette entries with the names of both apps: "Insert keyframe",
"Add keyframe", "Play animation", "New animation", "Animate mode".

## 10. Tests and gates

- Core: an auto-key test per gizmo kind with undo and redo (keys at the playhead, the rest transforms and the
  groups revision unchanged); the rotation sign test of 5; a drag between keys keys the interpolated value;
  locked bones; no animation, no gizmo; a click on an element selects its bone; each new command of 7 with undo
  and redo; `SetKeyframe` reads the old JSON.
- Scene: a ray hits the posed place of an object and misses its rest place; box selection and bounds in the pose.
- UI (headless egui): rows only for visible bones, marks bounded per row; click, Ctrl and Shift selection, box
  selection; a mark drag sends one move on release with the snapped delta; ruler scrub sends seeks; context menu
  interpolation; the Animate tab fields; one screenshot of the Animate mode in `cuboid-ui/tests/screens.rs` for
  the review.
- App: entering and leaving Animate (pose shown, rest pose after), selection previews at the playhead, keyframe
  copy and paste, duplicate, the pose fields key the channel, a drag in the views is one undo step.
- Gates: fmt, clippy `-D warnings`, the workspace tests, `cargo xtask layering`, `ledger`, `ui-style`, and the
  seven private oracles.
