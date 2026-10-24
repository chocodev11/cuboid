# Cuboid architecture proposal: parity-first

Status: proposal for review, 2026-10-18.
Angle: parity first (pixel-identical UI, bit-compatible files, behaviour-identical tools), then maintainability,
then performance. Where the three conflict, this document says which wins and why.

Inputs: the six functional specifications in `docs/spec/` (`formats_native.md`, `formats_interop.md`,
`model_and_tools.md`, `ui_layout_theme.md`, `ui_dialogs_menus.md`, `app_behavior.md`) and the Blockbench 5.2.2
study notes (used for architecture lessons only: no code, no assets, no wording). Section references such as
"native 4.4.3" or "tools 3.4" point into those specifications.

Conventions: "must", "should" and "may" are design decisions for the port, not spec requirements. Items marked
**UNVERIFIED** are assumptions this design depends on; section 20 collects them. Crate versions in section 3.4
were checked with `cargo info` on 2026-10-18.

---

## 0. Summary

- Parity is engineered, not hoped for. Every user-visible semantic has exactly one implementation path shared by
  all entry points (file load, undo restore, paste, import), and every optimised algorithm has a slow reference
  twin that differential tests compare against it.
- One numerics crate concentrates every rounding, snapping, Euler, colour-packing and number-to-text rule of the
  original runtime. All parity-sensitive code calls it, so a calibration change touches one crate.
- Files: an in-house BSON engine with raw-span retention makes unmodified `.cubik` payloads byte-identical on
  re-save; hand-built emitters reproduce every `cubik29` text and binary output. The `bson`, `serde_json`,
  `dot_vox`, `tobj` and `fastnbt` crates serve as independent oracles in tests (and as the modern-profile readers).
- UI: a uGUI-compatible layout and event layer (anchors, pivots, local scale, layout groups, layout elements,
  content size fitters, raycast targets, selection, navigation) sits on egui's painter, text and input. The window
  is authored in the same units and rules the original used, so geometry parity holds by construction and is
  checked against private reference dumps that never enter the repository.
- Shell: winit + egui-winit + egui-wgpu, not eframe. Shortcut parity needs keypad versus top-row identity and
  left/right modifier identity, which egui's `Key` enum does not carry, plus the original's own UI scale rule.
- Renderer: per-view offscreen targets in gamma space (`Rgba8Unorm`, reversed-Z depth, optional 4x MSAA) blitted
  pixel-exactly into the egui pass. Element transparency follows the original's per-object depth-prepass order
  exactly, yet 10 000 elements cost about ten draw calls per view: consecutive binary-alpha objects in sorted order
  are provably safe to batch.
- Voxels: the viewport draws the canonical non-greedy chunk mesh (AO interpolation, triangle diagonals, wireframe
  mode, picking and two exporters all depend on it). The budget in section 12 shows it meets the 256^3 sparse
  target; greedy meshing is an export algorithm and an opt-in performance mode, not the default.
- Undo: the original's snapshot semantics (11 entries, category masks, lazily built redo cache, quirky categories)
  with copy-on-write sharing (`Arc` per element, chunk, image), restored through the same apply path as file load.
- Time: a frame clock, a fixed 50 Hz clock and real-time timers reproduce deferrals, key repeat, fades and
  animations while the app still redraws only on demand.
- Flows: the original's coroutine-style operations (export with mid-way prompts, import with retries, the
  unsaved-changes prompt) become `async` functions on a UI-thread executor; heavy work runs on rayon or I/O threads
  with immutable snapshots, so the UI thread never blocks.
- Plan: nine milestones and about sixty work packages with disjoint file ownership, contracts first, sized for
  parallel AI coding agents (section 18).

---

## 1. Parity contract

### 1.1 Parity tiers

Every requirement in the ledger (1.4) carries one tier.

| Tier | Meaning | Typical scope | Check |
|---|---|---|---|
| P0 bit-exact | identical bytes or identical f32 bit patterns | `.cubik` payloads, every `cubik29` export, number text forms, values produced on exact arithmetic paths (snapping, quantisation, UV folding, colour packing, voxel indices) | golden bytes |
| P1 geometric | identical integer results and rectangles within 1/64 px | UI layout rects, hit areas, tool footprints (cells, texels), selection sets, list contents | conformance tests |
| P2 raster | per channel difference at most 2/255 on non-text pixels; at most 0.1 % mismatched pixels, only on edges | UI chrome with a skin pack, 2D overlays, unlit 3D | image diff with masks |
| P3 perceptual | SSIM at least 0.98 (threshold to be calibrated) | text glyphs, lit 3D, MSAA edges, shadows | image diff |
| P4 behavioural | same observable sequence: dialogs, button labels, status texts, undo labels, selection, dirty flag | flows and tools | scenario tests |

Two systematic exceptions to P0, both recorded in the ledger:

- Transcendental paths (rotation about a pivot, Euler decomposition, camera quaternions) cannot match a native
  engine bit for bit. They are compared with a tolerance of 1e-4 degrees or 1e-5 relative (native 4.3.2).
- Packed atlases (tools 5.4, interop 12.2) are compared by sampled pixels, as the specifications define.

### 1.2 Oracles

| Id | Oracle | Used for | Availability |
|---|---|---|---|
| O1 | Cubik Studio 2.9.480 on Windows | sample files, exports, screenshots, status strings, timing | maintainers only; captures stored outside the repository |
| O2 | private research data (UI hierarchy dump, sprites) | layout conformance, skin-pack pixel parity | local tests gated by environment variables; never committed |
| O3 | independent implementations (`bson`, `serde_json`, `dot_vox`, `tobj`, `fastnbt`, `gltf`, `image`) | differential tests: they must parse what we write and agree on what we read | dev-dependencies |
| O4 | vectors printed in the specifications | unit tests | in repository |
| O5 | in-repository reference implementations (slow, literal transcriptions of the spec) | differential tests against optimised code | `reference` modules behind `cfg(test)` or a `reference` feature |

Private oracles are read through `CUBOID_PRIVATE_UI_DUMP`, `CUBOID_PRIVATE_SPRITES` and `CUBOID_PARITY_REFS`. Tests
that need them report "skipped (oracle absent)" instead of passing.

### 1.3 Quirks, deviations and profiles

- Default rule: every QUIRK in the specifications is reproduced unconditionally inside the one semantic path. No
  switch, no branch, no "compat mode".
- A typed switch exists only where a specification names a decision: deviations D1 to D8 (app 0.2), the
  "cuboid fixes" items (for example the Model Properties restrict-flag defect, the Escape-with-modal-flag rule,
  custom key bindings applied at start-up, deep copy on paste), and anything a spec labels a port decision.
  All switches live in one struct, `cuboid_core::Compat`, with two constructors: `Compat::port()` (the shipped
  default) and `Compat::strict()` (used by parity tests whose reference comes from O1). Each field carries the
  spec reference in its doc comment and has a test for both values.
- `[FIX-ALWAYS]` items are fixed with no switch. `[FIX-MODERN]` and `[IMPORT-MODE]` items belong to the format
  profiles (`OutputProfile::Cubik29` default, `OutputProfile::Modern(target)`; `ImportMode::Cubik29` default,
  `ImportMode::Modern`), never to `Compat`.

### 1.4 Parity ledger

`parity/ledger.toml` lists every testable requirement with a stable id derived from the spec file and section
(`native.4.4.3.fold`, `tools.3.4.restricted-reject`, `ui.4.status-widths`), its tier, its status (`todo`,
`implemented`, `tested`, `verified-o1`, `unverified-spec`) and the names of the tests that cover it.
`cuboid-cli ledger check` (run in CI) fails when a `tested` entry names a test that does not exist in
`cargo test -- --list`, and prints coverage per spec document. Agents update the ledger in the same change as the
code; the ledger is the shared progress board and the definition of "100 %".

Companion files: `parity/strings.toml` (every user-visible string with its spec reference, including typos) and
`parity/compat.toml` (every `Compat` switch, default and spec reference). Tests assert the code tables equal them.

---

## 2. Design principles

1. **One semantic path.** Element creation, rotation storage, UV folding, voxel writes and texture registration
   each exist once. File load, undo restore, clipboard paste and importers call the same functions with the same
   arguments the original passed. Divergence between entry points was the main source of subtle parity bugs in
   comparable ports.
2. **Reference twin for every optimisation.** BVH face optimisation, parallel voxelisation, fast undo restore, the
   batched transparency renderer and the incremental atlas each keep a literal reference implementation;
   property tests prove identical results on random inputs.
3. **Numerics are centralised.** No parity-sensitive arithmetic outside `cuboid-math`; clippy forbids `as` casts
   from float to integer in other crates (`cast_possible_truncation` set to deny with targeted allows).
4. **Author in the original's units.** UI nodes use uGUI anchors, pivots and y-up offsets; scene math uses the
   left-handed, Y-up internal space; images are stored bottom row first; UVs have their origin bottom-left.
   Conversions happen once, at the edges (PNG I/O, egui's y-down screen space, Minecraft space in exporters).
5. **Pure core, thin shells.** `cuboid-core` is headless, deterministic and fully testable; the renderer and the UI
   are projections of its state plus input adapters. Nothing in the renderer writes back into the document.
6. **Deterministic and replayable.** Injectable RNG, ordered containers for anything that reaches output or
   affects palette order, fixed iteration orders, recorded input traces that replay headlessly.
7. **Never block the UI thread.** File I/O, parsing, meshing, voxelisation and encoding run on workers with
   immutable snapshots; the UI thread only mutates the document and encodes GPU work.
8. **Clean-room hygiene.** Implementers work from `docs/spec` only. No code, data tables or generated code come
   from the private research tree; private data appears only as an oracle in local tests (section 17).

---

## 3. Workspace and crates

### 3.1 Repository layout

```
cuboid/
  Cargo.toml              workspace: members, [workspace.package], [workspace.dependencies], [workspace.lints]
  rust-toolchain.toml     channel = "1.95.0"
  deny.toml               licence allow-list, advisories, bans, sources
  crates/
    cuboid-math/          engine-compatible numerics, number text forms, RNG
    cuboid-image/         RGBA images (bottom-up), stored pixel formats, BC1/BC3, raster primitives, packing
    cuboid-voxel/         voxel world, palette, writers, transforms, volume ops, canonical mesher, DDA
    cuboid-records/       plain records mirroring the native schema, retention handles
    cuboid-model/         document, objects, setters with original semantics, apply/capture, change journal
    cuboid-history/       generic snapshot history with the original's semantics
    cuboid-ops/           editing operations and tool algorithms (pure; reference twins)
    cuboid-scene/         cameras, views, navigation, picking, gizmo math, draw lists (CPU only, no wgpu)
    cuboid-core/          headless app core: commands, guards, selection, clipboards, interaction, Compat
    cuboid-io/            format registry, pipeline helpers, codec traits, emitters, permissive JSON, .lnk
    cuboid-fmt-cubik/     .cubik (ZIP + own BSON) and legacy .cub
    cuboid-fmt-minecraft/ Java JSON, resource pack, display presets, .mcpe, .js, .java, .jpm, entity atlas
    cuboid-fmt-mesh/      OBJ in/out, MTL, STL, PNG sprite import, PNG render export driver, glTF (modern)
    cuboid-fmt-voxel/     VOX, QB, QEF, binvox, schematic, NBT, ZOX, CSV, Stonehearth, Staxel, block tables
    cuboid-render/        wgpu renderer: offscreen views, pipelines, uploads, outline, PNG render
    cuboid-ui-kit/        uGUI-compatible layout, event system, widgets, skins, sprites, text metrics
    cuboid-ui/            the window: menus, panels, helpers, dialogs, file browser, strings catalogue
    cuboid-settings/      settings model, persistence, migration, paths, recent files, browser locations
    cuboid-assets/        own art, fonts (OFL), sounds, palettes, presets, default skin; embedded
    cuboid-shell/         winit + egui-winit + egui-wgpu integration, engine key map, headless capture
    cuboid-app/           binary `cuboid`: wiring, flow executor, jobs, backups, logging, platform glue
    cuboid-cli/           binary `cuboid-cli`: dump, convert, roundtrip, scenario, ledger
    cuboid-testkit/       dev only: golden harness, image diff tiers, canonical dumps, scenario DSL
  fuzz/                   cargo-fuzz targets; excluded from the workspace; nightly CI job only
  assets/                 source art and sounds (own work)
  parity/                 ledger.toml, strings.toml, compat.toml
  tests/golden/           committed golden fixtures (own fixtures only; O1 captures stay outside)
  docs/spec/  docs/design/
```

### 3.2 Crate responsibilities

| Crate | Owns | Must not |
|---|---|---|
| cuboid-math | rounding, snapping, angle wrap, Euler/quaternion conventions, engine equality, colour conversions and packing, hex text, G7/X9/NJ formatting, invariant parsing variants, `EngineRng` | depend on anything but `glam` |
| cuboid-image | `Image` (RGBA8, bottom-up rows), stored formats of native 4.5.1 (decode, encode, retain), BC1/BC3 decoding, file decoding (PNG, JPEG, TGA, DDS block skip), PNG encoding, texture signature (MD5 plus code page text), crop/mirror/nearest resize, raster primitives (Bresenham, disc, square, scanline flood fill), rectangle packing, padding bleed | know about elements or faces |
| cuboid-voxel | `VoxelWorld` (chunks, palette, active box, setup corners, flags), the three writers and two readers of tools 7.5, index/world mappings, flips and rotations, up/downscale, hollow/fill, component searches, greedy box decomposition, canonical chunk mesher with AO, export mesher, DDA ray cast | allocate chunks on reads (see 5.6) |
| cuboid-records | record structs for every table of native 4, "fresh" and "if absent" defaults, `Retained` handles, extension payload | contain behaviour beyond defaults |
| cuboid-model | `Document` and all object types, setters with the original's semantics, `apply_*` (load/restore/paste rules) and `capture_*` (save normalisations), selection set type, change journal, revisions, invariant checks | perform I/O, depend on UI, renderer or codecs |
| cuboid-history | `History<S>` with exact push/undo/redo/redo-cache semantics, `Categories` flags | know document types |
| cuboid-ops | every editing operation and tool algorithm of tools 4 to 14 as functions on `&mut Document` plus inputs; primitive mesh generators; reference twins | push undo entries or show messages (they return descriptors) |
| cuboid-scene | `ViewSet`, cameras, anchors, navigation, projection conventions, display-preview anchors, picking (BVH, single-sided triangles, voxel DDA on frozen snapshots), gizmo placement, hit volumes and drag-plane math, transparency sort keys, per-view draw lists | depend on wgpu |
| cuboid-core | `Core` facade: `Command` execution (guard, push, mutate, post), selection operations, clipboards, element start queue, interaction state machine (tools 8.3), custom tools, `Compat`, scenario hooks | depend on egui, winit or wgpu |
| cuboid-io | `Registry` (interop 4), lookup rules, pipeline helpers (validation texts, loading-box rule, status texts), `Importer`/`Exporter` session traits, prompts, profiles, text writers (UTF-8 without BOM, LF/NL policy), hand-built JSON emitters, the permissive JSON dialect parser, `.lnk` target parser | write files itself (it returns artifacts) |
| cuboid-fmt-* | one module per format, each with its own fuzz target and golden tests | share mutable state; call UI |
| cuboid-render | device-level resources, offscreen targets, pipelines and WGSL shaders, scene upload with dirty tracking, outline pass, viewport blit callback, offscreen PNG render and readback | read `Document` directly (it consumes `cuboid-scene` draw lists) |
| cuboid-ui-kit | layout engine, event system, focus and navigation, widgets, skins, sprite atlas, nine-slice painter, state-colour tweens, text metrics and wrapping | know about models or files |
| cuboid-ui | concrete window: menu data and mechanics, panels, helpers, dialogs, file browser, tooltips, status bar, strings catalogue | mutate the document except through `Core` commands and flows |
| cuboid-settings | `Settings` (every key of app 10.2 plus port keys), load/save, migration from the original's files and registry values, paths, recent files, browser locations | depend on UI |
| cuboid-shell | event loop, window rules (size, minimum, DPI scale, cursors, drag and drop, full screen), egui-winit state, egui-wgpu renderer, engine key mapping, wake-up scheduling, headless offscreen mode | contain application logic |
| cuboid-app | `App` state, flow executor, job system, backup loop, command line, logging, audio, OS integration | contain format or model logic |

### 3.3 Dependency graph

Arrows read "is used by". The graph is acyclic; `cuboid-history` and `cuboid-math` sit at the bottom.

```
cuboid-math ──┬──────────────┬───────────────┬────────────────┐
              v              v               v                v
        cuboid-image   cuboid-voxel   cuboid-records   cuboid-history (no deps)
              └──────────────┼───────────────┘                │
                             v                                │
                        cuboid-model                          │
              ┌──────────────┼────────────────┐               │
              v              v                v               │
         cuboid-ops     cuboid-scene      cuboid-io           │
              │              │                │               │
              └──────> cuboid-core <──────────┼───────────────┘
                             │         cuboid-fmt-cubik / -minecraft / -mesh / -voxel
                             │                │
        cuboid-render <── cuboid-scene        │        cuboid-settings
              │                               │               │
        cuboid-ui-kit                         │               │
              │                               │               │
              └──> cuboid-ui <── core, scene, render (callback type), io, settings
                        │
                  cuboid-shell (winit, egui-winit, egui-wgpu, wgpu)
                        │
                   cuboid-app (bin) <── all fmt-* crates, cuboid-assets
   cuboid-cli (bin) <── core, io, fmt-* (no UI, no GPU)
```

Dependency inversion where a format needs services: the PNG render exporter and the resource-pack icon call a
`RenderService` trait defined in `cuboid-io` and implemented by `cuboid-app` (on top of `cuboid-render`).
File dialogs, message boxes and clocks reach flows through `cuboid-app`, never through codecs.

### 3.4 External dependencies (verified with `cargo info`, 2026-10-18)

| Crate | Version | Licence | Used by | Purpose and notes |
|---|---|---|---|---|
| egui | 0.37.0 | MIT OR Apache-2.0 | ui-kit, ui, shell | painter, text layout, input; `default-features = false` (no bundled fonts) |
| epaint | 0.37.0 (via egui) | MIT OR Apache-2.0 | ui-kit | text via skrifa 0.44 + harfrust 0.12 + vello_cpu 0.2; hinting and sub-pixel binning are configurable (`TextOptions`, `FontTweak`), used for text calibration (10.6) |
| egui-winit | 0.37.0 | MIT OR Apache-2.0 | shell | winit to egui input, IME, clipboard for text fields |
| egui-wgpu | 0.37.0 | MIT OR Apache-2.0 | shell, render | egui renderer; depends on wgpu 30.0; prefers `Rgba8Unorm`/`Bgra8Unorm` (gamma) framebuffers, which matches the original's gamma pipeline |
| wgpu | 30 | MIT OR Apache-2.0 | render, shell | the version egui-wgpu 0.37 uses (must stay identical) |
| winit | 0.30.13 | Apache-2.0 | shell | latest stable (0.31 is beta); the version egui-winit 0.37 needs |
| glam | 0.34 | MIT OR Apache-2.0 | math and up | f32 vectors and quaternions (parity); f64 only for non-persisted derived values |
| serde, serde_json | 1 | MIT OR Apache-2.0 | settings, presets, modern formats | not used for `cubik29` writers (hand-built emitters, 8.5) |
| thiserror | 2 | MIT OR Apache-2.0 | all libraries | error enums |
| log | 0.4 | MIT OR Apache-2.0 | all | logging facade |
| indexmap | 2 | MIT OR Apache-2.0 | model, io, settings | ordered sets and maps (selection order, JSON key order) |
| rustc-hash | 2 | MIT OR Apache-2.0 | model, voxel, render | fast maps where iteration order never reaches output |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | records, model, history, io | face flags, change categories, capabilities |
| slotmap | 1.1.1 | Zlib | model | generational keys; only `DefaultKey` wrapped in own newtypes, because `new_key_type!` expands to an `unsafe impl` that `forbid(unsafe_code)` rejects |
| rayon | 1.12.0 | MIT OR Apache-2.0 | ops, voxel, render, app | CPU parallelism |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | render | only `cast_slice` on `[u32]`/`[f32]` words; no `derive(Pod)` (expands to `unsafe impl`) |
| image | 0.25.10 | MIT OR Apache-2.0 | image | PNG, JPEG, TGA decoding; PNG encoding; `default-features = false` with those codecs only |
| zip | 9.0.5 | MIT | fmt-cubik, fmt-minecraft, app | `default-features = false, features = ["deflate-flate2-zlib-rs"]` (pure Rust, no C codecs) |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | fmt-voxel | gzip for schematic and structure files |
| md-5 | 0.11.0 | MIT OR Apache-2.0 | image | texture content signature (native 4.5) |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | image, fmt-voxel | ANSI code pages for the signature text and QB names |
| fastnbt | 2.6.3 | MIT OR Apache-2.0 | fmt-voxel | NBT; root-name access to be confirmed in WP-3.9 (fallback: a small own reader) |
| dot_vox | 5.2.0 | MIT | fmt-voxel | modern VOX import (scene graph) and test oracle |
| tobj | 4.0.5 | MIT | fmt-mesh | modern-mode OBJ import and test oracle |
| gltf | 1.4.1 | MIT OR Apache-2.0 | fmt-mesh | modern glTF export (milestone M8) |
| etagere | 0.3.0 | MIT OR Apache-2.0 | render, image | atlas rectangle allocation |
| rfd | 0.17.2 | MIT | app | native dialogs when "Use Native file browser" is on |
| directories | 6.0.0 | MIT OR Apache-2.0 | settings | config, data and cache folders; pulls an MPL-2.0 helper (allowed in `deny.toml`) |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | settings, app, fmt-* | atomic writes (temporary file in the target folder, then persist) |
| jiff | 0.2.38 | Unlicense OR MIT | app, ui | local weekday names for backups, UTC dates in the file browser |
| opener | 0.9.0 | MIT OR Apache-2.0 | app | open folders, files and URLs; reveal files |
| pollster | 1.0.1 | Apache-2.0/MIT | shell | block on adapter and device requests at start-up |
| windows-registry | 0.100.0 | MIT OR Apache-2.0 | settings, app (Windows only) | legacy preference migration, ANSI code page, Steam path, explicit per-user file association |
| rodio | 0.23.0 (cpal 0.18.2) | MIT OR Apache-2.0 / Apache-2.0 | app (feature `audio`, default on) | message-box sounds at volume 0.4 |
| ureq | 3.4.2 | MIT OR Apache-2.0 | app (feature `net`, off until M8) | opt-in network features only (Sketchfab, skin lookup, update check) |
| bson | 3.1.0 | MIT | fmt-cubik (dev-dependency only) | BSON oracle for differential tests |
| proptest | 1.12.0 | MIT OR Apache-2.0 | dev | property and differential tests |
| insta | 1.49.0 | Apache-2.0 | dev | reviewed text snapshots of canonical dumps |
| criterion | 0.8.2 | Apache-2.0 OR MIT | dev | benchmarks |
| egui_kittest | 0.37.0 | MIT OR Apache-2.0 | dev (ui-kit) | widget-level interaction tests (features `wgpu`, `snapshot`) |
| arbitrary | 1.5.0, libfuzzer-sys 0.4.13 | MIT OR Apache-2.0; NCSA | fuzz only | fuzz targets (not shipped) |

Tools (not dependencies): cargo-deny 0.20.2, cargo-fuzz 0.13.2, cargo-nextest 0.9.148, cargo-packager 0.11.8.

### 3.5 Considered and not used

| Candidate | Decision | Reason |
|---|---|---|
| eframe 0.37 | not used | no access to keypad versus top-row or left/right modifier identity (`egui::Key` merges `Numpad1` into `Num1`, `NumpadAdd` into `Plus`; `Modifiers` has no sides); the original's bindings depend on both (menus 6.1, 6.4). The own shell also owns wake-up scheduling, the DPI rule, cursors and headless capture. It still uses egui's official integration crates. |
| bson as runtime dependency | dev-only oracle | byte-identical rewrite needs element-level raw spans, the original's lenient typed reads, duplicate-key and quaternion-order semantics, and explicit limits; an own reader/writer is about 700 lines and fully fuzzed |
| tobj, dot_vox for `cubik29` import | modern mode and oracle only | the original's OBJ tokeniser, index and triangulation quirks (interop 15.2) and its flat VOX chunk walk and palette rules (19.1) cannot be expressed through these APIs |
| stl_io | not used | the `cubik29` writer is 40 lines with a custom header rule |
| quick-xml | not used | no XML format in the parity scope |
| arboard (direct) | not used | the original never puts model data on the OS clipboard (app 14); text fields use egui-winit's clipboard |
| egui_extras | not used | the own layout engine replaces strips and tables |
| notify | not used | the original checks texture signatures only when the window regains focus (native 4.5) |
| rand | not used | `EngineRng` (xorshift, seedable) is enough and keeps tests deterministic |
| chrono, winreg | not used | jiff and windows-registry cover the needs |
| smallvec 2 | not used | beta release |
| tokio | not used | no async I/O need; the flow executor is a small local executor (10.12) |

### 3.6 Workspace conventions

- Edition 2024, `rust-version = "1.95"`, toolchain pinned to 1.95.0. Every crate starts with
  `#![forbid(unsafe_code)]`; the only exception is `fuzz/` (libFuzzer entry points need unsafe attributes), which is
  not shipped.
- `[workspace.lints]`: `unsafe_code = "forbid"`; clippy `all` as warnings and CI with `-D warnings`;
  `await_holding_refcell_ref = "deny"` (flow executor discipline); `unwrap_used`, `expect_used`, `panic` and
  `indexing_slicing` denied in non-test code of `cuboid-fmt-*`, `cuboid-io`, `cuboid-image` and `cuboid-voxel`
  (parsers must never panic); `float_cmp` allowed only inside `cuboid-math`; float-to-int `as` casts denied outside
  `cuboid-math`.
- Parity-relevant code cites its source in a doc comment: `/// spec: tools 3.4 (rotation setter)`.
- Libraries return `Result<_, CrateError>` built with thiserror; `anyhow`-style catch-all errors only in binaries.
- Release profile: `lto = "thin"`, `codegen-units = 1`, `panic = "unwind"` (job isolation, section 11),
  `debug = "line-tables-only"` with split debug info for crash reports.

---

## 4. Numerics: the engine-compatibility layer (`cuboid-math`)

### 4.1 Float model

- Stored document values are f32, exactly as in the original. Derived non-persisted values may use f64.
- **UNVERIFIED risk:** the specifications state that arithmetic is single precision. The original's runtime
  generation may have evaluated float expressions with wider intermediates, rounding only when a value is stored.
  Results differ only at rounding ties (snapping at exact .5 multiples, X9 text of computed values). Mitigation:
  every parity formula is written once, in `cuboid-math`, through small helpers whose evaluation policy is a
  compile-time constant (`Eval::Strict32` default, `Eval::Wide64` alternative: evaluate in f64, round to f32 at the
  store points the spec names). Golden vectors captured from O1 (snapping ties, rotated-element exports for X9)
  decide the policy; switching it touches one crate.
- No fused multiply-add (`mul_add` is never used), no reassociation, no "fast math"; operation order follows the
  spec formulas. Rust's float `%` already has the sign-of-dividend semantics the specs require.

### 4.2 Primitives

| Function | Rule (spec) |
|---|---|
| `round_half_even`, `trunc_toward_zero` | every "round" and every integer cast (tools 1.6, interop 3.2) |
| `snap_angle(v, restricted)` | 22.5 or 5 degree grid, half-to-even (tools 3.4) |
| `wrap_degrees` | `(c + 360) % 360`, sign-of-dividend remainder, inputs below -360 stay negative |
| `normalize_uv_rotation` | `(round(r / 90) * 90 + 360) % 360` |
| `clamp_uv_corner`, `fold_uv` | [-1, 2] clamp on write; fold rules of native 4.4.3 on read |
| `round4` | `round(v * 10000) * 0.0001` (drag clean-up, unrotated corner) |
| `engine_eq_vec3` | squared distance below 9.99999944e-11 (engine vector equality) |
| `euler_to_quat`, `quat_to_euler` | `q = qY * qX * qZ`, degrees; decomposition plus the positivity rule of native 4.3.2 |
| `closest_pow2`, `next_pow2` | entity texture resolution rules (interop 2.7) |
| `color_f32_to_u8` | `trunc(clamp01(c) * 255)` |
| `pack_argb_*` | four variants: element, sub-mesh, voxel, tool packing (native 4.10, tools 7.3) |
| `hex_to_color`, `color_to_hex` | tolerant parser and `#RRGGBBAA` upper-case writer (tools 7.3) |

### 4.3 Number text

- `numtext::g7`, `numtext::x9`, `numtext::nj` implement the three text forms of interop 3.1; `parse_invariant_or_zero`
  implements "try parse, 0 on failure" for fields; `parse_obj_number` reproduces the OBJ importer's acceptance of
  thousands separators; the permissive JSON dialect has its own token rule (interop 8.6).
- Digit generation must reproduce the original runtime's formatter, which is not guaranteed to be correctly
  rounded (**UNVERIFIED**, T-NUM-1). The implementation computes digits from the exact binary value with an
  explicit rounding stage so that a "round to 15 digits first" variant can be enabled if O1 vectors require it.
- Tests: spec examples; round trip of f32 patterns through `nj` (NJ must parse back to the same bits), sampled in
  CI and exhaustive over all 2^32 patterns in the nightly job; proptest for `g7`/`x9` against a slow
  arbitrary-precision reference.

### 4.4 Randomness

`EngineRng` (xorshift128, seedable) is owned by `Core` and passed explicitly to every operation that draws random
values: random element colours, blank-atlas colours, Randomize, noise jitter, z-fighting jitter, sprite colours.
Production seeds from the clock; tests seed fixed values or mask the affected fields (tools 16.5).

---

## 5. Core data model (`cuboid-model`, `cuboid-voxel`, `cuboid-records`)

### 5.1 Document aggregate and ownership

```rust
pub struct Document {
    pub props: ModelProps,           // mp groups, model origin, model box (live), overrides (not in .cubik)
    pub view: ViewProps,             // grid steps, snap flag, captured camera states
    pub blueprint: Blueprint,
    elements: ObjectList<Element>,   // list order = panel, file, export order
    meshes: ObjectList<Mesh>,
    textures: TextureList,           // ordered, unique labels, hidden "error" entry
    voxels: Option<VoxelWorld>,      // None = not initialised
    pub display: DisplaySlots,       // eight slots
    journal: ChangeJournal,          // drained once per frame (5.10)
    revision: Revision,              // bumps on every mutation; used to validate background results
}
```

- Objects, chunks and images are held behind `Arc` and mutated with `Arc::make_mut`. A `Document` clone is a
  snapshot that costs one pointer copy per object (10 000 elements: about 50 microseconds) and shares everything
  untouched. `Document: Send + Sync`, so snapshots travel to workers.
- The UI thread is the only writer. Background jobs receive snapshots or owned buffers and return results that the
  UI thread applies after checking the revision.

### 5.2 Identity

- `ObjectList<T>` = `SlotMap<DefaultKey, Arc<T>>` plus `order: Vec<DefaultKey>` plus the per-kind display-id
  counter. Public keys are newtypes (`ElementKey`, `MeshKey`) so they cannot be mixed.
- `ObjectRef = Element(ElementKey) | Mesh(MeshKey) | Blueprint | ModelOrigin` is what selection, hover and gizmos
  refer to.
- Display ids follow tools 2.1 exactly: assigned at creation from the counter, never stored, reset to 0 when all
  elements are cleared (new model, load, undo/redo restore), so they are re-issued in list order.

### 5.3 Elements, faces and UVs

- Fields follow tools 3.1 and 4.1. `Face { name, active, tint, cull: FaceFlags, texture: Label, uv: UvRect }` and
  `UvRect { rotation: i32, tl_raw: Vec2, br_raw: Vec2 }` with folding getters; raw (clamped) and folded values stay
  distinct, and everything downstream (mesh UVs, files, copies) reads folded values.
- All mutation goes through `ElementMut<'_>`, a guard that applies the original setter semantics (propagation to
  the selection, pivot-relative rescale and 0.001 clamp, rotation wrap/quantise/restrict and pivot rotation,
  UV-snap triggers and turn-off rules), records change flags in the journal and drops the retained raw record when a
  value really changes.
- Invariants asserted in debug builds and tests: setter-produced sizes are at least 0.001; stored rotations lie on
  the 5 or 22.5 degree grid; active-face count equals the face mask; display ids strictly increase in creation
  order; no reachable voxel outside the grid; active box half-open; palette starts with 0. Paths that bypass the
  setters on purpose (transform dialogs, tools 10.13) are marked and excluded from the size invariant.

### 5.4 Textures

- `TextureEntry { label, image: Arc<Image>, source: { file_path, file_name, mc_path, signature, file_size },
  retained: Option<RetainedRaw> }`. `RetainedRaw` keeps the stored format code, dimensions and raw bytes (mip chains
  included) of a texture read from a `.cubik` file and not modified, so it is rewritten byte for byte.
- `Image` stores rows bottom to top (the original's convention). Conversions happen only at PNG/JPEG/TGA I/O and
  when building GPU uploads; UV and atlas math never flips.
- The runtime atlas is renderer state (9.6), but its document-visible effects are model semantics in `cuboid-ops`:
  the width-only downscale of oversized images on insertion (tools 5.2) uses a fixed slot limit (default 8192,
  configurable; **UNVERIFIED** which device limit the original saw), independent of the local GPU.
- Texture signature (`cs`): opaque string; recomputed only for textures the port loads itself, with MD5 bytes
  decoded through the platform ANSI code page (read from the registry on Windows, Windows-1252 elsewhere;
  **UNVERIFIED** for bytes the original runtime decoded differently).

### 5.5 Meshes

`Mesh { base, kind, geometry, main_texture, submeshes, model_bounds, import_offset, norm_factor, imported }` with
`geometry = Custom(Arc<MeshGeometry>) | Primitive`. Primitive meshes are generated by `cuboid-ops::primitives`.
Their exact vertex layouts are not in the specifications (**UNVERIFIED**, risk R3); generators are parametric and
calibrated from OBJ exports of each primitive captured from O1, because geometry reaches rendering, voxelisation
and OBJ/STL output.

### 5.6 Voxel world

- `VoxelWorld { resolution, chunks: Vec<Option<Arc<Chunk>>> (dense index over the chunk grid), palette, palette_index,
  active_box, setup_corners, ao, initialised, world_dirty }`, `Chunk { cells: [u32; 4096], dirty, touched }`.
  Dense chunk indexing gives O(1) access and the deterministic (cx, cy, cz) order that saving, meshing and palette
  rebuilds need.
- `touched` reproduces the original's "empty flag" semantics for saving: set by any non-zero write, cleared only by
  a full clear. A chunk is written to the file iff touched (native 4.7). Lazy allocation by reads, which the
  original performs, is not emulated because it never changes saved output.
- Writers: full (box check, palette append, neighbour dirtying), bounded raw, unchecked raw (tools 7.5). The port
  dirties face, edge and corner neighbours (tools 7.5 port note).
- Palette: ordered `Vec<u32>` starting `[0]` plus a lookup map; append order equals write order, which is why every
  parallel algorithm replays its writes in the original scan order (11.3).

### 5.7 Special objects

Blueprint (default (-2, 0, 2), size 1, not deletable, origin equals position), model origin (writes `g.o` on move),
model box (six clamped setters, handle order, not undoable) follow tools 2.3 and 2.4.

### 5.8 Selection and interaction state

`Selection { list: IndexSet<ObjectRef>, primary, hovered, active_axis, default_axis, busy, grabbed, clone_guard }`
with the operations of tools 8.1, including the documented quirks (select-all keeps a stale primary, box apply by
origin, Shift-range by display id).

### 5.9 Records and the single apply path

- `cuboid-records` mirrors the schema tables of native 4 one to one (`DocumentRecord`, `ElementRecord`,
  `FaceRecord`, `TextureRecord`, `MeshRecord`, `VoxelDataRecord`, `DisplayRecord`, `CameraRecord`, ...), with the
  "fresh" and "if absent" defaults as constructors and `Retained` handles for byte-exact rewrites.
- `cuboid_model::apply::apply_document(doc, record, mode)` implements native 6.2 and 6.3; `mode` is
  `Open | Merge | Restore(categories) | Paste { offset }`. `capture_document(doc, scope)` implements native 5.2 and 5.3.
- Used by: `.cubik` load and save, automatic backups, undo restore, clipboard copy and paste. Rotation
  re-quantisation, size handling, `is` re-selection, palette rebuild, "start" UV mapping and id renumbering are
  therefore identical in all four places, as in the original, which reuses the document shape for undo and
  clipboard data.
- Fast paths (for example restoring voxel chunks by `Arc` instead of replaying every cell through the full writer)
  must produce the same document as the reference replay; a proptest compares them (principle 2).

### 5.10 Change journal

Per object `ChangeFlags { TRANSFORM, GEOMETRY, FACES, UV, COLOR, VISIBILITY, ORDER, LABEL }`; per document
`DocFlags { TEXTURES, ATLAS, VOXEL_CHUNKS(set), MODEL_BOX, DISPLAY, VIEW, SELECTION, LISTS }`. Drained once per frame
by scene sync (renderer uploads), list panels, the status bar and the title. The journal replaces the original's
per-object refresh calls and keeps idle frames free.

---

## 6. Commands, undo and clipboards

### 6.1 Command layer (`cuboid-core`)

- `Command` is an enum grouped by domain (elements, objects, meshes, textures, voxels, paint, view, recover, model).
  `Core::execute(cmd) -> CmdOutcome` runs the original's order: guard (selection kind, primary kind, voxels set up;
  failure returns a message request), undo push (label and categories, also when nothing will change where the
  spec says so), mutation through `cuboid-ops`, post-processing (start queue, journal flags).
- A static `COMMAND_TABLE` lists id, undo label template, category rule (fixed, or by selection kind) and guard
  kind. A test compares it with the undo label catalogue of tools 15.6 and the guard tables of menus 4.
- Commands that need confirmation or dialogs are flows (10.12) that call `execute` for the final step.
- `CmdOutcome` carries UI requests (message box, status text, panel refresh) as data; `cuboid-core` never draws.

### 6.2 History (`cuboid-history`)

- Exact semantics of tools 15.1: push truncates at the cursor and clears the redo cache, caps the list at 11 entries,
  marks the document dirty; undo appends a current-state snapshot to the redo cache only when the cache is shorter
  than `length - cursor`; redo restores `redo_cache[redo_count]` after decrementing; undo and redo never touch the
  dirty flag; new model and load clear everything.
- `Categories` bit values are the original's (1, 2, 4, 8, 16, 32, 64; named combinations 12, 20, 36, 40, 52, 255).
- Snapshot representation: `Snapshot { categories, props, view, elements: Option<ElementsSnap>, textures, voxels,
  meshes, display }` where `ElementsSnap = { blueprint, items: Vec<(Arc<Element>, selected: bool)> }` (all elements,
  or only the selected ones for copy and clone). Voxels are a frozen `Arc` chunk vector; textures and meshes are
  `Arc` lists. A push costs O(objects) pointer copies; memory grows only with what changes afterwards.
- Restore converts the snapshot into records and runs `apply_document` with `Restore(categories)` (deep copies of
  faces, port decision of tools 11.4). The known capture gaps of tools 15.5 (mesh changes missing from "Rotate model",
  clone mode recording elements only, and so on) are reproduced by the category rules in `COMMAND_TABLE`.

### 6.3 Clipboards

All in process, never the OS clipboard (app 14): object clipboard (snapshot subset plus copy anchor), voxel
clipboard (relative cells), face-property clipboard plus shade, UV clipboard of the UV tab, mesh texture and colour
clipboards, colour-picker presets. New model and replace-open clear the ones the specs name.

### 6.4 Start queue

Elements created, pasted, loaded or restored run `defaultUV(false)` once when they "start" (tools 4.7). Keys are
queued and processed at the end of the command or tick, before the next render, which gives the same visible and
saved result as the original's next-frame start.

---

## 7. Actions, menus, shortcuts, input and time

### 7.1 Action registry

`ActionId` enumerates every action of menus 2, 3 and 5 plus the hidden bindings (Delete with confirmation, F2, F3).
`ACTIONS: [ActionDef]` stores the original action name (traceability and tests), the handler group (file, edit,
objects, elements popup, mesh, voxel, texture, material, palette, view, tools, window, help) and the kind
(command, flow, UI toggle). Rows reached from a menu click or a shortcut run 0.05 s later (menus 1.5); toolbar
buttons call actions directly (menus 5.14). The three dead rows exist and do nothing.

### 7.2 Menu data

`MENUS` holds the bar order, minimum widths, rows (label, shortcut text, icon, action or child, flags A/U/L), the
dynamic rows of menus 1.7 and the enable model of 1.6 (menu level, row level matched by original label, linked
buttons). The data is transcribed from the specification tables; a test diffs labels, shortcuts and flags against
`parity/strings.toml`.

### 7.3 Engine keys

`cuboid-shell` maps each winit `KeyEvent` (physical key plus `KeyLocation`) and mouse button to an `EngineKey`
named like the original engine's key codes (`Alpha1`, `Keypad1`, `KeypadPlus`, `LeftAlt`, `RightControl`,
`Mouse0`..`Mouse6`, `F1`..`F15`, `Return`, `Delete`, `PageUp`, ...). It maintains the held set and the press order.
This is the reason for the own shell (3.5).

### 7.4 Shortcut dispatch and the input map

- Menu bindings: shortcut texts are canonicalised once (menus 6.1). Dispatch follows menus 6.2 with the spec's
  recommended event-ordered variant (each key-down processed in arrival order), gated by the global modal flag, the
  "UI control holds keyboard selection" rule (unless works-in-UI), menu enablement and always-active flags. The
  frame-based quirks (one key per frame, mouse press cancels the frame) are `Compat` switches for strict parity runs.
- Held-key input map: ModeRotate, ModeScale, ModeClone, PanView, Focus, ModifySelection, ModifySelectionMultiple,
  RotateView, arrow moves, Zoom, Submit, Cancel (menus 6.4). The five customisable strings are parsed and
  canonicalised per menus 6.6 and applied at start-up (deliberate fix, `Compat` switch).

### 7.5 Interaction state machine

`cuboid_core::interact` implements tools 8.3 to 9.6 as a pure state machine. Input per frame: `InputFrame` (held
bindings, buttons with edges, pointer position, wheel, text-focus flag, pointer-over-UI flag, active view,
time); queries through `cuboid_scene::Raycaster`; output: mutations through `ElementMut`/ops, the tool cursor
symbol, status texts. Fixed-step work (hover pick, pending box apply, arrow-repeat counter) runs from the fixed
clock. Paint, colour-picker and voxel tools implement `trait CustomTool { fn stroke(&mut self, cx, button,
phase, hit) }` with the stroke lifecycle of tools 12.2 and 13.2 (begin, Left Alt eyedropper, same-face gate, dedupe,
plane pin, preview cache with its 4096-entry quirk).

### 7.6 Time model

- `Clock { unscaled_time, frame_dt, fixed: FixedStepper { dt: 0.02, max_frame_dt } }`. The 0.02 s step is the
  original's configured value; the per-frame catch-up cap uses the engine default of one third of a second
  (**UNVERIFIED**).
- Timers in seconds: action deferral 0.05 s; colour fades 0.1 s; caret period 1/0.85 s; status reset to "Ready"
  after 10 s counted in whole seconds; list double-click 0.25 s; file-browser double-click 300 ms; arrow auto-repeat
  after more than 15 fixed steps; overlay alpha steps at 50 Hz; dimming-layer interpolation; floor-preview
  animation; backup interval `max(300, minutes * 60)` s.
- The shell computes the next wake-up from pending timers, animations and jobs (`ControlFlow::WaitUntil`); it
  redraws on input, timer expiry or journal changes, capped at the display rate when focused and 10 Hz when not
  (app 4.5). Fixed steps catch up on the next frame, so idle costs nothing.

---

## 8. File formats

### 8.1 Registry (`cuboid-io`)

`Registry` holds the 23 entries of interop 4.1 in order (order is semantic), with capabilities, labels, filters,
message names and visibility rules (removed versus hidden from lists, built-in-browser-only `.lnk`). Lookup is
case-sensitive with every `.` removed in `cubik29` and case-insensitive in modern import mode; `png` resolves to
Sprite when opening and Model Render when saving. Dialog filter lists are computed from capabilities exactly as
interop 4.4.

### 8.2 Codec traits

Importers and exporters are sessions, because the original asks questions in the middle of both.

```rust
pub trait Importer: Sync {
    /// Off-thread: read and parse; no document access.
    fn begin(&self, src: Source, cx: &ImportCx) -> Result<Box<dyn ImportSession>, IoError>;
}
pub trait ImportSession: Send {
    fn prompt(&mut self) -> Option<Prompt>;          // e.g. parent prompts, entity name with Retry
    fn answer(&mut self, answer: Answer) -> Step;    // Continue | Stop(UserMessage)
    /// Off-thread: mutate a working copy of the document through the normal model and ops API.
    fn apply(self: Box<Self>, doc: &mut Document, cx: &mut ApplyCx) -> Result<Outcome, IoError>;
}
pub trait Exporter: Sync {
    fn begin(&self, snapshot: Arc<DocSnapshot>, cx: &ExportCx) -> Box<dyn ExportSession>;
}
pub trait ExportSession: Send {
    fn prompt(&mut self) -> Option<Prompt>;          // pre-checks in spec order
    fn answer(&mut self, answer: Answer) -> Step;
    fn build(&mut self, progress: &mut dyn ProgressSink) -> Result<Artifacts, IoError>; // off-thread, pure
    fn post_build_prompt(&mut self) -> Option<Prompt>; // e.g. skipped-elements warning
}
pub struct Artifacts { pub files: Vec<OutFile>, pub notices: Vec<Notice> } // ordered writes, non-blocking boxes
```

- Imports are atomic by construction: `apply` runs on a copy-on-write clone; the UI thread swaps it in only on
  success and only if the base revision is unchanged. This satisfies the specs' "MUST import atomically" while the
  visible prompts and statuses stay those of the original (a failed `cubik29` JSON import therefore no longer
  leaves an empty model; the specs require this deviation).
- Progress events (`Status(text)`, `Progress(f32)`, `Log(text)`, `Notice(box)`) reproduce the original's texts and
  cadence ("Exporting Elements: p%" every 200 elements, voxel progress every 262 144 cells, ...).
- Profiles: `OutputProfile::{Cubik29, Modern(target)}` per export, remembered per format; `ImportMode::{Cubik29,
  Modern}`. App identity text is configuration; golden tests set the reference identity (interop 2.8).

### 8.3 Pipeline flows

Open, Merge, Import, recent files, drag and drop and the command line share one flow: validation messages (missing
extension, unknown type, cannot read), recent-list insertion before loading (never `.lnk`), the "Loading..." box
above 307 200 bytes, a wait of one fixed step, the parse job, prompts, the atomic apply, statuses (`Imported`/
`Loaded`), `ep` update and the wait-box closing rule (interop 5.2). Export follows interop 6.2, including the
inverted start wording, writer-helper messages and the final `Exported` status even after failures in `cubik29`
(`[FIX-MODERN]` in modern profiles). The original adopts the path of a file that failed to load; the port does not
(native 6.1 requires it).

### 8.4 Native `.cubik`

- **Container.** Reading accepts stored and deflated entries, data descriptors and ZIP64, finds `cubik` exactly and
  then case-insensitively, ignores other entries, and reports "not a ZIP", "no entry" and "corrupt entry"
  separately. Writing creates one `cubik` entry (deflate, fast level), no encryption, no ZIP64 unless needed, through
  a temporary file and rename. An optional `cuboid.json` entry carries port-only data (item overrides, profile
  memory) with its own version; a "Cubik-compatible save" setting omits it (native 8.4).
- **BSON engine** (`cuboid-fmt-cubik::bson`): a zero-copy reader over `&[u8]` yielding `(type, key, value span)`;
  typed lenient getters implementing native 3.6 (int64 and double to f32, half-even to i32, base64 strings and
  subtype 2 for binaries, case-insensitive enum names, null handling with warnings); duplicate keys last-wins; the
  quaternion override (members applied in document order, `eulerAngles` replaces the components); explicit limits
  (document size, depth 64, array lengths, texture `w * h`, voxel resolution against a memory budget) checked before
  allocation. The writer emits keys in schema order, widens f32 to f64 exactly, writes enums and the `v` byte as
  int32, and patches each document size when it closes (single pass).
- **Retention.** Each decoded record and section keeps a `Retained { buf: Arc<[u8]>, span, unknown: Vec<(after_key,
  span)> }`. When the model object is still clean, the writer copies the span verbatim; when it is dirty, it writes
  the record from values and re-inserts unknown elements after the known key they followed. This makes the
  "payload identity" test of native 10.2 pass for unmodified files and preserves third-party extensions.
- **Normalisations** of native 5.3 live in `capture_document`; read-side normalisations of native 6.3 live in
  `apply_document`. Both are shared with undo and clipboard (5.9).
- **Texture write policy.** Retained textures are written unchanged; edited or created textures are written as
  RGBA32 (`f = 4`) per native 4.5.1. The original writes atlas-recut ARGB32 data after paint strokes; a `Compat`
  switch reproduces that for strict file comparisons.

### 8.5 Legacy `.cub` and the permissive JSON dialect

`.cub` decoding follows native 9: BOM detection, UTF-16 code units, XOR with the two published keys, then the
permissive dialect parser of `cuboid-io::jsonlite`, shared with the `cubik29` Minecraft JSON importer (interop 8.6):
only `\uXXXX` decoded, number tokens up to the next delimiter, first-letter literals, last-wins duplicates, trailing
commas accepted, fixed "Invalid data found" message. Modern import mode uses `serde_json`.

### 8.6 Text emitters

- `emit::McJson`: hand-built Minecraft model writer (tabs, LF, the trailing-whitespace oddities, minify rule) with
  the `[FIX-ALWAYS]` escaping of quotes, backslashes and controls.
- `emit::NetJson`: the original serializer's style over a small ordered value tree: two-space indentation, CRLF
  newlines, `": "`, compact primitive arrays inside indented output, NJ floats, omitted nulls, declaration order,
  compact mode.
- `emit::Lines`: line writer with an explicit ending per line (LF, or NL rendered as CRLF on every platform in
  `cubik29`, interop 3.3), used by OBJ, MTL, QEF, CSV and the `.js` header.
- All text files: UTF-8 without BOM, existing files overwritten, null text written as an empty file.

### 8.7 Per-format implementation choices

| Format | `cubik29` implementation | Modern / import mode | Oracle tests |
|---|---|---|---|
| Minecraft JSON export/import | own emitter / own dialect parser | serde_json with version targets | serde_json validates output |
| Resource pack | zip writer, fixed texts, icon via `RenderService` | per-version `pack.mcmeta` | zip reader lists entries |
| `.mcpe` import, `.js`, `.java`, `.jpm` | own (NetJson for `.js`/`.jpm`) | Bedrock geo, JEM (M8) | serde_json parses `.js` body and `.jpm` |
| OBJ import | own tokenizer, index rules, recursive split triangulation, chunking with the fixed triangle | tobj plus ear clipping | tobj reads our OBJ export |
| OBJ/MTL, STL export | own | own (LF, welding as labelled) | tobj; STL triangle count check |
| PNG sprite import | own rectangle extraction | same | - |
| VOX | own flat chunk walk, own writer (version 150) | dot_vox scene graph | dot_vox reads our VOX |
| QB, QEF, binvox, ZOX, CSV | own | own | QB re-import; serde_json for ZOX |
| schematic, NBT | fastnbt values + flate2, exact tag order | Sponge `.schem`, modern structures (M8) | fastnbt reads ours |
| Stonehearth, Staxel | own (QB + NetJson; Staxel stub) | Staxel hidden | - |
| `.lnk` | own subset parser (interop 28) | full shell-link parser | - |

### 8.8 Robustness and determinism

- Every importer has a fuzz target; parsers enforce limits before allocating; errors map to the original's dialog
  titles and lead sentences with the port's own detail text (interop 5.4).
- Outputs are deterministic: ordered containers, fixed archive entry order, the injectable RNG for jitter (with
  "Avoid z-fighting" off in parity tests), and an atlas packer whose order depends only on input order.

---

## 9. Rendering (`cuboid-render`, `cuboid-scene`)

### 9.1 Frame architecture

1. Scene sync drains the journal and uploads changed instance ranges, finished chunk meshes and texture dirty
   rectangles, within a per-frame budget (9.13).
2. For each visible view (four, or one when maximised): render into its own targets: colour `Rgba8Unorm` (4x MSAA
   when "Enable anti-aliasing" is on, otherwise single-sampled), depth `Depth32Float` with reversed Z, resolve
   texture, highlight mask and outline composite.
3. The egui pass draws the UI. Each view quadrant is an `egui::Shape::Callback` whose `paint` blits the resolved
   texture one to one with a nearest-sampling pipeline, so viewport pixels reach the framebuffer unchanged and in
   the right z-order with the UI (view frames and labels are drawn after them).
4. The shell asserts a non-sRGB framebuffer (egui-wgpu's preferred format); the blit shader also handles an sRGB
   surface by linearising, so a misconfigured platform still shows correct colours.

Why offscreen per view: exact compositing (no colour conversion through egui texture sampling), per-view MSAA and
outline post-processing, and the same code path for the PNG render export. Reversed Z mirrors the depth convention
the original's engine generation introduced on its Windows backend (**UNVERIFIED**; it only affects z-fighting of
coplanar surfaces).

### 9.2 Look constants

`cuboid-render::look` gathers every visual constant with its spec reference: clear colour `#343434` (alpha 0 for
renders), light directions and intensities, flat ambient, the per-vertex lighting formula (app 8.3), tint-emulation
colours, highlight colours, grid weights, line colours, faint-box alpha 5/255, axis colours, preview tints. The
pipeline is in gamma space: textures uploaded as `Rgba8Unorm`, no sRGB views, blending on stored 8-bit values.
Blend factors for colour **and** alpha are `SrcAlpha, OneMinusSrcAlpha`, which reproduces the `a^2` alpha of
semi-transparent texels in PNG renders (app 15) instead of hiding it with premultiplied blending.

### 9.3 Elements: exact transparency at batch cost

- Instance data per element (about 224 bytes): 3x4 world transform, per-face rotated atlas UV corners (computed on
  the CPU with the clamp-to-rect rule of tools 4.6), per-face vertex colour (tint emulation rules), active-face
  mask, inverted-normals flag. The vertex shader expands 24 vertices and 36 indices per instance; inactive faces
  collapse to degenerate triangles. Lighting is evaluated per vertex as in app 8.3.
- Per view, visible objects (elements, meshes, preview models) are sorted back to front by the original's key
  (distance to the bounding-box centre; along the view axis for orthographic cameras), stable with list order as
  tie-break (**UNVERIFIED** tie rule).
- The original draws each object as a depth-only pass (alpha at least 0.5) followed by a blended colour pass with
  "less or equal" depth testing. For an object whose texels all have alpha 0 or 1 and whose vertex alpha is 1
  ("binary"), those two passes equal one alpha-tested, depth-writing, blended pass. Therefore a run of consecutive
  binary objects in sorted order can be drawn as one instanced draw without changing any pixel (within one draw,
  GPUs preserve primitive order for depth and blending). Translucent objects break runs and are drawn exactly as
  the original: depth-only pass, then colour pass, per object. Consecutive translucent objects whose screen
  rectangles do not overlap may share a pass pair (no pixel is shared, so order cannot matter).
- Binary classification: per texture, a flag "all texels are 0 or 255 alpha" recomputed on texture change; an
  element is binary when every active face's texture is binary and its vertex alpha is 1. Conservative by design.
- Cost: about `runs + 2 * translucent_objects` draws per view, typically under 20 for 10 000 elements. A diagnostic
  `Compat` switch forces strict per-object drawing to confirm equivalence on any scene.
- Equivalence test: random scenes rendered both ways must match bit for bit on the same adapter.

### 9.4 Meshes

Per-mesh vertex and index buffers (32-bit indices), sub-mesh materials (tint times texture), same lighting and
transparency treatment as elements; primitives share generated geometry buffers.

### 9.5 Voxels

- **Canonical mesher** (`cuboid-voxel::mesh::canonical`): the exact face emission, vertex order, triangle split
  `(q, q+1, q+2), (q, q+2, q+3)`, AO neighbours and factors and colour multiplication of tools 7.6. It is the single
  source for the viewport, wireframe mode, picking semantics and the two exporters that consume chunk meshes
  (OBJ detailed mode, STL).
- **Why not greedy in the viewport by default:** AO is interpolated per vertex across each quad; merging changes
  interpolation unless all four shades are equal; the fixed diagonal is visible in wireframe mode; merged quads
  create T-junctions that can leave single-pixel cracks; exports must not change. Section 12 shows the canonical mesh
  fits the budget: about 4 million vertices for a 256^3 grid with 1.7 million solid voxels, 8 bytes per vertex
  (chunk-local cell coordinates as bytes plus face and AO in one byte, colour times AO as RGBA8).
- **Greedy meshing** exists twice: the export mesher for "Optimized mesh with texture atlas" (algorithm per the voxel
  export specification; **UNVERIFIED** until that part is specified) and an opt-in viewport performance mode for
  grids beyond the parity targets (upscaled resolutions up to 254), labelled non-parity.
- Jobs: dirty chunks are meshed on rayon; results upload in (i', j', k') order; per-chunk frustum culling; voxels
  draw in the alpha-tested queue before transparent objects with the lighting approximation of app 8.5; optional
  soft shadows (two cascades, distance 128) at tier P3 in a late milestone.

### 9.6 Texture atlas

The editor's runtime atlas is emulated, not copied: an etagere allocator, 1 px transparent padding around each
entry (sampling at rect edges matches the original), nearest filtering, clamp addressing, the per-face clamp of UVs
into the entry. Layout is free because it is never persisted (tools 5.2). Paint strokes upload dirty rectangles;
adding a texture allocates incrementally instead of repacking (no visible difference). If the atlas would exceed the
adapter's maximum size, entries spill to additional pages and faces carry a page index.

### 9.7 Overlays

Sub-grid planes as an analytic shader (additive 0.124 per fully covered line pixel, line width `step / 256`, a fade
calibrated to imitate mip-mapping; **UNVERIFIED** pixels), main grid lines and guides as 1 px line lists
(`#FFFFFF20`, model box `#FFC80040`, selection box `#FF6400FF`), background image planes (unlit, nearest, alpha,
culled after a side swap), centre marker, north pointer, blueprint, origin markers, voxel pointer plate, 3D pointer,
hidden-element boxes, selection wireframes for at most four objects (app 6.3 to 8.10).

### 9.8 Highlight outline

Mask pass of highlighted silhouettes (alpha test 0.5, no depth), four diagonal taps at (+-2, +-2) px, per-channel
maximum colour and summed alpha, cut-out where any silhouette covers the pixel, then `mix(scene, rgb, a)` (app 8.7).

### 9.9 Gizmos

Own geometry for move, rotate and scale gizmos and the per-view variants; placement, root scale and the view size
factors of tools 9.1; overlay queue without depth. Hit testing uses explicit hit volumes that mirror the original's
collider shapes (spheres of radius 0.3 centred one unit along each axis arrow, thin boxes for plane handles, rings
for rotation), not the visual meshes, with the 100-unit ray of tools 8.3. Hit volume dimensions are recorded in the
model specification before implementation (**UNVERIFIED** for rings and scale handles).

### 9.10 Picking

- `cuboid-scene::pick`: a BVH over element and mesh world bounds (incremental refit on transform changes, rebuild on
  structural changes), exact ray tests against active-face triangles, front faces only (the original's physics
  queries ignore back faces; **UNVERIFIED**), hidden objects excluded; ray lengths 1000 (hover), 100 (gizmos) and
  200 (drag plane).
- Voxels: DDA over a **frozen collider snapshot** (an `Arc` of the voxel world) that is replaced when a chunk rebuild
  finishes and is not replaced during a stroke, then refreshed at stroke end. This reproduces the original's stale
  colliders during strokes (tools 7.6, 12.2), which decides where Dot, Sphere and Laser strokes land. A hit reports
  the point and face normal of the first exposed face entered, which equals a single-sided mesh-collider hit.

### 9.11 Cameras and navigation

`cuboid-scene::views` implements app 5 as pure math: four views with projections, poses, clip planes, anchors and
pan signs; maximise; side swap with the axis-keeping rule and the Main-label quirk; hover, pending and active view
latching; orthographic pan, Main pan, orbit, zoom limits and the hover gate; Focus; Reset Views; cinema frame;
display-preview anchors and first-entry cameras (app 9). Projection matrices follow the engine's vertical field of
view and orthographic half-height conventions.

### 9.12 Other render features

Wireframe mode (the wgpu line polygon-mode feature where available, barycentric fallback; triangles show their
diagonal as in the original), the display-preview scenes (player rig and skin, first-person crosshair and sky, item
frame, GUI slot, floor bob at 50 Hz), and the PNG render export (512^2 GUI or 1920x1080 Main, 2x supersampling,
the box downscale with its half-pixel offset, far plane 128, transparent background, content rules of app 15),
read back asynchronously and awaited by the export flow.

### 9.13 Dirty tracking and uploads

The journal drives uploads: changed instance ranges are coalesced into `write_buffer` calls; chunk meshes upload
when ready; texture changes upload dirty rectangles; atlas UV changes re-emit only affected instances. Upload budget
32 MiB per frame; leftovers carry over (the original also shows rebuilt chunks over several frames).

---

## 10. UI architecture

### 10.1 Shell (`cuboid-shell`)

- winit 0.30 event loop, egui-winit `State`, egui-wgpu `Renderer`, one wgpu device shared with `cuboid-render`.
- UI scale: `floor(max(96, dpi) / 96 * 2) / 2` clamped to [1, 1.99], with dpi from the winit scale factor times 96,
  plus a user override (ui 1.2). The shell sets egui's zoom so that points per pixel equal that scale.
- Window rules in physical pixels: first launch 1000x700; minimum 900x700 times the scale; position restore with
  the 0 and 30 000 guards (app 1.2). The Left Alt bypass at start-up is best effort (winit reports key state only
  through events) plus a `--reset-window` flag (**UNVERIFIED** equivalence).
- Custom cursor images (resize cursors, own art), drag and drop of files, IME, focus events (texture reload check,
  frame-rate policy), Alt+Enter and F11 full screen, title text.
- Headless mode renders frames into an offscreen texture with scripted input; the same code path as the app, used by
  UI snapshot and scenario tests (section 15).

### 10.2 Layout engine (`cuboid-ui-kit::layout`)

- A node arena authored in the original's terms: `RectTransform { anchor_min, anchor_max, anchored_position,
  size_delta, pivot, local_scale }` in y-up units, active flag, `LayoutGroup` (horizontal, vertical, grid with
  padding, spacing, child alignment, control-size and force-expand flags, start corner and axis, constraint),
  `LayoutElement` (ignore, min, preferred, flexible), `ContentSizeFitter`, `AspectRatioFitter`, mask (clip),
  raycast-target flag, graphic, widget behaviour.
- Algorithm: the uGUI order (widths bottom-up then top-down, then heights bottom-up then top-down), the stack and
  grid rules of ui 0.4 (normative there), default contributions of images and texts, the nine-slice squash rule;
  local scale applied at render and hit-test time around the pivot (the quadrant frames at half scale with labels at
  double scale depend on it). Float geometry throughout; edges snapped only when drawing (round half away from
  zero, ui 0.2). World rects convert to egui's y-down points at the end.
- Relayout only when something marks the tree dirty (window size, text change, activation, zone width).
- Node trees are written by hand in Rust builders from the specification numbers. A local conformance test loads
  the private hierarchy dump (O2), maps nodes by our own stable ids through a hand-maintained correspondence table,
  and compares every world rect. No code is generated from the dump (section 17).
- Scope (from the component census of the original window): image (simple, sliced, tiled), raw image, text, layout
  element, horizontal/vertical/grid groups, content size fitter, aspect ratio fitter, mask, scroll rect, scrollbar,
  slider, toggle and toggle group, input field, button, tab, text link, side grip, tooltip, texture view, input
  navigator.

### 10.3 Event system (`cuboid-ui-kit::events`)

- Raycast: the topmost graphic with its raycast-target flag set, in reverse draw order, honouring masks. The engine
  default is "on" for images and texts; the dump does not record the flag (**UNVERIFIED** per node, verified by
  behaviour tests).
- Pointer events (enter, exit, down, up, click, begin-drag after 5 px, drag, end-drag, scroll) bubble to the nearest
  handler ancestor.
- Selection: one "current selected" control. Text fields, dialog buttons and some checkboxes take selection on click
  and keep their hover colour until another control takes it; clicking something that is not selectable clears it
  (ui 16.1). Shortcut gating reads this state (7.4).
- Navigation events while a control is selected: arrows move between selectables (automatic navigation), Submit
  (Return) activates, Cancel (Escape) is routed, with a 0.5 s repeat delay and 10 actions per second (values from the
  original window's event-system settings; to be added to the UI specification). Tab and Shift+Tab follow ui 16.6.
- `pointer_over_ui()` gates view hover, wheel zoom and picking (app 5.4).

### 10.4 Widgets

Push button, toggle and toggle group (all-off allowed), tab, checkbox, radio, slider with linked field, dropdown and
field-with-list, virtualised scroll list (uniform rows, 28 px wheel step, always-visible scrollbar), scrollbar, text
link (view labels, separate left and right click), grips, classic tooltip (immediate, size formula, placement and
even-pixel truncation of ui 10.1), palette picker, colour field, texture view (zoom, pan, UV rectangle drag),
progress bar. Every interactive widget uses the colour state model of ui 0.5: texel times base times state times
multiplier, a 0.1 s linear cross-fade from the current colour (or instant), priority disabled > pressed > hover >
normal, colours resolved from the active skin at draw time.

### 10.5 Text field

Own widget on egui's text layout and input events, because the original's input field differs from egui's
`TextEdit` in focus and selection rules: select-all when a click focuses the field, word selection with the
separators of ui 16.6, a 1 px caret blinking with period 1/0.85 s, `fieldFocus` selection at 50 % alpha, horizontal
scrolling, content types (standard, integer, decimal, alphanumeric, name, e-mail), length limits, live change
notifications on every edit, mouse-wheel +-1 on numeric fields, IME through egui events, clipboard through egui-winit.
Multi-line mode for the map table and similar fields.

### 10.6 Text rendering and metrics

- Open Sans Regular, Bold and Italic are shipped (OFL); egui's default fonts are disabled.
- Layout uses a `TextMetrics` model, not glyph rasterisation: advances at 11 px, line height 15, word wrapping that
  drops lines which do not fit (ui 14), single-line widths for tooltips and caption chips. Integer versus fractional
  advances and kerning are calibration options (**UNVERIFIED** which reproduces the original; decided by the
  conformance test, because content-sized nodes encode text widths).
- Rasterisation uses egui (skrifa outlines, vello_cpu coverage) with hinting and sub-pixel binning chosen by a
  calibration test against O1 captures; glyph pixels are tier P3. Text placement follows the layout model, so a
  rasteriser mismatch never moves other elements.

### 10.7 Skins, sprites and skin packs

- `Skin { name, slots: [SpriteId; 8], colors }` with the exact Light, Dark and Rainbow tables of ui 12.2; a switch is
  complete and immediate (ui 12.5 port decision). The "unskinned" text rule is a per-node flag.
- Sprites are keyed by the original picture names that the specifications use. The built-in set in `cuboid-assets`
  is own look-alike art with the same sizes, borders and roles (ui 13, 15); it is packed into one egui texture at
  start-up (bilinear for UI chrome, nearest for palettes and texture previews), with greyscale copies for disabled
  icons.
- Skin packs: a folder of PNG files named by picture name, which a user may extract from their own installation;
  sizes are validated against the frame inventory; nothing is bundled or redistributed (ui 15.1). Pixel parity (P2)
  of the chrome is measured with such a pack loaded; with built-in art, parity means identical geometry, colours and
  colour logic.

### 10.8 Window composition (`cuboid-ui`)

Regions and draw order of ui 2 (app area, status bar, menu bar, dialogs, popups, file browser, overlay); side zones
with the managers and helpers groups, grips, docking commands and panel sets per edit mode; the left and right tool
strips; the ten manager panels and two helpers; the 4-view zone with quadrant nodes at half scale and the view
labels; the status bar with its seven cells and the Info timing; overlay (tooltip, tool cursor). The 3D quadrants
take their physical viewport rects from the layout, as in the original.

### 10.9 Menus

The mechanics of menus 1: engaged menu bar, open and close deferred by one frame except the engaged hover switch,
child placement and flipping with the original's mix of canvas scale and raw dpi scale and its row-height estimate,
outside-press closing for popups and dropdowns, row looks (instant hover, disabled grey with greyscale icons),
dynamic rows and the recent-files rebuild quirk.

### 10.10 Dialogs and message boxes

- Framework of menus 7 and ui 11: shared chrome, drag strip with 16 px overhang, resize grips with growth limits,
  re-centring with the original's integer formula on every show, default focus, Enter and Escape rules on key
  release, a single global modal flag (last writer wins) and the dimming layer animation, and the fixed stacking
  order.
- Message box: kinds with icons and sounds, the automatic size estimate, one to three buttons with the
  handler-to-button mapping (the close button acts like the last button with a handler), and replacement semantics:
  a box shown while another is open replaces it, and the replaced box's waiter resolves as `Replaced`.
- Each dialog is a module with its node tree, a view model filled on show and apply logic on OK, so behaviour is
  testable without rendering.

### 10.11 Built-in file browser

Its own work package: window and resize handles, listing rules (folders first, filters, hidden entries at 55 %
opacity), row colours, selection and activation timing, preview pane with cached image decoding, extension list,
prompts in the dark style, locations (Recent at most 5, favourites, default favourites), path aliases, keyboard
rules, drives per OS, "open in OS" through opener, and import of the original's stored locations format (menus 10).

### 10.12 Flow executor

Multi-step operations are `async fn`s polled on the UI thread by a small executor (no external runtime):

```rust
impl FlowCx {
    pub async fn next_frame(&self);
    pub async fn fixed_step(&self);
    pub async fn seconds(&self, s: f32);
    pub async fn message_box(&self, spec: MessageBoxSpec) -> BoxAnswer;  // Button(i) | Close | Replaced
    pub async fn text_prompt(&self, spec: PromptSpec) -> Option<String>;
    pub async fn file_dialog(&self, req: FileRequest) -> Option<PathBuf>; // built-in or native
    pub async fn job<T: Send + 'static>(&self, f: impl FnOnce(&JobCx) -> T + Send + 'static) -> T;
    pub fn with<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> R;     // never held across an await
}
```

This mirrors the original's coroutines one to one (wait one frame, wait 0.1 s, show a box and continue in the
handler), which keeps flows short and reviewable against the specs. State is borrowed only inside `with`; the clippy
lint `await_holding_refcell_ref` is denied. Flows: new file, open, merge, import, export, save, save as, backup,
unsaved-changes prompt with continuation, exit, recent files, drag and drop, command line, voxelization, texture
reload on focus, PNG render, Sketchfab (M8).

### 10.13 Strings and branding

All user-visible texts live in `cuboid-ui::strings` with their spec reference, typos included ("occured",
"Successfuly", "dowscale", ...). `{App}` and brand substitutions are applied there and nowhere else. A test compares
the catalogue with `parity/strings.toml` (T-UI-1). No localisation in version 1; the catalogue is the hook for it.

---

## 11. Threading and jobs

### 11.1 Thread map

| Thread | Work | Rules |
|---|---|---|
| UI thread | winit loop, egui, `Core` mutations, flow executor, scene sync, wgpu encoding and submission | the only writer of `Document`; never waits on a job |
| rayon pool (cores - 1) | chunk meshing, voxelisation, conversions, face optimisation, atlas creation, image decoding and PNG encoding, BVH builds, sorting for large scenes | pure functions on snapshots; results through channels |
| I/O worker(s) | file reads and writes, directory scans, texture signature hashing, backups | atomic writes; cancellation tokens |
| network (opt-in, M8) | uploads and lookups | never started without user action |

### 11.2 Mechanics

- `JobHandle<T>` wraps a one-shot channel; flows await it, panels poll it. Cancellation is an `Arc<AtomicBool>`
  checked at loop boundaries; superseded results are dropped by revision check.
- Panic isolation: each job runs under `catch_unwind`; a panic becomes an "exception" message box and a log entry,
  and the document stays untouched (results were never applied).
- No locks are held across frames; the only shared structures are immutable `Arc` snapshots and channels, so the
  design has no lock ordering and no deadlock surface.

### 11.3 Determinism under parallelism

Parallel algorithms that write voxels or create objects collect results per work unit and apply them on one thread
in the original scan order, so palette order, element order and display ids are identical to a sequential run.
Voxelisation parallelises per (i, j) column and replays columns in order; voxel-to-element conversions parallelise
the analysis and create elements in scan order.

---

## 12. Performance and memory budgets

Reference machine R1: 6-core desktop CPU (2019 class), 16 GB RAM, discrete GPU of the GTX 1060 class, 1920x1080.
R2: 4-core laptop with integrated graphics; targets at half the R1 frame rate.

| Scenario (R1) | Budget |
|---|---|
| Idle, focused, nothing animating | 0 redraws per second, under 1 % CPU |
| Orbit the Main view, 4-up, 10 000 elements (5 % translucent), anti-aliasing off | at least 60 fps; CPU at most 5 ms, GPU at most 8 ms per frame |
| Drag one element among 10 000 | one-frame latency; at most 0.5 ms CPU for the update |
| Drag a selection of 10 000 elements | at most 16 ms per drag frame (3 MB instance rewrite) |
| 256^3 grid, 1.7 M solid voxels, about 1 M exposed faces | full meshing at most 1.5 s on 8 threads; 4 M vertices, about 56 MB of GPU buffers; at least 60 fps steady |
| Single voxel edit | re-mesh and upload at most 2 ms (one to seven chunks) |
| Sphere brush, one stroke frame | at most 4 ms CPU |
| Open a 50 MB `.cubik` payload | interactive within 1.5 s (BSON decode at least 300 MB/s, voxel apply at least 50 M cells/s) |
| Save the same document | UI blocked at most 30 ms (snapshot); total at most 2 s off-thread |
| Undo push | typically under 1 ms (pointer copies), worst case 20 ms |
| Undo restore of 10 000 elements | at most 100 ms |
| Face optimisation over 10 000 elements | at most 1 s with the BVH (the reference performs 10^8 pair tests) |
| Voxelisation at resolution 128 of 100 000 triangles | at most 5 s |
| Memory: 10 000 elements, 1.7 M voxels, 16 MB of textures, 11 undo entries | at most 1 GB resident |

Unit costs: element about 0.5 KB on the CPU and 224 bytes of instance data; voxel chunk 16 KB; chunk mesh vertex 8
bytes; image RGBA8 on the CPU plus its atlas copy on the GPU. Benchmarks (criterion) and a frame-time overlay in debug
builds guard these numbers; CI fails on regressions above 20 % on the benchmark machine.

---

## 13. Settings, persistence and paths

- `cuboid-settings::Settings` contains every key of app 10.2 (same names and value formats, so imported values round
  trip) plus port keys: managers and helpers widths and docking side (persisted on release, ui 2.7), window
  rectangle, UI scale override, skin pack path, profile memory per format, custom key strings, browser locations,
  recent files (identified by full path, displayed by name), `Compat` overrides.
- Storage: `<config>/settings.json` (pretty JSON, `"version": 1`, unknown keys preserved, written atomically on
  Preferences OK and on quit, app 10.1). Data folder: `backup/<Weekday>_backup.cubik` (English local weekday names),
  `presets/*.json`, `skin.png`, `palette.png`, `logs/`. Cache folder: export temporaries (resource-pack staging,
  Sketchfab packaging), cleaned at start-up. Paths come from `directories`; a `cuboid.portable` marker next to the
  executable switches all folders to the executable's folder.
- First-run migration: read the original's `cubikdata.json` from its per-user folder, its stored preference values
  (browser locations string, favourites marker, Minecraft path override) from the registry on Windows, and offer to
  copy its presets folder. Never write to the original's locations.
- Atomic writes (temporary file in the same folder, then persist) for documents, backups, settings and presets
  (deviation D8).
- File association: only through the installer or an explicit menu action, per user (deviation D4).

---

## 14. Errors and logging

- Library errors are typed (`IoError`, `CodecError { kind, path_in_document, detail }`, `RenderError`, ...).
  User-facing errors map to the original's titles and lead sentences through `UserMessage` values from the strings
  catalogue; details are the port's own text.
- Never panic on input: parsers are fuzzed; arithmetic on sizes is checked; limits are enforced before allocation.
- `log` facade with an own logger in `cuboid-app`: a ring buffer (last 2000 lines) for a debug console, a rotating
  file in `<data>/logs/` (3 files of 5 MB), level Info by default and Debug with `--debug` or `CUBOID_LOG=debug`
  (replaces the original's marker-file debug mode, app 1.4). Log lines the specs quote ("Elapsed time to load: ...",
  "Sorted N transparent elements", ...) are emitted with the same text for parity diagnostics.
- A panic hook writes a crash report next to the logs; there is no crash-recovery prompt (as in the original); the
  backup folder stays the recovery path.

---

## 15. Testing strategy

| Layer | What | Where | Gate |
|---|---|---|---|
| Unit | numerics, BSON primitives, XOR, dialect parser, setters, every op | each crate | CI |
| Spec vectors | every byte example and table in the specs (native 3.5, 9.3; interop tables) | `tests/spec_vectors.rs` per crate | CI |
| Golden files | own fixtures: original file, decompressed payload, canonical text dump (key path, type, value, doubles as hex bits; insta snapshots), expected summary | `tests/golden/` | CI |
| Round trip | payload identity (read, write, compare bytes), semantic round trip (drop retention, write, compare except listed normalisations), proptest-generated documents | fmt-cubik | CI |
| Differential | optimised versus reference twins (face optimisation, voxelisation, voxel restore, batched rendering, atlas packing invariants); our outputs read by O3 oracles; random BSON through both our reader and the `bson` crate | per crate | CI |
| Fuzzing | every importer, the container, BSON, the dialect parser, `.lnk`; coverage-guided on nightly; on stable, every corpus file replays as a normal test | `fuzz/`, `tests/corpus_replay.rs` | nightly job (1 h per target), CI replay |
| Scenarios | scripted command and input sequences run headless against `Core`, producing canonical state dumps; cover the parity checklists of tools 17, app 20 and the menu and dialog flows | `cuboid-testkit` DSL, `tests/scenarios/*.ron` | CI |
| Input replay | recorded raw input traces (keys, mouse, timing) replayed through the shell's headless mode | `tests/replay/` | CI |
| UI snapshots | headless app frames at 1000x700 and 1280x720, scales 1.0 and 1.5, Light and Dark, each panel set and dialog, compared with approved images (built-in art) using tier masks; widget-level tests with egui_kittest | `cuboid-ui/tests` | CI (software adapters: WARP on Windows, lavapipe on Linux) |
| Layout conformance | every node's world rect against the private dump (O2); colour-logic and skin-pack pixel parity against private sprites | local, maintainers | release checklist |
| Reference captures | O1 screenshots, exports and files compared by tier; text calibration | local, maintainers | ledger `verified-o1` |
| Cross-application | native 10.3 and interop 30 procedures with Cubik installed | manual, scripted where possible | release checklist |
| Performance | criterion benchmarks of section 12; frame-time captures of standard scenes | `benches/` | weekly job, release |
| Static | rustfmt, clippy `-D warnings`, cargo-deny (licences, advisories, duplicates), `ledger check`, strings and compat table checks | CI | every change |

CI matrix: Windows (primary), Linux and macOS for build, unit, golden, scenario and replay tests; GPU snapshot tests on
Windows and Linux software adapters with per-backend approved images where rasterisation differs. Tests that need
O1 or O2 data are skipped with a visible message when the data is absent.

---

## 16. Packaging and distribution

- cargo-packager configurations: Windows installer (per-user install, optional `.cubik` association, file icon),
  macOS application bundle and disk image, Linux AppImage and `.deb` with a desktop entry and a MIME type for
  `.cubik`. A portable ZIP for Windows uses the portable marker (13).
- Release builds use `--locked` and the pinned toolchain; artifacts carry a third-party licence notice generated
  from cargo-deny's licence listing, the Open Sans licence and the asset licence.
- Assets (icons, frames, cursors, sounds, sky and frame textures, default skin, palettes) are embedded with
  `include_bytes!`; no network access at start-up; no telemetry, licence or update requests (deviations D1, D5).
- Code signing and notarisation are pipeline steps with secrets outside the repository; unsigned builds remain
  reproducible.

---

## 17. Licensing and clean-room rules

- Code licence: **MIT OR Apache-2.0**. It matches the dependency set (all permissive; the MPL-2.0 helper pulled by
  `directories` is file-level copyleft and allowed), is the Rust ecosystem default and maximises contributions.
  GPL would add obligations without protecting anything parity needs.
- Fonts: Open Sans under the SIL Open Font Licence 1.1, shipped as separate files. Own art and sounds: CC0 (or the
  code licence). Third-party notices bundled.
- Blockbench (GPL-3.0-or-later): learning reference only. No code, shaders, icons, themes or text are copied or
  translated; its study notes informed architecture choices (panel layout as data, transaction-scoped undo, typed
  conditions, dirty-flag batching, export from immutable snapshots).
- Cubik Studio: proprietary. The port ships none of its assets, scene data, strings beyond functional UI texts
  required for parity (with brand substitution), or logos. Its pictures are used only from a user's own
  installation as an optional local skin pack.
- Process: specification writers (who may read the research tree) and implementers (who read `docs/spec` and
  `docs/design` only) are separate roles; trace files that map specs to research sources stay private; no code is
  generated from research data; private oracles run only on maintainers' machines; questions go back to the spec
  writers, and answers land in `docs/spec`.
- Product naming avoids the original's brand; the trademark-free name `cuboid` and the `{App}` substitution apply to
  every output that embeds an application identity.

---

## 18. Implementation plan

### 18.1 Rules for parallel agents

1. **Contracts first.** WP-0.2 creates every crate with its public types and trait signatures. Afterwards a crate's
   public API changes only through its owning work package; consumers adapt in their own packages. Contracts freeze
   at milestone start.
2. **Disjoint files.** Each work package owns the listed paths exclusively. The workspace `Cargo.toml`, CI files and
   `parity/compat.toml` belong to the integration package of the current milestone.
3. **Definition of done.** Code, tests, ledger entries moved to `tested`, clippy clean, no new `UNVERIFIED` without a
   ledger entry and a note in section 20 of the owning spec or this document.
4. **Mocks for parallelism.** Renderer and UI tracks work against synthetic scenes and a fake `Core` until M6.
5. **Spec questions** are filed against the spec document, not solved locally by guessing.

Tracks after M0: A formats (M1, M3), B core (M1 model, M2), C rendering (M4), D UI kit and window (M5); they join in
M6. Critical path: M0, model (WP-1.7, WP-1.8), core (M2), flows (WP-6.7).

### 18.2 Milestones and work packages

**M0 Foundations (5 packages, parallel)**

| WP | Owns | Deliverable | Acceptance |
|---|---|---|---|
| 0.1 | workspace root, CI, `deny.toml`, toolchain, lint config | workspace skeleton, three-OS CI | build, test, clippy and deny green on Windows, Linux, macOS |
| 0.2 | `crates/*/src/lib.rs` public API stubs | contracts for records, model, history, io, scene, render, ui-kit, core | all crates compile; API review signed off |
| 0.3 | `crates/cuboid-math/**` | section 4 complete | spec vectors pass; NJ exhaustive round trip; proptests |
| 0.4 | `crates/cuboid-shell/**` | spike: window rules, scale, engine keys, offscreen MSAA render, exact blit, headless capture | blit of a test pattern reads back bit-exact; keypad and side keys distinguished in synthetic event tests; first frame 1000x700 at scale 1 |
| 0.5 | `crates/cuboid-testkit/**`, `crates/cuboid-cli/src/ledger.rs`, `parity/ledger.toml` | golden harness, image diff tiers, canonical dump format, oracle gating, ledger checker | ledger check runs in CI; diff tiers unit-tested |

**M1 Native files and data core (8 packages)**

| WP | Owns | Deliverable | Acceptance |
|---|---|---|---|
| 1.1 | `cuboid-records/**` | schema records with defaults | defaults equal native 4 tables (test per table) |
| 1.2 | `cuboid-fmt-cubik/src/bson/**`, `fuzz/fuzz_targets/bson.rs` | reader, writer, retention | native 3.5 vectors; `bson` oracle agreement on 10^5 random documents; fuzz 1 h clean |
| 1.3 | `cuboid-fmt-cubik/src/{container,schema}/**` | ZIP container, records to BSON per section, extension entry | native 10.1 container cases; payload identity on generated documents |
| 1.4 | `cuboid-io/src/jsonlite/**`, `cuboid-fmt-cubik/src/cub/**` | permissive dialect parser, `.cub` reader | native 9.3 vector; interop 8.6 accept/reject table |
| 1.5 | `cuboid-voxel/src/{world,palette,writers,mapping}.rs` | voxel storage core | index/world mappings and box half-openness tests; writer matrix of tools 7.5 |
| 1.6 | `cuboid-image/**` | images, stored formats, BC1/BC3, file codecs, signature | every format code round trip; bottom-up convention tests |
| 1.7 | `cuboid-model/src/{types,setters,journal,selection}/**` | object types and setters | tools 17 items 2, 3, 6, 7 as unit tests |
| 1.8 | `cuboid-model/src/apply/**`, `cuboid-cli/src/{dump,roundtrip}.rs` | load and save semantics, CLI | semantic round trip with listed normalisations; 50 MB load within budget |

**M2 Headless editing core (10 packages)**

| WP | Owns | Deliverable | Acceptance |
|---|---|---|---|
| 2.1 | `cuboid-history/**` | history engine | tools 15.1 semantics including redo cache and 11-entry cap |
| 2.2 | `cuboid-ops/src/elements/**` | create, clone, delete, rename, colour, visibility, order, face menus, face clipboard | label catalogue and guard tests |
| 2.3 | `cuboid-ops/src/transform/**` | quarter turns, restriction bake, model rotation, alignment, transform dialogs, rescale, randomize | tools 17 items 4, 5, 9 |
| 2.4 | `cuboid-ops/src/uv/**` | auto-UV, UV tools, projections | tools 17 items 6, 8 |
| 2.5 | `cuboid-ops/src/textures/**` | texture list operations, atlas creation, face crop, transparent sort, face optimisation and culling (reference plus BVH) | tools 17 item 17; differential test |
| 2.6 | `cuboid-ops/src/voxel/**`, `cuboid-voxel/src/{ops,search}.rs` | voxel tools, blueprint commands, flips, rotations, scaling, hollow and fill, setup presets | tools 17 items 14, 15 |
| 2.7 | `cuboid-ops/src/convert/**` | voxels to elements (three modes), voxelisation | tools 17 item 16; parallel equals sequential |
| 2.8 | `cuboid-ops/src/{mesh,primitives,paint}/**` | mesh tools, primitive generators, paint on textures and voxels | tools 17 item 18; primitive geometry flagged until O1 calibration |
| 2.9 | `cuboid-core/src/{core,commands,clipboard,compat,start}.rs` | command executor, clipboards, start queue, `Compat` | tools 17 items 1, 12, 13 |
| 2.10 | `cuboid-core/src/interact/**` | interaction state machine, custom tools, gizmo logic over a mock raycaster | tools 17 items 9, 10, 11 as scenarios |

**M3 Interop formats, `cubik29` profile (10 packages)**

| WP | Owns | Deliverable | Acceptance |
|---|---|---|---|
| 3.1 | `cuboid-io/src/{registry,pipeline,emit,text,session}/**` | registry, pipeline helpers, emitters | interop 4 lookup and filter tables; emitter goldens |
| 3.2 | `cuboid-fmt-minecraft/src/json_export/**` | Minecraft JSON export | T-JSON-1 on own fixtures, T-NUM-1 |
| 3.3 | `cuboid-fmt-minecraft/src/{json_import,presets}/**` | Minecraft JSON import, presets | T-JSON-2 |
| 3.4 | `cuboid-fmt-minecraft/src/resource_pack/**` | resource pack | T-RP-1 (icon through a mock `RenderService`) |
| 3.5 | `cuboid-fmt-minecraft/src/{mcpe,bedrock_js,java,jpm,entity_atlas}/**` | entity formats | T-MCPE-1, T-JS-1, T-JAVA-1, T-JPM-1 |
| 3.6 | `cuboid-fmt-mesh/src/{obj_import,obj_export,stl}/**` | OBJ, MTL, STL | T-OBJ-1..3, T-STL-1; tobj reads exports |
| 3.7 | `cuboid-fmt-mesh/src/{png_sprite,png_render}/**` | sprite import, render export driver | T-PNG-1 |
| 3.8 | `cuboid-fmt-voxel/src/{vox,qb,qef,binvox}/**` | voxel formats A | T-VOX-1/2, T-QB-1, T-QEF-1, T-BINVOX-1 |
| 3.9 | `cuboid-fmt-voxel/src/{schematic,nbt,zox,csv,stonehearth,staxel,tables}/**` | voxel formats B and block tables | T-SCHEM-1, T-NBT-1, T-ZOX-1, T-CSV-1, T-SH-1 |
| 3.10 | `cuboid-io/src/lnk/**` | shortcut target parser | T-LNK-1 |

**M4 Renderer (8 packages)**

| WP | Owns | Deliverable | Acceptance |
|---|---|---|---|
| 4.1 | `cuboid-render/src/{device,targets,blit,look,pipelines}/**` | render core | white-cube face values 245/219/184 on up/south/west (app 8.3) within 1/255 |
| 4.2 | `cuboid-render/src/{elements,meshes,sort}/**`, `cuboid-scene/src/drawlist.rs` | instance upload, sorted runs | batched equals strict per-object rendering on 1000 random scenes |
| 4.3 | `cuboid-voxel/src/mesh/**`, `cuboid-render/src/voxels/**` | canonical mesher, chunk jobs, voxel pipeline | mesher golden layouts; budget of section 12 |
| 4.4 | `cuboid-render/src/{grid,overlays,outline,wireframe}/**` | overlays, outline | own render goldens per overlay |
| 4.5 | `cuboid-scene/src/{views,camera,navigation,preview}/**` | views and navigation | app 20 "Views and cameras" items as tests |
| 4.6 | `cuboid-scene/src/{pick,bvh,gizmo}/**` | picking, gizmo math | single-sided, hidden and frozen-snapshot tests |
| 4.7 | `cuboid-render/src/{offscreen,readback,preview_scenes}/**`, preview art in `assets/preview/` | PNG render, preview scenes | app 15 content and sizing rules |
| 4.8 | `cuboid-render/src/shadows/**`, `benches/render.rs` | voxel shadows, renderer performance | section 12 rendering budgets |

**M5 UI kit and main window (8 packages)**

| WP | Owns | Deliverable | Acceptance |
|---|---|---|---|
| 5.1 | `cuboid-ui-kit/src/layout/**` | layout engine, conformance harness | ui 0.4 worked examples; conformance runner on O2 locally |
| 5.2 | `cuboid-ui-kit/src/events/**` | event system, selection, navigation | event routing and selection tests (kittest) |
| 5.3 | `cuboid-ui-kit/src/{skin,sprites,paint,tween}/**`, `assets/ui/frames/**` | skins, sprites, nine-slice painter, colour tweens, frame art | ui 12.6 effective colours reproduced from frames |
| 5.4 | `cuboid-ui-kit/src/widgets/**` | widget set | ui 16 behaviours |
| 5.5 | `cuboid-ui-kit/src/text/**` | text metrics, wrapping, text field | ui 14 and 16.6 behaviours |
| 5.6 | `cuboid-ui/src/{menus,actions,shortcuts}/**`, `cuboid-shell/src/keys.rs` | menu system, action registry, dispatcher, input map | menus 1 and 6 scenario tests; strings table |
| 5.7 | `cuboid-ui/src/{window,status,strips,zones,panels}/**` | main window | ui 17 checklist automated |
| 5.8 | `assets/ui/icons/**` | full own icon set (design work) | inventory of ui 15.2 complete with sizes |

**M6 Dialogs, helpers and application flows (8 packages)**

| WP | Owns | Deliverable | Acceptance |
|---|---|---|---|
| 6.1 | `cuboid-ui/src/dialogs/{framework,message_box,prompts,progress,color_picker}/**` | dialog framework and shared dialogs | menus 7 and 8 rules; message box sizes |
| 6.2 | `cuboid-ui/src/dialogs/preferences/**`, `cuboid-settings/**` | Preferences, settings, migration | app 10 table round trip; migration from a sample settings file |
| 6.3 | `cuboid-ui/src/dialogs/{model_properties,grid,voxel_setup,voxelization,png_import,faces_textures}/**` | model and tool dialogs | menus 9.2 to 9.7 behaviours |
| 6.4 | `cuboid-ui/src/helpers/element_settings/**` | Element Settings (three tabs, UV view) | ui 9.1 and tools 10.15 to 10.17 |
| 6.5 | `cuboid-ui/src/helpers/display_settings/**` | Display Settings, presets, mode switching | app 9 checklist |
| 6.6 | `cuboid-ui/src/file_browser/**` | built-in file browser | menus 10 behaviours |
| 6.7 | `cuboid-app/src/{app,flows,jobs,backup,lifecycle}/**` | flows and lifecycle | app 20 start-up, shutdown, files and data items as scenarios |
| 6.8 | `cuboid-app/src/{audio,os,folders,logging}/**` | sounds, OS integration, folder detection, logger | app 17 detection rules; logger rotation |

**M7 Hardening and release (5 packages)**: 7.1 performance and memory campaign; 7.2 fuzzing campaign (24 CPU-hours
per target), crash-safety tests (interrupted saves leave the previous file intact); 7.3 packaging, installers,
association action, portable mode; 7.4 O1 parity campaign (ledger entries to `verified-o1`, text calibration, layout
conformance at 0 mismatches); 7.5 user and contributor documentation, parity report generated from the ledger.

**M8 Modern profiles (after 1.0)**: modern Minecraft JSON targets and item definitions, modern resource packs,
Bedrock geometry, OptiFine JEM, Java entity templates, Sponge schematics and modern structures, glTF export,
Sketchfab upload API v3, opt-in skin lookup and update check (interop 31, app 18).

### 18.3 Milestone acceptance

| Milestone | Exit criteria |
|---|---|
| M0 | CI green on three OSes; shell spike passes exact-blit and key-identity tests; numerics vectors pass |
| M1 | native 10.1 tests pass; payload identity for generated documents; fuzzers clean for 1 h each; load budget met |
| M2 | tools 17 items 1 to 18 pass as tests or scenarios; differential tests green; undo catalogue matches tools 15.6 |
| M3 | every T-* test of interop 30 passes on own fixtures; O3 oracles accept all outputs; byte-exact against O1 fixtures where available |
| M4 | own render goldens stable on two software adapters; face brightness table matches; section 12 rendering budgets met on R1 |
| M5 | first-launch window passes ui 17 items 1 to 7 automatically; layout conformance runner has no mismatches on the main window |
| M6 | app 20 checklist complete except ledger items marked `unverified-spec`; strings test green; all dialogs reachable through scenarios |
| M7 | ledger fully `tested`; O1 campaign run; installers built and smoke-tested on three OSes |

---

## 19. Risks and mitigations

| # | Risk | Impact | Mitigation |
|---|---|---|---|
| R1 | the original runtime's float evaluation width and number formatting differ from the spec's f32 assumption | P0 text exports and snapping ties | numerics isolated in one crate with a switchable evaluation policy; O1 vectors (T-NUM-1, snapping ties) |
| R2 | text metrics or rasterisation do not match | layout shifts in content-sized nodes; glyph pixels | layout uses a calibrated metrics model; glyphs judged at P3; calibration in M7 |
| R3 | primitive mesh geometry is unspecified | rendering, voxelisation and OBJ/STL output of primitives | capture via O1 OBJ exports; parametric generators; ledger flags |
| R4 | the original's packers are not reproducible | persisted atlases differ | spec defines parity by sampled pixels; deterministic packer; sampling tests |
| R5 | reference samples from the original arrive late | unverifiable details | synthetic fixtures first; ledger status `unverified-spec`; spec 11 sample list drives the request |
| R6 | GPU and driver rasterisation differences | flaky image tests | tier masks; software adapters in CI; per-backend approved images |
| R7 | transparency tie-breaking or batching assumptions are wrong | rare order artefacts | strict per-object switch for diagnosis; equivalence tests; O1 captures of intersecting translucent elements |
| R8 | egui API changes between minor versions | UI rework | all egui use behind `cuboid-ui-kit` adapters (painter, text, input); pin 0.37 for 1.0 |
| R9 | UI scope (about 1300 images and 900 texts in the original window) | schedule | uGUI-compatible engine so transcription is mechanical; art pipeline in parallel; conformance runner catches errors |
| R10 | skin-pack extraction from an installation is technically or legally delicate | pixel parity measurement | v1 accepts a user-prepared PNG folder; an extractor only after review |
| R11 | the Left Alt start-up bypass needs key state before the first event | rescue path for off-screen windows | best-effort detection plus `--reset-window`; documented deviation |
| R12 | provenance of the Minecraft legacy palette values | legal | open decision (interop 33.3); regenerate from public data if reuse is rejected |
| R13 | a frozen-collider or start-queue subtlety is missed | tool results differ from the original | scenario tests per tool with O1 traces in M7 |

---

## 20. Assumptions marked UNVERIFIED and open decisions

Design assumptions (in addition to the open questions listed at the end of each specification):

1. Wider intermediate float evaluation in the original runtime (4.1); the evaluation policy stays switchable.
2. Number formatting digit generation of the original runtime (4.3).
3. Fixed-step catch-up cap of one third of a second (7.6).
4. Reversed-Z depth in the original's Windows backend (9.1); only coplanar z-fighting depends on it.
5. Stable tie-break by list order in transparency sorting (9.3).
6. Front-face-only physics queries in the original (9.10).
7. Raycast-target flags per UI node (not in the hierarchy dump); engine default assumed (10.3).
8. Event-system navigation values (5 px drag threshold, 0.5 s repeat delay, 10 actions per second) come from the
   original window's settings and must be added to the UI specification (10.3).
9. Integer versus fractional text advances and kerning (10.6).
10. Atlas slot limit 8192 as the document-level downscale threshold (5.4).
11. ANSI code page decoding of texture signatures for bytes the original runtime decoded differently (5.4).
12. Gizmo hit-volume shapes for rotation rings and scale handles (9.9).
13. Primitive mesh geometry (5.5).
14. Export greedy mesher for "Optimized mesh with texture atlas" (9.5) awaits its specification.
15. Whether egui_kittest's renderer can host the viewport callbacks; the headless shell is the fallback and the
    primary path for full-window snapshots (15).
16. Left Alt detection at start-up through window events only (10.1).

Open decisions for the owner:

- Approve MIT OR Apache-2.0 for code and CC0 for own assets.
- Approve the own shell instead of eframe (3.5, 10.1).
- Approve the texture write policy (RGBA32 for edited textures; strict switch for atlas-recut ARGB32, 8.4).
- Approve the recommended deviations D6, D7, D8 as `Compat::port()` defaults (app 0.2).
- Decide the source of the Minecraft legacy palette and the VOX default palette data (interop 33.3).
- Decide whether the viewport performance mode (greedy voxels) ships in 1.0 or later.
