<p align="center">
  <img src="crates/cuboid-assets/brand/logo.svg" width="128" height="128" alt="The cuboid logo: a bear cub peeking out of an open box">
</p>

<h1 align="center">cuboid</h1>

<p align="center">An open-source editor for Minecraft-style models and voxel art, written in Rust.</p>

cuboid builds models from boxes and voxels, textures and animates them, and saves them for Minecraft Java
Edition, Blockbench and Cubik Studio. It started as a clean-room successor of Cubik Studio 2.9 and keeps its file
formats; the interface is its own. On the first run it asks which app you know (Blockbench, Cubik Studio or
Blender) and sets navigation, keys and panels to match.

## Status

Pre-release: under active development, no release builds yet. The goals, in this order
([`docs/design/roadmap_lean.md`](docs/design/roadmap_lean.md)):

1. stability: no crash on any input or action, no lost work (autosave, atomic writes, recovery),
2. performance: render speed, input latency, memory, open and save times,
3. the features people use to model, voxel-paint and texture,
4. compatibility with `.cubik`, `.cub` and Minecraft Java JSON,
5. a modern interface.

## What it does

- Element models (boxes with per-face UV, colour, tint and culling) and voxel models with palettes.
- Groups with pivots, keyframe animation in an Animate mode with a timeline, and animated textures (frame
  strips and `.mcmeta`).
- An asset library (global and per project) with a project brief, and a validator for Minecraft Java block
  and item rules.
- Autosave, recovery after a crash, and atomic writes.
- `cuboid-cli` works without a window or a GPU: `dump`, `roundtrip`, `brief` and `check`.

| | Open | Save or export |
|---|---|---|
| Cubik Studio | `.cubik`, `.cub` | `.cubik` |
| Minecraft Java | block and item models (`.json`) | models, resource packs (`.zip` or a folder) |
| Blockbench | `.bbmodel`, with groups, animations and texture frames | `.bbmodel` |
| MagicaVoxel | `.vox` | `.vox` |
| Other | | Wavefront OBJ, glTF (`.gltf`, `.glb`), PNG renders |

## Building

Cargo and rustup are required. `rust-toolchain.toml` pins Rust 1.95.0 (edition 2024); the first `cargo` call in
a checkout installs it, or run `rustup toolchain install` once.

| Platform | Extra requirements |
|---|---|
| Windows | MSVC build tools; the CRT is linked statically (`.cargo/config.toml`) |
| Linux | `libasound2-dev` and `pkg-config` (audio) |
| macOS | Xcode command line tools |

```sh
cargo run --release -p cuboid-app   # the program `cuboid`
```

Checks (CI runs all of them except the UI style lint, with cargo-nextest for the tests):

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask layering        # dependency rules of architecture 3.3
cargo xtask ledger          # parity ledger of architecture 19.2 (file formats only), format in parity/README.md
cargo xtask brand --check   # the logo renders match their masters (docs/design/logo.md 3)
cargo xtask ui-style        # the UI rules of docs/design/ui_v2_spec.md
cargo deny check            # licences, advisories, bans, sources (needs cargo-deny)
```

Do not set `RUSTFLAGS` in the environment: it replaces the flags of `.cargo/config.toml`.

Fuzzing needs a nightly toolchain and cargo-fuzz: `cargo +nightly fuzz run --fuzz-dir fuzz <target>`. The
`fuzz/` crate is not a workspace member. Curated seeds live in `fuzz/corpus/<target>/` under readable names; the
hash-named inputs that cargo-fuzz generates there are not tracked.

## Layout

| Path | Content |
|---|---|
| `crates/` | 23 library crates in layers L0 to L5, the binaries `cuboid-app` (program `cuboid`) and `cuboid-cli`, and the dev-only `cuboid-testkit` |
| `crates/cuboid-assets/` | the embedded files: fonts (Inter), icons (Phosphor), data tables, and the logo masters in `brand/` |
| `xtask/` | `cargo xtask`: layering lint, parity ledger check, golden bless, UI style lint, logo renders and the Windows release package (`docs/design/l6_release.md` 4); `licences` and `bench-compare` are stubs |
| `fuzz/` | cargo-fuzz targets and their curated seeds |
| `assets/` | the asset policy |
| `parity/` | parity ledger, string catalogue and compatibility table |
| `tests/golden/` | public synthetic corpus |
| `docs/` | specifications (`spec/`) and design (`design/`) |

Every crate root forbids unsafe code (`fuzz/` is the only exception and is never shipped). Crates may depend
only on crates of lower layers; `cargo xtask layering` enforces this in CI.

## Clean room

cuboid is written from [`docs/spec`](docs/spec) and [`docs/design`](docs/design). Research material about the
original stays outside this repository, and no code, data table or generated code comes from it.

- Nothing of Cubik Studio's assets, scene data, logos or code is copied. Its pictures are used only from a user's
  own installation, as an optional local skin pack.
- Blockbench (GPL-3.0) is a learning reference only: no code, shaders, icons, themes or text are copied or
  translated.

Details: architecture section 20.3.

## Licence

- Code: `MIT OR Apache-2.0` ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)). This is the
  default chosen for the project and waits for the owner's approval (architecture section 23, decision 1).
- Own art and sounds: CC0, also waiting for approval. The logo's terms are a decision of their own
  ([`docs/design/logo.md`](docs/design/logo.md), D-L2).
- Third-party files: the Inter fonts (SIL Open Font Licence 1.1) and the Phosphor icons (MIT), listed in
  [`crates/cuboid-assets/THIRD_PARTY.md`](crates/cuboid-assets/THIRD_PARTY.md).
- The `repository` field in `Cargo.toml` is a placeholder (`OWNER`).
