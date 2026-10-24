# Lean roadmap

Status: adopted 2026-10-24 at the owner's request. Where this file conflicts with `docs/design/architecture.md`
(parity-first rules, UI parity, the work-package tables, parity tiers P1-P4), this file wins. The crate layout,
the layering rules and the code already built stay.

## 1. Goals, in priority order

1. Stability: no crash on any input or action, no data loss (autosave, atomic writes, recovery).
2. Performance: render speed, input latency, RAM use, open and save time.
3. The features people use to model, voxel-paint and texture, for Minecraft and general voxel art.
4. Compatibility: open and save `.cubik` (payload identity), open `.cub`, Minecraft Java JSON in and out.
5. A modern UI in the spirit of Blockbench and Blender, not a copy of Cubik's layout.

## 2. Kept from M0 and M1

| Crate | State | Use from now on |
|---|---|---|
| cuboid-shell | built, reviewed, fixed | window, input, headless frames |
| cuboid-bson | built, passed review | `.cubik` payload |
| cuboid-image | built, reviewed, fixed | textures |
| cuboid-voxel | storage, mesher and DDA built, reviewed, fixed | voxel worlds |
| cuboid-model | objects, setters, apply and capture built; apply under review | document model, undo snapshots |
| cuboid-io | built, reviewed, fixed | tolerant JSON, NetJson, text |
| cuboid-actions | built, reviewed, fixed | action registry and key dispatch; Cubik menu tables stay as data, not as the UI |
| cuboid-settings | built, reviewed, fixed | settings file, recent files |
| cuboid-fmt-cubik | in progress (W1.7) | `.cubik` and `.cub` |

## 3. Cut or deferred

| Item | Decision |
|---|---|
| UI parity (layout, sprites, dialog geometry, Cubik theme) | cut; new UI per 6 |
| Strict parity modes (frame-quirk shortcuts, quirk switches) | not built further; the port behaviour is the only behaviour |
| Parity ledger | kept only for file formats (`.cubik`, `.cub`, Minecraft JSON); other packages add no entries |
| UI snapshot matrix, input-replay oracles, UI reference twins | cut; unit tests, editing scenario tests and loader fuzzing stay |
| Formats kept | `.cubik`, `.cub` (read), Minecraft Java block and item JSON plus resource pack with item definitions, OBJ export, glTF export, VOX import and export, PNG render |
| Formats deferred | `.mcpe` and `.js`, `.java` entity code, `.jpm` and JEM, entity atlas, `.schematic` and `.nbt`, Sketchfab upload |
| Rarely used Cubik functions | deferred by default; added back on request |
| Agent toolkit (`agent_toolkit.md`) | after the first release (L7); L2 keeps commands as data so it stays cheap |

## 4. Milestones

| # | Milestone | Scope | Exit |
|---|---|---|---|
| L1 | Data core | W1.7 `.cubik`/`.cub`, W1.11 CLI with `dump` and `roundtrip` only | the private sample round-trips its payload byte for byte; loaders never panic on fuzz input |
| L2 | Editing core | `cuboid-core`, `cuboid-ops`: command bus with commands as data, undo and redo on W1.6 snapshots, selection, numeric transform, add, delete, duplicate, group and reparent, UV basics (auto UV, rotate, flip), texture paint basics (pencil, eraser, fill, picker, palette), voxel tools (pencil, erase, fill, box, line, picker, mirror), autosave | a scenario test per command; undo of every command restores the prior snapshot |
| L3 | Renderer | `cuboid-render`, `cuboid-scene`: opaque, cutout, sorted translucent and line passes, instanced cuboids, voxel chunk meshes with incremental upload, nearest-filtered textures, grid and axes, selection outline, gizmo drawing, picking, offscreen PNG, device-lost recovery | the targets of 8 met in release benchmarks |
| L4 | Modern UI | `cuboid-ui-kit`, `cuboid-ui`, `cuboid-app`: theme, docking layout, panels, viewport navigation, gizmos, command palette, keymap presets, start screen, settings, toasts, recovery dialog | every L2 command reachable from the UI; smoke scripts pass headless |
| L5 | Formats | Minecraft Java JSON and resource pack, OBJ, glTF, VOX, PNG render dialog | round-trip tests; public sample models import |
| L6 | Hardening and release | crash safety of 7, loader fuzzing in CI, budgets of 8, Windows build and installer, then macOS and Linux | release candidate |
| L7 | After release | agent toolkit, deferred formats, extension API over MCP | per item |

L2, L3 and L4 run in parallel once L1 is done, because their crates are disjoint. L5 starts when the L2 core
types settle. L6 work is continuous, with a final pass before release.

## 5. Process

- Fewer, bigger packages: one agent owns one crate or one panel group.
- Sonnet implements with tests. Every package passes `cargo clippy --all-targets -- -D warnings`, its tests,
  `cargo fmt --check`, `cargo xtask layering` and `cargo check --workspace --all-targets`.
- Opus reviews only where risk is high: `.cubik`/`.cub` I/O, undo and apply, the renderer core, the crash-safety
  layer, plus one short smoke review per milestone.
- `docs/spec` stays the source for data semantics and tool behaviour. For the UI and everything cut in 3 it is a
  reference only.

## 6. Modern UI baseline

- egui on the existing shell. Docking panels (candidate crate: egui_dock; check licence and egui version first).
  An icon font under a permissive licence (candidate: Phosphor). No Cubik or Blockbench art, and no copy of their
  themes: cuboid has its own palette, shapes and icons. Blockbench and Blender inspire interaction patterns only.
- Layout: menu bar with mode tabs (Model, Voxel, Paint, Display); outliner on the left; properties on the right
  (element, transform, faces and UV, textures, palette); viewport in the centre with optional split views; status
  bar with key hints. The UV editor and the texture panel are dockable.
- Viewport: Blender-style navigation (middle drag orbit, Shift pan, wheel zoom, numpad views, frame selected) and
  a Blockbench-style preset (right drag orbit). Transform gizmo with snapping (1/16 grid, whole units, 22.5 degree
  rotation), box select, hover highlight.
- Command palette (Ctrl+P and F3) for every action; customisable keymap; undo history panel.
- Start screen: recent files with thumbnails, new from template (block, item, voxel), drag and drop to open.
- Feedback: toasts for messages, modal dialogs only for destructive confirmations, progress for long jobs. The UI
  thread never blocks.
- Dark and light theme, UI scale 75-200 %, a high-contrast option.

## 7. Stability rules

- No `unwrap` or `expect` on external data. Every loader returns errors and checks sizes before allocating (done for
  images and voxels).
- Panic boundary: each frame's app logic runs inside `catch_unwind`. After a panic the frame is dropped, the
  document is restored from the last undo snapshot, a crash report and an autosave are written and a toast is shown.
  Repeated panics open the recovery dialog.
- Autosave every few minutes and before risky operations to a recovery folder; on start-up, offer recovery.
- Every file write is atomic (temporary file, then rename). Saves and exports run on the I/O thread.
- GPU: wgpu error scopes; on device loss, recreate the device and resources; on surface errors, reconfigure.
- Loader fuzz targets run nightly; the undo and apply proptests stay.
- Memory budgets for textures, voxel worlds and meshes: refuse with a message instead of aborting.

## 8. Performance and RAM targets

Initial targets, release builds, a mid-range PC; adjust after the first measurements.

| Measure | Target |
|---|---|
| Cold start to a usable window | 1 s or less |
| Idle RAM with an empty document | 150 MB or less |
| RAM with 1 M visible voxel faces | 400 MB or less in total |
| Frame time with 10 k elements or 2 M voxel faces | 16 ms or less |
| Voxel edit to updated picture | within the same frame (dirty chunks only) |
| Open or save of the 150 MB payload benchmark | off the UI thread, no frame above 50 ms |

CI benchmarks guard these numbers; a regression blocks the change.

## 9. Next steps

1. Finish L1: the W1.6 review, W1.7, then W1.11.
2. Write the L2, L3 and L4 package briefs from this file and start them in parallel.
