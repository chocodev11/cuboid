# L6 R4: audit of the stability rules

Date: 2026-10-28. Scope: package R4 of `docs/design/l6_release.md` (sections 1, 5 and 6). It checks the
stability rules of `docs/design/roadmap_lean.md` section 7 against the code. This document lists findings
only. No code was changed.

Method: I read the loaders, the write paths, the I/O thread and job code, the frame boundary and the GPU glue.
I searched the non-test code for panicking calls, allocations sized from data, file writes, file-system calls on
the UI thread, locks and thread pools. Where a finding depends on how a library behaves, I read that library's
source in the cargo registry: egui-wgpu 0.37.0, egui 0.37.0, wgpu 30.0.1, wgpu-core 30.0.1 and rayon-core
1.13.0. Nothing was built or run: no fuzzer, no GUI, no tests. Failing inputs are described, not executed.
Line numbers come from the working tree on 2026-10-28. R1 was editing `main.rs`, `lifecycle.rs`, `logging.rs`,
`mcp.rs` and `library.rs` in `crates/cuboid-app` at the same time, so lines in those files may be off by a few.

## Summary

I found no Critical finding: no user file or action that I could trace to a crash or to data loss. The format
crates deny `unwrap`, `expect`, `panic!` and indexing in non-test code, size everything before they allocate,
bound nesting and decompression, and have fuzz targets. Every save, export, autosave, settings and library
write is atomic and runs on an I/O thread. The frame boundary works as roadmap 7 describes.

The gaps are around these rules, not inside them:

- The GPU paint path of the shell is outside the panic boundary. A lost device makes egui-wgpu panic there and
  ends the process before the device-loss autosave is written (M1).
- The window never gets a new device (M2).
- The thumbnail renderer takes over the device-lost and error callbacks of the window renderer (M3).
- The MCP server has no panic boundary (M4).
- UI-thread `par_iter` calls wait behind background work in the one global rayon pool (M5).
- The UI thread stats recent files every 30 s (M6).
- The nightly fuzz job skips three targets (M7), and several parsers of external data have no target (M8).

Counts: Critical 0, Major 8, Minor 11.

| Rule (roadmap 7) | Verdict | Findings |
|---|---|---|
| 1. Loaders: no `unwrap`, `expect`, `panic!`, unchecked indexing or overflow on external data; sizes checked before allocating; depth limits; no decompression bombs | holds | m5, m10, m11 |
| 2. Every file write is atomic (temporary file in the target folder, then rename) | holds | m8 |
| 3. Saves and exports run off the UI thread; the UI thread never blocks on disk or GPU readback | gaps | M5, M6 |
| 4. Frame panic boundary: last good snapshot, crash report, autosave, toast, recovery dialog after repeated panics; jobs isolated | holds for the frame logic, gaps around it | M1, M4, m1, m2, m9 |
| 5. Memory budgets refuse with a message; limits for image sizes and voxel grids | holds for images, voxel worlds and file imports; gaps for edits and device limits | m3, m4 |
| 6. Every loader has a fuzz target, and the nightly job runs them | gaps | M7, M8, m7 |
| 7. GPU: error scopes, device-loss recovery, surface errors reconfigure | error scopes and surface errors hold; device loss has gaps | M1, M2, M3, m6 |
| Deadlock risk (UI thread and workers) | no deadlock found; one starvation stall | M5 |

## Findings

### Critical

None.

### Major

#### M1. A lost GPU device ends the process before the device-loss autosave

- Where: `crates/cuboid-shell/src/event_loop.rs:370-390`. `end_pass`, `handle_platform_output`, `tessellate`
  and `paint_and_update_textures` run after `app.frame` (`:352`), outside any `catch_unwind`. The boundary
  of `crates/cuboid-app/src/app.rs:26-46` covers only `front.frame`.
- Why it panics: egui-wgpu 0.37.0 `src/renderer.rs:1033` and `:1079` call `panic!` when
  `Queue::write_buffer_with` returns `None`. wgpu-core 30.0.1 `src/device/queue.rs:697` refuses every staging
  buffer once the device is invalid. egui-wgpu also has an `expect` in `configure_surface` (`src/winit.rs:143`),
  which it runs again for `Lost` and `Outdated`.
- Scenario: the GPU is reset by a Windows TDR, a driver update, an unplugged eGPU or some sleep and resume
  cycles. The device-lost callback fires during a submit. The next paint has UI vertices, so
  `create_staging_buffer` fails, egui-wgpu panics, and the panic leaves winit's `run_app`. The process ends with
  a crash report only.
  - `State::on_device_lost` (`crates/cuboid-app/src/state.rs:648-655`) never runs. Its flag is read at the start
    of the next app frame (`crates/cuboid-app/src/frontend.rs:237`), and that frame never comes.
  - The work since the last timer autosave is lost (default 10 minutes).
- Fix:
  1. In `redraw`, run everything from `ctx.end_pass()` to the texture frees under
     `catch_unwind(AssertUnwindSafe(..))`.
  2. On a panic, skip the rest of the frame, call a new `ShellApp::paint_failed(&mut self, message: &str)` (the
     default does nothing) and request a redraw.
  3. In cuboid-app, `paint_failed` queues `request_autosave(Reason::Risky)` of the live document. The document
     is not damaged, because the panic was in drawing. Write one autosave per incident, and log it.
  4. Count paint failures with a second `PanicGuard`. On repeated failures, quit through the normal quit path
     after the autosave instead of looping. M2 builds on this.

#### M2. The window never gets a new device, and the device-loss toast cannot be shown

- Where:
  - `crates/cuboid-shell/src/event_loop.rs:291-310` creates the `Painter` and its device only once, in
    `create_window`.
  - `crates/cuboid-app/src/render_glue.rs:64-77` (`follow_device`) is written for a new device, but nothing ever
    hands it one.
  - `crates/cuboid-app/src/state.rs:648-655` shows a toast and asks for a restart on a device that can no longer
    present.
- Scenario: once M1 stops the crash, the window keeps a dead device. Every frame fails and the window shows
  nothing new, including the toast and the unsaved-changes question when the user closes it. Roadmap 7 asks for
  the device and its resources to be recreated. The renderer half exists (`Renderer::rebuild`,
  `crates/cuboid-render/src/gpu.rs:339-357`). The shell half does not.
- Fix:
  1. Add `FrameRequest::recreate_gpu: bool`. The app sets it when `glue.take_device_lost()` fires
     (`frontend.rs:237`) or after `paint_failed`.
  2. Move the painter set-up of `event_loop.rs:291-310` into
     `fn new_painter(&self, window: &Arc<Window>) -> Result<(Painter, RenderState), ShellError>`. On the request,
     drop `win.painter`, build a new one and replace `win.gpu`.
  3. Make egui send its textures again. Call `ctx.set_fonts(..)` with the current definitions, which rebuilds the
     font atlas. The frontend drops its `ImageCache` (`crates/cuboid-ui/src/common.rs:76`). `follow_device`
     already resets the view textures (`frontend.rs:234-236`).
  4. If re-sending egui's textures does not fit in one package, use this fallback: wait for the autosave, show
     the message with `rfd::MessageDialog` (it needs no GPU), and quit through the normal path. The next start
     offers the autosave.

#### M3. The thumbnail renderer takes the device callbacks away from the window renderer

- Where: `crates/cuboid-app/src/thumbs.rs:129` creates a second `Renderer` on the shell's device.
  `Renderer::new` calls `watch` (`crates/cuboid-render/src/gpu.rs:278-290`, `:300`), which sets the device-lost
  callback and the uncaptured-error handler. wgpu keeps only one of each, so the second call replaces the first:
  - wgpu-core 30.0.1 `src/device/global.rs:1932-1943` (`device_lost_closure ... .replace(..)`);
  - wgpu 30.0.1 `src/backend/wgpu_core.rs:1840-1843` (`uncaptured_handler = Some(handler)`).
- Scenario: the start screen draws one recent-file thumbnail. From then on, the window renderer's `lost` flag
  and `last_error` never change: the callbacks point at the rig's `Shared`. That state outlives the rig, which is
  released after `IDLE_RELEASE`. `RenderGlue::take_device_lost` (`render_glue.rs:56-60`) can no longer fire, so
  even with M1 and M2 fixed, the device-loss path depends on the paint failing. The renderer health that tests
  and logs read is also wrong after the first thumbnail.
- Fix: install the callbacks once per device.
  1. Split `watch` and `Shared` out of `Renderer::new` into a `DeviceWatch` (`Arc<Shared>`) that the caller
     creates once.
  2. Give `Renderer::new` a `&Arc<DeviceWatch>` parameter, and have `rebuild` take the new one.
  3. `RenderGlue` owns the window's watch. The rig receives a clone, and `thumbs.rs:102` reads `device_lost()`
     from it.
  4. Add a test: two renderers on one device both report a simulated loss (`device.destroy()` on a headless
     device).

#### M4. The MCP server has no panic boundary

- Where: `crates/cuboid-mcp/src/protocol.rs:338` calls `server.call(name, arguments)` directly. `serve`
  (`protocol.rs:72-104`) and `crates/cuboid-app/src/mcp.rs:97-125` have no `catch_unwind` either. The tools run
  core commands, importers, the validator and the renderer on values from the agent. cuboid-mcp, cuboid-core,
  cuboid-ops and cuboid-model have no crate-level panic lints.
- Scenario: any panic inside a tool, such as an `assert!` or an index in the core reached by an unusual edit,
  ends the server process (exit 101, crash report). The client then shows the server as failed. Every open model
  (up to `MAX_OPEN` 8, `crates/cuboid-mcp/src/server.rs:23`) loses its edits since the last `project_save` and
  its checkpoints (`MAX_CHECKPOINTS`, `:25`). The user's files are safe, because writes are atomic.
- Fix:
  1. In `Connection::call`, wrap `server.call(..)` in `std::panic::catch_unwind(AssertUnwindSafe(..))`.
  2. Turn a panic into an `isError: true` result from a new `ToolError::internal(message)`.
  3. Close the model that the arguments name (`arguments.model`), or every model when none is named, because its
     state may be half applied. Say so in the hint. The other models keep their edits.
  4. Add a test through a test-only failing tool, or a `Server` flag under `cfg(test)`: the reply is an error,
     and the next request is served.

#### M5. UI-thread `par_iter` waits behind background work in the shared rayon pool

- Where:
  - UI thread: `crates/cuboid-scene/src/extract.rs:805-811` (chunk meshing in `extract_voxels`) and
    `crates/cuboid-voxel/src/world.rs:148-175` (`from_snapshot`, which undo runs through
    `crates/cuboid-model/src/apply.rs:484`).
  - Background: `crates/cuboid-fmt-cubik/src/encode.rs:438` and `save.rs:618` (save and autosave on the I/O
    thread), `load.rs:607` and `decode.rs:1086` (open, library thumbnails and imports).
  - There is no `ThreadPoolBuilder` anywhere, so all of them share the global pool. rayon-core 1.13.0
    `src/registry.rs:833-842`: a worker takes an injected job, such as a call from the UI thread, only after its
    own deque and stealing find nothing.
- Scenario: the timer autosaves a large voxel model, such as the 150 MB payload benchmark of roadmap 8, and the
  encode fills every worker. The user paints a voxel stroke, and `extract_voxels` calls `par_iter` from the UI
  thread. The UI thread blocks on a latch until the workers have drained the whole encode. That frame takes as
  long as the rest of the save, which can be seconds, against "no frame above 50 ms". The same happens during a
  library import of many models. No deadlock is possible, because the background jobs never wait for the UI.
- Fix:
  1. Run background jobs in their own pool: a `static BACKGROUND: OnceLock<rayon::ThreadPool>` with
     `available_parallelism - 1` threads (at least 1), named `cuboid-bg-N`.
  2. In `IoThread::spawn` (`crates/cuboid-app/src/io_thread.rs:19-33`), run each task as
     `BACKGROUND.install(task)`. Every `par_iter` inside a job then uses that pool. The UI thread keeps the global
     pool.
  3. Both I/O threads, the document one and the library one (`crates/cuboid-app/src/library.rs:321`), come from
     `IoThread::spawn`, so this covers both.
  4. Add `rayon.workspace = true` to `crates/cuboid-app/Cargo.toml`.
  - Note for R2: its section 3.3 assumes two pools already exist. Today there is one, and this fix creates the
    background pool that R2 would cap.

#### M6. The UI thread stats recent files every 30 s

- Where:
  - `crates/cuboid-app/src/frontend.rs:76-91`: `RecentTimes::modified` calls `std::fs::metadata` on the UI
    thread. The cache lasts 30 s (`:72`). `run_ui` calls it for every recent entry on every frame (`:354-366`),
    whether the start screen is visible or not.
  - The same class: `crates/cuboid-app/src/thumbs.rs:165` (`fs::metadata` of the model before a thumbnail job,
    recent files included) and `crates/cuboid-app/src/flows.rs:763` (`exists()` after Save As).
- Scenario: a recent file is on a network share or a mapped drive that is offline, for example a laptop away
  from the office network or a NAS that is asleep. Every 30 s, the UI thread waits out the SMB timeout for each
  such entry, which takes tens of seconds on Windows. The window freezes ("Not responding") while the user edits.
- Fix:
  1. Do the lookup off the UI thread, like the export dialog does (`crates/cuboid-ui/src/export_dialog.rs:364-371`).
     `RecentTimes` sends the expired paths to one helper thread, at most one batch at a time, and keeps showing
     the old value until the answer arrives.
  2. Drop the size check at `thumbs.rs:165`. `open_file` already refuses files over the limits inside the job
     (`flows.rs:395-405`, and `load_path` for `.cubik`).
  3. Move the `exists()` of `flows.rs:763` into the same helper, or accept it, since it runs once right after a
     dialog.

#### M7. The nightly job fuzzes 8 of the 11 targets

- Where: `.github/workflows/nightly.yml:23` lists `bson, image, dxt, cubik, cub, jsonlite, mcpe_json, lnk`.
  `fuzz/Cargo.toml` also has `bbmodel`, `minecraft_json` and `vox`, which nothing runs. The comment at
  `nightly.yml:16-17` ("Placeholder: every target calls nothing ...") is stale.
- Scenario: three loaders, which the L5 smoke review (finding 12) asked to have fuzzed, are never fuzzed,
  although their targets exist. Two of these are the loaders where L5 found an allocation without bound. A
  regression in them reaches a release unseen.
- Fix:
  1. Add `bbmodel, minecraft_json, vox` to the matrix.
  2. Pass `-rss_limit_mb=2048`, which the three targets ask for in their comments
     (`fuzz/fuzz_targets/bbmodel.rs:18`).
  3. Replace the stale comment.
  4. Add a step `actions/upload-artifact` of `fuzz/artifacts/${{ matrix.target }}` that runs `if: failure()`.

#### M8. Several parsers of external data have no fuzz target

- Where, with the parser and the file it reads:
  - Settings: `cuboid_settings::Settings::parse` (`crates/cuboid-settings/src/store.rs:117-126`), for
    `settings.json` and the legacy `cubikdata.json`.
  - Preferences: `cuboid_ui::Prefs::from_json` (`crates/cuboid-ui/src/prefs.rs:280`).
  - Library: the index (`crates/cuboid-library/src/store.rs:225-226`), `kind::classify` (`kind.rs:159`, `:202`),
    `meta::model_meta` (`meta.rs:186`), `texture_meta` with `.mcmeta` (`:537`, `:573`) and `lore_meta` (`:698`).
    The L5 smoke review (finding 12) asked for `classify` and `model_meta`.
  - MCP requests: `Connection::handle` (`crates/cuboid-mcp/src/protocol.rs:180-186`) and the typed tool
    arguments (`crates/cuboid-mcp/src/tools.rs:44`, `:58`).
  - The `.cubik` extension entry `cuboid.json` (`crates/cuboid-fmt-cubik/src/extension.rs:163-181`, with the
    tree, animation and frame decoders): the `cubik` target reaches it only through a ZIP with a correct CRC,
    which a fuzzer almost never builds, and `container.rs:90-92` drops a corrupt entry quietly.
  - Minecraft parent chains, texture files and `.mcmeta` (`crates/cuboid-fmt-minecraft/src/json_import/parents.rs`,
    `mcmeta.rs:29`): `minecraft_json` uses `NoFiles`, so none of these are ever read.
- Scenario: a malformed index, settings file, library file or agent request can only meet the hand-written
  hostile tests. Roadmap 7 and l6 section 1 say every loader has a target.
- Fix: add these targets to `fuzz/` with the crates as dependencies, and add them to the nightly matrix:
  - `settings`: `Settings::parse` with a fixed `Environment`.
  - `prefs`: `Prefs::from_json` on lossy UTF-8.
  - `library_file`: `classify`, `model_meta`, `texture_meta` (the first byte splits the input into a PNG and an
    mcmeta) and `lore_meta`.
  - `library_index`: expose a `#[doc(hidden)] pub fn parse_index(bytes)` around `store.rs:225`.
  - `mcp_requests`: `cuboid_mcp::serve` with the input as the request stream, `--read-only`, an empty temporary
    root made once, and a stub `Renderer`.
  - `cubik_extension`: wrap the input with `container::write(minimal_payload, Some(data))`, then `load_bytes`.
  - `minecraft_files`: an in-memory `TextureFiles` that serves parent JSON, a PNG and an `.mcmeta` cut from the
    input.

### Minor

#### m1. A failing stderr or stdout write panics, and inside the panic hook that is an abort

- Where:
  - `crates/cuboid-app/src/logging.rs:178-180` echoes every warning and error with `eprint!`, which panics when
    the write fails.
  - The hook logs again (`crates/cuboid-app/src/crash.rs:58-60`), and so does the recovery
    (`crates/cuboid-app/src/state.rs:592`, outside `crash::catch`).
  - `crates/cuboid-app/src/lifecycle.rs:65` and `:69` use `println!` for `--version` and `--help`.
    `crates/cuboid-app/src/mcp.rs:84-95` already handles a closed stdout for the same reason.
- Scenario: stderr is a pipe whose reader is gone, for example `cuboid model.cubik 2>&1 | tee run.log` after tee
  stopped. The next warning panics. The hook's `log::error!` panics again inside the panic hook, and the process
  aborts with no recovery and no autosave. The same may happen after R1 when the console that `cuboid.com`
  relayed is closed while `cuboid.exe` keeps running. I have not verified which error Windows returns there; std
  ignores only `ERROR_INVALID_HANDLE`.
- Fix: in `Logger::log`, use `let _ = std::io::stderr().write_all(line.as_bytes());`. Route `--version` and
  `--help` through the `write_stdout` of mcp.rs.
- Status: by the end of this audit, R1's uncommitted working tree, which ran in parallel, already made exactly
  these changes in `logging.rs` and `lifecycle.rs`. The R1 review should confirm them and close m1. The line
  numbers above are those of the committed code.

#### m2. `request_discard` after a caught panic has no effect

- Where: `crates/cuboid-app/src/app.rs:101-105` asks `request_discard` ("the shell should not show it"). The shell
  drives egui with `begin_pass` and `end_pass` (`crates/cuboid-shell/src/event_loop.rs:351`, `:370`) and paints
  whatever the pass holds. egui honours a discard only in the multi-pass loop of `Context::run` (egui 0.37.0
  `src/context.rs:2321-2347`).
- Scenario: for one frame after a caught panic, the half-built UI of that frame is shown, with panels cut off
  partway through layout. Only the look is affected.
- Fix: after `app.frame`, check `ctx.will_discard()`. If it is set, end the pass, apply the texture deltas as the
  minimised branch does (`event_loop.rs:373-380`), skip tessellation and painting, and redraw at once.

#### m3. Texture sizes are not checked against the device limit

- Where:
  - `crates/cuboid-ui/src/common.rs:63` and `:85-87`: `ImageCache` accepts sides up to 16384. The device limit
    (`painter.max_texture_side()`, passed to egui at `event_loop.rs:323`) is often 8192 on older integrated
    GPUs and on GLES or DX11-level adapters.
  - `crates/cuboid-render/src/meshes.rs:384-420` creates the mesh texture at the image size, outside `guarded`
    and without checking `max_texture_dimension_2d`.
- Scenario: a 16384 x 1024 texture, which `check_size` accepts, on an 8192 adapter.
  - Texture creation fails validation on every upload. The uncaptured handler logs it.
  - The texture panel and the UV editor show nothing, and the mesh draws without its picture.
  - In debug builds, egui's `debug_assert` (`src/context.rs:2822-2830`) panics inside the frame. Three uploads
    within 10 s put the editor into safe mode.
- Fix: in `ImageCache`, clamp to `ctx.input(|i| i.max_texture_side)` and downscale larger previews (nearest). In
  `texture_view`, downscale or fall back to the 1 x 1 white texture above the device limit, with a log line, and
  create the texture under `guarded`.

#### m4. Budgets are set per category and per file; nothing bounds the total or the edits

- Where:
  - `crates/cuboid-fmt-cubik/src/lib.rs:91-103`: payload 1 GiB, texels `1 << 28` (1 GiB as RGBA), voxels 1 GiB,
    retained copy 512 MiB.
  - The ZIP reservation follows 1100 times the compressed size (`container.rs:31`, `:114-120`), so a file of a
    few MB may legally inflate to the full payload.
  - No element, mesh or texture count limit exists in cuboid-model or cuboid-core. A search finds only
    `MAX_GROUPS` (`crates/cuboid-model/src/groups.rs:32`) and the history budget.
- Scenario: Duplicate or Paste on a large selection, repeated, doubles the model until an allocation fails. Rust
  then aborts the process without a message. A hostile `.cubik` that uses every category at once peaks at
  several GiB, which aborts on machines with little memory.
- Fix:
  1. Add document limits in cuboid-model: elements and meshes `1 << 18`, like the loaders' `max_objects`, and
     total texels `1 << 28`.
  2. Check them in the commands that add objects or textures (add, duplicate, paste, add texture) and return
     `CmdOutcome::Refused` with a message.
  3. Lower the per-file defaults so that one file peaks under about 2 GiB.

#### m5. Settings, preferences and recovery descriptions are read with no size cap

- Where:
  - `crates/cuboid-settings/src/store.rs:142` (`fs::read` of `settings.json`).
  - `crates/cuboid-app/src/ui_setup.rs:64` (`read_to_string` of the preferences).
  - `crates/cuboid-app/src/backup.rs:244` (`fs::read` of each recovery `.json`, on the I/O thread).
  - Every model loader, by contrast, checks the size first.
- Scenario: the settings file is replaced by a large wrong file, for example by a sync tool or disk damage. It
  is read in full on the main thread before the window opens. A large `.json` in the recovery folder is read
  whole by every scan.
- Fix: read through one helper (metadata, then `take(limit + 1)`): 16 MiB for settings and preferences, 1 MiB
  for recovery descriptions. An oversized settings file goes through the existing `keep_aside`
  (`store.rs:219-232`).

#### m6. During start-up, the shell's device has wgpu's panicking error handler

- Where: the uncaptured-error handler is installed only when the first app frame creates `RenderGlue`
  (`crates/cuboid-app/src/frontend.rs:233` leads to `gpu.rs:286`; `gpu.rs:276-277` notes that the default
  handler panics). `Painter::new` and `set_window` (`event_loop.rs:303-306`) configure the surface and build
  egui's pipelines before that.
- Scenario: a validation error during start-up becomes a panic and a crash report instead of a logged error.
  No document is open, so nothing is lost.
- Fix: install a logging `on_uncaptured_error` on the device right after the painter exists. The renderer, or
  the shared watch of M3, replaces it later.

#### m7. The fuzz crate is not built in CI, and the nightly job keeps no crashing input

- Where: `fuzz` is excluded from the workspace (`Cargo.toml:7`), and `.github/workflows/ci.yml` never builds it.
  `nightly.yml:39` runs the fuzzer but uploads nothing.
- Scenario: an API change in a loader breaks a target without notice until the next nightly, and when a nightly
  run fails, its input is gone.
- Fix: add a Linux CI step `cargo check --manifest-path fuzz/Cargo.toml --locked`; switch it to
  `cargo +nightly fuzz build` if libfuzzer-sys needs nightly. The artifact upload is in M7.

#### m8. Atomic writes leave temporary files behind, and the folder is not synced on Unix

- Where: `crates/cuboid-io/src/atomic.rs:23-30`. `NamedTempFile` deletes itself only on drop, and only the
  recovery folder is cleaned of `.tmp*` files (`crates/cuboid-app/src/backup.rs:277-280`). After `persist`, the
  folder is not synced.
- Scenario: when the process is killed during a save, a `.tmpXXXXXX` file is left next to the user's model. On
  Linux and macOS, a power cut right after a save can bring back the old file. Windows is not affected by the
  second point.
- Fix: create the temporary file with `tempfile::Builder::new().prefix(".cuboid-tmp-")` so it can be recognised,
  and remove stale ones with that prefix next to the target, at least an hour old, before a write. On Unix, call
  `File::open(dir)?.sync_all()` after `persist`. This is low priority before the Linux release.

#### m9. A thumbnail that panics restores the user's document

- Where: the rig draws library and recent-file models inside the frame logic: `crates/cuboid-app/src/frontend.rs:247`
  calls into `thumbs.rs:126-131` and `draw` at `thumbs.rs:192-209`.
- Scenario: a library model makes the extractor or the renderer panic. The frame boundary catches it and
  restores the user's open document to its last good state, which can drop up to 250 ms of edits
  (`crates/cuboid-app/src/editor.rs:21`). It also shows "the last action was dropped". Three such thumbnails
  within 10 s put the editor into safe mode.
- Fix: run the rig's `draw` and readback under `crash::catch`. On a panic, drop the rig renderer and call
  `render_done(request, None)`. Do not touch the document or the panic counter.

#### m10. Three crates that parse external data have no crate-level panic lints

- Where: the format crates carry
  `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]`
  (for example `crates/cuboid-io/src/lib.rs:43-51`). cuboid-library (`src/lib.rs:29`), cuboid-settings
  (`src/lib.rs:15`) and cuboid-mcp (`src/lib.rs`) do not.
- Scenario: I found no panic on external data in them. The indexing in `cuboid-library/src/meta.rs:738-789` and
  in `cuboid-mcp/src/select.rs:157-187` and `texture.rs:307` is bounded. But nothing keeps it that way.
- Fix: add the block to the three `lib.rs` files and replace whatever clippy then reports with `get` or `?`.

#### m11. Labels from `.cubik` files have no length limit

- Where: `crates/cuboid-fmt-cubik/src/load.rs:271-273` and `:285` accept any BSON string (payload up to 1 GiB)
  as a label. The `.bbmodel` importer cuts names and charges a label budget
  (`crates/cuboid-fmt-bbmodel/src/import.rs:598-616`). `galley` in `crates/cuboid-ui-kit/src/widgets/paint.rs:12-15`
  lays out the whole text. The tree rows use `galley_cut`, which still hashes and measures the whole text.
- Scenario (not run): a `.cubik` whose element label is 50 MB opens and is kept for payload identity. Every
  frame that shows the label then hashes and lays out 50 MB, so frames take a long time. A full-width text field
  could also emit one quad per glyph.
- Fix: cut labels for display only, and keep the stored label for the re-save. Add a helper in cuboid-ui-kit
  that clips to 256 characters with an ellipsis before layout, and use it in `galley`, `galley_cut` and the label
  fields.

## Checked and sound

Loaders (rule 1):

- No panicking calls. The format and data crates deny `unwrap`, `expect`, `panic!` and indexing outside tests
  (`crates/cuboid-{bson,image,io,fmt-cubik,fmt-bbmodel,fmt-minecraft,fmt-voxel,fmt-mesh,voxel,validate}/src/lib.rs`),
  with no `allow` exemption anywhere. A search of their non-test code finds no `unwrap`, `expect`, `panic!` or
  `unreachable!`. The divisions, `chunks_exact` widths, `split_at` offsets and modulo bases I checked are all
  guarded: `cuboid-image` contract.rs:150-175, bc.rs:102-108, pack.rs:144-152 and file.rs:180-192; `cuboid-fmt-bbmodel`
  signs.rs:140-245.
- Sizes are checked before allocating:
  - Images: the header size goes through `check_size` (`crates/cuboid-image/src/contract.rs:19-33`), the image
    crate has limits (`file.rs:150-165`), and DDS and TGA go through their own checks (fuzzed).
  - The ZIP entry reservation is bounded by the compressed size and read through `take(max + 1)`
    (`crates/cuboid-fmt-cubik/src/container.rs:96-130`).
  - Texture files: metadata first, regular files only, network and device paths refused, read through `take`
    (`crates/cuboid-io/src/texture_files.rs:44-81`).
  - `.bbmodel`: separators are counted before `serde_json` builds the tree, and labels are cut and charged
    (`crates/cuboid-fmt-bbmodel/src/import.rs:171-194`, `:598-616`).
  - Voxel worlds: `voxel_world_bytes` and the 1 GiB budget are checked in every loader and in voxel edits
    (`crates/cuboid-model/src/apply.rs:26-30`, `:284-301`; `crates/cuboid-fmt-cubik/src/load.rs:557-568`;
    `crates/cuboid-fmt-voxel/src/vox/import.rs:275`, `:313`; `crates/cuboid-ops/src/voxel/edit.rs:49-52`).
  - The library index is capped before it is read (`crates/cuboid-library/src/store.rs:444-461`), and so are
    thumbnails and references (`crates/cuboid-library/src/thumb.rs:103-128`).
  - MCP: lines are capped at 16 MiB (`crates/cuboid-mcp/src/protocol.rs:31`, `:118-146`), the JSON in the
    project listing is capped (`project.rs:250-262`), and the tool inputs have limits (texture 1024, view 2048,
    grid 512, 500 operations).
- Nesting is bounded:
  - `serde_json` stops at its default depth of 128.
  - jsonlite and tolerant stop at `MAX_DEPTH` 128 (`crates/cuboid-io/src/text/mod.rs:28`, `jsonlite.rs:94`,
    `:122`, `tolerant.rs:125`, `:150`).
  - BSON stops at `max_depth` (`crates/cuboid-fmt-cubik/src/lib.rs:95`).
  - Groups stop at `MAX_DEPTH` 64 (`crates/cuboid-model/src/groups.rs:35`), and so does the `.bbmodel` outliner
    (`import.rs:470`).
  - Minecraft parents stop at `max_parents`, with a loop guard (`json_import/parents.rs:46-60`).
  - The library walk stops at depth 32 and keeps symbolic links inside the chosen folder
    (`crates/cuboid-library/src/import.rs:39-49`, `:481-541`).
  - There is no Molang evaluator, so no recursion over expressions.
- The only decompression is the `.cubik` ZIP entry and PNG through the image crate with limits. Nothing else
  inflates data.
- Open, import and apply run inside the I/O job (`crates/cuboid-app/src/flows.rs:409-500`), so a panic in an
  importer or in `apply_document` comes back as `JobError::Panicked`.

Atomic writes (rule 2):

- `write_atomic` (`crates/cuboid-io/src/atomic.rs:17-32`) writes a temporary file in the target folder, calls
  `sync_all`, and renames it over the target.
- These paths use it: saves and exports (`flows.rs:263-390`), `.cubik` saves
  (`crates/cuboid-fmt-cubik/src/lib.rs:259-268`), mesh and resource-pack files, autosaves (`backup.rs:171-199`,
  where the `.json` is written last as the commit marker), settings (their own copy, `store.rs:234-245`),
  preferences (`ui_setup.rs:76-81`), library index, imports, thumbnails and `brief.md`, pins (`library.rs:1481-1488`),
  PNG renders (`frontend/render_job.rs:302-316`), MCP exports and renders (`project.rs:588`, `views.rs:229`) and
  the CLI round trip.
- Accepted exceptions, by their nature: crash reports (`crash.rs:99`, diagnostic), the log files (append and
  rotate), session locks (`backup.rs:133-158`) and the MCP journal (append, `server.rs:303-325`).

Off the UI thread (rule 3):

- Open, save, Save As, export, autosave, the recovery scan, texture reload, the settings write and PNG encoding
  are all `io.submit` jobs (`flows.rs:853-948`, `state.rs:237-241`, `:343-380`, `:555-582`; `reload.rs:32-48`;
  `render_job.rs:302`).
- The library has its own I/O thread (`library.rs:321`).
- Readbacks are polled with `try_recv` and `PollType::Poll` (`crates/cuboid-render/src/readback.rs:90-120`,
  `:203`).
- File dialogs and existence checks run on threads (`flows.rs:815`, `library.rs:729`, `export_dialog.rs:365`).
- Quit waits for a running document job (`flows.rs:569-587`), and `IoThread` drains its queue on drop
  (`io_thread.rs:59-80`).
- Settings, preferences and fonts are read on the main thread before the window exists, which is start-up work.

Frame boundary (rule 4):

- `run_guarded` (`app.rs:26-46`) makes a frame's document the last good one only when the frame finished
  (`editor.rs:181-195`, at most every 250 ms).
- `recover_from_panic` (`state.rs:591-626`):
  - restores the document under `crash::catch` (`editor.rs:199-214`, history and selection cleared by design);
  - queues one autosave of the last good state (`state.rs:630-645`), never in safe mode;
  - shows the toast.
- The panic hook writes the report (`crash.rs:25-61`) and uses `try_lock` so that it cannot deadlock on the
  logger or the report folder (`crash.rs:40-44`, `logging.rs:50-61`).
- Three panics within 10 s (`crash.rs:163-201`) switch to safe mode. Safe mode reads no document
  (`frontend.rs:282-305`, `:368-373`), and its scan opens the recovery dialog (`state.rs:279-282`, `:465-494`).
- Jobs are isolated: `run_isolated` (`jobs.rs:102-111`), and rayon brings worker panics back to the caller. No
  `rayon::spawn` exists.
- Release builds keep `panic = "unwind"` (`Cargo.toml:137`).

Memory budgets (rule 5):

- Images: `MAX_SIDE` 16384 and `MAX_TEXELS` `1 << 26` (`contract.rs:19-33`), also in texture operations
  (`crates/cuboid-ops/src/textures/mod.rs:23`, `:147`, `:249`).
- Voxels: `MAX_VOXEL_RES` 254, with the 1 GiB world budget that refuses with a message.
- History: 256 entries and 256 MiB (`crates/cuboid-core/src/history.rs:13-17`).
- GPU buffers grow only within the device limits, and refuse with a log line (`crates/cuboid-render/src/elements.rs:100-122`).

GPU (rule 7):

- Every buffer, texture and pipeline creation in the renderer runs under OOM and validation error scopes
  (`crates/cuboid-render/src/util.rs:27-45`, used by atlas, elements, voxels, meshes, targets, offscreen and
  pipelines).
- The device-lost callback and the uncaptured-error handler log and set flags, never panic (`gpu.rs:276-290`).
- `Renderer::rebuild` re-uploads from the shadow copies (`gpu.rs:339-357`).
- The MCP renderer opens a lost device again (`crates/cuboid-app/src/mcp_render.rs:98-100`), with a 30 s
  readback timeout (`:31`).
- The rig drops its renderer on a lost or changed device (`thumbs.rs:99-108`).
- Surface errors: the shell keeps egui-wgpu's default `on_surface_status` (`event_loop.rs:299-302`): `Outdated`
  reconfigures, `Lost` recreates the surface, and `Occluded` and `Timeout` skip the frame (egui-wgpu 0.37.0
  `src/lib.rs:389-411`, `src/winit.rs:605-630`).

Deadlocks:

- No mutex is held across a blocking call or a wgpu call: the logger sink, the report folder and the renderer's
  error slot.
- The UI thread never waits on a channel (`try_recv` everywhere). The only wait between threads is
  `IoThread::shutdown` at exit, at most 3 s.
- `egui_wgpu` renderer locks are taken one after another on the UI thread, never nested with a wait.
- The one starvation is M5.
