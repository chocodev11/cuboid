# Review: agent toolkit R1, W-R1.1 library and CLI

Status: 2026-10-27. Reviewer: Opus. Scope: W-R1.1 of `docs/design/agent_toolkit_r1.md` as built in the working tree:
`crates/cuboid-library` (role, schema 3, style sources, style metadata, summary, outline, loader and check),
`crates/cuboid-cli` (brief, check, outline), `Cargo.lock` and the `SetBrief` change in
`crates/cuboid-app/src/library.rs`. The UI, UI kit, README, brand and logo changes are not part of it. Run: `cargo
test` of cuboid-library and cuboid-cli, clippy `-D warnings` on both, `cargo fmt --check` on both and cuboid-app, `xtask
layering`; all pass. A throwaway crate outside the repository (deleted) ran the scenarios of findings 1 to 4 against
the library and measured the outline page and the brief JSON. Not run: workspace and app tests, the private oracles.

Verdict: not yet. Two major findings: Rescan loses references that were renamed or moved inside their folder, and a
Rescan through a path form the import refuses deletes every reference of the folder. The minor findings fit in the
same round. Schema 3, the move of the loader and check, and the brief outputs are sound.

## Major

1. `crates/cuboid-library/src/style.rs:110` Rescan imports first and drops stale entries after. A file renamed or
   moved inside the folder (`refs/a.json` to `refs/b.json`, same content) is found by the import as a duplicate of
   the old entry and skipped; `drop_stale` then finds nothing at `refs/a.json` and removes the entry and its stored
   copy. Ran: after the rename and one Rescan the library holds nothing from the folder (added 0, duplicates 1,
   dropped 1); a second Rescan brings it back. Two files that swap names lose both (traced). Fix: when the folder is
   known, call `drop_stale` before `self.import`; it removes only entries whose file is gone or changed, and since
   the id is the SHA-256 prefix a moved file comes back with the same id and its new `source`. Test: rename one file
   and swap two, rescan once, all three present with their new sources.

2. `style.rs:191` `drop_stale` counts every `Refusal` as stale (`Err(_) => true`), against its own comment ("A file
   that exists but cannot be read keeps its reference"). `DiskFiles::read` refuses by the text of the path first, so
   when the project path has a form that `cuboid_io::texture_files::form` refuses (a verbatim `\\?\` path, which
   `fs::canonicalize` returns on Windows and which W-R1.3 meets when it canonicalises the root as 7 asks, or a UNC
   path), the import refuses the folder and `drop_stale` then removes every reference of the folder with its copy.
   Ran: a library opened at its canonical root, one Rescan: the folder refused, 1 dropped, no stored copy left. Also,
   `folder.is_dir()` (`:88`) asks the system about a network path before any form check, and a UNC project gets a
   source remembered with no references. Fix: refuse a refused form of `folder` with `LibraryError::Source` before
   `is_dir`, the import and the source update; in `drop_stale` count as stale only a missing file, another hash,
   `TooLarge` and `NotRegular`. Test: rescan through a verbatim root keeps the entries and returns an error.

## Minor

3. `crates/cuboid-library/src/import.rs:274` Content is kept once, under whoever came first, and removal looks no
   further. (a) A reference imported first, then the same file imported as own work: "1 already in the library", the
   entry stays `style`, and removing the source deletes it (ran). (b) Two sources hold the same file: the second gets
   nothing, and removing the first leaves the second without an entry for it until its next Rescan (ran). The other
   order of (a) is right and tested. Fix: in `import_one`, a Work import that meets a Style duplicate sets it to Work
   (own work wins and outlives the source); for (b), rescan the other sources after a removal, or at least say in
   the report that the file is already a reference from another folder.

4. `style.rs:79` The checks on the folder are lexical and case-sensitive. `strip_prefix` compares names by case, so
   `D:\Proj\refs` against a library opened as `D:\proj\cuboid-library` is refused as "not a folder inside the
   project" (ran), and the guard at `:85` lets `CUBOID-LIBRARY\thumbs` through as a style folder (ran: accepted; its
   pictures become references). A junction `refs` that points outside the project is accepted and walked (traced),
   though the doc says the folder lies inside the project and 4.1 says what the server reads stays there. Fix:
   canonicalise the folder and the project once, take `relative` from the canonical pair (this settles case, `..`,
   links and junctions), refuse a canonical folder under the canonical library root, and keep joining `relative` to
   the project as given for the walk, since canonical paths are verbatim (finding 2).

5. `crates/cuboid-library/src/outline.rs:16` A page of 128 elements is far over the budget: the outline of 128 cubes
   with six textured faces and a rotation is 57 KB of JSON (measured, 446 bytes an element), not "about 6,000
   tokens" (4.3), and over the 8,000-token results of 5.1 at any plausible bytes per token. Fix: 32 elements a page,
   or a compact face form such as `"north": ["body", 1.5, 2.25, 4.5, 7.75]` with a smaller page; correct 4.3 and the
   CLI help.

6. `crates/cuboid-library/src/report.rs:209` The brief JSON carries each model's `style` inside its `meta`: about
   1 KB of pretty JSON a reference (measured: 11 KB for 9 small JSON references), so a few hundred references make
   `brief_get` (5.2) many times the result budget, while the 4 KB summary already says what they share and `refs_get`
   gives the details. Fix: write the entries' `ModelMeta` without `style` in the brief JSON (a JSON-only view in
   `EntryJson`; the index keeps it), or page the entries in W-R1.3; record the choice in 4.5.

7. `style.rs:486` `typical` adds a distance term only when the model has the value, so a model without a density
   (every Minecraft JSON) or without `sides` scores as if it sat on that median; in a folder of JSON and bbmodel
   references the typical list prefers the JSON ones (traced). Fix: divide the sum by the number of terms that apply.

8. `crates/cuboid-library/src/store.rs:377` Nothing changes a role. 4.6 needs "Use as style reference" and "Use as own
   work", and W-R1.5 lists no file of `cuboid-library`, so the library side belongs to this package. Entries of
   schema 2 have `style: None`, so a model turned into a reference would add only its count to the summary. Fix:
   `Library::set_role(id, role) -> bool` for models and textures that fills a missing `ModelMeta::style` from the
   stored copy; such an entry keeps its absolute `source`, so no style folder removes it, which is right.

9. `outline.rs:73` The outline has no handle to its objects. 5.2 `model_outline` gives ids (`e<n>`, `g<n>`) and 4.3
   wants one `outline()` for all three callers, but an element has only `number` and a group only its path, which
   repeats when names repeat (2). Fix: a `#[serde(skip)] pub node: ObjRef` on `OutlineElement` and `OutlineGroup`,
   so W-R1.3 adds ids without walking the outliner a second time and zipping.

## Nit

10. `crates/cuboid-library/src/meta.rs:385` `words` does not bound a token; JSON labels keep 256 characters, so
    eight long tokens (CJK names without separators, say) can take the summary over 4 KB, which the 1,000-reference
    test cannot reach with its short tokens. Cut tokens at about 24 characters.
11. `meta.rs:360`, `style.rs:426` Names with neither `_` nor `-` do not count, so one `left_arm` among fifty `Cube`
    makes a model snake, and 3 snake models of 10 make the summary snake. Take the plurality of snake, kebab and any.
    The importer's default label `Element` also becomes the token `element`.
12. `style.rs:344` References that are all textures give elements "0 to 0, median 0", element size 0 and groups 0 in
    the JSON and in brief.md; leave the model parts out (`Option`) when there are no models.
13. `style.rs:114` Adding a known folder again with another licence or tags changes the source but not its existing
    entries, and `scanned` moves on after a cancelled import too.
14. `style.rs:181` `drop_stale` reads and hashes every reference of the folder again after the import read them all,
    which doubles the I/O of a Rescan; the import could hand back the hashes it found under the folder.
15. `style.rs:132`, `:149` `rescan_style` and `remove_style_source` match `path` exactly; pass it through
    `source_path` so `refs\lamps` or `refs/lamps/` from a caller matches.
16. `report.rs:502` "1 textures": use `plural`-style counts.
17. `outline.rs:19` is a third copy of the slot names (two are private in `cuboid-fmt-minecraft`); export one there.
    `meta.rs:238` `describe` became `pub` without a caller outside the crate.
18. `crates/cuboid-cli/src/outline.rs:60` An element without listed faces prints a line of four spaces.
19. `crates/cuboid-library/src/check.rs:104`, `crates/cuboid-cli/src/brief.rs:27` For an index that cannot be read,
    the message now starts with the folder given (`proj:`) instead of the library root (`proj\cuboid-library:`).
    Stderr only; exit codes and stdout are unchanged.
20. `style.rs:222` `ElementSize::p10` is documented as the 10th percentile of the longest sides; it is the 10th
    percentile of five quantiles per model. `store.rs:80` `Entry::source` should say that a reference's is relative to
    the project with `/`. `meta.rs:70` should name the space of `bounds`. `agent_toolkit_r1.md:6` still says "Nothing
    implemented yet" above the new status line.
21. `outline.rs:157` `page(offset, 0)` returns no elements and `next == offset`; the CLI maps 0 to all, W-R1.3 must
    refuse 0 or a client pages forever.

## The reported deviations and doubts

1. Observed values, suggestions and the brief.md Library tables ignore style entries: accepted. References would
   otherwise steer "Use observed values" and raise suggestions about files the project does not own; 4.4 gives them
   a summary of their own. Both sides are tested.
2. A style import takes models and textures only, others skipped with a notice: accepted, it is 4.2.
3. A style entry's `source` relative to the project with `/`: accepted; it is what lets a moved project rescan
   (tested) and removal find its entries. It needs the doc of finding 20 and the path handling of finding 4.
4. `StyleMeta::case`; `display` `None`, a preset id or `custom`, the parent breaking ties: accepted. The summary's
   `naming.case` needs a case per model, and `None` keeps references whose slots are all off out of `custom`. The
   parent only decides between presets whose slots are equal, which is the right use of it.
5. Outline of enabled textured faces and enabled slots, UVs in texels with v down, `number`: accepted for this
   package. Texels with v down are what an agent will see in `texture_view`; 4.3 must say so and W-R1.3's
   `set_face.uv` must take the same unit. `number` matches `check`, but W-R1.3 needs more (finding 9).
6. Summary choices: accepted as approximations that keep a model's metadata to a few hundred bytes. Pooling five
   numbers per model weighs models, not elements (doc, finding 20); density from per-model medians is what
   `median_density` does; `texture_sizes` can count an embedded texture and its PNG twice, which 4.4 should say;
   `depth` as a median is fine. `typical` is not (finding 7).
7. brief.md Id column and "(none)": accepted; the id is what `refs_get` takes, and "(none)" is what Rules prints.
8. Minecraft JSON bounds in document space: accepted for consistency with `describe`; nothing outside the library
   reads `ModelMeta::bounds`, and entries of older indexes keep their file-space bounds because a stored entry is
   never described again. Agents read these bounds in the brief JSON while 5.1 and the outline use Java model space,
   so the doc must say which space (finding 20), or the JSON converts them now that it is at version 2.
- The app fix: correct and minimal. `SetBrief` runs on the library thread against the library it edits, `brief_of`
  has no other caller, and the dialog does not show sources. The file is W-R1.5's, but this package needs it to build
  without wiping sources. Add a test with W-R1.5 (`crates/cuboid-app/src/frontend/library_tests.rs`): a library with
  a source, one `SetBrief`, the source is still there.
- A `/` in a label: harmless here, the outline is data. W-R1.3 should match a path selector against the joined labels
  as one string, so `a/b` at the top and `b` in group `a` both match and give `E_AMBIGUOUS`, which an id resolves;
  5.1 should say so.
- `import_style` comparing paths by case: a real refusal and a hole in the library guard (finding 4).
- A file that became too large or not a regular file is dropped on Rescan: correct, it no longer holds the content.
  The wrong case is a refusal by the form of the path (finding 2).

## Checked and sound

- Schema 3: `SCHEMA = 3`; schemas 1 and 2 read with `role: work`, `style: None` and no sources and save as 3 (tests
  for both); `read` and `open` give `Newer(4)` and `open` does not move a newer index aside; older builds refuse 3
  the same way, so none of them saves over the new fields; every new field has a serde default; sources are cleaned
  on read (relative, unique, at most 16).
- Loader and check: `doc::load` and `check::check` are the CLI code with the error mapping kept in `doc::fail`; the
  `Report` fields, their order and `text()` are unchanged, so `cuboid-cli check` prints the same bytes; an unknown
  extension is still exit 1, a failed read 2, issues 4; `read_folder` gives the CLI's old not-found message; the
  unchanged check tests pass. Nothing in the app used the moved code.
- Minecraft JSON through `cuboid-fmt-minecraft` with `NoFiles`: nothing beside the file is read, built-in parents
  give display and parent, the checkerboards are taken out before `describe`, the importer's limits apply.
- Limits: 16 sources on import and on read; sizes checked before reading in the import, in `drop_stale` (through
  `DiskFiles`) and in `doc::load`; the summary is under 4 KB at 1,000 references (test) and 401 bytes at 9 (measured).
- Determinism: summary keys in struct order, `BTreeMap` for sizes and presets, ties broken by key or id, maps sorted
  before truncating in the style metadata; the brief JSON keys follow the documented order (test).
- Removal deletes the copies and pictures of its entries only, never the folder (test); a folder that is gone makes
  Rescan an error instead of dropping its references.
- Layering: `cuboid-library` (L5) gains `cuboid-fmt-minecraft` (L4) and `cuboid-num` (L0), the CLI drops four
  dependencies, `xtask layering` passes. The tests check behaviour; the outline and brief.md goldens are synthetic.

## Fixes

Status: 2026-10-27, after the fix round. fmt, clippy `-D warnings`, the tests of both crates, `cargo check -p
cuboid-app` and `xtask layering` pass. Sizes are bytes of JSON; tokens are estimates at 3 bytes a token (2 for the
worst case), since no tokenizer was run.

1. Fixed. `import_style` calls `drop_stale` before the import, so a renamed or moved file, two files that swap names
   and a file whose twin went all come back with their new `source` (same id). Test
   `a_file_renamed_or_swapped_inside_a_style_folder_survives_a_rescan` failed before (0 added, 3 dropped).
2. Fixed. `import_style` refuses a refused form of the folder or of the project (`refused_form`, now `pub(crate)`)
   with `LibraryError::Source` before `is_dir`, the import and the source update; `drop_stale` counts as stale only
   another hash, a missing file, `TooLarge` and `NotRegular`. Test `a_rescan_through_a_path_form_the_import_refuses_deletes_nothing`
   (Windows, verbatim root) failed before (Ok, entry dropped).
3. Fixed in part. (a) `import_one`: a work import that meets a style duplicate sets it to work through `set_role`;
   test `a_reference_that_the_project_imports_as_its_own_work_becomes_work`. (b) Not rescanned after a removal (that
   needs a `Control` and the I/O of every other folder); `ImportReport::shared` and a line of `notices()` name the file
   and the reference that holds its content, test `a_file_that_two_style_folders_hold_is_named_in_the_report`.
4. Fixed. The folder, the project and the library root are canonicalised, `relative` comes from the canonical pair
   and the walk joins it to the project as given. Tests `style_folders_are_compared_as_the_system_resolves_them`
   (case, Windows) and `a_style_folder_that_links_out_of_the_project_is_refused` (junction or link).
5. Fixed. Whole numbers have no `.0` and a face is `[texture, u0, v0, u1, v1(, rotation)]`; `OUTLINE_PAGE` is 24.
   The worst elements (six faces, rotation on two axes, pivot, long names, fractions) are 387 bytes each in a model
   with four groups: 9.3 KB a later page, 10.6 KB the first (about 3,100 and 3,500 tokens; 4,600 and 5,300 at 2
   bytes a token). The 128-element page was 57 KB. Test `a_page_of_the_worst_elements_stays_under_the_result_budget`
   (12 KB limit). The groups with their children are not paged: they grow with the model (about 25 bytes an element
   for the children) and come whole on the first page, which `model_outline` must cut with `depth`; 4.3 says so.
6. Fixed. The `meta` of a model in the brief JSON leaves out `style` (`listed` in `report.rs`; the key is skipped
   when `None`, so the index leaves out `"style": null` too). A reference is 443 bytes, was 1,143; 300 references are
   133 KB, which no per-entry shape with the documented keys brings under 8,000 tokens, so 4.5 records that
   `brief_get` pages `entries` (about 25 a page). The style of one model stays in the index for `refs_get`
   (`Library::entry`); the CLI has no command for it, `cuboid-cli outline` on the stored copy gives the geometry.
7. Fixed. `typical` divides by the number of terms that apply; test
   `a_model_without_a_density_does_not_score_as_if_it_sat_on_the_median`.
8. Fixed. `Library::set_role(id, role) -> bool` for models and textures; a model without style gets it from its
   stored copy (through `DiskFiles` and `model_meta`) when it becomes a reference, its `source` is kept. Tests
   `a_model_becomes_a_reference_and_back` and `a_model_of_an_older_index_gets_its_style_from_its_copy_when_it_becomes_a_reference`.
9. Fixed. `OutlineElement::node` and `OutlineGroup::node` (`ObjRef`, `#[serde(skip)]`); test
   `outline_items_name_their_nodes_in_the_document`.
10. Fixed. `words` cuts a token at 24 characters; test `a_name_token_is_cut_at_24_characters`.
11. Fixed. `meta::plurality` of snake, kebab and neither, per model and for the summary, a tie is `any`; the
    importer's label `Element` counts for nothing. Tests `the_case_of_a_model_is_the_plurality_of_snake_kebab_and_neither`
    and `the_case_is_the_plurality_of_snake_kebab_and_neither`.
12. Fixed. `elements`, `element_size`, `groups`, `rotation` and `naming` of `StyleSummary` are `Option`, left out of
    the JSON and of brief.md without models; test `references_that_are_all_textures_have_no_model_parts`.
13. Fixed. Adding a known folder with another licence or tags sets them on its references too (a rescan, which brings
    the source's own, leaves edits by hand); a cancelled import keeps `scanned`. Tests
    `a_known_folder_added_again_with_another_licence_changes_its_references` and `a_cancelled_import_does_not_move_the_time_of_the_scan`.
14. Stays. With finding 1's order the import runs after `drop_stale` and cannot hand back the hashes it found; a size
    check first would skip the read of changed files only, never of unchanged ones, and the second read comes from the
    system cache. Nothing changed.
15. Fixed. `style_source` cleans `path` with `source_path`; test `a_style_folder_is_found_by_any_form_of_its_path`.
16. Fixed. "1 texture"; the golden of brief.md says so.
17. Fixed in part. `describe` is private. The slot names stay a third copy: the export belongs in
    `cuboid-fmt-minecraft`, which this round does not own; `pub const SLOT_KEYS` in `json_import/mod.rs` and a
    `pub use` would do.
18. Fixed. No line for an element without faces; test `an_element_without_faces_prints_no_empty_line`.
19. Fixed. `Library::locate` replaces `read_folder` (its only callers were the two messages); a missing index is named
    by the folder, an unreadable one by the library root again. Tests `an_unreadable_index_is_named_by_the_library_root_not_the_folder_given`
    and `an_unreadable_index_of_the_brief_is_named_by_the_library_root`.
20. Fixed in part. Docs of `ElementSize`, `Entry::source`, `ModelMeta::bounds` and `StyleSummary::texture_sizes` (the
    double count of 4.4 sits there, since 4.4 is outside the sections this round may edit). `agent_toolkit_r1.md:6`
    stays for the same reason: "Nothing implemented yet" should go.
21. Fixed in the library. `Outline::page` reads a limit of 0 as 1, so `next` always moves on; test in
    `a_page_cuts_the_elements_and_keeps_the_rest_on_the_first_page`. The CLI still maps 0 to all.

Spec: 4.3 (space, UV unit and v down, compact faces, page of 24 and its sizes, groups whole on the first page, the
nodes) and 4.5 (no `style` in `meta`, sizes, paging of `entries`, the space of `bounds`, 24 by default) only. For
later packages: W-R1.3 pages `entries` and groups and refuses a limit of 0; the app test of `SetBrief` and the
`set_role` request are W-R1.5's.
