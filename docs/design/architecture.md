# Cuboid architecture

> 2026-10-24: `docs/design/roadmap_lean.md` replaces the parity-first scope of this document (UI parity, strict
> modes, the work-package tables, parity tiers P1-P4). Where the two conflict, the lean roadmap wins; the crate
> layout and layering rules here still apply.

Status: final architecture baseline, 2026-10-18. It replaces the two proposals in this folder
(`arch_parity-first.md`, `arch_performance-first.md`), which stay as background material only.

Inputs: the six functional specifications in `docs/spec/` and the two proposals. Section references such as
"native 4.4.3", "tools 3.4", "interop 5.2", "ui 0.4", "menus 6.2" and "app 5.5" point into
`formats_native.md`, `model_and_tools.md`, `formats_interop.md`, `ui_layout_theme.md`, `ui_dialogs_menus.md`
and `app_behavior.md`. "P" and "F" in the decision record name the parity-first and the performance-first
proposal.

Conventions: MUST / SHOULD / MAY are design decisions for the port. **UNVERIFIED** marks an assumption that
nobody has confirmed yet; section 22 collects them. Crate versions were checked with
`cargo search <crate> --limit 1` on 2026-10-18. API facts about egui, winit, wgpu, zip, flate2, rodio and
slotmap were checked in the published sources of the exact versions listed in section 4.

---

## 0. Summary

- **Base: the parity-first proposal.** Only that proposal can reach the stated goal of 100 % UI parity. The
  original's bindings tell keypad keys from top-row keys and left from right modifiers, and egui's key model
  (and therefore eframe) cannot (1.2, fact F1). The base also brings the single apply path for load, undo
  restore and paste, reference twins for every optimisation, the frozen voxel colliders of tool strokes,
  the switchable float-evaluation policy, the parity ledger and the strict/port compatibility profiles.
- **Grafted from the performance-first proposal:** compact GPU records (176-byte element instances, 8-byte
  voxel face records with per-direction culling), per-view frame keys and attachments shared by all views,
  a standalone BSON crate with schema tables and canonical fast paths, the identity-preserving `ObjList`
  with `ObjId` keys (instead of slotmap), lazy image decoding with alpha statistics, a pure
  action/menu/input crate, a foreground and a background worker pool plus one I/O thread, highlight state
  separate from the selection, crash backups and the deviation table.
- **Dropped from both:** a general uGUI layout engine (the UI spec needs only its rule set, ui 0.4),
  automatic greedy merging of viewport voxels, history compression and draw-list hoisting as 1.0 features
  (they stay available as measured-need optimisations in M7), eframe, slotmap, and four extra packaging
  tools.
- **Shape:** 20 library crates, 2 binaries, a dev-only test kit and `xtask`; `#![forbid(unsafe_code)]` in
  everything that ships. The document is owned by the UI thread; workers get immutable `Arc` snapshots; no
  lock guards the document.
- **Plan:** nine milestones, 66 work packages with disjoint file ownership. M0 freezes the contracts; then
  four tracks (data core and formats, editing core, scene and renderer, UI) run in parallel and join in M6.

---

## 1. Decision record

### 1.1 Scores

Scale 1 (poor) to 10 (excellent). The weights are equal; the goal statement puts parity and compatibility
first, which also decides ties.

| Criterion | Parity-first | Performance-first | Deciding observations |
|---|---|---|---|
| Parity feasibility | 9 | 6 | F lacks key identity (keypad, modifier sides), models no frozen voxel colliders during strokes, swaps undo snapshots without the record-shape normalisation of load (loses the "chunk not written after restore" rule), formats floats through the correctly rounded std formatter only, and auto-enables a merged voxel mesh above a face threshold. P covers all of these. |
| Performance | 7 | 9 | F: 8-byte voxel faces with direction culling, frame keys, shared attachments, parallel BSON with fast paths, restores that keep GPU data. P: 32 bytes per voxel face plus indices, restores rebuild every object, and its "under 20 draws for 10 000 elements" claim holds only when translucent objects are few or clustered (worst case is runs + 2 x translucent). |
| Safety and stability | 8 | 8 | Both: no unsafe, fuzzing, limits before allocation, atomic writes, panic isolation. P denies indexing and panics in parsers; F adds a crash backup and a device-loss plan. P's own shell is extra code, but it is built from the official egui integration crates. |
| Simplicity | 6 | 5 | P over-builds the UI (a uGUI clone with content-size and aspect fitters) although ui 0.4 says no general anchor engine is needed. F over-builds the hot paths (history compression, hoisting compiler, flat-merge mode, pool compaction, two submission tiers, retention cap). |
| Crate choices | 8 | 6 | F depends on eframe, which blocks parity, and lists five packaging tools, a reserved XML crate, smallvec and keyring in the core plan. P needs a workaround for slotmap's unsafe key macro. F contributes `codepage`, which the signature rule needs. |
| Testability | 9 | 8 | P: five oracle classes, five parity tiers, ledger, reference twins, headless shell and input replay. F: literal-versus-batched render proof, command-log replay, benchmark gates. |
| Parallelizability of the plan | 8 | 7 | P: four tracks from M0 with mocks and contracts. F: vertical feature slices (good), but UI and interop start late and M0 is serial. |
| **Total (of 70)** | **55** | **49** | |

### 1.2 Facts that decided the design (verified in the published sources)

| Id | Fact | Consequence |
|---|---|---|
| F1 | egui-winit 0.37.0 maps `Numpad0..9` to the same egui keys as `Digit0..9`, `NumpadAdd` to `Plus`, `NumpadSubtract` to `Minus`, `NumpadEnter` to `Enter`, and ignores the winit key location; egui's `Modifiers` has no left/right sides; eframe 0.37.0 hands the application only egui input (`raw_input_hook`), never winit key events. | menus 6.1 ("+" is keypad plus only), 6.3 (Ctrl + keypad digits), 6.4 and 6.6 (side-specific key names), tools 12.2 (Left Alt eyedropper) and the element settings Right Ctrl rule cannot be met on eframe. Cuboid uses its own shell on winit 0.30.13 with egui-winit and egui-wgpu. |
| F2 | slotmap 1.1.1's `new_key_type!` expands to an `unsafe impl` in the calling crate. | Own `ObjList<T>` (section 8.2). |
| F3 | egui's text cursor treats only alphanumerics and `_` as word characters. ui 16.6 separates words only at space, comma, full stop, colon, semicolon and line breaks. | Own text field widget (14.5). |
| F4 | egui-wgpu 0.37 `register_native_texture` takes an `Rgba8Unorm` texture; its gamma-framebuffer shader returns texel times vertex colour unchanged when dithering is off. | 3D views composite exactly through egui images if their alpha is 1 (13.1). |
| F5 | zip 9.0.5 depends on flate2 with default features off. | The crates that use zip also depend on flate2 directly to enable its backend (4.1). |
| F6 | wgpu 30 `multi_draw_indirect` requires `DownlevelFlags::INDIRECT_EXECUTION`; `VERTEX_STORAGE` and `INDIRECT_FIRST_INSTANCE` are separate capabilities. | Two voxel submission tiers (13.5). |
| F7 | egui, egui-winit and egui-wgpu 0.37.0 and windows-registry 0.100.0 declare `rust-version = "1.95"`. | Toolchain pinned to 1.95.0. |

### 1.3 Provenance of the main decisions

| Decision | From |
|---|---|
| Own shell (winit + egui-winit + egui-wgpu `winit::Painter`), engine key identity, UI scale rule, wake-up scheduling, headless mode | P |
| Single apply path for open, merge, restore and paste; reference twins with property tests | P |
| Identity-preserving `ObjList` with `ObjId`, display ids in a parallel vector, restores that keep unchanged GPU data | F, made subject to P's reference twin |
| Numerics crate with switchable evaluation policy and a formatter with an explicit rounding stage | P |
| Standalone BSON crate, schema tables generating encoder and decoder, canonical fast paths, parallel per-chunk coding, retention cap | F |
| Retained raw spans for byte-identical re-save | both |
| Element instances of 44 words, vertex pulling without vertex buffers | F |
| Voxel faces as 8-byte records in six direction pools, direction and frustum culling | F |
| Canonical, unmerged voxel faces in the viewport; greedy only for exports | P |
| Transparency: runs of binary-alpha objects batched in sorted order; translucent objects drawn exactly as the original | P, with F's face-crop alpha statistics |
| Frame keys per view; attachments shared by all views | F |
| Frozen voxel collider snapshot during strokes | P |
| Highlight set separate from the selection | F |
| Spec-rule layout functions; own pointer-event routing with uGUI raycast semantics; own focus model and text field | judge: P's event and focus layer without P's general layout engine |
| Async flows on a UI-thread executor; codec sessions as explicit step machines | P (flows), both (sessions) |
| Two rayon pools plus one FIFO I/O thread | F |
| Crash backup from the panic hook | F |
| Parity ledger, string and compat catalogues, oracle classes, parity tiers | P |
| Deviation table needing owner sign-off | F, merged with the `Compat` profiles of P |

---

## 2. Principles

1. **Parity before speed.** An optimisation is admissible only when it is provably or testably equivalent to
   the specified behaviour; otherwise it is a deviation (section 17) that needs owner sign-off.
2. **One semantic path.** Element creation, rotation storage, UV folding, colour packing, voxel writes and
   texture registration exist once. Load, undo restore, paste and importers call the same functions.
3. **Reference twin for every optimisation.** BVH face optimisation, parallel voxelisation, fast restore,
   batched transparency, the voxel face pools and the incremental atlas each keep a slow, literal reference;
   property tests prove identical results.
4. **Numerics are centralised** in `cuboid-num`; float-to-int `as` casts are denied elsewhere.
5. **Author in the original's units.** Left-handed, Y-up internal space; images stored bottom row first; UV
   origin bottom-left; UI geometry in canvas pixels. Conversions happen once, at the edges (PNG I/O, egui's
   y-down points, Minecraft space in exporters).
6. **Pure core, thin shells.** Model, operations, history, interaction and scene math are headless and
   deterministic. The renderer and the UI are projections of state plus input adapters.
7. **The UI thread never waits** on disk, compression, encoding, network or long operations.
8. **Pay for what changed.** Revisions, a change journal and copy-on-write `Arc`s drive uploads, caches and
   re-renders; idle frames cost nothing.
9. **Deterministic regardless of thread count.** Parallel results merge in a fixed order; randomness comes
   from an injected, seeded generator.
10. **No panic on input.** Every size read from a file is checked against a limit before allocation; every
    parser is fuzzed.
11. **Clean room.** Implementers work from `docs/spec` and `docs/design` only (section 20.3).

---

## 3. Workspace

### 3.1 Layout

```
cuboid/
  Cargo.toml              members, [workspace.package], [workspace.dependencies] (all versions), [workspace.lints], profiles
  rust-toolchain.toml     channel = "1.95.0", components rustfmt, clippy
  .cargo/config.toml      +crt-static for *-windows-msvc
  deny.toml  about.toml   licence allow-list, advisories, bans, sources; third-party notice template
  crates/
    cuboid-num/           L0  rounding, snapping, Euler/quaternion, colour packing, G7/X9/NJ text, invariant parsing, EngineRng
    cuboid-bson/          L0  BSON subset: zero-copy reader, size-patching writer, lenient getters, spans, canonical dump
    cuboid-actions/       L0  EngineKey, key names, binding strings, chords, dispatcher, held-key map, ActionId, menu tables
    cuboid-assets/        L0  embedded own art, fonts, sounds, bundled presets, palettes, data tables, skin-pack loader
    cuboid-image/         L1  Image (bottom-up), stored formats, BC1/BC3, file codecs, signature, raster primitives, packing, alpha stats
    cuboid-voxel/         L1  VoxelWorld, chunk table, palette, writers, mappings, transforms, searches, face mesher, DDA
    cuboid-settings/      L1  settings schema with Cubik keys, defaults, load/save, legacy import, recent files
    cuboid-model/         L2  Document, ObjList, objects, setters, UV, journal, selection, apply/capture, snapshots, invariants
    cuboid-ui-kit/        L2  layout rules, pointer events, focus, skins, sprites, 9-slice, tweens, text metrics, widgets
    cuboid-ops/           L3  every editing operation and tool algorithm (pure), primitive generators, reference twins
    cuboid-scene/         L3  views, cameras, navigation, preview anchors, picking (BVH, DDA, colliders), gizmo math, atlas layout, draw lists
    cuboid-io/            L3  format registry, pipeline texts, session traits, profiles, text dialects and emitters, .lnk, atomic writes
    cuboid-validate/      L3  the model validator: documents and profiles in, issues with stable codes and fixes out
    cuboid-core/          L4  Core facade, command table, history, clipboards, start queue, interaction, tools, Compat, Clock
    cuboid-fmt-cubik/     L4  .cubik container, records, schema, retention, extension entry; legacy .cub
    cuboid-fmt-bbmodel/   L4  Blockbench .bbmodel in/out
    cuboid-fmt-minecraft/ L4  Java JSON in/out, resource pack, display presets, .mcpe, .js, .java, .jpm, entity atlas
    cuboid-fmt-mesh/      L4  OBJ/MTL in/out, STL, PNG sprite import, PNG render export driver, glTF (M8)
    cuboid-fmt-voxel/     L4  VOX, QB, QEF, binvox, schematic, NBT, ZOX, CSV, Stonehearth, Staxel, block tables
    cuboid-render/        L4  wgpu renderer: targets, pipelines, uploads, transparency runs, voxel pools, overlays, PNG render
    cuboid-ui/            L5  the window: zones, strips, view zone, status bar, menus, panels, popups, helpers, dialogs, file browser, strings
    cuboid-shell/         L5  winit event loop, egui-winit state, egui-wgpu painter, engine key mapping, window rules, headless mode
    cuboid-library/       L5  the asset library and project brief: scopes, import, style references, outline, brief JSON
    cuboid-cli/           L6  binary `cuboid-cli`: dump, roundtrip, brief, check, outline (no UI, no GPU)
    cuboid-mcp/           L6  the MCP server of the agent toolkit (docs/mcp.md): protocol, project paths, the 18 tools, the Renderer trait, client snippets
    cuboid-term/          L6  binary `cuboid-term`: the console twin of `cuboid`, installed next to it as `cuboid.com` (no dependencies)
    cuboid-app/           L7  binary `cuboid`: wiring, flow executor, jobs, I/O thread, backups, lifecycle, crash, platform, audio, logging; `cuboid mcp` and its offscreen renderer
    cuboid-testkit/       dev golden harness, diff tiers, canonical dumps, scenario DSL, oracle gating, fixtures
  xtask/                  ledger check, layering lint, bless, licences, package, bench-compare
  fuzz/                   cargo-fuzz targets (nightly; not a workspace member; not shipped)
  assets/                 own source art, fonts, sounds, data tables
  parity/                 ledger/<wp-id>.toml, strings.toml, compat.toml
  tests/golden/           public synthetic corpus (no user files, no private captures)
  docs/spec/  docs/design/
```

### 3.2 Crate responsibilities

| Crate | Owns | Must not |
|---|---|---|
| cuboid-num | every parity-sensitive formula of tools 1.6, 3.4, 4.5, 7.3, native 4.3.2, 4.10, interop 3.1-3.2; evaluation policy; `EngineRng` | depend on anything but glam |
| cuboid-bson | BSON wire format of native 3.1-3.6, spans, limits | know about Cubik records |
| cuboid-actions | engine key names (menus 6.6), canonical chords (menus 6.1), dispatch (menus 6.2), held map (menus 6.4), action registry and menu data (menus 1-3, 5) | depend on egui, winit or core |
| cuboid-assets | embedded bytes plus inventories (picture names, sizes, borders, ui 13, 15) | decode anything heavier than PNG headers |
| cuboid-image | `Image`, stored formats of native 4.5.1, codecs, signature (native 4.5), raster primitives (tools 13.2), packers, alpha statistics | know elements or faces |
| cuboid-voxel | tools 7 storage and algorithms, the canonical face mesher (tools 7.6), DDA | allocate chunks on reads |
| cuboid-settings | app 10, 11 data and persistence | call OS APIs (the app passes legacy values in) |
| cuboid-model | document state and the setter semantics of tools 2-6, apply/capture of native 5-6 | perform I/O, spawn threads, know codecs or UI |
| cuboid-ui-kit | ui 0, 10, 12-16 mechanics | know documents or files |
| cuboid-ops | tools 4-14 operations as functions over an edit context | push undo, show messages, know cameras |
| cuboid-scene | app 5, 7-9 math, picking, gizmo geometry and hit volumes, draw-list compilation | depend on wgpu |
| cuboid-io | interop 1-6 shared machinery, text dialects (native 9.4, interop 3, 8.6, 10.1), `.lnk` (interop 28) | write files itself (the I/O thread writes artifacts) |
| cuboid-core | commands, guards, undo/redo (tools 15), clipboards (tools 11), start queue, interaction (tools 8-9, 12-13 lifecycles), `Compat`, `Clock` | depend on egui, winit or wgpu |
| cuboid-fmt-* | one module per format with its fuzz target and goldens | share mutable state, call UI, touch the GPU |
| cuboid-render | GPU resources and passes of app 8, 15 | read the document directly (it consumes scene extracts) |
| cuboid-ui | everything visible in the window | mutate state except through intents applied after the frame build |
| cuboid-shell | platform window, input translation, painter, presentation, headless frames | contain application logic |
| cuboid-validate | the validator: issue codes, rules, fixes as plain data | perform I/O, or depend on the library (the app, and `cuboid_library::check` for the CLI and the server, fill the profile from the brief) |
| cuboid-library | `library.json`, stored copies, import and classification, the brief, style references, the outline | know the window, the GPU or the command layer |
| cuboid-mcp | the MCP protocol of both eras, the project paths, open models, the 18 tools and the ops of `model_edit`, client snippets, the self-check | use wgpu, egui, winit, async code or a socket (the app gives it a `Renderer`) |
| cuboid-app | composition, flows, jobs, OS integration, `cuboid mcp` and its offscreen renderer | contain format or model logic |

### 3.3 Dependency graph and layering rules

```
L0  num   bson   actions   assets
L1  image(num)   voxel(num)   settings(num)
L2  model(num, image, voxel)   ui-kit(assets)
L3  ops(model, image, voxel, num)   scene(model, voxel, image, num)   io(model, num)   validate(model, assets)
L4  core(model, ops, scene, actions, num)   fmt-cubik(io, bson, model)   fmt-minecraft(io, model, ops)
    fmt-mesh(io, model, ops, voxel)   fmt-voxel(io, model, voxel)   fmt-bbmodel(io, model, num)   render(scene, image)
L5  ui(ui-kit, core, actions, io, settings, scene)   shell(actions)
    library(model, io, fmt-bbmodel, fmt-cubik, fmt-minecraft, validate, image, assets, num)
L6  cli(core, io, model, bson, library, fmt-cubik, fmt-mesh, fmt-minecraft, fmt-voxel)
    mcp(core, library, validate, io, ops, model, image, assets, fmt-cubik, fmt-minecraft)
    term()
    testkit(dev): model, io, fmt-*, image
L7  app(everything)
```

`cuboid-mcp` uses `cuboid-library` (L5), so it sits on L6, and `cuboid-app` uses `cuboid-mcp`, so it moves to L7
(`agent_toolkit_r1.md`, D-R3). `LAYERS` in `xtask/src/layering.rs` holds the layer of every crate.

Rules, enforced by `cargo xtask layering` over `cargo metadata`:

1. A crate depends only on crates of lower layers.
2. Only `cuboid-render`, `cuboid-shell` and `cuboid-app` depend on wgpu; only `cuboid-ui-kit`, `cuboid-ui`,
   `cuboid-shell` and `cuboid-app` depend on egui; only `cuboid-shell` depends on winit.
3. Format crates never depend on core, scene, render or UI crates.
4. `cuboid-model` performs no I/O and spawns no threads; `cuboid-ops` knows no views or cameras.

---

## 4. External dependencies

### 4.1 Runtime (versions from `cargo search`, 2026-10-18)

| Crate | Version | Used by | Configuration and notes |
|---|---|---|---|
| egui | 0.37.0 | ui-kit, ui, shell | default features are empty (no bundled fonts); painter, fonts, galleys, input |
| egui-winit | 0.37.0 | shell | `default-features = false`, features `clipboard`, `wayland`, `x11` (no `links`; `accesskit` later) |
| egui-wgpu | 0.37.0 | shell, render | `default-features = false`, features `winit`, `wayland`, `x11`; `RendererOptions { dithering: false, msaa_samples: 1 }` |
| wgpu | 30.0.1 | render, shell, app | `default-features = false`, features `std`, `parking_lot`, `wgsl`, `dx12`, `vulkan`, `metal`, `gles`; the version egui-wgpu 0.37 requires |
| winit | 0.30.13 | shell | latest stable; `cargo search` shows 0.31.0-beta.3, a pre-release; egui-winit 0.37 requires 0.30.13 |
| glam | 0.34.1 | num and up | f32 for stored values; f64 only for derived, non-persisted values |
| serde | 1.0.229 | settings, assets, fmt-* (modern), io | `derive` |
| serde_json | 1.0.154 | same | `preserve_order` where modern JSON import keeps unknown keys |
| thiserror | 2.0.21 | all libraries | error enums |
| log | 0.4.34 | all | facade; own sink in the app |
| indexmap | 2.14.2 | model, io, settings | ordered sets and maps (selection order, key order) |
| rustc-hash | 2.1.3 | model, voxel, scene, render, actions | maps whose iteration order never reaches output |
| bitflags | 2.13.2 | model, actions, core, io | face flags, categories, capabilities |
| rayon | 1.12.0 | ops, voxel, scene, fmt-*, app | two pools (section 11) |
| bytemuck | 1.25.2 | render | `cast_slice` on `[u32]` / `[f32]` only; no `derive` feature (its derives expand to `unsafe impl`) |
| fixedbitset | 0.5.7 | voxel, scene | dirty sets, visited sets of tool searches |
| image | 0.25.10 | image, assets | `default-features = false`, features `png`, `jpeg`, `tga`; DDS uses Cubik's own rule (native 4.5.1) |
| zip | 9.0.5 | fmt-cubik, fmt-minecraft | `default-features = false`, features `deflate-flate2` |
| flate2 | 1.1.10 | fmt-cubik, fmt-minecraft, fmt-voxel | default features (`rust_backend`, miniz_oxide); a `fast-deflate` cargo feature switches to `zlib-rs` if W7.1 benchmarks justify it |
| md-5 | 0.11.0 | image | texture signature (native 4.5) |
| encoding_rs | 0.8.42 | image, io | ANSI code pages, UTF-16 |
| codepage | 0.1.3 | image | Windows code page number to encoding |
| fastnbt | 2.6.3 | fmt-voxel | schematic and structure NBT (root compound name control) |
| dot_vox | 5.2.0 | fmt-voxel | modern-mode VOX import only; test oracle |
| tobj | 4.0.5 | fmt-mesh | modern-mode OBJ import only; test oracle |
| etagere | 0.3.0 | scene, image | runtime atlas allocation; deterministic packing of persisted atlases |
| rfd | 0.17.2 | app | native dialogs when "Use Native file browser" is on |
| directories | 6.0.0 | app | config, data, cache folders (pulls an MPL-2.0 helper, allowed in `deny.toml`) |
| tempfile | 3.27.0 | io, settings, app | atomic replace (temporary file in the target folder, then persist) |
| jiff | 0.2.38 | app, ui | local weekday names for backups, dates in the file browser |
| opener | 0.9.0 | app | open folders, files and links; reveal files |
| pollster | 1.0.1 | shell | block on adapter and device requests at start-up |
| windows-registry | 0.100.0 | app (Windows only) | legacy preferences, ANSI code page, Steam path, explicit association action |
| rodio | 0.23.0 | app, feature `sound` (default on) | `default-features = false`, feature `playback`; embedded PCM played at volume 0.4 (menus 8.3) |
| profiling | 1.0.18 | optional | development builds only (already used inside egui-wgpu) |

### 4.2 Optional or later (versions verified; not in the 1.0 dependency graph unless the feature is enabled)

| Crate | Version | When |
|---|---|---|
| ureq | 3.4.2 | feature `net` (M8): Sketchfab v3, opt-in skin lookup, opt-in update check |
| keyring | 4.2.0 | feature `keyring` (M8): Sketchfab token in the OS credential store |
| gltf-json | 1.4.1 | M8 glTF/GLB export |
| lz4_flex | 0.14.0 | history compression, only if W7.1 shows that the memory budget fails without it |

### 4.3 Development and tools

| Crate / tool | Version | Use |
|---|---|---|
| bson | 3.1.0 | dev-dependency only: differential oracle for `cuboid-bson` |
| proptest | 1.12.0 | property and differential tests |
| insta | 1.49.0 | reviewed text snapshots of canonical dumps and emitted text formats |
| criterion | 0.8.2 | benchmarks with regression gates |
| egui_kittest | 0.37.0 | widget-level interaction tests (features `wgpu`, `snapshot`) |
| image-compare | 0.5.0 | SSIM for the P3 parity tier |
| stl_io | 0.11.0 | STL read-back oracle |
| gltf | 1.4.1 | glTF read-back oracle (M8) |
| arbitrary, libfuzzer-sys | 1.5.0, 0.4.13 | fuzz crate only |
| cargo-deny | 0.20.2 | licences, advisories, bans, sources |
| cargo-about | 0.9.2 | third-party notices |
| cargo-fuzz | 0.13.2 | coverage-guided fuzzing (nightly job) |
| cargo-nextest | 0.9.148 | CI test runner |
| cargo-packager | 0.11.8 | Windows installer, macOS bundle and disk image, Linux AppImage and .deb |

### 4.4 Considered and not used

| Candidate | Reason |
|---|---|
| eframe 0.37.0 | no access to winit key events (fact F1) |
| slotmap 1.1.1 | key macro expands to `unsafe impl` (fact F2); a 200-line `ObjList` also gives revision stamps |
| bson as a runtime dependency | byte-identical rewrite needs element spans, lenient typed reads, duplicate-key and quaternion-order semantics and explicit limits |
| bvh 0.13.0, parry3d 0.31.1 | an incrementally refit BVH over boxes and triangles is small and tuned to this data |
| smallvec (1.16.3 stable, 2.0.0-beta.3) | no measurable need |
| winreg 0.56.0 | windows-registry (Microsoft-maintained) covers the needs |
| tokio | no async I/O need; flows run on a local executor, work on rayon and one I/O thread |
| crossbeam-channel 0.5.17 | `std::sync::mpsc` suffices |
| quick-xml 0.42.0 | no XML format in scope |
| arboard 3.7.0 (direct) | model data never goes to the OS clipboard (app 14); egui-winit uses it for text fields |
| notify | textures are re-checked on focus gain only (native 4.5) |
| rand | `EngineRng` is enough and keeps tests deterministic |
| guillotiere 0.7.0 | etagere covers both atlas uses |
| cargo-deb, cargo-wix, cargo-bundle, cargo-dist | cargo-packager covers every target |

---

## 5. Toolchain, lints and profiles

- Edition 2024, `rust-version = "1.95"`, toolchain pinned to 1.95.0 (fact F7).
- Every crate root starts with `#![forbid(unsafe_code)]`; `[workspace.lints.rust] unsafe_code = "forbid"`.
  Only `fuzz/` (generated libFuzzer entry points) is exempt, and it is never shipped.
- Clippy: `all` as warnings, CI with `-D warnings`; `await_holding_refcell_ref = "deny"` (flow discipline);
  `unwrap_used`, `expect_used`, `panic` and `indexing_slicing` denied in non-test code of `cuboid-bson`,
  `cuboid-io`, `cuboid-image`, `cuboid-voxel` and every `cuboid-fmt-*`; `float_cmp` allowed only in
  `cuboid-num` and `cuboid-model` (exact comparison is specified behaviour); `cast_possible_truncation`
  denied outside `cuboid-num`.
- Parity-relevant code cites its source in a doc comment, for example `/// spec: tools 3.4`.
- Libraries return `Result<_, CrateError>` built with thiserror; catch-all errors only in binaries.
- Profiles: release `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "unwind"` (job isolation
  and the crash hook), `debug = "line-tables-only"` with split debug info; dev `opt-level = 1` with
  `[profile.dev.package."*"] opt-level = 3`; bench inherits release with full debug info.
- Windows MSVC builds link the CRT statically (`.cargo/config.toml`). Baseline CPU features only.

---

## 6. Key types (signatures only)

The sketches below fix names and shapes of the cross-crate contracts; they are not compilable code. W0.2
turns them into stubs.

```rust
// cuboid-num ------------------------------------------------------------------------------------
pub enum Eval { Strict32, Wide64 }                       // evaluation policy, one const for the workspace
pub const EVAL: Eval;
pub fn round_he(x: f32) -> f32;                          // half to even
pub fn trunc_i32(x: f32) -> i32;                         // toward zero, saturating, NaN -> 0 (documented)
pub fn wrap360(c: f32) -> f32;                           // (c + 360) % 360, sign of dividend
pub fn snap_angle(c: f32, restricted: bool) -> f32;      // 22.5 or 5 degree grid
pub fn uv_rotation(r: i32) -> i32;                       // (round(r / 90) * 90 + 360) % 360
pub fn round4(v: f32) -> f32;
pub fn engine_eq(a: Vec3, b: Vec3) -> bool;              // squared distance below 9.99999944e-11
pub fn euler_to_quat(deg: Vec3) -> Quat;                 // q = qY * qX * qZ
pub fn quat_to_euler(q: Quat) -> Vec3;                   // with the positivity rule of native 4.3.2
pub mod pack { pub fn element(c: [f32; 4]) -> i32; pub fn submesh(c: [f32; 4]) -> i32;
               pub fn voxel_tool(c: [u8; 4]) -> u32; pub fn unpack_element(v: i32) -> [f32; 4];
               pub fn hex(v: u32) -> String; pub fn parse_hex(s: &str) -> [u8; 4]; }
pub mod numtext { pub fn g7(x: f32) -> String; pub fn x9(x: f32) -> String; pub fn nj(x: f32) -> String;
                  pub fn parse_or_zero_f32(s: &str) -> f32; pub fn parse_or_zero_i32(s: &str) -> i32; }
pub struct EngineRng { /* xorshift128 */ }
impl EngineRng { pub fn seeded(seed: u64) -> Self; pub fn unit_f32(&mut self) -> f32;
                 pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32; }

// cuboid-bson -----------------------------------------------------------------------------------
pub struct Span { pub start: u32, pub end: u32 }
pub struct Limits { pub max_doc: u32, pub max_depth: u8, pub max_array: u32 }
pub struct RawDoc<'a> { /* bytes of one document */ }
impl<'a> RawDoc<'a> {
    pub fn parse(bytes: &'a [u8], limits: &Limits) -> Result<Self, BsonError>;
    pub fn iter(&self) -> RawIter<'a>;                                   // document order
    pub fn last(&self, key: &[u8]) -> Result<Option<RawElem<'a>>, BsonError>;  // duplicate keys: last wins
}
pub struct RawElem<'a> { pub key: &'a [u8], pub ty: ElemType, pub value: Span, pub whole: Span }
impl RawElem<'_> {                                                       // lenient conversions of native 3.6
    pub fn f32(&self) -> Result<f32, BsonError>; pub fn i32(&self) -> Result<i32, BsonError>;
    pub fn bool(&self) -> Result<bool, BsonError>; pub fn enum_i32(&self, names: &[&str]) -> Result<i32, BsonError>;
    pub fn str_opt(&self) -> Result<Option<&str>, BsonError>; pub fn bytes_opt(&self) -> Result<Option<Cow<'_, [u8]>>, BsonError>;
    pub fn doc(&self) -> Result<Option<RawDoc<'_>>, BsonError>;
}
pub struct DocWriter { /* Vec<u8> + open-document stack */ }
impl DocWriter {
    pub fn begin(&mut self, key: Option<&'static [u8]>) -> Mark; pub fn end(&mut self, m: Mark);  // size patching
    pub fn f32(&mut self, key: &'static [u8], v: f32); pub fn i32(&mut self, key: &'static [u8], v: i32);
    pub fn bool(&mut self, key: &'static [u8], v: bool); pub fn str(&mut self, key: &'static [u8], v: &str);
    pub fn null(&mut self, key: &'static [u8]); pub fn bin(&mut self, key: &'static [u8], v: &[u8]);
    pub fn raw(&mut self, element_bytes: &[u8]);                         // retained span copy
}

// cuboid-actions --------------------------------------------------------------------------------
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum EngineKey { Alpha0, /* .. */ Alpha9, Keypad0, /* .. */ Keypad9, KeypadPlus, KeypadMinus, KeypadEnter,
    A, /* .. */ Z, F1, /* .. */ F15, LeftShift, RightShift, LeftControl, RightControl, LeftAlt, RightAlt,
    LeftCommand, RightCommand, LeftWindows, RightWindows, Space, Return, Escape, Tab, Backspace, Delete,
    PageUp, PageDown, UpArrow, DownArrow, LeftArrow, RightArrow, Mouse0, /* .. */ Mouse6, /* ... */ }
impl EngineKey { pub fn name(self) -> &'static str; pub fn from_name(s: &str) -> Option<Self>; }
pub struct Chord(Box<str>);                                   // canonical "Ctrl+Shift+S"
pub fn canonical_shortcut(display: &str) -> Chord;           // menus 6.1
pub fn parse_binding(s: &str) -> [Option<EngineKey>; 4];     // menus 6.6
pub fn canonical_binding(keys: &[Option<EngineKey>; 4]) -> String;
pub struct DispatchGate { pub modal: bool, pub ui_holds_selection: bool, pub enabled_menus: MenuMask }
pub struct Dispatcher { /* held list in press order + chord table */ }
impl Dispatcher {
    pub fn key_down(&mut self, k: EngineKey, gate: &DispatchGate) -> Vec<ActionId>;      // event-ordered
    pub fn key_up(&mut self, k: EngineKey);
    pub fn frame_strict(&mut self, downs: &[EngineKey], mouse_down: bool, gate: &DispatchGate) -> Vec<ActionId>;
}
pub enum HeldBinding { ModeRotate, ModeScale, ModeClone, PanView, Focus, ModifySelection,
    ModifySelectionMultiple, RotateView, MoveRight, MoveLeft, MoveUp, MoveDown, Submit, Cancel }
pub struct HeldMap { /* up to four keys per binding */ }
impl HeldMap { pub fn is_held(&self, b: HeldBinding, held: &HeldKeys) -> bool;
               pub fn set_custom(&mut self, b: HeldBinding, keys: [Option<EngineKey>; 4]); }
pub enum ActionId { NewFile, Open, Merge, Save, /* every row, popup, toolbar and hidden binding */ }
pub struct RowDef { pub label: &'static str, pub shortcut: Option<&'static str>, pub icon: Option<&'static str>,
                    pub target: RowTarget, pub flags: RowFlags /* A, U, L */ }
pub struct MenuDef { pub id: MenuId, pub min_width: u16, pub rows: &'static [RowDef] }
pub static MENUS: &[MenuDef];

// cuboid-image ----------------------------------------------------------------------------------
pub struct Image { pub w: u32, pub h: u32, pub stored: StoredPixels, pub rev: Rev /* + lazy RGBA8 and AlphaStats */ }
impl Image {
    pub fn rgba(&self) -> &[u8];                                  // level 0, rows bottom to top, decoded on first use
    pub fn alpha(&self) -> &AlphaStats;                           // summed-area table of 0 < alpha < 255
    pub fn edit(&mut self) -> &mut RgbaBuf;                       // becomes RGBA32, drops retained bytes
}
pub struct StoredPixels { pub format: i32, pub raw: Arc<[u8]> }   // format code and bytes as read (mips included)
pub fn decode_file(bytes: &[u8], kind: ImageFileKind) -> Result<Image, ImageError>;
pub fn encode_png(img: &Image) -> Result<Vec<u8>, ImageError>;
pub fn signature(file_bytes: &[u8], code_page: u16) -> String;   // MD5 bytes decoded with the ANSI code page

// cuboid-voxel ----------------------------------------------------------------------------------
pub struct VoxelWorld { pub res: u32, n: u32, slots: Vec<ChunkSlot>, pub palette: Palette,
    pub active_box: IBox3, pub setup_corners: WBox3, pub ao: bool, dirty: FixedBitSet, pub rev: Rev }
pub enum ChunkSlot { Unallocated, EmptyWritten, Data(Arc<Chunk>) }   // slot = (cx * n + cy) * n + cz
pub struct Chunk { cells: [u32; 4096] /* x + 16y + 256z */, nonzero: u16, occ: [u64; 64] }
impl VoxelWorld {
    pub fn new(res: u32, ao: bool, active: IBox3) -> Self;
    pub fn read(&self, i: I3) -> u32; pub fn read_solid_outside(&self, i: I3) -> u32;
    pub fn write_full(&mut self, i: I3, v: u32); pub fn write_bounded(&mut self, i: I3, v: u32);
    pub fn write_unchecked(&mut self, i: I3, v: u32);
    pub fn write_many(&mut self, cells: &mut dyn Iterator<Item = (I3, u32)>, kind: WriteKind);
    pub fn take_dirty(&mut self) -> FixedBitSet;
}
#[derive(Copy, Clone)] pub struct VoxelFace(pub [u32; 2]);          // 13.5 layout; direction implied by pool
pub fn mesh_chunk(world: &VoxelWorld, slot: usize, out: &mut [Vec<VoxelFace>; 6]);
pub fn expand_face(face: VoxelFace, dir: Dir, cell: f32) -> [ExportVertex; 4];   // CPU quads for exports
pub fn dda(world: &VoxelWorld, ray: Ray, max_len: f32) -> Option<VoxelHit>;

// cuboid-model ----------------------------------------------------------------------------------
pub struct ObjId(NonZeroU64);                                    // session-unique, never persisted
pub struct Rev(u64);
pub enum ObjRef { Element(ObjId), Mesh(ObjId), Blueprint, ModelOrigin }
pub struct ObjList<T> { ids: Vec<ObjId>, items: Vec<Arc<T>>, display_ids: Vec<u32>,
                        next_display_id: u32, index: FxHashMap<ObjId, u32>, structure_rev: u64 }
impl<T: Clone> ObjList<T> {
    pub fn get(&self, id: ObjId) -> Option<&T>;
    pub fn edit(&mut self, id: ObjId, j: &mut ChangeJournal) -> Option<&mut T>;   // Arc::make_mut, stamp, journal
    pub fn push(&mut self, item: T) -> ObjId;                                     // next display id
    pub fn remove_set(&mut self, ids: &FxHashSet<ObjId>);
    pub fn stable_partition(&mut self, front: impl FnMut(&T) -> bool);
    pub fn snapshot(&self) -> ListSnap<T>;                                        // pointer copies
    pub fn restore(&mut self, s: &ListSnap<T>);                                   // keeps ObjIds, renumbers from 1
    pub fn clear(&mut self);                                                      // display id counter back to 0
}
pub struct Document {
    pub props: Arc<ModelProps>, pub view: Arc<ViewProps>, pub blueprint: Blueprint, pub model_box: ModelBox,
    pub elements: ObjList<Element>, pub meshes: ObjList<Mesh>, pub textures: TextureList,
    pub voxels: Option<VoxelWorld>, pub display: Arc<DisplaySlots>, pub overrides: Arc<[OverrideCase]>,
    pub retained: Option<Arc<Retained>>, pub journal: ChangeJournal, pub revision: Rev,
}
pub struct ObjectBase { pub label: Label, pub p: Vec3, pub s: Vec3, pub e: Vec3, pub o: Vec3,
    pub hidden: bool, pub locked: bool, pub perms: Perms, pub drag_sensitivity: f32,
    pub gizmo_size: Vec3, pub gizmo_pivot: Vec3 }
pub struct Element { pub base: ObjectBase, pub kind: ElementType, pub color: [f32; 4],
                     pub faces: [Face; 6], pub flags: ElementFlags }
pub struct Face { pub name: FaceName, pub active: bool, pub tint: i32, pub cull: FaceFlags,
                  pub texture: Label, pub uv: UvRect }
pub struct UvRect { rot: i32, tl_raw: Vec2, br_raw: Vec2 }
impl UvRect { pub fn tl(&self) -> Vec2; pub fn br(&self) -> Vec2;              // folded (native 4.4.3)
              pub fn set(&mut self, tl: Vec2, br: Vec2); pub fn set_rot(&mut self, r: i32);
              pub fn rotated_corners(&self) -> [Vec2; 4]; }
pub struct Mesh { pub base: ObjectBase, pub kind: MeshKind, pub geometry: Arc<MeshGeometry>,
                  pub texture: Option<Arc<Image>>, pub submeshes: Arc<[Submesh]>, pub bounds: MeshBounds,
                  pub norm_factor: f32, pub import_offset: Vec3 }
pub struct TextureEntry { pub label: Label, pub image: Arc<Image>, pub source: TextureSource }
pub struct Selection { pub list: IndexSet<ObjRef>, pub primary: Option<ObjRef>,
                       pub highlighted: IndexSet<ObjRef> }        // highlight is separate state (tools 8.1 quirk)
pub struct EditEnv { pub restrict_rotations: bool, pub render: RenderPrefs, pub code_page: u16 }
pub struct EditCtx<'a> { pub doc: &'a mut Document, pub sel: &'a mut Selection,
                         pub env: &'a EditEnv, pub rng: &'a mut EngineRng }
pub struct ElementMut<'a> { /* guard: setter semantics, propagation, journal */ }
impl ElementMut<'_> {
    pub fn set_position(&mut self, p: Vec3, propagate: bool); pub fn set_origin_move(&mut self, o: Vec3, propagate: bool);
    pub fn set_size(&mut self, s: Vec3); pub fn set_rotation(&mut self, r: Vec3, o: RotOpts) -> RotResult;
    pub fn default_uv(&mut self, force: bool);
}
bitflags! { pub struct Categories: u8 { /* 1, 2, 4, 8, 16, 32, 64; named combinations 12, 20, 36, 40, 52, 255 */ } }
pub struct Snapshot { pub categories: Categories, pub props: Option<Arc<ModelProps>>, pub view: Option<Arc<ViewProps>>,
    pub elements: Option<ElementsSnap>, pub textures: Option<Arc<TextureList>>,
    pub voxels: Option<Option<VoxelSnap>>, pub meshes: Option<MeshesSnap>, pub display: Option<Arc<DisplaySlots>> }
pub enum ApplyMode { Open, Merge, Restore(Categories), Paste { offset: Vec3 } }
pub enum ApplySource<'a> { Loaded(Box<LoadedDoc>), Snapshot(&'a Snapshot) }
pub fn apply_document(cx: &mut EditCtx<'_>, src: ApplySource<'_>, mode: ApplyMode) -> Result<ApplyReport, ApplyError>;
pub fn capture(doc: &Document, sel: &Selection, scope: CaptureScope) -> Snapshot;
pub fn check_invariants(doc: &Document, sel: &Selection) -> Result<(), InvariantError>;

// cuboid-scene ----------------------------------------------------------------------------------
pub enum ViewId { Top, Side, Front, Main }
pub struct Camera { pub pos: Vec3, pub rot: Quat, pub ortho: bool, pub ortho_size: f32, pub fov_deg: f32,
                    pub near: f32, pub far: f32 }
pub struct ViewSet { pub views: [View; 4], pub maximized: Option<ViewId>, pub pivot: Vec3,
    pub hovered: Option<ViewId>, pub pending: Option<ViewId>, pub active: ViewId, pub cinema: bool }
pub trait Raycaster {
    fn pick_model(&self, ray: Ray, max_len: f32) -> Option<Hit>;          // front faces only, hidden excluded
    fn pick_gizmo(&self, ray: Ray, view: ViewId, gizmo: &GizmoState) -> Option<GizmoHit>;
    fn drag_plane(&self, ray: Ray, through: Vec3, max_len: f32) -> Option<Vec3>;
}
pub struct Hit { pub target: HitTarget, pub point: Vec3, pub normal: Vec3, pub face: Option<FaceSlot>, pub uv: Option<Vec2> }
pub struct SceneIndex { /* BVH over element and mesh bounds, per-mesh triangle BVH, voxel collider snapshot */ }
impl SceneIndex {
    pub fn sync(&mut self, doc: &Document, journal: &ChangeJournal);
    pub fn freeze_voxel_colliders(&mut self); pub fn refresh_voxel_colliders(&mut self, world: &VoxelWorld);
}
pub struct ViewDrawList { pub key: FrameKey, pub voxel_ranges: [Vec<ChunkRange>; 6], pub items: Vec<DrawItem>,
                          pub overlays: Vec<OverlayItem>, pub outline: Vec<DrawItem> }
pub fn compile_draw_list(view: &View, cache: &SceneCache, prefs: &RenderPrefs) -> ViewDrawList;

// cuboid-io -------------------------------------------------------------------------------------
pub struct Registry { /* 23 entries in interop 4.1 order */ }
impl Registry { pub fn lookup(&self, path: &Path, mode: LookupMode, im: ImportMode) -> Lookup<'_>;
                pub fn dialog_entries(&self, caps: Caps, vis: &FormatVisibility) -> Vec<&FormatEntry>; }
pub trait Importer: Send + Sync {
    fn begin(&self, src: Source, cx: &ImportCx) -> Result<Box<dyn ImportSession>, IoError>;   // worker: read, parse
}
pub trait ImportSession: Send {
    fn prompt(&mut self) -> Option<Prompt>;                       // parent prompts, entity name with Retry, ...
    fn answer(&mut self, a: Answer) -> Step;                      // Continue | Stop(UserMessage)
    fn apply(self: Box<Self>, work: &mut EditCtx<'_>) -> Result<ImportOutcome, IoError>;  // worker, CoW copy
}
pub trait Exporter: Send + Sync { fn begin(&self, snap: Arc<DocSnapshot>, cx: &ExportCx) -> Box<dyn ExportSession>; }
pub trait ExportSession: Send {
    fn prompt(&mut self) -> Option<Prompt>;                       // pre-checks in spec order
    fn answer(&mut self, a: Answer) -> Step;
    fn build(&mut self, p: &mut dyn ProgressSink) -> Result<BuildStep, IoError>;   // worker
    fn post_build_prompt(&mut self) -> Option<Prompt>;
}
pub enum BuildStep { NeedRender(RenderRequest), Done(Artifacts) }   // a render is answered by the app (UI thread)
pub struct Artifacts { pub files: Vec<OutFile>, pub notices: Vec<Notice> }
pub struct OutFile { pub path: PathBuf, pub bytes: Vec<u8>, pub kind: OutKind /* Binary | Text | Png */ }
pub enum Profile { Cubik29, Modern(Target) }
pub enum ImportMode { Cubik29, Modern }
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()>;

// cuboid-core -----------------------------------------------------------------------------------
pub struct Core { /* doc, sel, views, hist, clip, start queue, interaction, preview, compat, clock, rng, status, dirty */ }
impl Core {
    pub fn execute(&mut self, cmd: Command) -> CmdOutcome;       // guard, push, mutate, post (10.1 order)
    pub fn undo(&mut self) -> CmdOutcome; pub fn redo(&mut self) -> CmdOutcome;
    pub fn tick(&mut self, input: &InputFrame, ray: &dyn Raycaster) -> TickOutput;    // frame + fixed steps
    pub fn export_snapshot(&self) -> Arc<DocSnapshot>;
    pub fn swap_document(&mut self, doc: Document, base: Rev, kind: SwapKind) -> Result<(), Stale>;
    pub fn query(&self) -> AppQuery;                             // enable states for menus and buttons
}
pub struct History<S> { /* entries, cursor, redo cache, redo count */ }
impl<S> History<S> {
    pub fn push(&mut self, label: Label, snap: S, cb: Option<UndoCallback>);
    pub fn undo(&mut self, capture_current: impl FnOnce(Categories) -> S) -> Option<&Entry<S>>;
    pub fn redo(&mut self) -> Option<&Entry<S>>;
    pub fn undo_label(&self) -> Option<&str>; pub fn redo_label(&self) -> Option<&str>; pub fn clear(&mut self);
}
pub struct Compat { /* section 17.2 fields */ }
impl Compat { pub fn port() -> Self; pub fn strict() -> Self; }
pub struct Clock { pub now: f64, pub frame_dt: f32 /* + fixed stepper, timers */ }
impl Clock { pub fn advance(&mut self, now: Instant) -> u32 /* fixed steps due */; pub fn next_deadline(&self) -> Option<Instant>; }
pub struct InputFrame { pub held: HeldKeys, pub downs: Vec<EngineKey>, pub ups: Vec<EngineKey>,
    pub pointer: Option<Vec2>, pub wheel: f32, pub over_ui: bool, pub text_focus: bool, pub view_rects: [Rect; 4] }
pub trait CustomTool {
    fn begin(&mut self, cx: &mut ToolCx<'_>, button: Button, hit: &Hit) -> StrokeStep;
    fn drag(&mut self, cx: &mut ToolCx<'_>, hit: Option<&Hit>) -> StrokeStep;
    fn end(&mut self, cx: &mut ToolCx<'_>);
}
pub enum CmdOutcome { Done(Effects), Refused(UserMessage), NeedsFlow(FlowRequest) }

// cuboid-render ---------------------------------------------------------------------------------
pub struct FrameKey { pub scene: u64, pub camera: u64, pub overlay: u64, pub prefs: u64, pub size: (u32, u32), pub anim: u32 }
pub struct Renderer { /* device-level resources, pools, atlas pages, shared attachments, 4 view targets */ }
impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, caps: &AdapterCaps) -> Self;
    pub fn sync(&mut self, queue: &wgpu::Queue, ex: &RenderExtract, budget: UploadBudget);
    pub fn render_views(&mut self, enc: &mut wgpu::CommandEncoder, lists: &[ViewDrawList]) -> [ViewTexture; 4];
    pub fn start_png_render(&mut self, enc: &mut wgpu::CommandEncoder, req: &RenderRequest) -> ReadbackId;
    pub fn poll_readbacks(&mut self, device: &wgpu::Device) -> Vec<(ReadbackId, RgbaImage)>;   // never blocks
}

// cuboid-ui-kit ---------------------------------------------------------------------------------
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }       // canvas px, y down
pub struct Hints { pub min: f32, pub preferred: f32, pub flexible: f32 }
pub fn stack(avail: f32, pad: (f32, f32), spacing: f32, expand: bool, items: &[Hints], align: Align) -> Vec<(f32, f32)>;
pub fn grid_columns(inner: f32, cell: f32, spacing: f32) -> u32;
pub fn anchored(parent: Rect, a: &Anchors) -> Rect;
pub fn nine_slice(dst: Rect, b: Borders) -> [Rect; 9];                 // with the squash rule
pub struct PointerRouter { /* targets registered in draw order, uGUI-style events */ }
impl PointerRouter { pub fn register(&mut self, id: WidgetId, rect: Rect, sense: Sense);
                     pub fn events(&self, id: WidgetId) -> PointerEvents; pub fn over_ui(&self) -> bool; }
pub struct Focus { /* one "selected" control, editing state, Tab order */ }
impl Focus { pub fn holds_keyboard(&self) -> bool; pub fn tab(&mut self, forward: bool); }
pub struct Skin { pub name: SkinName, pub colors: SkinColors, pub slots: SlotPictures }
pub struct StateColors { pub normal: Rgba, pub hover: Rgba, pub pressed: Rgba, pub disabled: Rgba, pub fade_s: f32 }
pub struct TextMetrics { /* calibrated advances and wrapping on epaint galleys */ }
pub struct TextField<'a> { /* own widget, 14.5 */ }
impl<'a> TextField<'a> { pub fn new(id: WidgetId, text: &'a mut String, kind: ContentKind) -> Self;
                         pub fn show(self, kit: &mut Kit<'_>, rect: Rect) -> FieldResponse; }

// cuboid-shell ----------------------------------------------------------------------------------
pub trait ShellApp {
    fn engine_key(&mut self, ev: EngineKeyEvent);                 // physical key + location, in arrival order
    fn window_event(&mut self, ev: ShellEvent);                   // focus, resize, drop, close request, scale
    fn frame(&mut self, cx: &mut FrameCx<'_>) -> FrameRequest;    // build UI, render views, return wake-up
}
pub struct FrameCx<'a> { pub egui: &'a egui::Context, pub gpu: &'a egui_wgpu::RenderState,   // device, queue, egui renderer
    pub ui_scale: f32, pub now: Instant }
pub struct FrameRequest { pub next_wake: Option<Instant>, pub cursor: CursorKind, pub title: Option<String> }
pub fn run(app: impl ShellApp + 'static, opts: ShellOptions) -> Result<(), ShellError>;
pub struct Headless { /* device without surface, egui renderer into an offscreen target */ }
impl Headless { pub fn new(size: (u32, u32), scale: f32) -> Result<Self, ShellError>;
                pub fn frame(&mut self, app: &mut dyn ShellApp, input: &[ScriptedInput]) -> RgbaImage; }

// cuboid-app ------------------------------------------------------------------------------------
pub struct FlowCx { /* handle to the executor and AppState */ }
impl FlowCx {
    pub async fn next_frame(&self); pub async fn fixed_step(&self); pub async fn seconds(&self, s: f32);
    pub async fn message_box(&self, spec: MessageBoxSpec) -> BoxAnswer;   // Button(i) | Close | Replaced
    pub async fn text_prompt(&self, spec: PromptSpec) -> Option<String>;
    pub async fn file_dialog(&self, req: FileRequest) -> Option<PathBuf>;  // built-in or native
    pub async fn job<T: Send + 'static>(&self, pool: Pool, f: impl FnOnce(&JobCx) -> T + Send + 'static) -> Result<T, JobError>;
    pub async fn io(&self, task: IoTask) -> Result<IoResult, std::io::Error>;
    pub async fn render(&self, req: RenderRequest) -> Result<RgbaImage, RenderError>;
    pub fn with<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> R;     // never held across an await
}
pub enum Pool { Foreground, Background }
pub struct JobHandle<T> { rx: std::sync::mpsc::Receiver<JobEvent<T>>, cancel: Arc<AtomicBool> }
```

---

## 7. Numerics (`cuboid-num`)

- Stored document values are f32 as in the original; every formula follows the operation order of the
  spec; no fused multiply-add, no reassociation.
- **Evaluation policy.** The specs state single-precision arithmetic. The original's runtime may evaluate
  single-precision expressions with double intermediates and round only at stores (**UNVERIFIED**, risk R1);
  results differ only at rounding ties (snapping at exact half steps, X9 text of computed values). Parity
  formulas are written once through small helpers whose evaluation width is the workspace constant `EVAL`
  (`Strict32` until golden vectors from the original decide otherwise). Switching touches one crate.
- **Number text.** `g7`, `x9` and `nj` (interop 3.1) generate digits from the exact binary value with an
  explicit rounding stage, so that a "round to 15 significant digits first" variant can be enabled if
  reference vectors show the original's formatter is not correctly rounded (**UNVERIFIED**, T-NUM-1). Tests:
  spec examples, NJ round trip of sampled f32 patterns in CI and of all 2^32 patterns in the nightly job,
  proptest against an arbitrary-precision reference.
- **Parsing.** `parse_or_zero_*` implements "try parse, 0 on failure" with the invariant culture (`.` only;
  the whole process is culture-invariant, app 1.2). The OBJ importer's thousands-separator acceptance and the
  permissive JSON dialect have their own token rules in `cuboid-io`.
- **Randomness.** `EngineRng` (xorshift128, seedable) is owned by `Core` and passed explicitly to every
  operation that draws random values (tools 16.5); production seeds from the clock.

---

## 8. Document model (`cuboid-model`, `cuboid-voxel`, `cuboid-image`)

### 8.1 Aggregate and ownership

- The document is flat (tools 2.1). Small aggregates sit behind `Arc` and change through `Arc::make_mut`, so a
  `Document` clone is a snapshot that costs one pointer copy per object and shares everything untouched
  (`Document: Send + Sync`).
- The UI thread is the only writer. Workers receive snapshots or owned buffers and return values; the UI
  thread applies them after checking the document revision.

### 8.2 Identity

- `ObjList<T>` keeps `ids`, `items` (`Arc<T>`), `display_ids` in list order, plus an `ObjId -> position` map
  rebuilt lazily after structural changes. List order is panel, file, export and render-sort tie order.
- `ObjId` is session-unique and never persisted. Records keep their `ObjId` through undo, redo and restore,
  so renderer caches and list rows stay keyed; paste, clone and import allocate fresh ids.
- Display ids follow tools 2.1: assigned from a per-kind counter at creation, reset to 0 when all elements
  are cleared (new model, load, restore) and re-issued in list order. They live in the parallel vector, so
  renumbering never copies a record.
- `ObjRef = Element | Mesh | Blueprint | ModelOrigin` is what selection, hover, highlight and gizmos refer
  to.

### 8.3 Objects

- Fields follow tools 3.1, 4.1, 4.4, 5.1, 6.1 and native 4. `UvRect` keeps raw (clamped) and folded values
  distinct; every consumer reads folded values. `Label` is an `Arc<str>` interned per document.
- All writes with Cubik semantics go through guards (`ElementMut`, `MeshMut`, `BlueprintMut`): position
  propagation, pivot-relative resize with the 0.001 clamp, rotation wrap / quantise / restricted rejection /
  pivot rotation, UV-snap triggers and turn-off rules. Paths that bypass the setters on purpose (transform
  dialogs, tools 10.13) are explicit functions and are excluded from the size invariant.
- Images are stored bottom row first; a texture read from `.cubik` keeps its format code and raw bytes
  (mip chains included) until edited; decoding to RGBA8 is lazy (`OnceLock`), so loading many textures
  decodes only those the atlas needs. `AlphaStats` answers "does this crop contain partial alpha" in O(1).
- Primitive geometry comes from `cuboid-ops::primitives`; its exact layout is pending spec work (S2), so
  the generators are parametric and flagged until calibrated.

### 8.4 Voxel world

- A dense slot table in the persistence order of native 4.7 (cx outermost, then cy, then cz), so saving walks
  memory linearly and parallel results merge in file order.
- `ChunkSlot::EmptyWritten` reproduces "a chunk whose voxels were erased one by one is still written". It
  survives neither load nor undo restore, because both re-write cells through the full writer into a fresh
  world (native 6.3, tools 7.10): restore converts it to `Unallocated`.
- `occ` (4096-bit occupancy) keeps mesher, hollow, fill and searches on bit operations. Batch writes group
  cells by chunk, so a brush stroke copies each touched chunk once. Every write dirties all chunks within one
  voxel (tools 7.5 port note).
- Whole-world operations compute output chunks in parallel from input chunks and commit the new table at
  the end, which makes them cancellable and leaves snapshots untouched.

### 8.5 Selection, highlight and interaction state

`Selection { list, primary, highlighted }` implements tools 8.1. The highlight set is separate because
select-all and select-by-name empty the list without un-highlighting (tools 8.1 quirk). Hover, active axis,
default axis, busy, grabbed and clone guard are interaction state in `cuboid-core::interact`.

### 8.6 Change journal and dirty flag

- Per object `ChangeFlags { TRANSFORM, GEOMETRY, FACES, UV, COLOR, VISIBILITY, ORDER, LABEL }`; per document
  `DocFlags { TEXTURES, ATLAS, VOXEL_CHUNKS(bitset), MODEL_BOX, DISPLAY, VIEW, SELECTION, HIGHLIGHT, LISTS }`.
  Drained once per frame by scene sync, list panels, the status bar and the title.
- The dirty flag follows the original literally: set by every undo push, cleared by a native save, new file
  or native open, untouched by undo, redo, exports and backups (tools 15.1, app 2.2). It is not derived from
  revisions, because edits without an undo step (element settings panel, keyboard nudges, model box drags)
  must not mark the document dirty.

### 8.7 Invariants

Checked in debug builds after every command, after every property-test step and by the fuzz harness: sizes
from the size setter at least 0.001; stored rotations on the 5 or 22.5 degree grid (with the documented
below -360 quirk); six faces per element; UV rotations in {0, 90, 180, 270}; raw UV corners in [-1, 2];
active-face count equals the face mask; display ids strictly increasing in creation order; palette starts
with 0 and holds distinct values; `nonzero` and `occ` agree with the cells; active box half-open and inside
the grid; selection refers to live objects only.

---

## 9. Apply path, undo and clipboards

### 9.1 One apply path

`apply_document(cx, source, mode)` implements native 6.2-6.3 for `Open`, `Merge`, `Restore(categories)` and
`Paste { offset }`; `capture(doc, sel, scope)` implements native 5.2-5.3 and the snapshot shapes of tools
15.2. Section order, rotation re-quantisation under the current restriction setting, size handling,
`is` re-selection, palette rebuild, display-id renumbering and the start queue are therefore identical in all
four places, as in the original, which reuses the document shape for undo and clipboard data.

- **Record normal form.** Snapshots have the document record shape (tools 15.2, native 12 item 14), so a
  restore or paste must yield what a record round trip yields: colour packing (native 4.10), UV clamp and
  fold, UV rotation normalisation, rotation re-quantisation, size rules. The fast path keeps an `Arc`
  untouched when the object is a fixed point of that round trip (a cheap check) and replaces it with the
  normalised copy otherwise; ObjIds survive, so the renderer re-uploads only changed records.
- **Voxels.** The fast path swaps the slot table, turns `EmptyWritten` into `Unallocated`, clears cells
  outside the current active box (copying only boundary chunks) and recomputes the palette in scan order
  (parallel per chunk, ordered merge).
- **Reference twin.** A literal implementation (capture to records, recreate every object through the
  setters, replay every voxel through the full writer) lives behind `cfg(test)` / feature `reference`;
  proptest compares both on random documents and histories (W1.6 acceptance).
- **Paste** applies the stored blueprint record and the offset rules of tools 11.3 and always deep-copies
  (tools 11.4 port decision; sharing `Arc`s is equivalent because shared records are immutable).

### 9.2 History (`cuboid-core::history`)

- Exact semantics of tools 15.1: push truncates at the cursor and clears the redo cache, keeps at most 11
  entries, marks the document dirty and refreshes the Edit menu; undo appends a current-state snapshot to the
  redo cache only when the cache is shorter than `length - cursor`; redo restores `redo_cache[redo_count]`
  after decrementing; undo and redo never touch the dirty flag; new model and load clear everything.
- `Categories` uses the original's bit values; the content of each category is owned by tools 15.2. The known
  capture gaps of tools 15.5 are reproduced by the category rules of the command table.
- A push costs O(objects) pointer copies (about 20 microseconds for 10 000 elements); memory grows only
  with what changes afterwards. History compression is not part of 1.0 (section 18 note).

### 9.3 Clipboards

All in-process, never the OS clipboard (app 14): object clipboard (snapshot subset plus copy anchor), voxel
clipboard (relative cells), face-property and shade clipboards, UV clipboard, mesh texture and colour
clipboards, colour-picker presets. New model and replace-open clear the ones the specs name.

### 9.4 Start queue

Elements created, pasted, loaded or restored run `default_uv(false)` once when they "start" (tools 4.7).
Keys queue during a command or tick and are processed before the next render, which gives the visible and
saved result of the original's next-frame start.

---

## 10. Commands, actions, input, time and interaction

### 10.1 Command layer (`cuboid-core`)

- `Command` is an enum grouped by domain. `Core::execute` runs the original's order: guard (selection kind,
  primary kind, voxels set up), undo push (label and categories, also where the spec says the entry is pushed
  even if nothing changes), mutation through `cuboid-ops`, post-processing (start queue, journal).
- A static `COMMAND_TABLE` (id, label template, category rule, guard) is tested against the label catalogue
  of tools 15.6 and the guards of menus 4.
- Commands that need confirmation or dialogs are flows (11.1) that call `execute` for the final step.
  `CmdOutcome` carries UI requests (message box, status text, panel refresh) as data; core never draws.

### 10.2 Actions and menus (`cuboid-actions`)

- `ActionId` covers every row of menus 2-3, every toolbar and panel button of menus 5.14 and ui 6-8, and the
  hidden bindings Delete (with confirmation), F2 and F3. `ACTIONS` stores the original action name (for
  traceability), handler group and kind; the three dead rows exist and do nothing.
- `MENUS` holds bar order, minimum widths, rows, flags A/U/L, dynamic rows and the enable model of menus
  1.6-1.7. A test diffs labels, shortcuts and flags against `parity/strings.toml`.
- Rows activated by a click or a shortcut run 0.05 s later (menus 1.5); toolbar buttons call actions
  directly.

### 10.3 Engine keys and dispatch

- `cuboid-shell` maps every winit `KeyEvent` (physical key code plus key location) and mouse button to an
  `EngineKey` named like the original's key codes, in arrival order, ignoring OS key repeat. Modifier sides
  come from the physical keys (`AltLeft` / `AltRight`, ...).
- Shortcut texts are canonicalised once (menus 6.1). The dispatcher processes each key-down separately in
  arrival order (the spec's recommendation), gated by the global modal flag, the keyboard-selection rule
  (unless works-in-UI) and menu enablement (unless always-active). The frame-based quirks of menus 6.2 (one
  key per frame, a mouse press cancels the frame) are a `Compat::strict()` mode for parity runs.
- The held-key map implements menus 6.4. The five customisable strings are parsed and canonicalised
  (menus 6.6) and applied at start-up as well (deliberate fix, switchable).
- Letters match by logical key (the character printed in the menus, so Ctrl+Z works wherever Z is printed);
  digits, keypad keys, modifiers, function and navigation keys match by physical key. How the original's
  engine maps keys on non-QWERTY layouts is **UNVERIFIED** (R9) and decides this rule once tested.

### 10.4 Time

- `Clock { now, frame_dt, fixed stepper (0.02 s, catch-up cap 1/3 s, both UNVERIFIED), timers }`.
- Timers in seconds: action deferral 0.05; colour fades 0.1; caret period 1/0.85; status "Ready" after
  10 whole seconds; list double click 0.25; file-browser double click 0.3; arrow auto-repeat after more than
  15 fixed steps; overlay alpha steps and the floor animation at 50 Hz; dimming layer
  `lerp(colour, target, 2 * dt)` per frame; backup interval `max(300, minutes * 60)`.
- The shell sleeps until the earliest of: input, the next timer, a running animation, a finished job or
  readback (`ControlFlow::WaitUntil`); redraws are capped at the display rate while focused and 10 Hz while
  unfocused, and happen only when something changed (app 4.5). Fixed steps catch up on the next frame.

### 10.5 Interaction state machine (`cuboid-core::interact`)

- A pure function of `InputFrame` (held bindings, edges, pointer, wheel, text-focus, pointer-over-UI, view
  rects) plus queries through `Raycaster`. It implements tools 8.3-9.6 (gizmo press and hover, mode
  resolution, dispatch, end-drag clean-up, restriction bake, drag plane, mouse drags, nudges, clone mode,
  box selection, model box handles) and app 5.4-5.5 (hover, pending and active view latching, pan, orbit,
  zoom, focus). Fixed-step work (hover pick, pending box apply, arrow-repeat counter, voxel rebuild
  scheduling) runs from the fixed stepper.
- Paint, colour-picker and voxel tools implement `CustomTool` with the stroke lifecycle of tools 12.2 and
  13.2: begin, Left Alt eyedropper, same-face gate, dedupe, plane pin, preview cache with its 4096-entry
  quirk, first-hit-ignored rule for textures.
- **Frozen voxel colliders.** Ray hits on voxels use a collider snapshot (`Arc` of the voxel world) that is
  refreshed when a rebuild pass completes, frozen when a stroke begins and refreshed when it ends
  (tools 7.6, 12.2). This decides where Dot, Sphere and Laser strokes land.

---

## 11. Flows, jobs and threads

### 11.1 Flows

Multi-step operations are `async fn`s polled on the UI thread by a small executor (wakers built with
`std::task::Wake`, no external runtime). They mirror the original's coroutines one to one: wait one frame,
wait one fixed step ("Loading..." box, interop 5.2), wait 0.1 s, show a box and continue with its answer.
State is borrowed only inside `FlowCx::with` closures; the clippy lint `await_holding_refcell_ref` is denied.

Flows: new file, open, merge, import, export, save, save as, backup, unsaved-changes prompt with
continuation, exit, recent files, drag and drop, command line, voxelisation, texture reload on focus gain,
PNG render, Sketchfab (M8). A message box shown while another is open replaces it; the replaced box's
waiter resolves as `Replaced` and that flow ends, as the original drops the replaced handlers.

### 11.2 Codec sessions

Importers and exporters are resumable step machines (6). A flow runs `begin` and `build` / `apply` on a
worker, shows each `Prompt` on the UI thread and resumes with the `Answer`; no worker ever waits for the UI.
Imports apply to a copy-on-write working document and are swapped in only on success and only if the base
revision is unchanged, which satisfies the specs' atomic-import requirement (native 6.4, 9.6) while the
visible prompts and statuses stay the original's. A PNG render requested by an exporter (model render,
resource-pack icon) is answered by the flow on the UI thread through the renderer.

### 11.3 Threads

| Thread | Work | Rules |
|---|---|---|
| UI thread | event loop, input, interaction, commands, flow executor, egui, scene sync, GPU encoding and submission | only writer of the document; never waits on another thread except bounded waits at shutdown |
| foreground pool (rayon, cores - 1) | chunk meshing, BVH builds and refits, large extractions, short kernels | latency-sensitive work only |
| background pool (rayon, `(cores - 1).clamp(2, 8)`, threads `cuboid-bg-N`; l6_release.md 3) | BSON encode and decode, conversions, voxelisation, hollow/fill, face optimisation, atlas creation, image decoding, PNG encoding | long work; cancellable |
| I/O thread (one, FIFO) | file reads and atomic writes, ZIP inflate/deflate streaming, directory scans, signature hashing | FIFO order means a backup and a save never write the same path at once |
| network (feature `net`, M8) | uploads and lookups | started only by user action or opt-in |

### 11.4 Rules that rule out deadlocks

1. No lock guards the document or any other shared mutable state; ownership, immutable `Arc` snapshots and
   channels only.
2. The UI thread never blocks on a job; flows poll `JobHandle`s with `try_recv` through their wakers.
3. A job never waits on another job of the same pool; the foreground pool never waits on the background pool.
4. The only locks are leaf locks (logger ring buffer, the crash snapshot slot taken with `try_lock`, the
   egui-wgpu renderer lock, taken on the UI thread only), never held while calling rayon, sending on a channel
   or calling other locking code.
5. GPU read-back is `map_async` plus non-blocking polling.
6. Panic isolation: each job runs under `catch_unwind`; a panic becomes an "Exception" box and a log entry,
   and the document is untouched because results were never applied.

### 11.5 Determinism under parallelism

Parallel algorithms that write voxels or create objects collect results per work unit and apply them on one
thread in the original scan order (voxelisation per `(i, j)` column, conversions in scan order, palettes per
chunk merged in slot order), so palette order, element order and display ids equal a sequential run. Key
kernels are tested with 1 and 8 threads.

---

## 12. File formats

### 12.1 Layers

1. **Codecs** turn bytes into format records and back (pure, fuzzed). For `.cubik` the records mirror native
   4 one to one and live in `cuboid-fmt-cubik::records`.
2. **Mapping** turns records into model objects with the read-side normalisations (native 6.3, interop per
   format) and objects into records with the save normalisations (native 5.3). It lives next to the codec.
3. **`cuboid-io`** owns the registry (interop 4), validation and pipeline texts (interop 5.2, 6.2-6.5),
   sessions, profiles and the atomic write helper; flows in the app own the sequence.

### 12.2 Registry and profiles

- 23 entries in interop 4.1 order (order is semantic), capabilities, labels, filters, message names and
  visibility rules (removed versus hidden, built-in-browser-only `.lnk`). Lookup removes every `.` and is
  case-sensitive in `ImportMode::Cubik29`, case-insensitive in `Modern`; `png` resolves to Sprite when
  opening and Model Render when saving.
- `Profile::{Cubik29 (default), Modern(target)}` per export, remembered per format; `ImportMode::{Cubik29
  (default), Modern}`. `[FIX-ALWAYS]` items are fixed without a switch, `[FIX-MODERN]` and `[IMPORT-MODE]`
  items belong to the profiles, never to `Compat`. The app identity string is configuration; golden tests set
  the reference identity (interop 2.8).

### 12.3 `.cubik`

- **Container.** Reads stored and deflated entries, data descriptors and ZIP64; finds `cubik` exactly, then
  case-insensitively; reports "not a ZIP", "no entry" and "corrupt entry" separately. Writes one `cubik`
  entry with deflate at the fastest level, streamed into a temporary file in the target folder and renamed.
  An optional `cuboid.json` entry carries port-only data (item overrides, profile memory) with its own
  version; a "Cubik-compatible save" setting omits it (native 8.4).
- **BSON (`cuboid-bson`).** Zero-copy reader yielding `(key, type, value span, element span)` with document
  sizes validated against their enclosing bounds; lenient getters of native 3.6; duplicate keys last-wins;
  quaternion members applied in document order with the `eulerAngles` override; limits (document size,
  depth 64, array lengths, texture `w * h` against data length, voxel resolution against a memory budget)
  checked before allocation. Writer with keys as `&'static [u8]` including the terminator, f32 widened with
  `f64::from`, enums and `v` as int32, size patching in one pass.
- **Schema tables.** Each record type is declared once in a `macro_rules!` table (key, BSON type,
  if-absent default, nullability, accessor) that generates the encoder (exact key order, native 3.3) and the
  decoder (any order, unknown elements collected), so the two cannot drift.
- **Fast paths.** Voxel records with the canonical 33-byte layout (native 3.5) and 27/38-byte vector
  documents are recognised by their fixed bytes and read at fixed offsets; anything else takes the generic
  path. Voxel chunks and element ranges are encoded and decoded in parallel into separate buffers and
  concatenated in file order.
- **Retention.** The decompressed payload is kept as one `Arc<[u8]>`; each section and each record keeps its
  span and the revision it had after loading, plus unknown elements as "raw element after known key k".
  Unchanged objects are written by copying their span; changed ones are re-encoded with unknown elements
  re-inserted. Camera states are compared with the loaded ones, so an unchanged `vp` is copied too.
  Retention is dropped above a memory cap (default 512 MiB) and can be switched off.
- **Texture write policy.** Retained textures are written unchanged; edited or created textures are written
  as RGBA32 (`f = 4`, native 4.5.1). A strict switch reproduces the original's ARGB32 atlas recut after paint
  strokes for file comparisons.
- **Stability.** In debug builds every written payload is decoded again with the port's own reader and
  compared (native 8.2 item 6).

### 12.4 Legacy `.cub` and the permissive JSON dialect

BOM-aware decoding to UTF-16 code units, XOR with the two published keys (native 9.3), then the dialect
parser of `cuboid-io::text::jsonlite`, shared with the `cubik29` Minecraft JSON importer: only `\uXXXX`
decoded, number tokens up to the next delimiter, first-letter literals, last-wins duplicates, fixed
"Invalid data found" message. Modern import mode uses serde_json. The import is atomic.

### 12.5 Text emitters (`cuboid-io::text`)

`McJson` (hand-built Minecraft writer: tabs, LF, the trailing-whitespace details, minify, `[FIX-ALWAYS]`
escaping), `NetJson` (the original serializer's layout: two spaces, CRLF, `": "`, compact primitive arrays,
NJ floats, omitted nulls, declaration order, compact mode), `Lines` (explicit LF or NL-as-CRLF per line on
every platform, interop 3.3). All text files are UTF-8 without BOM.

### 12.6 Per-format choices

| Format | `cubik29` implementation | Modern / import mode | Oracle tests |
|---|---|---|---|
| Minecraft JSON export / import | own emitter / own dialect parser | serde_json with version targets (M8) | serde_json validates output |
| Resource pack | zip writer, fixed texts, icon through a render step | per-version `pack.mcmeta` (M8) | zip reader lists entries |
| `.mcpe`, `.js`, `.java`, `.jpm`, entity atlas | own (tolerant JSON for `.mcpe`, NetJson for `.js` / `.jpm`) | Bedrock geo, JEM (M8) | serde_json parses outputs |
| OBJ import | own tokenizer, index rules, recursive triangulation, chunking | tobj | tobj reads our OBJ export |
| OBJ/MTL, STL export | own (G7, mixed line endings, header rule) | LF, weld as labelled | tobj, stl_io |
| PNG sprite import | own rectangle growth | same | rectangle counts |
| VOX | own flat chunk walk, own writer | dot_vox scene graph (M8) | dot_vox reads our VOX |
| QB, QEF, binvox, ZOX, CSV | own | own | QB re-import, serde_json for ZOX |
| schematic, NBT | fastnbt + flate2, exact tag order, root name | Sponge, modern structures (M8) | fastnbt reads ours |
| Stonehearth, Staxel | own (QB + NetJson; Staxel stub) | Staxel hidden | - |
| `.lnk` | own subset parser (interop 28) | full parser | T-LNK-1 |

Every importer has a fuzz target; errors map to the original's titles and lead sentences with the port's own
detail text (interop 5.4). Outputs are deterministic: ordered containers, fixed archive entry order,
injected RNG for jitter, packers whose result depends only on input order.

---

## 13. Rendering (`cuboid-render`, `cuboid-scene`)

### 13.1 Frame and composition

1. Scene sync drains the journal: changed instance ranges, finished chunk meshes and texture dirty
   rectangles upload within a per-frame budget (default 8 MiB or 4 ms; leftovers carry over).
2. For each visible view whose `FrameKey` changed: render into the shared attachments (MSAA colour when
   "Enable anti-aliasing" is on, `Depth32Float` with reversed Z, highlight mask), resolve, then run a
   full-view composite pass (outline of app 8.7, alpha forced to 1) into the view's own `Rgba8Unorm`
   texture. The pass runs for every rendered view, highlighted objects or not, because blending also blends
   alpha and egui composites with alpha. Views render one after another, so one attachment set (sized to the
   largest view in 64-pixel buckets) serves all four; only the final textures are per view.
3. The egui pass draws the UI; each quadrant is an egui image of its view texture (registered once with
   `register_native_texture`, re-bound on resize), nearest filtering, rect snapped to physical pixels,
   texel-to-pixel ratio 1. With dithering off and alpha 1 the gamma-framebuffer shader leaves the pixels
   unchanged (fact F4), and view frames and labels draw on top in the right order.
4. The surface uses a non-sRGB format (egui's preference); the shell asserts it and logs otherwise.

A frame where only UI changed (tooltip, hover fade) costs egui's pass plus four textured quads. Hidden views
(another view maximised) are not rendered.

### 13.2 Look constants

`look.rs` gathers every visual constant with its spec reference: clear `#343434` (transparent for PNG
renders), light directions and intensities, flat ambient, the per-vertex lighting formula (app 8.3),
tint-emulation colours (app 8.4), highlight colours, grid weights and line colours, faint-box alpha 5/255,
axis colours, preview tints. The pipeline is gamma space: textures as `Rgba8Unorm`, no sRGB views, blending on
stored 8-bit values, blend factors `SrcAlpha, OneMinusSrcAlpha` for colour and alpha (this reproduces the
`a^2` alpha of PNG renders, app 15). Depth comparisons are mirrored for reversed Z (near 0.3, far 128,
Main 256, app 5.1).

### 13.3 Elements

- One instance of 44 words (176 bytes) per element: position (3), flags (view mask, highlight, inverted
  normals, hidden-render mode), size (3), slot, rotation quaternion (4), per-face atlas-space `tl, br`
  already clamped to the entry rectangle (24; per-corner clamping of tools 4.6 commutes with the corner
  rotation), per-face vertex colour RGBA8 (6), per-face state bytes (2: active, UV rotation, atlas page).
- Non-indexed draw with 36 vertices per instance and no vertex buffer; corner and triangle tables
  `(3, 1, 0), (3, 2, 1)` of tools 4.3 in constant memory; inactive faces collapse to degenerate triangles;
  lighting per vertex (app 8.3).
- An edit rewrites one record; toggling "Emulate elements tint colors" or "Show elements color" rewrites only
  the colour words.

### 13.4 Transparency: exact batching

- The original draws every element, mesh, preview model, sub-grid plane and background plane in one queue
  sorted back to front (distance to the bounding-box centre; along the view axis for orthographic cameras),
  each object with a depth pre-pass (alpha at least 0.5) followed by a blended colour pass with "less or
  equal" (app 8.3, 8.6).
- An object is **binary** when its vertex alpha is 1 and every texel it can sample has alpha 0 or 255
  (checked with `AlphaStats` on conservative face crops, cached per record and texture revision). For a
  binary object the two passes equal one alpha-tested, depth-writing, blended pass. A run of consecutive
  binary objects in sorted order is drawn as one instanced draw without changing any pixel (within one draw
  the GPU keeps primitive order for depth and blending). Translucent objects break runs and are drawn exactly
  as the original, pre-pass then colour pass; consecutive translucent elements with pairwise disjoint screen
  rectangles share a pass pair. Additive sub-grid planes stay in sequence.
- Cost: `runs + 2 x translucent groups` draws per view; one draw in the common case without partial alpha.
  Ties keep list order (**UNVERIFIED** tie rule, D-P8).
- Proof obligation: random scenes rendered batched and literally (two draws per object in sorted order) are
  bit-identical on the same adapter (W3.4).
- Hoisting binary objects ahead of farther translucent ones (separating-plane or disjoint-rectangle test)
  is a documented M7 optimisation, admissible only with the same proof.

### 13.5 Voxels

- **Face records.** The canonical mesher (tools 7.6: face emission, vertex order, triangle split
  `(q, q+1, q+2), (q, q+2, q+3)`, AO neighbours and factors) emits one 8-byte record per exposed face:
  word 0 = local x, y, z (4 bits each) and chunk x, y, z (6 bits each); word 1 = RGB (24 bits) and the AO code
  of the four vertices (2 bits each). The direction is implied by the pool. The vertex shader rebuilds the
  quad from the same tables, so colour interpolation, AO gradients and the wireframe diagonal equal the
  original's unmerged quads. Exporters expand the same records on the CPU (`expand_face`), so viewport,
  wireframe and the chunk-mesh exporters share one source.
- **Pools and culling.** Six GPU buffers (one per direction) with a range per allocated chunk. Per view and
  chunk a direction is drawn only if a face plane of that direction can face the camera and the chunk
  intersects the frustum; the three axis-aligned orthographic views skip five of six directions.
- **Submission tiers.** Tier A (`INDIRECT_EXECUTION` plus either `INDIRECT_FIRST_INSTANCE` for
  instance-rate records or `VERTEX_STORAGE` for vertex pulling; W3.5 picks by benchmark): one
  `multi_draw_indirect` per pool over the visible chunk ranges. Tier B (GL or missing capabilities): one draw
  per pool over its used extent, GPU back-face culling only.
- **Jobs.** Dirty chunks are meshed on the foreground pool from pointer clones of the chunk and its 26
  neighbours; results carry the chunk revision and are dropped if stale; uploads apply in slot order within
  the budget. A single brush stroke is meshed and uploaded in the same frame.
- **Shading.** Lambert approximation of app 8.5 in the alpha-tested queue before the transparent queue;
  optional two-cascade soft shadows of the main light (late WP, tier P3).
- No greedy or "flat" merging in the viewport; greedy boxes exist only for the export modes that the specs
  define (tools 14.3, interop 16 pending S3).

### 13.6 Meshes, atlas and overlays

- Meshes: per-mesh vertex and 32-bit index buffers, one draw per sub-mesh, model matrix per draw, same
  lighting and transparency rules; primitives share generated geometry; the Cubik vertex limit is enforced
  only by the `.cubik` writer.
- Runtime atlas: emulated, not copied. etagere allocation with 1-pixel transparent padding, nearest
  filtering, clamp, incremental insertion (no repack), page array when one page is not enough. Its layout is
  never persisted (tools 5.2). The document-visible width-only downscale of oversized images uses a fixed
  slot limit of 8192 (a 16384 reference device), independent of the local GPU (D-P7). Paint strokes write the
  entry's CPU image and upload dirty rectangles; the square brush is clipped to its entry (D-P6).
- Overlays (app 6.3-8.10): analytic sub-grid shader (additive 0.124, line width `step / 256`, a fade that
  imitates mip-mapping, **UNVERIFIED** pixels), 1-pixel main grid lines and guides, background planes
  (unlit, nearest, culled after a side swap), centre marker, north pointer, blueprint, origin markers, voxel
  pointer plate, hidden-element boxes, selection wireframes for at most four objects, gizmos in the overlay
  queue without depth, outline (mask pass alpha-tested at 0.5, four diagonal taps at (+-2, +-2), per-channel
  maximum colour and summed alpha, cut-out, `mix(scene, rgb, a)`).
- Wireframe mode: `POLYGON_MODE_LINE` where available, barycentric edge shader otherwise.
- Display preview: the anchor transform is a per-view model-matrix uniform, so the 50 Hz floor animation
  and first-person anchors update one uniform; the player rig is instanced boxes with the skin texture
  (geometry pending S2).

### 13.7 Picking (`cuboid-scene`)

- Elements and meshes: BVH over world bounds (binned build, refit on transform changes, rebuild on
  structural changes); exact ray tests against active-face triangles, front faces only (**UNVERIFIED**
  single-sidedness of the original's physics queries), hidden objects excluded; ray lengths 1000 (hover),
  100 (gizmos), 200 (drag plane); meshes test a per-geometry triangle BVH shared through the geometry `Arc`.
- Voxels: DDA over the collider snapshot (10.5), returning the point and the face normal of the first
  exposed face entered, which equals a single-sided mesh-collider hit.
- Gizmos: analytic hit volumes mirroring the original's colliders (spheres of radius 0.3 at arrow tips, thin
  boxes for plane handles, rings), not the visual meshes; dimensions pending S8.

### 13.8 PNG render

Offscreen target of `W * aa x H * aa` (512 x 512 GUI preview or 1920 x 1080 Main, 128 x 128 for `pack.png`),
cleared to transparent (the port defines the clear; the original relied on the initial contents of a fresh
target), Main far plane 128, content rules of app 15, "Show elements color" off for the render, 2 x 2 box
downscale (half-pixel offset **UNVERIFIED**), straight alpha in the PNG. Read-back is `map_async` polled each
frame; downscale and PNG encoding run on the background pool.

### 13.9 Backends and failure handling

D3D12, Vulkan and Metal first, GL as last resort. Device loss and uncaptured errors are logged; on device
loss the app writes a crash backup and offers a restart (**UNVERIFIED** whether the renderer can be rebuilt in
place).

---

## 14. UI (`cuboid-ui-kit`, `cuboid-ui`, `cuboid-shell`)

### 14.1 Shell

- winit 0.30 event loop (`ApplicationHandler`), egui-winit `State` for egui input, IME and text clipboard,
  egui-wgpu `winit::Painter` for surfaces and the egui pass, one wgpu device shared with the renderer.
- UI scale `S = clamp(floor(max(96, dpi) / 96 * 2) / 2, 1, 1.99)` with dpi from the winit scale factor times
  96, plus a user override (ui 1.2); egui's pixels-per-point is set to `S`, so one egui point is one canvas
  pixel. egui's own keyboard zoom is disabled.
- Window rules in physical pixels: first launch 1000 x 700, minimum 900 x 700 times `S`, position restore
  with the 0 and 30 000 guards (app 1.2, 4.2). The Left Alt bypass at start-up is best effort (winit reports
  key state only through events) plus a `--reset-window` flag (**UNVERIFIED** equivalence).
- Cursors (own art), file drops, focus events (texture reload check, frame-rate policy), Alt+Enter and F11
  borderless full screen, title text.
- `Headless` renders full app frames into an offscreen texture with scripted input through the same
  `ShellApp::frame` path; UI snapshot and replay tests use it.

### 14.2 Layout rules (`cuboid-ui-kit::layout`)

- Pure functions for the rules of ui 0.4: anchored rectangles, stacks (min, preferred, flexible and the three
  cases), grids (the column formula), default contributions (picture size, single-line text size, 9-slice
  border sums) and the 9-slice squash. Floats throughout; frame edges snapped to physical pixels with one
  rule (round half away from zero, ui 0.2).
- The window geometry is computed from window size, layout state (zone widths, docking side, visible panels,
  edit mode, maximise, cinema) and text metrics, cached, and recomputed only when one of them changes. View
  rectangles are therefore known before the UI is built, so input reaches the views without a frame of lag.
- Geometry tests reproduce every worked example and checklist value of the specs (status cells at 900, 1000
  and 1280 px, tool grids at 180/220/300, panel distributions, menu panels, dialog button rows, ui 17 items).
  No general RectTransform engine, content-size fitter or aspect fitter is built (ui 0.4 states it is not
  needed).

### 14.3 Pointer events and focus (`cuboid-ui-kit::events`, `focus`)

- Every raycast target (frames, panels, dialogs, widgets) registers its rectangle in draw order each frame;
  the topmost target under the pointer (from the previous frame's list, as in any immediate-mode GUI)
  receives enter, exit, down, up, click (down and up on the same target), begin-drag after a threshold,
  drag, end-drag and scroll, bubbling to the nearest handler. This is the uGUI behaviour the specs describe
  (ui 0.3); egui supplies raw pointer input and painting only.
- `over_ui()` is true unless the topmost target is the transparent 4-view zone; it gates view hover, wheel
  zoom and picking (app 5.4). Entering any dialog clears the view hover (menus 7.3).
- One "selected" control per window (ui 16.1): text fields, dialog buttons and the specified checkboxes take
  selection on click and keep their hover colour; a click on a non-selectable control clears it. The
  dispatcher reads `holds_keyboard()`. Tab and Shift+Tab follow the visual reading order (ui 16.6); arrow
  navigation, Submit and Cancel use the original's event-system values (drag threshold, repeat delay and
  rate are **UNVERIFIED** until added to the UI spec).

### 14.4 Skins, sprites and painting

- `Skin` with the Light, Dark and Rainbow tables of ui 12.2 and slot pictures of ui 12.3; colours are resolved
  at draw time, so a skin switch is complete (ui 12.5 port decision); the `NoSkin` text rule is a per-node
  flag.
- Sprites keyed by the picture names the specs use, own look-alike art with the same sizes, borders and roles
  (ui 13, 15), packed into one egui texture at start-up (bilinear for chrome, nearest for palettes and texture
  previews), greyscale copies for disabled icons. Optional skin packs: a user-prepared folder of PNGs named by
  picture name, size-validated, never bundled (ui 15.1).
- Colour-state model of ui 0.5 (texel x base x state x multiplier, 0.1 s linear cross-fade, priority
  disabled > pressed > hover > normal), tween state stored per widget id.

### 14.5 Text

- Open Sans Regular, Bold and Italic (OFL) shipped; egui's default fonts are off. Layout uses a
  `TextMetrics` model on epaint galleys (11 px, line height 15, word wrap that drops lines that do not fit,
  ui 14); integer versus fractional advances and kerning are calibration options decided by captures
  (**UNVERIFIED**). Glyph pixels are tier P3; text placement follows the layout model, so a rasteriser
  difference never moves other elements.
- **Own text field** on egui's galley layout and input events (fact F3): select-all when a click focuses the
  field, word selection with the separators of ui 16.6, 1-pixel caret with period 1/0.85 s, selection colour
  `fieldFocus` at 50 %, horizontal scrolling, content kinds (text, integer, decimal, alphanumeric, name,
  e-mail), length limits, live change notification on every edit, wheel +-1 on numeric fields without focus,
  IME and clipboard through egui-winit events, multi-line mode for dialogs.

### 14.6 Window composition and data flow

- Regions and draw order of ui 2 (app area, status bar, menu bar, dialogs in the fixed stacking order of
  menus 7.6, popups, file browser, overlay); side zones with grips, docking commands and panel sets per edit
  mode; tool strips; ten manager panels and two helpers; the 4-view zone (labels, maximise, side swap
  requests, cinema bars); the status bar (seven cells, Info timing); tooltip and tool cursor overlay.
- Frame data flow: `logic` (poll jobs and readbacks, run flows, deferred actions, fixed steps, interaction,
  commands) then `ui` (build the whole UI from immutable state, collect intents: actions, commands, field
  edits; apply intents; drain the journal; extract render changes; render dirty views). The UI never mutates
  state while widgets are built, which keeps every mutation on the command path (undo, dirty flag, journal).
- Menus implement menus 1 (engaged bar, deferred open and close except the engaged hover switch, child
  placement and flips with the original's mix of `S` and raw dpi scale and its row-height estimate, outside
  press closing, instant row hover, disabled grey and greyscale icons, the recent-files rebuild quirk).
- Dialogs share one framework (menus 7, ui 11): chrome, drag strip with 16-unit overhang, resize grips with
  growth limits, re-centring with the original's integer formula on every show, default focus, Enter and
  Escape on key release, the single global modal flag (last writer wins) and the dimming layer. Each dialog
  is a module with its node geometry, a view model filled on show and apply logic on OK, so behaviour is
  testable without rendering. The message box implements the size estimate of menus 8.2, the handler mapping
  of menus 8.4 and replacement semantics.
- The built-in file browser is a work package of its own (menus 10): listing (folders first, then natural
  order; hidden entries at 55 % opacity), filters, selection and activation timing, preview pane with cached
  decoding, prompts, locations (Recent at most 5, favourites, defaults), path aliases, keyboard rules, drives
  per OS, import of the original's stored locations string. Folder scans run on the I/O thread.
- Lists are virtualised (uniform rows, 28 px wheel step); formatted strings are cached by revision; status
  counts are maintained from the journal.

### 14.7 Strings

All user-visible texts live in `cuboid-ui::strings` with their spec reference, typos included ("occured",
"Successfuly", "dowscale", ...); `{App}` and brand substitutions are applied there and nowhere else. A test
compares the catalogue with `parity/strings.toml`. No localisation in 1.0; the catalogue is the hook.

---

## 15. Settings, persistence and platform

- `cuboid-settings::Settings` holds every key of app 10.2 with the original names and value formats, so
  imported values round-trip, plus a `cuboid` section: zone widths and docking side (written on grip
  release), window rectangle, scale override, skin pack path, profile memory, custom key strings, file
  browser locations, recent files (identified by full path), `Compat` overrides. First-run defaults and
  missing-key defaults differ where menus 9.1 says so (S11 completes the null-value effects).
- Storage: `<config>/settings.json` (pretty JSON, `"version": 1`, unknown keys preserved, written atomically
  on Preferences OK and on quit). Data folder: `backup/<Weekday>_backup.cubik` (English weekday of the local
  date), `presets/*.json`, `skin.png`, `palette.png`, `logs/`. Cache folder: export staging (resource pack,
  Sketchfab), cleaned at start-up. Folders from `directories` with the port's own names; a `cuboid.portable`
  marker next to the executable switches everything to the executable's folder. The environment variable
  `CUBOID_HOME` names one folder that holds both (`config/` and `data/` below it) and wins over the marker and
  the user's folders; the tests of `cuboid mcp` set it so that they never write the user's log.
- First-run migration (app 10.1, 10.4, menus 10.10): read the original's `cubikdata.json` from its per-user
  folder and, on Windows, its stored preference values (locations string, favourites marker, Minecraft path
  override) through `cuboid-app::platform`; offer to copy its presets folder. Never write to the original's
  locations.
- Platform module (`cuboid-app::platform`): registry access through windows-registry (ANSI code page for
  signatures, Steam path, legacy values, explicit per-user file association action, deviation D4), folder
  detection of app 17, opener calls. The code page number reaches `cuboid-image` through `EditEnv`, so the
  image crate stays platform-independent (Windows-1252 elsewhere, overridable).
- Atomic writes (temporary file in the same folder, then rename) for documents, backups, settings and
  presets (D8).

---

## 16. Errors, logging and crash safety

- Typed library errors (`BsonError`, `ContainerError { NotZip, NoEntry, Corrupt }`, `CodecError`,
  `IoError`, `ApplyError`, `RenderError`, ...). User-facing errors map to the original's titles and lead
  sentences through `UserMessage` values from the strings catalogue; detail texts are the port's own.
- Parsers never panic: sizes checked, arithmetic on sizes checked, recursion bounded, slices bounds-checked,
  fuzzed.
- Logging: `log` facade with an own sink in the app (ring buffer of the last 2000 lines for a debug window,
  rotating file in `<data>/logs/`, 3 files of 5 MiB). `--debug` or `CUBOID_LOG=debug` raises the level
  (replaces the original's marker-file mode, app 1.4). Log lines the specs quote are emitted with the same
  text for diagnostics. Tokens and file contents are never logged.
- Start-up measurement: `--trace-startup` or `CUBOID_TRACE_STARTUP=1` logs one line per start-up phase (Info,
  milliseconds since the first instant of `main`) and echoes it on stderr. `CUBOID_EXIT_AFTER=<seconds>` is read
  only while that trace is on: it quits that long after the first full frame through the close button's path, so
  unsaved work still gets its question, and logs the fire at warn. Both are for scripted measurements
  (docs/design/l6_release.md 3).
- Crash safety: the UI thread keeps a "last snapshot" (pointer copies, refreshed when the revision changes)
  in a slot used only by the panic hook, which takes it with `try_lock` (a panic while it is held skips the
  backup instead of deadlocking), logs the panic and asks the I/O thread to write
  `backup/crash_<timestamp>.cubik`, waiting a bounded time; if the I/O thread is gone it writes directly.
  No recovery prompt is shown, matching app 12.2 (extension D-P9).

---

## 17. Compatibility switches and deviations

### 17.1 Rules

- Every QUIRK of the specifications is reproduced unconditionally inside the one semantic path.
- A typed switch exists only where a specification or this document names a decision. All switches live in
  `cuboid_core::Compat` with two constructors: `Compat::port()` (the shipped default) and `Compat::strict()`
  (parity runs whose reference comes from the original). Each field documents its spec reference and has a
  test for both values; `parity/compat.toml` mirrors the table and a test asserts equality.
- Profile matters (`[FIX-MODERN]`, `[IMPORT-MODE]`) belong to the format profiles, never to `Compat`.

### 17.2 Switches

| Field | `port()` | `strict()` | Reference |
|---|---|---|---|
| `dnd_unsaved_prompt` | on | off | app 0.2 D6 |
| `recent_files_without_settings` | working | dead | app 0.2 D7 |
| `custom_keys_at_startup` | applied | defaults until Preferences OK | menus 6.6 |
| `event_ordered_shortcuts` | on | frame-based quirks | menus 6.2 |
| `escape_only_visible_box` | on | hidden-box handlers run | menus 8.5 |
| `focus_poll_every_frame` | on | 50 Hz polling | app 5.5(f) |
| `model_properties_restrict_defect` | fixed | reproduced (after its runtime check, tools 18 item 4) | tools 9.5 |
| `atlas_recut_argb32` | off | on | 12.3 texture write policy |
| `abort_cancels_long_ops` | owner decision (off until signed off) | off | D-P3 |
| `strict_transparency` (diagnostic) | off | off | 13.4 proof switch |

Always on, not switchable: atomic imports and atomic writes (native 6.4, 9.6, D8), never adopting the path of
a failed load (native 6.1), deep copy on paste and restore (tools 11.4), the remeshing port notes of the specs
(every write dirties all chunks within one voxel, tools 7.5; model box changes remesh every affected chunk;
"Crop voxels outside model box" raises the world-dirty flag, tools 7.8), no network without action or opt-in
(D1, D2, D3, D5), association only by installer or explicit action (D4).

### 17.3 Design deviations needing owner sign-off

| Id | Original | Port | Reason |
|---|---|---|---|
| D-P1 | saves synchronously, clears the dirty flag | asynchronous save; the flag is cleared only if nothing changed after the snapshot | non-blocking UI; identical when the user waits |
| D-P3 | Abort in the Progress dialog does nothing (menus 9.11) | Abort cancels the running long operation | long work is cancellable off the UI thread |
| D-P6 | square brush can paint across atlas entries | clipped to its entry | depends on the original's packing layout, which cannot be reproduced |
| D-P7 | width-only downscale uses the device texture limit | fixed reference limit (slot 8192) | document independent of the GPU |
| D-P8 | equal sort keys resolve in the engine's order | list order | the engine's tie order is unknown |
| D-P9 | no crash backup | `backup/crash_<timestamp>.cubik` from the panic hook | crash safety, no prompt |

### 17.4 Recorded deviations after M0

Tooling and test-support decisions taken while building M0 that differ from the text of sections 3, 4, 5
and 19. They change no behaviour of the program and need no owner sign-off.

| Id | Architecture said | Now | Reason |
|---|---|---|---|
| M0-1 | `cuboid-testkit` depends on model, io, fmt-* and image (3.3) | it depends on no workspace crate (external crates only: image, image-compare, ron, serde, sha2, thiserror); canonical dumps are written by the callers from their own types through `cuboid_testkit::dump::Dump` | The kit is a dev-dependency of crates of every layer. A dependency back on them forms a dev-dependency cycle: Cargo compiles the crate under test a second time for the kit, and the types the kit sees differ from the ones the test uses. |
| M0-2 | canonical dumps are insta snapshots (4.3, 19.3) | plain golden files (`payload.dump.txt`) in `tests/golden`, compared by `Golden::expect_text` or `diff::dumps` (which takes the ignored paths of a semantic round trip) and blessed with `CUBOID_BLESS=1` (`cargo xtask bless`); insta stays in `[workspace.dependencies]` and in the dev-dependencies of `cuboid-io` and the `cuboid-fmt-*` crates for snapshots of emitted text formats | One harness, one bless flow and one refusal in CI cover bytes, text, numbers and rasters by tier. The dump is the port's own format (LF, final newline), and insta cannot ignore paths. |
| M0-3 | tests that need an oracle report "skipped (oracle absent)" (19.1) | an oracle-gated test also carries `#[ignore = "oracle <Ox>: <env var>"]`, for example `#[ignore = "oracle O1: CUBOID_PARITY_REFS"]`, next to its `oracle::require` guard; maintainers run it with `--include-ignored` (nextest: `--run-ignored all`) | libtest cannot mark a running test as skipped, and a test that returns early counts as passed. The attribute makes the report show the test as ignored, and the guard still prints the notice and returns early when the variable is unset. |
| M0-4 | 4.1 and 4.3 list no `ron`, `sha2` or `toml` | declared in `[workspace.dependencies]`: `ron` 0.12.2 (scenario files `tests/scenarios/*.ron`, 19.3), `sha2` 0.11.1 (SHA-256 of binary values in canonical dumps, native 10.2) and `toml` 1.1.8 (ledger files; features `parse` and `serde`, default features off). `cuboid-testkit` uses the first two, `xtask` the third. | Versions checked with `cargo search` on 2026-10-18. Hoisted so that versions stay in one place (21.1 rule 2). |
| M0-5 | `rust-toolchain.toml`: channel 1.95.0, components rustfmt and clippy (3.1) | also `profile = "minimal"` | `rustup toolchain install` in CI and in a new checkout would otherwise add the components of the default profile (documentation among them) that no job uses. Both components stay listed. |
| M0-6 | opener 0.9.0 "reveal files" (4.1) | declared with the cargo feature `reveal` | Selecting a file in the file manager is behind an opt-in feature of opener 0.9.0, which pulls zbus on Linux and two `windows-sys` features on Windows. |

---

## 18. Performance and memory budgets

Reference machines: R1 desktop (6-8 cores, mid-range discrete GPU of the GTX 1660 / RX 6600 class,
1920 x 1080); R2 laptop (4 cores, integrated GPU of the Iris Xe / Radeon 680M class), targets at half the R1
frame rate. Scenes: S1 = 10 000 elements, random sizes, 20 % rotated, two 16 x 16 binary textures, 5 %
translucent; S2 = R 128 world, 256^3-cell extent, about 4 M voxels, about 2 M exposed faces, AO on; S3 = S1 and
S2 together; S4 = 256^3 checkerboard stress scene (must stay stable, may be slow). All values are targets
(**UNVERIFIED** until W7.1 measures them).

| Scenario (R1) | Target |
|---|---|
| Idle, focused, nothing animating | 0 redraws per second, under 1 % CPU |
| Orbit Main view, 4-up, S1, AA off / on | at least 60 fps; CPU at most 5 ms, GPU at most 8 ms |
| S2 in four views | at least 60 fps on R1, 30 fps on R2; face records about 16 MiB; full meshing at most 1.5 s on 8 threads |
| Drag one element among 10 000 | one-frame latency; update at most 0.5 ms CPU |
| Drag a selection of 10 000 elements | at most 16 ms per frame (1.7 MiB instance rewrite) |
| Single voxel edit | remesh and upload at most 2 ms |
| Sphere brush, one stroke frame | at most 4 ms CPU |
| Open a 150 MB payload (S2 saved) | interactive within 2 s (BSON decode at least 300 MB/s) |
| Save the same | UI blocked at most 30 ms (snapshot); total at most 2 s off-thread |
| Open or save a 10 000-element file | at most 150 ms |
| Undo push | under 1 ms typical, 20 ms worst case |
| Undo or redo restore, S1 or S2 | at most 50 ms (fast path) |
| Hover pick | at most 0.3 ms |
| Face optimisation over 10 000 elements | at most 1 s |
| Voxelisation at R 128 of 100 000 triangles | at most 5 s |
| Memory, S3 plus 16 MiB textures and 11 undo entries | at most 1 GiB resident |
| Time to first frame | at most 0.5 s after start, excluding driver initialisation |

Unit costs: element record about 0.5 KiB plus 176 bytes of instance data; allocated voxel chunk 16.5 KiB;
voxel face 8 bytes on the GPU; image 4 bytes per texel on the CPU plus its atlas copy.

Enforcement: criterion benchmarks per crate (BSON coding of S2 voxels, chunk meshing, picking, undo push and
restore, draw-list compilation, atlas insertion) and a headless frame benchmark; CI compares against stored
baselines on a fixed runner and fails on regressions above 10 %. A debug overlay (`--debug`) shows stage
times, GPU timestamps when `TIMESTAMP_QUERY` exists, and memory counters.

Measured-need optimisations kept out of 1.0 unless W7.1 shows a budget failure: lz4 compression of
history-only chunks and images, transparency hoisting (13.4), copy-on-write image tiles for very large
painted textures, the `fast-deflate` backend.

---

## 19. Testing and parity tracking

### 19.1 Parity tiers and oracles

| Tier | Meaning | Check |
|---|---|---|
| P0 bit-exact | identical bytes or f32 bit patterns: `.cubik` payloads, `cubik29` exports, number text, exact arithmetic paths | golden bytes |
| P1 geometric | identical integers, rectangles within 1/64 px: UI rects, hit areas, tool footprints, selection sets | conformance tests |
| P2 raster | per channel at most 2/255 on non-text pixels, at most 0.1 % mismatching edge pixels | image diff with masks |
| P3 perceptual | SSIM at least 0.98 (to be calibrated): text glyphs, lit 3D, MSAA edges, shadows | image diff |
| P4 behavioural | same sequence of dialogs, labels, statuses, undo labels, selection, dirty flag | scenario tests |

Exceptions to P0, recorded in the ledger: transcendental paths (rotation about a pivot, Euler decomposition,
camera quaternions; tolerance 1e-4 degrees or 1e-5 relative, native 4.3.2) and packed atlases (sampled
pixels, interop 30.1).

Oracles: O1 the original program on Windows (maintainers only; captures stored outside the repository); O2
private research data (UI hierarchy dump, sprites) read only by local tests through `CUBOID_PRIVATE_UI_DUMP`,
`CUBOID_PRIVATE_SPRITES` and `CUBOID_PARITY_REFS`; O3 independent crates (bson, serde_json, dot_vox, tobj,
fastnbt, stl_io, gltf, image); O4 vectors printed in the specifications; O5 in-repository reference twins.
Tests that need O1 or O2 report "skipped (oracle absent)" instead of passing. The correspondence table that
maps cuboid's widget ids to entries of the private dump lives in the private tree, not in the repository.

### 19.2 Ledger

`parity/ledger/<wp-id>.toml` files (one per work package, so parallel agents never edit the same file) list
every testable requirement with a stable id derived from the spec section (`native.4.4.3.fold`,
`tools.3.4.restricted-reject`, `ui.4.status-widths`), its tier, status (`todo`, `implemented`, `tested`,
`verified-o1`, `unverified-spec`) and covering tests. `cargo xtask ledger` fails when a `tested` entry names a
test that `cargo test -- --list` does not know, and prints coverage per spec document. The ledger is the
shared progress board and the definition of "100 %".

### 19.3 Test layers

| Layer | What | Gate |
|---|---|---|
| Unit | numerics, BSON primitives, XOR, dialect parser, setters, every op, layout rules | CI |
| Spec vectors | every byte example and table of the specs | CI |
| Golden files | own fixtures: file, decompressed payload, canonical dump (insta), summary | CI |
| Round trip | payload identity; semantic round trip with listed normalisations; proptest documents | CI |
| Differential | reference twins against optimised paths; O3 oracles read our outputs; random BSON through both readers | CI |
| Fuzzing | every importer, container, BSON, dialects, `.lnk`, DXT, settings and preset JSON, locations text; corpus replay on stable | nightly 1 h per target, CI replay |
| Scenarios | scripted command and input sequences run headless against `Core` (testkit DSL, `tests/scenarios/*.ron`), covering tools 17, app 20, menus and dialog flows | CI |
| Input replay | recorded raw input traces through `Headless` | CI |
| UI snapshots | headless frames at 1000 x 700 and 1280 x 720, scales 1.0 and 1.5, Light and Dark, each panel set and dialog, own art, tier masks; kittest for widgets | CI on WARP and lavapipe |
| Layout conformance | every rect against spec tables (CI) and the private dump (O2, local) | CI / release checklist |
| Render | white-cube face values 245 / 219 / 184, tint emulation, transparency cases, outline, grid fade, voxel AO, batched == literal | CI on software adapters |
| Reference captures | O1 screenshots, exports and files compared by tier; text calibration | ledger `verified-o1` |
| Cross-application | native 10.3 and interop 30 procedures with Cubik installed | release checklist |
| Performance | section 18 benchmarks | weekly job, release |
| Static | rustfmt, clippy `-D warnings`, cargo-deny, ledger, strings and compat table checks, layering lint | every change |

CI matrix: Windows (primary), Linux and macOS for build, unit, golden, scenario and replay tests; GPU tests on
Windows and Linux software adapters with per-backend approved images where rasterisation differs.

---

## 20. Packaging, licensing and clean room

### 20.1 Packaging

cargo-packager: Windows installer (per-user, optional `.cubik` association with cuboid's own ProgID and
icon), portable ZIP (portable marker), macOS app bundle and disk image (document type for `.cubik`), Linux
AppImage and `.deb` (desktop entry, MIME type). Release builds use `--locked`, the pinned toolchain and fixed
archive timestamps; cargo-about generates the third-party notices shipped with the Open Sans licence and the
asset licence. Assets are embedded with `include_bytes!`; no network at start-up, no telemetry, licence or
update requests (D1, D5). Signing and notarisation are pipeline steps with secrets outside the repository.

### 20.2 Licensing

Code: MIT OR Apache-2.0 (all dependencies are permissive; the MPL-2.0 helper of `directories` is file-level
and allowed). Own art and sounds: CC0. Open Sans: SIL Open Font Licence 1.1. Data tables: the legacy
Minecraft id table is factual and regenerated from public data; palette colour tables follow the owner's
decision (interop 33.3, S7).

### 20.3 Clean-room rules

- Implementers read `docs/spec` and `docs/design` only. Specification writers may read the private research
  tree; trace files mapping specs to research sources stay private; no code, data table or generated code
  comes from research data; private oracles run only on maintainers' machines; questions go back to the spec
  writers and answers land in `docs/spec`.
- Blockbench (GPL-3.0) is a learning reference only: no code, shaders, icons, themes or text are copied or
  translated.
- Cubik Studio material: none of its assets, scene data, logos or code; functional UI strings only where
  parity requires them, with brand substitution. Its pictures are used only from a user's own installation as
  an optional local skin pack.

---

## 21. Implementation plan

### 21.1 Rules for parallel work

1. **Contracts first.** W0.2 creates every cross-crate type and trait as stubs. A crate's public API changes
   afterwards only through its owning package or the milestone's integration role.
2. **Disjoint ownership.** Each package owns the listed paths exclusively. W0.1 pre-creates every crate
   manifest, module file, fuzz target entry and `[workspace.dependencies]` line, so later packages never
   edit shared manifests. The workspace `Cargo.toml`, CI files and `parity/compat.toml` belong to the
   integration role of the running milestone.
3. **Definition of done.** Code, tests, the package's ledger file at `tested`, clippy and deny clean on three
   OSes, no new **UNVERIFIED** without a ledger entry.
4. **Mocks.** Renderer and UI tracks work against synthetic scenes and a fake `Core` until M6; interaction
   works against a mock `Raycaster` until W3.2 lands.
5. **Spec questions** are filed against the spec documents, never resolved by guessing. A package blocked by
   a pending spec section (21.2) implements everything else and leaves the gap as `unverified-spec`.

### 21.2 Pending spec sections (owned by the spec team)

| Id | Missing specification | Blocks |
|---|---|---|
| S1 | texture management: entry model, texture and material popups, label rules, image loading per format, external editor round trip, reload from disk, mesh texture commands | W2.5, W6.6 |
| S2 | geometry of built-in meshes: primitives, sprite extrusion, player rig and skin UVs, markers, sky box | W2.8, W3.7 |
| S3 | OBJ export of voxels in the texture modes (palette and atlas meshers) | W4.5 |
| S4 | rotation restriction of primitive meshes | W1.5 (meshes), W2.3 |
| S5 | UV editor view (zoom, pan, nudges, double-click rectangle, write-back) | W6.4 |
| S6 | palette-conversion texture layout | W2.7 |
| S7 | shipped data tables and palettes with their licensing decision | W4.8, W6.6 |
| S8 | gizmo meshes, hit volumes and materials | W3.2, W3.6 |
| S9 | OBJ material and texture names, hidden elements in OBJ | W4.5 |
| S10 | tool toggle groups and their auto-off rules | W2.9, W6.6 |
| S11 | settings load with missing or null keys | W1.10 |
| S12 | status bar cell semantics (Face, X, Y, Z, Rotation) | W5.6 |
| S13 | display preview angle helpers, initial tab, interaction lock | W6.5 |
| S14 | default texture payloads of a fresh model (byte identity of fresh saves) | W1.7 goldens |
| S15 | colour palette panel: preset palette sizes, palette loading and reduction, swatch mapping, colour field rules | W6.6 |

### 21.3 Milestones and work packages

Dependencies list packages that must have landed (contract stubs from W0.2 are always available).

**M0 Foundations**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W0.1 Workspace and CI | root `Cargo.toml`, `rust-toolchain.toml`, `.cargo/`, `deny.toml`, `about.toml`, CI workflows, all `crates/*/Cargo.toml` and empty module files, `xtask/` skeleton with the layering lint, `fuzz/Cargo.toml` | - | build, test, clippy `-D warnings` and `cargo deny check` green on Windows, Linux, macOS; every crate root forbids unsafe code; layering lint passes |
| W0.2 Contracts | public stubs (`contract.rs` / `lib.rs` items) of model, voxel, image, io, scene, core, actions, render, ui-kit, shell | W0.1 | compiles; every public item documented with its spec reference; owner API review signed off |
| W0.3 Numerics | `crates/cuboid-num/**` | W0.1 | spec vectors of interop 3.1, tools 1.6, 3.4, 4.5, native 4.3.2 (1e-4 degrees), 4.10; NJ sampled round trip in CI, exhaustive nightly; both `Eval` policies tested |
| W0.4 Shell spike | `crates/cuboid-shell/**` | W0.2 | synthetic winit events distinguish keypad from digit keys and left from right modifiers; a test-pattern view texture composited through egui reads back bit-exact; first window 1000 x 700 at scale 1; minimum 900 x 700 x S; headless frame capture on WARP and lavapipe |
| W0.5 Test kit and ledger | `crates/cuboid-testkit/**`, `parity/**`, `xtask/src/ledger.rs` | W0.1 | ledger check in CI; P0-P4 diff helpers unit-tested; oracle-absent tests report skipped |

**M1 Data core and native format (parallel)**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W1.1 BSON | `crates/cuboid-bson/**`, `fuzz/fuzz_targets/bson.rs` | W0.1 | native 3.5 vectors byte-exact; agreement with bson 3.1.0 on 10^5 random documents in both directions; 1 h fuzz clean; decode at least 300 MB/s recorded |
| W1.2 Images | `crates/cuboid-image/**`, `fuzz/fuzz_targets/{image,dxt}.rs` | W0.3 | every format code of native 4.5.1 decodes and re-encodes; DDS bottom-up rule; signature vectors for code pages 1252 and 1258; raster footprints of tools 13.2; alpha statistics |
| W1.3 Voxel storage | `crates/cuboid-voxel/src/{world,chunk,palette,writers,mapping}.rs` | W0.3 | tools 7.2 mappings, 7.5 writer matrix, persistence flags including "erased chunk still written" and "not after load or restore" |
| W1.4 Voxel mesher and DDA | `crates/cuboid-voxel/src/{mesh,dda}.rs` | W1.3 | face records equal the tools 7.6 tables for all six faces and AO cases; CPU expansion goldens; DDA hit point and normal; dense 4096-chunk meshing benchmark recorded |
| W1.5 Model objects and setters | `crates/cuboid-model/src/{ids,list,objects,setters,uv,journal,selection,invariants}.rs` | W1.2, W1.3, S4 (mesh rotation only) | tools 17 items 1, 2, 3, 6, 7; `ObjList` operations; invariant checker |
| W1.6 Apply and capture | `crates/cuboid-model/src/{apply,capture,snapshot,normal_form,reference}.rs` | W1.5 | native 6.2-6.3 rules as tests; restore and paste fast paths equal the record-shape reference twin (proptest, 10^4 cases) |
| W1.7 `.cubik` and `.cub` | `crates/cuboid-fmt-cubik/**`, `fuzz/fuzz_targets/{cubik,cub}.rs` | W1.1, W1.6, W1.8, S14 (fresh-save goldens only) | native 10.1 tests; payload identity on generated documents; semantic round trip with listed normalisations; distinct container errors; 150 MB payload opens within budget |
| W1.8 I/O core and text | `crates/cuboid-io/**`, `fuzz/fuzz_targets/{jsonlite,mcpe_json,lnk}.rs` | W0.3 | native 9.4 and interop 8.6 accept/reject tables; NetJson and line-writer goldens; interop 4.3-4.4 lookup and filter tables; interop 5-6 message texts; T-LNK-1 |
| W1.9 Actions and keys | `crates/cuboid-actions/**` | W0.2 | menus 6.1-6.7 tests; 58 + 3 bindings; menu tables equal `parity/strings.toml` rows |
| W1.10 Settings | `crates/cuboid-settings/**` | W0.1, S11 | app 10.2 keys round-trip; first-run and missing-key defaults (menus 9.1, S11); recent-file rules (app 11, full-path identity); atomic writes |
| W1.11 CLI | `crates/cuboid-cli/**` | W1.7 | `dump` format of native 10.2; `roundtrip` reports payload identity; `diff` |

**M2 Headless editing core (parallel by domain)**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W2.1 Core, history, commands | `crates/cuboid-core/src/{core,history,commands,clipboard,start,compat,clock,status,query}.rs` | W1.6 | tools 15.1-15.6 (11-entry cap, redo cache, labels); history against a naive deep-copy model (proptest); command table equals tools 15.6 and menus 4 |
| W2.2 Element ops | `crates/cuboid-ops/src/elements/**` | W1.5 | tools 10.1-10.10; tools 17 item 17; grid-accelerated face optimisation equals the O(n^2) reference |
| W2.3 Transform ops | `crates/cuboid-ops/src/transform/**` | W1.5, S4 | tools 17 items 4, 5; tools 10.11-10.13 |
| W2.4 UV ops | `crates/cuboid-ops/src/uv/**` | W1.5 | tools 17 items 6, 8; tools 10.17 |
| W2.5 Texture and atlas ops | `crates/cuboid-ops/src/textures/**` | W1.2, W1.5, S1 | tools 5.4-5.5 atlas creation and crops; S1 operations once specified |
| W2.6 Voxel ops | `crates/cuboid-ops/src/voxel/**`, `crates/cuboid-voxel/src/{ops,search}.rs` | W1.3 | tools 17 items 14, 15; tools 12.3-12.5, 7.7-7.9 |
| W2.7 Conversions | `crates/cuboid-ops/src/convert/**` | W2.6, W1.5, S6 | tools 17 item 16; voxelisation parallel equals sequential |
| W2.8 Mesh, primitives, paint | `crates/cuboid-ops/src/{mesh,primitives,paint}/**` | W1.2, W1.5, S2 | tools 17 item 18; tools 6.3, 10.14, 13; primitive generators `unverified-spec` until S2 |
| W2.9 Interaction and tools | `crates/cuboid-core/src/{interact,tools}/**` | W2.1, S10 | tools 17 items 9-11 as scenarios on a mock raycaster; stroke lifecycle with frozen colliders; nudge repeat timing; app 5.4-5.5 navigation |

**M3 Scene and renderer (parallel; starts after W0.4 and W1.5)**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W3.1 Views and cameras | `crates/cuboid-scene/src/{views,camera,navigation,preview}.rs` | W0.3 | app 20 "Views and cameras" items; app 9.1-9.4 anchors and slot transforms |
| W3.2 Picking and gizmos | `crates/cuboid-scene/src/{pick,bvh,colliders,gizmo}.rs` | W1.4, W1.5, S8 | picking equals brute force; single-sided and hidden rules; frozen-snapshot semantics; gizmo placement of tools 9.1 |
| W3.3 Render core | `crates/cuboid-render/src/{gpu,targets,look,composite}.rs`, `crates/cuboid-render/shaders/core.wgsl` | W0.4 | white cube 245 / 219 / 184 within 1/255 (app 8.3); unchanged views not re-rendered; view textures opaque |
| W3.4 Elements, meshes, atlas, transparency | `crates/cuboid-render/src/{elements,meshes,atlas,sort}.rs`, `crates/cuboid-scene/src/{drawlist,atlas_layout,extract}.rs` | W3.3, W3.2 | batched equals literal per-object rendering bit for bit on 1000 random scenes (same adapter); S1 budget |
| W3.5 Voxel rendering | `crates/cuboid-render/src/voxels.rs`, `shaders/voxel.wgsl` | W3.3, W1.4 | GPU expansion equals CPU canonical quads on goldens; both tiers; S2 budget |
| W3.6 Overlays and outline | `crates/cuboid-render/src/{grid,overlays,outline,wireframe,backgrounds}.rs` | W3.3, S8 (gizmo meshes) | own goldens per overlay; outline construction of app 8.7 |
| W3.7 PNG render and preview scenes | `crates/cuboid-render/src/{offscreen,readback,preview,shadows}.rs`, `assets/preview/**` | W3.4, W3.1, S2 | app 15 sizes, content, `a^2` alpha, transparent clear; non-blocking read-back; preview scenes of app 9.5 |

**M4 Interop formats, `cubik29` profile (parallel by crate; starts after W1.6 and W1.8)**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W4.1 Minecraft JSON export and presets | `crates/cuboid-fmt-minecraft/src/{json_export,presets}/**` | - | T-JSON-1, T-NUM-1 |
| W4.2 Minecraft JSON import | `crates/cuboid-fmt-minecraft/src/json_import/**` | - | T-JSON-2 |
| W4.3 Resource pack | `crates/cuboid-fmt-minecraft/src/resource_pack/**` | W4.1 | T-RP-1 with a stub render answer |
| W4.4 Entity formats | `crates/cuboid-fmt-minecraft/src/{mcpe,bedrock_js,java,jpm,entity_atlas}/**` | - | T-MCPE-1, T-JS-1, T-JAVA-1, T-JPM-1 |
| W4.5 OBJ, MTL, STL | `crates/cuboid-fmt-mesh/src/{obj_import,obj_export,stl}/**` | W1.4, S3, S9 | T-OBJ-1..3, T-STL-1; tobj and stl_io read our outputs |
| W4.6 PNG sprite and render driver | `crates/cuboid-fmt-mesh/src/{png_sprite,png_render}.rs` | - | T-PNG-1 |
| W4.7 Voxel formats A | `crates/cuboid-fmt-voxel/src/{vox,qb,qef,binvox}/**` | - | T-VOX-1/2, T-QB-1, T-QEF-1, T-BINVOX-1; dot_vox reads our VOX |
| W4.8 Voxel formats B and tables | `crates/cuboid-fmt-voxel/src/{schematic,nbt,zox,csv,stonehearth,staxel,tables}/**`, `assets/data/**` | S7 | T-SCHEM-1, T-NBT-1, T-ZOX-1, T-CSV-1, T-SH-1 |

**M5 UI kit and main window (parallel; starts after W0.4)**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W5.1 Layout rules | `crates/cuboid-ui-kit/src/layout/**` | W0.2 | every worked example of ui 0.4, 2.1, 4, 6.1, 8.1 |
| W5.2 Skins, sprites, painting | `crates/cuboid-ui-kit/src/{skin,sprites,paint,tween}/**`, `assets/ui/frames/**` | W0.2 | ui 12.2 tables; ui 12.6 effective colours; 9-slice squash |
| W5.3 Events and widgets | `crates/cuboid-ui-kit/src/{events,widgets}/**` | W5.1, W5.2 | ui 16 behaviours through kittest; uGUI event order (click, drag threshold, bubbling) |
| W5.4 Text and focus | `crates/cuboid-ui-kit/src/{text,focus}/**` | W5.1 | ui 14 overflow rule; ui 16.6 field behaviours; keyboard-selection gating of menus 6.7; Tab order |
| W5.5 Menus | `crates/cuboid-ui/src/menus/**` | W1.9, W5.3 | menus 1 mechanics as scenarios; placement and flip formulas |
| W5.6 Main window | `crates/cuboid-ui/src/{window,zones,strips,viewzone,status,tooltip,cursor}/**` | W5.1, W5.3, S12 | ui 17 items 1-7 automated against spec tables |
| W5.7 Icons, cursors, sounds | `assets/ui/{icons,cursors}/**`, `assets/sounds/**` | - | ui 15.2 inventory complete with sizes and roles; menus 8.3 sound set |

**M6 Integration: dialogs, panels, application (needs M2, M3, M5)**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W6.1 Dialog framework | `crates/cuboid-ui/src/dialogs/{framework,message_box,prompts,progress,about,color_picker}/**` | W5.3, W5.4 | menus 7-8 (placement formula, modal flag, Enter and Escape on release, size estimate, handler mapping, replacement) |
| W6.2 Preferences | `crates/cuboid-ui/src/dialogs/preferences/**` | W6.1, W1.10 | app 10.2-10.3 every row; path-existence rule; canonical key strings |
| W6.3 Model and tool dialogs | `crates/cuboid-ui/src/dialogs/{model_properties,grid,voxel_setup,voxelization,png_import,faces_textures}/**` | W6.1 | menus 9.2-9.7; app 6.2 Grid quirks |
| W6.4 Element Settings | `crates/cuboid-ui/src/helpers/element_settings/**` | W6.1, S5 | ui 9.1; tools 10.15-10.17 |
| W6.5 Display Settings | `crates/cuboid-ui/src/helpers/display_settings/**` | W6.1, W3.7, S13 | app 9 checklist; presets of app 9.8 |
| W6.6 Manager panels and popups | `crates/cuboid-ui/src/{panels,popups}/**` | W5.6, W2.2-W2.8, S1, S7, S10, S15 | ui 8; menus 3 rows identical to tables; shared exclusive tool group |
| W6.7 File browser | `crates/cuboid-ui/src/file_browser/**` | W6.1 | menus 10 behaviours; legacy locations import |
| W6.8 App and flows | `crates/cuboid-app/src/{app,executor,flows,jobs,io_thread,backup,lifecycle,crash,render_glue}/**` | all of M1-M5, W6.1 | app 20 start-up, shut-down and files items as headless scenarios; UI-thread watchdog test (no frame above 50 ms during save, open and voxelisation) |
| W6.9 Platform, audio, logging | `crates/cuboid-app/src/{platform,audio,logging}/**`, `crates/cuboid-app/src/main.rs` | W1.10 | app 17 folder detection; code page detection; association action; log rotation; sounds at volume 0.4 |

**M7 Hardening and release 1.0**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W7.1 Performance and memory | `benches/**`, `xtask/src/bench.rs`, tuning changes coordinated with crate owners | M6 | section 18 targets met on R1 and R2 or budgets revised with data; measured-need optimisations only with their equivalence proofs |
| W7.2 Fuzzing and crash safety | `fuzz/corpus/**`, `tests/crash/**` | M6 | 24 CPU-hours per target clean; interrupted saves leave the previous file intact |
| W7.3 Packaging | `packaging/**`, `xtask/src/{package,licences}.rs` | M6 | installers built and smoke-tested on three OSes; notices complete; association action works per user |
| W7.4 Parity campaign | `parity/ledger/**` updates, private conformance runs (outside the repository) | M6 | ledger fully `tested`; O1 entries `verified-o1`; layout conformance with no mismatches on the main window; text calibration decided |
| W7.5 Documentation | `docs/user/**`, `CONTRIBUTING.md`, parity report generated from the ledger | M6 | user guide, contributor guide with clean-room rules, parity report |

**M8 Modern profiles (after 1.0)**

| WP | Owns | Deps | Acceptance |
|---|---|---|---|
| W8.1 Modern Java models | `crates/cuboid-fmt-minecraft/src/modern/**` | W4.1-W4.3 | interop 31.2-31.4 with their UNVERIFIED points resolved against official data first |
| W8.2 Bedrock and OptiFine | `crates/cuboid-fmt-minecraft/src/{geo,jem}/**` | W4.4 | interop 31.5-31.6 |
| W8.3 Modern voxel | `crates/cuboid-fmt-voxel/src/modern/**` | W4.7, W4.8 | interop 31.8 |
| W8.4 glTF / GLB | `crates/cuboid-fmt-mesh/src/gltf/**` | W4.5 | interop 31.9; gltf crate reads our output |
| W8.5 Network features | `crates/cuboid-app/src/net/**` | W6.8 | interop 31.10, app 16 and 18.2 (Sketchfab v3, opt-in skin lookup and update check, OS credential store) |

### 21.4 Milestone gates

| Gate | Exit criteria |
|---|---|
| M0 | CI green on three OSes; key-identity and exact-composition tests of the shell pass; numerics vectors pass |
| M1 | native 10.1 tests and payload identity pass; fuzzers clean for 1 h each; load and save budgets met |
| M2 | tools 17 items 1-18 pass as tests or scenarios; reference twins green; undo catalogue equals tools 15.6 |
| M3 | render goldens stable on two software adapters; face brightness table matches; batched equals literal; S1 and S2 budgets met on R1 |
| M4 | every T-* test of interop 30 passes on own fixtures; O3 oracles accept all outputs |
| M5 | ui 17 items 1-7 pass automatically; geometry tests pass for every spec table |
| M6 | app 20 checklist complete except items marked `unverified-spec`; strings test green; every dialog reachable through scenarios |
| M7 | ledger fully `tested`; O1 campaign run; installers smoke-tested on three OSes |

### 21.5 Critical path and tracks

Critical path: W0.1 -> W0.2 -> W1.5 -> W1.6 -> W2.1 -> W2.9 -> W6.8 -> W7.4. Track A (formats: W1.1, W1.7,
W1.8, M4), track B (editing core: W1.5, W1.6, M2), track C (scene and renderer: W0.4, M3) and track D (UI:
W5.x, then M6 dialogs and panels) run in parallel after M0 and join in W6.8. From the end of M0 about ten
packages can run at the same time (for example W1.1, W1.2, W1.3, W1.8, W1.9, W1.10, W3.1, W3.3, W5.1, W5.2
and W5.7); M4's eight packages are independent of each other except W4.3.

---

## 22. Risks and UNVERIFIED assumptions

| # | Risk or assumption | Mitigation |
|---|---|---|
| R1 | the original's float evaluation width and number formatting differ from the specs' f32 assumption | one numerics crate with a switchable policy and an explicit rounding stage; O1 vectors (T-NUM-1, snapping ties) decide |
| R2 | text metrics or glyph rasterisation differ | calibrated metrics model; glyphs judged at P3; calibration in W7.4 |
| R3 | built-in mesh geometry, gizmo colliders and texture management are not yet specified (S1, S2, S8) | parametric placeholders, ledger flags, spec work before the blocked packages close |
| R4 | packed atlases of the original cannot be reproduced | specs define parity by sampled pixels; deterministic packers |
| R5 | reference samples from the original arrive late | synthetic fixtures first; `unverified-spec` statuses; native 11 sample list drives the request |
| R6 | GPU and driver rasterisation differences | tier masks; software adapters in CI; per-backend approved images |
| R7 | transparency tie-breaking or batching assumptions wrong | strict per-object switch; equivalence tests; O1 captures of intersecting translucent elements |
| R8 | egui API changes | all egui use behind ui-kit painters and the shell; pin 0.37 for 1.0 |
| R9 | key mapping on non-QWERTY layouts | test the original on AZERTY and QWERTZ; until then letters match by logical key, everything else by physical key (10.3) |
| R10 | Left Alt start-up bypass needs key state before the first event | best-effort detection plus `--reset-window` |
| R11 | renderer rebuild after device loss | crash backup and restart prompt |
| R12 | provenance of palette values | owner decision (interop 33.3); regenerate from public data if reuse is rejected |
| R13 | a frozen-collider or start-queue subtlety is missed | scenario tests per tool with O1 traces in W7.4 |
| R14 | UI scope (about 1300 images and 900 texts in the original window) | spec-table geometry tests; art in parallel (W5.7); private conformance runner |

Further UNVERIFIED design assumptions: fixed step 0.02 s and catch-up cap 1/3 s (10.4); reversed-Z depth in
the original (13.2, affects only coplanar z-fighting); front-face-only physics queries (13.7); event-system
drag threshold, repeat delay and rate (14.3); integer versus fractional text advances (14.5); slot limit 8192
for the document-visible downscale (13.6); ANSI decoding of signature bytes that the original runtime decodes
differently (15); sub-grid pixels under the analytic fade (13.6); PNG downscale half-pixel offset (13.8); all
performance targets (18); whether egui_kittest can host native view textures (the headless shell is the
fallback and the primary full-window path).

---

## 23. Open decisions for the owner

1. Approve MIT OR Apache-2.0 for code and CC0 for own assets.
2. Approve the own shell instead of eframe (fact F1).
3. Approve the texture write policy (RGBA32 for edited textures; strict switch for the ARGB32 recut).
4. Sign off the recommended spec deviations D6, D7, D8 as `Compat::port()` defaults and the design
   deviations D-P1, D-P3 and D-P6 to D-P9.
5. Decide the source of the Minecraft legacy palette and the VOX default palette (S7).
6. Decide whether "Abort cancels" (D-P3) ships in 1.0.
