# cuboid architecture proposal: performance-first

Status: design proposal, 2026-10-18. Audience: maintainers and the agents that implement cuboid.

Angle: highest frame rate, lowest memory and fastest open/save of very large models, without giving up
any of the behavioural parity that the functional specifications define.

Normative inputs, cited with a prefix and a section number:

| Prefix | Document |
|---|---|
| N§ | `docs/spec/formats_native.md` |
| I§ | `docs/spec/formats_interop.md` |
| M§ | `docs/spec/model_and_tools.md` |
| L§ | `docs/spec/ui_layout_theme.md` |
| D§ | `docs/spec/ui_dialogs_menus.md` |
| A§ | `docs/spec/app_behavior.md` |

Precedence: the specs define *what* cuboid does; this document defines *how*. A performance technique
described here is admissible only if it is observationally equivalent to the specified behaviour (with
the equivalence argued here and covered by a test), or if it is listed as a deviation in section 18,
which needs owner sign-off.

Markers: MUST / SHOULD / MAY as in RFC 2119. **UNVERIFIED** marks anything not confirmed by compiling,
benchmarking or reading the relevant source. Every performance number in this document is a target,
not a measurement, unless it says otherwise.

---

## 0. Summary

- **Workspace**: 23 library crates and the application crate in 8 layers, plus a test-support crate,
  two tools and a fuzz crate. Pure logic (numbers, codecs,
  model, voxels, history, ops, scene preparation) has no GPU or UI dependency and is tested headless.
  Only `cuboid-render` and the app touch wgpu; only `cuboid-ui-kit`, `cuboid-ui` and the app touch egui.
- **Data model**: the document is flat (as in Cubik): ordered lists of `Arc` records with
  copy-on-write, a texture list of `Arc` images, and a voxel world stored as a dense table of `Arc`
  16x16x16 chunks. An undo snapshot is a set of pointer copies; a mutation copies only the records or
  chunks it touches. This makes Cubik's "snapshot the whole category before every edit" model cheap.
- **Undo/redo**: exact Cubik semantics (11 entries, redo cache, labels, categories, quirks) on
  structurally shared snapshots; history-only chunks and images are compressed in the background.
- **.cubik codec**: purpose-built BSON reader/writer (exact key order, f32 to f64 widening, size
  patching), schema tables defined once for both directions, canonical fast paths for 33-byte voxel
  records and 38-byte vector documents, parallel encode/decode per voxel chunk, deflate level 1
  streamed to a temporary file and renamed. Untouched data is re-emitted from retained raw spans, so a
  load/save cycle is byte-identical at payload level.
- **Renderer**: each of the four views renders offscreen (wgpu, depth, optional MSAA 4x) into an
  `Rgba8Unorm` texture shown by egui; only views whose inputs changed are re-rendered. Elements are one
  instanced draw (176-byte instance, vertices generated from the vertex index). Voxels are 8-byte face
  records in six direction pools; back-facing directions and off-screen chunks are skipped, which for the
  three axis-aligned orthographic views removes five sixths of the faces. Transparency uses an
  exact-equivalence batching scheme that reproduces Cubik's per-object sorted two-pass drawing with a
  handful of draw calls in the common case.
- **Threads**: the UI thread owns the document; workers receive immutable snapshots. Two rayon pools
  (foreground for latency-sensitive work such as remeshing, background for throughput work such as
  encoding) and one IO thread. No locks on the document, no blocking of the UI thread.
- **UI**: egui supplies input, text shaping, text editing and painting; geometry comes from a small
  layout engine that implements the uGUI rules the specs extracted (anchors, stacks, grids, 9-slice
  squash), so every rectangle can be unit-tested against the spec tables. Skins are token tables plus
  nine frame pictures; all user-visible strings live in string tables.
- **Quality**: spec byte vectors, golden payload identity, differential testing against the `bson`
  crate, property tests, cargo-fuzz targets for every parser, headless GPU image tests, egui_kittest
  snapshots, layout geometry tests, criterion benchmarks with regression thresholds.
- **Licence**: MIT OR Apache-2.0 for all crates (all dependencies are permissive); clean-room rules for
  Blockbench (GPL-3.0) and Cubik material.
- **Plan**: milestones M0 to M7; every work package owns disjoint directories so agents can work in
  parallel after the M0 skeleton freezes the cross-crate interfaces.

---

## 1. Goals, constraints and principles

### 1.1 Goals, ranked

1. Behavioural parity with Cubik Studio 2.9.480 Beta as specified (UI geometry, editing semantics,
   file bytes, messages).
2. Maximal compatibility with Cubik files: read everything Cubik reads, write files Cubik reads the same
   way, keep untouched data byte-identical, import the user's Cubik settings, presets and locations.
3. Performance: 60+ fps with 10,000 elements or 256^3-extent sparse voxel models in four views;
   sub-second to two-second open/save of the largest realistic files; memory proportional to content.
4. Stability: no panic on any input file, atomic writes, periodic and crash backups.
5. Safety and supply chain: no `unsafe` in our crates; small set of vetted dependencies.

### 1.2 Hard constraints and their consequences

- **Toolchain**: Rust edition 2024 on stable 1.95, pinned with `rust-toolchain.toml`. The pinned
  version is also the MSRV of every crate.
- **No unsafe in our crates**: every `lib.rs`/`main.rs` starts with `#![forbid(unsafe_code)]`, and the
  workspace sets `[workspace.lints.rust] unsafe_code = "forbid"`. Consequences:
  - no derive macros that expand to `unsafe impl` in our crates. `bytemuck_derive` 1.12.1 emits
    `unsafe impl` for `Pod`/`Zeroable` (checked in its source), so GPU data is packed into `Vec<u32>`
    and `Vec<f32>` words (`f32::to_bits`) and handed to wgpu through bytemuck's safe `cast_slice` on
    primitive slices;
  - no memory-mapped files (the mapping APIs are unsafe); files are read with `std::fs::read` or
    buffered readers, which is fast enough at the sizes involved (section 6.5);
  - no direct OS FFI (shell change notification, ANSI code page query and similar); such needs are met
    through safe-API crates or not met at all (sections 12 and 15).
- **Dependencies**: popular, maintained crates with safe APIs, versions verified on crates.io on
  2026-10-18 (section 2.4). `cargo-deny` enforces the licence allowlist, advisories, duplicate versions
  and source restrictions in CI.
- **Platforms**: Windows 10/11 x64 first (wgpu backends D3D12, then Vulkan, GL as last resort); Linux
  x64 (Vulkan, GL; X11 and Wayland); macOS arm64 and x64 (Metal).
- **UI and 3D stack**: eframe/egui 0.37 with custom painting; wgpu 30 for the views, composited by
  egui-wgpu 0.37.

### 1.3 Headline targets (UNVERIFIED until measured; reference machines in 10.1)

| Target | Value |
|---|---|
| Frame rate, 4 views at 1920x1080, MSAA 4x, scene S1 (10,000 elements) | 60+ fps on both reference machines |
| Frame rate, scene S2 (R = 128 world, 256^3-cell model extent, about 4 M voxels, about 2 M exposed faces) | 60+ fps on the desktop reference, 30+ fps on the laptop reference |
| Open .cubik with a 150 MB BSON payload (S2 saved) | under 2 s on the desktop reference |
| Save the same | under 2 s |
| Open/save a 10,000-element file | under 150 ms |
| Undo push, any category, S1 or S2 | under 1 ms |
| Undo or redo restore, S1 or S2 | under 50 ms |
| Hover pick | under 0.3 ms per query |
| Idle | no GPU work and near-zero CPU when nothing changes |
| Time to first frame | under 0.5 s after start, excluding GPU driver initialisation |

### 1.4 Non-goals for version 1

Hierarchies, bones and animation (the Cubik document is flat, M§2.1); a web build; scripting or
plugins; collaborative editing; any feature that requires compute shaders (compute may be added later
as an optional accelerator); HDR or linear-light rendering (Cubik renders in gamma space, A§8.1).

### 1.5 Design principles

- **P1 Parity before speed.** Optimise only where equivalence is provable or testable; otherwise list a
  deviation.
- **P2 The UI thread never waits** for disk, compression, encoding, network or long operations.
- **P3 Pay for what changed.** Revision stamps, dirty sets and copy-on-write everywhere; every cache is
  keyed by revisions.
- **P4 Share, do not copy.** `Arc` records, `Arc` chunks, `Arc` images; snapshots are pointer copies.
- **P5 Hot data in hot shape.** Packed GPU records, structure-of-arrays derived caches for picking and
  culling, dense chunk tables instead of hash maps.
- **P6 One owner for mutable state.** The UI thread owns the document. Workers get immutable snapshots
  and return results over channels. There is no lock around the document, so there is nothing to
  deadlock on.
- **P7 Deterministic regardless of thread count.** Parallel results are merged in a fixed order (list
  order, chunk slot order); randomness comes from an injected seeded generator.
- **P8 No panic on input.** Every size read from a file is checked against limits before allocating;
  all parsers are fuzzed.
- **P9 Redraw on demand.** egui repaints only on input, animation or job completion; a view re-renders
  only if its frame key changed.
- **P10 Everything testable headless.** No window needed for any test except UI snapshots, and those run
  on a software GPU adapter.

---

## 2. Workspace and crate layout

### 2.1 Directory tree

```
cuboid/
  Cargo.toml              workspace members, [workspace.dependencies] (all pinned), [workspace.lints], profiles
  rust-toolchain.toml     channel "1.95.0", components rustfmt and clippy
  .cargo/config.toml      static CRT for windows-msvc targets
  deny.toml               cargo-deny policy (licences, bans, advisories, sources)
  about.toml              cargo-about template for third-party notices
  crates/
    cuboid-num/           cuboid-bson/        cuboid-actions/      cuboid-assets/
    cuboid-textfmt/       cuboid-image/       cuboid-voxel/        cuboid-settings/
    cuboid-filebrowser/   cuboid-ui-kit/
    cuboid-model/
    cuboid-history/       cuboid-ops/         cuboid-scene/
    cuboid-format-native/ cuboid-format-mc/   cuboid-format-entity/ cuboid-format-voxel/ cuboid-format-mesh/
    cuboid-interact/      cuboid-render/
    cuboid-io/            cuboid-ui/
    cuboid-testkit/       (development only)
  apps/cuboid/            binary crate "cuboid-app", executable "cuboid"
  tools/cubik-dump/       canonical payload dump and diff (N§10.2 item 3)
  xtask/                  bless goldens, package, licence report, benchmark comparison
  fuzz/                   cargo-fuzz targets (nightly toolchain; not a workspace member)
  assets/                 own source art, fonts, data tables (embedded by cuboid-assets)
  tests/golden/           public synthetic corpus (private user samples live outside the repository, 14.3)
  docs/spec/  docs/design/
```

### 2.2 Crates

| Crate | Layer | Responsibility | Internal deps | External deps |
|---|---|---|---|---|
| `cuboid-num` | 0 | Numeric parity primitives: half-even rounding, truncating casts, f32/f64 widening rules, Unity-convention Euler/quaternion compose and decompose, angle wrap and quantise, packed ARGB colours and hex text (M§7.3), invariant number parsing with "0 on failure", float text forms G7/X9/NJ (I§3.1), seeded PRNG | none | glam |
| `cuboid-bson` | 0 | BSON subset (N§3.1): zero-copy reader, writer with size patching, lenient typed accessors (N§3.6), canonical text dump | none | none |
| `cuboid-actions` | 0 | `ActionId` registry and metadata, menu trees as data (D§2-3), shortcut canonicalisation and dispatcher (D§6.1-6.2), held-key input map and its strings (D§6.4-6.6), enable-state bitsets | none | bitflags, rustc-hash, smallvec |
| `cuboid-assets` | 0 | Embedded own art (icons, frames, cursors, glyphs), fonts, bundled presets, palettes, legacy id tables, default textures; skin-pack folder loader | none | image (png) |
| `cuboid-textfmt` | 1 | BOM-aware text decoding and UTF-16 unit views; Cubik's lenient JSON dialect (N§9.4, I§8.6); tolerant JSON for `.mcpe` (comments, single quotes, bare names, I§10.1); hand-emitter helpers (tab/LF/CRLF writers); Json.NET-compatible serializer writer (I§3.3-3.4); minify (I§7.9) | num | indexmap |
| `cuboid-image` | 1 | `Image` type (rows bottom to top), engine pixel formats incl. DXT1/DXT5 block decoding (N§4.5.1), file decode/encode (PNG, JPEG, TGA, DDS rule), alpha statistics and summed-area tables, atlas allocator and deterministic packers, paint rasterisers (M§13.2), palette images, texture signature (N§4.5) | num | image, etagere, md-5, encoding_rs, codepage, rayon |
| `cuboid-voxel` | 1 | Voxel world (M§7): chunk table, palette, write/read primitives with the active box, flips/rotations, up/downscale, hollow/fill, tool footprints (M§12.3), editor chunk mesher (M§7.6) producing face records, DDA ray cast, greedy box decomposition (M§14.3), voxelization core (M§14.1) | num | glam, rayon, rustc-hash, fixedbitset |
| `cuboid-settings` | 1 | Settings schema with Cubik key names (A§10.2), missing-key defaults, legacy import, recent files (A§11), paths, atomic file write helper | num | serde, serde_json, directories, tempfile |
| `cuboid-filebrowser` | 1 | Built-in browser model (D§10): listing, sort, filters, wildcards, locations persistence and legacy text format, path aliases, `.lnk` subset (I§28) | num | directories |
| `cuboid-ui-kit` | 1 | Layout engine (anchors, stacks, grids, 9-slice squash; L§0.4), skin tokens and slots (L§12), colour-state model and cross-fades (L§0.5), widget set (L§16), label overflow rules (L§14) | assets | egui |
| `cuboid-model` | 2 | Document aggregate and records, identity, `ObjList`, setters with Cubik semantics (M§3), UV folding, auto-UV (M§4.5-4.7), quarter turns (M§4.9), selection, revisions and change log, invariant checker | num, image, voxel | glam, bitflags, smallvec, rustc-hash, indexmap |
| `cuboid-history` | 3 | Undo/redo with Cubik semantics (M§15) on structurally shared snapshots; per-category restore; background compaction | model | lz4_flex, rayon |
| `cuboid-ops` | 4 | Every editing command and tool (M§3-14) as deterministic functions over an edit context; clipboard (M§11); continuations for confirmations | model, history, image, voxel, num | rayon |
| `cuboid-scene` | 3 | CPU side of rendering and interaction: cameras and navigation maths (A§5), derived per-object caches, BVH, picking (elements, meshes, voxels, gizmos), gizmo geometry, grid maths, atlas layout, render extraction into instance records, voxel mesh cache and pool bookkeeping, transparency draw-list compiler | model, voxel, image, num | glam, rayon |
| `cuboid-format-native` | 4 | `.cubik` container and schema records, encode/decode, load/save mapping and normalisations (N§5-6), retention for lossless round trip (N§8.3), `.cub` decoder (N§9) | bson, textfmt, model, image, voxel, num | zip, flate2 |
| `cuboid-format-mc` | 4 | Minecraft Java model JSON import/export (I§7-8), resource pack (I§9.1), modern Java targets and item definitions (I§31.2-31.4) | textfmt, model, image, num | zip, serde_json |
| `cuboid-format-entity` | 4 | `.mcpe` import, `.js`, `.java`, `.jpm` export, entity atlas (I§10-14), modern Bedrock and JEM (I§31.5-31.7) | textfmt, model, image, num | none |
| `cuboid-format-voxel` | 4 | vox, qb, qef, binvox, schematic, nbt, zox, csv (I§19-26), Stonehearth and Staxel (I§27), modern voxel additions (I§31.8) | model, voxel, image, num | fastnbt, flate2, serde, dot_vox |
| `cuboid-format-mesh` | 4 | OBJ import/export (I§15-16), STL (I§17), PNG sprite import (I§18.2), glTF/GLB export (I§31.9) | model, image, textfmt, num | tobj, gltf-json |
| `cuboid-interact` | 5 | Per-frame interaction state machine (M§8.3-8.7), gizmo drags, nudges, box selection, clone mode, tool strokes, view navigation input (A§5.5), fixed-step clock | scene, ops, model | none |
| `cuboid-render` | 4 | wgpu executor: pipelines, GPU pools, view targets, passes, readback | scene | wgpu, bytemuck |
| `cuboid-io` | 5 | Format registry (I§4), import/export sessions and pipeline (I§5-6), message texts, atomic writes, backups (A§12), recent-file updates, Sketchfab flow (A§16) | format-*, model, ops, settings | tempfile, rayon |
| `cuboid-ui` | 6 | Window shell (L§2-10), panels, menus, dialogs (D§7-9), file browser UI (D§10), status line (A§19) | ui-kit, actions, model, ops, scene, settings, filebrowser, io | egui |
| `cuboid-app` (`apps/cuboid`) | 7 | eframe application, frame loop, job system, platform integration, logging, crash handling | everything above | eframe, egui-wgpu, rfd, opener, log; winreg on Windows; optional ureq, keyring, rodio |
| `cuboid-testkit` | dev | Golden harness, document summaries, image diff, fixture builders | model, formats, image | image |
| `cubik-dump` | tool | Payload dump (key path, BSON type, value, doubles as hex bit patterns) and structural diff | bson, format-native | none |
| `xtask` | tool | `bless`, `package`, `licences`, `bench-compare` | none | none |

### 2.3 Dependency graph and layering rules

```
L7  cuboid-app ................ everything below
L6  cuboid-ui ................. ui-kit, actions, filebrowser, settings, scene, ops, io, model
L5  cuboid-interact ........... scene, ops, model
    cuboid-io ................. format-native, format-mc, format-entity, format-voxel, format-mesh,
                                ops, model, settings
L4  cuboid-ops ................ history, model, image, voxel, num
    cuboid-render ............. scene
    cuboid-format-native ...... bson, textfmt, model, image, voxel, num
    cuboid-format-mc, -entity . textfmt, model, image, num
    cuboid-format-mesh ........ textfmt, model, image, num
    cuboid-format-voxel ....... model, voxel, image, num
L3  cuboid-history ............ model
    cuboid-scene .............. model, voxel, image, num
L2  cuboid-model .............. image, voxel, num
L1  cuboid-image, cuboid-voxel, cuboid-textfmt, cuboid-settings, cuboid-filebrowser ... num
    cuboid-ui-kit ............. assets
L0  cuboid-num, cuboid-bson, cuboid-actions, cuboid-assets (no internal dependencies)
```

Rules (checked by an `xtask` lint over `cargo metadata`):

1. A crate depends only on crates of lower layers.
2. Only `cuboid-render` and `cuboid-app` depend on wgpu; only `cuboid-ui-kit`, `cuboid-ui` and
   `cuboid-app` depend on egui. This keeps all domain logic testable without a GPU or a window.
3. Format crates never depend on UI, scene or render crates; exporters read an immutable document
   snapshot and importers produce a plan (6.2).
4. `cuboid-model` performs no IO and spawns no threads.
5. `cuboid-ops` does not know about views or cameras; anything camera-dependent (nudge planes, box
   selection, hits) is resolved by `cuboid-interact` and passed to ops as plain data.

### 2.4 Third-party crates (verified with `cargo search` and the crates.io API on 2026-10-18)

| Crate | Version | Used by | Choice and notes |
|---|---|---|---|
| eframe | 0.37.0 | app | `default-features = false`, features `wgpu`, `accesskit`, `wayland`, `x11`; no `glow`, no `persistence` (own settings), no `default_fonts` (own fonts) |
| egui | 0.37.0 | ui-kit, ui, app | |
| egui-wgpu | 0.37.0 | app | native texture registration, renderer options (7.1) |
| egui_extras | 0.37.0 | optional | not needed by this design; allowed for debug tooling |
| egui_kittest | 0.37.0 | dev | features `wgpu`, `snapshot` |
| wgpu | 30.0.1 | render, app | native backends only |
| glam | 0.34.1 | num, model, scene, voxel | f32 SIMD maths; f64 only for derived, non-persisted values (M§16.1) |
| bytemuck | 1.25.2 | render | safe `cast_slice` on primitive slices; derive feature off |
| serde, serde_json | 1, 1 | settings, formats | `serde_json/preserve_order` only in modern JSON import |
| bson | 3.1.0 | dev only | independent oracle for differential tests of `cuboid-bson` |
| zip | 9.0.5 | format-native, format-mc, io | `default-features = false, features = ["deflate-flate2"]` (the default set pulls zopfli, zlib-rs, aes, bzip2, zstd, lzma) |
| flate2 | 1.1.10 | formats, zip | `rust_backend` (miniz_oxide); `zlib-rs` behind an opt-in speed feature after benchmarking |
| image | 0.25.10 | image, assets, testkit | `default-features = false`, features `png`, `jpeg`, `tga`; DDS uses Cubik's own rule (N§4.5.1) |
| rfd | 0.17.2 | app | async file dialogs; default `xdg-portal` on Linux (no GTK) |
| arboard | 3.7.0 | transitive (eframe) | no direct use: Cubik's clipboard is internal (A§14) |
| thiserror | 2.0.21 | all libraries | error enums |
| log | 0.4.34 | all | facade; own logger (13.2) |
| directories | 6.0.0 | settings, filebrowser | `ProjectDirs`, `UserDirs` |
| indexmap | 2.14.2 | model, textfmt, formats | ordered maps and sets for deterministic output |
| rustc-hash | 2.1.3 | most | `FxHashMap`, `FxHashSet` |
| quick-xml | 0.42.0 | reserved | only if an XML format is added later; not needed for parity |
| fastnbt | 2.6.3 | format-voxel | gzip-wrapped big-endian NBT; `SerOpts::root_name("Schematic")` gives the exact root |
| dot_vox | 5.2.0 | format-voxel | modern `.vox` import (scene graph); the `cubik29` reader is own code (6.7) |
| tobj | 4.0.5 | format-mesh | modern OBJ import only; `cubik29` OBJ parsing is own code (6.7) |
| stl_io | 0.11.0 | dev | read-back oracle for the own STL writer |
| gltf-json / gltf | 1.4.1 / 1.4.1 | format-mesh / dev | glTF types for writing; loader as test oracle |
| rayon | 1.12.0 | many | two pools (section 9) |
| smallvec | 1.16.3 | model, actions | stable 1.x line (the 2.0 line is still beta) |
| bitflags | 2.13.2 | model, actions | |
| fixedbitset | 0.5.7 | voxel, scene | dirty sets |
| lz4_flex | 0.14.0 | history | default safe implementation |
| etagere | 0.3.0 | image | shelf atlas allocation |
| md-5 | 0.11.0 | image | texture signature `cs` |
| encoding_rs, codepage | 0.8.42, 0.1.3 | image, textfmt | code-page decoding of `cs`, UTF-16 |
| tempfile | 3.27.0 | settings, io | `NamedTempFile::new_in` plus `persist` for atomic replace |
| opener | 0.9.0 | app | open and reveal folders, open URLs |
| ureq | 3.4.2 | app, feature `net` | HTTPS for opt-in network features only |
| keyring | 4.2.0 | app, feature `keyring` | Sketchfab token in the OS credential store |
| rodio | 0.23.0 | app, feature `sound` | `default-features = false`, playback only, embedded PCM (no decoder crates) |
| winreg | 0.56.0 | app, Windows only | read the ANSI code page value and legacy registry values (12.3, 12.4) |
| profiling | 1.0.18 | all, optional | already used by egui-wgpu; puffin or tracy backends in development builds only |
| proptest, criterion, insta | 1.12.0, 0.8.2, 1.49.0 | dev | property tests, benchmarks, text snapshots |
| arbitrary, libfuzzer-sys | 1.5.0, 0.4.13 | fuzz crate | nightly only |
| winresource | 0.1.34 | app build-dependency, Windows | icon and version resource |

Tools used by CI and packaging (not dependencies): cargo-deny 0.20.2, cargo-about 0.9.2, cargo-fuzz
0.13.2, cargo-packager 0.11.8, cargo-deb 3.8.0, cargo-wix 0.3.9, cargo-bundle 0.12.0, cargo-dist 0.32.0.

Rejected on purpose: memory-mapping crates (unsafe API), C-backed compressors (`zstd`, C zlib), C
allocators, `slotmap` (the custom `ObjList` in 3.3 needs `Arc` records and revision stamps per slot, and
is about 200 lines of safe code), `bvh` and `parry3d` (an incrementally refit BVH tuned to this data is
small and avoids a large dependency).

### 2.5 Build profiles, features and toolchain configuration

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "unwind"            # required by the crash handler (13.3)
debug = "line-tables-only"  # symbolised crash reports; debug info is split off at packaging time

[profile.dev]
opt-level = 1
[profile.dev.package."*"]
opt-level = 3               # egui, wgpu, image and miniz_oxide are too slow at opt-level 0

[profile.bench]
inherits = "release"
debug = true
```

- Cargo features of `cuboid-app`: `net`, `keyring`, `sound`, `profile-puffin` (dev).
- Windows MSVC builds link the CRT statically (`-C target-feature=+crt-static` in `.cargo/config.toml`),
  so no Visual C++ redistributable is needed.
- Baseline CPU features only (x86-64 SSE2, aarch64 NEON); no AVX2 requirement.
- Lints: `unsafe_code = "forbid"`; clippy `-D warnings` in CI; `clippy::unwrap_used` and
  `clippy::expect_used` denied in non-test code of parser crates; `clippy::float_cmp` allowed in the
  parity crates (exact float comparison is part of the specified behaviour, for example M§3.4).

---

## 3. Core data model

### 3.1 Document aggregate

```rust
pub struct Document {
    pub props: Arc<ModelProperties>,     // mp (N§4.2): option groups, origin g.o, stored box g.mb
    pub view: Arc<ViewProperties>,       // vp (N§4.3): grid steps, snap flag, four camera states
    pub blueprint: Blueprint,            // small value type (M§2.3)
    pub model_box: ModelBox,             // live box, always present (M§2.4)
    pub elements: ObjList<Element>,
    pub meshes: ObjList<Mesh>,
    pub textures: TextureList,
    pub voxels: Option<VoxelWorld>,
    pub display: Arc<ModelDisplay>,      // md (N§4.8): eight slots
    pub ext: Arc<Extensions>,            // port-only data (item overrides, ...) saved in cuboid.json (N§8.4)
    pub retained: Option<Arc<Retained>>, // raw spans for byte-identical re-save (N§8.3)
    pub revs: Revisions,                 // per-category revision counters
}
```

The fields map one to one to Cubik's undo categories (M§2.5): model properties (including the box,
origin and display), view properties, elements (with the blueprint), textures, voxels, meshes and model
display. Snapshots and restores therefore work per field (4.2). Small aggregates sit behind `Arc` so a
snapshot is a pointer copy and a mutation is `Arc::make_mut`.

### 3.2 Identity

- `ObjId(NonZeroU64)`: session-unique, allocated from a process-wide atomic counter, never reused, never
  persisted. Records keep their `ObjId` through undo, redo and restore, so the renderer, the lists and the
  selection have stable keys. Paste, clone and import allocate fresh ids.
- Display ids (the `#n` shown in lists, M§2.1) are Cubik-visible and are re-issued in list order after
  every load, undo and redo. They are **not** stored in the records: they live in a vector parallel to the
  list order inside `ObjList`. Renumbering after a restore therefore touches no record and causes no
  copy-on-write.
- `ObjRef` names anything selectable: `Element(ObjId)`, `Mesh(ObjId)`, `Blueprint`, `ModelOrigin`.

### 3.3 `ObjList<T>`: ordered, shared, copy-on-write records

```rust
pub struct ObjList<T> {
    items: Vec<Arc<T>>,          // list order = list panel order = file order = export order
    display_ids: Vec<u32>,       // parallel to items (M§2.1)
    next_display_id: u32,        // per-kind counter; reset to 0 by "clear all"
    pos: FxHashMap<ObjId, u32>,  // id -> position, rebuilt lazily after structural edits
    pos_valid_for: u64,          // structure revision the map was built for
    structure_rev: u64,          // bumped by insert, remove, reorder, restore
}
```

| Operation | Cost |
|---|---|
| lookup by id | O(1) after one O(n) rebuild per structural change |
| append | O(1) |
| remove a set, stable partition (move to top/bottom, sort transparent) | one O(n) pass |
| snapshot | O(n) pointer clones (about 20 microseconds for 10,000 records) |
| restore | O(n) pointer copies plus normalisation checks (4.3) |
| first write to a record shared with a snapshot | one record copy (about 0.5 KiB) |

All mutation goes through `ObjList::edit(id) -> RecordMut<'_, T>`, which performs `Arc::make_mut`,
stamps the record with a fresh revision and records the id in the frame's change log. A mutable borrow
counts as a modification; this is conservative (it may cost a re-upload or a re-encode) but never wrong.

### 3.4 Records

```rust
pub struct ObjectBase {                 // M§3.1
    pub label: Label,
    pub p: Vec3, pub s: Vec3,           // position (min x, min y, max z corner), size
    pub e: Vec3,                        // Euler degrees, wrapped and quantised (M§3.4)
    pub o: Vec3,                        // pivot
    pub hidden: bool, pub locked: bool,
    pub perms: Perms,                   // move/rotate/scale, restrictable, snap-on-scale, deletable
    pub drag_sensitivity: f32, pub gizmo_size: Vec3, pub gizmo_pivot: Vec3,
}
pub struct Element {                    // M§4.1
    pub id: ObjId, pub rev: Rev, pub base: ObjectBase,
    pub kind: ElementType,              // Cube, PlaneX, PlaneY, PlaneZ
    pub color: [f32; 4],
    pub faces: [Face; 6],               // down, up, north, south, west, east
    pub flags: ElementFlags,            // uv_snap, uv_mirror, rescale, shade, invert
}
pub struct Face { pub name: FaceName, pub active: bool, pub tint: i32, pub cull: FaceFlags,
                  pub texture: Label, pub uv: UvRect }
pub struct UvRect { pub rot: i32, tl_raw: Vec2, br_raw: Vec2 }  // setters clamp to [-1, 2]; getters fold (M§4.5)
pub enum FaceName { Down, Up, North, South, West, East, Other(Label) } // hand-edited names survive (N§4.4.2)

pub struct Mesh {                       // M§6.1
    pub id: ObjId, pub rev: Rev, pub base: ObjectBase, pub kind: MeshKind,
    pub geometry: Arc<MeshGeometry>,    // shared by clones and snapshots; primitives share one per kind
    pub texture: Option<Arc<Image>>, pub tint: [f32; 4],
    pub bounds: MeshBounds, pub norm_factor: f32, pub import_offset: Vec3,
}
pub struct TextureEntry {               // M§5.1, N§4.5
    pub label: Label, pub file_path: String, pub file_name: Option<String>,
    pub mc_path_explicit: Option<String>, pub signature: Option<Arc<str>>,
    pub file_size: Option<u64>, pub image: Arc<Image>, pub rev: Rev,
}
pub struct CameraState { pub pos: Vec3, pub rot: StoredQuat, pub ortho: bool, pub ortho_size: f32, pub fov: f32 }
pub struct StoredQuat { pub q: Quat, pub euler: Vec3 } // both members kept for exact re-save (N§3.4)
```

- `Label` is an `Arc<str>`. Loads and pastes intern labels through a per-document pool, so the six face
  labels of most elements share one allocation and compare by pointer first.
- An `Element` is about 500 bytes; 10,000 elements are about 5 MiB of records.
- Field visibility: plain data fields are public for reading; every write path that has Cubik semantics
  (position propagation, pivot-relative resize, rotation wrap/quantise/restrict, UV snap refresh) is a
  method in `cuboid-model` (M§3.2-3.5, M§4.7), shared by ops and importers.

### 3.5 Images

```rust
pub struct Image {
    pub w: u32, pub h: u32,
    pub stored: StoredPixels,           // engine format code and raw bytes as read from .cubik (incl. mip chain)
    rgba: OnceLock<Arc<[u8]>>,          // decoded level 0, RGBA8, rows bottom to top
    stats: OnceLock<AlphaStats>,        // partial-alpha flag, zero-alpha flag, summed-area table (lazy)
    pub rev: Rev,
}
```

- Rows are stored **bottom to top** in memory (the engine order of N§4.5.1). Saving an RGBA32 image is a
  copy, and all UV maths keeps Cubik's V-up convention. Rows are flipped exactly once, when decoding or
  encoding PNG, JPEG or TGA files.
- An image read from a `.cubik` file in any format keeps its original format code and raw bytes and is
  re-saved unchanged unless edited (N§4.5.1). Editing produces a new RGBA32 image.
- Decoding is lazy and thread-safe (`OnceLock`), so loading a file with many textures does not decode
  images that are never displayed until the atlas needs them, and workers can decode in parallel.
- `AlphaStats` holds a summed-area table of "texel has 0 < alpha < 255"; the renderer asks whether a
  face's UV crop (M§5.5) contains any partial alpha in O(1) (7.8).

### 3.6 Voxel world

```rust
pub struct VoxelWorld {
    pub res: u32,                       // R (M§7.1)
    n: u32,                             // chunks per axis = ceil(3R / 16)
    slots: Vec<ChunkSlot>,              // n^3 entries; slot = (cx * n + cy) * n + cz
    pub palette: Palette,               // [0, ...] in first-write order (M§7.4)
    pub active_box: IBox3,              // half-open, from the model box (M§7.2)
    pub setup_corners: WBox3,           // remembered for up/downscale (M§7.2 quirk)
    pub ao: bool,
    dirty: FixedBitSet,                 // slots needing remeshing
    pub rev: Rev,
}
pub enum ChunkSlot { Unallocated, EmptyWritten, Data(Arc<Chunk>) }
pub struct Chunk { cells: [u32; 4096] /* x + 16y + 256z */, nonzero: u16, occ: [u64; 64], rev: Rev }
pub struct Palette { values: Vec<u32>, index: FxHashMap<u32, u32> }
```

- The slot order equals Cubik's persistence order (cx outermost, then cy, then cz, N§4.7), so saving walks
  memory linearly.
- `EmptyWritten` reproduces Cubik's persistence rule without storing cells: a chunk that once held a
  non-zero voxel and was then erased voxel by voxel is still written with an empty `v` array, while a
  chunk that never held a voxel, or was cleared by "Clear voxels", is not written (N§4.7, M§7.1). Cubik
  also allocates chunks on reads; such chunks keep their "empty" flag and are never written, so
  allocating on writes only gives identical files.
- `occ` is a 4096-bit occupancy mask kept in sync with `cells`; neighbour tests in the mesher, hollow and
  fill become bit operations.
- Memory: 16.5 KiB per allocated chunk. A dense 256^3 region is 4,096 chunks (66 MiB). The slot table
  at the maximum resolution R = 254 is 48^3 = 110,592 entries (about 1.7 MiB).
- All three write primitives of M§7.5 exist as methods; batch variants (`write_many`) group cells by
  chunk so a brush stroke does one copy-on-write per touched chunk. Every write dirties all chunks within
  one voxel of the cell (the port note of M§7.5).
- Whole-world operations (flip, rotate, colour all, up/downscale, hollow, fill, crop) are written in a
  functional style: each output chunk is computed from input chunks in parallel and the new table is
  committed at the end, which makes them cancellable and leaves undo snapshots untouched.

### 3.7 Selection and highlight state

`Selection { list: IndexSet<ObjRef, FxBuildHasher>, primary: Option<ObjRef> }` implements M§8.1 with
O(1) membership. The highlight set (what the renderer outlines) is separate state, updated by the same
events as in Cubik, because select-all and select-by-name empty the list without removing highlights
(M§8.1 quirk); deriving highlights from the selection would hide that quirk. Hover, active axis, busy,
grabbed and clone-guard flags are interaction state owned by `cuboid-interact`, not by the document.

### 3.8 Hierarchy policy

The Cubik document is flat (M§2.1) and version 1 stays flat. Because `ObjId`s are stable, groups can be
added later as a side table `ObjId -> GroupId` persisted only in `cuboid.json` (never in the BSON
payload), without touching records, the codec or undo.

### 3.9 Invariants

Checked by `Document::check_invariants()` in debug builds after every op, by every property test and by
the fuzz harness after every applied plan:

- every live `ObjId` appears exactly once in its list; the position map agrees with the list; display ids
  are `1..=n` in list order after a renumbering and strictly increasing in creation order otherwise;
- sizes set through the size setter have every component at least 0.001; stored rotations are in
  `[0, 360)` (except the documented below -360 quirk) and on the 5 or 22.5 degree grid (M§16.2);
- each element has six faces; UV rotations are 0, 90, 180 or 270; raw UV corners are within `[-1, 2]`;
- the voxel palette starts with 0 and holds distinct values; every `Data` chunk's `nonzero` and `occ`
  match its cells; the active box is half-open and inside the grid;
- the texture label index matches the entries; selection references only live objects.

### 3.10 Revisions, change log and the dirty flag

- A document-wide `Rev(u64)` counter stamps every record, chunk and image write; each category has its
  own revision; `ObjList` has a structure revision.
- Each frame ops append to a `ChangeLog`: touched element and mesh ids, dirty voxel slots (bitset),
  texture changes, and flags such as `SELECTION`, `HIGHLIGHT`, `CAMERA(view)`, `GRID`, `DISPLAY`,
  `RENDER_PREFS`. The renderer, the scene caches, the list panels and the status line consume the log
  once per frame.
- The document's dirty flag (used by the unsaved-changes prompt) follows Cubik literally: set by every
  undo push, cleared by a native save or a reset, untouched by undo and redo (M§15.1, A§2.2). It is not
  derived from revisions, because edits that push no undo step (element settings panel, keyboard
  nudges, model box drags) must not mark the document dirty.

### 3.11 Numeric parity layer (`cuboid-num`)

| Function | Rule |
|---|---|
| `round_he(x)` | `f32::round_ties_even`; Mono's `Mathf.Round` rounds the widened double half to even, which gives the same result for every f32 |
| `trunc_i32(x)` | truncation toward zero; saturation and NaN handling documented and tested against the specified casts |
| `wrap360(c)` | `(c + 360) % 360` with the dividend's sign (C# remainder), so inputs below -360 stay negative (M§3.4) |
| `quantise(c, step)` | `round_he(c / step) * step` in f32 |
| `euler_to_quat(x, y, z)` | `qY * qX * qZ`, degrees, left-handed (N§4.3.2) |
| `quat_to_euler(q)` | inverse decomposition, made positive per component with the -0.0057295779 / 359.99427 rule (N§4.3.2); tests compare with 1e-4 degree tolerance |
| `pack_argb`, `unpack_argb`, `hex_*` | M§7.3 and N§4.10 packing variants, each named after its use |
| `parse_invariant_f32/i32` | "try parse, 0 on failure", `.` separator (M§1.6) |
| `fmt_g7`, `fmt_x9`, `fmt_nj` | I§3.1 float text forms, implemented from std's correctly rounded formatting, never from locale formatting |
| `Rng` | small seeded generator (for example PCG32), injectable for tests (M§16.5) |

All parity-sensitive maths uses f32 in the specified operation order (M§1); f64 is used only for
derived values that never feed back into stored values.

---

## 4. Edit pipeline, undo and redo

### 4.1 Edit context and the op contract

```rust
pub struct EditCtx<'a> {
    pub doc: &'a mut Document,
    pub sel: &'a mut Selection,
    pub hist: &'a mut History,
    pub env: &'a EditEnv,     // settings snapshot (restrict rotations, render prefs), time, constants
    pub rng: &'a mut Rng,
    pub clip: &'a mut Clipboard,
    pub out: &'a mut OpOutput, // change log, status text, UI effects, continuations
}
```

Every command is a plain function `fn op(cx: &mut EditCtx, args) -> Result<(), OpError>` that:

1. checks its guard and emits the specified message as a UI effect when it fails (D§4.2);
2. pushes its undo entry *before* mutating, with the exact label and categories, including the
   specified capture gaps (M§15.5);
3. mutates through the model's setter methods;
4. reports changes in `cx.out`, never touches UI or GPU state, and is deterministic for a given
   document, selection, environment and seed.

Commands that need answers (confirmations, prompts) return `UiEffect::Ask` with *continuations*: plain
`enum` values naming the follow-up op and its arguments (5.6). The same continuations drive the
command-log replay tests (14.8).

### 4.2 Undo and redo

Observable semantics follow M§15 exactly: one entry per push with label, snapshot of the pushed
categories taken before the mutation and an optional callback; pushing truncates the redo tail, clears
the redo cache, drops the oldest entry beyond 11, sets the dirty flag and refreshes the Edit menu; undo
creates the redo-cache entry from the current state on first undo; redo never creates entries; undo and
redo leave the dirty flag alone.

Storage shares structure instead of copying:

| Category | Snapshot content | Capture cost |
|---|---|---|
| modelProperties | `Arc<ModelProperties>`, model box value, `Arc<ModelDisplay>` | O(1) |
| viewProperties | `Arc<ViewProperties>` (cameras captured first) | O(1) |
| elements | blueprint value, `Vec<Arc<Element>>` in list order, selected flags as a bit vector over positions | O(n) pointer clones |
| elements, selection only (copy, clone, M§15.2) | the selected records only | O(k) |
| textures | `Vec<Arc<TextureEntry>>` | O(t) |
| voxels | resolution, setup data, AO flag, `Vec<ChunkSlot>` (pointer clones) | O(n^3 slots); 13,824 at R = 128 |
| meshes | `Vec<Arc<Mesh>>` (geometry shared) | O(m) |
| modelDisplay | `Arc<ModelDisplay>` | O(1) |

The port decision of M§11.4 (deep copy on every restore and paste) is satisfied by construction: shared
records are immutable, and any later write copies them first, so edits never leak into snapshots or
sibling pastes.

### 4.3 Restore

Restores follow M§15.3 step by step, with each normalisation written to be idempotent and to copy a
record only if it would change it:

- **elements**: clear the selection; replace the list with the snapshot's pointers; renumber display ids
  from 1; restore the blueprint; re-quantise rotations under the current restriction setting; run the
  start-of-life auto-UV pass for elements with UV snap (M§4.7, applied immediately instead of on the next
  frame, which is not observable); re-select records whose flag was set. Records whose content does not
  change keep their revision, so the renderer re-uploads nothing for them.
- **voxels**: Cubik recreates the world, clears it and re-writes every cell with the full writer, which
  drops cells outside the *current* active box and rebuilds the palette in scan order (M§7.10). The port
  swaps the slot table, clears cells outside the current active box (only chunks crossing the box
  boundary are copied) and recomputes the palette in file order (chunk slot order, then x, y, z), in
  parallel per chunk with an ordered merge. The result is identical.
- **textures**: replace the list; the renderer rebuilds the atlas layout (7.5) and refreshes every face's
  atlas UVs, as Cubik does when any texture changes (M§5.1).
- **meshes** and the small categories: pointer replacement plus the specified re-selection.

### 4.4 Memory control

- After every frame, chunks and images referenced only by history (strong count 1 once the live document
  has copied them) are queued for compression on the background pool with `lz4_flex`; history entries
  then hold `Compressed(Arc<[u8]>)` variants. Restoring decompresses in parallel. Voxel chunks typically
  compress 10 to 50 times.
- History accounts for its unique bytes. Above a cap (default 1 GiB after compression) the oldest
  entries are released early and the status line says so; with 11 entries this only happens with huge
  textures or whole-world voxel edits of very large worlds (deviation D-P4).
- Texture snapshots share whole images. Painting a 4096x4096 texture copies 64 MiB once per stroke; if
  profiling shows this matters, images can move to 64x64-texel copy-on-write tiles without changing any
  API outside `cuboid-image` (open item R-6).

### 4.5 Gestures and long operations

- Drags push their entry on the first drag frame and never coalesce (M§15.4); with sharing this costs
  O(n) pointer clones, about 20 microseconds for 10,000 elements.
- Long operations (hollow and fill, voxelization, voxel-to-element conversions, face optimisation, atlas
  creation, up/downscale on large worlds) push their entry, then run on the background pool against
  copy-on-write working data while the modal progress dialog of D§9.11 is shown, and commit atomically
  on the UI thread. Document editing is blocked while the dialog is modal, as in Cubik. Cubik's Abort
  button does nothing; the port may make it cancel (deviation D-P3).

### 4.6 Clipboard

The internal clipboard of M§11 holds either an object snapshot (selected element and mesh records plus
the blueprint record, shared by pointer, and the copy anchor) or a voxel clipboard (a list of
`(dx, dy, dz, value)` cells relative to the box minimum). Paste allocates fresh ids and applies the
specified offset and normalisations.

---

## 5. Actions, menus and shortcuts (`cuboid-actions`)

### 5.1 Registry

`ActionId` is one `enum` covering every menu action, hidden binding, toolbar and panel button of D§2-5
and L§6-8. A `const` table maps each id to its metadata: label, icon name, shortcut display text, flags
(always active, works in UI, linked button), owning menu, undo label and category (for documentation and
tests). A test checks the table against the spec counts (58 row bindings plus 3 hidden bindings, D§6.3).

### 5.2 Menus as data

Menu trees are `const` data: menu id, minimum width, rows (label, icon, shortcut, action or child menu,
flags, separators), in the exact order of D§2-3. Dynamic rows (Undo, Redo, Paste, recent files) carry a
label provider. The UI renders menus from this data, so menus need no per-menu code.

### 5.3 Enable states

Each frame the app computes a small `AppQuery` (selection kind and primary kind, history availability,
clipboard kind, recent list emptiness, modal flag) and derives a bitset of enabled menus and rows with
the rules of D§1.6-1.7. Linked toolbar buttons read the same bitset. Cost: a few hundred bit operations.

### 5.4 Shortcut dispatch

- Canonicalisation of shortcut text follows D§6.1 exactly ("+" means keypad plus, "PgUp" means PageUp,
  "Ctrl+1 (Num. keypad)" means Ctrl+Keypad1, and so on).
- Dispatch processes each key-down event separately in arrival order (the recommendation of D§6.2):
  update the held-key list, build the combo in press order, look it up in an
  `FxHashMap<Chord, SmallVec<[ActionId; 2]>>`, and fire bindings that pass the modal, keyboard-focus
  (works-in-UI) and menu-enabled tests. Key repeats are ignored, as in Cubik.
- Key identity: letter and function-key chords match logical keys (the text printed in menus); held
  mode keys such as `Alpha1` match physical keys, so that digit holds work on layouts where digits need
  Shift. How Cubik's engine mapped non-QWERTY layouts is UNVERIFIED (R-9).
- eframe turns Ctrl+C, Ctrl+X and Ctrl+V into copy, cut and paste events for text editing; the input
  adapter converts those events back into chords when no text field has focus. The exact event shape in
  eframe 0.37 must be checked during implementation (UNVERIFIED).

### 5.5 Held bindings

The input map of D§6.4 (ModeRotate, ModeScale, ModeClone, PanView, Focus, modifiers, arrows) is a table
of up to four keys per binding. User strings (`advKeys*`) are parsed and re-stored canonically with the
rules of D§6.6. cuboid applies stored strings at start-up as well (the deliberate fix in D§6.6).

### 5.6 Continuations and deferred execution

- Dialog flows are data: `UiEffect::Ask { boxspec, on: [Option<Continuation>; 3] }`. The message-box
  handler mapping of D§8.4 (which button and which close action run which handler) is implemented once
  in the message-box widget.
- Cubik runs menu and shortcut actions 0.05 s after the click or key (D§1.5). A time-ordered queue
  processed in `logic()` reproduces this; toolbar buttons call actions directly (D§5.14).

---

## 6. File formats and IO

### 6.1 Three layers

1. **Codec crates** turn bytes into format records and back (pure functions, fuzzed). For `.cubik` the
   records mirror N§4 one to one; for interop formats the records are each format's own structure.
2. **Mapping** turns records into an `ImportPlan` against a snapshot of the current document, applying
   Cubik's apply order and normalisations (N§6, I§5-27), and turns a `DocSnapshot` into records with the
   save normalisations (N§5.3). Mapping lives in the format crate, next to the codec.
3. **`cuboid-io`** owns the registry (I§4), validation and messages (I§5.2, I§6.2), the interactive
   session loop, atomic writes, status texts, recent-file updates and backups.

### 6.2 Traits

```rust
pub trait Format: Send + Sync + 'static {
    fn key(&self) -> &'static str;                  // registry key, extension without dot
    fn caps(&self) -> Caps;                         // LOAD | SAVE | IMPORT | EXPORT
    fn label(&self) -> &'static str;                // dialog label
    fn message_name(&self, dir: Direction) -> &'static str;
    fn visibility(&self) -> Visibility;             // I§4.2 rules
    fn importer(&self) -> Option<&dyn Importer> { None }
    fn exporter(&self) -> Option<&dyn Exporter> { None }
}
pub trait Importer: Send + Sync {
    fn begin(&self, src: ImportSource, opts: ImportOptions, base: DocSnapshot) -> Box<dyn ImportSession>;
}
pub trait ImportSession: Send {
    fn step(&mut self, answer: Option<Answer>, cx: &mut StepCx) -> ImportStep; // runs on a worker
}
pub enum ImportStep { Progress(Progress), Ask(Prompt), Done(ImportPlan), Failed(ImportFailure) }
pub trait Exporter: Send + Sync {
    fn begin(&self, snap: DocSnapshot, target: PathBuf, opts: ExportOptions) -> Box<dyn ExportSession>;
}
pub enum ExportStep { Progress(Progress), Ask(Prompt), Done(Vec<OutFile>), Abandon }
```

- Sessions are resumable state machines: when a step returns `Ask`, the worker returns, the UI thread
  shows the prompt (for example the parent-model questions of I§8.2, the `.mcpe` name prompt of I§10.3,
  the voxel-only prompt of I§6.3), and the answer resumes the session on a worker. No worker ever blocks
  waiting for the UI.
- `ImportPlan` contains fully built parts (new element and mesh lists or appended records, texture list,
  voxel world, property changes, side effects such as `ep`, grid steps and the model name) and is applied
  atomically on the UI thread in time proportional to the changed parts.
- `OutFile { path, bytes, kind }` lists every file to write in order; `cuboid-io` writes them with the
  specified per-file messages (I§6.4).
- Output profiles `Profile::Cubik29 | Modern(Target)` and import modes `Cubik29 | Modern` are explicit
  parameters (I§1.2); there is no global format state.

### 6.3 `.cubik`

**Container (N§2).** `zip` 9 reads stored and deflated entries, data descriptors and ZIP64; the entry
`cubik` is found case-sensitively, then case-insensitively; distinct errors for "not a ZIP", "no entry"
and "corrupt entry". The writer emits a single entry `cubik` with deflate at the fastest level (Cubik
uses "best speed"), streams into a `NamedTempFile` in the target directory and renames it over the
target; a failed save is reported as a failure.

**BSON (`cuboid-bson`).**

- Writer: `begin_doc() -> Mark` reserves the 4-byte size, `end_doc(mark)` writes the terminator and
  patches the size; keys are `&'static [u8]` with their NUL already appended; `f32` values are widened
  with `f64::from`; integers, enumerations and the byte field `v` are int32; array keys come from a
  precomputed table for indices below 4,096 and from integer formatting above.
- Reader: validates each document's size against the enclosing bounds, then iterates elements as
  `(key, type, value span, element span)` without copying; typed accessors implement every lenient
  conversion of N§3.6; duplicate keys resolve last-wins by iteration order; quaternion members are
  applied in document order with the `eulerAngles` override (N§3.4).
- Canonical fast paths: a voxel record with the canonical layout (`21 00 00 00 10 'x' 00 .. 10 'c' 00 ..
  00`, 33 bytes, N§3.5) is recognised by checking its fixed bytes and its four values are read at fixed
  offsets; Vector2 (27 bytes) and Vector3 (38 bytes) documents likewise. Anything else falls back to the
  generic path, so hand-edited files still load.
- Schema tables: each record type of N§4 is declared once with a `macro_rules!` table (key, BSON type,
  default if absent, nullability, field accessor). The same table generates the encoder (exact key order,
  N§3.3) and the decoder (any order, unknown keys collected), so key order cannot drift between the two.

**Normalisations.** Save-time rules of N§5.3 (quote replacement, `gu = 0.5`, `gl = false`, blueprint
fields, packed colours, folded UVs, mesh geometry only for custom meshes, display scale zeros as
0.0001, voxel scan order) are applied by the mapping from model to records; load-time rules of N§6.3
(apply order, rotation quantisation with the file's restriction flag, plane thickness, texture
replacement, voxel recreation, grid snap reset) by the mapping from records to plan.

**Lossless round trip (N§8.3).** The decompressed payload is kept as one `Arc<[u8]>`; each section and
each element, mesh, texture and voxel chunk record keeps its raw span and the revision it had after
loading. The writer copies the raw span when the object is unchanged and re-encodes it otherwise.
Unknown keys of a re-encoded record are kept in a side table as "raw element after known key k" and
re-emitted at their position. Camera states are compared with the loaded ones before re-encoding, so an
unchanged `vp` is copied too. Retention can be switched off, and is dropped automatically above a memory
cap (default 512 MiB).

**Extensions (N§8.4).** Port-only data (for example Minecraft item overrides) goes into an extra ZIP
entry `cuboid.json` with its own version number, written only when such data exists and the user has not
disabled extensions; the `cubik` payload never receives extra keys.

**Limits.** Configurable limits checked before allocating: payload size (at most the BSON int32 range),
array lengths, texture `w*h` and data length against the format's level-0 size, mesh vertex counts,
voxel resolution and chunk count. Violations are recoverable errors with clear messages (N§8.1 item 4).

### 6.4 How compatibility is guaranteed

| Guarantee | Mechanism |
|---|---|
| Exact bytes for each record type | unit tests with the vectors of N§3.5 and hand-computed bytes per record (N§10.1) |
| Untouched data re-saves byte-identically | golden "payload identity" test per sample (N§10.2) |
| Edited data matches Cubik except documented normalisations | golden "semantic round trip" test listing the normalisations (N§10.2) |
| Decoder accepts what Cubik accepts | lenient-form tests per N§3.6; fuzzing; differential decoding of random documents produced by the `bson` crate |
| Our BSON is well-formed BSON | every encoder test also decodes the output with the `bson` crate and compares trees; debug builds decode every saved payload with our own reader (N§8.2 item 6) |
| Files from Cubik open correctly | user-provided sample corpus (N§11) with expected summaries |
| Files from cuboid open correctly in Cubik | cross-application campaign on Windows (N§10.3), manual at first |

### 6.5 Large-file performance

```
open:  IO thread   read file -> ZIP index -> inflate "cubik" into Vec<u8> (size from header, limit checked)
       bg pool     index top-level sections -> decode records; voxel chunk records and mesh arrays in parallel
                   -> map to ImportPlan against a base snapshot (merge semantics need it)
       UI thread   apply plan atomically -> history reset (open) -> scene marks everything dirty
       fg pool     remesh all chunks in parallel; uploads in slot order within a per-frame budget

save:  UI thread   DocSnapshot (pointer copies; cameras captured first, N§5.2)
       bg pool     encode sections; chunks and element ranges in parallel into separate buffers; unchanged
                   records copied from retained spans; concatenate in order and patch root sizes
       IO thread   deflate (fastest level) streamed into a temporary file in the target folder -> rename
       UI thread   status, title, recent files; dirty flag cleared only if no edit happened after the snapshot
```

Estimates for scene S2 (about 4 M voxels, BSON payload about 150 MB; UNVERIFIED): voxel record decode
through the fast path about 10 ns per record (40 ms single-threaded, less in parallel), inflate 0.3 to
0.4 s, deflate 0.6 to 0.8 s with miniz_oxide at the fastest level, everything else small. Deflate of the
single entry is the only serial stage of a save; an optional `zlib-rs` backend roughly halves it
(UNVERIFIED).

The voxel palette is rebuilt from colours in file order (N§4.7): each chunk computes its local
first-occurrence list in parallel, then the lists are merged in chunk order, which preserves the order a
sequential scan would produce.

### 6.6 Legacy `.cub`

BOM-aware decode (UTF-8 with U+FFFD for invalid sequences, UTF-16 LE/BE), XOR over UTF-16 code units
with the two keys of N§9.3, then the Cubik dialect parser shared with Minecraft JSON import (I§8.6). The
import is atomic (N§9.6 requires it). Load only; no writer.

### 6.7 Interop formats: own code or crate

| Format | `cubik29` implementation | Reason | Modern profile |
|---|---|---|---|
| Minecraft JSON import | own dialect parser (I§8.6) | must accept and reject exactly what Cubik does | `serde_json` with ordered maps, escapes decoded, extension map |
| Minecraft JSON export | own hand emitter (tabs, LF, X9/G7) | whitespace and number text must be exact (I§7) | serializer-based writer (I§31.2) |
| Resource pack | `zip` writer, entry order of I§9.1, names without leading slash | | fixed timestamps, version table (I§31.4) |
| `.mcpe` | own tolerant JSON (comments, single quotes, bare names) | `serde_json` rejects these inputs | |
| `.js`, `.jpm`, `.zox`, Stonehearth JSON | own Json.NET-compatible writer (two spaces, CRLF between lines, `": "`, NJ floats, compact primitive arrays where specified) | a generic serializer cannot reproduce the layout (I§31.12) | |
| `.java` | own template emitter | exact whitespace (I§13.2) | typed emitters per template (I§31.7) |
| OBJ import | own parser with the quirks of I§15.2 | single-space tokens, thousands separators, index rules, recursive triangulation | `tobj` |
| OBJ/MTL export | own emitter (G7, mixed line endings) | I§16.3 | LF, shortest floats (I§31.9) |
| STL | own binary writer (header padding, triangle order) | I§17; `stl_io` reads the output back in tests | ASCII option, transforms applied |
| PNG render | renderer offscreen target (7.12) | | |
| PNG sprite import | own rectangle growth algorithm (I§18.2) | exact element count and order | |
| VOX | own reader with the exact chunk walk semantics (I§19.1) and own writer | parity of SIZE/XYZI/RGBA handling | `dot_vox` for scene graphs |
| QB, QEF, binvox, CSV, Zoxel | own code | small formats with specified quirks | I§31.8 additions |
| schematic, structure NBT | `fastnbt` plus `flate2` gzip | maintained NBT codec; field order follows serde struct order; root name set with `SerOpts` | Sponge, modern palettes (I§31.8) |
| glTF/GLB | `gltf-json` types plus own GLB container | | default Sketchfab payload (I§31.10) |
| `.lnk` | own subset parser (I§28) | parity of the subset | full parser |

### 6.8 IO pipeline details

- Validation, statuses and messages exactly as I§5.2 and I§6.2, including the inverted
  "Importing"/"Loading" start statuses and the "Loading..." wait box above 307,200 bytes. Because loading
  is asynchronous the wait box is non-blocking but looks the same.
- Recent files are updated when loading starts (I§5.2), identified by full path (A§11 port rule).
- Backups (A§12.1) run the same save pipeline on a snapshot to `backup/<Weekday>_backup.cubik` in the data
  folder, every `max(300 s, interval)`, without touching the document path, title or dirty flag. A
  backup and a user save never write the same path at the same time because all writes go through the
  single IO thread in order.
- Every write (documents, backups, settings, presets, exports) uses temporary file plus rename
  (deviation D8 of A§0.2).

---

## 7. Renderer

### 7.1 Integration with egui

- Each view renders into its own **offscreen** colour texture of format `Rgba8Unorm` (egui-wgpu requires
  that format for `register_native_texture`), registered once and updated in place with
  `update_egui_texture_from_wgpu_texture` when the view is resized. The UI draws it as an image at the
  quadrant rectangle, snapped to physical pixels, with nearest filtering and a texel-to-pixel ratio of 1.
- egui prefers gamma-space framebuffers (`Rgba8Unorm`/`Bgra8Unorm`) and passes colours through
  unchanged, so the gamma-space colours of the views (A§8.1) reach the screen exactly. The app sets
  `NativeOptions::dithering = false` (egui-wgpu dithers by default, which would add noise to UI colours),
  `multisampling = 0` and `depth_buffer = 0` (egui needs neither).
- Views are encoded at the end of `App::ui` with the device and queue from `Frame::wgpu_render_state()`
  and submitted before egui's own submission, so they show the state of the same frame. Offscreen
  textures instead of paint callbacks mean that a frame in which only the UI changed (a tooltip, a hover
  fade) costs only egui's pass plus four textured quads.

### 7.2 GPU resource model

| Resource | Scope | Content |
|---|---|---|
| Element instance pools | shared | 176-byte records: opaque pool and translucent pool (7.4, 7.8) |
| Highlight instance buffer | shared | copies of hovered and selected records for the outline mask |
| Mesh buffers | shared | per mesh: positions, normals, UVs, colours, indices; per submesh ranges and material bind groups |
| Voxel face pools | shared | six pools (one per direction) of 8-byte face records (7.7) |
| Atlas | shared | 2D array texture, pages of P x P texels, nearest sampling, clamp (7.5) |
| Gizmo, marker, preview meshes | shared | static vertex buffers |
| Light and shading constants | shared | uniform block (A§8.2-8.5) |
| Camera block, frame key, sorted translucent order | per view | |
| Resolve texture (egui image) | per view | `Rgba8Unorm`, physical-pixel size of the view |
| MSAA colour and depth attachments, outline mask | **shared by all views** | sized to the largest view, size-bucketed |

Views render one after another and every view clears its attachments, so a single set of MSAA colour,
depth and mask textures serves all four views; only the resolved results are per view. At 3840x2160
(quadrants 1920x1080) with MSAA 4x this is about 33 MiB colour + 33 MiB depth + 8 MiB mask shared, plus
4 x 8 MiB resolve textures, about 106 MiB instead of about 330 MiB for separate sets. Attachments are
allocated in 64-pixel size buckets so that a window resize does not reallocate every frame.

Depth is `Depth32Float` with reversed Z (near 0.3, far 128 for orthographic views and 256 for the Main
view, A§5.1, clip planes kept exactly); depth comparisons are mirrored accordingly ("less or equal"
becomes "greater or equal").

### 7.3 Passes per view

```
1  clear colour #343434, clear depth                                         (A§5.1)
2  opaque:      voxels (alpha-tested queue, per-pixel Lambert approximation)  (A§8.5)
                binary-alpha elements and meshes hoisted from the transparent queue (7.8)
3  transparent: compiled draw list in Cubik order: per-object depth pre-pass + colour pass for
                translucent items, in-order binary items, additive sub-grid planes, background
                images, blueprint, preview decorations                     (A§8.1-8.3, 8.6)
4  overlay:     gizmos, centre marker; no depth write                        (A§8.10)
5  lines:       main grid lines, guides, hidden-element boxes, selection wireframes (A§6.4-6.5, 8.8-8.9)
6  resolve      MSAA colour into the view texture (when MSAA is on)
7  outline:     mask pass of highlighted silhouettes (depth ignored, alpha test 0.5), then a composite
                pass with the four diagonal taps at (+-2, +-2) pixels           (A§8.7)
```

Wireframe mode (A§5.9) switches the triangle pipelines to line polygon mode when the adapter offers
`POLYGON_MODE_LINE`; otherwise a barycentric edge shader draws triangle edges (pixel parity of the
fallback UNVERIFIED).

### 7.4 Elements: one instanced draw

Each element is one instance of 44 words (176 bytes):

| Words | Content |
|---|---|
| 0-2 | position `p` (f32) |
| 3 | element flags: view mask (4 bits, A§5.11), highlight (2), inverted normals (1), hidden-render mode (2) |
| 4-6 | size `s` (f32) |
| 7 | element slot (used by the outline and debugging) |
| 8-11 | rotation quaternion (f32) |
| 12-35 | per face: atlas-space `tl.x, tl.y, br.x, br.y` (f32), already clamped to the entry rectangle (M§4.6) |
| 36-41 | per face: vertex colour RGBA8 (tint emulation and element colour rules of M§4.3 and A§8.4) |
| 42-43 | per face: 8-bit state (active 1 bit, UV rotation 2 bits, atlas page 5 bits) |

- The vertex shader receives 14 instance attributes (within the limit of 16 in both the default and the
  downlevel limits of wgpu 30) and no vertex buffer: the draw is non-indexed with 36 vertices per
  instance; `vertex_index / 6` is the face slot and the corner comes from the triangle table
  `(3, 1, 0), (3, 2, 1)` of M§4.3. Constant tables hold the 24 corner positions for outer and inverted
  normals, face normals and the UV corner assignment (M§4.3, M§4.5); an inactive face collapses to a
  degenerate triangle pair.
- World position is `p + R * (corner * s)`; lighting is evaluated per vertex with the formula and light
  directions of A§8.2-8.3 (equivalent to Cubik's view-space evaluation for directional lights).
- Vertex colours are computed on the CPU per face, so toggling "Emulate elements tint colors" or "Show
  elements color" rewrites 24 bytes per element and no geometry.
- An element edit rewrites one record (176 bytes); dragging all 10,000 selected elements uploads
  1.7 MiB per frame. The opaque draw of all binary-alpha elements is one call per view.

### 7.5 Texture atlas

- Cubik packs all textures into one runtime atlas and repacks all of them on every insertion (M§5.2);
  the layout is never persisted and only rendering must match. cuboid allocates entries incrementally
  with `etagere` (1-pixel padding, clamp), so adding a texture uploads only that texture.
- Pages: a 2D array texture of pages of `P = min(adapter limit, 8192)` texels; when the textures do not
  fit one page, further pages are added (Cubik's behaviour when its atlas overflows the device limit is
  UNVERIFIED).
- Atlas-space UVs are baked into the instance records (7.4); a layout change rewrites the UV words of
  the affected elements (60,000 face rectangles for 10,000 elements take well under a millisecond).
- The image down-scaling rule for wide images (slot limit `max(512, deviceMax / 2)`, M§5.2) changes the
  stored image, so it uses a fixed reference value (16384, giving 8192) instead of the live adapter limit,
  which keeps results independent of the GPU (deviation D-P7).
- Painting writes into the texture entry's CPU image and uploads the dirty rectangle into the atlas
  each frame. Cubik paints into the atlas and cuts entries back out at stroke end (M§5.3); the visible
  result is the same except for the square brush, which in Cubik can paint across neighbouring atlas
  entries (M§13.2 quirk) and depends on Cubik's packing layout; cuboid clips it to the entry
  (deviation D-P6).

### 7.6 Meshes

Per mesh: vertex buffers (positions, normals, UVs, colours) and 32-bit indices, one draw per submesh,
one bind group per material texture (nearest sampling). Meshes are few in practice. A mesh's model
matrix is a per-draw uniform, so moving a mesh uploads 64 bytes. Mesh textures are not packed into the
atlas because mesh UVs may tile. Cubik's engine limits meshes to about 65,000 vertices (N§4.6.1); the
renderer has no such limit, the `.cubik` writer enforces it for Cubik compatibility.

### 7.7 Voxels

**Face records.** The mesher (M§7.6) emits one 8-byte record per exposed voxel face:

| Bits | Content |
|---|---|
| word 0, bits 0-11 | local x, y, z (4 bits each) |
| word 0, bits 12-29 | chunk x, y, z (6 bits each; supports up to 64 chunks per axis, R up to 341) |
| word 1, bits 0-23 | voxel RGB (alpha forced opaque for display, M§7.3) |
| word 1, bits 24-31 | AO code of the four vertices v0..v3 (2 bits each: 1.0, 0.9, 0.8; M§7.6) |

The direction is implied by the pool. The vertex shader rebuilds the quad from the corner table of M§7.6
and draws the triangles `(q, q+1, q+2), (q, q+2, q+3)`, so vertex-colour interpolation, AO gradients and
the diagonal seen in wireframe mode are identical to Cubik's unmerged quads. Compared with 4 vertices of
16 to 32 bytes plus indices, 8 bytes per face is 8 to 16 times smaller.

**Pools.** Six GPU buffers, one per direction, each with a range allocator; every allocated chunk owns
one range per direction. Freed ranges are filled with an "invalid" marker that the shader collapses, and
pools are compacted with GPU buffer copies when fragmentation exceeds a threshold.

**Culling.** Per view and per chunk, a direction is drawn only if at least one face plane of that
direction in the chunk can face the camera (orthographic: the direction faces the view direction;
perspective: the camera is on the positive side of the chunk's first plane in that direction), and only
if the chunk intersects the frustum. In the three default orthographic views, which look exactly along
an axis, five of the six directions are back-facing or edge-on and are skipped entirely; the Main view
draws at most three directions per chunk.

**Submission tiers.** Tier A (adapters with `INDIRECT_EXECUTION` and `INDIRECT_FIRST_INSTANCE`): one
`multi_draw_indirect` per pool over the ranges of visible chunks, from an indirect buffer filled on the
CPU. Tier B (GL or missing features): one draw per pool over its whole used extent, relying on GPU
back-face culling and per-pool skipping only.

**Meshing jobs.** Dirty slots are meshed on the foreground pool; a job takes pointer clones of the chunk
and its 26 neighbours, so it needs no lock and sees a consistent state. Results carry the chunk revision
they were built from and are dropped if the chunk changed meanwhile. Uploads are applied at frame start
in slot order (the deterministic upload of M§7.6) within a per-frame budget (default 8 MiB or 4 ms), the
rest on following frames. Single brush strokes touch at most a few chunks and are meshed and uploaded in
the same frame.

**Merging policy.** The editor view uses exact per-face records by default, because merged quads would
change wireframe output and AO interpolation. An optional "flat merge" (coplanar runs of identical colour
with all AO factors 1.0, slightly expanded to avoid T-junction cracks) is equivalent in shaded output by
construction and is enabled automatically only when the face count exceeds a threshold and wireframe mode
is off. Export meshing uses its own specified algorithms: the greedy boxes of M§14.3 exactly, and the OBJ
texture-mode mesher, whose algorithm the specs do not yet define (open item R-5).

**Shading and shadows.** Voxel shading uses the port approximation of A§8.5. The optional soft shadows
of A§8.2 add a two-cascade shadow-map pass from the main light, only when the preference is on.

### 7.8 Transparency: exact-equivalence batching

Cubik draws every element, mesh, preview model, sub-grid plane and background plane in one transparent
queue sorted back to front by bounding-box centre distance, each object with a depth pre-pass (alpha at
least 0.5 writes depth) followed by a blended colour pass with "less or equal" (A§8.3, A§8.6). Doing that
literally costs two draw calls per object per view (80,000 calls for 10,000 elements in four views). The
port compiles an equivalent, much shorter draw list.

Classification of each item (cached per record and texture revision):

- **B, binary**: every texel it can sample has alpha 0 or 255 and its vertex alpha is 1. Texture alpha
  comes from the summed-area table over each face's UV crop (3.5); vertex alpha from the element colour
  rules (M§4.3). For such an item the two passes produce exactly what a single alpha-tested opaque pass
  produces (alpha 0 texels change nothing, alpha 1 texels overwrite and write depth).
- **T, translucent**: may produce alpha strictly between 0 and 1.
- **X, additive**: the sub-grid planes (no depth write).
- **I, invisible**: vertex alpha 0; skipped.

Compilation per view (cached by camera and scene revision):

1. Sort the T and X items (and the decorations) by Cubik's key: distance to the camera position in
   perspective, distance along the view direction in orthographic projection. Ties keep list order;
   Cubik's tie-breaking is UNVERIFIED (deviation D-P8 covers z ties).
2. A B item is **hoisted** into the opaque pass if, for every T or X item that precedes it in Cubik's
   order (that is, lies farther away), either their screen rectangles do not overlap, or the B item lies
   entirely on the camera's side of one of that item's face planes (for a plane item, its own plane). In
   both cases the B item covers the earlier item wherever both appear, so drawing it first changes no
   pixel. Items that precede nothing translucent are always hoisted.
3. B items that cannot be hoisted stay in sequence; consecutive ones merge into one instanced draw.
4. T elements are boxes (convex, back faces culled), so one box never overlaps itself on screen.
   Consecutive T elements whose screen rectangles are pairwise disjoint share no pixel and are merged into
   one pre-pass draw plus one colour-pass draw. T meshes (not convex) are drawn one by one.
5. X items stay in sequence.

In the common case (no partial-alpha textures, camera above the floor) the transparent queue reduces to
the grid and decoration draws and every element is in one opaque draw. The cost of compilation is
O(B x T) rectangle and plane tests with T small; above 64 T items the T rectangles are bucketed in a
screen grid. A "fast transparency" preference (opt-in) hoists all B items without the tests.

### 7.9 Overlays

- Sub-grid planes: analytic shader drawing lines every displayed step with width `step / 256`, additive
  white of 0.124 per channel and a fade that imitates mip-mapping (A§6.3; calibration against
  screenshots is UNVERIFIED). Large mode scales the planes by 16.
- Main grid lines `#FFFFFF20` (side view at x = 16, A§6.4), guides, hidden-element boxes and selection
  wireframes (at most four selected objects, alpha 5/255, A§8.8) are line lists rebuilt only when they
  change.
- Background image planes: unlit, nearest, alpha-blended, back-face culled so they disappear after a side
  swap (A§7).
- Gizmos: own meshes, constant screen size with the factors of M§9.1, overlay queue without depth write.
- Outline: mask texture at full physical resolution without MSAA, composite after resolve, exactly the
  construction of A§8.7.

### 7.10 Display preview

The anchor transform of the active display mode (A§9.1, A§9.4) is a per-view model-matrix uniform
applied to all model content, so the 50 Hz floor animation and first-person anchors update one uniform
per frame and no instance data. The player rig is a set of instanced boxes using the element pipeline
with the skin texture bound instead of the atlas.

### 7.11 Frame keys and redraw policy

A view re-renders only if its frame key changed: `(scene revision, camera revision, overlay revision,
render-preference revision, size, display-mode animation time if animating)`. Hidden views (another view
maximized) are not rendered. eframe repaints on input; animations request repaints only while they run
(`request_repaint_after`); while the window is unfocused, repaints are limited to 10 per second (A§4.5).

### 7.12 PNG render and readback

The PNG render (A§15, I§18.1) uses an offscreen target of `W*aa x H*aa` with a 32-bit depth buffer and the
specified camera copy and content rules; read-back uses `map_async` polled each frame (never a blocking
wait on the UI thread); the box-filter downscale with its half-pixel offset and the PNG encoding run on
the background pool.

### 7.13 Backend tiers and failure handling

- Tier A: D3D12, Vulkan, Metal with the features named in 7.7. Tier B: GL/GLES or adapters without
  them; the same shaders, simpler submission, smaller atlas pages when `max_texture_dimension_2d` is 2048.
- Device loss and uncaptured errors are logged; on device loss the app writes a backup and offers a
  restart (re-creating the renderer inside eframe after device loss is UNVERIFIED).

---

## 8. Picking and interaction maths

### 8.1 Derived caches (`cuboid-scene`)

Structure-of-arrays caches per element, refreshed by revision: world OBB, world AABB, active face planes
and quads (outer or inverted), bounding-box centre, alpha class, resolved atlas slots. A BVH over the
AABBs (binned SAH build, about 1 ms for 10,000 boxes, parallel build above 50,000) is refit in O(n) when
only transforms change and rebuilt when the structure changes. Meshes get a per-mesh triangle BVH in
local space, built once per geometry and shared through the geometry `Arc`.

### 8.2 Ray queries

- **Elements**: Cubik picks against mesh colliders built from the active faces, and its physics queries
  report front-face hits only. The port tests the ray against each candidate's active face quads (two
  triangles each, front-facing only), which reproduces holes from disabled faces and the far-plane hits of
  inverted elements.
- **Meshes**: front-face-only triangle tests through the mesh BVH.
- **Voxels**: a 3D DDA over cells with the active-box read rule returns the hit point and face normal, and
  the inward and outward cells of M§12.2 follow from them.
- **Gizmos**: analytic hit shapes (spheres, boxes, ring meshes) with the ray length of M§8.3; the shapes
  must follow the original's gizmo colliders, whose exact data the current specs do not contain (R-4).
- Ray lengths, layers, hidden-object rules and precedence follow M§8.3-8.4.

### 8.3 Interaction state machine (`cuboid-interact`)

```rust
pub fn step(&mut self, input: &InputFrame, views: &ViewSet, cx: &mut EditCtx) -> InteractOut;
```

- Implements M§8.3-8.7 (gizmo press/hover, mode resolution, dispatch, end-drag cleanup, restriction bake),
  M§9 (drag plane, mouse drags, keyboard nudges with repeat after more than 15 fixed steps), M§10.2
  (clone mode) and A§5.4-5.5 (hover and active view latching, pan, orbit, zoom, focus).
- A fixed-step clock (50 Hz assumed, M§18 item 3) runs the specified fixed-step behaviours (hover pick,
  box-selection apply, arrow repeat counter) with an accumulator, so timing is the same at any frame
  rate. While an arrow key is held or a fixed-step behaviour is pending, the app keeps requesting repaints.
- The hover pick result is cached and recomputed only when the cursor moved or the scene revision
  changed.
- Box selection tests object origins against the box (M§8.6), a linear scan over the origin array.
- Tools (voxel, paint, colour picker) implement `Tool::stroke(&mut self, cx, phase, hit)`; the plane
  pin, same-face gate, dedupe and preview-cache rules of M§12.2 live in shared helpers.
- `InputFrame` is a plain struct built from egui input, so the state machine is unit-tested with scripted
  inputs and no window.

---

## 9. Threading and jobs

### 9.1 Threads

| Thread | Work | Notes |
|---|---|---|
| UI thread | event loop, input, interaction, ops, document ownership, egui, GPU submission | never blocks on other threads |
| foreground rayon pool | remeshing, BVH builds, extraction of large selections, short parallel kernels | `cores - 1` threads |
| background rayon pool | encode/decode, conversions, voxelization, hollow/fill, history compression, PNG encode | `max(1, cores / 2)` threads |
| IO thread | file reads and writes, ZIP deflate/inflate streaming, network (opt-in) | one thread, FIFO, so writes to one path never race |

Two pools keep a long background job from delaying remeshing during a brush stroke. Pool sizes are
configurable for benchmarking.

### 9.2 Job API

```rust
pub struct JobHandle<T> { rx: Receiver<JobEvent<T>>, cancel: Arc<AtomicBool> }
pub enum JobEvent<T> { Progress { frac: f32, status: Option<String> }, Done(Result<T, JobError>) }
```

Jobs receive immutable inputs (document snapshots, chunk pointers, byte buffers) and return values; the
UI thread polls handles in `App::logic` and applies results at the start of the frame. Workers wake the
UI with `egui::Context::request_repaint`. Interactive sessions return to the UI thread for prompts
instead of blocking (6.2).

### 9.3 Rules that rule out deadlocks

1. There is no lock around the document or any other shared mutable state; ownership plus channels only.
2. The UI thread never waits on a job; the only bounded waits are at shutdown (pending saves, with a
   visible progress dialog and a timeout).
3. A job never waits on another job of the same pool; the foreground pool never waits on the background
   pool.
4. Locks inside crates (for example the logger, 13.2) are leaf locks: never held while calling into
   rayon, sending on a channel or calling other locking code.
5. GPU read-back is `map_async` plus non-blocking polling in `logic`.

### 9.4 Determinism

Parallel kernels write into per-item output slots and results are concatenated in a fixed order (list
order for elements, slot order for chunks, column order for voxelization), so results never depend on
thread count or scheduling. Tests run key kernels with 1 and with 8 threads and compare.

---

## 10. Performance and memory budgets

### 10.1 Reference machines and scenes

| Machine | CPU | GPU | Purpose |
|---|---|---|---|
| Desktop reference | 6 to 8 cores, 2021-or-later class | mid-range discrete (GTX 1660 / RX 6600 class) | 60 fps targets |
| Laptop reference | 4 cores | integrated (Iris Xe / Radeon 680M class) | minimum targets |

| Scene | Content |
|---|---|
| S1 | 10,000 elements, random sizes, 20 % rotated, two 16x16 binary-alpha textures, four views, MSAA 4x |
| S2 | R = 128 world, model extent 256^3 cells, about 4 M voxels, about 2 M exposed faces, AO on |
| S3 | S1 and S2 together |
| S4 | stress: 256^3 checkerboard (about 8 M voxels, about 50 M faces); must stay stable, may be slow |
| S5 | 50 textures up to 1024x1024, 1,000 elements, painting with the round brush |

### 10.2 Frame budget at 60 fps (desktop reference, S3, UNVERIFIED)

| Stage | Budget |
|---|---|
| input, interaction, fixed steps | 0.5 ms |
| ops (typical frame) | 1.0 ms |
| job results applied, uploads (budgeted) | 1.5 ms |
| egui layout and tessellation | 3.0 ms |
| extraction, draw-list compilation, encoding of dirty views | 1.5 ms |
| GPU, four views (only dirty views) | 8 ms |

### 10.3 Memory per unit

| Unit | CPU | GPU |
|---|---|---|
| element | about 0.5 KiB record + 0.1 KiB derived caches | 176 B instance |
| allocated voxel chunk | 16.5 KiB (+ about 0.3 to 1.5 KiB per compressed history copy) | 8 B per exposed face |
| texture | 4 B per texel decoded (+ raw bytes if retained in a non-RGBA32 format) | 4 B per texel in the atlas |
| history entry | pointers + copies of touched records/chunks/images | none |
| baseline (no document) | target under 150 MiB resident including wgpu and fonts | view targets per 7.2 |

### 10.4 Enforcement

`criterion` benchmarks per crate (BSON encode/decode of S2 voxels, chunk meshing, picking, undo push and
restore, draw-list compilation, atlas insertion) and a headless frame benchmark; CI compares against
stored baselines on a fixed runner and fails on regressions above 10 %. A debug heads-up display (behind
`--debug`) shows CPU stage times, GPU timestamps when `TIMESTAMP_QUERY` is available, and memory counters
(document bytes, history bytes, GPU pool usage).

---

## 11. UI architecture

### 11.1 Frame pipeline and data flow

```
App::logic   poll jobs -> deferred actions -> fixed steps -> interaction state machine (uses view
             rectangles from the layout engine, which depends only on window size and layout state)
             -> apply ops -> backup and timer housekeeping
App::ui      build the whole UI from immutable state (document, selection, UI state) -> collect intents
             (actions, continuations, field edits) -> apply intents as ops -> extract render changes
             -> encode and submit dirty views -> draw view images
```

- The UI never mutates the document while building widgets; it emits intents, applied right after the
  build. This avoids borrow conflicts and keeps every mutation on the op path (undo, dirty flag, change
  log). Text fields apply live on each change, as Cubik does (L§16.6), through intents.
- Because geometry comes from the layout engine and not from egui's layout pass, `logic` knows the view
  rectangles before the UI is built, so input reaches the views with no frame of lag.

### 11.2 `cuboid-ui-kit`

- **Layout engine**: anchored rectangles (anchor min/max, offset, size, pivot), stacks with min,
  preferred and flexible sizes and the three cases of L§0.4, grids with the column formula of L§0.4, the
  9-slice squash rule, and default contributions (picture size, text size). Rectangles stay fractional;
  frame edges are snapped to physical pixels with one rounding rule (L§0.2). The engine is a set of pure
  functions with unit tests that reproduce every worked example of the specs (status cells at 900, 1000
  and 1280 px wide, tool grids at 180/220/300, tab rows, dialog button rows).
- **Skin**: the token tables of L§12.2 and the slot pictures of L§12.3 for Light, Dark and Rainbow;
  colours are resolved from the active skin at draw time (the port behaviour of L§12.5).
- **Colour-state model**: texel x base x state x multiplier (L§0.5) with 0.1 s linear cross-fades stored
  per widget id.
- **Widgets**: push button, toggle button, checkbox, tab and tab row, text field (egui `TextEdit` without
  frame, custom painted frame, placeholder, caret blink period 1/0.85 s, selection colour, content
  filters), slider, dropdown and field with list, field-style button, scroll view and scrollbars,
  virtualized list with fixed row heights, classic tooltip with the exact size and placement formulas
  (L§10.1), 9-slice frame painter (one egui mesh per frame), icon with tint and greyscale-disabled look.
- **Text**: Open Sans regular, bold and italic shipped as fonts (L§14 allows real bold and italic).
  Labels implement Cubik's overflow rule (wrap at word boundaries, drop lines that do not fit) with egui
  text wrapping limited to the number of whole lines that fit. Glyph rasterisation will differ slightly
  from Unity's (L§18 item 1).

### 11.3 Window shell (`cuboid-ui`)

- All regions of L§2 (menu bar, app area, status bar, left/right side zones with grips, tool strips,
  4-view zone) are computed by the layout engine from window size and layout state; one `geometry`
  module holds every constant from the specs.
- Panel framework: a panel declares its group, order, layout hints and a `ui(rect, cx)` function; the
  zones stack panels with the stack rules (L§8.1); edit modes select panel sets (L§2.6).
- Draw order follows L§2.1 back to front (app area, status bar, menu bar, dialogs, popups, file browser,
  overlay) using egui layers and painter order; nothing clips except scroll views (L§0.3).
- UI scale: `floor(max(96, dpi) / 96 * 2) / 2` clamped to [1.0, 1.99] (L§1.2), applied through egui's
  zoom factor relative to the OS scale, with egui's own zoom shortcuts disabled; an explicit override is
  offered as a port setting. The minimum window size is set through the viewport builder (900x700 times
  the scale, L§1.3).

### 11.4 Menus, dialogs and the file browser

- Menus are drawn from the data of 5.2 by one popup widget implementing D§1 (deferred open and close by
  one frame, hover cascades, child placement and flips, outside-press closing, disabled look).
- Dialogs share one framework implementing D§7: chrome, centring rule, drag strip with 16-pixel overhang,
  resize grip, fixed stacking order (D§7.6), Enter/Escape on key release, default focus, the single
  global modal flag (last writer wins), and the dimming layer animation (D§8.6). The message box applies
  the automatic size formula of D§8.2 and the handler mapping of D§8.4.
- The file browser model (`cuboid-filebrowser`) implements listing, sorting, filtering, selection,
  history and locations persistence; its UI implements the layout and prompts of D§10. Folder scans run
  on the IO thread and are cached by modification time.

### 11.5 4-view zone

Quadrant split, view frames, labels (text-link widget with the state colours of L§16.11), maximize on
left click, side swap on right click (A§5.3 incl. quirks), cinema bars (A§5.10), hovered/pending/active
view latching (A§5.4) and the tool cursor overlay (L§10.2). Each view displays its offscreen texture
(7.1).

### 11.6 Theming and skin packs

Built-in Light, Dark and Rainbow skins use cuboid's own art drawn to the recipes of L§13 (sizes,
borders and colour logic match; no picture comes from Cubik). An optional skin pack is a folder of PNG
files named after the picture names used in the specs, plus an optional token file; it stays on the
user's machine. Extracting pictures directly from a local Cubik installation requires a reader for
Unity 5 asset files; it is a separate, later work package whose feasibility is UNVERIFIED.

### 11.7 UI performance rules

Virtualized lists (only visible rows exist; 10,000 elements cost about 30 rows per frame); formatted
strings cached by revision; no per-frame allocation in widget code beyond egui's own; menu rows built
only while a menu is open; status-line counts maintained incrementally from the change log; layout
recomputed only when window size or layout state changes.

### 11.8 Strings

Every user-visible string of the specs is a constant in a per-domain string table, typos included
(D§0). The product name placeholder `{App}` and brand substitutions are applied at load time; the tables
are the hook for later translations.

---

## 12. Settings, persistence and paths

### 12.1 Locations

| Content | Location |
|---|---|
| settings (`settings.json`) | `ProjectDirs` config directory of cuboid |
| backups, presets, skin cache, palette image, file-browser locations | `ProjectDirs` data directory |
| logs, temporary export folders | local data or cache directory |
| legacy Cubik data (read once for import) | `%USERPROFILE%\AppData\LocalLow\BDcraft\Cubik\` on Windows |

The project directories use cuboid's own names only, never the original vendor's.

### 12.2 Settings file

- One JSON object whose keys and value formats are those of `cubikdata.json` (A§10.2), so values
  round-trip with Cubik, plus a `cuboid` sub-object for port settings (side-zone widths and docking side,
  window rectangle, compatibility switches, profiles, extension options).
- Missing keys take the "missing key" defaults, which differ from first-run defaults for a few options
  (D§9.1); unknown keys are kept and re-written.
- Written on Preferences OK, on quit, and debounced after layout changes (grip drags write on release,
  L§2.3 port note); always through temporary file plus rename. Several instances share the file and the
  last writer wins (A§3.1).

### 12.3 Legacy import

On first run, if a Cubik settings file exists, cuboid imports it, offers to copy the Cubik presets
folder, and on Windows reads the file-browser locations and marker values from the registry with
`winreg` (D§10.10, A§10.4). File association is left to the installer (A§0.2 D4);
Explorer refresh notification needs an unsafe OS call and is therefore not attempted from cuboid's code.

### 12.4 Texture signature code page

The `cs` signature (N§4.5) is the MD5 digest decoded with the Windows ANSI code page. On Windows the
app reads the active code page from the registry value that records it (safe API through `winreg`);
elsewhere the default is Windows-1252, overridable in settings. The app passes the code page number to
`cuboid-image` through the edit environment, so the image crate stays platform-independent; mapping the
number to a decoder uses `codepage` and `encoding_rs`. Signatures read from files are kept as opaque
text.

---

## 13. Errors, logging and crash safety

### 13.1 Errors

- Library crates define `thiserror` enums (`BsonError`, `ContainerError { NotZip, NoEntry, Corrupt }`,
  `ImportFailure`, `ExportFailure`, `OpError`, ...). `cuboid-io` maps them to the exact titles and texts
  of the specs (for example "Cubik model Import" / "An error occured while importing Cubik model:"), with
  `{App}` substituted.
- Parsers never panic: lengths are checked, arithmetic on sizes is checked, recursion depth is bounded,
  every slice access is bounds-checked; fuzzing enforces this (14.6).
- Ops return `Result`; a failed op that already pushed an undo entry leaves the document in a valid state
  and reports through the status line or a message box as the spec says.

### 13.2 Logging

A small own logger implements `log::Log`: a size-capped rotating file in the logs directory (5 MiB, three
files) and an in-memory ring buffer of recent lines for the debug window. `--debug` or the environment
variable `CUBOID_DEBUG=1` raises the level (A§1.4). The log never contains tokens or file contents.

### 13.3 Crash safety

- The UI thread keeps a "last snapshot" (pointer copies, refreshed when the document revision changes)
  behind a mutex used only by the panic hook. The hook takes it with `try_lock` (a panic while the lock
  is held then skips the backup instead of deadlocking), logs the panic and asks the IO thread to write
  `backup/crash_<timestamp>.cubik` from that snapshot, waiting a bounded time; if the IO thread itself
  panicked, the hook writes the file directly. No recovery prompt is
  shown at the next start, matching A§12.2; the file sits next to the regular backups (extension D-P9).
- Release builds use `panic = "unwind"` so the hook can run; worker panics are caught at job boundaries
  and reported as job errors.

---

## 14. Testing strategy

### 14.1 Unit tests

Every crate tests the rules it implements, using the parity checklists of the specs as the test list
(M§17, A§20, L§17, N§10.1, I§30): numeric rules and float text forms, BSON vectors, UV folding and
auto-UV tables, quarter turns, rotation setter and restriction bake, voxel index maps, AO factors,
flips and rotations including the R = 8 shift quirk, tool footprints, shortcut canonicalisation, layout
worked examples, message-box sizing, tooltip placement.

### 14.2 Property tests (`proptest`)

Codec round trips (random records to bytes and back), history against a naive deep-copy reference
implementation (random op sequences, then compare states after every undo and redo), voxel operation
algebra (flip twice is identity; four rotations are identity when `M = N`), layout invariants, invariant
checks after random op sequences.

### 14.3 Golden files

`tests/golden/` holds, per sample (N§10.2): the original file, the decompressed payload, the canonical
dump produced by `cubik-dump`, and an expected summary. Tests: payload identity, semantic round trip,
summary. `cargo xtask bless` regenerates outputs after review. Samples made by users of Cubik contain
absolute paths that can reveal user names (N§4.2); they are kept in a private corpus outside the public
repository, and the public corpus contains only synthetic files with neutral paths.

### 14.4 Differential tests

Our BSON against the `bson` crate (both directions); modern OBJ import against `tobj`; VOX reading
against `dot_vox`; NBT against `fastnbt` decoding; STL output read back with `stl_io`; glTF output loaded
with `gltf`.

### 14.5 Interop goldens

Each I§30.2 test id becomes a fixture with expected output files compared by the rules of I§30.1
(byte-exact text after identity normalisation, decompressed comparison for ZIP and gzip, decoded pixels
for PNG, sampled pixels for packed atlases). Emitted text files are also kept as `insta` snapshots for
readable diffs.

### 14.6 Fuzzing

`cargo-fuzz` targets (nightly toolchain, `fuzz/` crate outside the workspace and outside the unsafe
policy, since the fuzz entry points are generated by `libfuzzer-sys`): ZIP container, BSON reader,
`.cubik` decode and apply, `.cub` decode, Cubik JSON dialect, tolerant `.mcpe` JSON, Minecraft JSON
import, OBJ (both modes), VOX, QB, QEF, schematic and structure NBT, Zoxel, `.lnk`, DXT block decoding,
settings and presets JSON, file-browser locations text. Seed corpora come from the golden files. CI runs
each target for a few minutes per night; release candidates get 24-hour campaigns. Structure-aware
targets use `arbitrary` to build documents, encode them and check decode equality and invariants.

### 14.7 Renderer image tests

`cuboid-render` tests create a headless device (software adapters: WARP on Windows, lavapipe on Linux)
and render canonical scenes: white cube from the default Main camera (face values 245/219/184 on up,
south and west, A§8.3 table), tint emulation colours, transparency ordering cases (including the
intersection artefacts that Cubik shows), outline shape, grid fade, voxel AO. Exact probes check
values; full images are compared with a per-channel tolerance; every scene is also rendered with the
"literal" reference path (two draws per object in sorted order) to prove the batching of 7.8 equal pixel
for pixel.

### 14.8 UI tests

- Layout geometry tests: the layout engine dumps `(widget path, rect)` lists for the window sizes of
  L§2.1 and the modes of L§6-9, compared with tables derived from the spec text.
- `egui_kittest` snapshot tests with the `wgpu` and `snapshot` features and egui-wgpu's
  `RendererOptions::PREDICTABLE` (no dithering, predictable texture filtering), embedded fonts and fixed
  scale factors 1.0 and 1.5, Light and Dark skins. Lavapipe on Linux CI is the reference rasteriser.
- Interaction tests drive menus, dialogs and panels through kittest and assert intents and resulting
  document summaries.
- Command-log replay: recorded sequences of actions and continuations (serialisable) replayed headless,
  compared with expected summaries and checked for invariants after every step.

### 14.9 Cross-application parity

A Windows machine with Cubik 2.9.480 runs the campaigns of N§10.3 and the screenshot comparisons of
L§17 and A§20 (manual first, scripted later), feeding new golden files and resolving UNVERIFIED items.

---

## 15. Packaging and distribution

| Platform | Artefacts | Notes |
|---|---|---|
| Windows x64 | MSI (WiX) and NSIS installer via cargo-packager, portable ZIP | per-user install by default; `.cubik` association with cuboid's own ProgID and icon written by the installer (A§3.3, D4); icon and version resource via `winresource`; static CRT; optional Authenticode signing |
| macOS | `.app` and `.dmg` (cargo-packager or cargo-bundle), universal binary | document type and UTI for `.cubik`; signing and notarisation need the owner's Apple developer account |
| Linux x64 | AppImage and `.deb` (cargo-packager or cargo-deb), Flatpak manifest | `.desktop` file and shared-mime-info entry for `.cubik`; works on X11 and Wayland; file dialogs through the desktop portal |

- Releases are built with `--locked` from tagged commits by CI; archives use fixed timestamps; checksums
  are published; `cargo-dist` may automate the GitHub release.
- Third-party notices are generated by `cargo-about` and shipped with every artefact, together with the
  font licence and the licence of cuboid's own art.
- Debug info is split off and kept per release for symbolising crash reports.
- The cargo-packager file-association configuration must be confirmed during implementation
  (UNVERIFIED).

---

## 16. Licensing and provenance

**Recommendation: MIT OR Apache-2.0 for every crate and the application.**

- Every planned dependency is permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0); nothing forces
  copyleft. Optional features bring a few extra licence texts (for example the root-certificate data of
  the TLS stack used by `ureq`); the `cargo-deny` allowlist names each accepted licence explicitly.
- Dual MIT/Apache matches the egui and wgpu ecosystem, lets the codec crates (`cuboid-bson`,
  `cuboid-format-native`) be reused by other tools, and keeps contributions simple.
- Alternative if the owner prefers copyleft for the application: GPL-3.0-or-later for `apps/cuboid` only,
  libraries staying MIT OR Apache-2.0; all dependencies are compatible with that choice.
- Assets: cuboid's own icons, frames and sounds under CC-BY-4.0 (or CC0); Open Sans under the SIL Open
  Font License 1.1, shipped with its licence text; no picture, sound, font or text from Cubik Studio; the
  default player skin and sky texture are original art (A§9.5, A§9.7).
- Data tables: the legacy Minecraft id table is factual and regenerated from public data; the palette
  colour tables are an open legal question (I§33.3) and are regenerated from public sources unless the
  owner decides otherwise.
- Clean-room rules (also in `CONTRIBUTING`): Blockbench (GPL-3.0) is a study reference only; no code,
  shaders, icons or theme files are copied or closely translated. No decompiled Cubik code and no
  obfuscated identifiers ever enter the public repository; the research material stays private. Specs are
  written in own words; reviews check provenance of non-trivial algorithms.

---

## 17. Implementation plan

### 17.1 Rules for parallel work

- Each work package (WP) owns a set of directories or files; no two concurrent WPs own the same file.
- M0 creates every crate, every planned module file (empty with a doc comment) and the workspace
  `Cargo.toml` with all workspace dependencies, so later WPs never edit shared manifests or `lib.rs`
  module lists.
- Cross-crate interfaces are frozen as stubs in M0 (public types, traits and function signatures with
  `unimplemented!()` bodies). Changing a frozen interface needs a small separate interface change owned
  by the integration role.
- Domain features that span crates are split by file: `crates/cuboid-ops/src/<domain>/`,
  `crates/cuboid-ui/src/panels/<panel>.rs`, `crates/cuboid-ui/src/dialogs/<dialog>.rs`,
  `apps/cuboid/src/exec/<domain>.rs` (the action executor dispatches by domain), string tables per domain.
- Every WP ends green on `cargo clippy --workspace --all-targets --locked -- -D warnings` and
  `cargo test --workspace --locked` on Windows, Linux and macOS CI, plus its own acceptance criteria.
- One integration role per milestone merges, resolves interface requests and runs the milestone gate.

### 17.2 Milestones and work packages

**M0 Skeleton and contracts (sequential).**

| WP | Owns | Acceptance |
|---|---|---|
| WP0.1 Workspace | root `Cargo.toml`, `rust-toolchain.toml`, `.cargo/`, `deny.toml`, `about.toml`, CI workflows, all crate manifests and empty module files, `xtask` skeleton, `fuzz/` skeleton | CI green on three OS; `cargo deny check` green; every crate forbids unsafe code |
| WP0.2 Interface stubs | public signatures in all crates (model records, `ObjList`, voxel API, `History`, `EditCtx`, `Format`/`Importer`/`Exporter`, scene extraction records, render API, `ActionId`) | compiles; an interface document generated by `cargo doc` reviewed by the owner |

**M1 Foundations (parallel).**

| WP | Owns | Inputs | Acceptance |
|---|---|---|---|
| WP1.1 | `crates/cuboid-num` | M§1, N§3.2, N§4.3.2, I§3.1 | rules of 3.11 tested; float text forms tested against the I§3.1 examples |
| WP1.2 | `crates/cuboid-bson`, `fuzz/fuzz_targets/bson_*` | N§3 | N§3.5 vectors byte-exact; differential tests with `bson` 3.1; one-hour fuzz without findings |
| WP1.3 | `crates/cuboid-image` | N§4.5.1, M§5, M§13.2 | all pixel formats decode; DDS rule; packer and rasteriser tests; signature tests |
| WP1.4 | `crates/cuboid-voxel` | M§7, M§12, M§14 | M§17 item 14-16 tests; meshing benchmark (dense 4,096 chunks) recorded |
| WP1.5 | `crates/cuboid-actions` | D§1-6 | tables complete; canonicalisation and dispatch tests of D§6 |
| WP1.6 | `crates/cuboid-ui-kit` (+ `examples/gallery`) | L§0, L§12-16 | layout worked examples pass; widget gallery snapshots |
| WP1.7 | `crates/cuboid-settings` | A§10-11, D§9.1 | legacy file round trip; missing-key defaults |
| WP1.8 | `crates/cuboid-assets`, `assets/` | L§13-15 | every picture name used by the specs exists with the specified size and borders |
| WP1.9 | `crates/cuboid-filebrowser` | D§10, I§28 | listing, filters, wildcard and locations-format tests |
| WP1.10 | `crates/cuboid-textfmt` | N§9.4, I§3, I§8.6, I§10.1 | dialect accept/reject tests; Json.NET writer layout tests |

**M2 Model, history, native format, scene (parallel; WP2.1 lands its types first).**

| WP | Owns | Acceptance |
|---|---|---|
| WP2.1 | `crates/cuboid-model` | M§3-6 and M§8.1 tests; invariant checker; setters parity tests (M§17 items 1-7) |
| WP2.2 | `crates/cuboid-history` | M§15 tests; property tests against the deep-copy reference; push/restore benchmarks |
| WP2.3 | `crates/cuboid-format-native` | N§10.1 tests; payload identity on available samples; S2 open/save benchmarks recorded |
| WP2.4 | `crates/cuboid-scene` | camera and navigation constants (A§5); picking equals brute force; draw-list compiler tests |
| WP2.5 | `tools/cubik-dump`, `crates/cuboid-testkit` | dump format of N§10.2; golden harness with `bless` |

Gate G2: headless open, edit and save of `.cubik` files with payload identity and the codec budgets.

**M3 Renderer, shell, interaction (parallel).**

| WP | Owns | Acceptance |
|---|---|---|
| WP3.1 | `crates/cuboid-render` | image tests of 14.7 incl. literal-versus-batched transparency equality |
| WP3.2 | `apps/cuboid/src/{main,shell,jobs,logging,crash,platform}.rs` | start-up order of A§1.2; frame loop; jobs; backups; drag and drop; command line |
| WP3.3 | `crates/cuboid-ui/src/{shell,menubar,statusbar,zones,viewzone,tooltip}.rs` | layout dumps match L§2-7 tables; Light and Dark snapshots |
| WP3.4 | `crates/cuboid-interact` | scripted-input tests for M§17 items 9-11 and A§5.5 |
| WP3.5 | `crates/cuboid-io/src/{registry,pipeline,native,backup,messages}.rs` | Open, Merge, Save, Save As, backups, recent files with exact statuses |

Gate G3: the editor runs with elements (create, select, move, rotate, scale, undo, redo, save, load) at
60 fps on S1 on the desktop reference.

**M4 Features (parallel by domain; each WP owns its ops domain folder, its panel and dialog files and its
executor file).**

| WP | Domain | Spec |
|---|---|---|
| WP4.1 | elements: create, clone, delete, rename, colour, visibility, order, faces, textures, tint, culling, optimisation, sorting, transform dialogs; Element tools, Elements list, element popups, Element Settings General and Faces | M§10, L§8.2, L§8.5, L§9.1, D§3.2, D§5.4-5.5 |
| WP4.2 | UV mapping tab, UV view, UV tools and projections | M§10.16-10.17, L§9.1 |
| WP4.3 | textures and materials: list, popups, atlas creation, reload on focus | M§5, D§3.3-3.4, D§5.11 |
| WP4.4 | voxels: tools, popup, setup, conversions, voxelization dialog | M§7, M§12, M§14, D§3.6, D§5.12, D§9.3-9.4 |
| WP4.5 | paint and colour tools, palette and presets, colour picker | M§13, L§8.8-8.10, D§9.8 |
| WP4.6 | meshes: primitives, mesh tools, meshes list and popups | M§6, L§6.4, L§8.4, L§8.7, D§3.1 |
| WP4.7 | display settings preview, panel, presets, player skin | A§9, L§9.2 |
| WP4.8 | dialogs: Preferences, Grid size, Model Properties, prompts, message box, progress, about | D§7-9 |
| WP4.9 | file browser UI | D§10 |
| WP4.10 | clipboard, Edit menu dynamics, Recover menu, recent-files submenu | M§11, D§1.7, A§11-12 |

Gate G4: the feature checklists of A§20, M§17 and L§17 pass for every domain.

**M5 Interop formats (parallel by crate).**

| WP | Owns | Acceptance |
|---|---|---|
| WP5.1 | `crates/cuboid-format-mc` + PNG render export hook | T-JSON-1, T-JSON-2, T-RP-1, T-NUM-1 |
| WP5.2 | `crates/cuboid-format-voxel` | T-VOX-1/2, T-QB-1, T-QEF-1, T-BINVOX-1, T-SCHEM-1, T-NBT-1, T-ZOX-1, T-CSV-1, T-SH-1 |
| WP5.3 | `crates/cuboid-format-mesh` | T-OBJ-1/2/3, T-STL-1, T-PNG-1 |
| WP5.4 | `crates/cuboid-format-entity` | T-MCPE-1, T-JS-1, T-JAVA-1, T-JPM-1 |
| WP5.5 | `crates/cuboid-io/src/{interop,sketchfab}.rs` | I§4-6 flows, T-UI-1 texts and T-LNK-1; Sketchfab dialog flow |

**M6 Modern profiles (parallel).** WP6.1 glTF/GLB; WP6.2 modern Java targets and item definitions;
WP6.3 Bedrock `geo.json` and JEM; WP6.4 modern voxel (Sponge, structure export, VOX scene graph); WP6.5
network features (Sketchfab v3, opt-in update check, opt-in skin lookup). Acceptance: I§31 items with
their UNVERIFIED points resolved against official sources first.

**M7 Hardening and release.** WP7.1 budget verification and tuning on both reference machines; WP7.2
long fuzz campaigns and limit review; WP7.3 installers for three platforms; WP7.4 accessibility review
(AccessKit labels) and translation hooks; WP7.5 cross-application parity campaign (14.9).

### 17.3 Critical path

M0 -> WP1.1/1.2/1.4 -> WP2.1 -> WP2.2/2.3/2.4 -> WP3.1/3.4 -> G3. The UI kit (WP1.6) and the shell
(WP3.3) are off the critical path and can start immediately against the spec geometry.

---

## 18. Deviations and decisions needing sign-off

| Id | Cubik behaviour | Proposed port behaviour | Reason |
|---|---|---|---|
| D-P1 | saves synchronously and clears the dirty flag | asynchronous save; the dirty flag is cleared only if nothing changed after the snapshot | non-blocking UI; never loses the "unsaved" state |
| D-P2 | some imports reset the scene before parsing (I§8.1 step 3) | every import is atomic; a compatibility switch can reproduce reset-before-parse | consistent with the MUST of N§6.4 and N§9.6 |
| D-P3 | Abort in the progress dialog does nothing (D§9.11) | Abort cancels the running long operation and restores the pre-operation state | long operations run off the UI thread |
| D-P4 | undo keeps 11 entries regardless of size | 11 entries, but entries are released early above a memory cap | bounded memory with huge worlds and textures |
| D-P5 | model box changes remesh only the chunks of the first changed side (M§7.2) | all affected chunks are remeshed | same final picture, no stale seams |
| D-P6 | square brush can paint across atlas entries (M§13.2) | clipped to the entry | depends on Cubik's packing layout, which is not reproducible |
| D-P7 | wide-image down-scaling uses the device's maximum texture size (M§5.2) | fixed reference maximum 16384 | results independent of the GPU |
| D-P8 | transparent objects with equal sort keys or coplanar faces resolve in Unity's order | list-order ties; exact otherwise | Unity's tie order is unknown |
| D-P9 | no crash backup | `backup/crash_<timestamp>.cubik` written by the panic hook | crash safety, no prompt |
| D-P10 | fixed-step behaviours at the engine's step (assumed 50 Hz) | exactly 50 Hz | the original value is UNVERIFIED (M§18 item 3) |

---

## 19. Risks and open questions

| Id | Item | Plan |
|---|---|---|
| R-1 | Text rasterisation differs from Unity's (L§18 item 1) | geometry tests are authoritative; screenshot tests mask text |
| R-2 | Voxel shading approximation of spherical-harmonics lighting (A§8.5, A§21 item 2) | calibrate against Cubik screenshots |
| R-3 | Unity's quaternion-to-Euler decomposition in the last bits (N§12 item 8) | tolerance tests; retained camera states keep untouched files exact |
| R-4 | Gizmo meshes and collider shapes are not in the specs (M§18 item 15) | add a gizmo section to the model spec before WP3.4 finishes |
| R-5 | Primitive mesh geometry (M§6.2) and the OBJ texture-mode voxel mesher (I§16.2) are not specified | spec work needed before WP4.6 and WP5.3 |
| R-6 | Painting very large textures copies whole images per stroke | move images to copy-on-write tiles if benchmarks show a problem (4.4) |
| R-7 | GL tier coverage (`INDIRECT_EXECUTION`, line polygon mode) on old drivers | tier B path; test on a GL-only machine |
| R-8 | The unsafe-code lint behaviour with derive macros was inferred from `bytemuck_derive` emitting `unsafe impl` | confirm in WP0.1 with a compile check; the packing approach of 7.4 avoids the question |
| R-9 | Key mapping of non-QWERTY layouts in Cubik | test with the original on AZERTY and QWERTZ |
| R-10 | How eframe 0.37 delivers Ctrl+C/X/V and whether `rfd` dialogs can be parented from worker threads | check during WP3.2 |
| R-11 | Re-creating the renderer after GPU device loss inside eframe | check during WP3.2; fall back to backup plus restart |
| R-12 | Skin extraction from a Cubik installation needs a Unity 5 asset reader | separate optional WP after M7 |
| R-13 | All performance numbers in this document are targets | measured in WP7.1; budgets revised with data |

---

## Appendix A. Facts verified for this proposal (2026-10-18)

- Crate versions in 2.4 were checked with `cargo search` and the crates.io API; the 1.x line of
  `smallvec` is the latest stable (the 2.0 line is a beta).
- egui-wgpu 0.37.0: `Renderer::register_native_texture(device, view, filter)` requires an `Rgba8Unorm`
  texture; `update_egui_texture_from_wgpu_texture` re-binds an existing id; `RendererOptions` has
  `msaa_samples`, `depth_stencil_format`, `dithering` (default true) and
  `predictable_texture_filtering`, plus a `PREDICTABLE` preset for snapshot tests; egui prefers
  `Rgba8Unorm`/`Bgra8Unorm` framebuffers and writes gamma values unchanged to them; `CallbackTrait` has
  `prepare`, `finish_prepare` and `paint`; `RenderState` exposes adapter, device, queue, target format
  and renderer.
- eframe 0.37.0: `NativeOptions` has `multisampling`, `depth_buffer`, `stencil_buffer`, `renderer`,
  `wgpu_options`, `persist_window`, `persistence_path`, `dithering` and `clipboard_shortcuts`; the `App`
  trait has `logic` (non-UI work, called before `ui`) and `ui`.
- wgpu 30.0.1: `multi_draw_indirect` and `multi_draw_indexed_indirect` require
  `DownlevelFlags::INDIRECT_EXECUTION`; features `POLYGON_MODE_LINE`, `INDIRECT_FIRST_INSTANCE`,
  `TIMESTAMP_QUERY` exist and push constants are now `IMMEDIATES`; default limits include
  `max_texture_dimension_2d` 8192, `max_vertex_attributes` 16 and a 64 KiB uniform binding (downlevel:
  2048 and 16 KiB).
- zip 9.0.5: the `deflate-flate2` feature selects flate2 without the zopfli, zlib-rs, aes, bzip2, zstd
  and lzma code of the default set; flate2 1.1.10 defaults to the `rust_backend` (miniz_oxide).
- image 0.25.10 formats are individual features (`png`, `jpeg`, `tga`, `dds`, ...); rfd 0.17.2
  defaults to the desktop portal on Linux; egui_kittest 0.37.0 offers `wgpu` and `snapshot` features.
- fastnbt 2.6.3 serialises with a chosen root compound name (`SerOpts::root_name`,
  `to_bytes_with_opts`); dot_vox 5.2.0 exposes `load_bytes` returning models, palette, materials and
  scene nodes.
- `bytemuck_derive` 1.12.1 generates `unsafe impl` blocks for its derives.
