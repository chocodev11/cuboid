# Cuboid

Cuboid is an open-source, clean-room reimplementation of Cubik Studio 2.9.480 in Rust. The goals, in
this order:

1. 100 % parity of the user interface and of behaviour with the original,
2. maximal backward compatibility: `.cubik` and legacy `.cub` files, and every import and export
   format of the original, byte for byte where the specifications demand it,
3. maximal performance and stability: no panic on input, every parser fuzzed, a UI thread that never
   waits.

## Status

Pre-alpha. The foundations of milestone M0 exist: the workspace with its CI (format, clippy, tests,
layering lint, parity ledger check, cargo-deny), the numerics crate, the golden test kit and the parity
ledger. Most crates are still skeletons and the programs only print their version. The plan, with 66
work packages and their file ownership, is in
[`docs/design/architecture.md`](docs/design/architecture.md) section 21.

## Clean room

Implementers work from [`docs/spec`](docs/spec) and [`docs/design`](docs/design) only. Specification
writers may read private research material about the original; it stays outside this repository, and
no code, data table or generated code comes from it. Private oracles run only on maintainers' machines.

- Nothing of Cubik Studio's assets, scene data, logos or code is copied. Its user-visible texts appear
  only where parity requires them, each with its specification reference and the brand substituted
  (architecture section 14.7). Its pictures are used only from a user's own installation, as an
  optional local skin pack.
- Blockbench (GPL-3.0) is a learning reference only: no code, shaders, icons, themes or text are
  copied or translated.
- Questions about the original's behaviour go back to the specification writers, and the answers
  land in `docs/spec`.

Details: architecture section 20.3.

## Licence

Code: `MIT OR Apache-2.0` ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)). **This is
the default chosen for the project and is pending the owner's approval** (architecture section 23,
decision 1), as is CC0 for the project's own art and sounds. The shipped Open Sans fonts use the SIL
Open Font Licence 1.1. The `repository` field in `Cargo.toml` is a placeholder (`OWNER`).

## Building

Cargo and rustup are required. `rust-toolchain.toml` pins Rust 1.95.0 (edition 2024); the first
`cargo` call in a checkout installs it, or run `rustup toolchain install` once.

| Platform | Extra requirements |
|---|---|
| Windows | MSVC build tools; the CRT is linked statically (`.cargo/config.toml`) |
| Linux | `libasound2-dev` and `pkg-config` (audio) |
| macOS | Xcode command line tools |

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask layering          # dependency rules of architecture 3.3
cargo xtask ledger            # parity ledger of architecture 19.2, format in parity/README.md
cargo deny check              # licences, advisories, bans, sources (needs cargo-deny)
```

Do not set `RUSTFLAGS` in the environment: it replaces the flags of `.cargo/config.toml`.

Fuzzing needs a nightly toolchain and cargo-fuzz: `cargo +nightly fuzz run --fuzz-dir fuzz <target>`.
The `fuzz/` crate is not a workspace member. Curated seeds live in `fuzz/corpus/<target>/` under
readable names; the hash-named inputs that cargo-fuzz generates there are not tracked.

## Layout

| Path | Content |
|---|---|
| `crates/` | 20 library crates in layers L0-L5, the binaries `cuboid-app` (program `cuboid`) and `cuboid-cli`, and the dev-only `cuboid-testkit` |
| `xtask/` | `cargo xtask`: layering lint, parity ledger check and golden bless; licences, package and bench-compare are still stubs |
| `fuzz/` | cargo-fuzz targets and their curated seeds |
| `assets/` | own source art, fonts, sounds and data tables |
| `parity/` | parity ledger, string catalogue and compatibility table |
| `tests/golden/` | public synthetic corpus |
| `docs/` | specifications (`spec/`) and design (`design/`) |

Every crate root forbids unsafe code (`fuzz/` is the only exception and is never shipped). Crates may
depend only on crates of lower layers; `cargo xtask layering` enforces this in CI.
