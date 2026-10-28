# The logo: Box<Cub> in the app

Status: 2026-10-27, owner request. The logo is a cub peeking out of an open cuboid box: the name is "cub" plus
"oid", the box is the product, and `Box<T>` is the nod to Rust. This spec puts it into the executable, the
window, the UI and the README. The cub's look (ears, eyes, fur) is still open (D-L1); nothing below depends on it,
because every picture is generated from the masters.

## 1. Today

- No icon anywhere. `cuboid-app` has no `build.rs`, so `cuboid.exe` carries the default Windows icon, and
  `cuboid-shell/src/event_loop.rs` opens the window without an icon, so Alt+Tab, the taskbar and the title bar
  of a decorated window show the generic one.
- The top bar shows the brand as the word "cuboid" in the Strong type role (`cuboid-ui/src/topbar.rs`); an
  inactive window draws it in the weak text colour (`title_bar.md` 2).
- Menus are text only (`MenuItem` has no picture). The action table keeps the picture roles `app_logo` and
  `project_logo` of the specification (`cuboid-actions/src/menus.rs`). No About dialog is drawn yet.
- `cuboid-ui` shows pictures through `ImageCache` (`common.rs`): straight RGBA that the app hands in as
  `ImageData`, uploaded with `TextureOptions::NEAREST` for pixel art.
- egui's mipmaps exist only on the glow backend. cuboid paints with egui-wgpu, so a picture drawn much smaller
  than its texture aliases.
- Own art is CC0 under the project default, which waits for the owner's approval (README, Licence).

## 2. Masters

Three hand-drawn SVG masters in `crates/cuboid-assets/brand/`, one per size band: a large logo scaled down to
16 pixels turns to mush, so the small sizes are drawn for their grid.

| Master | Grid | Rendered for | Content |
|---|---|---|---|
| `logo.svg` | 150 units | 48 px and up | isometric box (44 units per edge at 30 degrees), the opening, the cub with inner ears, eyes and nose |
| `logo-32.svg` | 32 | 24 to 40 px | front view: ears, head, eyes, nose, the box front in two tones with its rim |
| `logo-16.svg` | 16 | 16 and 20 px | the front view fitted to whole pixels: box edges and eyes on the pixel grid, no nose |

- Every master is the mark on a tile (a rounded square in `#1a1f1c`), so the logo reads on light and dark
  taskbars and in every UI theme. There is no tile-less version to keep in step.
- The colours are fixed: they follow neither the theme nor the accent the user picks. Tile `#1a1f1c` (Graphite),
  box faces `#e3bb3b` and `#a8861f`, rim `#f5d77a`, opening `#5e4a10`, eyes and nose `#1a1f1c`. Fur, inner ear
  and muzzle come from D-L1; the default is cream `#f1ead8` with inner ears `#d9c6a0`.
- The element ids `tile`, `opening`, `cub` (with `ears`, `head`, `eyes`, `nose`) and `box` stay stable; a new
  variant changes only the `cub` group.
- No text, filters, gradients, masks or external references, so every renderer draws the masters the same.

## 3. Generated files

`cargo xtask brand` (new) renders the masters into `crates/cuboid-assets/brand/`:

- `png/logo-{16,20,24,32,40,48,64,96,128,256}.png`, straight RGBA, each from the master of its band (2).
- `cuboid.ico` with the 16, 20, 24, 32, 40, 48, 64 and 256 px images.
- `manifest.txt`: the SHA-256 of every master and every output.

Rules:

- The outputs are committed and never edited by hand. A normal build needs neither the masters nor a renderer.
- resvg 0.49 (Apache-2.0 OR MIT, default features off: no text, no system fonts) and `ico` 0.6 (MIT) are
  dependencies of `xtask` only; nothing of them ships in `cuboid`.
- `cargo xtask brand --check` recomputes the hashes and fails when a master or an output changed without a new
  render. CI runs it with the other xtask checks.

## 4. Executable and window

Windows:

- `crates/cuboid-app/build.rs` (new) embeds `brand/cuboid.ico` as icon resource 1 with `winresource` 0.1 (MIT,
  build dependency), only when `CARGO_CFG_TARGET_OS` is `windows`. The same resource carries the version
  information: product name and file description "cuboid", file and product version from
  `CARGO_PKG_VERSION`, the copyright line per D-L2. `rc.exe` comes with the MSVC build tools the README
  already asks for.
- Explorer, a pinned taskbar button and the Start menu take the icon from resource 1.
- The shell sets the window icons from the same resource, so nothing is decoded and Windows picks the image
  closest to the size it asks for. With S the scale factor of the window's monitor:
  `Icon::from_resource(1, Some(16·S))` through `Window::set_window_icon` (ICON_SMALL: the title bar of a
  decorated window and Alt+Tab) and `Icon::from_resource(1, Some(32·S))` through
  `WindowExtWindows::set_taskbar_icon` (ICON_BIG). Both are set right after the window is created, once S is
  known (`open` in `event_loop.rs`), and again on a scale factor change. A failed load logs a warning and keeps
  the generic icon.

Other platforms:

- `ShellOptions` gains `icon`: RGBA pixels and their size. The app decodes `png/logo-64.png` through
  `cuboid-image` before the window opens; the shell passes it to `set_window_icon`. X11 shows it.
- Wayland and macOS ignore window icons. Their icons come with packaging (`cargo xtask package`, a stub today):
  a `.desktop` file with hicolor PNGs on Linux, an `.icns` in the app bundle on macOS.

Next to this but not part of it: `cuboid` still opens a console window on Windows (`main.rs` has no
`windows_subsystem` attribute).

## 5. In the app

- Pictures: `cuboid-assets` keeps the PNG bytes and the size table; its rule allows header reads only. The app
  decodes the rasters once at start-up through `cuboid-image` and hands them to the UI as `ImageData` in the UI
  model. The UI uploads them with `TextureOptions::LINEAR`, not through the NEAREST path of `ImageCache`.
- Size choice: a logo drawn at `p` points uses the smallest raster with at least `p·S` pixels, where S is the
  pixels per point. Up to 128 pixels no raster is shrunk by more than a third, which linear filtering handles
  without mipmaps.
- Top bar (`topbar.rs`): the brand becomes the logo at 20 points followed by the word "cuboid" (D-L3), with
  space XS between them, centred in the 32-point bar. An inactive window draws the logo at 60 % opacity next to
  the weak word. The logo is no button: a drag on it moves the window and a double-click maximizes, as on the
  empty bar. A narrow window drops the word right after the document name; the logo stays.
- First run (`first_run.rs`, the "Which app are you used to?" modal): the logo at 64 points above the question.
- Start screen (`start.rs`): the sidebar starts with the logo at 24 points, the word and the version in the weak
  colour, above "Open file". The compact sidebar keeps the logo only.
- About dialog (`docs/spec/ui_dialogs_menus.md`, when it is built): the logo at 96 points, the word, the version
  and the credits.
- Menus stay text only. `app_logo` keeps naming the role for a later icon pass; `project_logo` is no brand, the
  website row would take the kit's globe icon.

## 6. README and assets

- The README opens with `crates/cuboid-assets/brand/logo.svg` at 128 pixels and a one-line description.
- `assets/README.md` lists the brand folder.

## 7. Owner decisions

- D-L1 The cub: ears (round, small, big, square), eyes (dot, shiny, happy, pixel, sleepy), fur (cream, honey,
  cocoa, rust, grey), muzzle (with or without). Until decided: round ears, dot eyes, cream fur, no muzzle. A
  decision changes the `cub` group of the three masters, then `cargo xtask brand` renders the rest.
- D-L2 The logo's licence: (a) CC0 like the other own art; (b) excluded from the code and art licences: anyone
  may show it to refer to cuboid, not on other products or on modified builds. Recommended: (b), so a fork that
  behaves differently does not look like cuboid. The choice sets the copyright line of the version resource.
- D-L3 Top bar: logo and word (default) or the logo alone.

## 8. Work packages

- B1 Masters (done, default variant). `cargo xtask brand` with `--check`, the generated PNGs, ICO and manifest,
  the CI step, the `assets/README.md` row.
- B2 `build.rs` with the icon and version resource; window icons in `cuboid-shell` (the resource on Windows,
  RGBA elsewhere), set again on a scale change.
- B3 UI: logo rasters in the UI model, the LINEAR upload and the size choice; top bar, first run, start screen.
- B4 With packaging, later: Linux `.desktop` and hicolor PNGs, macOS `.icns`; the About dialog.

## 9. Checks

- `cargo xtask brand --check` in CI.
- `cuboid-assets` test: the header of every brand PNG (width, height, RGBA) matches its file name.
- `cuboid-shell` test on Windows: `Icon::from_resource` with a missing resource returns an error that the shell
  turns into a warning, never a panic.
- Screenshot review (the loop of `ui_v2_spec.md`): the top bar in Mocha and Latte at 100 % and 150 %, focused
  and inactive; the first run and the start screen. By hand on Windows 11, light and dark: Explorer, the taskbar
  and Alt+Tab at 100 %, 125 % and 150 %.
