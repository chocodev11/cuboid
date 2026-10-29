# Review: logo B1 to B3, the generated files, the window icons and the logo in the UI

Status: 2026-10-29. Reviewer: Opus. Scope: the uncommitted work packages B1, B2 and B3 of `docs/design/logo.md`
(sections 2 to 5, 8 and 9): `xtask/{Cargo.toml,src/brand.rs,src/main.rs,src/package.rs}`, `.github/workflows/ci.yml`,
`Cargo.lock`, `crates/cuboid-assets/{src/brand.rs,src/lib.rs,brand/cuboid.ico,brand/manifest.txt,brand/png/}`,
`crates/cuboid-shell/src/{icon.rs,contract.rs,event_loop.rs,lib.rs}`, `crates/cuboid-app/src/{ui_setup.rs,lifecycle.rs}`,
`crates/cuboid-ui-kit/{src/widgets/modal.rs,tests/components.rs}`, `crates/cuboid-ui/{Cargo.toml,src/*,tests/*}`. Not
reviewed: the coordinator's edits of `logo.md`, B4. Read against winit 0.30.13 in the cargo registry
(`platform_impl/windows/icon.rs` `from_resource_ptr`, `window.rs` `set_window_icon` and `set_taskbar_icon`), the
committed files themselves (ICO directory, PNG headers and pixels), and the screenshots in `target/ui-screens/`, eight
more of which I rendered (the editor at 150 % with the bar, the start screen with its sidebar).

Run (see Gates): `cargo xtask brand --check` (ok), `cargo xtask brand` over a backup of the folder (0 files written),
`layering` (ok), `ui-style` (ok), `cargo fmt --all -- --check` (clean), clippy of the six touched crates (clean), the
tests of cuboid-ui, cuboid-ui-kit, cuboid-shell, cuboid-assets, xtask and the `ui_setup` tests of cuboid-app (all
pass), two screenshot runs. Not run: the app on the desktop, a release build, `cargo deny` (no crate added to the lock),
the hand check on Windows 11 of `logo.md` 9.

Verdict: ready after the two minors; no blocker, no major. The three packages do what the spec asks. One renderer and
one size table serve `brand` and `package`; every size comes from the master of its band; the outputs are reproducible
(a fresh render writes nothing) and written only when they change; the ICO has the eight sizes with BMP frames below
64 and byte-identical frames where HEAD had them. The shell loads both Windows icons from resource 1 at 16·S and 32·S
right after the window is created and again on a scale change, a failure is a warning, headless is untouched, and
other platforms get the 64 px picture. The UI picks the smallest raster with at least p·S pixels, uploads it LINEAR
once and keeps it, leaves the NEAREST path of the other pictures alone, and the top bar, the first run and the start
sidebar match their sizes, gaps, opacity and drag rules, with tests that check geometry, texture options and window
requests rather than mere execution. The minors are a gap of `--check` that CI cannot see and a missing part of the
screenshot set of `logo.md` 9.

## Minor

1. `xtask/src/brand.rs:1-7, 201-224` with `.github/workflows/ci.yml:82-83`. `--check` hashes files against
   `manifest.txt`; it never renders. It catches what `logo.md` 3 names (an edited master, a hand-edited, missing or
   unlisted output, a stale manifest line; `.gitattributes` keeps the masters LF on every checkout, so the hashes
   hold on all three CI systems). It cannot see a change of the render itself: an edit of `band`, `ICO_SIZES` or
   `ICO_PNG_FROM` in `brand.rs` (a `PNG_SIZES` change does show, through `expected_names`), or a bump of resvg,
   tiny-skia, png or ico in `Cargo.lock` (a Dependabot PR). CI then stays green while `cargo xtask brand` would write
   other files, and the next `cargo xtask package`, which calls `brand::write` before the release build, rewrites the
   committed icon and PNGs without failing, so the released `cuboid.exe` embeds an icon the repository does not hold
   and the release checkout is dirty. Fix, cheapest first: a Windows-only CI step after the check,
   `cargo run --locked --package xtask -- brand` then `git diff --exit-code -- crates/cuboid-assets/brand` (the
   committed files were rendered on Windows x64, the same target as `windows-latest`, so the bytes match; the module
   doc's point about other renderers stays true for Linux and macOS); or a `renderer` line in the manifest (the resvg
   version read from `Cargo.lock`, the band limits, both size tables and `ICO_PNG_FROM`) that `--check` compares.

2. `crates/cuboid-ui/tests/screens.rs:1053-1064`. `logo.md` 9 asks for the top bar in Mocha and Latte at 100 % and
   150 %, focused and inactive. The title bar loop renders scale 1.0 only, and the inactive bar in Mocha only, so the
   set in `target/ui-screens/` has no bar at 150 % and no inactive Latte bar; `SCREENS_ONLY=titlebar` cannot redo the
   review the spec asks for. I judged 150 % from the editor screens (`editor_model_*_150_1920`, a decorated bar, same
   brand code) instead. Fix: give the loop a scale list, `[1.0, 1.5]` for `Bar::Editor` and `Bar::Inactive`, with
   `[false, true]` themes for `Bar::Inactive`, and a size that stays at or above 906 points at 150 % (1920 x 1080),
   so the 150 % picture shows the word; update the module doc (lines 12-14) to match.

## Nit

3. `README.md:58-67, 82`. CI now runs `cargo xtask brand --check`, but the list that "CI runs all of them except the
   UI style lint" does not have it, and the `xtask/` row does not name `brand`. Fix: add
   `cargo xtask brand --check   # the logo renders match their masters (logo.md 3)` to the list and `logo renders` to
   the row, as `xtask/Cargo.toml` already says.

4. `crates/cuboid-ui/src/first_run.rs:211-247`. After `let outcome = modal.show(ctx, |ui| {` the closure body is still
   indented for the old method chain (12 spaces instead of 8, the closing `});` at 8 instead of 4). rustfmt gives up on
   the expression because of the two string literals longer than the line limit (215, 246), so `cargo fmt --check` is
   clean and will never fix it. Fix: re-indent the body by hand; optionally split the long caption on line 246 into a
   `let` so rustfmt formats the closure again.

5. `xtask/src/brand.rs:45-52`. `[other, ..]` names the first argument as the unknown one, so
   `cargo xtask brand --check --force` (or the `-j 4` the alias forwards, as I did once) reports
   "unknown argument `--check`". Fix: report the first argument that is not `--check`, e.g.
   `let Some(bad) = args.iter().find(|a| *a != "--check")`, and treat `[]`/`["--check"]` as now.

6. `crates/cuboid-ui/src/common.rs:111-113` and the tests below it. `texture` now goes through `upload` with an options
   parameter, and no test pins that ordinary pictures still upload NEAREST; nor does any test show that the logo is not
   uploaded again on later frames (`the_logo_is_the_raster_for_its_size_and_is_uploaded_with_linear_filtering` counts
   one texture, which a per-frame `handle.set` would also pass). Fix: in the cache tests, assert
   `meta(id).options == TextureOptions::NEAREST` after `texture`; in `title_bar.rs`, run a few more steps and assert
   the logo's `TextureId` is unchanged and the last output's `textures_delta.set` has no entry for it.

7. `crates/cuboid-ui/src/ui.rs:176-177` with `crates/cuboid-ui/src/topbar.rs:221-226`. The doc of `set_logo` says that
   without a logo "the top bar, the start screen and the first-run dialog show the word only". The first-run dialog has
   no word, and below `WORD_MIN` a top bar without a logo shows no brand at all. Only a broken embedded file gets
   there (the app test decodes all nine), so the doc is the fix: "Without them the top bar and the start screen show
   the word only (the top bar drops it below 906 points), and the first-run dialog shows no picture." Keeping the word
   when no logo is drawn would need `common::logo` to return whether it drew; not worth it.

## The implementer's choices

1. `Modal::mark(texture, size)` in the kit modal. Accepted. The modal draws its own heading, so a hook is the only way
   to put a picture above it without copying the modal; the hook is generic (a texture and a size, no brand
   knowledge), a palette ignores it as documented, and `a_modal_draws_its_mark_square_and_above_the_heading` checks the
   size and the MD gap. The mark is left-aligned with the heading, which reads right in the screenshots.

2. The word goes below a fixed `WORD_MIN = 906` points. Accepted. A free-space rule is not strictly circular (the
   preset already measures natural widths, and the left side could be measured the same way, or the previous frame's
   free width used), but with a 900-point minimum the rule only acts between 900 and 905 points, the left side is Inter
   on every platform (the system font is only the fallback chain), and
   `the_word_never_meets_the_search_while_it_shows` fails as soon as a new menu or tab name moves the edge. The order
   of `logo.md` 5 holds: the name needs about 100 more points than the word
   (`a_narrow_window_drops_the_word_after_the_name_and_keeps_the_logo`). Cost: a decorated window or the start screen
   also loses the word in those 6 points, where it would fit.

3. The changed steps of `a_tight_bar_shrinks_the_preset_to_its_icon_and_then_drops_it`. Accepted: at 900 the dropped
   word frees enough room for the preset icon, and 860 still exercises the "gone" step even though no window gets that
   narrow. The "Tight bars" paragraph of `title_bar.md` (lines 84-86) now says more than is true at 900; that is spec
   text for the coordinator.

4. Windows icon sizes from winit's monitor `scale_factor()`. Accepted; it is what `logo.md` 4 says (S is the scale
   factor of the window's monitor). The system asks for SM_CXSMICON and SM_CXICON at the monitor's DPI, not at the
   floored canvas scale, so 20/40 at 125 % and 24/48 at 150 % are the right requests, and the ICO has those images.

5. The logo set lives in `ImageCache` and comes in once through `Ui::set_logo`. Accepted. `LogoSet` is a type of the
   UI model (`model.rs`) holding `ImageData`, as the spec says; carrying it in `UiCx` each frame would cost a field and
   a borrow for data that never changes. The cache clears its GPU map on a new epoch but keeps the set, so the logo
   comes back after a device loss.

6. The sidebar version from `env!("CARGO_PKG_VERSION")` of cuboid-ui. Accepted: `statusbar::version` already does the
   same, and the crate inherits the workspace version.

7. The stale `render_icon` mention left alone. Accepted for the implementer. Only `logo.md` 1 line 12 names it
   (`l6_release.md` 4 is the historical R3 package and names no function); the rest of `logo.md` 1 ("still opens the
   window without an icon", the word-only top bar) is also history now. The coordinator should update section 1 and
   the status line when B1 to B3 land.

## Notes for the owner (spec, not findings)

- The start screen now shows the brand twice, 32 points apart: the top bar (logo 20, XS, word at x = 32) and the
  sidebar (logo 24, SM, weak word at x = 40, the version), with "cuboid 0.1.0" a third time in the status bar
  (`start_data_latte_100_1280`). Both are what `logo.md` 5 asks; the stack reads as repetition and the two words do
  not line up. If that is not wanted: the sidebar keeps the logo and the version only, or the start screen's top bar
  drops its brand (D-L3 territory).
- The 20 px raster, which the top bar uses at 100 %, is the 16-unit master scaled by 1.25: the eyes become two-pixel
  grey smears and the box rim a half-tone row (the 24 px one, the 32-unit master at 0.75, likewise). The 16, 32 and
  larger rasters are crisp. A crisp bar at 100 % would need a 20-unit master or a 16-point bar logo (`logo.md` 2).
- In Mocha the Graphite tile almost matches the bar, so the mark reads without its tile; in Latte it is a dark tile.
  Both read well at 100 % and 150 %; the inactive logo at 60 % reads as faded, not as disabled.
- The hand check of `logo.md` 9 (Explorer, the taskbar and Alt+Tab at 100, 125 and 150 % in light and dark) is still
  open; agents give no desktop input.

## Checked and sound

- B1. `band`: 16 and 20 from `logo-16.svg`, 24 to 40 from `logo-32.svg`, 48 and up from `logo.svg` (contact sheet:
  no nose below 24, the isometric box from 48). PNGs are straight RGBA (a corner pixel of `logo-16.png` is
  (29, 32, 29, 80), the tile colour at partial alpha), all ten headers 8-bit colour type 6. `cuboid.ico`: 8 entries, 32
  bpp, BMP for 16 to 48, PNG for 64 and 256; the 16, 32, 48 and 256 frames have the byte sizes of HEAD's icon, so the
  L6 R3 images did not change. `write` compares bytes before writing and writes the manifest last. `package` calls
  `brand::write` and lost its own renderer; `sha2` was already in the lock. CI step runs on all three systems.
- `cuboid-assets` keeps bytes and a size table; the IHDR reader is test code; nothing decodes.
- Layering: cuboid-ui gains dev-dependencies on cuboid-assets (L0) and cuboid-image (L1), both below L5; the library
  itself does not depend on the brand crate. The app decodes through `cuboid_model::image`, the re-export of
  cuboid-image.
- B2. winit 0.30.13 `from_resource` calls `LoadImageW` on the executable's own module with the given size and returns
  `BadIcon::OsError` on failure; `set_window_icon` sets ICON_SMALL and `set_taskbar_icon` ICON_BIG and both keep the
  icon alive in the window state. `icon::set` runs after `create_window` with `window.scale_factor()` and again in
  `ScaleFactorChanged`; a failed load only skips the call and logs. `build.rs` embeds the icon with winresource's
  default ID 1. Headless (`headless.rs`) is untouched. The non-Windows path clones 16 KB on each scale change, which
  is nothing.
- B3. `LogoSet::pick` and `a_raster_is_never_shrunk_by_more_than_a_third` (every 0.1 px from 16 to 128). The logo
  keys (`logo:{edge}`) cannot meet the other cache keys (`texture:`, `recent:`, `lib:`). The rasters carry `rev` 1, so
  the slot is reused, and the bar draws it every frame, so `KEEP_FRAMES` never evicts it; the 64 pt first-run raster
  goes 600 to 720 frames after the dialog closes (the sweep runs every 120 frames). Straight RGBA goes through `from_rgba_unmultiplied`, so LINEAR filters
  premultiplied texels at the tile's rounded corners.
- Top bar: logo at x = 8, centre y = 16, word 4 points after it, File after the usual gap; 60 % tint alpha
  152 to 154; the logo is an `Image` with hover sense, so a drag on it reaches the drag area (a `Move` request) and
  a double-click maximizes; it is no button for accessibility. Screens: 20 px raster at 100 %, 32 px at 150 % (30
  px drawn), aligned to whole pixels at both scales.
- First run: 64 pt, above the question with MD; the 96 px raster at 150 %. At 1280 x 800 px and 150 % (853 x 533
  points, below the minimum window) the taller dialog still fits.
- Start sidebar: logo 24, word in Strong with the weak colour, version in Caption (weak by role), above "Back to the
  model" and "Open file"; the compact sidebar centres the logo alone.
- App: `decode` flips rows to top-first (the app test reads the box's left face at (48, 80) of the 128 raster), keeps
  16 to 128 only, and runs once in the `app` start-up phase; the decode test alone finishes in 0.00 s three runs out
  of three, so under 5 ms and in line with "well under a millisecond". I did not run `--trace-startup`; the before and
  after timings `logo.md` 5 asks for belong in the implementer's report. On Windows `window_icon` decodes nothing.

## Gates

| Command | Result |
|---|---|
| `cargo run -j 4 --package xtask -- brand --check` | `brand: ok` |
| `cargo run -j 4 --package xtask -- brand` (folder backed up in the scratchpad first) | `wrote 0 file(s)`, SHA-256 of all outputs unchanged |
| `cargo run -j 4 --package xtask -- layering` | ok, 28 crates |
| `cargo run -j 4 --package xtask -- ui-style` | ok |
| `cargo fmt --all -- --check` | clean (see nit 4) |
| `cargo clippy -p cuboid-ui -p cuboid-ui-kit -p cuboid-shell -p cuboid-assets -p xtask --all-targets -j 4 -- -D warnings` | clean |
| `cargo clippy -p cuboid-app --all-targets -j 4 -- -D warnings` | clean |
| `cargo test -p cuboid-ui -j 4` | 388 passed, 0 failed, 4 ignored (screens, timing) |
| `cargo test -p cuboid-assets -p cuboid-shell -p xtask -j 4 --lib --bins` | 10, 58 and 30 passed (with the three brand, two icon and five xtask brand tests) |
| `cargo test -p cuboid-ui-kit -j 4 --test components` | 40 passed |
| `cargo test -p cuboid-app -j 4 --bin cuboid ui_setup` | 7 passed; `the_logo_has_...` alone 0.00 s, three times |
| `SCREENS_ONLY=editor_model_` and `SCREENS_ONLY=start_data`, `cargo test -p cuboid-ui -j 4 --test screens -- --ignored` | 8 + 8 pictures, 8.2 s and 7.3 s |
| Pillow probes (read-only) of `cuboid.ico`, `png/*.png` and the screenshots | as above |

Disk on D: 12 GB free before, 11 GB after. Nothing of mine is left running.

## Fixes

1. Fixed, the reviewer's first fix. `.github/workflows/ci.yml`: a step "Logo renders match the renderer" after
   "Logo renders", `if: runner.os == 'Windows'`, that runs `cargo run --locked --package xtask -- brand` and then
   `git diff --exit-code -- crates/cuboid-assets/brand`. It uses `shell: bash`: the default PowerShell of a
   multi-line `run` goes on after a failing command and would only report the last one, so a failed render could
   pass. The cross-platform `brand --check` step stays. `xtask/src/brand.rs`: the module doc now says what each of
   the two steps catches (the check: masters, hand-edited, missing, unlisted outputs, a stale manifest line; the
   Windows render: a change of `band`, `ICO_SIZES`, `ICO_PNG_FROM` or of resvg, tiny-skia, png and ico in
   `Cargo.lock`) and why Linux and macOS stay with the check. On this Windows machine `cargo xtask brand` writes 0
   files, so the committed bytes match the renderer; the new step itself has not run on GitHub.

2. Fixed. `crates/cuboid-ui/tests/screens.rs`, `render_the_screen_set`: the title bar loop takes a scale and a
   size per entry: the editor and the maximized inactive window in Mocha and Latte at 100 % (1280 x 800) and at
   150 % (1920 x 1080, that is 1280 points, so the word shows); the start screen and the narrow window as before.
   The module doc is updated. `SCREENS_ONLY=titlebar` writes 11 pictures (6 before), among them
   `titlebar_{editor,inactive}_{mocha,latte}_150_1920.png` and `titlebar_inactive_latte_100_1280.png`. I looked at
   all five new ones: at 150 % the 30 px logo (the 32 px raster), the word and the menus line up as at 100 %; the
   inactive logo reads as faded in both themes, not as disabled, the word is weak and the Restore button shows.

3. Fixed. `README.md`: `cargo xtask brand --check   # the logo renders match their masters (docs/design/logo.md 3)`
   is in the check list (the comments of that block are aligned again to the longer command), and the `xtask/`
   row names `logo renders`. The row still calls `package` a stub, which it no longer is; that is not part of this
   finding and I left it.

4. Fixed. `crates/cuboid-ui/src/first_run.rs`, `show`: the closure body is indented as rustfmt wants it. The two
   long literals are now `let intro` and `let accent_note` at the top of the closure, so rustfmt formats the closure
   again; `cargo fmt -p cuboid-ui -- --check` is clean with the new layout.

5. Fixed. `xtask/src/brand.rs`, `run`: the first argument that is not `--check` is the unknown one
   (`--check --force` now reports `--force`, checked by running it); no argument or `--check` work as before.
   `--check --check` is accepted as `--check` now, where it was an error.

6. Fixed, with one change to the suggested test. `crates/cuboid-ui/src/common.rs`: the cache test
   `malformed_pictures_are_refused_and_good_ones_are_cached` asserts that `texture` uploads with
   `TextureOptions::NEAREST`. `crates/cuboid-ui/tests/title_bar.rs`: the new test
   `the_logo_is_sent_to_the_renderer_once_and_keeps_its_texture` checks that the logo texture is sent to the
   renderer once during the first frames and not again in eight more, and that its `TextureId` stays. The harness
   takes `output().textures_delta` for its renderer in every step, so that list is always empty and an assertion
   on it would pass whatever the cache does; the test rig's `frame` therefore takes the texture delta itself, keeps
   the ids in `Rig::uploads` and clears the delta (the test does not render). Both tests were checked against a
   temporary mutation, which is reverted: `texture` with LINEAR fails the first, `upload` that calls `set` on every
   frame fails the second (6 uploads against 1).

7. Fixed. `crates/cuboid-ui/src/ui.rs`, the doc of `set_logo`: "Without them the top bar and the start screen show
   the word only (the top bar drops it below 906 points), and the first-run dialog shows no picture."

8. Owner decision 2026-10-29: no brand in the start sidebar. The start screen has no brand of its own: the top bar
   above it already shows the logo and the word, and the status bar the version (`logo.md` 5, already worded so).
   `crates/cuboid-ui/src/start.rs`: removed `brand`, its call in `sidebar` (which lost its unused `images`
   parameter), the `LOGO` constant and the `logo` import; the sidebar starts with "Back to the model" when it can
   close and with "Open file" otherwise, as before the package. The top bar, the first run and the `ImageCache` logo
   set are untouched. `crates/cuboid-ui/tests/smoke.rs`: the two sidebar tests
   (`the_start_sidebar_starts_with_the_logo_the_word_and_the_version_above_open_file`,
   `the_compact_start_sidebar_keeps_the_logo_only`) became `the_start_sidebar_draws_no_logo_wide_or_compact`: with
   the logo set handed in, a start screen at 1280 and at 1000 points draws exactly one logo, the 20 point one inside
   the 32 point top bar. The first-run and "without a logo" tests stay. `crates/cuboid-ui/src/ui.rs`: the doc of
   `set_logo` no longer lists the start screen among the places that show the word without a logo (this supersedes
   the wording of fix 7). The docs of `ImageCache::logo` and `common::logo` and `tests/screens.rs` named no sidebar
   logo.
