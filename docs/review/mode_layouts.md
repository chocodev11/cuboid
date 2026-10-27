# Review: Layouts per mode

Status: 2026-10-27. Reviewer: Opus. Scope: docs/design/mode_layouts.md as built in the working tree: the panel filter
(`crates/cuboid-ui/src/layout.rs`, `ui/dock.rs`), the Properties tabs per mode (`props_area.rs`), the selection rule and
the panel commands (`ui.rs`, `topbar.rs`, `commands.rs`), the one New animation button (`animations.rs`, `timeline.rs`,
`props_animate.rs`), the tab fitting and `tab_button_max` in the kit, the tests, the six review screenshots and the
edits to `ui_v2_spec.md` 6, 8 and 8.1. The library, CLI, app and brand work in the tree is not part of this review.
`cargo fmt --check` and clippy `-D warnings --all-targets` on cuboid-ui and cuboid-ui-kit, the cuboid-ui lib tests, the
`mode_layouts`, `panels`, `animate` and `smoke` tests and the kit's tab tests were run and pass; the workspace tests,
the xtask gates and the private oracles were not run for this review.

Verdict: nearly. One major finding: a change of mode still rewrites the front tab of a group even where the mode would
show that panel anyway, so the default arrangement loses the user's tab on the way Model, Animate, Model, which rule 2
promises to keep, and the test meant to guard the layout blanks the front tabs before it compares. The rest is small
and fits in the same round.

## Major

1. `crates/cuboid-ui/src/ui.rs:881` `arrange_for` calls `Layout::bring_forward` (`layout.rs:345`), which stores the
   panel's place as the group's `active`, and marks the preferences dirty on every change of mode. With the filter, the
   Animate rule (`bring_forward(Animations)`) changes nothing on screen in any of the three arrangements, because
   Animations is the only Animate panel of its group and is the shown tab anyway, but it overwrites the one stored tab
   that the other modes read. Scenario, Blockbench arrangement: in Model click History in the lower left group
   (`active` 2), switch to Animate (`active` becomes 3, Animations), switch back to Model: Animations is not a Model
   panel, so the group shows its first Model panel, Textures, and History is gone. Rule 2 says the panels of the mode
   come back where the user left them, and `ui_v2_spec.md` 8 now says a mode never rewrites the stored layout.
   `going_through_the_modes_moves_no_panel_and_changes_no_size` (`tests/mode_layouts.rs:438`) sets every `active` to 0
   before it compares, so it cannot see this. Fix: give `bring_forward` the mode and let it leave a group alone that
   already shows the panel in that mode (select only when `state.groups.iter().all(|g| g.current(mode) !=
   Some(panel))`), and set `prefs_dirty` in `arrange_for` only when a tab moved (`props_tab` is not a preference). The
   Voxel and Paint rules then still put the palette and the UV editor in front when another tab of the mode was there,
   as section 3 asks; a Model choice those two rules replace stays lost until a front tab is kept per mode, which
   section 11 leaves out, and `ui_v2_spec.md` 8 should say that the front tab of a group is the one thing a change of
   mode may write. Test: Model, click History, Animate, Model: History is in front and `groups[1].active == 2`; and let
   the round-trip test compare `active` for the groups no rule brings forward.

## Minor

2. `crates/cuboid-ui/src/ui.rs:953` The four Cubik actions that show a panel (Show colour tools and Show colour palette
   for the palette, Show outliner, Show textures) go through `Ui::reveal`, which now does nothing for a panel the mode
   does not use, yet the command palette lists them enabled: `runnable` (`commands.rs:145`) only knows `TogglePanel`,
   and the app enables every App action (`action_enabled` is `None` for them, read as true). Scenario: in Model, Ctrl+P,
   type "palette": "Toggle Palette panel" is disabled with "Not used in Model", "Show colour palette" in the same list
   is enabled, and choosing it closes the palette and does nothing. Fix: one
   `fn panel_of(id: ActionId) -> Option<PanelId>` for the four arms of `run_action`, used there and by the enabled
   check, which `runnable` and `Bar::enabled` (`topbar.rs:57`) should share since they are the same match;
   `entry_row` then gives the tip to any row whose panel the mode does not use. Test: the palette test of
   `TogglePanel`, for "Show colour palette" in Model.

## Nit

3. `ui/dock.rs:264` The Properties page is drawn inside the group's `PanelFrame` when its group shows another tab, and
   straight into the group's ui when it is alone, so its widgets are one id level deeper in the first case (the frame's
   body ui). With the filter this now changes with the mode: with the Properties moved into the lower left group of the
   Blockbench arrangement, that group has tabs in every mode but Display, so the Validator page keeps one scroll
   position in Display and another in the other modes. Not reachable in the three arrangements, where the
   Properties stand alone. Fix: give the page an id scope that does not depend on the frame around it, for example
   `UiBuilder::scope_id` with a fixed id in `properties_page` (the panel exists once).
4. `ui.rs:300` `last_primary` lives across documents: when a document is opened over another, its first frame is
   compared with the old document's primary (node ids are per document and can coincide), so it can count as a new
   primary and turn Texture or Model into Element; the first-frame rule holds only for the first document of the
   session. Fix: set `last_primary` to `None` in the branch where `doc_name` changes, or in frames without a document.
5. `topbar.rs:103`, `commands.rs:179` `not_used_in` formats a `String` for every disabled row on every frame the menu
   or the palette is open, also when the row is not hovered (spec 8). Fix: `on_disabled_hover_ui` with the label
   built inside its closure, which only runs while the tip shows.
6. Tests: `mode_layouts.rs:378` ends with an unexplained `get_by_label("Resize")`; `:101` reads the view twice to
   destructure it (one `let [view_left, view_top, view_right, view_bottom]` does). Missing: the round trip of finding 1,
   the Show actions of finding 2.
7. Docs and comments: `mode_layouts.md:66` still says "in the order of `PropsTab::ALL`" (say "in strip order", as
   `ui_v2_spec.md` 8 does); `ui/dock.rs:38` says the group keys are for the headers' fold state, but no group header
   folds and the keys now also salt the group's ids; `ZoneState::first_current` (`layout.rs:199`) has no caller outside
   the layout tests and gained a mode only for them, so it can go.

## The reported deviations

- `PropsTab::ALL` removed: accepted. Nothing else used it, `of_mode` is now the one source of the strip order, and
  `the_strip_of_each_mode_lists_exactly_the_tabs_of_its_table` pins the order of each mode. The design's column header
  should follow (nit 7).
- One New animation button without a current animation: accepted. Both cases have exactly one button named New
  animation on screen and both are in the Animations panel, which is what section 6 asks for; the list's Plus is the
  panel's own button, and a second empty-state button above a list of animations would repeat it.
- The first frame with a document never counts as a new primary: accepted, a document that opens with a selection
  keeps the tab of the mode; but the rule should hold per document (nit 4).
- Room for a move button in a group of Properties and another panel: accepted. The room is the same whichever tab is in
  front, so a click between the Properties and the other panel (whose header has the button) never reflows the row
  under the pointer; it costs 32 points in that one group.
- `Ui::reveal`, the toggle keys and `rename_primary` do nothing for a panel the mode does not use: accepted. They match
  the disabled rows of View > Panels, and a key that switched the mode would be the larger surprise; `rename_primary`
  needs its guard, or the outliner would start a rename that shows up in the next mode. The Show actions reach the same
  no-op through enabled rows (finding 2).
- An `id_salt` per group: accepted. In egui 0.37 a child's scope id is its parent's scope with the salt, without the
  counter, so both groups of an area had the scope `<area>.child` and the Library and Palette scroll areas clashed in
  Voxel. It resets nothing a user had: egui's memory is not saved between runs, and the salt is the stored slot
  (`left_top` and so on), which no mode changes. The one id path that does follow the mode is unrelated to it (nit 3).
- The `sed -i` on `crates/cuboid-ui-kit/src/widgets/mod.rs`: no damage. The file differs from HEAD only by the
  `tab_button_max` export, and every changed file is UTF-8 with LF and no BOM.

## Checked and sound

- Filter: `PanelId::serves` matches the table of section 3; a group shows its active panel when it serves the mode,
  else the first that does, and a stale `active` falls back the same way; a click stores the clicked panel by its
  place in the full list (`chosen` starts at the stored `active`, so drawing writes nothing); a group without a panel
  for the mode takes the one-group path and the split is neither read nor written; an area without one returns before
  its egui panel, which keeps its width in memory, so the stored width is untouched.
  `drawing_in_every_mode_leaves_the_stored_layout_unchanged` compares the preferences JSON.
- `is_shown`, `shows`, `toggle` and View > Panels take the mode; `validator_visible` stays right in every mode, since
  the Properties serve all of them, and `uv_outlines_wanted` is false wherever the UV editor is not drawn, so the app
  stops listing outlines; `show_validator` opens the Validator in every mode. `checked` is only used for UI commands,
  so its disabled tip only ever reaches panel rows.
- Strip: the current tab is always one of `of_mode`: it is set by `for_mode`, by `show_validator` (in every mode), by
  `follow_primary` (Element, in Model and Voxel only) and by clicks on the strip.
- Selection: a new primary in Model and Voxel turns Texture and Model into Element; Element, Faces and UV and the
  Validator stay; a cleared selection, the same primary with more objects, and a selection made in another mode change
  nothing; undo goes through the same comparison.
- Hints: the two lines are the texts of section 6 in the caption style; the timeline is a resizable bottom panel, so
  its height does not follow the line; the only New animation buttons left are the Animations panel's.
- Tab fitting: the header's tabs ui has no item spacing, `TAB_ICON_WIDTH` is the width `tab_icon` allocates, and the
  room (header width minus the 24 point button and the 8 point edge) ends at the move button's left edge, so steps 2
  and 3 fill it exactly; the clip is set on the tabs ui only, so the actions are unaffected, and the clipped part of a
  tab neither paints nor takes the pointer. The cut tab's accessible name and tooltip are the whole title.
  `tabs_paint-narrow_mocha_100_1920.png` shows "UV e..." and three icons ending at the button.
- Kit: `galley_cut` takes the layout out of `paint_clipped` without changing it (the placeholder colour is replaced
  when painted).
- Cost: the tabs of a group come from an iterator over its list; nothing new is kept per frame apart from nit 5.
- Screenshots: Model, Voxel, Paint, Display and Animate at 1280 show the panels of the table, the strips of section 4
  and, in Display, the views reaching the tool strip.

## Fixes

1. Fixed: `Layout::bring_forward(panel, mode)` leaves a group alone that already shows the panel in the mode and
   returns whether a tab moved; `arrange_for` marks the preferences dirty only then. `ui_v2_spec.md` 8 and
   `mode_layouts.md` 2, 3 and 11 say that the front tab is the one thing a change of mode may write (9 lists the
   tests). New:
   `a_mode_that_shows_a_panel_anyway_leaves_the_tab_in_front_of_its_group` (Model, History, Animate, Model) and
   `bring_forward_leaves_a_group_alone_that_already_shows_the_panel_in_the_mode`; the round trip now compares every
   `active` but the lower left group's, which the Paint rule moves to the palette.
2. Fixed: `keymap::panel_of` is the one mapping from the four Show actions to their panels; `run_action` reveals
   through it, and `Command::panel` and `Command::runnable` replace `commands::runnable` and `Bar::enabled`, so
   the palette and the menus disable the rows of a panel the mode does not use, and `entry_row` gives each of them
   the tip. New: `the_show_actions_of_a_panel_that_another_mode_uses_are_disabled_with_the_tip` (Model and Display
   disabled with the tip, Enter keeps the palette open; Voxel, Paint and Animate enabled).
3. Fixed: `properties_page` draws inside `scope_builder` with a fixed `UiBuilder::scope_id`, so its widget ids no
   longer depend on the frame around it. No test: the three arrangements never put the page in a frame.
4. Fixed: `last_primary` is reset where `doc_name` changes. A document opened over another with the same title
   still compares with the old primary; the UI has no document identity to tell it by and no test (the fixture has
   one title).
5. Fixed: the tip is built by `on_disabled_hover_ui` in the palette row and in `Bar::checked`, so the text exists
   only while it shows.
6. Fixed: the unexplained `get_by_label("Resize")` is gone, the view is destructured once, and the two missing
   tests are written (findings 1 and 2).
7. Fixed: the design's column header says "in strip order", the comment of `GROUP_KEYS` names what the keys do now
   (they salt the group's ids and name the frame of its tabs), and `ZoneState::first_current` is removed with its
   two test uses moved to `Group::current`.
