# Review: agent toolkit R1, W-R1.4 App

Status: 2026-10-28. Reviewer: Opus. Scope: W-R1.4 of `docs/design/agent_toolkit_r1.md` as built in the working tree:
`cuboid mcp` in `crates/cuboid-app/src/{lifecycle.rs,main.rs,mcp.rs}`, the renderer `src/mcp_render.rs`,
`frame_model` in `src/frontend/render_job.rs`, `validation.rs` and `flows.rs` on `cuboid_mcp::{apply_fixes,
fixable}`, `tests/mcp_stdio.rs`, the app's `cuboid-mcp` dependency in `Cargo.toml` and `Cargo.lock`, and the Status of
the spec; W-R1.3 only as W-R1.4 uses it. Run: the six stdio tests; the unit tests of `mcp`, `mcp_render`,
`validation`, `render_job`, `lifecycle`, the export dialog and the fixes (all pass); clippy `-D warnings` on all
targets, `cargo fmt -p cuboid-app --check`, `cargo xtask layering`. The probes ran a copy of the binary in the
scratchpad with the portable marker beside it, so they wrote nothing into the user's log (deleted after): root forms,
a byte order mark, `CUBOID_LOG=trace`, `WGPU_BACKEND=vulkan`, the input closed during a sheet, five exits after a
picture, `--check` into a closed pipe, 200 edits each followed by a render, and a line piped from Windows PowerShell
5.1. Not run: workspace tests, the gates, the private oracles; mesh and voxel models (no fixture).

Verdict: ready after the minor fixes; no blocker or major. Stdout carries protocol lines only on every path traced and
probed, the process ends 66 to 174 ms after its input, the root check, `--read-only` and the exit codes hold, the
renderer keeps the `Renderer` contract, the W-R1.3 review's one scale and `TooLarge`, and the cameras are the ones the
contract names; a Validator fix is one undo step. Four minor findings: pictures of mesh and voxel models after another
model, the faces of the cameras left unnamed to the agent, the stdio tests writing the user's log, and `--check` into a
closed pipe.

## Minor

1. `crates/cuboid-app/src/mcp_render.rs:188` A new revision gets a fresh `Extractor` on the same `Renderer`, which
   keeps what the previous scene left in it. Meshes: the renderer drops a mesh only on `removed_meshes`
   (`crates/cuboid-render/src/meshes.rs:246`), which a fresh extractor never sends for ids it has not seen, and
   `render_view {"ref": id}` loads the stored copy anew on every call (`crates/cuboid-mcp/src/views.rs:206`), with new
   ids. So each picture of a reference with meshes (a `.bbmodel` mesh, an imported OBJ) leaves its buffers and geometry
   behind until the memory budget refuses uploads ("no GPU memory budget left for a mesh") and later pictures lose
   their meshes. Voxels: the renderer drops a chunk mesh older than the one it holds for that slot
   (`crates/cuboid-render/src/voxels.rs:144`), and a chunk's revision is its world's process-wide stamp. Scenario:
   open the voxel models `a.cubik` and `b.cubik` in that order, then `render_view` a, b, a: the third picture's chunks
   carry a's older stamp and are dropped, so a's draw ranges read b's face records. Traced, not run. Element models
   are not affected: 200 edits each followed by a render kept the process at 227 to 228 MB. The library thumbnails
   (`crates/cuboid-app/src/thumbs.rs:119`) share the mesh half; they load each model just before they draw it, so not
   the voxel half. Fix: give `Scene` a flag `heavy` (`!doc.meshes.is_empty() || doc.voxels.is_some()`) and, when the
   scene being replaced was heavy, make the renderer anew (`self.glue = RenderGlue::new(&self.state)`) before the
   upload; a Minecraft model never pays it. Tests (skipped without an adapter, like the others): a mesh document drawn
   three times as fresh loads keeps `renderer.health().gpu_bytes`; voxel documents a and b, made in that order and
   drawn a, b, a, give the first picture twice.
2. `crates/cuboid-mcp/src/views.rs:72`, with `crates/cuboid-app/src/mcp_render.rs:288`. `front` shows the north face,
   as the `Camera` contract says and as Minecraft's directional blocks carry their front texture; but the app's Front
   view and the Front of the Render to PNG dialog look from the south at the south face
   (`crates/cuboid-scene/src/camera.rs:49`, `render_job.rs:197`), the app's Iso looks from the south-west, and the
   outline's Java model space has z pointing south, towards an unturned GUI camera. The camera description says only
   "front, back, left, right, top, bottom, iso (default) or gui". Scenario: the user says the front looks bare,
   meaning the app's Front view; the agent renders `front`, sees the north face and edits the other side. Fix: keep
   the mapping and name it: "front looks at the north face (-z; the app's Back view), back at the south face, left at
   the west face (-x), right at the east face, top with the north up, bottom; iso from above the north-east corner,
   as the inventory shows a block; gui the inventory slot (emulated)".
3. `crates/cuboid-app/tests/mcp_stdio.rs:45`, `:146` Every test process logs into the user's
   `%APPDATA%\cuboid\data\logs\cuboid.log` (`Dirs::detect`, `crates/cuboid-app/src/platform.rs:23`): the log held 119
   "serving" lines of test runs. A gate run adds a dozen processes, a failing test can leave crash reports there, and
   the rotation keeps three files of 5 MiB, so test lines push out the lines around a real crash. `directories` asks
   Windows for the known folder, so no existing variable moves it, and a portable marker beside
   `target/debug/cuboid.exe` would change the development binary for every run. Fix: `Dirs::detect` takes
   `CUBOID_HOME` when it is set, as `Dirs::under(Path::new(&home))` (the portable layout); `Session::start` and `run`
   set it to a temporary folder of their own, not the project, whose listing the check test asserts. A test can then
   assert that no `crash_*.txt` was written.
4. `crates/cuboid-app/src/mcp.rs:114` `--check` prints with `print!` and `println!`, which panic when stdout is gone.
   Ran: `cuboid mcp --check` with its stdout closed, as `| head -3` or `| Select-Object -First 3` close it: exit 101,
   "failed printing to stdout: The pipe is being closed. (os error 232)", and a crash report in the logs folder; 6.1
   says 0 or 2, and the module doc says 1 when the streams break. `--help` (`:55`) is the same. Fix: build the text in
   a `String` and write it once with `io::stdout().lock().write_all`, an error giving `ExitCode::FAILURE`. Test: the
   check with its stdout closed exits 1 and leaves no crash report (with finding 3).

## Nit

5. `mcp_render.rs:283` and `:364` say the scene's +Z is the game's south; it is the north (`FaceName::North` is "+Z
   internal", `crates/cuboid-model/src/objects.rs:179`), which is why the north face is seen along -Z. The code is
   right, and the comment of the iso test (`:388`) says it right.
6. `mcp_render.rs:35` `MAIN_VIEW: u8 = 3` is a third spelling of the Main view (`render_job.rs:28` `MAIN`, the literal
   `3` at `:241`). Use `ViewId::Main.index()` for the index and `ViewId::Main as u8` for `ViewParams::from_camera` in
   both files.
7. `mcp_render.rs:155` `RenderState::create` also builds an egui renderer and its pipeline that the server never draws
   with, and of `RenderGlue` only `renderer` is used. Acceptable as the device setup the window shares; if the first
   picture should open faster, request the device with wgpu directly and hold a `Renderer`.
8. `mcp.rs:35` A second `--root` replaces the first without a word; refuse it ("--root is given twice"). `mcp.rs:103`
   A `ServeError::Root` (the folder went away after the check) exits 1 where every other root refusal exits 2; send
   it through `refuse`.
9. `mcp.rs:14` The usage says "an absolute path that is not a drive root", but UNC and `\\?\` roots are refused too
   (ran: "network paths are not read"; a verbatim local path hears "device paths and special names are not read"),
   while a junction to a share or a mapped network drive passes after canonicalisation. Say "an absolute local
   folder"; refusing network roots is fine for R1.
10. `mcp.rs:120` prints `%APPDATA%\Claude\claude_desktop_config.json` on every system; on macOS the file is in
    `~/Library/Application Support/Claude/`.
11. `tests/mcp_stdio.rs:565`, `:576` `iso` and `gui` check coverage only, which is why `gui` is checked by eye. Add
    pixels: in `iso` the north colour right of the middle, the east colour left of it, the up colour above; after
    `set_display` with the block preset (rotation 30, 225, 0, scale 0.625) `gui` shows the same three colours in the
    same places, smaller. That pins the corner and the inverse pose.
12. Spec. The Status should name this review; its line 9 runs to 139 characters; the W-R1.4 row of 8 leaves out
    `src/flows.rs` and `src/frontend.rs`, which changed.

## The reported choices

1. `Args.mcp`, the order and the exit codes: accepted. Only a leading `mcp` counts, so `./mcp` still opens a file
   (tested); the server starts after logging and the crash hook, so a panic leaves a report, and before settings,
   recovery, the I/O thread or the window. The codes are as reported, with finding 4 and nit 8. Several servers and
   the GUI now append to one `cuboid.log`, and a line does not say which process wrote it; the process id in the
   server's lines is worth a follow-up.
2. No `NoRenderer`, the device opened at the first picture and kept: accepted. A session that never renders loads no
   driver and exits 66 to 73 ms after its input.
3. Adapter and device: accepted. Discrete first is fine (on a laptop it wakes the discrete GPU; the integrated one
   would serve as well). DX12 alone unless `WGPU_BACKEND` is set: with `vulkan` the picture was right, the exit took
   133 ms, and this machine's broken layer reached stderr through the logger only. WARP as the fallback is on every
   Windows 10 and 11. `TooLarge` is right but out of reach in practice, since egui-wgpu asks for 8192 px and the tools
   stop at 2048; keep it. One upload per revision: right for elements, see finding 1 for the rest. The read-back
   bound of 30 s and a lost device reopened by the next picture: accepted; the call that meets the loss answers
   `E_IO`, which reopening and drawing once more within that call would hide. `mem::forget`: safe, since the journal
   is opened, written and closed per call, the log file is unbuffered and the `Server` drops normally; it also keeps
   a hung device from holding the exit. Measured: 109 to 122 ms after a picture, 149 to 174 ms when the input closes
   during a nine-view sheet, which is finished and answered first.
4. Conventions: right. The six faces are those of the `Camera` contract, proven by the pixel test. The iso corner is
   the game's: the `gui` rotation (30, 225, 0) of `block/block` turns the north face to the right and the east face
   to the left of a camera looking along -z, which is a camera 30 degrees above the north-east corner. One
   orthographic scale from the 3D bounds (the sphere of the box). `gui`: the inverse pose is right for rotation,
   translation and a uniform scale (an untouched slot looks at the south face, as the game shows a flat item; the
   block slot gives the iso corner); averaging a non-uniform scale is fine; a negative scale is not mirrored, and
   parts more than 64 units from the slot centre along the line of sight pass the far plane (128), which Java
   elements (-16 to 32) never reach. Name the faces (finding 2) and pin them with pixels (nit 11).
5. `frame_model` shared: accepted. The PNG dialog runs the same lines as before (its tests pass), and the doc states
   the one-scale rule of the contract.
6. `validation.rs` and `flows.rs` on `apply_fixes` and `Core::batch`: accepted. The supported set, the clamp and the
   failure texts are the old ones (`ToolError::of`); a texture removal now shares the step (new test; it was a
   second step), a fix that changes nothing leaves no step, and the export dialog still fixes errors only, in one
   step, and checks again (its test passes). The undo label becomes "Fix validation issues".
7. The stdio tests in the user's log: not accepted, finding 3.
8. Byte order mark: confirmed in Windows PowerShell 5.1 with `$OutputEncoding` set to UTF-8 (its default, ASCII,
   sends none but turns non-ASCII paths into `?`). Recommendation: tolerate one U+FEFF at the very start of the input
   in `serve` (`crates/cuboid-mcp/src/protocol.rs:81`, first line only), which RFC 8259 8.1 allows a parser; a
   separate small follow-up in `cuboid-mcp` with one transcript test, since no client sends it.
9. `windows_subsystem`: accepted as stated, and already among the "Not verified" of 8. A switch would need
   `AttachConsole(ATTACH_PARENT_PROCESS)` for `--check` and `--help` in a terminal, which takes unsafe code or a
   crate, and `main.rs` forbids unsafe.
10. `gles`: `opengl32.dll` is in the import table of `cuboid.exe`; the 55 ms were not measured here. Recommendation:
    keep it this round. Architecture 13.9 keeps GL as the last resort of the window (GPUs with a GL driver but no DX12
    one), and the exit stays under 175 ms. If the budget tightens, a separate follow-up can measure delay-loading
    `opengl32.dll` (`/DELAYLOAD` on the `cuboid` binary), which keeps GL for the window and drops the cost where GL is
    never touched; dropping `gles` on Windows would need the owner and the architecture table.

## Checked and sound

- Stdout: in server mode only `serve` writes it, one `writeln!` and flush per reply; the logger, the crash hook and
  the previous panic hook write stderr and the file; `Args::parse` returns before its `eprintln!`; `--help` and
  `--check` print only when nothing is served. Probed: every line was JSON-RPC with `CUBOID_LOG=trace` and with
  Vulkan forced, whose loader errors went to stderr.
- End of input: `serve` returns at once, a closed stdin reads as the end (exit 0 in every probe), and the input
  closed during a sheet still gets the sheet's reply. The stdio tests hold the exit under 250 ms, well under the
  300 ms of Claude Code on Windows.
- Root: relative, drive-relative (`C:evkey`), missing, a file, a drive root, `C:\evkey\..` and a junction to a drive
  root are refused with exit 2, a line on stderr and nothing on stdout; a junction to a folder and a trailing
  backslash serve.
- `--read-only` reaches `Options` (tested with a save); `--check` builds its server read-only and writes nothing into
  the project (tested).
- Renderer: the document is cloned for the extraction and its cameras are not used; the shading switches between
  block and item light before each recorded draw; the PNG targets and read-back buffers go with each picture; the
  instance buffer and the atlas pages do not grow across revisions (the 200 edits); `TooLarge` comes before any
  upload; `RenderError::TooLarge` reaches `E_LIMIT` through `render_error`.
- Validator: `fixable` and `apply_fixes` are the panel's set; `Core::batch` commits what was done and records one
  step only when a command was done.
- `Cargo.lock`: W-R1.4 adds only `cuboid-mcp` to the app's dependencies; layering passes with `cuboid-app` on L7.

## Fixes

1. Fixed in `mcp_render.rs`: `Gpu` keeps `heavy` (the last upload had meshes or voxels) and, when the scene is replaced,
   builds the `Renderer` anew before the upload; plain Minecraft models keep the cheap path. The flag sits on `Gpu`, not
   on `Scene`, so a picture that failed after its upload still counts. Tests (both fail without the rebuild, the mesh
   one by 66 KB a picture): `a_mesh_model_loaded_again_for_each_picture_keeps_the_gpu_memory_bounded` and
   `a_voxel_model_drawn_again_after_another_shows_its_own_faces` (a, b, a); both skip without an adapter.
2. Fixed in `crates/cuboid-mcp/src/views.rs`: the `camera` and `views` descriptions name the face of each camera
   (front the north face, the app's Back view; back south; left west; right east; top with the north up; iso from above
   the north-east corner; gui the inventory slot); the mapping is unchanged. Test
   `the_schemas_say_which_face_each_camera_shows`.
3. Fixed: `Dirs::detect` (`platform.rs`) takes `CUBOID_HOME` first, as `Dirs::under`, and its doc comment says so;
   `architecture.md` 15 (not part of this package) could carry a line. The stdio tests give every process a temporary
   `CUBOID_HOME`, and `assert_clean` fails on a `crash_*.txt`. The user's `cuboid.log` kept its 652 lines and its time
   across the tests of the two crates and the full gate run.
4. Fixed in `mcp.rs`: the usage and the check are built as a `String` and written once with `write_all` and `flush`
   (`write_stdout`); a closed pipe logs the error and exits 1. Test
   `help_and_check_into_a_closed_pipe_exit_1_without_a_crash_report` (a pipe whose reader is closed before the start).
   `cuboid --help` and `--version` in `lifecycle.rs` still use `println!`; the review did not name them.
5. Fixed: the comments of `scene_camera` and of the face test say the scene's +Z is the game's north.
6. Fixed: `ViewId::Main.index()` and `ViewId::Main as u8` in `render_job.rs` and `mcp_render.rs`; `MAIN_VIEW` and `MAIN`
   are gone. `thumbs.rs` has a `MAIN` of its own, which the review did not name; left.
7. Fixed: `open_device` asks wgpu for the adapter and the device (the instance, the limits and the features are those
   of the window's setup) and `Gpu` holds a `Renderer`; no egui renderer is built. `adapter_caps` in `render_glue.rs`
   takes the adapter and the device, and the `graphics:` line is logged by `Gpu::open`.
8. Fixed: a second `--root` is "--root is given twice" (exit 2; unit and stdio tests); `ServeError::Root` goes through
   `refuse` (exit 2; unit test `a_folder_that_went_away_after_the_check_is_refused_with_exit_2`).
9. Fixed: the usage says "an absolute local folder".
10. Fixed: `DESKTOP_CONFIG` is the Windows, macOS or other path for the system (the Linux one is not verified against a
    Claude Desktop release); the check test expects the one of its system.
11. Fixed in `a_known_model_renders_the_expected_pixels_from_each_side`: `iso` checks the north colour right of the
    middle, the east colour left of it and the up colour above, with the block shades; after `set_display` with
    rotation 30, 225, 0 and scale 0.625, `gui` shows the same three colours in the same places (`assert_hue`, since the
    GUI light shades the faces on its own: 0.4, 0.65 and 1.0 now).
12. Fixed in the spec: the Status names this review, line 9 is 107 columns, and the W-R1.4 row names `src/flows.rs` and
    `src/frontend.rs`, with `logging.rs`, `platform.rs` and `render_glue.rs` that these fixes changed.

Also done as the review advises: the server's log lines carry the process id (`[<pid>] ` before the target; the logger
takes `tag_process`, set for `cuboid mcp` only; unit test and a check in the legacy stdio test); `serve` skips one
leading UTF-8 byte order mark on the first line only (`a_byte_order_mark_is_skipped_on_the_first_line_only`). The wgpu
`gles` feature is as it was.
