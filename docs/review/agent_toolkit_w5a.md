# Review: agent toolkit R1, W-R1.5 style references in the UI

Status: 2026-10-27. Reviewer: Opus. Scope: the Style references half of W-R1.5 of `docs/design/agent_toolkit_r1.md`
(4.1, 4.2, 4.6 and the W-R1.5 row of 8) as built in the working tree: `crates/cuboid-ui` (brief dialog section, card
line, Style chip, role actions, `start.rs`, `lib.rs`, tests) and `crates/cuboid-app` (`library.rs`, its frontend
tests). The AI clients half, `crates/cuboid-core`, the README, brand and logo changes are not part of it. Run: `cargo
test -p cuboid-ui -j 4` (all pass), clippy `-D warnings` on cuboid-ui with all targets, `cargo fmt --check` on
cuboid-ui and cuboid-app. Not run: the cuboid-app tests, because `cuboid-core` of the other package does not compile
(`run_edit` is called with an `Option` at `core.rs:596`); the screenshot run; the gates.

Verdict: not yet. One major finding: Save in the brief dialog is lost without a word while the import that the
dialog itself started is running. The minor findings fit in the same round. The requests, the scope, the counts and
the removal are sound.

## Major

1. `crates/cuboid-ui/src/brief.rs:449` with `crates/cuboid-app/src/library.rs:842`. Save, and Enter with it, stays
   enabled while a job holds the library, and `edit` returns without a toast while `slot.job` is set. Before this
   package the dialog opened only when the library was idle (Edit is disabled while busy). Now Add folder and Rescan
   start an import from inside the dialog and leave it open, so the ordinary flow loses edits: Add folder, pick a large
   folder, change the goal while it imports, Save. The dialog closes, `SetBrief` reaches `edit`, `edit` returns, and
   the goal is gone (traced end to end). The brief_ui test `the_folders_follow_the_library_while_the_dialog_is_open`
   sets `busy` but checks only the three style buttons. Fix: `ModalButton::primary("Save").enabled(!busy)`; the kit
   then ignores Enter too. Also make the early return in `edit` push the "The library is busy; try again in a moment."
   toast that `import_job` gives, so a request is never dropped silently. Tests: in brief_ui, while busy Save is
   disabled and Enter sends nothing; in the app, `SetBrief` during an import gives the toast.

## Minor

2. `brief.rs:574` While its import or rescan runs, the dialog only greys out the section. The job's progress and its
   Cancel sit in the status bar under the modal's backdrop, which takes no clicks. So a long import can be cancelled
   only by closing the dialog first, which loses unsaved edits because Save cannot run (finding 1). The export dialog
   cancels its own job. Fix: give `LibraryView` the job of the shown library (label, fraction and id, all of which
   `LibraryService::jobs` has). While `busy`, draw it at the top of the section with `progress` and a Cancel button
   that start.rs sends as `UiRequest::CancelJob(id)`, through a `StartEvent` the way `statusbar.rs:108` does. Test:
   busy with a job, Cancel sends `CancelJob`.

3. `crates/cuboid-app/src/library.rs:1360` `inside` copies `within` (`crates/cuboid-library/src/style.rs:40`). The
   Remove question promises the count that `inside` gives, but `remove_style_source` deletes what `within` selects. A
   later change to either one (case, a trailing `/`) makes the question wrong. Fix: make `within` `pub`, add it to
   the `pub use style::{...}` of `crates/cuboid-library/src/lib.rs` and call it in `styles_of`. Better still, add a
   `Library::style_counts()` beside `style_ids`, which replaces the walk in `styles_of`. The library is committed,
   but one `pub` is smaller than a second copy, and W-R1.1 took `set_role` for the same reason.

4. `library.rs:751` with `:791` A picked style folder goes into the library of the scope shown when the picker
   returns, not the project library it was asked for. The folder dialog does not block the window. Scenario: Add
   folder, then Save, switch the start screen to Global, then pick. The toast says "The import failed: style folders
   belong to the library of a project" and the pick is lost. If another project is opened meanwhile and its folder
   contains the first one, the folder is imported into the wrong project's library. Fix: `PickFor::Style { licence,
   root }`, with `slots[1].root` taken at the request. `poll_pickers` drops a pick whose root is no longer
   `slots[1].root`, with an Info toast. `import_job` takes a scope, so the style job runs on slot 1 whatever scope is
   shown.

## Nit

5. `library.rs:675` `remove_style_source` saves the index itself, and `edit` saves it again, even after a refusal
   that changed nothing. That is two writes of `library.json` for one click. Skip the second save when the change
   saved.
6. `brief.rs:184` "from N folders" counts every folder, empty ones too, while the totals include references that
   were marked one by one. One empty folder and two marked models read "Style: 2 models from 1 folder". Count only
   the folders that hold references, and write "from N folders" only when their counts add up to the totals.
7. `brief.rs:569` In the Global scope of a saved project, the caption tells the user to save a model. It should say
   to switch the library to Project; `LibraryView::project_available` knows which case applies.
8. `brief.rs:153` `counts` still writes "1 models", so use `noun` there too. `noun` is also a second plural helper
   beside `export_dialog.rs:721` `plural`; keep one.
9. `brief.rs:419` The licence field starts with the text `unknown`, so a new licence needs select-all first (the
   test types Ctrl+A). Start the field empty with the hint `unknown`: `clean_licence` turns an empty licence into
   `unknown` anyway.
10. Wording: the caption says "the folder is never changed" but the question says "The folder itself is not
    touched", so pick one. A failed rescan reports "The import failed: ..." (`library.rs:810`); pass the verb along
    with the label.
11. There is no review shot of the Remove question over the dialog, so its two backdrops are unchecked. Add a
    `Dialog::BriefStyleRemove`.

## The reported choices

1. The Remove question takes a click only: accepted. The Modal row of `ui_v2_spec.md` says a destructive confirmation
   takes a click, and the library panel's question already works that way. Esc closes the question only, since the
   brief modal is not the top one (tested). `BriefAnswer::Style` keeping the dialog open: accepted. It lets the
   section follow its own jobs, but it is what makes findings 1 and 2 reachable. `show` reading the live view:
   accepted. The inputs stay the dialog's, while folders and counts follow the library, and a pending question
   closes when its folder goes (tested).
2. Style chip through `ListRow::shortcut("Style")`: accepted for this round. `key_chip` is `badge(.., Tone::Neutral)`,
   so the row draws the Badge of ui_v2_spec. Only the name is wrong, and the comment says so; add a `ListRow::badge`
   the next time the kit is changed. The detail as a tooltip is fine: in a 230 pt panel the name needs the room.
3. Card and folder lines that name textures, with "from N folders" only when there are folders: accepted, with nit 6.
4. One caption in the Global scope: accepted. D-R5 says so and `import_style` refuses Global anyway; see nit 7.
5. The copied `within` in `inside()`: not accepted, see finding 3.
6. `screens.rs` scrolling to Edit, and the new shots: accepted. At 1280 and 150 % the card sits under the recent
   files. The old brief shot now uses the style fixture too, which is fine.
7. `lib.rs` and `tests/common/mod.rs` outside the file list: accepted, since the new type and fields force both.
   `start.rs`, which was not reported, is also outside the list and is forced by the new `show` signature.
   `requests.rs`, which is in the list, needed nothing, because `other => state.library.apply(other)` routes the new
   requests.
8. Context menus about 600 pt wide: pre-existing, and the cause is confirmed. `MenuItem::ui`
   (`crates/cuboid-ui-kit/src/widgets/menu.rs:101`) takes `ui.available_width()`. When a popup opens, egui first runs
   a sizing pass in an Area that is `spacing.default_area_size.x` wide (600 by default), the row takes all of it, and
   the Area keeps that size. Every menu built from `MenuItem` takes this path, including the menu bar's (traced, not
   shot). Fix: `let fill = if ui.is_sizing_pass() { 0.0 } else { ui.available_width() };`, then
   `fill.max(content).max(MENU_MIN)`, which is what egui's own `Separator` does. The fix is small and safe, but it
   lives in the kit, outside W-R1.5, and it changes every menu and every shot with one. Make it a kit change of its
   own right after this round, with a kit test: a two-item context menu is `MENU_MIN` wide, and all rows of a menu
   share one width.

## Checked and sound

- Requests: Add folder opens the picker in the project folder, then runs `import_style` with the licence as an import
  job with progress and cancel, toasting a refusal and returning the library either way. Rescan runs `rescan_style`
  as an import job. Remove runs `remove_style_source` as an edit, which toasts the count or the error. SetRole runs
  `set_role` as an edit. All of them go through `requests.rs`.
- Project library only: the section needs `brief.project`, which comes from the scope of the library view. The menu
  items need `LibScope::Project` and a model or texture, the same kinds `set_role` takes. The library refuses style
  folders in Global.
- Removal: `remove` deletes only the copy, companion and picture under the library (`file_path`, `companion_path`,
  `thumb_path`). The folder and its files stay, as the app tests check.
- Counts: `styles_of` runs on every library that comes back. Its filter (`role == Style` and the prefix test) matches
  `style_ids`. A reference turned into own work drops out of its folder's count and survives both Rescan and removal,
  as in the library. A one-by-one reference keeps its absolute source and belongs to no folder. The slot keeps the
  last counts while a job holds the library.
- Brief save: `SetBrief` takes the library's folders at the time of the save; the app test checks memory and disk.
- One click, one request: a Style answer returns before Save is read. Enter on a focused button does not confirm the
  modal (`egui_wants_keyboard_input`). The modal ids are unique, and Esc and Enter reach only the top modal (kit,
  `is_top_modal`). Rescan and Add folder are disabled once the view is busy, and `import_job` refuses a second job.
- Edge cases: without a saved project the scope is Global and the caption shows. An empty folder is kept and reads
  "No models or textures", its question has no count, and the card shows no line. A folder outside the project or
  inside the library is refused as an error toast and the library stays usable (app test). While a job runs, the
  style buttons are disabled (test).
- The tests check behaviour through accessibility labels and the requests sent; the app tests use real folders.
  Labels are in sentence case and match 4.6, and the question text is the spec's.

## Fixes

Status: 2026-10-27. Every finding was traced in the code first; all are as the review says. Run: `cargo fmt -p`
and clippy `-D warnings` (all targets) on cuboid-ui, cuboid-app and cuboid-library, their tests (all pass, the app
ones included), `cargo xtask layering` and `ui-style`. Numbers are those of the findings.

1. Fixed. Save is `ModalButton::primary("Save").enabled(!view.busy)`, so the kit ignores Enter too; `edit` refuses
   with the `import_job` pattern (one let-else on root, library and job), so a refused edit gives the "The library is
   busy; try again in a moment." toast. Tests: brief_ui `save_and_enter_wait_while_a_job_holds_the_library_and_the_edit_is_kept`
   (Save disabled, Enter sends nothing, the goal typed meanwhile goes out with the later Save); app
   `an_edit_while_a_job_holds_the_library_is_refused_with_a_toast` (Add folder, `SetBrief` while the import runs; fails
   without the fix).
2. Fixed. `LibraryView.job` is the `JobView` of the shown library (the app fills it in `rebuild` and refreshes its
   progress every `step`, since the view is rebuilt only on a change). The section draws it above the folders, outside
   the greyed group: label, bar and a "Cancel import" button (not "Cancel", which the modal already has), sent as
   `BriefAnswer::CancelJob` and `StartEvent::CancelJob` to `UiRequest::CancelJob(id)`. Test
   `the_job_of_the_library_shows_in_the_section_and_cancel_import_stops_it`; shot `dialog_brief-style-job`.
3. Fixed with the first option: `within` is `pub` and re-exported, `styles_of` calls it, `inside` is gone. Not
   `Library::style_counts()`: `styles_of` also totals the references that belong to no folder, so the app would keep a
   walk of its own, and one `pub` is the smaller change.
4. Fixed. `PickFor::Style { licence, root }` takes `slots[1].root` at the request (the picker also starts from it);
   `poll_pickers` drops a pick for another root with the Info toast "The project changed while you picked a folder; add
   it again."; `import_job` takes the scope and the style import runs on `Scope::Project` whatever is shown. Test
   `a_picked_style_folder_goes_to_the_project_it_was_asked_for` (Global shown when the pick returns; another project;
   fails without the fix).
5. Fixed. `edit` has a `save` flag and the removal passes false, since `remove_style_source` saves itself: one write per
   removal, none for a refusal. Test `a_refused_removal_leaves_the_index_alone_and_a_removal_reaches_the_disk` (the
   index file is untouched after a refusal; the writes of a success are not counted).
6. Fixed. "from N folders" counts the folders that hold references and appears only when their counts add up to the
   totals; with references marked one by one the line reads "Style: 3 models". Tests in
   `the_card_says_how_many_references_the_project_has_and_from_how_many_folders`.
7. Fixed. A saved project with Global shown reads "Style references belong to the project library. Switch the library
   to Project to add them."; without a saved project the old text stays. `BriefDialog::show` now takes the
   `LibraryView`, which has `project_available`. Tests `the_global_library_has_no_style_folders` and
   `without_a_saved_project_the_caption_asks_for_a_saved_model`.
8. Fixed. `noun` is gone; `brief.rs` uses `export_dialog::plural` (made `pub(crate)`, the one change in that file), also
   for the card's "1 model, 1 texture". Test `the_counts_of_the_card_use_the_singular_for_one`.
9. Fixed. The licence field starts empty with the hint `unknown`; the request carries the empty text and the library
   reads it as `unknown`. Tests: brief_ui (empty field, hint, empty licence sent, typing without select-all) and app
   (`style_folders_are_rescanned_and_removed_without_touching_the_folder` picks with an empty licence and finds
   `unknown`).
10. Fixed. The caption says "the folder itself is not touched", the question's words. `import_job` takes the verb with
    the label, so a failed rescan reads "The rescan failed: ..." (tested in the same app test).
11. Fixed. `Dialog::BriefStyleRemove` renders `dialog_brief-style-remove_{mocha,latte}_100_1280` and `mocha_150`; both
    backdrops are as they should be in Mocha and Latte. Added `Dialog::BriefStyleJob` for the card of finding 2.

Reported choice 8 (menu width in `crates/cuboid-ui-kit`): not touched, as asked; it stays a kit change of its own. Two
files outside the list were forced: `export_dialog.rs` (the `pub(crate)` of `plural`) and `ui.rs` (one match arm for
`StartEvent::CancelJob`).
