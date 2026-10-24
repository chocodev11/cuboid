# Spec review, round 1: completeness critic

> Internal working note of the spec workflow. It is not a specification. It points into the private research
> tree (`D:/evkey/cuboid-research`, decompiled sources and dumps) so that follow-up readers know what to read.
> Do not publish it with the repository; strip or move it before any release.

Reviewed documents (state of 2026-10-18 13:46):

| Doc | Size | Verdict |
|---|---|---|
| `formats_native.md` | 1244 lines | Very complete and mostly verified. 2 wrong statements, 1 claim backed by decoy code. |
| `formats_interop.md` | 3023 lines | Very complete. 1 unresolved conflict with `formats_native` (primitive rotation snapping). Voxel texture-mode OBJ mesher missing. |
| `model_and_tools.md` | 1861 lines | Complete for editing logic. 3 wrong statements. Texture management, primitive geometry, UV-view details and palette layout are delegated to documents that do not exist. |
| `ui_layout_theme.md` | 1752 lines | Pixel-level and verified. 1 wrong statement (number locale), 2 imprecise ones. Status bar semantics thin. |
| `ui_dialogs_menus.md` | 1404 lines | All menus, popups and dialogs of the dump are present and match the scene data row by row. Minor wording issues. |
| `app_behavior.md` | 1359 lines | Broad and mostly verified. 7 wrong or overstated statements (recent-file order, picker title, angle helper, settings defaults, browser modal flag, PNG background, token box). |

Paths below are relative to `D:/evkey/cuboid-research/` unless absolute. `SRC` = `src/Assembly-CSharp`,
`G` = `src/Assembly-CSharp/_global`, `FP` = `src/Assembly-CSharp-firstpass`.

---

## 1. Method

1. Read all six specs completely, and the six private trace files for the evidence behind them.
2. Listed every `.cs` file of the source tree (489 files) and grepped each class name against the traces to find
   classes no spec or trace accounts for (section 6).
3. Walked the UI dump top-down (`ui/ui_compact.txt`, 3206 objects) and dumped every `SubMenu` component of
   `ui/scene_tree.json` (all menu bars, popups, sub-popups and list dropdowns) and compared them with
   `ui_dialogs_menus.md` sections 2 and 3. Checked every dialog, panel and popup root of the dump against the UI
   specs.
4. Spot-checked 35 claims directly against the decompiled source, the scene data and the text assets
   (section 2). Liveness was judged as the traces do: decoys carry junk constants, impossible indices, shuffled
   UI strings, or `PlayerID.Two` input reads, and live code never calls them.
5. Extracted every message-box title and every status text literal from the source and searched for them in the
   specs, to find user-visible features that no spec mentions.

---

## 2. Spot checks (35)

| # | Claim (doc, section) | Checked against | Result |
|---|---|---|---|
| 1 | Root `.cubik` keys and order `v, mp, vp, bp, el, tx, vx, me, md`; `v` is a byte (native 4.1) | root record class `G/o129.cs` | confirmed |
| 2 | FaceRecord keys `n, a, ti, c, t, uv` and defaults; FaceUv `r, tl, br`; fold getters; [-1, 2] clamp in setters; rotation rounding (native 4.4.2-4.4.3, tools 4.5) | `G/o267.cs`, `G/o273.cs` | confirmed |
| 3 | GeneralSettings key order `o, mb, ee, ev, em, me, rer, ehe, pbt, pzf, d, n`; `"` replaced by U+201D in `d`/`n`; MinecraftSettings order (native 4.2.1-4.2.2) | `G/o650.cs`, `G/o651.cs` | confirmed |
| 4 | ViewProperties keys `ctl..cbr, gm, gk, gu, gd, gl`; grid snap not stored; defaults 0.5 / 0.125 / 0.5 / 1 (native 4.3) | `SRC/CubikApp/ViewProperties.cs`, scene `o1071` block | confirmed |
| 5 | Minecraft JSON `from`/`to`/origin/angle use the custom pattern `0.#########` (interop 3.1, 7.5) | JSON exporter `G/o1505.cs` | confirmed (pattern); 7-digit rounding still runtime-UNVERIFIED |
| 6 | Display members written as `"rotation":  [ x, y, z ]` (two spaces), default float text, no comma before the first slot (interop 7.7) | same exporter, live display writer | confirmed |
| 7 | `tintindex` only when >= 0; pre-check order type, element-only, texture warnings, rotation (interop 7.2, 7.6) | same exporter | confirmed |
| 8 | Every menu row, shortcut, A/U flag, icon, minWidth, disabled-at-start flag and hidden binding (menus 2, 3, 6.3) | all `SubMenu` and `AddCustomShortcuts` components in `ui/scene_tree.json` | confirmed, no missing or extra row |
| 9 | Held bindings: ModeClone = Alpha3, Keypad3, RightAlt, LeftAlt; RotateView = both Alt and both Ctrl; PanView = Mouse2, Space; Zoom = wheel only, sensitivity 0.1 (menus 6.4, app 5.6, tools 8.2) | scene InputManager "CubikStudio" key codes | confirmed |
| 10 | View anchors Up/Down, West/East, South/North, Main (-4,24,-20)/(45,45,0); ortho size 12; clip 0.3/128 (Main 256); FOV 64; clear `#343434` (app 5.1) | scene AppViewsManager and cameras | confirmed |
| 11 | Zoom: ortho `clamp(size - 6z, 1, 30)`; perspective step `12z` with 2-unit stop; pan 0.008 (app 5.5) | `SRC/CubikApp/AppViewsManager.cs` zoom method, serialized `_zoomSpeed` 6, `_panSpeed` 0.008 | confirmed |
| 12 | Grid dialog slider ranges 2..64 and 16..512 (menus 9.2, app 6.1) | dialog sliders in the dump | confirmed |
| 13 | Light skin button colours `#c5e7ff` / `#a5cae5` / `#dcdcdc78` (layout 12.2) | menu bar button colour blocks in the dump | confirmed |
| 14 | UI scale clamped to [1, 1.99] (layout 1.2) | canvas scale setter in `AppUIManager.cs` | confirmed |
| 15 | Side zone ranges 180..300 and 350..640; width keys read and written with different case (layout 2.2, 2.7) | `UISideHandle.cs`, `AppPanelsManager.cs` | confirmed |
| 16 | Culture: invariant for the process (app 1.2, interop 3.1, tools 1.6) versus "user's locale" (layout 16.6) | `AppManager.Awake` sets the invariant culture | invariant confirmed; layout 16.6 is wrong |
| 17 | Recent-files submenu order | submenu rebuild in `AppUIManager.cs`; `SubMenu` row insert at index 0 | newest at the top; app 11 is wrong |
| 18 | Texture-editor picker title | `Dialog_Preferences.cs` picker helper and `G/o4132.cs` executable picker | "Select executable file"; app 10.3 is wrong |
| 19 | Voxel popup "Paste voxels" undo | `SMH_PopVoxel.PasteAtBlueprint` versus `SMH_MenuEdit.Paste` | no undo entry; tools 12.4 is wrong |
| 20 | Model Properties OK clears an already-set "Restrict rotations" (menus 9.6, tools 9.5) | live OK override of `Dialog_ModelProperties.cs` | decompiled logic confirms; runtime still to check |
| 21 | Select by name matching (menus 5.3, tools 8.1) | wildcard helper `G/o4825.cs` | anchored glob, `*` and `?`, case-insensitive; closes the tools open question |
| 22 | UV view pan gesture (tools 10.16 "Ctrl-drag", layout 9.1 "middle button") | `UITextureViewCoordinates.OnDrag` | live code pans while PanView is held (middle mouse or Space); the Ctrl variant is a decoy; tools is wrong |
| 23 | Primitive meshes snap to 22.5 degrees under "Restrict rotations" (native 6.3) | `MeshManager.cs` | the flag is never set in live code for primitives (prefab value only); the trace cites two decoy variants; unverified |
| 24 | Settings defaults for missing keys (menus 9.1 versus app 10.2) | `CubikData.cs` field initializers versus `Reset()` | menus correct; app incomplete |
| 25 | Random element colour channels [0.6, 1]; Set Color clamps to >= 0.25 (tools 10.1, 10.5) | `ElementsManager.cs` | confirmed |
| 26 | Element mesh corner table, inverted variant, invert setter rebuilds with all faces active (tools 4.3) | `CubikElement.cs` mesh build | confirmed |
| 27 | Bundled display presets and the ignored `enable` key (app 9.8) | `ui/textassets/*.txt`, `ModelDisplaySettingPreset.cs` | confirmed |
| 28 | JSON (and `.cub`) face `uv` written raw without the [-1, 2] clamp (interop 8.5) | uv rect setter in `G/o273.cs` | confirmed; native 9.5 implies clamping |
| 29 | Display edits use undo category modelDisplay; the model-properties copy excludes display and overrides | `Helper_DisplaySettings.cs`, `G/o651.cs` copy | confirmed; tools 2.5 / 15.2 are wrong |
| 30 | Status bar texts (layout 4, app 19.2) | status update code in `AppUIManager.cs` | confirmed but the specs leave out formats and the Face cell |
| 31 | File browser listing order (menus 10.4 "UNVERIFIED") | file-system service `G/o5048.cs`, comparer `G/o4086.cs`/`G/o4084.cs` | folders first, then natural order (digit runs numeric, case-insensitive) |
| 32 | Hover and selection wire boxes drawn with `objectLinesWidth` (layout 12.1) | box creators in `SelectManager.cs` | creators have no callers: dead in 2.9.480 |
| 33 | Slider handle colours (layout 16.7) | all 90 sliders of the dump | 75 use `#0080ff` hover/pressed; Grid size, Voxelization, Color Picker and Progress sliders use `#f5f5f5` / `#c8c8c8` |
| 34 | `.cub` / JSON import error titles (native 9.4 "display name", interop 9.3 "CUB") | live importer `G/o2546.cs` | `"Invalid " + KEY.ToUpper()` with KEY = registry key; interop correct |
| 35 | Display Settings "Arm angle" (app 9.6 "rotates a helper gizmo only") | `Helper_AngleGO.cs`, scene `_angleOption` | rotates `arm_right` / `arm_left` of the preview player about local X; app is wrong |

---

## 3. Contradictions between documents

| # | Topic | Doc A | Doc B | Resolution |
|---|---|---|---|---|
| C1 | Number text culture | layout 16.6 and 18.17: original follows the user's locale (decimal comma) | app 1.2, interop 3.1, tools 1.6: invariant culture | B is right (spot check 16). Fix layout. |
| C2 | Recent-files submenu order | app 11: list order, oldest at the top | menus 2.1: newest at the top | B is right (17). Fix app. |
| C3 | Texture-editor picker title | app 10.3: "Select Texture Editor executable file" | menus 9.1: "Select executable file" | B is right (18). Fix app. |
| C4 | Undo of voxel popup "Paste voxels" | tools 12.4: "Paste" | menus 5.12: none | B is right (19). Fix tools. |
| C5 | Rotation snapping of primitive meshes when `rer` is on | native 6.3: primitives snap to 22.5 | interop 2.2.1: "Meshes are excluded from this branch" | Unresolved; depends on prefab data (gap G04). |
| C6 | UV view pan | tools 10.16: Ctrl-drag | layout 9.1: middle button | B is right (22); PanView also includes Space. Fix tools and its trace. |
| C7 | Select-by-name case | tools 8.1 / 18.10: UNVERIFIED | menus 5.3: case-insensitive | B is right (21). |
| C8 | One shared tool toggle group | tools 8.8: UNVERIFIED for voxel/paint | layout 8.1: verified, 22 toggles | Align tools with layout (the layout trace resolved the group). |
| C9 | Import error title for `.cub` | native 9.4: upper-cased display name | interop 9.3: `CUB` | B is right (34). |
| C10 | Third tab of Element Settings | tools 10.15, menus 5.4: "UV Mapping" | layout 9.1 and the dump: "Textures UV" | Use the UI label "Textures UV"; "UV Mapping" is only the menu wording. |
| C11 | Settings defaults | app 10.2: one default per key | menus 9.1: first-run defaults differ from missing-key defaults | menus is right (24). Fix app (gap G12). |
| C12 | Meaning of File > Import | menus 5.1: "replaces the document" | interop 5.3: replace semantics per importer (OBJ/PNG/LNK add; voxel importers re-create only the voxel world) | interop is right. Fix menus wording. |
| C13 | Modal flag and the built-in browser | app 4.6: "the built-in Save browser" | menus 10.1 / 7.3: set while the browser is open in either mode | menus is right. |
| C14 | Missing Sketchfab token box | app 16: "info box" | menus 5.8 and interop 29: Sketchfab-icon box | menus/interop right (the call uses the Sketchfab box kind). |
| C15 | "Browse last save/export Folder" before anything was saved | app 17.4: "does nothing" | menus 5.8: UNVERIFIED | Code passes the directory of a null path to the OS opener; outcome unverified (gap G21). |
| C16 | modelProperties undo category content | tools 2.5 / 15.2: includes display settings | native trace and source: the copy excludes display and overrides | Source wins (29). Fix tools. |

---

## 4. Wrong or unsupported claims (verified)

1. **layout 16.6 / 18.17**: "Number formatting follows the user's locale in the original (decimal comma on some
   systems)". The main thread runs with the invariant culture from start-up; fields parse and print with `.`.
2. **app 11**: the recent-files submenu lists "oldest at the top". Each row is inserted at index 0 while the
   stored list (oldest first) is walked, so the newest file is the top row.
3. **app 10.3**: the Texture editor picker is titled "Select Texture Editor executable file". The caption-specific
   title is dropped; the executable picker always uses "Select executable file" (filter "Executable", exe).
4. **tools 12.4**: voxel popup "Paste voxels" records undo "Paste". It selects the blueprint and pastes without
   any undo entry (only Edit > Paste records "Paste", objectsAndVoxels). Note also that it pastes whatever the
   clipboard holds, objects included.
5. **tools 10.16** (and its trace): "Ctrl-drag pans" the UV view. The live drag handler pans while PanView is
   held (middle mouse or Space); the Ctrl branch is in a decoy method.
6. **tools 2.5 and 15.2**: the modelProperties snapshot "includes display settings". The model-properties copy
   skips the display slots and the overrides; display transforms are only captured by category modelDisplay
   (64), which is what "Edit display settings" uses. Overrides are captured by no category.
7. **native 6.3** (and its trace): primitives "are restrictable" and snap to 22.5 degrees, cited from two
   `MeshManager.cs` locations that are decoy variants (bogus constants, out-of-range array writes). Live code
   never sets the flag for primitives; it comes from the prefab. Must be UNVERIFIED until the prefab data is read
   (G04). **interop 2.2.1** "Meshes are excluded" is only verified for imported meshes.
8. **native 9.4 / 9.5**: `<FORMAT>` in "Invalid <FORMAT>" is "the upper-cased format display name". It is the
   upper-cased registry key (`CUB` for `.cub`, `JSON` for `.json`).
9. **native 9.5**: `.cub` face `uv` is "converted with 4.4.3", which includes the [-1, 2] clamp. The importer writes
   the raw corner fields without clamping (same setter as JSON import); only `.cubik` loading clamps.
10. **layout 12.1**: `objectHover` is used for a "hover box", `objectSelect` for a "selection box", and
    `objectLinesWidth` is "the line width of the hover and selection boxes". The two wire-box creators have no
    callers in 2.9.480; the live consumers are the highlight outline (hover, selection) and the GL select-box
    lines (`objectSelect`). `objectLinesWidth` has no live consumer.
11. **layout 16.7**: every slider hovers/presses with `fieldFocus`. The Grid size, Voxelization, Color Picker and
    Progress sliders keep the default state colours `#ffffff` / `#f5f5f5` / `#c8c8c8` / `#c8c8c880`.
12. **app 9.6**: the helper angle "rotates a helper gizmo only". The Arm angle rotates the preview player's
    `arm_right` (right-hand page) or `arm_left` (left-hand page) about its local X axis relative to its rest
    pose, so the held model moves with the arm. It is never stored (consistent with `md.*.a` = 0).
13. **app 10.2**: the default column is presented as the defaults of the settings file. These are the first-run
    defaults; a settings file that lacks a key gets the field initializer instead (`renderHiddenElements` on,
    `objCenterOnImport` off, `advAtlasSquare` off, `appSkin`, `minecraftSkin`, path and key strings null).
14. **menus 5.1**: File > Import "replaces the document". Only JSON, `.cub` and `.mcpe` reset the model; voxel
    importers re-create only the voxel world; OBJ, PNG sprite and LNK add to the model.
15. **app 4.6**: shortcuts are suppressed while "the built-in Save browser" is open. The global modal flag is set
    for the built-in browser in both Open and Save mode.
16. **app 15** (marked [V]): "Background transparent (alpha 0)". The render camera clears depth only (background
    `#ff00ff` unused) into a freshly created render texture; the transparent result relies on the initial
    contents of that texture. Mark UNVERIFIED and let the port define a transparent clear.
17. **app 16**: the missing-token box is an "info box"; it is the Sketchfab-icon box (minor).

Resolved open questions (no defect, update the docs):

- tools 18.9: "transparent element" = any texel of a face crop with alpha < 255 (8-bit test).
- tools 18.6: "Apply Default UV" and "Paste > Tint" are dispatched by method name to handlers that do not exist
  (`UVDefaultOK`, `PasteTint`): no effect, only an engine log.
- tools 18.10: select-by-name is an anchored, case-insensitive glob (`*`, `?`).
- tools 18.12: the 22 tool toggles share one group (layout trace).
- menus 11.19 (sort): folders first, then natural name order (gap G18 for the exact comparer).
- native 12.7: kind 10 (sprite) geometry is the extrude prefab, not data from the PNG, so a reload rebuilds the
  same geometry with the stored `tx`; only the prefab mesh itself is unknown (G02).

---

## 5. Gaps (prioritized, 25)

Severity: critical = a whole feature area has no spec; high = parity or file compatibility impossible without it;
medium = visible behaviour unspecified or vague; low = detail.

### G01 (critical) Texture management has no specification

`model_and_tools` 5.1, `ui_dialogs_menus` 5.11 and `formats_interop` 7.4/8.3/12.1 all defer to a "texture
specification" that does not exist. The reader report `reports/mesh_material.md` (sections 3.9-3.13, 5.4, 8.3)
holds most of the material, but none of it reached a spec. User-visible strings found in the source and absent
from every spec include "Too many textures" / "You reached the maximum number of textures..." and "Missing
Texture editor" / "A Texture Editor is not setup...".

- Target: `model_and_tools` (new section "Texture list and texture operations").
- Files: `SRC/CubikApp/MaterialManager.cs`, `SRC/CubikApp/SMH_PopMaterial.cs`, `SRC/CubikApp/TextureListItem.cs`,
  `SRC/CubikApp/FormatCubikTexture.cs`, `SRC/CubikApp/MeshManager.cs` (mesh texture commands),
  `SRC/CubikApp/SMH_MenuObject.cs`, `G/o431.cs`, `G/o893.cs`, `G/o1358.cs`, `G/o79.cs`, `G/o3778.cs`,
  `G/o3780.cs`, `G/o1052.cs`, `reports/mesh_material.md`.
- Question: Specify, from live code only: (a) the entry model (label, path, Minecraft path / `#shortcut`,
  signature, size and weight strings, internal `error` entry, the shortcut placeholder image) and the list/atlas
  invariants; (b) every Texture popup, Material popup and "+" action with algorithm, guards, messages ("Label
  already exists", "Too many textures" and when the atlas refuses an add, "Too many unnamed textures", "Delete
  Texture", "Missing Texture editor") and undo timing; (c) label sanitising and new-name generation (`particle`,
  `texture`, `texture1`..`texture100`); (d) image loading per format (PNG/JPG resulting pixel format, DDS
  DXT1/DXT5 header rules, TGA variants, width-only slot downscale and its filter, `.mcmeta` crop) and PNG saving;
  (e) the external-editor round trip (temp file name and folder, signature test, reload on focus) and "Reload
  textures from HDD"; (f) texture-row gestures (click selects, double click opens "Select texture file", the
  options gesture) and the start-folder chain; (g) the texture image's internal name used for the sanitized PNG
  names of the Minecraft JSON export; (h) Load/Save/Clear/Remove alpha/Copy/Paste of mesh textures.

### G02 (high) Geometry of built-in meshes is unspecified

Primitive geometry is not stored in `.cubik` (native 4.6) and is rebuilt from prefabs, so rendering, painting
(UV layout), voxelization (triangle tests), OBJ export and the look of every loaded file depend on prefab meshes
that no spec describes. The same holds for the extrude/sprite prefab, the preview player rig (UV layout of the
64x64 skin, inner/outer layer offsets), the blueprint mesh, the model-origin diamond, the pointer meshes and the
sky box.

- Target: `model_and_tools` (meshes) and `app_behavior` 9.5 (player rig).
- Files: `SRC/CubikApp/MeshManager.cs` (prefab fields `_primitive*`, primitive creation), `SRC/CubikMesh.cs`,
  `SRC/CubikApp/CubikObject.cs`, `SRC/CubikApp/DisplaySettingsManager.cs`, `ui/ui_compact.txt` (World and
  DisplaySettingsPreview subtrees), and the original install `D:/evkey/CubikStudio/CubikStudio_Data` (extract
  Mesh assets and prefab MonoBehaviours with UnityPy, as done for the `app_behavior` trace).
- Question: For each of the 10 primitive prefabs (cube, sphere, cylinder, cone, pyramid, slope, corner-in,
  corner-out, torus, extrude/sprite) give vertex positions, normals, UVs, triangles, sub-meshes, local pivot,
  collider and the serialized CubikMesh/CubikObject flags (restrictable, snap-on-scale, can move/rotate/scale,
  drag sensitivity, gizmo factors, bounds); and the player rig parts with their UV rectangles and offsets. State
  the art/data policy for shipping equivalents.

### G03 (high) OBJ export of voxels in texture modes is specified nowhere

`formats_interop` 16.2 delegates "the voxel mesher (greedy when optimized) and one atlas" to the voxel
specification; `model_and_tools` 16.4 says greedy meshing "is an export concern, export specification". Neither
describes it.

- Target: `formats_interop` (section 16).
- Files: `G/o4547.cs` (voxel quad and atlas builder), `G/o5584.cs`, `G/o5585.cs`, `G/o5604.cs`, `G/o1714.cs`,
  `G/o1710.cs`, `G/o1732.cs`, `reports/g_misc_a.md`, `reports/g_export_d.md`, `reports/g_export_f.md`.
- Question: For "Detailed mesh with texture palette" (`gmt`) and "Optimized mesh with texture atlas" (`omt`),
  specify the live path only: exposure test and region scanned, quad directions and vertex order, greedy merge
  rules (axis order, equality test), chunking and vertex limits, palette image and atlas layout (cell size,
  ordering, padding with `pt`, bleeding), UV per quad, normals, part names and materials, and the interaction with
  smoothing and AO. List which builder methods are decoys.

### G04 (high) Rotation snapping of primitive meshes is contradictory and unverified

C5 above. The only evidence for "primitives snap to 22.5" is decoy code.

- Target: `model_and_tools` (3.4, 6.1); then align `formats_native` 6.3 and `formats_interop` 2.2.1.
- Files: `SRC/CubikApp/CubikObject.cs` (rotation setter), `SRC/CubikApp/MeshManager.cs` (primitive creation,
  import adoption, the two decoy adoption variants), `SRC/CubikApp/FormatCubikMesh.cs`, `G/o136.cs`, prefab data
  in the install.
- Question: What serialized value does the restrictable flag have on each primitive prefab (and the sprite
  prefab)? Hence: do primitives snap to 22.5 degrees with single-axis rejection, or to 5 degrees, in drags,
  keyboard nudges, `.cubik` load, undo restore and quarter turns?

### G05 (medium) UV editor view behaviour is incomplete

`ui_layout_theme` 9.1 points to a "UV editor spec" that does not exist; `model_and_tools` 10.16 covers only the
drag (with a wrong pan gesture, wrong claim 5). The live view also zooms with the wheel while hovered (0.5 per
wheel unit, range 0.5..4, scroll position preserved), moves the UV rectangle with the arrow keys by the UV snap
step, and on a double click (<= 0.3 s) creates a one-face-pixel rectangle at the pointer.

- Target: `model_and_tools` 10.16.
- Files: `SRC/UnityEngine.UI.Extensions/UITextureView.cs`, `SRC/UnityEngine.UI.Extensions/UITextureViewCoordinates.cs`,
  `SRC/CubikApp/Helper_ElementSettings.cs`, `SRC/CubikApp/Helper_ElementUVMapping.cs`, `SRC/FixedAtCorner.cs`,
  `SRC/FitInside.cs`, `SRC/NoScale.cs`.
- Question: Specify the live texture view: hover gating, zoom formula and limits, pan (PanView), arrow-key nudges
  (edge-triggered, step, axes, Y sign), double-click rectangle, move/resize snapping and clamping, negative
  (mirrored) rectangles, rotation and origin markers, and the exact write-back to the face (`tl`/`br`, Y flip,
  rotation kept) including when UV snap is cleared.

### G06 (medium) Palette-conversion texture layout is missing

`model_and_tools` 14.3 says the palette conversion uses "<base>_palette" with "palette image layout: UI/export
specification"; no spec has it.

- Target: `model_and_tools` 14.3 (and 7.4).
- Files: `SRC/CubikApp/VoxelManager.cs`, `SRC/CubikApp/VoxelizationManager.cs`, `G/o4034.cs`, `G/o940.cs`.
- Question: How is the "<base>_palette" image built from the world palette (size, pixels per entry, entry 0,
  row order), which UV rectangle a face gets for colour `v`, and how label clashes with existing textures are
  handled?

### G07 (medium) Data tables and palettes the port must ship are not given

`formats_interop` 2.5 and 23.4 require the VOX default palette, the Minecraft 1.12 colour palette and the legacy
id-to-name table (254 names) but list neither values nor sources; the seven colour-picker palettes (rgb256,
hsl256, jmp16, generic16, pico8, edg32, edg16) are named but not described anywhere.

- Target: `formats_interop` (2.5, 23.4, 33.3).
- Files: `G/o1932.cs` (legacy id table), `SRC/CubikApp/VoxelManager.cs` (VOX and Minecraft palette textures),
  `SRC/CubikApp/ColorManager.cs` (picker palettes), install assets (Texture2D palettes).
- Question: Extract every table and palette image (dimensions, values, ordering) into a reviewable data appendix
  and record for each whether it is public data that can be regenerated (Minecraft 1.12 ids, MagicaVoxel default,
  PICO-8, Endesga) or product art that needs an own replacement.

### G08 (medium) Default texture payloads for `.cubik` byte identity

`formats_native` 4.5/4.5.1 says the two default textures are written with mip chains, but not their size,
pixel format, pixels or mip count, so the port cannot produce byte-identical fresh-model files, and the art policy
(own look-alike images) conflicts with that goal without a stated rule.

- Target: `formats_native` (4.5.1, 8.3, 10.2).
- Files: `SRC/CubikApp/MaterialManager.cs` (default entries, reset and clear), `G/o1052.cs` (duplicate with mip
  chain), `SRC/CubikTools/SerializableTexture2D.cs`, install assets `e_particle`, `e_texture`, `e_error`,
  `e_shortcut`.
- Question: Exact `w`, `h`, `f`, mip-chain length and bytes of `particle` and `texture` in a fresh save; whether
  the port reproduces them or substitutes own images; how golden tests treat the difference.

### G09 (medium) Colour palette panel and colour fields are thin

- Target: `ui_layout_theme` 8.9-8.10 (behaviour may live in `model_and_tools` 13.4).
- Files: `SRC/CubikApp/ColorManager.cs`, `SRC/CubikApp/SMH_PopColor.cs`, `SRC/UIPalette.cs`,
  `SRC/CubikApp/ToggleGroup_ColorTools.cs`, `SRC/CubikTools/ColorSelector.cs`.
- Question: Size of each preset palette; the "Load new palette" reduction to at most 16x32 (scale or crop,
  aspect, filter); when `palette.png` is written and whether it is restored at start-up; what "Save palette"
  writes; swatch hit mapping and L/R marker placement when a colour occurs twice or is absent; the hex field
  parse and display rules and the transparent-colour cross glyph.

### G10 (medium) Gizmo meshes, hit areas and materials

`model_and_tools` 9.1 gives placement and size factors only (18.15 open). Picking parity depends on collider
shapes and sizes visible in the dump (sphere colliders r 0.3 at the arrow tips, 0.5 plane boxes, ring mesh
colliders, model-box and select-box handle spheres r 0.5).

- Target: `model_and_tools` 9.
- Files: `SRC/CubikApp/AxisNode.cs`, `SRC/CubikApp/AxisScale.cs`, `SRC/Axis.cs`, `SRC/GizmoResize.cs`,
  `SRC/CubikApp/GizmoResizeAuto.cs`, `SRC/CubikApp/GizmoHandle.cs`, `SRC/CubikApp/GizmoHandleModelBox.cs`,
  `SRC/CubikApp/GizmoHandleSelectBox.cs`, `SRC/CubikApp/HandlesBox.cs`, `ui/ui_compact.txt` (World/Gizmos,
  Handles), install meshes `axeX`, `axisY`, `axisXZ`, `RotationX..Z`, `ScaleXm..Zp`.
- Question: Shape and size of every handle, collider type/size/centre, material and colours (rest and
  hovered/active), layer per view, draw-on-top behaviour, and the scale-handle positions.

### G11 (medium) Tool toggle groups and their auto-off rules

The shared group of 22 toggles also contains the three Element-tools toggles that open Element Settings. What a
paint or voxel tool choice does to an open Element Settings panel, and when groups switch themselves off, is
only sketched (`model_and_tools` 8.8).

- Target: `model_and_tools` 8.8, 12.1, 13.1.
- Files: `SRC/CubikApp/ToggleGroup_ElementTools.cs`, `SRC/CubikApp/ToggleGroup_VoxelTools.cs`,
  `SRC/CubikApp/ToggleGroup_PaintTools.cs`, `SRC/CubikApp/ToggleGroup_ColorTools.cs`,
  `SRC/CubikApp/ToggleGroup_GizmosTools.cs`, `SRC/CubikApp/ToggleGroup_MeshTools.cs`, `SRC/CubikApp/PaintManager.cs`,
  `SRC/CubikApp/VoxelManager.cs`, `SRC/CubikApp/ColorManager.cs`, `SRC/CubikApp/SelectManager.cs`.
- Question: For each tool group: effect of switching on (custom mode, selection clearing, other panels), what
  turns it off (panel close, new primary, edit-mode change, voxel reset, dialog), and the parsing of the size
  fields (empty, decimals, limits).

### G12 (medium) Settings load: missing-key defaults and null handling

- Target: `app_behavior` 10.1-10.2.
- Files: `SRC/CubikApp/CubikData.cs`, `SRC/CubikApp/AppManager.cs` (settings load), `SRC/CubikApp/Dialog_Preferences.cs`,
  `SRC/CubikApp/AppPanelsManager.cs`.
- Question: For a `cubikdata.json` with missing or null keys, give the effective value and visible effect of every
  key (field initializer versus `Reset()`), including null paths (dialog start folders), null `appSkin` (no skin
  applied), null key strings, null `recentFiles`, and property side effects that run during deserialization.

### G13 (medium) OBJ material/texture names and hidden elements (T-OBJ-2, T-OBJ-3)

`formats_interop` 16.2/16.5 leave both UNVERIFIED although they are determinable from code.

- Target: `formats_interop` 16.
- Files: `G/o1710.cs`, `G/o1714.cs`, `G/o1732.cs`, `SRC/CubikApp/CubikElement.cs`, `SRC/CubikApp/MaterialManager.cs`,
  `SRC/CubikApp/MeshManager.cs`, `SRC/CubikApp/ElementsManager.cs`.
- Question: Which material and texture names are emitted for elements (shared material, runtime atlas), primitives,
  imported meshes, voxels in detailed and atlas mode, and are inactive (hidden) element renderers exported?

### G14 (medium) Status bar cell semantics

`ui_layout_theme` 4 lists the cells; `app_behavior` 19.2 defers the axis, rotation and face cells to a "model
spec" that does not contain them.

- Target: `ui_layout_theme` 4.
- Files: `SRC/CubikApp/AppUIManager.cs` (status and selection summary), `SRC/CubikApp/CubikElement.cs` (face name
  from a normal), `SRC/CubikApp/CubikObject.cs`, `SRC/CubikApp/SelectManager.cs` (hit normal).
- Question: Exact text and update triggers of Details, Face, X, Y, Z and Rotation: X/Y/Z are the Minecraft-space
  position (Z negated) in default float text; Rotation is the axis letter, then ": " and the Minecraft angle
  (0.1 degree) and "°" when non-zero; Face is the hovered element face computed from the hit normal in element
  space ("Down"... or "-"). Confirm, and state behaviour for meshes, blueprint, model origin and multi-selection.

### G15 (medium) Display preview: angle helpers, interaction lock, first tab

- Target: `app_behavior` 9.
- Files: `SRC/CubikApp/Helper_AngleGO.cs`, `SRC/CubikApp/Helper_DisplaySettingsContent.cs`,
  `SRC/CubikApp/Helper_DisplaySettings.cs`, `SRC/CubikApp/Helper_DisplaySettingsTabs.cs`,
  `SRC/CubikApp/DisplaySettingsManager.cs`, `SRC/CubikApp/SelectManager.cs`, `SRC/CubikApp/EditModes.cs`.
- Question: What the Arm angle and Head angle controls rotate (targets, axis, rest-pose capture and reset on tab
  switch, preset choice and panel close); which tab is active when the panel first opens; which interactions
  (picking, gizmos, nudges, paint/voxel tools, menus) are disabled while a preview mode is active (app 21.11,
  21.19).

### G16 (medium) Voxelization mode while its dialog is open

- Target: `model_and_tools` 14.1.
- Files: `SRC/CubikApp/VoxelizationManager.cs`, `SRC/CubikApp/Dialog_Voxelization.cs`, `SRC/CubikApp/MeshManager.cs`.
- Question: What entering and leaving "voxelization mode" changes in the scene (layer of source meshes,
  visibility, selection, any bounds or resolution preview), what Close does while idle versus running, and how the
  status hint and progress are restored.

### G17 (low) Tool cursor visibility and tint

`ui_layout_theme` 10.2 defers "when it is shown" to a "tools spec"; `model_and_tools` mentions only axis tints.

- Target: `model_and_tools` 8.
- Files: `SRC/CubikApp/CursorBullet.cs`, `SRC/CubikApp/SelectManager.cs`.
- Question: When is the cursor symbol visible (views only, during drags, custom tools, over UI), which sprite per
  mode, and which tint (axis colours) is applied?

### G18 (low) File browser sort order

- Target: `ui_dialogs_menus` 10.4.
- Files: `G/o5048.cs`, `G/o4084.cs`, `G/o4086.cs`.
- Question: Confirm folders first, then natural order (digit runs compared numerically, case-insensitive culture
  compare under the invariant culture, tie-break), and whether drives and location rows use the same order.

### G19 (low) List row gestures

- Target: `ui_dialogs_menus` 3 and 11.4 (rows are also in `model_and_tools` 8.9).
- Files: `SRC/CubikApp/ElementListItem.cs`, `SRC/CubikApp/MeshListItem.cs`, `SRC/CubikApp/TextureListItem.cs`,
  `FP/EnhancedUI/EnhancedScroller.cs`, list-row prefabs in the install.
- Question: Which pointer event or row button opens the Elements, Meshes and Texture popups; confirm the texture-row
  double click (load dialog) and the mesh-row Shift range failure when elements are selected; drag scrolling.

### G20 (low) Cross-document name map

The specs refer to each other by informal names ("files spec", "settings spec", "selection spec", "tools spec",
"voxelize spec", "shell spec", "views spec", "display-settings spec", "clipboard spec", "import/export spec",
"UV editor spec", "texture spec"). Two do not exist (G01, G05).

- Target: `app_behavior` (or a shared index file).
- Files: none.
- Question: Add one table mapping each informal name to a document and section, and replace the dangling ones.

### G21 (low) "Browse last save/export Folder" before any save

- Target: `app_behavior` 17.4.
- Files: `SRC/CubikApp/SMH_MenuTools.cs`, `SRC/CubikApp/FilesManager.cs`, `G/o114.cs`.
- Question: What happens with no remembered path (directory of a null path handed to the OS opener), and which
  operations set the remembered path (save, export, import, open)?

### G22 (low) Dirty flag for changes without undo

- Target: `ui_dialogs_menus` 9.6 (and `app_behavior` 2.2).
- Files: `SRC/CubikApp/Dialog_ModelProperties.cs`, `SRC/CubikApp/Dialog_GridSettings.cs`, `SRC/CubikApp/FilesManager.cs`,
  `G/o536.cs`, `G/o477.cs`.
- Question: Do Model Properties OK, Grid size, Preferences, element-settings edits and the Display Settings copy
  buttons mark the document modified? If not, state that the unsaved-changes prompt does not cover them.

### G23 (low) PNG render background

- Target: `app_behavior` 15.
- Files: `G/o1641.cs`, `SRC/CubikApp/AppViewsManager.cs`, `G/o1036.cs` (camera copy helper).
- Question: Which camera fields are copied to the render camera (clear flags, background?), and what colour and
  alpha the background has in the PNG (wrong claim 16).

### G24 (low) Element Settings texture field and face lists

- Target: `ui_dialogs_menus` 3.5 / `ui_layout_theme` 9.1.
- Files: `SRC/CubikApp/Helper_ElementFace.cs`, `SRC/CubikApp/Helper_ElementFaces.cs`,
  `SRC/CubikApp/Dialog_FacesTextures.cs`, `SRC/UnityEngine.UI.Extensions/UIInputFieldList.cs`.
- Question: Which list feeds the Faces-tab texture field and the Change Faces Textures fields (texture label list,
  refresh time), and what a chosen label does (write, notify, undo)?

### G25 (low) Inventory of unclaimed classes

The task requires every class to be covered or explicitly marked third-party/irrelevant. Section 6 lists the
classes that no trace mentions.

- Target: `app_behavior` (umbrella; the inventory itself belongs in the private trace folder).
- Files: see section 6.
- Question: For each class in section 6, record covered (spec section), third-party, dead or decoy, in a private
  trace inventory.

---

## 6. Source classes no trace mentions

Named classes (58). Classification is mine; "covered" means a spec already describes the behaviour although
the trace does not cite the file.

| Class (path under `SRC/` unless noted) | Status |
|---|---|
| `-PrivateImplementationDetails-*` (ArrayCopy*, obfuscator marker), `Properties/AssemblyInfo.cs` | compiler/obfuscator artefacts |
| `Boomlagoon.JSON/JSONArray.cs`, `JSONValueType.cs` | part of the permissive JSON parser (native 9.4, interop 8.6) |
| `CubikApp/AddCustomShortcuts.cs` | hidden shortcuts Del, F3, F2 (menus 6.3): covered |
| `CubikApp/AppSkinThemeColors.cs` | colour table (layout 12.2): covered |
| `CubikApp/CameraAnchor.cs` | view anchor record (app 5.1, 5.3): covered |
| `CubikApp/GizmoHandleSelectBox.cs`, `CubikApp/GizmoResizeAuto.cs`, `GizmoResize.cs` | gizmos: partly covered (G10) |
| `CubikApp/Helper_AngleGO.cs` | preview arm angle: wrongly described (G15) |
| `CubikApp/Helper_DisplaySettingsTabs.cs` | tab rows (layout 9.2): covered |
| `CubikApp/Helper_ElementFace.cs`, `Helper_ElementFaces.cs`, `Helper_ElementSettingsTabs.cs`, `Helper_ElementUVMapping.cs` | Element Settings tabs (tools 10.15-10.17, layout 9.1): covered, untraced (G05, G24) |
| `CubikApp/MeshListItem.cs`, `CubikApp/TextureListItem.cs` | list rows: partly covered (G01, G19) |
| `CubikApp/MessageBoxIcon.cs`, `CubikApp/RecentFile.cs`, `FileIcon.cs`, `ModelDisplayMode.cs`, `ModelDisplaySettingPreset.cs` | enums and records: covered |
| `CubikApp/ToggleGroup_ColorTools.cs`, `_GizmosTools.cs`, `_MeshTools.cs`, `_PaintTools.cs`, `_VoxelTools.cs` | tool panels: covered in parts, untraced (G11) |
| `CubikSR.FileBrowser/SfbButton.cs`, `SfbDisabledWindowOverlay.cs`, `SfbDraggable.cs`, `SfbDropMenu.cs`, `SfbDropMenuItem.cs`, `SfbEntry.cs`, `SfbEntryWrapper.cs`, `SfbLocationPanel.cs`, `SfbPromtDelete.cs`, `SfbPromtNewFolder.cs`, `SfbPromtOverwrite.cs`, `SfbPromtWarning.cs`, `SfbResizeButton.cs`, `SfbResizeable.cs`, `SfbWindow.cs` | built-in file browser (menus 10): covered, the trace cites only the core classes |
| `CubikTools/ColorSelector.cs` | colour field (layout 8.9): partly covered (G09) |
| `FitInside.cs`, `FixedAtCorner.cs`, `NoScale.cs` | UI helpers of the UV view markers (G05) |
| `MinecraftSkin.cs` | skin download (app 9.7, D2): covered |
| `ModelOverrides.cs`, `ModelOverridesCase.cs` | item overrides (interop 7.8, 8.3): covered |
| `RenderAlpha.cs` | not attached in the scene: dead |
| `UnityEngine.UI.Extensions/UITextureView.cs` | UV texture view: missing (G05) |

Obfuscated globals (62) not mentioned by any trace:

| Group | Classes (`G/`) | Status |
|---|---|---|
| Embedded JSON node library (used only for the Sketchfab reply) | o3327, o3328, o3338, o3340, o3348, o3361, o3618 | third-party-like; interop 29 covers the use |
| File-system service, entries, history, enums (file browser) | o5037-o5047 (enums, o5045), o5048, o5050, o5065, o5169, o5195, o5239, o5241, o5356 | covered by menus 10 |
| Natural-order comparer | o4084, o4086 | G18 |
| Win32 interop (drag and drop hook, window rect) | o3787, o3788, o3789, o3794, o3821, o3824, o3834, o3841 | covered by app 13 |
| Line pool and wire boxes | o1087, o1111 (hover/selection boxes dead), o1074 (IMGUI lines) | wrong claim 10 |
| Voxel quad/atlas builder for OBJ texture modes | o4547, o5584, o5585, o5604 | G03 |
| OBJ text buffer | o1732 | interop 16 (untraced) |
| DDS loader | o3778 | G01 |
| DTOs and small helpers | o600 (undo entry), o1358 (texture list wrapper), o5408, o5429 (list wrappers), o5778, o5779 (selection event), o4426 (paint dedupe key), o3135, o3186 (PNG import options and cells), o74, o1024-o1029 (tool state enums), o3619 (palette kind), o3758 (voxelization status), o5455 (axis flags), o5068 | covered implicitly |

Firstpass libraries: EnhancedUI (list virtualization), HighlightingSystem (outline, app 8.7), LibNbt (NBT),
TeamUtility.IO (input map), Unity.IO.Compression (gzip), the HSV colour picker family (menus 9.8), ObjImporter
(interop 15), Obeeware NativeHelper (native helper, unused functions), OrbCreationExtensions (no reference from the
main assembly), Beebyte attributes. All third-party; none needs its own spec beyond the uses already described.

---

## 7. UI dump coverage

Every root of the dump is accounted for:

- 3D scene: gizmos, handles, background planes, grid planes, markers, preview scenes, lights, cameras: covered by
  `app_behavior` 5-9 and `model_and_tools` 9; geometry gaps G02 and G10.
- Canvas: App (zone, left/right strips, both side zones and grips), status bar, menu bar (including the dormant
  Selection menu): covered by `ui_layout_theme`.
- Dialogs: Preferences, Grid size, Voxel Model Setup, Voxelization, PNG Import (object name Dialog_Extrusion),
  Model Properties, text prompt, vector prompt, Color Picker, Change Faces Textures, About, Message box, Register,
  Progress: all in `ui_dialogs_menus` 9 with chrome in `ui_layout_theme` 11.
- PopMenus: 8 display-preset lists, skin list, culling list, UV-face list, Meshes and Elements popups with all
  sub-popups, texture label list, Material popup and its sub-popup, Texture popup, Voxel popup with 3 sub-popups,
  Color popup: all in `ui_dialogs_menus` 3, rows identical to the scene data.
- File browser window, entries and four prompts: `ui_dialogs_menus` 10.
- Overlay: tool cursor and classic tooltip: `ui_layout_theme` 10 (visibility rules G17).

---

## 8. Notes for round 2

1. Fix the 17 wrong claims of section 4 and the contradictions C1-C16 (most are one-line edits).
2. G01 is the largest missing piece and should get a dedicated reader on `MaterialManager.cs` plus the helpers;
   the report `mesh_material.md` is a good starting point but contains at least one claim already corrected by the
   tools trace (the atlas build does clear the texture list).
3. G02, G04, G07, G08 and G10 need the original install data; the UnityPy setup described in the
   `app_behavior` trace already reads prefabs, meshes and textures.
4. The `model_and_tools` trace marks UITextureViewCoordinates as verified for "Ctrl pans"; re-check other
   UnityEngine.UI.Extensions claims in that trace with the decoy rule (in this code base, input reads with the
   second player id appeared only in decoy methods in every case checked during this review).
5. The PNG render camera copies only position, rotation, field of view, projection and orthographic size from
   the Main camera (camera copy helper in `G/o1036.cs`); its clear flags stay "depth only" from the scene, which
   is the basis of wrong claim 16 and G23.
