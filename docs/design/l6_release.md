# L6: hardening and the first Windows release

Status: 2026-10-28, approved by the owner with the recommended decisions of 7 (`cuboid.com`, per-user installer,
R2 stops at the targets and reports numbers if they need a redesign). Covers the open part of milestone L6 of `roadmap_lean.md` (4, 7, 8): what
the code still lacks before a release candidate, in the order the owner set (stability, performance, RAM). The agent
toolkit R1 is done (`agent_toolkit_r1.md`); the test rig R2 comes after this. R3 written 2026-10-29 (`cargo xtask package`,
`cuboid.ico`, `packaging/`, `release.yml`); the release build and the installer smoke test are still to do.

## 1. Ground truth

| Need (roadmap) | Today | L6 |
|---|---|---|
| Panic boundary per frame, crash report, autosave, recovery dialog | `crates/cuboid-app/src/{crash,backup,jobs}.rs`, recovery flow | audit only (R4) |
| Atomic writes, saves off the UI thread | I/O thread, temporary file then rename | audit only (R4) |
| Device loss | handled in render glue, MCP renderer and offscreen paths | audit only (R4) |
| No console window on double click | the release binary is a console program: a black window opens next to the editor | R1 |
| Cold start to a usable window <= 1 s | about 1.7 s measured by hand on 2026-10-24; no phase timings exist | R2 |
| Idle RAM <= 150 MB | about 204 MB working set, 402 MB private on 2026-10-24 | R2 |
| Windows build and installer | none (`packaging/` is empty, `xtask package` is a stub) | R3 |
| Loader fuzzing in CI | `nightly.yml` runs a 60 s smoke per target | keep; R4 checks every loader has a target |

## 2. R1: no console window, the terminal still works

The binary keeps one name, `cuboid.exe`, because the AI client lines and file associations name it.

1. `cuboid.exe` is a Windows GUI program in release builds:
   `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`. Debug builds and tests stay console
   programs. A GUI program still reads and writes standard handles that its parent passes, so `cuboid mcp` run by
   Claude Code, Codex or Claude Desktop keeps working through pipes; writes to missing handles are dropped by std.
2. A second binary, `cuboid-term` (console program, safe std only, about 80 lines), is installed next to it as
   `cuboid.com`. It starts `cuboid.exe` from its own folder with the same arguments and with pipes for its standard
   streams, relayed to its own, waits, and exits with its code. Typing `cuboid --check`, `cuboid --help` or `cuboid --version` in Command Prompt or
   PowerShell runs `cuboid.com` first (PATHEXT lists `.COM` before `.EXE`), so the output appears and the shell waits.
3. `Snippets::check` names `cuboid.com` on Windows (`& "<dir>\cuboid.com" mcp --root "<folder>" --check`); the three
   client lines keep `cuboid.exe`. The guide (`docs/mcp.md` 2.1) and `guide.rs` follow.
4. Proof: a release-mode probe by the coordinator (not an agent): `cuboid.exe mcp --check` through pipes, the client
   handshake of the `mcp_stdio` test against the release binary, `cuboid.com --version` in both shells, and a double
   click start (launch and quit through the app's own quit path only; no synthetic desktop input).

Alternatives not taken: attaching to the parent console needs `unsafe` FFI (every crate forbids it); a console
program that hides its own window flashes a console and also needs FFI.

## 3. R2: start-up and idle memory

1. Measure first. `--trace-startup` (also `CUBOID_TRACE_STARTUP=1`) logs one line per phase with milliseconds since
   the first instant of `main` (the loader's time before `main` is outside every number): settings, window created,
   instance and adapter, device, surface configured, fonts and icons, pipelines, library scan, first frame
   presented, first frame with the document. `CUBOID_EXIT_AFTER=<seconds>`, read only while the trace is on (a
   value left in the environment must not close the editor of a user who is not measuring), quits through the
   normal quit path that many seconds after the first full frame (0 = at once), for scripted measurement of
   start-up and idle memory without any input. The quit asks about unsaved work like the close button.
2. Then cut, in this order, keeping only changes the trace proves:
   - present a first frame (start screen frame, theme background) before the slow work; GPU device creation and
     pipeline building on a worker thread while the window shows;
   - pipelines built on first use per pass, or all on a worker thread, never on the UI thread after the first frame;
   - fonts: rasterise the atlas once at the needed sizes; no unused weights;
   - library scan and thumbnails strictly after the first frame, on the job pool.
3. Idle memory: measure the working set and private bytes with an empty document after 10 s idle, from outside the
   process (the probe reads `WorkingSet64` and `PrivateMemorySize64` with PowerShell `Get-Process`). Trim what the
   numbers point at: staging buffer sizes, preallocated vertex pools, image caches, rayon pool thread counts (two
   pools of logical-core size are large on 16+ cores: cap the background pool), egui texture atlases.
4. Targets stay those of roadmap 8 (1 s, 150 MB). If a target needs a design change beyond these steps, the package
   stops and reports the numbers.

### Measurements (2026-10-29, step 2)

Release build, Windows 11, empty `CUBOID_HOME`, `--trace-startup`, milliseconds since the first instant of `main`
(the time before it is not measured; to bound it once from outside, run `Measure-Command { Start-Process
cuboid.exe -Wait }` with `CUBOID_TRACE_STARTUP=1` and `CUBOID_EXIT_AFTER=0` set, not done yet). Warm: settings 7,
app 10, event_loop 54, window 69, instance 297, adapter 476, gpu 582, visible 605, renderer 657, library_done 660,
ui 668, built 676, first_frame 679. Cold first run: instance 552, adapter 750, first_frame 966. Idle after 10 s:
working set 157 MB (target 150), private 373 MB, 49 threads. So the instance (about 230 ms warm, 470 ms cold) and the
adapter (about 175 ms) take 60 % of the start; the shell enabled every backend, so the Vulkan loader with its
layers and ICDs, the GL driver and DX12 all loaded.

Backends compared, release build, warm, milliseconds to the first frame and working set after 10 s idle:

| Backends | First frame (ms) | Working set (MB) | Private (MB) |
|---|---|---|---|
| all (before R2; wgpu chose Vulkan; 49 threads) | 680 | 157 | 373 |
| DX12 with DXC | 595 | 171 | 337 |
| DX12 with FXC (no `dxcompiler.dll`, as on a user machine) | 615 | 164 | 336 |
| Vulkan only (34 threads) | 623 | 133 | 345 |
| Vulkan first, DX12 and GL fallback (cold 640, 34 threads) | 630 | 133 | 346 |

| Release candidate: Vulkan first, final pool of 8 workers (cold 656, 38 threads) | 637 | 133 | 345 |

The first row is before R2 (15 background workers on this 16-core machine); the rows between were measured with the
interim cap of 4 background workers; the last row is the release candidate with the final pool,
`(cores - 1).clamp(2, 8)`. Both targets of roadmap 8 are met: first frame under 1 s cold and warm, 133 MB idle.

Phases, Vulkan only: instance about 180 ms (the loader), adapter 15, gpu 265, renderer 53. DX12: instance 13, adapter
205, gpu 92 to 120, renderer 152 to 162 (the HLSL compile). Every row is under the 1 s target; only Vulkan alone is
under the 150 MB one. So the window starts on Vulkan on Windows, the backend the app has used since L3, and falls back
to DX12 and GL when Vulkan gives no adapter or the device request fails. The MCP offscreen renderer keeps its W-R1.4
rule (DX12 first, no fallback): it has no memory target, and a Vulkan instance delays the end of the process.

Changes:

1. `cuboid-shell/src/backend.rs`: `BackendPlan` (pure rule, tested), `window()` and `offscreen()`. Windows window:
   Vulkan, then DX12 and GL when it gets no adapter or no device or when the first attempt panics (a driver that
   cannot build egui's pipelines); Windows offscreen (the MCP renderer): DX12 alone; macOS: Metal; elsewhere Vulkan
   and GL. `WGPU_BACKEND` is used alone, with no fallback.
2. One `instance_flags()` in `backend.rs`, used by the window's instance and by the MCP renderer's device, drops
   `VALIDATION_INDIRECT_CALL`, the only flag a release build sets: it builds compute pipelines at every device
   creation, and nothing draws or dispatches indirectly. `WGPU_*` variables still apply.
3. The background pool is all cores but one, between two and eight (was cores - 1: 15 threads on 16 cores, 8 now).
   The coordinator's decision: the work in the pool is CPU-bound per chunk (decode, chunk tables, encode), and the
   save of a large voxel model runs there off the UI thread, so speed wins over a few idle threads. The global
   rayon pool is not built at start-up: nothing builds it, rayon creates it on the first voxel loop (meshing, undo),
   so building it early would add threads to every empty start. Capping it belongs where a voxel document is first
   installed.
4. No preallocation to trim: the renderer's pools start at 256 elements, 4096 voxel faces and a 256 px atlas of two
   layers. The persistent threads are `cuboid-io` twice (documents, library) and the pool; every other spawn is a
   one-shot job.

Not changed, for the next measurement: egui-wgpu enumerates the adapters and `request_adapter` enumerates them again; a
native adapter selector would drop one pass.

## 4. R3: Windows package

1. `xtask package` builds release, rasterises `crates/cuboid-assets/brand/logo-16.svg`, `logo-32.svg` and `logo.svg`
   to 16, 32, 48 and 256 px and writes `cuboid.ico` (crates already in the tree only; no new rasteriser unless
   `cargo search` shows a maintained MIT/Apache one, named in the report).
2. The icon is embedded in `cuboid.exe` and `cuboid.com` with a build script crate (candidate `winresource`; checked
   with `cargo search`, licence allowed by `deny.toml`), Windows only.
3. `cargo-packager` (0.11.8) config under `packaging/`: an NSIS installer per user (no administrator), Start menu
   entry, uninstaller, optional "Open .cubik files with cuboid" checkbox off by default, `docs/mcp.md` and the
   licences in the install folder; plus a portable zip. Output names carry the version.
4. CI: `release.yml` on a `v*` tag builds both on Windows and attaches them to a draft release (never publishes).
   Local: the coordinator builds once and smoke-tests the installer (install, start, quit, uninstall) in the
   scratchpad's own folder where possible.

## 5. R4: audit of the stability rules

One Opus reader checks roadmap 7 against the code and lists gaps as findings (no code): every loader free of
`unwrap`/`expect` on external data and sized before allocation; every file write atomic; every save and export on
the I/O thread; the frame panic boundary restores the last snapshot; memory budgets refuse with a message; every
loader has a fuzz target. Gaps found become R4 fixes by one sonnet agent.

## 6. Order and process

R1 and R4 first in parallel (disjoint files: R1 is `crates/cuboid-app/src/main.rs`, a new `crates/cuboid-term`,
`cuboid-mcp/src/connect.rs`, the guide; R4 reads only). Then R2 (app start-up, render, shell), then R3 (packaging,
xtask, CI). Each: sonnet builds, Opus reviews into `docs/review/l6_<id>.md`, sonnet fixes, full gates, owner OK
before each commit.

## 7. Owner decisions

1. D-R1: the console twin is named `cuboid.com` (recommended; the devenv.com pattern) or `cuboid-term.exe`.
2. D-R2: the installer is per user (recommended) or for all users.
3. D-R3: R2 stops at the targets or at "best effort in one package" if the targets need a renderer redesign.
