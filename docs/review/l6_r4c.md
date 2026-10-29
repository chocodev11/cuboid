# Review: L6 R4-C, MCP panic boundary, fuzz targets, panic lints

Status: 2026-10-28. Reviewer: Opus. Scope: the uncommitted R4-C fixes of audit findings M4, M7, M8, m7 and m10 of
`docs/review/l6_r4_audit.md` (spec `docs/design/l6_release.md` 5): `crates/cuboid-library/src/{lib,meta,store,style,
thumb,view}.rs`; `crates/cuboid-mcp/src/{lib,error,schema,server,protocol,brief,check,edit,history,inspect,picture,
project,select,texture,views}.rs` and the `Value` reads of `connect.rs`; `crates/cuboid-mcp/tests/{common/mod,protocol}.rs`
and the `internal` line of `guide.rs`; `fuzz/Cargo.toml`, `fuzz/Cargo.lock`, the seven new targets;
`.github/workflows/{nightly,ci}.yml`. Read with `git diff`, plus the code the changes reach: `tools.rs`,
`cuboid-app/src/{crash,lifecycle,mcp,mcp_render}.rs`, `cuboid-fmt-cubik/src/{lib,load,container}.rs`,
`cuboid-library/src/{kind,report}.rs`, `Rev` in `cuboid-image`, and `libfuzzer-sys` 0.4.13 `src/lib.rs`.
Run: `cargo clippy -p cuboid-mcp -p cuboid-library --all-targets -j 4 -- -D warnings` (clean),
`cargo test -p cuboid-mcp -p cuboid-library -j 4` (210 pass: library 110, mcp 100),
`cargo check --manifest-path fuzz/Cargo.toml --locked` (clean), `cargo fmt --check -p cuboid-mcp -p cuboid-library`
(clean), `cargo fmt --check --manifest-path fuzz/Cargo.toml` (three new targets differ, Nit 3). Not run: the fuzz
binaries (they do not link on stable MSVC), the nightly workflow, release builds, the app.

Verdict: ready after one major. The boundary works: a panic in a tool becomes an `isError` result with `E_INTERNAL`,
the next request is served, stdout carries only replies, and nothing is left half borrowed. The lints are in both
crates and every rewritten index keeps its behaviour. The nightly job runs all 18 targets with a memory limit and
keeps the crashing input, CI checks the fuzz crate, and the seven new targets are the ones the audit asked for. The
one major is the closing rule: it throws away unsaved edits after panics that cannot have changed any model, which
are the likeliest panics, and after a panic in a call that names no model it closes all of them.

## Major

1. `crates/cuboid-mcp/src/server.rs:322-357` (`panicked`) with `crates/cuboid-mcp/src/protocol.rs:342-349`: the
   closing rule is wider than the state a panic can damage (reported point 1). Only three tools change an open model
   in memory: `model_edit` (`edit.rs:1359`), `texture_paint` (`texture.rs:467`) and `history` (`history.rs:108`,
   `:129`, `:147`). Their dry runs and `history` `list` change nothing: a dry run edits `core.scratch()`, a copy
   (`edit.rs:1383`, `texture.rs:469`). Every other tool reaches an open model only through `Server::open`, a
   shared borrow: `validate` (`check.rs:122`), the four `model_*` reads, `texture_view`, `project_export` and the
   renders, whose renderer gets a `&Document`. `project_save` reads the model until `save_path` returns and then makes
   four assignments that cannot panic (`project.rs:477-480`). `project_open` builds the `OpenModel` before `insert`.
   The tools that name no model (`brief_get`, `refs_import`, `refs_get`, `project_list`, a render of a `ref`) never
   take a model mutably: `brief.rs` has no access to `models`, `project_list` reads them (`project.rs:326`, `:351`),
   and the `ref` branch of `views.rs` skips `Server::model`. `Core`, `OpenModel` and what they hold have no interior
   mutability (no `Cell`, `RefCell` or `Mutex` in cuboid-core, -model, -ops, -validate, -scene, -image). A shared
   borrow therefore cannot leave a model half applied.
   What the rule costs: the audit names the validator and the renderer as panic sources, and both only read. A
   panic in them throws away the unsaved edits and checkpoints of the model they read. A panic in `render_sheet` with
   `ref` (step 2 of `INSTRUCTIONS`) or in `brief_get` closes every open model, up to 8. That is the loss of the crash
   M4 fixes, without the restart. A panic in the verifying reload of `project_save` (`verify: true`,
   `project.rs:460`) closes the very model whose edits the save was meant to keep. The new protocol test pins the
   loss: `tests/protocol.rs:231-233` expects a render panic to drop the edit of call 3.
   Fix: close the named model only when the call changes a model in memory. Otherwise close nothing, and drop the
   close-all branch. In `tools.rs`, next to `writes`:
   ```rust
   /// Whether a call changes an open model in memory, so that a panic may leave it half changed.
   /// The other tools read the models, or write files from a model they only read.
   pub(crate) fn changes_model(tool: &str, arguments: &Value) -> bool {
       matches!(tool, "model_edit" | "texture_paint" | "history") && writes(tool, arguments)
   }
   ```
   In `Connection::call`, take `model` only when `tools::changes_model(name, &arguments)` holds. In `panicked`, use
   `None | Some(Err(_)) => Vec::new()` and remove the `mem::take` arm and the `mem` import. The three tools require
   `model` (`edit.rs:42`, `texture.rs:54`, `history.rs:33`), so `None` no longer means "not known". Hint when
   nothing closed: "This is a bug of the server, not of the call. The open models are as they were before the
   call." Tests: the protocol test expects `undo_total` 1 for `lamp.bbmodel` and a hint without "Closed".
   `a_panic_of_a_call_that_names_no_model_closes_every_model` becomes "closes nothing" with both models kept. Add a
   unit test in which `panicked("model_edit", Some("a.cubik"), ..)` closes `a.cubik` and keeps `b.cubik`. Because
   `panicked` takes the model already filtered, no tool has to panic for real. A further step could keep even the
   edited model by taking `core.checkpoint()` before each changing call and rebuilding from it after a panic. That
   costs a snapshot on every call of the common path, for a bug path, so I would not do it.

## Minor

1. `docs/mcp.md:308-325`: the guide does not have `E_INTERNAL` (reported point 5). The person who reads the
   troubleshooting table is also the one who can send the crash report. The guide test checks only one direction:
   every code in the guide is a code of the server. Fix: add a row to section 7: ``| `E_INTERNAL`: "the tool ...
   panicked" | A bug in cuboid. The server caught it and keeps serving. The hint names the model it closed, which
   loses its unsaved edits and checkpoints. A report `crash_*.txt` is written next to `cuboid.log`. | Send the
   report and the call that failed. The agent opens the model again from its file. |`. `docs/mcp.md` is in R1's
   diff, so add the row after R1 is committed. Optional: in
   `guide.rs`, also assert the other direction for the `ToolError` list, so that the next new code cannot be left
   out.
2. `fuzz/fuzz_targets/mcp_requests.rs:13-16` and `:51`: the target never reaches the pixel code. The model has no
   texture, and `NoRenderer` answers `E_NO_GPU`. So `texture_paint` on an existing texture (`paint_grid`,
   `fill_rect`), `texture_view` with `outline_rect`, the sheet compositing and `Picture::coverage` never run. m10
   named exactly that indexing (`texture.rs:307`). A read-only server still allows dry runs of `texture_paint`
   (`tools.rs:298`) and renders without a file, so both are reachable. Fix: give `MODEL` a 16 x 16 texture as a data
   URL, built like `texture_url` in `tests/common/mod.rs`, and use it on the cube's faces. Replace `NoRenderer`
   with a stub in the target that returns `Picture::new(size, size, ..)` with an opaque square in a clear field,
   like `Fake` in `tests/common/mod.rs:22-42`, so that `coverage` has edges.

## Nit

1. `.github/workflows/nightly.yml:57-59`: `-rss_limit_mb=2048` is libFuzzer's default, and `-malloc_limit_mb`
   follows it, so the flag changes nothing. The comment "the memory limit proves it" reads as a tighter limit. Fix:
   keep the flag and write "libFuzzer's default of 2048 MB, made explicit: a loader that allocates past it fails
   the run."
2. `fuzz/fuzz_targets/mcp_requests.rs:29`: the comment says "Bit 0 reads the rest as the stream of requests", but
   the code takes the stream when bit 0 is clear (`mode & 1 == 0`, `:34`). Fix: "A clear bit 0 reads the rest as
   the stream of requests; a set bit 0 makes the next byte pick a tool ...".
3. `fuzz/fuzz_targets/library_file.rs:9`, `mcp_requests.rs:51` and `minecraft_files.rs:58` are not formatted by
   rustfmt. CI formats only the workspace. Fix: rustfmt these three files. `cubik.rs:33` is older and outside R4-C.
4. `fuzz/fuzz_targets/mcp_requests.rs:22`: each fuzz process leaves a folder `cuboid-fuzz-mcp-<pid>` in the temp
   folder. Fix: use a fixed name. The content is the same in every process and the server writes nothing there. A
   torn read by a second process would only be a load error.
5. `crates/cuboid-mcp/src/texture.rs:281-311`: the second lookup `if let Some(part) = work.iter_mut().find(..)`
   skips the op without a word if the part is missing. For the same impossible case, `Server::open` answers
   `E_INTERNAL`. Fix: keep one lookup, for example `Some(at) => work.get_mut(at)`, and `None` pushes and takes
   `work.last_mut()`, followed by
   `.ok_or_else(|| ToolError::internal("a texture of the call is gone"))?`.
6. `crates/cuboid-mcp/src/select.rs:204`: no test pins the empty-text branch `rest.last().copied().unwrap_or(head)`
   (`:189`). Add `assert!(matches("", "") && matches("*", "") && matches("**", ""))` and
   `assert!(!matches("a", "") && !matches("?", ""))`.
7. `crates/cuboid-mcp/src/server.rs:352`: after a close-all, the hint says "the other open models keep their edits",
   but no open model is left. Major 1 removes that case.

## The reported deviations

1. Close all models after a panic in a call that names no model: not accepted. A narrower rule is safe, and Major 1
   gives it. Only `model_edit`, `texture_paint` (both unless `dry_run`) and `history` (unless `list`) can leave a
   model half applied. Each of them names its model.
2. `schema::set` and `open`/`open_mut`: accepted. Every one of the ~45 targets is an object, built by `json!({..})`,
   `object()`, `select()` or `subject_properties()`. With `preserve_order`, `Map::insert` keeps the position of a key
   that is already there and appends a new one, as `IndexMut` did. `IndexMut` would have turned a `Null` into an
   object where `set` does nothing, but no target is `Null`. In `edit.rs`, the closure of an op now sets `op` on the
   properties before `object()`. `needed` always holds `"op"`, so `required` is still written. `tests/tools.rs` still
   checks every example against its schema. `open`/`open_mut`: the index comes from `model`, `find` or `insert` a
   moment earlier, so the error cannot happen. An answer with a code is better than an `allow`.
3. Fuzz inputs: accepted. `load_from` (`cuboid-fmt-cubik/src/lib.rs:158-161`) loads the payload first. The empty
   document passes `decode` and `sections`, which refuse only bad textures and voxels, so every input reaches
   `extension::read`. The bbmodel gives the model tools a model to work on, and the read-only server still allows
   dry runs, so `model_edit` ops are fuzzed on a scratch copy. For the coverage gap, see Minor 2.
4. Only `cargo check` of the fuzz crate: accepted. `#![no_main]` binaries need the link that cargo-fuzz sets up. A
   Linux `cargo check` catches API drift, which is what m7 asks for, and the build script of `libfuzzer-sys` compiles
   its C++ part during a check too. The first real link and run of the seven targets will be the next nightly. After
   the commit, start the workflow once by hand (`workflow_dispatch`) and read the 18 jobs.
5. `E_INTERNAL` missing from the guide: not accepted. See Minor 1.
6. Test module appended with a heredoc: the content is sound. It is a `#[cfg(test)] mod tests` at the end of the
   file, as in `picture.rs` and `select.rs`. The file is LF only, has no BOM, is ASCII and ends with a newline:
   checked byte by byte, and `git ls-files --eol` gives `i/lf w/lf`. `cargo fmt --check` passes. The tests call
   `panicked` directly and need no tool that panics. The breach was in the process only, not in the result.

## Checked and sound

- Index rewrites, compared with HEAD one by one. The glob matcher (`select.rs:164-190`) maps one to one: `head` is
  `row[0]`, `rest[i]` is `row[i + 1]`, `left` is the new `row[at - 1]` and `diagonal` the old one. The result is
  `rest[n - 1]`, or `head` for an empty text, so `*`, `**`, `?` and the empty text behave as before.
- `blend` (`picture.rs:181`), `outline_rect` (`texture.rs:600`) and the `OUTLINES` lookup keep their behaviour.
  `compose` does too, apart from Nit 5.
- `edit.rs` keeps its behaviour: `check_box`, the centre of `mirror`, the rotation tolerance and `faces.get` are the
  same. `FaceName` has six values for six faces, and `uv_rect` falls back to the full rectangle as it does for a
  missing element.
- In `inspect.rs`, the box test uses `Vec3::cmpge` and `cmple` with the same 1e-4 margins, and a NaN still fails.
  `history.rs` checkpoints keep their order: replace, then the limit, then push.
- In cuboid-library: `Bounds` zips the axes. In `quantile`, `low` and `high` stay at most `last` for `q` in [0, 1],
  and a NaN gives index 0 as before. `median` uses `mid - 1` only for an even, non-empty list. `png_size` still needs
  24 bytes. `as_chunks::<4>` drops the remainder as `chunks_exact` did; Rust 1.95 has it. `lore_meta` gives the
  same as `min`.
- `by_sha`, `counts`, the role change, `View::count`, `style_summary` and `import_style` take their indices from a
  `position` on the same list. The digest in `thumb.rs` is 32 bytes. `downscale` reads four bytes and writes four
  where it used to read 3 + 1, and an empty box now gives 0 where it would have divided by zero. Neither case can
  happen.
- The `Value` reads in `connect.rs` give `None` where `Index` gave `Null`, with the same defaults.
- Unwinding. The closure borrows the server mutably and owns the arguments. After a panic, `models` is a valid
  `Vec`, because `remove` and `push` are atomic, `clock` and `used` are plain integers and `session` does not
  change. The journal is written only after `run` returns `Ok` (`tools.rs:322-326`), so a panicked call leaves no
  line and never half of one. Revision tokens are safe: `Rev::next` is a global atomic counter
  (`cuboid-image/src/contract.rs:52`), so the token of a closed model never matches a model opened again, and the
  answer is `E_REVISION`, not a false match. `GpuRenderer::draw` takes its cached scene before drawing
  (`mcp_render.rs:190`), so a panic leaves no stale scene. The mutexes of cuboid-render recover from poison.
  `payload.as_ref()` downcasts the inner value, so both `&str` and `String` messages are found.
- stdout. The tools cannot reach the writer, and `serve` writes the reply only after the boundary returns. The app's
  hook writes `crash_*.txt` to the logs folder (rotated, `crash.rs:114`) and then calls std's hook, which prints to
  stderr. The logger writes to stderr and to the file. One dependency remains: Major 1 of `docs/review/l6_r1.md`
  (`eprint!` in the logger) must land, or a caught panic while stderr is broken panics again inside the hook and
  aborts.
- The panic strategy: release builds use `panic = "unwind"` (`Cargo.toml:137`), and dev builds use the default.
- `--check` (`connect.rs:146`) calls `Server::call` outside the boundary. A panic there ends the one-shot check with
  a crash report, which is acceptable.
- `libfuzzer-sys` replaces the panic hook with one that aborts (`src/lib.rs:91-95`), so `mcp_requests` still reports
  a caught tool panic as a crash. The boundary hides no bug from the fuzzer.
- The lints: both `lib.rs` files have the four-lint block under `not(test)`, and clippy is clean. The non-test
  indexing that is left uses constant indices into arrays, or glam `Vec3` with constant axes (`fixes.rs:92-106`).
- The new targets follow the audit's plan. Each split stays in bounds (`len * b / 255 <= len`), and `.png.mcmeta` is
  tested before `.png`. `parse_index` calls the same `Library::parse` as `open`, the scope does not change the
  parsing, and the four summaries read no file. No new dependency builds native code: in `fuzz/Cargo.lock`, only
  `libfuzzer-sys` compiles C++. So the nightly job needs no `apt` step.
- The workflows. The matrix has 18 targets, the same as the 18 `[[bin]]` of `fuzz/Cargo.toml`. The artifact path is
  where cargo-fuzz writes, `<fuzz-dir>/artifacts/<target>`. The artifact name is unique per matrix leg, as
  `upload-artifact@v4` requires. `if-no-files-found: ignore` covers a failed build. The CI step runs on Linux only,
  after clippy, and `rust-cache` keeps `fuzz/target`.
- The protocol test checks the M4 contract end to end: a panic gives `isError` with `E_INTERNAL` and the panic
  text, the next request is served, and the other model keeps its undo step. The unit tests cover a model name that
  is not open.

## Fixes

Status: 2026-10-28. Checks, all clean: `cargo fmt -p cuboid-mcp -- --check`,
`cargo clippy -p cuboid-mcp --all-targets -j 4 -- -D warnings`, `cargo test -p cuboid-mcp -j 4` (103 pass) and
`cargo check --manifest-path fuzz/Cargo.toml --locked`. (Another package added `cuboid-settings` to the dependencies of
`cuboid-library` while this one was open, so `fuzz/Cargo.lock` has that line too: `cargo check --offline` wrote it.)

- Major 1: Fixed. `tools::changes_model` (`model_edit` and `texture_paint` unless `dry_run`, `history` unless `list`)
  decides whether `Connection::call` takes the model name; `Server::panicked` closes that model only, the close-all
  branch and the `mem` import are gone, and the hint without a closed model reads "The open models are as they were
  before the call." `tests/protocol.rs` now expects `undo_total` 1 for `lamp.bbmodel` and a hint without "Closed". The
  unit tests: a panic of `model_edit` on `a.cubik` closes it and keeps `b.cubik`; a call that names no model closes
  nothing; a model that is not open closes nothing; a new test in `tools.rs` pins `changes_model` for the three tools
  and for the dry runs, `list`, `project_*`, renders and `validate`.
- Minor 1: Fixed. `docs/mcp.md` section 7 has the `E_INTERNAL` row, as the review gives it; the guide test passes. The
  optional second direction in `guide.rs` is not done.
- Minor 2: Fixed. The `MODEL` of `mcp_requests` has a 16 x 16 texture as a data URL (made once in `root()` with
  `encode_png` and `png_url`) on four faces of the cube, and `NoRenderer` is replaced by `Square`, a stub that returns a
  `Picture` with an opaque square in a clear field, like `Fake`. The target now needs `cuboid-model` for the
  `Document` of the `Renderer` signature: added to `fuzz/Cargo.toml` and `fuzz/Cargo.lock` (the `--locked` check
  passes). A throwaway test on the same model showed `texture_view` with four rects, a `texture_paint` dry run on the
  existing texture (`new: false`) and a render with `coverage` and `bounds`; the label of the texture is `0`, its id.
- Nit 1: Fixed by the coordinator: `.github/workflows/nightly.yml:57` now reads "# libFuzzer's default of 2048 MB,
  made explicit: a loader that allocates past it fails the run."
- Nit 2: Fixed. The comment of `mcp_requests` says that a clear bit 0 reads the rest as the stream of requests and a
  set bit 0 makes the next byte pick a tool.
- Nit 3: Fixed. `rustfmt --edition 2024` on `mcp_requests.rs`, `library_file.rs` and `minecraft_files.rs`. `cubik.rs`
  is older, outside R4-C, and still differs.
- Nit 4: Fixed. The folder is `cuboid-fuzz-mcp` in the temp folder, with no process id, so the processes share one.
- Nit 5: Fixed. `compose` looks the part up once (`position`, then `get_mut`, or push and `last_mut`) and answers
  `E_INTERNAL` ("a texture of the call is gone") if it is missing, as `open()` does.
- Nit 6: Fixed. `select.rs` has the test `only_stars_match_an_empty_name` with the two assertions of the review.
- Nit 7: Fixed by Major 1. The hint of a closed model says the other open models keep their edits, which now holds
  because at most one model closes.
