# Review: L6 R4-A, device loss, panic boundaries and the paint loop

Status: 2026-10-28. Reviewer: Opus. Scope: the uncommitted R4-A of `docs/design/l6_release.md` section 5, the fixes of
M1, M2, M3, m2, m3, m6, m9 and step 2 of M6 of `docs/review/l6_r4_audit.md`, in `crates/cuboid-shell/src/{event_loop,
contract,headless}.rs`, `crates/cuboid-render/src/{gpu,meshes,lib,tests}.rs`, `crates/cuboid-app/src/{app,state,
render_glue,thumbs,crash,tests}.rs`, the `rig.step` call in `crates/cuboid-app/src/frontend.rs` and
`crates/cuboid-ui/src/common.rs`. Read against egui 0.37.0, egui-wgpu 0.37.0, wgpu 30.0.1, wgpu-core 30.0.1 and
wgpu-hal 30.0.1 in the cargo registry. Run (`df -h /d`: 8.7 GB free at the start):
`cargo clippy -p cuboid-shell -p cuboid-render -p cuboid-app -p cuboid-ui --all-targets -j 4 -- -D warnings` (clean),
`cargo test -p cuboid-shell -p cuboid-render -j 4` (see Gates), `cargo test -p cuboid-app -j 4 -- tests:: thumbs
render_glue state` and `cargo test -p cuboid-ui -j 4 common` (4 pass). Not run: the GUI, release builds, a real device
loss, the workspace tests.

Verdict: ready after two small majors. The shape of the package is right. The paint path from `end_pass` to the texture
frees is inside a boundary, a failure keeps the work once per revision, the shell rebuilds the painter and device with
the old swap chain gone first, egui's font atlas goes into the new renderer whole, the one `TextureHandle` owner of the
UI clears on the epoch, the view textures are registered again, one `DeviceWatch` serves every renderer of a device,
and the retry cannot spin. Two things keep it from closing M1 and M6: the resize handler still calls the egui-wgpu
`expect` that M1 names, outside any boundary, and the removal of the thumbnail stat also removed the 32 MiB policy, so
every save of a large model is decoded and uploaded a second time for a 160 px picture. Of the three minors, the
one to take with the majors is Minor 5: in debug builds a paint panic can turn into an abort through epaint's
`TexturesDelta` assertion.

## Major

1. `crates/cuboid-shell/src/event_loop.rs:637-644`. `WindowEvent::Resized` calls `Painter::on_window_resized`
   outside `guarded`. In egui-wgpu 0.37.0 that is `resize_and_generate_depth_texture_view_and_msaa_view`
   (`src/winit.rs:462-481`, `:338-357`) and then `configure_surface`, whose
   `get_default_config(..).expect("The surface isn't supported by this adapter")` (`src/winit.rs:131-143`) is the
   panic M1 cites. wgpu turns a failed capability query into empty capabilities (`src/backend/wgpu_core.rs:3960-3967`,
   `unwrap_or_default`), so `get_default_config` returns `None`. The Vulkan backend fails that query whenever
   `vkGetPhysicalDeviceSurfaceSupport/Capabilities/PresentModes` fails, for example on a lost surface
   (wgpu-hal 30.0.1 `src/vulkan/swapchain/native.rs:60-130`); DX12 reports capabilities from the window rectangle and
   fails only when `GetClientRect` fails. Scenario: a driver reset or a display change loses the surface, and before
   the next paint the window gets a resize, a restore from minimised, a maximize, or a monitor change (the
   `ScaleFactorChanged` size request ends in `Resized`). The panic leaves `window_event`, winit resumes it out of
   `run_app`, and the process ends with a crash report. The device-lost flag is read only by the next app frame, so no
   Risky autosave was queued: the M1 scenario through another door, on events that often come with a GPU reset.
   Fix: run the call under the same boundary and let the next frame do the rest:

   ```rust
   if let (Some(width), Some(height), Some(painter)) = (
       NonZeroU32::new(size.width),
       NonZeroU32::new(size.height),
       win.painter.as_mut(),
   ) {
       let resized = guarded(|| {
           painter.on_window_resized(ViewportId::ROOT, width, height);
           Ok(())
       });
       if let Err(message) = resized {
           // The next frame has no painter: it reports the failure and builds a new one.
           log::error!("cannot resize the surface: {message}");
           win.painter = None;
       }
   }
   ```

   `Resized` already sets `schedule.changed`, so the next redraw comes at once, `present` returns "the window has no
   painter", `paint_failed` keeps the work, and the painter is made again.

2. `crates/cuboid-app/src/thumbs.rs:185-195`. M6 asked to take the `fs::metadata` off the UI thread. The change also
   dropped the policy behind it: models over 32 MiB on disk got no picture, now every model up to the loader limits
   gets one (`load_path(path, &Limits::default())`, payload up to 1 GiB, `crates/cuboid-app/src/flows.rs:416`). Cost
   per picture: the library I/O thread decodes the whole model, the UI thread extracts it over several frames
   (`ExtractBudget::default()`), and the rig's second renderer uploads all of it again and keeps its shadow copies, so
   RAM and VRAM hold the model about twice more while the user's own document stays open. It is not only the start
   screen: every Save and Save As queues a picture of the saved file (`flows.rs:1065`, `library.rs:1130-1135`), so a
   user who saves a 300 MB voxel model every few minutes pays a full background reload each time, and a model whose
   picture fails is tried again at each start (`Thumb::Missing` lives for one session, `library.rs:1139-1143`). A
   library of large models queues up to 64 of these (`MAX_RENDERS`). This works against the RAM goal of R2 (idle
   150 MB) and, on a machine with little memory, adds an allocation failure risk in a background feature. Fix: keep
   `MAX_MODEL_BYTES` (`32 << 20`) and measure inside the job, on the library I/O thread, where the Loading step already
   turns an error into `render_done(None)`:

   ```rust
   .map(|io| {
       io.submit(move |ctx| {
           let bytes = std::fs::metadata(&source)
               .map_err(|error| JobError::Failed(error.to_string()))?
               .len();
           if bytes > MAX_MODEL_BYTES {
               return Err(JobError::Failed(format!("{bytes} bytes is too large for a picture")));
           }
           open_file(&source, None, &env, ctx)
       })
   })
   ```

   The UI thread then touches no file, which is all M6 step 2 needs.

## Minor

3. `crates/cuboid-app/src/state.rs:665-672` with `crates/cuboid-app/src/io_thread.rs:106-110`. The exit after
   repeated paint failures does not wait for the work it queued. The loop ends at the next frame (500 ms), and
   `IoThread::drop` waits at most 3 s. The I/O queue is FIFO, so a running Save or export, or a timer autosave, can be
   ahead of the Risky autosave. A Risky autosave queued at the eighth failure also gets only that 3.5 s: frames still
   run every 500 ms and still apply the user's input, so the revision can change between failures. For a large model
   the write is then cut. The write is atomic, so the old file survives, but the newest work is lost and a temporary
   file is left behind. In the common case the first autosave has 7 s or more, which is why this is minor. Fix: on
   `repeated`, set `self.leave_by = Some(self.now + LEAVE_WAIT)` (60 s) instead of `quit`, and in `State::step` set
   `quit` once `self.autosaves.is_empty()` and no save job runs, or once `now` passes `leave_by`. The frames keep
   coming at the retry pace, so no new timer is needed. This works like `when_idle` of the normal quit
   (`flows.rs:562-575`). Test: a paint failure while a slow job is queued keeps `quit` false until the queue is
   empty.

4. `crates/cuboid-app/src/state.rs:681-689` and `crates/cuboid-shell/src/event_loop.rs:462`. The frame that quits
   still keeps work and still builds a device. (a) After "Don't save", `finish_quit` cancels the autosaves and queues
   the purge (`flows.rs:711-737`). If the paint of that last frame fails, `keep_work` queues a Risky autosave behind
   the purge, the drain on exit writes it, and the next start offers the document the user discarded. (b) Any
   quitting frame whose paint fails, including the leave after repeated failures, runs `new_painter`
   (`pollster::block_on` of the adapter and device requests on a failing GPU) right before the loop ends. Both are
   rare, and both are cheap to close: `if self.quit { return ""; }` at the top of `keep_work`, and
   `let recreated = (!request.quit && (painted.is_err() || request.recreate_gpu)).then(..)`.

5. `crates/cuboid-shell/src/event_loop.rs:503-537`. In debug builds a paint panic can abort the process instead of
   being caught. epaint 0.37.0 asserts in `Drop for TexturesDelta` (`src/textures.rs:335-344`, `debug_assert!`) that
   every delta was applied. egui-wgpu drains `textures_delta.set` before `update_buffers` (`winit.rs:569`) but drains
   `free` only after the submit (`:775`). So when `update_buffers` panics (`renderer.rs:1033`, `:1079`) in a pass that
   freed a texture, unwinding drops `out` with frees left in it, the assertion panics during the unwind, and Rust
   aborts. A pass that frees textures is likely in the failure series: the epoch clear of `ImageCache` frees every
   picture in the first pass after a recreate. The early return `ok_or("the window has no painter")?` drops the same
   non-empty delta on the normal path. In debug builds the assertion text then replaces the error message. Release
   builds have no assertion and are not affected, but debug builds are where this recovery gets tried. The same
   assertion failed in my run in R4-B's
   `frontend::recent::tests::a_pass_with_a_batch_out_asks_to_come_back_and_an_idle_one_does_not` (see Gates). Fix: hold the delta in a wrapper that clears it when it is dropped. The renderer that missed those
   changes is replaced anyway:

   ```rust
   /// Texture changes of a pass. When the painting fails they are dropped: the renderer that
   /// missed them is replaced, and epaint asserts in debug builds that a delta was applied.
   struct Deltas(egui::TexturesDelta);

   impl Drop for Deltas {
       fn drop(&mut self) {
           self.0.clear();
       }
   }
   ```

   In `present`, `let mut textures = Deltas(std::mem::take(&mut out.textures_delta));` and use `&mut textures.0` for
   the painter, the minimised path and the frees.

## Nit

6. `crates/cuboid-shell/src/event_loop.rs:240-242`. `recreate_gpu` installs the painter before it sends the font
   atlas. If `send_egui_textures` panics, the window has a painter whose renderer has no font texture. egui-wgpu then
   skips every text mesh with a "Missing texture" warning (`renderer.rs:572-588`), and no retry follows until a new
   glyph makes the partial atlas update panic. Install the painter last:
   `self.gpu = gpu; send_egui_textures(ctx, &self.gpu); self.painter = Some(painter);`. A panic then leaves no painter,
   and the next frame retries.
7. `crates/cuboid-app/src/state.rs:651-677`. One device loss usually gives two error toasts in the same frame, "The
   graphics device was lost ..." from `on_device_lost` and "The window could not be drawn ..." from the paint that
   fails right after. Skip the second toast when a GPU toast was given at the same `self.now`, for example with a
   `gpu_toast_at: Option<Instant>` field.
8. `crates/cuboid-app/src/thumbs.rs:105-118`, `crates/cuboid-app/src/frontend.rs:217-227`. While `watch.lost()` is
   set, the rig drops its renderer, then starts the next request and builds a new renderer on the lost device. So the
   models queued during the loss window are marked missing for the session. In safe mode the rig never steps, so after
   a recreate its renderer keeps the old device alive. Return from `work` after the drop when `watch.lost()`, and
   release the rig's renderer when `follow_device` returns true (a `ThumbRig::release`), before `update`.
9. `crates/cuboid-render/src/meshes.rs:452`. The bytes recorded for a mesh texture are those of the image before
   `fit`. For a texture that was cut down, health and the memory budget over-count. Use the uploaded size:
   `u64::from(size.0) * u64::from(size.1) * 4`.
10. `crates/cuboid-shell/src/contract.rs:151`, `crates/cuboid-ui/src/common.rs:69`. Each crate checks its own copy of
    the key against the literal. A change on one side, with its test updated, passes both tests and silently turns
    the epoch off. Make the cuboid-ui constant `pub` and replace the two literal tests with one in cuboid-app, which
    depends on both: `assert_eq!(cuboid_shell::GPU_EPOCH, cuboid_ui::GPU_EPOCH)`.
11. `crates/cuboid-shell/src/event_loop.rs:449-492`. The decisions after the paint have no test: the retry, the
    discard limit, recreate-on-failure, and the wake. Only `guarded` is tested. Move them into a pure function of
    (paint result, recreate result, discard, the two wakes, `now`) that returns `retry_at` and the wake, and test it:
    a failure waits `PAINT_RETRY`, a discard asks for `now` but never twice in a row with `max_passes` 2, and a
    success clears the retry.
12. `crates/cuboid-shell/src/event_loop.rs:196-204` (m6). The logging handler comes after `set_window`. egui's
    pipelines and the first surface configuration of a new device still run under wgpu's panicking default handler.
    At start-up there is no document. On a recreate it is inside `guarded`, so it counts as one paint failure. That is
    acceptable. The full fix, if a start-up crash is ever reported, is `WgpuSetup::Existing`: the shell creates the
    instance, adapter and device, installs the handler, then hands them to egui-wgpu.

## The nine points

| Point | Verdict |
|---|---|
| 1. `present` under `catch_unwind`, `paint_failed`, retry, second `PanicGuard`, quit without `finish_quit` | Sound in design. The autosave usually lands but is not waited for (Minor 3). The quitting frame can add a stray autosave (Minor 4). egui's context, `AssertUnwindSafe` and the absence of a busy loop are sound in release builds: see below. In debug builds a paint panic with pending frees aborts (Minor 5). The resize handler is a hole in the boundary (Major 1). |
| 2. M2 full recreate | Sound. Every egui texture is valid after the recreate, and nothing keeps the old device past the next frame, except the rig in safe mode (Nit 8). Painting under surface `Lost`/`Outdated` is now guarded, but the resize path still reaches the `expect` (Major 1). Install the painter last (Nit 6). |
| 3. M3 `DeviceWatch` | Sound. After a recreate the rig gets the new watch: `follow_device` replaces `glue.watch` before `rig.step` in the same frame (`frontend.rs:210-226`). |
| 4. m2 discard | Sound. egui's own count restarts at 0 in a begin/end loop, so the shell's counter is what limits discards to `max_passes - 1` in a row. |
| 5. m3 device limit | Sound. The implementer is right: `texture_view` runs inside `guarded(device, "mesh buffers", ..)` (`meshes.rs:477-503`). Byte count: Nit 9. |
| 6. m6 early handler | Partial, accepted (Nit 12). |
| 7. m9 rig under `crash::catch` | Sound. `fail_draw` and its assert are `cfg(test)` only, so release builds are unchanged. |
| 8. M6 step 2 | Not accepted as is: Major 2. |
| 9. No native message on the repeated-failure exit | Agreed. A modal `rfd` dialog inside a winit callback re-enters the event loop. The next start shows the recovery dialog, and the log has the reason. A message after `cuboid_shell::run` returns (`lifecycle.rs:134`), when the window and the loop are gone, would be safe if the owner wants one: the app leaves a note in an `Arc<OnceLock<String>>` cloned before `run`. Optional. |

## Checked and sound

- The paint boundary. `guarded` covers `end_pass`, the platform output, tessellation, `paint_and_update_textures`,
  the texture writes of the path that does not draw, and the frees (`event_loop.rs:456`, `:503-537`). The recreate has
  its own boundary (`:462-467`). The panics of egui-wgpu that M1 lists are reached from inside it: `renderer.rs:1033`
  and `:1079`, and the `expect`s at `:699-701` and `winit.rs:143`. wgpu's `ErrorScopeGuard` pops on drop
  (`src/api/device.rs:980-985`), so a panic inside the renderer's `guarded` leaves no error scope pushed on the shared
  device.
- egui after a caught paint panic. The panics come after `end_pass` returned, so the pass is closed and the next
  `begin_pass` starts clean. egui's locks are parking_lot locks and are not poisoned. Only a panic inside `end_pass`
  itself would leave an extra entry on egui's viewport stack (`context.rs:473`, `:3280-3284`). egui would then stop
  sending viewport commands, which the shell does not use. No plugin or app code runs in `end_pass`.
- `AssertUnwindSafe`. After a panic, the recreate replaces the painter and the `RenderState`. So a half-updated
  egui-wgpu renderer (for example its `textures` map between `remove` and `insert`, `renderer.rs:694-699`) is never
  painted with again. If the recreate fails, the old renderer only takes texture writes on the minimised path, and
  that path is inside the boundary too. The window and the egui-winit state are left consistent by every call that
  can panic. The one unsound case is epaint's assertion on a dropped delta in debug builds (Minor 5).
- No busy loop. After a failure there is one try per `PAINT_RETRY`. A redraw during the wait returns at once with the
  wake set to the retry (`event_loop.rs:421-426`), so input costs only that early return, and the minimised path of
  `about_to_wait` gets `Until(at)`. `paint_failed` runs at most once per frame (`:457-473`), so eight failures take at
  least 3.5 s.
- One autosave per revision. `last_autosaved` is set when the autosave is queued (`state.rs:559`). `on_device_lost`
  and the failed paint of the same frame write once (test). `State::paint_failed` runs under `crash::catch`, and when
  that step panics with `repeated` set, the app still quits (`app.rs:134-142`).
- The quit without `finish_quit` keeps the autosave. No purge is queued, the session lock ends with the process,
  `backup::scan` offers autosaves of dead sessions (`backup.rs:318-329`), and the `Shell` drops `app` first (field
  order), so the I/O thread drains while the window still exists (with the limit of Minor 3).
- egui textures after a recreate:
  - The font atlas goes in whole, under `TextureId::default()` and with the atlas's own options (`LINEAR`,
    epaint `texture_atlas.rs:191`), before any partial delta can reach the new renderer. egui takes the atlas delta in
    `end_pass`, and the delta of a failed pass is dropped with that pass (in debug builds, see Minor 5), so nothing is
    pending.
  - `ImageCache` is the only owner of `TextureHandle`s outside tests (`ui.rs:74`; the start screen, the library, the
    texture panel, the texture properties and the UV editor all go through it), and it clears on the epoch.
  - The view textures are registered again because `follow_device` resets `shown` before the UI of the frame
    (`frontend.rs:211-213`, `:747-771`).
  - Icons are Phosphor glyphs in the font atlas. The app mark and the gizmo are shapes. No egui image loader and no
    paint callback is used.
- The old device. `win.gpu` is replaced. `RenderGlue` and its renderer move at the next `follow_device`. The rig drops
  its renderer on `own != device`, except in safe mode (Nit 8). The PNG jobs hold only readbacks, which `rebuild`
  fails. The `DeviceWatch` closures hold an `Arc<Shared>` without a device, so there is no cycle. The MCP renderer
  runs in another process, and its `Renderer::new` replaces its renderer, never stacks two. The old painter, with its
  surface and swap chain, is dropped before the new one is made (`event_loop.rs:235`). egui-winit gets the new
  maximum texture side (`:237-239`).
- M3. There is one `DeviceWatch` per device (`render_glue.rs:51`, `:74`). `take_device_lost` reads the watch. Test
  `two_renderers_on_one_device_both_report_its_loss` covers two renderers on one device.
- m2. `end_pass` takes the viewport output (`context.rs:3340`), so `will_discard` is true whenever a discard was asked,
  and the shell's `discarded` counter is what bounds it (`event_loop.rs:452-454`). A dropped pass still applies its
  texture deltas and platform output, and asks for the next frame at once, as egui's `run` does.
- m3. `color_image` and `fit` keep every k-th texel, with k = ceil(longest side / limit). The sizes ceil(w/k) x
  ceil(h/k) match the loops. The limit is egui's `max_texture_side`, which egui-winit takes from the painter at open
  and at recreate. Previews keep their UV mapping because `paint_image` uses the 0..1 rectangle.
- m9. On a panic, the rig drops its renderer and ends the request with `render_done(None)`. The frame guard, the
  document and the counter are untouched (test `a_thumbnail_that_panics_does_not_restore_the_document`).
- Tests reviewed and meaningful: `a_panic_of_the_painter_costs_the_frame_and_nothing_else`,
  `a_new_renderer_gets_the_font_atlas_and_the_epoch_changes`, `two_renderers_on_one_device_both_report_its_loss`,
  `a_mesh_texture_above_the_device_limit_is_cut_down_and_the_mesh_still_draws`,
  `a_picture_above_the_limit_keeps_every_other_texel`, the three paint-failure tests of `tests.rs`, the two
  `ImageCache` tests, and the rig test.

## Gates

- Clippy on the four crates: clean.
- `cargo test -p cuboid-shell`: 45 pass, including the three new tests.
- `cargo test -p cuboid-render --lib`: the first full run had one failure,
  `a_face_on_a_strip_shows_the_tile_of_the_tick_and_only_the_frame_is_uploaded`. Its first render showed the
  background `[52, 52, 52]` where the red face was expected. It passed 3 of 3 times alone and in three more full runs
  (52 pass, 1 ignored). Nothing in R4-A touches the atlas or element path, and the WARP adapter ran 50 devices in
  parallel. Watch it in the full gate run.
- `cargo test -p cuboid-app -- tests:: thumbs render_glue state`: 236 pass, 1 fail. All R4-A tests pass: the three
  paint-failure tests and `a_thumbnail_that_panics_does_not_restore_the_document`. The failure is R4-B's
  `frontend::recent::tests::a_pass_with_a_batch_out_asks_to_come_back_and_an_idle_one_does_not`: "Dropped
  TexturesDelta with 1 unapplied deltas" (epaint `textures.rs:337`, the assertion of Minor 5), because its `pass`
  helper dropped the `run_pass` output. That file has changed since my run and now clears the delta
  (`frontend/recent.rs:204`). I did not run it again; it belongs to R4-B's review. The form in the brief,
  `cargo test -p cuboid-app -j 4 tests:: thumbs render_glue state`, is refused by cargo ("unexpected argument
  'thumbs'"): several filters go after `--`.
- `cargo test -p cuboid-ui common`: 4 pass.

## Fixes

Status: 2026-10-29. Checks: `cargo fmt -- --check` clean, clippy `--all-targets -D warnings` clean on cuboid-shell,
cuboid-render and cuboid-app, `cargo test -p cuboid-shell -p cuboid-render` (45 and 52 pass, 1 ignored),
`cargo test -p cuboid-app` (242 pass, 1 ignored, plus 7 integration tests).

1. Fixed. `Resized` runs `on_window_resized` inside `guarded`; on error it logs and sets `win.painter = None`, so the
   next frame reports the failure through `paint_failed` and builds a new painter.
2. Fixed. `MAX_MODEL_BYTES` (32 MiB) is back in `thumbs.rs`; the size is read inside the job on the library I/O thread
   and a larger file ends in `JobError::Failed`, so the UI thread touches no file.
3. Not fixed. Minor, outside the permitted files (`io_thread.rs`, `flows.rs`) and not requested.
4. Fixed. `State::keep_work` returns at once when `quit` is set; the shell skips `recreate_gpu` when the frame quits.
5. Fixed. `present` holds the texture changes in `Deltas`, which clears the `TexturesDelta` on drop.
6. Not fixed. Nit, skipped by brief.
7. Not fixed. Nit, skipped by brief.
8. Not fixed. Nit, skipped by brief.
9. Not fixed. Nit, skipped by brief.
10. Not fixed. Nit, skipped by brief.
11. Not fixed. Nit, skipped by brief.
12. Not fixed. Accepted in the review as is.
