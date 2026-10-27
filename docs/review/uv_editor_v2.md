# Review: UV editor v2

Status: 2026-10-27. Reviewer: Opus. Scope: docs/design/uv_editor_v2.md as built in the working tree: the editor
(`crates/cuboid-ui/src/uv.rs`), `MoveFaceUv` and `uv::move_faces`, the mapping in the app, the grid preference, the
tests and the three review screenshots. The other work in the tree (README, brand assets, logo and mode layout
designs) is not part of this review. fmt, clippy `-D warnings` on cuboid-ui, cuboid-app, cuboid-core and cuboid-ops,
`xtask layering`, `ui-style` and `ledger`, and the focused UI, app, core and ops tests were run and pass; the full
workspace tests and the private oracles were not run for this review.

Verdict: not yet. Two major findings need fixing: one click of a toolbar action on several faces is several undo
steps, and a move with several elements selected does not follow the pointer and moves faces the editor does not
show. The minor findings are small and fit in the same round.

## Major

1. `crates/cuboid-ui/src/uv.rs:448` Auto UV, Rotate, Mirror and Flip send one `Edit::Face` per chosen face, and the
   app ends a gesture after each one (`crates/cuboid-app/src/requests.rs:195`, `:522`), so with three faces chosen a
   click on Rotate is three "Rotate UV" steps and Ctrl+Z turns back one face at a time. Fix: one request that names
   the faces, for example `Edit::Faces { faces: Vec<Face>, change: FaceChange }`, mapped in `requests.rs` to one
   `AutoUv`, `RotateUv` or `FlipUv` whose `faces` lists them all; the commands already take a list. Test: two chosen
   faces and one click send one request (UI), and it is one undo step (app).

2. `crates/cuboid-app/src/requests.rs:396` With two elements selected, the move goes to both (`targets` empty is the
   selection), but the editor draws only the primary and bounds the drag with the primary's faces alone
   (`uv.rs:685`). The core then cuts each step down to what keeps the faces of every selected element on the texture
   (`crates/cuboid-ops/src/uv/mod.rs:141`), while the app records the full requested total (`requests.rs:404`).
   Scenario: primary A has North at u 4 to 8, B has North at u 10 to 14 on a 16 texel texture; dragging North 6
   texels right moves A by 2, the face stops short of the pointer for no visible reason, and dragging back to +3 sends
   a step of -3, so A ends 1 texel left of its start while the pointer is 3 right of it. If B's North uses another
   texture, it moves by the primary's texel size converted to its own, a different number of its texels, out of
   sight. The Properties panel also tells the user "edits apply to the primary one" (`properties.rs:119`). Fix: the
   app names the primary in `targets` for `MoveFaceUv` (the editor's bound then equals the core's, so nothing
   drifts). Moving every selected element, as Blockbench does, would need the editor to draw their faces and bound
   the drag with them; that is a later round. Test: an app test with two selected elements where the second one
   would hit the edge first.

## Minor

3. `crates/cuboid-app/src/frontend.rs:113` The move keeps state across requests that can go stale. `uv_moved` is
   reset only by a committed `MoveFaces`, and `UvState::drag` only by `drag_stopped`. If the editor is not drawn when
   the button comes up (F8 pressed mid-drag closes the panel, or Alt+A in the Blender preset clears the selection,
   which takes the early return at `uv.rs:291`), no commit is sent: the next arrow key computes `1 - total` and moves
   the faces the wrong way inside the old undo step, and because `hovered` includes `drag.is_some()` (`uv.rs:337`),
   the arrow keys and + and - go to the editor from anywhere until the next press in the view. The arrow keys also
   work during a drag (`uv.rs:554`): Right while dragging 3 texels sends a committed total of 1, so the faces jump
   back 2 until the release, and the drag ends as two undo steps. Fix: let the editor send the step since its last
   request (it keeps `sent` already) and drop `Frontend::uv_moved`; skip `nudge` while `self.drag` is some; at the
   top of `show`, drop a drag when no button is down and send its commit if it sent anything. At `requests.rs:515`,
   a commit whose command maps to nothing should still advance `gesture`.

4. `crates/cuboid-ui/src/ui.rs:528` Keys the editor owns still reach the keymap first. `handle_keys` runs before the
   panels and keeps only + and - from the keymap while the editor is hovered, so in Animate, with the UV editor
   open, Left and Right (and with Shift) step the playhead (`keymap.rs:339`) and never move the faces. In the Cubik
   navigation, a Space and left-button pan of the UV view that runs over a 3D view also pans that view
   (`viewport.rs:381`: its Space pan needs only hover, and egui's hit test still reports the cell under the pointer
   while the UV view holds the drag). Fix: skip the arrow keys in `handle_keys` while `over_uv`, as for + and -; add
   `ui.ctx().dragged_id().is_none()` to the Space pan condition of the viewport.

5. `crates/cuboid-ui/src/uv.rs:665` The choice is decided on press, which costs two cases. A Shift press on a chosen
   face of a multi-choice takes it out, so holding Shift first and then dragging, the natural way to get the
   quarter-texel snap, drops the face and moves nothing. And `face_at` (`uv.rs:584`) prefers chosen faces, so a
   small face lying inside a larger chosen one can never be chosen by a click, only from the menu. Fix: as
   `collapse` already does, decide on release when the pointer did not travel: a Shift or Ctrl click toggles then,
   a plain click chooses the smallest face under the pointer; a drag keeps the chosen-first rule and moves the choice
   with the snap of the held keys. Test: a Shift press then drag on one of two chosen faces sends a quarter-texel
   move of both.

6. `crates/cuboid-app/src/uimodel.rs:205` `update_outlines` rebuilds the list on every revision, for every element,
   whether or not the editor is open or All UVs is on: during a gizmo drag of a 5 000 element model that is up to
   30 000 `Arc<str>` allocations and as many linear `texture_size` lookups per frame. Fix: build it only
   while All UVs is on (keep the toggle in the preferences like the grid so the app can read it, or report it in
   the UI output), and carry the texture as an index or a clone of the `Arc` in the texture list.

## Nit

7. `uv.rs:102` A whole notch lands on whole points to a texel, not whole pixels: at a UI scale of 150 % a zoom of 9
   points is 13.5 pixels and texels alternate between 13 and 14 pixels. Count the steps in `scale *
   pixels_per_point`.
8. `uv.rs:840`, `:847` The outlines of faces that are not chosen and the All UVs outlines are one faint stroke of
   `text_weak`; on a busy texture in Latte they are close to invisible (`uv_editor_latte_100_1920.png`). A dark line
   with a light one beside it reads on any texture.
9. `uv.rs:300` The second toolbar's band is allocated before the checks for a missing or empty texture, so those
   cases show an empty row above the message.
10. Spec 2 and 3 say + and - and the arrows work "while the pointer is over the editor"; they work over the view only,
    not over its toolbars or numbers. Either is fine; the spec should say which.
11. Tests: `crates/cuboid-ui/tests/panels.rs:184` now clicks whichever "180°" comes first, which may be the UV
    editor's, so the Faces tab's rotation control is no longer covered; look it up inside the properties panel.
    `panels/uv.rs:480` says "leaves it alone" where the assertion checks that the choice collapses to it.
    `panels/uv.rs:702` finds the All UVs outlines by their exact stroke colour, which pins a drawing detail. Missing:
    a multi-element move, a Shift held before the press, an arrow key during a drag, Escape during a drag.

## The reported deviations

- `targets` on `MoveFaceUv`: accepted, it matches the other UV commands; the app should fill it with the primary
  (finding 2).
- `Edit::MoveFaces` carries the total since the drag began: rejected. It needs state in the app that a missed release
  or a key during the drag leaves wrong (finding 3); the editor should send the step since its last request.
- Moves bounded to the texture rather than `[-1, 2]`, corner drags clamped: accepted. The editor shows the folded
  corners, which always lie in `[0, 1]`, so the bounds are never inverted, and a move past the edge would fold back.
- Toolbar actions as one request per face: rejected (finding 1).
- View controls in a second toolbar row, readout `2700%`: accepted; one row does not fit the 180 point area, and a
  compact readout suits a toolbar button.
- A plain click on a chosen face of a multi-choice keeps only that face: accepted, it is decided on release so a drag
  still moves the whole choice.
- Escape during a drag commits: accepted for now. egui ends any drag on Escape, so `drag_stopped` commits here as it
  ends the outliner and timeline drags. A cancel that puts the faces back is a follow-up for all of them.
- With several elements selected the core moves all of them: rejected (finding 2).
- A plain wheel zooms: accepted. The panel does not scroll, and `wheel_notches` ignores modifiers, so the Blockbench
  habit of Ctrl and the wheel zooms as well.
- Mechanical edits with `sed` and `python`: no damage found. The import lists and the `texels` helper are correct,
  fmt and clippy are clean, every changed file is UTF-8 with LF and no BOM.

## Checked and sound

- Zoom about the pointer: `View::zoomed` keeps the texel under `about`; the range is 25 % to 3 200 %, a notch from
  100 % up lands on whole points, 100 % is a stop on the way up, a touchpad's fraction is a plain factor.
- Pan: middle, right, and Space with the left button are told apart at `drag_started` before a press can grab a
  face; the clamp keeps 32 points of the texture (or all of it) in view; Fit and the readout refit, a new texture
  or size refits, another face of the same texture keeps the view.
- Grid: only the boundaries inside the visible part are drawn, fading in from 6 to 10 points, stronger every 16
  texels; the picture is one image; nothing requests a repaint while idle (only a choice change and Fit do).
- Moves: the snap (whole texels, quarters with Shift, none with Alt) applies to the total since the press; for one
  element the editor's bound equals the core's; flips, size and rotation are kept; a corner drag moves one corner on
  the texture; a drag is one "Move UV" step through the gesture, a step clamped to nothing is `Unchanged`, an arrow
  key is one step.
- Core: `MoveFaceUv` has the element guard, the finite check, its label and a JSON round trip; undo and redo, flips
  and bounds are tested.
- Preferences: a file without `uv_grid` reads as on; a toggle marks the preferences dirty and is saved.
- Keys: + and - (and =) zoom about the pointer and stay away from the 3D zoom while the editor is hovered; a focused
  field blocks the zoom and the arrows.
- egui ids: the view's id is persistent under the panel, so a drag survives changes in the widgets above; the fields
  are salted by index.
- Layering: the UI sends `Edit::MoveFaces` and the app maps it to `MoveFaceUv`; `xtask layering` passes.
- Screenshots: the zoomed view with the grid, the letters, the handles of the chosen faces, the numbers and the
  warning colour of the density in both themes at 100 %; at 150 % the face picker keeps its icon in the narrow area.

## Fixes

Status: 2026-10-27, after the review. `AutoUv`, `RotateUv` and `FlipUv` all take a list of faces, so no command changed.

1. Fixed. The toolbar sends one `Edit::Faces { faces, change }` for the chosen faces; `requests.rs` maps it (through
   `face_command`, which `Edit::Face` uses too) to one `AutoUv`, `RotateUv` or `FlipUv` naming all of them, acting on the
   selection as the Faces tab does. Tests: `a_toolbar_action_on_several_chosen_faces_is_one_request` (UI),
   `a_uv_action_on_several_faces_is_one_undo_step` (app).
2. Fixed. `MoveFaces` names the primary element in `targets`, so the editor's bound is the core's. The corner, the
   numbers and the rotation had the same flaw (`SetUvRect` with empty `targets` wrote the primary's rectangle to every
   selected element): they send the new `Edit::FaceOfPrimary`, which names the primary too; the Faces tab keeps acting on
   the selection. Tests: `a_move_from_the_uv_editor_acts_on_the_primary_element_and_follows_the_pointer`,
   `the_corner_and_the_numbers_of_the_uv_editor_change_the_primary_element_only` (app).
3. Fixed. `Frontend::uv_moved` is gone: `MoveFaces` carries the step from where the faces are in the document (the editor
   reads the first face's corner from the props and keeps the pointer's total only to know when to ask again), so a step
   the core cuts short is made up for by the next. `nudge` is skipped while a drag runs, `show` ends a drag whose button
   is up (and sends its commit) before anything else, and a commit that maps to nothing still advances `gesture`.
   Tests: `a_step_is_worked_out_against_where_the_faces_are_in_the_document`,
   `an_arrow_key_during_a_drag_leaves_the_move_to_the_pointer`,
   `a_drag_whose_release_the_editor_missed_still_ends_its_undo_step_and_lets_go_of_the_keys` (UI),
   `the_end_of_a_move_still_ends_the_undo_step_when_the_selection_is_gone` (app).
4. Fixed. `handle_keys` skips the arrow keys while the pointer is over the editor, as for + and -; the Space pan of a 3D
   view also needs `ctx.dragged_id()` to be none. Tests: the Animate test
   `the_arrow_keys_move_uv_faces_not_the_playhead_while_the_pointer_is_over_the_uv_editor`,
   `the_pan_of_the_uv_view_with_space_does_not_pan_a_3d_view_under_the_pointer`.
5. Fixed. A press on a chosen face grabs the whole choice and decides on release when the pointer did not travel: Shift
   or Ctrl toggles, a plain click chooses the smallest face under the pointer; a press on a face that is not chosen still
   chooses it at once. Tests: `shift_held_before_the_press_drags_the_choice_in_quarter_texels_and_keeps_it`,
   `a_click_takes_the_smallest_face_under_the_pointer_and_a_drag_keeps_the_chosen_one`.
6. Fixed. The editor reports All UVs through `Ui::uv_outlines_wanted()` (not kept in the preferences), the app builds
   `uv_outlines` only then and clears it otherwise, and the outlines carry the `Arc` of the texture list. Tests: the app
   test `all_uvs_lists_the_active_faces_of_the_other_elements` and the UI test of All UVs.
7. Fixed. `zoom_step` takes `pixels_per_point` and rounds a whole notch to whole pixels to a texel; the 100 % stop and
   the 25 % to 3 200 % range stay in points. Test: `at_a_scaled_screen_a_wheel_notch_still_lands_on_whole_pixels`.
8. Fixed. Faces that are not chosen and the All UVs faces are outlined with a line of `text_strong` inside and a line of
   `bg_panel` outside, one dark and one light in both themes (looked at in the Latte and Mocha screenshots).
9. Fixed. The band of the view controls is allocated after the checks for a missing or empty texture. Test:
   `a_face_with_a_missing_texture_shows_its_message_right_under_the_first_toolbar`.
10. Fixed in the code, as the spec says: + and - and the arrow keys work while the pointer is over the whole editor
    (zoom about the middle of the view when it is not over the view); the wheel zooms over the view only. Tests: the
    zoom and arrow key tests hover a toolbar and the numbers.
11. Fixed. The Faces tab test takes the rotation control right of the UV view, the comment says the choice collapses to
    the face, and the All UVs test looks for outlines by their place. New: a multi-element move and primary-only
    edits (app), a Shift press then drag, an arrow key during a drag, Escape during a drag
    (`escape_during_a_drag_ends_it_where_it_is`: the drag commits where it is and the pointer moves nothing more).
    The test rig of the panels now applies a `MoveFaces` step to its fixture, as the app does.
