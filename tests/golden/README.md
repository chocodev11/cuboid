# Golden corpus

Public, synthetic fixtures for the golden, round-trip and differential tests (architecture 19.3).

- Every file is generated or hand-built for this repository. User files and private captures of the
  original program never enter this folder; they stay outside the repository and are reached only
  through the oracle variables `CUBOID_PRIVATE_UI_DUMP`, `CUBOID_PRIVATE_SPRITES` and
  `CUBOID_PARITY_REFS`.
- Files are compared byte for byte, so `.gitattributes` switches line-ending conversion off here.
  Files the port emits, text or binary, are stored exactly as emitted: `cubik29` text uses CRLF
  (interop 3.3) and several formats end without a final newline. Only the port's own canonical
  dumps and summaries (`payload.dump.txt`, `summary.txt`) are stored with LF line endings and a
  final newline, because the port defines them.
- A golden is changed only through `cargo xtask bless` (it runs the tests with `CUBOID_BLESS=1`),
  and the diff is reviewed like code. Goldens that still pass their tier are not rewritten.

## Layout

One folder per area, one subfolder per fixture; the name of a golden is its path below this folder
with `/` as separator.

| Path | Files | Tier |
|---|---|---|
| `cubik/<fixture>/` | `model.cubik` (the file), `payload.bin` (the decompressed payload), `payload.dump.txt` (canonical dump), `summary.txt` (expected summary), see native 10.2 | P0 |
| `<format>/<fixture>/` | the emitted file of an export format and, for binary formats, its canonical dump | P0 |
| `ui/<scene>/` | `<width>x<height>-s<scale>-<theme>.png` headless frames | P2, P3 |
| `testkit/` | the goldens of `cuboid-testkit`'s own corpus test (`dump.txt`, `gradient.png`) | P0, P2, P3 |

A test reaches a golden through `cuboid_testkit::golden::Golden::new("cubik/fresh/payload.bin")` and
compares with `expect_bytes` (emitted files, any encoding), `expect_text` (UTF-8 text, exact like
`expect_bytes` but reported by line), `expect_numbers`, `expect_raster` or `expect_ssim`. Pictures
that need an approved image per software adapter add the adapter to the file name.
