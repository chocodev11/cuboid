# Parity files

Progress board and catalogues that tie the code to the specifications (architecture 19.2, 14.7, 17.2).

| Path | Content | Checked by |
|---|---|---|
| `ledger/<wp>.toml` | one file per work package: every testable requirement with its tier, status and covering tests | `cargo xtask ledger` |
| `strings.toml` | user-visible texts with their spec reference | tests of `cuboid-ui` and `cuboid-actions` |
| `compat.toml` | mirror of the `Compat` switch table | a test of `cuboid-core` |

## Ledger

The ledger is the definition of "100 %": a work package is done when all its entries are `tested`
(architecture 21.1). Update the file of your work package in the same change as the code; one file
per package keeps parallel agents from editing the same file. A file looks like this (the entry is
an illustration; `ledger/w0.5.toml` is a real one):

```toml
wp = "W1.4"

[[entry]]
id = "tools.7.6.face-order"
spec = "tools 7.6"
tier = "P0"
status = "tested"
summary = "The mesher emits the six faces of a cell in the order of the specification"
tests = ["cuboid-voxel::mesh::tests::face_order"]
```

| Key | Meaning |
|---|---|
| `wp` | Work package id such as `W1.10`. The file is named after it in lower case (`w1.10.toml`). |
| `id` | Stable id `<doc>.<section>.<slug>`: the document and section of `spec`, then a slug of lower-case letters, digits and hyphens. Unique over all files, never reused for another requirement. |
| `spec` | `<doc> <section>`, the section the requirement comes from. The document and a numbered heading with that number must exist. |
| `tier` | `P0` bit-exact, `P1` geometric, `P2` raster, `P3` perceptual, `P4` behavioural (architecture 19.1). |
| `status` | See below. |
| `summary` | The requirement in one sentence. |
| `tests` | `<package>::<test path>`, the name `cargo test -- --list` prints prefixed with the package: `cuboid-testkit::diff::tests::bytes_name_the_first_differing_offset`. Unit and integration tests; doc tests are not listed. |
| `note` | Free text. Required for `unverified-spec`. |

Documents: `arch` (`docs/design/architecture.md`), `native` (`formats_native.md`), `interop`
(`formats_interop.md`), `tools` (`model_and_tools.md`), `ui` (`ui_layout_theme.md`), `menus`
(`ui_dialogs_menus.md`) and `app` (`app_behavior.md`), the last six in `docs/spec`.

| Status | Meaning |
|---|---|
| `todo` | Not started. |
| `implemented` | The code exists, no covering test yet. |
| `tested` | Covered by the named tests, which pass in CI. `tests` must be non-empty and every test must exist. |
| `verified-o1` | `tested`, and compared with captures of the original program (oracle O1) by a maintainer. |
| `unverified-spec` | The specification is pending, contradictory or unverified here (the pending sections of architecture 21.2, every **UNVERIFIED** item). `note` names the gap. The code may exist; the entry stays until the specification is settled. |

```sh
cargo xtask ledger               # all checks; builds the test binaries of the packages the entries name
cargo xtask ledger --no-tests    # static checks and the coverage table only
cargo xtask ledger --wp W1.4     # verify the tests of one work package only
```

The check fails on malformed files, unknown keys, tiers or statuses, ids that do not start with their
`spec` reference or that repeat, references to a missing document or section, `tested` and
`verified-o1` entries without tests or with tests `cargo test` does not list, and `unverified-spec`
entries without a note. On success it prints the number of entries per document and status.

## Catalogues

`strings.toml` and `compat.toml` start with their format description. Rows are appended by the work
packages that own the texts and the switches.
