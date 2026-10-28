# Review: agent toolkit R1, W-R1.3 cuboid-mcp

Status: 2026-10-28. Reviewer: Opus. Scope: W-R1.3 of `docs/design/agent_toolkit_r1.md` as built in the working tree:
the new `crates/cuboid-mcp`, `check_document`, `Checked` and `document_meta` in `crates/cuboid-library` with one outline
test, the workspace entries in `Cargo.toml` and `Cargo.lock`, the layers in `xtask/src/layering.rs`, and section 3 and
the Status of the spec. The README, brand and logo changes are not part of it. Run: `cargo test -p cuboid-mcp` (75
pass), clippy `-D warnings` on all its targets, `cargo fmt -p cuboid-mcp --check`, `cargo xtask layering`; all pass. A
throwaway crate in the scratchpad (deleted) measured `tools/list` and ran seven probes: an empty selector list, a
created model and a checkpoint after eight other opens, `refs_get` through a junction, the first outline page of 200
groups, `texture_view` of a 1024 px texture at scale 16, and globs with many stars. Not run: workspace tests, the gates,
the private oracles.

Verdict: not yet. The protocol matches the official pages in both eras, and the resolver, the read-only rule, the
revision tokens, the batches and the fix mapping are sound. One major finding: `project_export` overwrites any file in
its folder, the model's own source included, where the export dialog and spec 7 ask first. Six minor findings are
limits and one path that the code does not hold: memory of `texture_view`, the stored copies read past the resolver,
what the LRU closes, the outline's groups, the glob matcher and a copy of the app's template rule. The entry point fits
W-R1.4; nit 18 lists two additions to the `Renderer` contract.

## Major

1. `crates/cuboid-mcp/src/project.rs:589` `project_export` writes each file with `write_atomic` whether it exists or
   not. Scenario: open `lamp.json` (a Minecraft model) and call `project_export {"model": "lamp.json", "format":
   "java_model", "folder": "."}`: the export replaces `lamp.json` itself, and a texture without a Minecraft path
   replaces a `lamp.png` of the same name beside it; files are outside the undo history, so nothing brings them back.
   `folder: "textures"` or a pack folder the user keeps by hand fare the same. The export dialog refuses this:
   `export_minecraft_json` and `export_resource_pack` (`crates/cuboid-app/src/flows.rs:234`, `:281`) return the taken
   paths and write nothing unless `replace` is set, and spec 7 guards saves the same way (`E_FILE_CHANGED` unless
   `overwrite`). Fix: resolve every target first; when any exists, answer `E_FILE_CHANGED` naming up to 50 of them and
   write nothing, unless the new input `overwrite: true` (schema, example, description: "a second export to the same
   folder needs overwrite"). Test: an export over an existing file is refused and leaves its bytes; with `overwrite` it
   replaces them; the default `exports/<stem>` works the first time.

## Minor

2. `crates/cuboid-mcp/src/texture.rs:519` `texture_view` enlarges the whole texture by `scale` (`picture.rs:139`) and
   only then fits it to 1024 px. Ran: a 1024 px texture at `scale: 16` takes 4.8 s and allocates 16384 x 16384 x 4 =
   1 GiB for a picture that leaves as 1024 px. Opened files may hold textures up to 16,384 px
   (`cuboid_image::MAX_SIDE`); a 4096 px one at scale 16 asks for 16 GiB, and a failed allocation aborts the process
   with the unsaved edits of every open model. The previews of `texture_paint` (`texture.rs:480`) have the opposite
   gap: a region over 256 px is sent at full size, not fitted to `MAX_IMAGE`, so a fill over a 2048 px texture returns a
   2048 px picture (spec 5.1: at most 1024). Fix: use `scale.min(MAX_IMAGE / longest side).max(1)` (the result
   already reports `scale`); for a texture over 1024 px fit it first and scale the rectangles by the same factor; end
   the preview with `.fitted(MAX_IMAGE)`. Test: the 1024 px case answers `scale: 1` at once.
3. `crates/cuboid-mcp/src/brief.rs:261` and `views.rs:197` load a stored copy at `library.file_path(entry)`, which does
   not go through `Root::resolve`, against spec 7 and the crate doc (`lib.rs:9`). Ran: after `refs_import`,
   `cuboid-library/files` replaced by a junction to a folder outside the root: `refs_get` returns the outline of the
   file out there and `render_view {"ref": id}` loads it, while `resolve` refuses the same path with `E_PATH`. The index
   names plain file names only, so the reach is the folder the junction points to. Fix: one helper, say
   `Server::stored_copy(&library, entry)`, that passes the path through `self.root.resolve(&self.root.relative(&path))`,
   used by both; `the_library_index_cannot_send_a_tool_outside_the_folder` gains the junction case.
4. `crates/cuboid-mcp/src/server.rs:242` The LRU keeps only dirty models. Ran: `project_open {"model": "new.cubik",
   "create": "item"}`, eight other opens, then `model_outline new.cubik`: `E_NO_MATCH` "new.cubik is not a file", since
   a model made from a template is not dirty. Ran: `checkpoint c` on a saved model, eight opens, `restore c`:
   `E_NO_MATCH` "there is no checkpoint c"; its undo steps went too. Fix: close only a model that has a file
   (`stamp.is_some()`), no checkpoints and no unsaved edits; when none is left, `E_LIMIT` with the hint "Save one of the
   open models with project_save" (the hint of `ToolError::limit`, `error.rs:108`, says to split the work, and there is
   no close tool); say in the `project_open` description that a saved model that closes loses its undo steps. Test: both
   runs above keep their model and checkpoint.
5. `crates/cuboid-mcp/src/inspect.rs:132` The first outline page keeps every group (`Outline::page` clears them only
   after it); `depth` cuts nesting and children stop at 24, but nothing bounds the number of groups. Ran: 200 groups of
   one cube each give a default first page of 31,281 bytes, over the 24,000 bytes the tests hold as 8,000 tokens and
   over the 10,000 tokens at which Claude Code warns. The budget test `busy` has 12 groups. Fix: list at most 60 groups
   a page with `groups_total`, the rest on the following pages in outline order, or cut at 60 and tell the agent to
   narrow with `select`. Test: `busy` with 200 groups stays under the budget.
6. `crates/cuboid-mcp/src/select.rs:159` `matches` tries every split for each `*`, so its time grows with the text to
   the power of the stars. Ran: `*a` ten times then `b` on a 40-character name: 11.3 s for one name (eight stars:
   0.46 s). `Tree::item` runs it for every object, `project_list` for every file, on the one thread, and a client
   timeout does not stop it. Fix: the iterative matcher that backtracks only to the last star (`**` the same, but it may
   cross `/`), or a memo over (glob, text) positions; both are O(glob x text). Test: that glob answers false at once.
7. `crates/cuboid-mcp/src/project.rs:270` `template_document` copies the document half of `Preset::for_template`
   (`crates/cuboid-app/src/preset.rs:177`): the rotation rule of the tier, the item kind, the display slots of the
   brief and the `preset` loop. W-R1.4 plans to drop the app's fix mapping, not this, so a change to the app's templates
   would make agent models differ from GUI ones. Fix: move it into `cuboid-library` beside `DisplayPreset` (it has
   `cuboid-assets` and `builtin_display` already), call it here, and from `for_template` in W-R1.4.

## Nit

8. `select.rs:91` An empty list passes serde despite `minItems: 1`. Ran: `delete` and `move` with `select: []` give the
   core's `E_NOTHING_SELECTED` "nothing is selected" (about a selection the agent never makes), `model_get` a total of
   0; 5.1 says no match is `E_NO_MATCH`. Refuse an empty list in `Tree::select` with `E_INPUT`.
9. `texture.rs:425` A dry run of `texture_paint` composes only and runs no command, so a refusal of `AddTexture` or
   `ReplaceTexture` shows only in the real run. Run the same batch on `core.scratch()`, as `model_edit` does.
10. `edit.rs:1287` `face_id_of` repeats `fixes::face_id` (`fixes.rs:44`); make one `pub(crate)`. The nine
    `.at(self.index)` in `edit.rs` are covered by `map_err(|error| error.at(index))` at `:1369`; drop them.
11. `edit.rs:576` `changed.updated` counts commands: one `add_cube` reports `added: 1, updated: 4` (size, position,
    pivot and name of the new cube). Count distinct objects and leave out the ones the call created.
12. `protocol.rs:242` A modern request without `clientInfo` (optional) keeps the name of an earlier request, so the
    journal can credit another client; the base page says identity is not taken from earlier requests. Set
    `server.client` to `unknown` when the modern `_meta` names none. Renders write files under `renders/`, but `writes`
    (`tools.rs:294`) leaves them out of the journal; journal them or say in 5.1 that the journal holds edits only.
13. `edit.rs:1099` `W_ROTATION_SNAPPED` says "steps of 22.5 degrees up to 45", the Classic rule; the core snaps on any
    tier but `MultiAxisFree`. Say "the rotation rule of this model's version allows ..." with the rotation it kept.
14. `tools.rs:81` `refs_import` has `destructiveHint: false`, though a second import of a known folder drops the
    references whose file changed and deletes their stored copies. Set it true.
15. `inspect.rs:583` The palette target of `model_stats` repeats `near` (`crates/cuboid-library/src/brief.rs:481`) with
    another rule: met only when no colour is outside, while the brief flags a texture when fewer than half of its
    colours are near. Export the library's test and use its rule, or call the value `colours_outside`.
16. `crates/cuboid-library/src/check.rs:252` `Found.fix` is `fix.is_some()` with the doc "Whether the Validator panel
    offers a fix"; the panel offers `fixable` only, as `validate` now does (`SetCull` and `SetDisplay` fixes are not
    offered). Correct the doc now; W-R1.4 may map those two too, since `SetFaceCull` and `SetDisplaySlot` exist.
17. `tests/common/mod.rs` The appended block (`:285` to `:331`) is fine as bytes (LF, UTF-8 without BOM, final newline,
    rustfmt-clean), but it opens a second `impl Client<'_>` (`:296`) beside the first (`:90`); move its four methods
    into the first and `modern` up beside `transcript`. `text` and `join` (`:140`, `:144`) are used nowhere, which
    `#![allow(dead_code)]` hides; remove them.
18. W-R1.4 contract. `render.rs:79` should say that the orthographic cameras of one document share one scale (framed
    from the 3D bounds, not from each projection), else the front and top cells of a sheet are not comparable; and a
    size over the adapter limit wants its own `RenderError` that maps to `E_LIMIT` (spec 7), not `Failed` and `E_IO`.
    On Windows Claude Code ends the process tree 300 ms after it closes stdin (code.claude.com MCP page), so the exit
    test of W-R1.4 ("within 1 s") should ask for well under 300 ms.
19. Spec. The Status names no review for W-R1.3: add this file. `ping` (legacy only) sits in the "Both eras" row: move
    it to "Legacy". The code list of 5.1 lacks `E_INPUT`, `E_IO`, `E_EXPORT`, `E_NOT_YET`, `E_FAILED` and the refusal
    codes (`E_NOTHING_SELECTED`, `E_OP`).

## The reported points

1. Protocol: accepted. Modern by `_meta` naming the version or the capabilities, then both required (-32602): the
   versioning page serves "a request carrying modern per-request `_meta`" statelessly and the base page makes a request
   without a required field -32602. `server/discover`, `tools/list`, `tools/call` with `resultType: "complete"` and
   `serverInfo` in the result's `_meta`: as the pages show them. -32022 with `{supported, requested}`: the shape of the
   versioning page, whose example lists a legacy version in `supported`, as this server does. Legacy `initialize` for
   2025-11-25 and 2025-06-18, else 2025-11-25: the 2025-11-25 lifecycle rule. `ping` legacy only: right, the
   2026-07-28 changelog removes it. `ttlMs` and `cacheScope` on discover and list: required ("MUST include caching
   hints") by the caching page, and `public` fits a list that never changes. Arrays and lines over 16 MiB: -32600 is
   right, batching went in 2025-06-18. Tool failures as `isError` results with text and no `structuredContent`: as the
   execution error example of the tools page. Both corrections to section 3 are right. Only nit 12 remains.
2. `tools/list`: measured 45,807 bytes: input schemas 23,381 (`model_edit` 10,206), output schemas 15,889, descriptions
   2,852, examples 2,954 inside the input schemas. Acceptable. Claude Code with tool search, its default on current
   models, loads only tool names and the server instructions at start and a definition when Claude searches for it;
   without tool search a client gives the model name, description and input schema, about 26 KB or 8,000 tokens. A
   Messages API tool definition has no field for an output schema (per client not verified). If a cut is wanted:
   shorten the descriptions of the 16 op branches of `model_edit` and share `select` and the face list as `$defs`,
   then drop examples that repeat a description. The instructions (1.3 KB) stay under Claude Code's 4,096-character cut
   and name the tools to search for.
3. Inputs beyond 5.2: `cursor` and `limit` on `brief_get` and `validate`, accepted (the budget needs them);
   `namespace` and `id` on `project_export`, accepted (the default `minecraft:cubik_model` would make every pack the
   same model); `target` on `apply_fixes`, accepted, the words of `validate`. `texture_view.faces` as a selector:
   accept the selector, rename it `select`: every other tool says `select` for objects and `faces` for face names
   (`set_face`, `auto_uv`, `flip_uv`), so `faces: ["north"]` now gives `E_NO_MATCH`. `validate.fix` meaning that
   `apply_fixes` can run it: accepted, it is what the panel offers (nit 16). `model_edit` returning `dry_run`,
   `textures` and warnings: accepted. Rotations rounded with a warning: accepted, it is the core's restricted rule
   (`snap_angle`) as the GUI applies it (nit 13). `E_INPUT`, `E_IO`, `E_EXPORT`: accepted; list them in 5.1 (nit 19).
4. Folders only, no zip: accepted for R1; `ResourcePack::to_zip` (`flows.rs:322`) is there if an input asks for it.
   The folder write needs finding 1.
5. The `cat >>` append: the bytes and the tests are fine; the placement is nit 17.

## Checked and sound

- Pages read on modelcontextprotocol.io: 2026-07-28 base protocol, versioning, `server/discover`, stdio, tools, caching,
  cancellation and the changelog; 2025-11-25 lifecycle; 2025-06-18 changelog. Also the MCP page of code.claude.com
  (tool search, output limits, images, Windows shutdown).
- Framing: a line keeps at most 16 MiB, the rest is skipped and the next line served; the last line needs no newline,
  CRLF and blank lines are fine; -32700 and -32600 carry id null; null and fractional ids are refused; notifications and
  responses get no reply; `notifications/cancelled` names a request already answered, which the cancellation page lets
  a server ignore; stdout gets only `writeln!` of `serde_json` values, which hold no raw newline; end of input returns.
- Paths: `resolve` refuses `..`, absolute, drive-relative, UNC, `\\?\`, `\\.\`, `/??/`, streams, device names with any
  extension and the superscript digits, trailing dots and spaces, NUL and over 1024 bytes; it follows links on the part
  that exists and checks the canonical prefix, so a junction out is refused for a file that exists and for a new one
  (tested with `mklink /J`), names below are plain, and a dangling link fails. `Root::new` refuses relative, network and
  drive roots. `project_list` follows no link; the library walk refuses links out (W-R1.1).
- No socket, process, thread or tokio in `src`; `--read-only` refuses every write before it runs and renders without a
  file (tested on the folder listing).
- Revisions and edits: random session per process, `E_SESSION` and `E_REVISION`; a dry run reports the real revision
  and calls its ids provisional; `model_edit` and `texture_paint` are one batch, one step, rolled back whole;
  `apply_fixes` in a batch goes on past a refused fix as the panel does; `MoveBy` and `SetPosition` skip covered
  descendants, so `head/**` moves once; undo and redo count steps; restore is one step or `Unchanged`; 16 checkpoints.
- No unsaved edit is ever closed: eight dirty models give `E_LIMIT`.
- `validate` equals the library check (order, counts, codes, messages, hints, element ids); `check` calls
  `check_document`, so `cuboid-cli check` is unchanged; `document_meta` equals `model_meta` of the file (test).
- `fixes.rs` is the app's mapping on the batch API: the same supported set, clamp and failure texts;
  `apply_fixes(core, issues)` is one step, so W-R1.4 drops the two gestures.
- Saves: `E_FILE_CHANGED` from size and time, a name open as another model refused, the library refused, a `.bbmodel`
  saved as `.cubik` renamed. Exports: pack paths checked by `stays_inside` and the resolver, the library refused.
- Results: brief pages of 50 entries, `validate` 60 issues, `model_get` 32, `model_find` 300, `project_list` 150, an
  outline of 48 elements stay under 24 KB in the tests; render pictures are at most 1024 px.
- Layers: `cuboid-mcp` on 6 uses layers 0 to 5, `cuboid-app` on 7; `base64` 0.22.1 was in the lock already.
- W-R1.4 entry: `serve(root, options, renderer, input, output)`, `Options { read_only }`, `ServeError::Root` for a bad
  folder, `self_check` and `snippets` give the strings of 6.1 and 6.2 (tested), `NoRenderer` makes only the render
  tools `E_NO_GPU`.

## Fixes

Status: 2026-10-28, Sonnet. All 19 findings hold when traced; none was left unfixed as wrong.

1. Fixed. `project_export` resolves every target first and, when any exists, answers `E_FILE_CHANGED` naming up to 50 of
   them and writes nothing, unless the new input `overwrite: true` (schema, example, description). Tests: an export into
   the folder of the open model `stone.json` is refused byte for byte for the model and for a model never saved, a pack
   likewise, `overwrite` replaces, the default `exports/<stem>` works once and then needs `overwrite`.
2. Fixed. `texture_view` takes `scale.min(1024 / longest side).max(1)`, fits the texture to 1024 px before it
   enlarges and draws the rectangles by the ratio of picture to texture; `texture_paint` fits a region to 1024 px first,
   then enlarges small ones. Tests: 1024 px at scale 16 answers `scale: 1`, 128 px answers 8, a 4096 px texture gives a
   1024 px picture with its outline at 512, all three calls in under 5 s, a fill over 2048 px gives 1024 px.
3. Fixed. `Server::stored_copy` passes the stored path through `Root::resolve`; `refs_get` and `render_view` and
   `render_sheet` with `ref` use it. A junction in place of `cuboid-library/files` is `E_PATH` (own test, since the
   existing index test needs no link); a bad file name in the index is now `E_IO` for the render tools too, as for
   `refs_get`.
4. Fixed. A model closes only when it has a file, no checkpoints and no unsaved edits; among those one without undo
   steps goes first, then the least recently used, so undo steps are lost only when nothing else can close (the
   description of `project_open` says so). With none left `E_LIMIT` says "Save one of the open models with
   project_save so that it can close; a model with checkpoints stays open." Tests: both runs of the review keep
   `new.cubik` and the checkpoint, the order of closing, the hint.
5. Fixed by the second way: the first page lists at most 60 groups in outline order and says `groups_total`; the
   rest is reached with `select`. Test: 300 elements in 200 groups give 60 groups under 24,000 bytes.
6. Fixed. `Cargo.lock` has no glob crate (`regex` only, which only `criterion` pulls in), so `matches` is a table over
   (glob, text), O(glob x text). The classic matcher that backtracks to the last star only does not fit: a `*` that
   stops at `/` next to a `**` needs the earlier star to move (`**a*/b` on `xa/ya/b`). Tests: the 10-star case and two
   more in under 500 ms together, the `**` rules as before.
7. Fixed. `cuboid_library::brief::minecraft_template` and `restrict_rotations` hold the rule; `project_open` and
   `ProjectSettings::for_template` call the first, `Preset::restrict_rotations` the second; `BriefDefaults.display` is a
   `DisplayPreset`. The app's preset tests pass unchanged; a library test covers the rule.
8. Fixed. `Tree::select` refuses an empty list with `E_INPUT`, which covers every tool that takes a selector.
9. Fixed. A dry run of `texture_paint` sends the same commands to `core.scratch()`, so it refuses what a real run would
   and names the labels the real run gives.
10. Fixed. `fixes::face_id` is `pub(crate)` and `face_id_of` is gone; the nine `.at(self.index)` are gone, the loop of
    `model_edit` names the op.
11. Fixed. `changed.updated` counts distinct objects that were there before and still are: the targets of the commands
    that were done (the fixes of `apply_fixes` through the elements of the issues they fixed), minus the ones the call
    made or deleted. Test: a new cube is `updated: 0`, four commands on one old cube and one on another are 2.
12. Fixed. The client name is set on every modern request and on `initialize` (`unknown` without a name). Renders that
    wrote a file are journaled, and a read-only server still journals none (it writes no file). Tests for both.
13. Fixed. The warning says "the rotation rule of this model's version" and the rotation it kept, with the rule the core
    applies (one axis, 22.5 degree steps); it no longer claims a tier.
14. Fixed. `refs_import` has `destructiveHint: true` (test on the annotations) and its description says what a second
    import drops.
15. Fixed. `cuboid_library::brief::{colours_outside, outside_palette}` hold the test and the rule; `model_stats` calls
    them: the target is met when fewer than half of the model's colours are outside, `actual` is the count outside.
16. Fixed. The doc of `Found.fix` says the validator names a fix and the panel offers a smaller set. Mapping the
    `SetCull` and `SetDisplay` fixes stays for W-R1.4.
17. Fixed. The four methods are in the first `impl Client`, `modern` is beside `transcript`, `text` and `join` are gone;
    `#![allow(dead_code)]` stays, since each test file uses part of the helpers.
18. Fixed in the crate. The `Renderer` doc says the orthographic cameras of a document share one scale framed from the
    3D bounds, and `RenderError::TooLarge(limit)` maps to `E_LIMIT` with the hint to ask for a smaller size (test; the
    self-check reports it). `serve` says it returns at once at the end of the input and a test holds it under 100 ms.
    Not done: the "within 1 s" of the W-R1.4 row in section 8 of the spec; that row is outside the lines this fix may
    change, so W-R1.4 takes "well under 300 ms" from here.
19. Fixed in the spec: the Status names this review, `ping` moved to the Legacy row, 5.1 lists all codes (the 21 of
    `Refusal::code()`, `E_NOT_YET`, `E_FAILED` and the 12 of the server). Left for whoever edits the spec next: 5.1
    still says the least recently used saved model closes (it is now the rule of finding 4), 5.2 still lists `faces`
    for `texture_view` and no `overwrite` for `project_export`.
