# Review: agent toolkit R1, W-R1.2 core batch

Status: 2026-10-27. Reviewer: Opus. Scope: W-R1.2 of `docs/design/agent_toolkit_r1.md` as built in the working tree:
`crates/cuboid-core/src/{core.rs,commands.rs,lib.rs}`, the new `crates/cuboid-core/tests/batch.rs` and the Status
sentence of the spec. The UI, app library, README, brand and logo changes are not part of it. Run: `cargo test -p
cuboid-core` (all pass, 9 in `batch.rs`), clippy `-D warnings` on all its targets, `cargo fmt -p cuboid-core --check`;
all pass. A throwaway crate in the scratchpad (deleted) ran six probes against the core: a wrong category set, the
selection after a rollback, a zero display scale, the status texts and a restore that changes nothing, dry-run ids and
colours, gestures around a batch. Not run: workspace tests, the gates, the private oracles.

Verdict: not yet. One major finding: the closure form trusts the categories its caller passes, and a wrong set gives
an undo step that misses part of the batch and a rollback that leaves a change under the old revision with the model
marked saved. Its fix (the batch collects the categories itself and rolls back from a clone taken at the start) also
settles finding 3. The one-step history, the redo stack, the dry run, the restore step and the codes are sound.

## Major

1. `crates/cuboid-core/src/core.rs:495` `batch` snapshots the `categories` its caller passes and nothing checks them.
   Ran: `batch("x", ELEMENTS, ..)` that removes the texture `particle` and then names a missing element: the error
   comes back, the texture stays removed, the revision equals the one before and the model counts as saved. Had the
   closure returned `Ok`, the step would lack the texture list and undo would not bring the texture back. Both callers
   to come must build the set by hand without knowing their commands up front: W-R1.3 resolves each op of 5.3 against
   the core as the ops before left it, and the fix loop W-R1.4 ports reads the core between fixes
   (`crates/cuboid-app/src/validation.rs:412`). Fix: let the batch collect what it ran. `run`'s `in_batch: bool`
   becomes `Option<&mut Categories>` and `run_edit` ORs in its `categories` (origin case included) before `perform`;
   `batch` keeps `self.doc.clone()`, `self.sel.clone()` and `self.clip.clone()` from the start (pointer copies, as
   `export_snapshot` takes); at commit it records `capture(&doc0, &sel0, collected)`, on an error it puts the three
   back with `dirty` and calls `refresh_pose` (the revision, the journal and the voxel dirty set come back with the
   document). `batch` loses its parameter, `execute_batch` its prediction loop (`:463`, a second copy of the origin
   rule of `run_edit` at `:689`), and the logged `ApplyError` of `restore` (`:745`) can no longer leave a changed
   document under the old stamp. The smaller alternative keeps the parameter: `Batch` holds the set and `execute`
   refuses with `Refusal::Op` a command whose categories it does not cover. Test: a closure that removes a texture
   with no category named either gets one step that undoes it (collected) or is refused and keeps the texture.

## Minor

2. `core.rs:693` In a batch `run_edit` takes no snapshot, so an `Err` from `Batch::execute` can leave part of the
   command, and a closure that goes on commits it. Going on is the natural port of the fix loop, which records a
   refusal and continues (`validation.rs:416`). Traced: `Duplicate` of a group in a model near `MAX_GROUPS` pastes
   the elements (`crates/cuboid-ops/src/groups/mod.rs:276`) and then `graft` refuses `TooMany` (`:345`); alone the
   command is rolled back (`core.rs:730`), in a closure that continues the copies end up in the step. Fix: capture
   `before` in a batch too and keep the rollback of `:726`; only `record` is skipped. `Batch::execute` then has the
   contract of `Core::execute`, and its doc drops "the command may have left a part of its change".

3. `core.rs:519` The rollback goes through the undo path, which does not give back the state before. (a) Ran: the
   selection model origin, element 2, element 0 (origin primary); a batch of `SetColor` on element 0 and a refused
   texture removal leaves elements 0 and 2 selected, element 0 primary, the origin gone: a restore of the elements
   without `HIERARCHY` rebuilds the selection from the element flags. (b) Ran: the head slot at scale 0, then a batch
   that edits the gui slot and is refused: the head scale is 0.0001 afterwards, under the old revision and with the
   model marked saved, so a save of that revision that finishes later marks clean a model that differs from its
   file (`crates/cuboid-app/src/editor.rs:311`), and an agent's revision token names content that changed. Quoted
   model names and the voxel palette order (rebuilt in scan order, `crates/cuboid-voxel/src/world.rs:65`) behave the
   same. Fix: the clone of finding 1. Test: selection order, primary and origin, and a zero scale, all equal after a
   refused batch; `common::state` sorts the selection and leaves out the primary, so compare `selection()` directly.

## Nit

4. `core.rs:121` The refusal of an instant command in a batch sets no status, so after the rollback the status line
   shows the label of a command that was undone (ran: "Move"). Refuse it inside `run`, which sets the status (`:602`).
5. `core.rs:581` The status after a restore is "Restore " plus the label, "Restore Restore checkpoint" with the test's
   label (ran). Set the status to the label alone, as `run_edit` does; the Edit menu then reads "Undo Restore ...".
6. `core.rs:554` The checkpoint holds `VIEW_PROPERTIES`, so a restore also puts back the stored cameras and grid steps
   of the views, which no command edits. Use `Categories::ALL.difference(Categories::VIEW_PROPERTIES)` for the
   capture and the step.
7. `core.rs:452` "A command that changes nothing is skipped": it runs and answers `Unchanged`; say that.
8. `crates/cuboid-core/tests/batch.rs:303` The kinds are listed by hand; `code()` stops the build on a new variant, the
   test does not see it. Add a match without a wildcard over `Refusal`, `GroupError` and `AnimationError` next to the
   list, so a new variant does not compile until it is listed with its code.
9. `crates/cuboid-core/src/commands.rs:1797` `BatchRefused::outcome` can be `NotYet` or `Failed`, which have no code;
   W-R1.3 needs one each (say `E_NOT_YET`, `E_FAILED`) and 5.1 lists neither.
10. `core.rs:546` `dry_run` is `scratch` plus `execute_batch`. W-R1.3 resolves ops step by step and will call
    `scratch().batch(..)`, which leaves `dry_run` to the tests; keep it only if W-R1.3 calls it.
11. `docs/design/agent_toolkit_r1.md:6` The status of W-R1.2 names no review; add `docs/review/agent_toolkit_w2.md` as
    for W-R1.1.

## The reported choices

1. `execute_batch(label, Vec<Command>)` and `batch(label, categories, ..)`: two forms are right, and the closure form
   is needed now, not with W-R1.4: 5.3 lets a later op name what an earlier op created, and the fix loop reads the
   core between fixes. Caller-supplied categories with no check: not accepted (finding 1).
2. Union of `def.categories` with model properties for the origin, the private flag, one entry at commit when the
   revision moved: right in substance; collect the union while the commands run (finding 1) and keep the per-command
   rollback in a batch (finding 2). The prediction can only be wider than `run_edit`'s, since no edit adds the origin
   to the selection. Gestures: a merged drag, a batch, the same drag again give the steps "Move", the batch, "Move"
   (ran): the batch pushes with no gesture, so it never merges into a drag and a drag after it starts a new step.
3. Rollback through the undo path with stamp, unsaved flag and pose put back, later commands not run, redo kept: the
   last three are right and tested; the undo path is not, and its normalisation is not harmless once the old stamp
   comes back (finding 3).
4. Selection, copy, undo, redo and palette commands refused with `Refusal::Op`: accepted; undo in a batch would break
   the history, the palette commands edit without a step, and the server never selects. Clipboards not rolled back:
   take them with the clone (finding 1); `Cut` is an edit a batch may run and the copy costs one `Arc`. Status not
   rolled back: fine. Selection changes are in fact rolled back, but rebuilt wrongly (finding 3).
5. `scratch()` and `dry_run`: accepted. The copy is isolated: lists, chunks and images are `Arc`s copied on write, the
   `OnceLock` indexes clone with their content, nothing else in the model is mutable through `&`, and no command reads
   the state `scratch` leaves out (`interact`, `tool`, `animate`, `texture_tick`, `clock`). The generator is cloned,
   so the real run repeats the random colours of the dry run (ran); `ObjId`s come from a process counter, so the ids a
   dry run creates are not the real run's (ran). W-R1.3 must report the real revision for a dry run and call its ids
   provisional.
6. `checkpoint()` of all categories and `restore_checkpoint` through `Restore(ALL)` as one step: accepted; undo and
   redo are clean (tested); leave out the view properties (finding 6). Always a step: a restore to the current state
   adds a step and marks the model unsaved (ran); let `checkpoint` return its revision with the snapshot and
   `restore_checkpoint` answer `Unchanged` while it is current. Item overrides: no command changes them, so there is
   nothing to restore. A checkpoint keeps what it shares alive outside `MAX_BYTES`; W-R1.3 should cap checkpoints per
   model. The `after_apply` split and `collider_refresh` keep undo and redo as they were (same calls, same order).
7. `Refusal::code()`: accepted. One code per kind, distinct, in one style, an exhaustive match over the three enums
   (none is `non_exhaustive`). `E_OP` folds every operation refusal, a missing texture included, which the codes of
   5.1 allow. The hand-made test list: finding 8.
8. `history.rs` unchanged: right. `push_gesture`, `continues` and the undo-redo swap cover the batch; the spec lists
   the file as one the package may touch.

## Checked and sound

- One step: a committed batch is one entry under its label, undo and redo swap the whole union (tested with a move and
  a texture removal, and with the model origin); the redo stack goes at commit only and stays on a refusal, on a
  refusal of the first command and on a batch that changed nothing (tested).
- Declared categories: texture removal (faces keep their label), add element, group, ungroup, reparent and set group
  (no element transform changes), duplicate and mirror copies change only what they declare. A batch inherits any gap
  in a declaration exactly as each undo step does, and the existing round-trip tests cover the commands one by one.
- Revision: a commit keeps the stamp of the last command, a rollback the stamp before, a dry run leaves the real core's
  stamp, history and unsaved flag alone (tested).
- Effects: the outcomes keep each command's refresh flags and the collider refresh of the voxel commands; the step
  gets the collider callback when it holds voxels; after a rollback the views need nothing, since they never saw the
  intermediate state and the journal marks what the restore touched. The pose is refreshed after every command and
  after the rollback; `env.restrict_rotations` needs no refresh, since no command changes the setting.
- Instant and deferred commands stop the batch before or in `run`, `validate` refusals too; nesting is impossible,
  since `Batch` hands out `&Core` only.
- `Refusal::code()` names match the enum; the test also checks the code of a refusal a command really gets.

## Fixes

Status: 2026-10-27, Sonnet. `crates/cuboid-core/{src/core.rs,src/commands.rs,src/lib.rs,tests/batch.rs}` and the Status
sentence of the spec. Changed API (all of it new in this package, no caller outside `cuboid-core`): `Core::batch` loses
`categories`, `Core::checkpoint` returns the new `Checkpoint` and `restore_checkpoint` takes `&Checkpoint`,
`Core::dry_run` is gone, `CmdOutcome::code()` is new.

1. Fixed. `Batch` holds the collected `Categories`; `run` takes `Option<&mut Categories>` in place of `in_batch`, and
   `run_edit` ORs its categories in (the origin case included) when the command is done, not before `perform` as the
   review wrote: a refused command rolls itself back, so its categories would only widen the step. `batch` keeps clones
   of the document, the selection and the clipboards from the start; a commit records `capture` of those clones with
   the collected union, an error puts the three back with `dirty` and refreshes the pose (the revision, the journal and
   the voxel dirty set return with the document). `execute_batch` lost the prediction loop and the second origin rule;
   a batch with nothing done (no category collected) leaves no step. Tests
   `a_closure_that_is_refused_leaves_the_document_the_revision_and_the_saved_state` (failed before: the texture stayed
   removed) and `one_undo_restores_everything_a_closure_changed`.
2. Fixed. `run_edit` captures `before` in a batch too and keeps its rollback; only `record` is skipped, so
   `Batch::execute` has the contract of `Core::execute`. Test
   `a_closure_that_goes_on_after_a_refusal_commits_only_the_commands_that_succeeded` runs the traced case (a group copy
   in a model with 65536 groups; it failed with the old condition: four cubes), and
   `a_closure_with_no_command_done_leaves_no_step`. The per-command rollback still goes through the restore path, so
   the normalisation of finding 3 can show on a command that refused half way and after which the closure goes on; a
   rollback of the whole batch is exact.
3. Fixed by the clone of finding 1. Tests `a_rollback_gives_back_the_selection_with_its_order_and_primary` (origin,
   element 2, element 0, compared through `selection().list`, `primary` and `highlighted`),
   `a_rollback_keeps_a_display_scale_of_zero` and `a_rollback_gives_back_the_clipboards` (reported choice 4). Checked
   by putting the old rollback back for a moment (restore of a category set): the origin is gone and the head scale is
   0.0001; without the clipboard clone the cut stays.
4. Fixed. The refusal of an instant command is made in `run`, which sets the status; checked in
   `selection_and_history_commands_are_refused_in_a_batch`.
5. Fixed. `after_apply` no longer sets the status: `after_history` sets "Undo ..." or "Redo ...", `restore_checkpoint`
   the label alone. Checked in `restoring_a_checkpoint_is_one_undo_step`.
6. Fixed. A private `CHECKPOINT` (`ALL` without `VIEW_PROPERTIES`) serves the capture, the restore mode, the step and
   the collider refresh. Test `a_restore_leaves_the_cameras_of_the_views_alone` sets a camera before the checkpoint,
   since the apply path keeps the current camera where the snapshot has none; it fails with `ALL`.
7. Fixed. The doc of `execute_batch` says a command that changes nothing runs and answers `Unchanged`.
8. Fixed. `expected_code` matches `Refusal`, `GroupError` and `AnimationError` without a wildcard and holds the codes;
   `kinds` is a plain list of values, so a new kind stops the build until it has its code. Adding the row to `kinds`
   stays by hand.
9. Fixed. `CmdOutcome::code()` gives the code of a refusal, `E_NOT_YET`, `E_FAILED` and `None` for `Done` and
   `Unchanged`; the test checks the style and that no code repeats. Section 5.1 of the spec still lists neither new
   code and is not changed here; W-R1.3 can name them from `CmdOutcome::code()`.
10. Dropped `dry_run`, kept `scratch`. W-R1.3 resolves the ops step by step and calls `scratch().batch(..)`, and the
    spec names no caller of `dry_run`; `a_dry_run_leaves_the_real_core_alone_and_shows_what_the_real_run_does` runs
    `scratch()` and `execute_batch`.
11. Fixed. The Status sentence names `docs/review/agent_toolkit_w2.md`; the lines of that paragraph were rewrapped to
    120 columns, nothing else in the spec changed.
12. Reported choice 6 (restore to the same state). `checkpoint()` returns a `Checkpoint` (snapshot and revision,
    exported from `lib.rs`); `restore_checkpoint` answers `Unchanged` while the revision is the checkpoint's: no step
    and no unsaved mark (`restoring_the_model_as_it_is_changes_nothing`). Stamps only prove equality: an undo and redo
    pair gives new stamps for the same content, and a restore then still adds a step.
