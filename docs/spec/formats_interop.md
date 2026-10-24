# Interop formats: import and export specification

Status: draft for implementation and parity testing.
Reference product: Cubik Studio 2.9.480 Beta (Windows build).
Target: cuboid, a clean-room Rust reimplementation.

This document specifies every non-native file format the reference product reads or writes, the shared
pipeline around them, and the modernization work the port should add on top. The native project
formats (`.cubik` and the legacy `.cub`) are listed in the registry for completeness. Their payload is
specified in the native project format specification, except where the legacy `.cub` reader shares the
Minecraft JSON reader (section 9.3).

## Contents

1. Scope, terms and profiles
2. Shared definitions (spaces, elements, faces, voxels, palette, colours, settings)
3. Global I/O rules (numbers, rounding, encodings, line endings, JSON writers, containers)
4. Format registry
5. Import pipeline
6. Export pipeline
7. Minecraft Java block or item model (`.json`), export
8. Minecraft Java block or item model (`.json`), import
9. Minecraft resource pack (`.zip`), Cubik alpha (`.cub`) and display presets
10. Bedrock legacy geometry import (`.mcpe`)
11. Bedrock legacy entity export (`.js`)
12. Entity texture atlas (shared by `.js` and `.java`)
13. Java entity model source (`.java`)
14. OptiFine part model (`.jpm`)
15. Wavefront OBJ import (`.obj` with `.mtl`)
16. Wavefront OBJ export (`.obj`, `.mtl`, `.png`)
17. STL export (`.stl`)
18. PNG render export and PNG sprite import (`.png`)
19. MagicaVoxel (`.vox`)
20. Qubicle binary (`.qb`)
21. Qubicle exchange (`.qef`)
22. binvox (`.binvox`)
23. MCEdit schematic (`.schematic`) and the colour-to-block map
24. Minecraft structure (`.nbt`)
25. Zoxel (`.zox`)
26. Sproxel CSV (`.csv`)
27. Stonehearth (`.stonehearth`) and Staxel (`.staxel`)
28. Windows shortcut (`.lnk`)
29. Sketchfab upload
30. Parity test plan
31. Modernization
32. Compatibility matrix
33. Unverified items and open questions
34. Data appendix: legacy tables and palettes

---

## 1. Scope, terms and profiles

### 1.1 Normative terms

MUST, SHOULD and MAY have their usual meaning. "Cubik" means the reference product. "The port" means
cuboid.

### 1.2 Output profiles

Every exporter in the port takes an output profile.

- `cubik29` (the default) MUST reproduce the reference output as specified here. That includes the
  reference product's quirks, unless a quirk is listed with the label **[FIX-ALWAYS]**.
- `modern-*` profiles are opt-in. Section 31 defines them. They MAY change layout, fix defects marked
  **[FIX-MODERN]**, and add features.

Importers SHOULD accept a superset of what the reference product accepts. Where the reference product
interprets valid input in a surprising way, the importer has two modes: `cubik29`, the default, keeps
the reference interpretation, and `modern` uses the corrected one. Each such case is labelled
**[IMPORT-MODE]**.

**[FIX-ALWAYS]** applies only to behaviour that would crash, hang, corrupt memory or produce a file the
reference product itself cannot read back. It never applies to valid output that merely looks odd.

### 1.3 Confidence markers

Unmarked statements were verified against the reference implementation. **UNVERIFIED** marks a
statement that could not be verified by reading the implementation, or that depends on engine
behaviour that was not executed. Section 33 collects them.

*(spot-checked)* marks the few passages taken from a secondary analysis, where only the entry points
and key constants were re-verified.

---

## 2. Shared definitions

### 2.1 Coordinate spaces and units

- **Editor space (E).** The reference editor runs in a left-handed, Y-up space: X right, Y up, Z
  forward. One Minecraft block is 16 units.
- **Model space (M).** Editor space with Z negated: `M(x, y, z) = (x, y, -z)`. Java block-model JSON
  coordinates are model-space coordinates.
- **The block.** The default model box is the block `M = [0,16] x [0,16] x [0,16]`, which is
  `E = [0,16] x [0,16] x [-16,0]`.
- **Rotation matrices.** Rotations use Unity semantics. A rotation by angle `t` about +X maps
  `(x, y, z)` to `(x, y·cos t - z·sin t, y·sin t + z·cos t)`. About +Y it maps to
  `(x·cos t + z·sin t, y, -x·sin t + z·cos t)`. About +Z it maps to
  `(x·cos t - y·sin t, x·sin t + y·cos t, z)`.
- **Euler order.** `Euler(a, b, c)` applies Z first, then X, then Y, so `R = Ry(b)·Rx(a)·Rz(c)`.
- **Mirroring between E and M.** When a rotation stored in E is expressed in M, the X and Y angles
  change sign and the Z angle keeps its sign.

### 2.2 Element transform data

Each element (a textured box, or a plane-type box) has these fields.

| Field | Meaning |
|---|---|
| `c` | Stored corner position in E. It is the minimum corner in X and Y and the maximum corner in Z. The element box spans `[c.x, c.x+s.x] x [c.y, c.y+s.y] x [c.z-s.z, c.z]` in E before rotation. |
| `s` | Size. Each axis is at least 0.001 because the setter clamps it. |
| `q` | Pivot in E. |
| `e` | Euler rotation in degrees, stored normalized (2.2.1). |
| label | Free text. It is never sanitized. |
| hidden | Hidden flag. |
| shade | Default true. |
| rescale | Default false. |
| invert | Default false. It swaps "from" and "to" on Minecraft JSON export (7.5). |
| type | Cube 0, PlaneX 1, PlaneY 2, PlaneZ 3. |

When an element is rotated, the editor rotates the whole transform about the pivot. The stored corner
`c` is therefore the rotated corner, not the unrotated one. Exporters that need the unrotated box
compute it (2.2.2).

#### 2.2.1 Rotation storage rule

When a rotation `v` is assigned to an element:

1. Normalize each component: `v_i := (v_i + 360) mod 360`, using a remainder that keeps the sign of
   the dividend, as in C#. Inputs below -360 therefore stay negative. This edge case was not tested.
2. Snap the angles. Let `restrict` be the model setting "Restrict rotations" (`rer`, section 2.7).
   - If `restrict` is true and the object's own restrict flag is set, each component snaps to the
     nearest multiple of 22.5, using `roundHE(v/22.5)·22.5`.
   - Otherwise each component snaps to `roundHE(v/5)·5`.
   - The per-object flag is set by default in code. Adopting an imported mesh clears it (15.4), so
     imported meshes always snap to 5. Elements and primitive meshes are created from prefabs, and
     the flag value stored in those prefabs is not visible in the analysed material. This document
     assumes elements keep the default (snap to 22.5), as the native format specification does for
     elements and primitives. Both cases are **UNVERIFIED** (T-JSON-2); the two specifications must
     be aligned once one is resolved.
   - `roundHE` is round-half-to-even (3.2). Snapping to 5 therefore maps 22.5 to 20 and 67.5 to 70.
3. A value of 360 can survive snapping, because 357.5 or more can round up to 360. It is stored as is.
4. Rotate the transform about the pivot by the new rotation, after removing the old one. This step
   yields the stored corner `c`.

Interactive editing has one more branch: when `restrict` is true and the object's flag is set, it
refuses rotations on more than one axis. Importers bypass that branch, so imported multi-axis
rotations are kept and snapped.

#### 2.2.2 Derived quantities used by exporters

- `f(a) = a - 360` if `a > 180`, otherwise `a`.
- `e_M = (-f(e.x), -f(e.y), f(e.z))` is the rotation in model space.
- `corner_M = (c.x, c.y, -c.z)` and `pivot_M = (q.x, q.y, -q.z)`.
- `unrot_M = round4( Euler(-e_M) · (corner_M - pivot_M) + pivot_M )`. Here `round4(x)` rounds each
  component with `roundHE(x·10000)·0.0001`, in single precision.
- Rotated: `e != (0,0,0)` under approximate vector equality, which counts a vector as zero when its
  squared length is below 1e-10. The axis tests below compare each component exactly.
- Axis letter: `x` if `e.x != 0`, else `y` if `e.y != 0`, else `z` if `e.z != 0`, else `-`.
- Axis angle: the component of `e_M` on that axis, rounded with `roundHE(a·10)/10`.
- Invalid for Minecraft (export is refused) when any of these holds:
  1. Two or more components of `e` are non-zero.
  2. Some component lies strictly between 45 and 315.
  3. Some component is not an exact multiple of 22.5, tested as `roundHE(a/22.5)·22.5 != a` on the
     stored value.

  The valid stored values per axis are therefore 0, 22.5, 45, 315 and 337.5.

### 2.3 Face data and UVs

Each element has six faces, always in this order: down, up, north, south, west, east. Each face has
these fields.

| Field | Default | Notes |
|---|---|---|
| enabled | true | A disabled face is culled. |
| texture label | `texture` | Names a texture entry by label. |
| cullface | none | Bit values: down 1, up 2, north 4, south 8, west 16, east 32, any or all 63. |
| tintindex | -1 | |
| UV rotation | 0 | Stored as `((roundHE(r/90)·90) + 360) mod 360`. |
| `tl`, `br` | (0,0), (0,0) | Normalized UV corners in the face's texture, Y up. The setters clamp each component to [-1, 2]. |

The reported corners are wrapped. A reader of `tl` and `br` sees these values:

- `tl.x`. If the raw `tl.x < 0` and `br.x > 0`, the value is 0. Otherwise, if `tl.x < 0`, add 1.
  Otherwise, if `tl.x > 1`, or `tl.x = 1` and `br.x > 1`, subtract 1. `tl.y` follows the same rule
  with the Y components.
- `br.x`. If the raw `tl.x < 1` and `br.x > 1`, the value is 1. Otherwise, if `br.x > 1`, subtract 1.
  Otherwise, if `br.x < 0`, or `br.x = 0` and `tl.x < 0`, add 1. `br.y` follows the same rule with the
  Y components.

The 16-unit forms that exporters use are derived from the wrapped values.

- `uv0 = 16·(tl.x, 1 - br.y)` and `uv1 = 16·(br.x, 1 - tl.y)`. In both, Y is measured from the top.

A face rectangle given in normalized top-down coordinates `(x, y, w, h)` maps to storage as
`tl = (x, 1-(y+h))` and `br = (x+w, 1-y)`.

### 2.4 Voxel world

| Symbol | Meaning |
|---|---|
| `N` | Resolution, in voxels across the 16-unit block. The setup dialog allows 2 to 128. Importers may use other values (each section says which). |
| `G = 3N` | Grid extent per axis. The block occupies the middle third. |
| `C = N` | Index of the block's minimum corner. |
| `s = 16/N` | Voxel edge length, in world units. |

- **Grid to world** (voxel corner): `x_w = (gx - C)·s`, `y_w = (gy - C)·s`, `z_w = (C - gz)·s`. Grid
  Z runs opposite to editor Z, so grid Z increases with model-space Z.
- **World to grid** for a point:
  `g = (roundHE((x_w+16)/s), roundHE((y_w+16)/s), roundHE(-(z_w-16)/s))`. Some tools use truncation
  instead of rounding. The voxel specification owns those tools.
- **Write region.** The model box, converted to grid indices with the rule above, defines a region
  `[min, max)` per axis. Voxel writes outside the region are ignored. Voxel reads outside it return 0.
- **Voxel bounds for export.** Every voxel exporter exports the write region (the model box), not the
  tight bounds of the voxels. All sizes below are `max - min` of the write region.
- **The region follows the box.** When an importer moves or resizes the model box, the write region
  follows before voxels are written.
- **Initialized flag.** The voxel world is either initialized or not. The flag behind the messages
  "...does not contain any element but voxels" is this flag. It does not test whether any voxel is set.

### 2.5 Palette

- **Storage.** The palette is an ordered list of packed colours (2.6). Index 0 always holds 0, the
  empty voxel. A reset leaves exactly `[0]`. That is the only reset any live path uses, including the
  voxel-world re-creation of 5.3.
- **Lookups.**
  - colour at `i`: the entry, or opaque (160, 210, 255) when `i` is out of range.
  - index of colour `v`: the first index holding `v`, or 0 when there is none.
- **Adding** colour `v`:
  - A non-forced add appends only if `v`'s alpha byte is non-zero and `v` is not already present.
  - A forced add always appends, even duplicates and transparent colours.
  - Writing a non-zero voxel through the normal write path performs a non-forced add.
- **Built-in palette images.** Two 16x16 opaque RGB images ship with the product (34.4).
  - The *VOX palette* follows the generation rule of MagicaVoxel's default palette (34.4.1).
  - The *Minecraft palette* holds one colour per legacy Minecraft 1.12 block id (34.4.2).
  - Loading an image appends its pixels as forced adds, without clearing the palette first. The order
    starts at the bottom-left pixel and runs left to right, then up row by row (bottom row first, as
    the image is normally viewed).
  - For an image of exactly 256 pixels, which both shipped images are, loading stops as soon as the
    palette holds 256 entries. On a fresh palette `[0]`, entry `k` (k = 1..255) becomes image pixel
    `k-1` and the last pixel is never used. Entry `k` of the Minecraft palette is therefore the colour
    of legacy id `k`. Known values include id 2 `#5A9643` and id 3 `#A46939`.
  - When the palette is not fresh (an import with replace off), the preset is appended after the
    existing entries, so palette index `k` no longer holds preset entry `k`.
  - The port MUST ship equivalent tables. Section 34 classifies each table and says which ones can be
    regenerated and which need an own replacement.

### 2.6 Colour packing

- **Packed colour.** A packed colour is a signed 32-bit integer `0xAARRGGBB`.
- **Packing bytes.** Packing `(a, r, g, b)` returns 0 when `a = 0`.
- **Packing floats.** Packing float channels in [0,1] truncates each channel with `trunc(c·255)`.
- **Opaque packing.** Packing a byte colour with forced alpha 255 also returns 0 when the input alpha
  is 0.
- **Unpacking.** Unpacking keeps the stored alpha. One helper forces alpha 255, and each format
  section says which it uses.
- **Painted voxels.** Painted voxels are stored with alpha 255.
- **Round trips.** A byte goes to float (`b/255`) and back exactly, in single or double precision, for
  all 256 values. Byte-sourced colours therefore never drift.
- **Hex text.** Colour hex text is `#RRGGBBAA`: uppercase, two digits per channel, alpha last.

### 2.7 Model settings read by interop code

Defaults are the values after File > New, or after any importer that resets the model.

| Key | Meaning (UI label) | Default |
|---|---|---|
| `g.n` | Model name ("Model name (required)"). The setter replaces `"` with U+201D. | `Cubik Model` |
| `g.d` | Description ("Author, description..."). Same quote rule. When unset, it is filled from the signature template (2.8). | signature |
| `g.o` | Model origin, in E. | (0,0,0) |
| `g.ee`, `g.ev`, `g.em` | Exclude elements, voxels, meshes. Read only by OBJ export. | false |
| `g.me` | "Compress data": minify JSON outputs. | false |
| `g.rer` | "Restrict rotations (required for Minecraft block/item model)". | **true** |
| `g.ehe` | "Exclude hidden elements". | false |
| `g.pbt` | "Avoid bleeding textures by modifying UV mapping". | false |
| `g.pzf` | "Avoid z-fighting by applying random offsets". | false |
| `mc.t` | Type: none 0, Block 1, Item 2, Entity 3, Optifine JPM 4, Map 5. | Block |
| `mc.ao` | "Enable Ambient Occlusion (Block only)". | **true** |
| `mc.uen` | "Include Elements name": per-element `__comment`. | **true** |
| `mc.uvds` | "Automatic parent for vanilla display settings". Stored only; no exporter reads it. | false |
| `mc.uds` | "Include Display settings". | false |
| `mc.uos` | "Keep imported Overrides settings (Item only)". | false |
| `mc.p` | "Parent:". Same quote rule. | empty |
| `mc.eab` | Entity "Attach:" bone name. | empty |
| `mc.etr` | Entity texture resolution R. The dialog applies `ClosestPowerOfTwo(clamp(v,16,512))`. | 16 |
| `mc.etx`, `mc.ety` | Entity texture offset, in cells. The dialog applies `max(0, v)`. | 0, 0 |
| `mc.etw`, `mc.eth` | Entity minimum texture size, in cells. The dialog applies `max(0, v)`. | 64, 64 |
| `mc.esb` | "Group all elements as unique bone + Use Origin as pivot (MCPE)". | true |
| `mc.est` | "Use same element texture for doublons (smaller texture)". | true |
| `mc.ecn` | "Use elements label for names". | false |
| `mc.mt` | Map text, "color code = block name". | `empty = air`, `default = stone`, `#5A9643 = grass`, `#A46939 = dirt`, joined by LF |
| `sh.pog`, `sh.pow` | Stonehearth "Placeable on ground", "Placeable on walls". | false, false |
| `sh.v` | Stonehearth value in gold. | 10 |
| `sh.r` | Stonehearth rarity. | `common` |
| `sh.b`, `sh.s` | Stonehearth "Buyable", "Sellable". | true, true |
| `qb.rgb` | "Use RGB color (instead of BGR)". | false |
| `qb.aiz` | "Right handed axis (inverted Z)". | false |
| `obj.gmv` | "Detailed mesh with colored vertices (no texture)". | true |
| `obj.gmt` | "Detailed mesh with texture palette". | false |
| `obj.omt` | "Optimized mesh with texture atlas". | false |
| `obj.ao` | "Render fake ambient occlusion". | true |
| `obj.pt` | "Add padding to textures in voxel atlas on export". | false |
| `obj.sv` | "Smooth voxels vertices on export". | false |
| `obj.wv` | "Weld vertices at exact same coordinates". **The polarity is inverted: true means no welding** (16.4). | true |
| `obj.mo` | "Merge all objects in same mesh". | false |
| `png.gp` | "GUI display preview". | true |
| `png.mv` | "Main view". | false |
| `png.aa` | "Anti-aliasing". | true |

Display transforms (eight slots) and item overrides are part of the model as well, with these rules.

- Display transforms are saved natively. Section 7.7 describes them.
- Overrides are not saved in `.cubik`. They live only in memory after a JSON import.

### 2.8 Application settings read by interop code

| Setting | Meaning | Default |
|---|---|---|
| `minecraftExportTextures` | Preferences > Formats > Minecraft: "Save textures as png on export (beside .json)". | false |
| `objCenterOnImport` | Preferences > Formats > OBJ (Polygons): "Center model and origin on import". | true |
| `sketchfabToken` | Preferences > Formats > Sketchfab token. | empty |
| `formatHideMinecraft`, `formatHideStonehearth`, `formatHideStaxel`, `formatHideVoxel`, `formatHideMesh` | Preferences > Formats > Import/Export dialog: "Hide ... formats". | all false |
| `useNativeFileBrowser` | Use the OS file dialog. When false, the `.lnk` importer is registered. | false |
| `pathImport`, `pathExport` | Default folders of the Import and Export dialogs. | Desktop |
| `signatureTemplate` | Default description template. Keywords `%author`, `%email` and `%cubik` are resolved from the account and app identity. | `Designed by %author with %cubik - https://cubik.studio` |

**App identity string.** It is `Cubik Studio 2.9.480 Beta`: the product name, a space, then the
version text. Several outputs embed it. The port MUST substitute its own identity, e.g.
`cuboid <version>`, in its own outputs. In the `cubik29` profile the identity string is a
configuration value. Golden tests set it to the reference string.

---

## 3. Global I/O rules

### 3.1 Culture and number text

All formatting and parsing runs with the invariant culture, because the application sets it at
startup on the thread that runs every importer and exporter. The decimal separator is `.` and no
thousands separator is written. Three float text forms are used.

| Form | Definition | Examples |
|---|---|---|
| **G7** (default float text) | Shortest text of the value rounded to 7 significant digits. Exponent form `d.ddddddE+dd` or `E-dd` (at least two exponent digits) when the decimal exponent is -5 or lower, or 7 or higher, so `0.0001` stays fixed and `0.00001` prints as `1E-05`. | `0.1`, `22.5`, `-8`, `1E-06`, `1.234568E+07`, `0.6235294` |
| **X9** (custom pattern `0.#########`) | Round to 7 significant digits, then print fixed-point with at most 9 fractional digits. Trailing zeros and a trailing point are dropped. No exponent form. | `5.6568` (stored float 5.65680027), `0.01`, `0` |
| **NJ** (JSON serializer float) | Shortest round-trip text, using 7 significant digits when that round-trips and 9 otherwise. Append `.0` when the text has no `.`, `E` or `e`. | `8.0`, `22.5`, `0.1`, `1E-05` |

Integers are plain decimal with a leading `-` when negative. Negative zero is expected to print as `0`
in G7 and X9 and as `0.0` in NJ (**UNVERIFIED**, T-NUM-1).

The 7-digit rounding of G7 and X9 follows the runtime's number formatter. It was established from the
runtime's documented formatting rules, not observed on a real export (**UNVERIFIED**, test T-NUM-1).

### 3.2 Rounding primitives

| Name | Rule |
|---|---|
| `roundHE(x)` | Round half to even. Used for every "round" in this document. |
| `trunc(x)` | Truncate toward zero. Used for every integer cast. |
| `ceil(x)` | Ceiling to an integer. |

### 3.3 Text encoding and line endings

- Text files are written as UTF-8 **without** a byte-order mark, and an existing file is overwritten.
  A null text is written as an empty file.
- Text files are read BOM-aware: a UTF-8, UTF-16 or UTF-32 BOM is honoured, otherwise UTF-8 is
  assumed.
- Two newline sources exist.
  - **LF** where a literal `\n` is written.
  - **NL** where a line-oriented writer is used, which on the reference Windows build is **CRLF**.
- Each format section states which source applies to each line. In `cubik29` the port MUST emit CRLF
  for NL on every platform, so golden files are platform-independent.
- The JSON serializer's indented mode uses NL between lines, two spaces per level, and `": "` after
  property names. Its compact mode emits no whitespace.

### 3.4 JSON writers

Two kinds of JSON writer exist.

- **Hand-built writers**: Minecraft model JSON, resource-pack snippets.
  - They concatenate text exactly as specified.
  - They do **not** escape strings.
  - Name, description and parent fields cannot contain `"`, because their setters replace it with
    U+201D.
  - Element labels, texture labels, texture paths and override model paths are inserted raw.
- **Serializer writers**: Bedrock `.js`, JPM, Zoxel, Stonehearth.
  - They escape strings per JSON. Control characters use `\n`, `\r`, `\t`, `\b`, `\f` or `\uXXXX`.
    Non-ASCII is written raw.
  - They write members in declaration order.
  - Only writable members are serialized. Get-only properties never appear, because the application
    installs a global contract resolver that keeps writable members only.

**[FIX-ALWAYS] Escaping in hand-built writers.** The port MUST escape `"`, `\` and control characters
inside string values of hand-built writers. This changes output only for inputs on which the reference
writes invalid JSON.

### 3.5 Binary conventions

- Integers are little-endian unless a format says otherwise. NBT is big-endian.
- Floats are IEEE-754 binary32 little-endian.

### 3.6 Compressed containers

- **Formats.** `.schematic` and `.nbt` are gzip-wrapped NBT. The resource pack and the Sketchfab
  upload are ZIP archives.
- **Parity.** The reference compressors cannot be reproduced bit for bit. For these formats, parity is
  defined on the **decompressed** content and on the entry list (names, order and data).
- **Writers.** The port MAY use any conforming gzip or deflate writer.

### 3.7 File naming helpers

- **Stem.** The stem is the file name without its last extension.
- **Sibling paths.** Exporters build sibling paths from the directory of the target path, a `/`
  separator and the stem.
  - The reference "directory with trailing slash" helper turns backslashes into `/` and appends `/`
    when the path is empty or lacks one. An empty directory therefore becomes `/`, the filesystem
    root.
  - **[FIX-ALWAYS]** The port resolves relative or bare targets against the current directory instead.
  - The dialogs always pass absolute paths, so normal use is unaffected.
- **PNG extension.** The PNG writer appends `.png` unless the path already ends with `.png`, compared
  case-insensitively.

---

## 4. Format registry

### 4.1 Registry table

Each registry entry has a key, which is the extension without its dot, a dialog label, a filter, a
capability set and an optional importer and exporter. Capability values: Load 1, Save 2, Import 4,
Export 8.

Rows are in registry order, and the order is significant (4.3, 4.4). The visibility column names the
rule from 4.2: (M) Hide Minecraft, (SH) Hide Stonehearth, (SX) Hide Staxel, (V) Hide Voxel, (H) Hide
Mesh, (L) built-in file browser only, (-) always present.

| # | Key | Dialog label | Filter | Capabilities | Name in messages | Visibility |
|---|---|---|---|---|---|---|
| 1 | `cubik` | Cubik model | `*.cubik` | Load, Save | Cubik | - |
| 2 | `cub` | Cubik model alpha | `*.cub` | Load | (key `CUB`) | - |
| 3 | `json` | Minecraft model | `*.json` | Import, Export | export: "Minecraft 3D Model"; import: `JSON` | M |
| 4 | `mcpe` | Minecraft PE Geometries | `*.mcpe` | Import | "MCPE entity" | M |
| 5 | `js` | Minecraft PE Entity Part model | `*.js` | Export | "Minecraft PE Entity Model" | M |
| 6 | `jpm` | Optifine Entity part model | `*.jpm` | Export | "Optifine Model Part" | M |
| 7 | `java` | Minecraft Mod Entity model | `*.java` | Export | "Forge Java model" | M |
| 8 | `zip` | Minecraft ResourcePack | `*.zip` | Export | "Minecraft ResourcePack" | M |
| 9 | `stonehearth` | Stonehearth model | `stonehearth` (no wildcard) | Export | "Stonehearth data" | SH |
| 10 | `staxel` | Staxel model | `staxel` (no wildcard) | Export (stub, 27.2) | "Staxel data" | SX |
| 11 | `obj` | Wavefront obj model | `*.obj` | Import, Export | "Wavefront OBJ" | H |
| 12 | `stl` | Stereolithography Model | `*.stl` | Export | "Stereolithography STL" | H |
| 13 | `png` | Model Render | `*.png` | Export | "PNG render" | - |
| 14 | `png` | Sprite | `*.png` | Import | (key `PNG`) | - |
| 15 | `binvox` | Binvox voxels | `*.binvox` | Export | "Binvox" | V |
| 16 | `csv` | Sproxel voxels | `*.csv` | Export | "Sproxel CSV" | V |
| 17 | `qb` | Qubicle binary voxels | `*.qb` | Import, Export | export "Qubicle Binary"; import "Qubicle Binary QB" | V |
| 18 | `qef` | Qubicle exchange voxels | `*.qef` | Import, Export | export "Qubicle Exchange"; import "Qubicle QEF" | V |
| 19 | `schematic` | MCEdit voxels | `*.schematic` | Import, Export | export "MCEdit Schematic"; import "MCEdit SCHEMATIC" | V |
| 20 | `nbt` | MC Structures voxels | `*.nbt` | Import | "MC Structure NBT" | V |
| 21 | `vox` | Magicavoxel voxels | `*.vox` | Import, Export | export "Magicavoxel VOX"; import "MagicaVoxel VOX" | V |
| 22 | `zox` | Zoxel voxels | `*.zox` | Import, Export | "Zoxel ZOX" | V |
| 23 | `lnk` | Windows Shortcut | `*.lnk` | Import | (key `LNK`) | L |

The "Name in messages" strings appear verbatim in dialogs and statuses. The Sketchfab upload is a
Tools menu action and is not a registry entry (section 29).

### 4.2 Visibility rules

- **M, SH, SX, V.** The entry is absent from the registry when the preference is on. Opening or saving
  such a file then fails with "Unknown file type".
- **H.** The entry stays registered but is hidden from dialog filter lists. A typed `.obj` or `.stl`
  path still works.
- **L.** The entry exists only while the built-in file browser is in use.

### 4.3 Extension lookup

1. Take the extension of the chosen path and remove every `.` from it.
2. Compare it **case-sensitively** against the keys in registry order, so `.VOX` is "Unknown file
   type".
3. In open mode, return the first matching entry that has an importer.
4. In save mode, return the first matching entry that has an exporter.
5. Otherwise return the last matching entry, or nothing.

For `png`, open mode resolves to Sprite and save mode to Model Render.

**[IMPORT-MODE]** In `modern` the port matches extensions case-insensitively.

### 4.4 Dialog filter lists

A dialog for capability set `F` lists, in registry order, every visible entry whose capabilities
include all of `F`.

| Dialog | Entries listed |
|---|---|
| Open (Load) | cubik, cub |
| Merge (Load) | cubik, cub |
| Save as (Save) | cubik |
| Import (Import) | json, mcpe, obj, png Sprite, qb, qef, schematic, nbt, vox, zox, lnk (only while the built-in browser is in use) |
| Export (Export) | json, js, jpm, java, zip, stonehearth, staxel, obj, stl, png Model Render, binvox, csv, qb, qef, schematic, vox, zox |

The native dialog renders each entry as `<label> (*.<key>)|*.<key>`. Section 6 of the file-browser
specification covers the rest of the dialog behaviour.

---

## 5. Import pipeline

### 5.1 Entry points

| UI path | Dialog title | Default folder | Replace flag |
|---|---|---|---|
| File > Open (Ctrl+O) | "Open a file" | `pathLoadSave` | true |
| File > Merge (Ctrl+M) | "Merge a file with current model" | `pathLoadSave` | false |
| File > Import (Ctrl+I) | "Import a file" | `pathImport` | true |
| Recent file click | none | none | true |
| Drag and drop of the first dropped file | none | none | true, with no unsaved-changes prompt |
| Command-line argument 1 | none | none | true |

Open, Merge and Import first run the unsaved-changes prompt (application specification). Merge lists
only native formats, so interop importers effectively always run with replace = true.

### 5.2 Steps

1. **Validation.** The validation runs in open mode.
   - No extension: an error box titled `Missing extension` with the text `The selected file doesn't have
     any extension.` + LF + LF + `Operation aborted...`.
   - Unknown key: title `Unknown file type "<.ext>"` (the extension keeps its dot), text `The selected
     file can't be opened in Cubik Studio.` + LF + LF + `Operation aborted...`. The product name is
     the app name.
   - Entry without Load or Import: title `Can't read "<.ext>"`, text `Cubik Studio doesn't know how to
     read "<.ext>" files yet.` + LF + LF + `However it knows how to write them!`.
2. **Recent files.** The file is added to the recent list (name = file name) before loading. `.lnk`
   paths are never added.
3. **Loading box.** A file larger than 307200 bytes shows a wait box titled `Loading...` with the text
   `Cubik Studio is loading "<file name>" (<size>)` + LF + LF + `Please wait...`. The pipeline waits
   one physics frame. `<size>` is the byte count, integer-divided by 1024 while it is at least 1024
   and the unit is below GB. It is printed without decimals, followed by `B`, `KB`, `MB` or `GB` with
   no space (for example `351KB`).
4. **Importer.** The importer runs synchronously with (path, replace).
5. **Status and model fields.**
   - Import-capable entry: status `Imported <EXT>: <path>`, where `<EXT>` is the extension without its
     dot, uppercased. The model's "last import/export path" (`ep`) is set to the path.
   - Load entry: status `Loaded <EXT>: <path>`, the save path is set, and the window title is updated.
6. **Closing the wait box.** The wait box is closed only if the importer reported no error. An importer
   that shows its own dialog marks an error so that this step does not close its dialog.

The importer's error flag starts false on every run, because each lookup creates fresh importer
instances.

**Side effect of step 5.** Setting `ep` changes the next Export dialog's defaults to the imported
file's folder, stem and extension (6.1). This also happens for `.lnk`.

### 5.3 Meaning of "replace" per importer

| Importer | replace = true does | replace = false does |
|---|---|---|
| Minecraft JSON, `.cub`, `.mcpe` | Full model reset with base textures. The texture list is cleared and only the internal `error` texture remains. The reset happens before parsing for JSON and `.cub`, and after the name prompt for `.mcpe`. | Merge into the current model. |
| Voxel importers (vox, zox, qb, qef, schematic, nbt) | Re-create the voxel world at the importer's resolution. Elements and meshes stay. | Write into the existing world and palette; see each section for offset quirks. |
| OBJ, PNG sprite, LNK | Ignored. The import always adds to the current model. | Same. |

**Re-creating the voxel world** (voxel importers):

1. Clear voxels and reset the palette to `[0]`, but only if the world was initialized.
2. Create the world at the new resolution, keeping the model's `obj.ao` flag.
3. Set the write region from the **current** model box.

### 5.4 Error reporting convention

Unless a section says otherwise, an importer that fails does this:

- It shows an error box titled `<Name> Import` with the text
  `An error occured while importing <Name>:` + LF + `<message>`.
  - The misspelling "occured" is part of the reference text.
  - `<message>` is the underlying error text.
- It sets one button labelled `Abort`.
- It keeps whatever it had already applied. Nothing is rolled back.

In `cubik29` the port MUST use the same titles and lead sentences. Its `<message>` texts are its own,
because the reference messages come from the runtime.

---

## 6. Export pipeline

### 6.1 Export dialog

File > Export (Ctrl+E) opens a save dialog titled `Export Cubik model` with these defaults.

- **Folder.** `pathExport`.
- **File name.** The model name.
- **Extension.** `json`.
- **When a last import/export path exists.** If the model's `ep` is set and its directory exists, the
  folder, stem and extension all come from `ep`.

The filter list is the Export list of 4.4.

### 6.2 Steps

1. **Validation.** The validation runs in save mode. The messages for a missing extension and an
   unknown file type are the same as in 5.2. An entry without Save or Export shows the title
   `Can't generate "<.ext>"` with the text `Cubik Studio doesn't know how to write "<.ext>" files yet.`
   + LF + LF + `However it knows how to read them!`.
2. **Start status.**
   - Export entry: status `Importing <EXT>: <path>`. The wording is the reference wording. It is
     replaced later.
   - Save entry: status `Loading <EXT>: <path>`.
   - The model's `ep` is set to the path.
3. **Build.** The exporter runs as a cooperative task. It may show dialogs, update the status and
   progress bar, and yield to the UI. It builds the whole output in memory.
4. **Abandon on an early stop.** If the exporter stops early (empty model, user cancel in a dialog
   that aborts, or an internal error), nothing more happens. No file is written and no further status
   is shown.
5. **Write.** Otherwise the exporter writes its files, as each format section describes.
   - The writer helpers report `Successfuly saved file: <path>` or `Successfuly saved texture: <path>`.
     The misspelling "Successfuly" is part of the reference text.
   - On I/O failure a helper shows an error box `Error while saving file` and sets
     `Failed saving file: <path>` or `Failed saving texture: <path>`. The texts are listed in 6.4.
   - Helper failures do not abort the export.
6. **Unexpected errors.** An unexpected exception during the write shows the error box
   `Error while saving` with the text `An error occured while saving <Name> export:` + LF +
   `<message>` and the button `Abort`.
7. **Final status.** The status is set to `Exported <EXT>: <path>`. This happens **even after the
   failures of steps 5 and 6**.
8. **Recent files.** When the format also has an importer, the path is added to the recent files list.

**[FIX-MODERN]** The port reports a failed write as a failure in modern profiles. In `cubik29` it MUST
still show both the error dialog and the final `Exported` status, so the visible sequence matches.

### 6.3 Voxel-only prompt

The Binvox, Zoxel, Qubicle binary, Qubicle exchange, MCEdit schematic and MagicaVoxel exporters use a
common prompt. CSV has its own variant (section 26).

When the model contains at least one element, the exporter first shows an information box.

- **Title.** `<Name> Export`. Example: `Magicavoxel VOX Export`.
- **Text.** `Exporting to <Name> format only export voxels. Elements and Meshes are not exported.` +
  LF + LF + `To export elements or meshes, convert them first to voxels.`
- **Buttons.** `Continue`, `Cancel`.
- **Cancel.** Sets the status `Cancelled <KEY> export...`, where `<KEY>` is the uppercased registry
  key.

The export then continues **whichever button was pressed**. In `cubik29` the port MUST reproduce
this, so Cancel changes only the status. **[FIX-MODERN]** Modern profiles abort on Cancel.

### 6.4 Writer helper messages

| Situation | Dialog title | Dialog text | Status |
|---|---|---|---|
| Binary write OK | none | none | `Successfuly saved file: <path>` |
| Binary write failed | `Error while saving file` | `An error occured while saving file "<file name>".` + LF + LF + `<message>` | `Failed saving file: <path>` |
| Text write OK | none | none | `Successfuly saved file: <path>` |
| Text write failed | `Error while saving file` | `An error occured while saving "<file name>".` + LF + LF + `<message>` | `Failed saving file: <path>` |
| PNG write OK | none | none | `Successfuly saved texture: <path>` |
| PNG write failed | `Error while saving file` | `An error occured while saving texture "<file name>".` + LF + LF + `<message>` | `Failed saving texture: <path>` |

The PNG writer encodes the image as 8-bit RGBA PNG. PNG parity is defined on the decoded pixels, not on
the bytes.

### 6.5 Element-only pre-check

The `.js`, `.java` and `.jpm` exporters run this check first, and the Minecraft JSON exporter runs it
after its type check.

When the model has no elements, they show an error box and stop. The texts below use `<Name>` for the
exporter's message name.

| Condition | Title | Text |
|---|---|---|
| The voxel world is initialized | `Voxel model` | `Your model does not contain any element but voxels.` + LF + LF + `To export your model to <Name> format, you must convert voxels to elements first.` |
| Otherwise, at least one mesh exists | `Mesh model` | `Your model does not contain any element but meshes.` + LF + LF + `To export your model to <Name> format, you must convert meshes to elements first (via a voxelization).` |
| Otherwise | `Empty model` | `Your model is empty.` + LF + LF + `To export your model to <Name> format, you must create at least one element first.` |

The Minecraft JSON exporter writes `(via voxelization).` without the article "a" in the Mesh model
text.

## 7. Minecraft Java block or item model (`.json`), export

The target is the pre-1.13 Java block and item model format (game versions 1.9 to 1.12). The writer is
hand-built (3.4). All its newlines are LF and it uses tab indentation.

### 7.1 Modes

The exporter has a **normal** mode, used by File > Export, and a **forced** mode, used only by the
resource-pack exporter (9.1). Forced mode changes these behaviours.

- **Texture PNGs.** Every texture's PNG is written regardless of the setting (7.4).
- **Texture warnings.** The "No texture" and "unknown paths" dialogs are suppressed. The pre-checks of
  7.2, including the texture questions, still run.
- **Hidden elements.** The hidden-element exclusion is off.
- **Element filters.** The two element filters of 7.5 are off, and jitter is off.
- **Display block.** It is always written with all eight slots and all three members.
- **Overrides block.** It is always written.
- **Element comments.** The per-element `__comment` is written unless the output is minified.

### 7.2 Pre-checks

The pre-checks run in this order. Any stop ends the export without a file.

1. **Model type.** When the model type is neither Block nor Item, an error box appears titled
   `Invalid Model Type`. Its text is `Current model is not defined as a block or an item and so can't
   be exported as a Minecraft 3D Model.` + LF + LF + `Set your model as a block or an item in Model
   settings to proceed.`, and the export stops.
2. **Element-only check.** The check of 6.5 runs with Name = `Minecraft 3D Model` and the
   "(via voxelization)" wording.
3. **Texture checks.** The texture entries are visited in list order. The first entry that meets one
   of these conditions shows a warning box with `Continue` and `Cancel`, and the export waits for the
   answer. `Cancel` stops the export. Only one warning is ever shown.
   - **Non-square image.** Title `Non square texture`, text `The texture "<label>" is not a square and
     so will not work in Minecraft.` + LF + LF + `Do you wish to continue anyway?`.
   - **Uppercase in the Minecraft path.** The image is square but its Minecraft path (7.4) contains an
     uppercase letter. Title `Uppercase character in path`, text
     `The texture "<label>" with path "<path>" uses an uppercase character.` + LF + LF +
     `Since version 1.11, Minecraft does not load textures containing uppercase characters in path.` +
     LF + LF + `Do you wish to continue anyway?`.
4. **Rotation check.** The first element whose rotation is invalid for Minecraft (2.2.2) shows an
   error box titled `Invalid Element rotation`, and the export stops. The text is
   `Current model contains <n> element<s> with free rotation angles that can't be handled in
   Minecraft.` + LF + LF + `Please fix element "<label>" #<id> to proceed.`
   - `<n>` counts all invalid elements.
   - `<s>` is `s` when `n > 1`.
   - `<id>` is the element's display id.

### 7.3 Document layout

The status is set to `Exporting...`. The document is the concatenation of these parts, in order.

```
{
<header lines>
<textures block>
<elements block>
<display block, optional>
<overrides block, optional>
LF }
```

- **Header lines.** Each line is LF, tab, key, value and `,`.
  - `__comment` with value `"<description>"`, written when the description is not empty.
  - `parent` with value `"<parent>"`, written when the parent is not empty.
  - `ambientocclusion` with value `false`, written when "Enable Ambient Occlusion" is off. The default
    is on, so the line is normally absent.
- **Textures block.**
  - It opens with LF, tab and `"textures": {`.
  - Each entry is LF, tab, tab and `"<label>": "<path>"`. The entries are joined by `,`.
  - It closes with LF, tab and `},`. An empty map is still written.
- **Elements block.**
  - It opens with LF, tab, `"elements": [`, then a space, a tab and a space. That trailing whitespace is
    reference output.
  - The element objects are joined by `,`.
  - It closes with LF, tab and `]`.
- **Display and overrides blocks.** Each begins with `,` (7.7 and 7.8).
- **Closing.** LF then `}`. No final newline follows.

### 7.4 Texture map

**Minecraft path of a texture entry.**

1. If an explicit path was assigned, use it. An import assigns one (8.3).
2. Otherwise derive the path from the source file path, normalized to `/`:
   - When the path contains `/textures/` at an index above 0, take the text after the last
     `/textures/` and drop the extension.
   - Otherwise take the file name without its extension.
3. Otherwise the path is empty. Generated textures have no source file.

**Value written for each entry**, in list order:

1. If the Minecraft path is not empty, write it verbatim. No namespace is added.
2. Otherwise the entry counts as "unsaved" and a value is built from a name.
   - The name is the label when `minecraftExportTextures` is on.
   - Otherwise it is the *sanitized image name*. That name comes from the texture image's internal
     name, which the texture specification owns, with all whitespace removed and lowercased
     (invariant). It falls back to the label, sanitized the same way, when empty. The PNG is written
     immediately as `<output folder>/<sanitized>.png`.
   - The value is `blocks/<name>` for a Block model and `items/<name>` for an Item model.

**PNG side output.** When `minecraftExportTextures` is on, or in forced mode, every texture entry is
first written as `<output folder>/<sanitized image name>.png`, before the document is built. With the
setting on, the map value uses the label while the file uses the sanitized name. The two can differ;
**[FIX-MODERN]** modern profiles make them equal.

**Texture-map dialogs** (normal mode only). These are information boxes with the buttons
`Open folder` (opens the output folder) and `Continue`. The export does not wait for them.

- **No entries.** Title `No texture`, text `Your model don't have any texture defined in Elements
  Material.` + LF + LF + `Be sure to load texture on labels and assign those to your elements or your
  model will be displayed with a purple texture in game.`.
- **Unsaved entries.** Title `Model with unknown textures paths`.
  - With exactly one unsaved entry the text is `Your model uses an unsaved texture.` + LF + `To
    proceed, Cubik Studio exported it as PNG beside JSON.` + LF + `Move it in your ResourcePack and
    update the JSON if necessary.` + LF + LF + `If you get purple and black colors on your model in
    game, verify in JSON that the texture path exists.`
  - With `<n>` > 1 the text is `Your model uses <n> unsaved textures.` + LF + `To proceed, Cubik Studio
    exported them as PNGs beside JSON.` + LF + `Move all PNGs in your ResourcePack and update the JSON
    if necessary.` + LF + LF + `If you get purple and black colors on your model in game, verify in
    JSON that each texture path exists.`

All message boxes share one window, so a later box replaces an earlier one that is still open. In
practice the user sees the last box shown during the export.

### 7.5 Elements

Elements are visited in list order. A hidden element is skipped when "Exclude hidden elements" is on,
in normal mode. The remaining elements pass these filters, which apply only in normal mode **and**
while the display-settings preview is not active.

- **No enabled face.** An element with all six faces disabled is omitted silently.
- **Out of bounds.** An element is omitted and counted as "skipped" when either of these points falls
  outside the editor box `x in [-16,32], y in [-16,32], z in [-32,16]`:
  - its stored corner `c`;
  - `c + (s.x, s.y, -s.z)`.

  Both ends are inclusive. Note that the test uses the rotated stored corner.

For each emitted element:

1. **Origin.**
   - `from = unrot_M` (2.2.2).
   - When "Avoid z-fighting" is on, in normal mode, and `from != (0,0,0)` under approximate vector
     equality, add an independent uniform random offset in `[-0.001, 0.001]` to each component. The
     output is then not reproducible; parity tests keep the option off.
   - `to = from + s`. The size is not rounded.
2. **Text.**
   - It opens with LF, two tabs and `{`.
   - When "Include Elements name" is on, or in forced mode, and the output is not minified, a comment
     line follows: LF, three tabs, `"__comment": "<label>",`.
   - Two coordinate lines follow, each LF, three tabs, then `"from": [ X, Y, Z ],` and `"to": [ X, Y, Z ],`
     with values in X9 joined by `", "`. When **invert** is on, the two value triples are swapped.
     The keys stay in order.
   - When rotated, a rotation line follows: LF, three tabs,
     `"rotation": { "origin": [ X, Y, Z ], "axis": "<a>", "angle": <A><R> },`. The origin is
     `pivot_M` in X9, `<a>` is the axis letter (2.2.2), `<A>` is the axis angle in X9, and `<R>` is
     `, "rescale": true` when rescale is on, otherwise empty.
   - When shade is off, a shade line follows: LF, three tabs, `"shade": false,`.
   - LF, three tabs, `"faces": {`, then the faces (7.6).
   - LF, three tabs, `}`. The `,` after the last face line is removed.
   - LF, two tabs, `}`.
3. **Progress.** Every 200 processed elements, the status shows `Exporting Elements: <p>%`, where
   `p = trunc(done/total·100)` and the total counts all elements. The exporter then yields one frame.

After the elements block, the status shows `Exporting data...`.

**Skipped elements warning.** In normal mode, when the skipped count is above zero, a warning box is
shown before the file is written. Its title is `Skipped elements`, and its text is
`<n> element(s) have not been exported because of invalid position (outside authorized Minecraft
bounds).` + LF + LF + ` Be sure to check that all elements are inside grid.`. The second paragraph
starts with a space in the reference text.

### 7.6 Faces

Faces are written in the order down, up, north, south, west, east. Disabled faces are omitted, and
enabled faces are written in this form.

```
LF + 4 tabs + "<face>": { "uv": [ U0, V0, U1, V1 ], "texture": "#<label>"<cull><rot><tint> },
```

- **UV values.** `(U0, V0) = uv0` and `(U1, V1) = uv1` (2.3), in X9. Mirrored UVs keep their order.
- **Bleed inset.** When "Avoid bleeding textures" is on, which applies in every mode, each edge moves
  toward the centre.
  - The horizontal step is `dx = min(0.01, 0.32/W)` and the vertical step is
    `dy = min(0.01, 0.32/H)`. `W` and `H` are the pixel width and height of the image of the texture
    entry named by the face label. An unknown label uses the internal error image.
  - For an increasing pair, `U0 += dx` and `U1 -= dx`. For a mirrored pair the signs swap. `V0`, `V1`
    and `dy` follow the same rule.
- **Texture key.** `#<label>` is the face's texture label. It is not checked against the map.
- **cullface.** `<cull>` is `, "cullface": "<name>"` when cullface is not none. The name is the enum
  name: `down`, `up`, `north`, `south`, `west` or `east`. The value 63 prints as `any` or `all`
  (**UNVERIFIED** which), and other combinations print as decimal numbers.
- **UV rotation.** `<rot>` is `, "rotation": <r>` when the rotation is above 0. `<r>` is 90, 180 or 270.
- **tintindex.** `<tint>` is `, "tintindex": <t>` when the tintindex is 0 or more.

### 7.7 Display block

The block is written when "Include Display settings" is on, or in forced mode. It opens with `,` LF
tab `"display": {` and closes with LF tab `}`. The slots are visited in this order:

`thirdperson_righthand`, `thirdperson_lefthand`, `firstperson_righthand`, `firstperson_lefthand`,
`gui`, `head`, `ground`, `fixed`

The `fixed` slot holds the editor's item-frame transform.

- **Skipping a slot.** A slot is skipped when it is disabled, outside forced mode.
- **Members.** In order, the members are `rotation` (written when not (0,0,0)), `translation` (written
  when not (0,0,0)) and `scale` (written when not (1,1,1)). In forced mode every member is written.
  - The comparisons use approximate vector equality: the squared distance must be below 1e-10.
  - A scale component that is exactly 0 reads as 0.0001.
- **Member text.** LF, three tabs, `"<member>":`, **two** spaces, then `[ x, y, z ]`. The values are G7,
  joined by `", "`.
- **Separators.** Members are separated by `,` written before the next member.
- **Slot text.** A slot with at least one member is written as an optional leading `,` (absent for the
  first slot written), then LF, two tabs, `"<slot>": {`, the members, and LF, two tabs, `}`.
- **Empty block.** If no slot is written, the whole display block is omitted.
- **Round trip.** An enabled slot that only holds default values disappears on export, so it comes back
  disabled on import.

### 7.8 Overrides block

The block is written when "Keep imported Overrides settings" is on, or in forced mode. That still
holds when the list is empty, in which case the block is `,` LF tab `"overrides": [` LF tab `]`.

Each case is written in this form.

```
[","] LF 2tabs "{" LF 3tabs "\"predicate\": {" <members> LF 3tabs "}," LF 3tabs "\"model\": \"<model>\"" LF 2tabs "}"
```

- **Members.** The keys are `angle`, `blocking`, `broken`, `cast`, `damage`, `damaged`, `lefthanded`,
  `pull`, `pulling` and `time`, in that fixed order. Each member is written only when it has a value,
  as LF, four tabs, `"<key>": <value>`, with `,` before every member except the first. Floats are
  `angle`, `damage`, `pull` and `time`, written in G7. The other keys are integers.
- **Closing.** The array closes with LF, tab and `]`.

### 7.9 Minify

When "Compress data" is on, the whole document passes through one rule: every run of whitespace
outside a string literal is removed. A string literal is `"` followed by any characters other than `"`
and `\` or escape pairs `\<any>`, followed by `"`. In minified output the per-element `__comment` lines
are already omitted (7.5).

### 7.10 Write

The text is written to the chosen path (UTF-8, no BOM). The writer status and messages follow 6.2 and
6.4.

---

## 8. Minecraft Java block or item model (`.json`), import

### 8.1 Reading and parsing

1. Read the text BOM-aware.
2. Remember the model's folder (with a trailing `/`) and stem.
3. If replace is set, reset the model with base textures (5.3). This happens **before** parsing, so an
   import aborted later still leaves an empty model.
4. Parse the text with the tolerant-but-limited parser of 8.6.
5. On a syntax error, show an error box and stop, with the error flag set.
   - Title: `Invalid JSON`.
   - Text: `"<stem>" is not a valid JSON file:` + LF + LF + `Invalid data found`. The reference parser
     always reports this fixed message.
6. An empty file yields no document. The reference then fails with an unhandled null dereference.
   **[FIX-ALWAYS]** The port treats it as a syntax error.

### 8.2 Model gate

- **Missing keys.** If the root has none of `elements`, `textures` and `parent`, show an error box
  titled `Invalid JSON Model` with the text `"<stem>" is not a valid Model` + LF + LF + `Please check
  the file or contact the author.`, and stop.
- **Parent prompts.** If the root has `parent` (string value `<p>`), a prompt or stop applies. The
  first matching row of the table wins. The error flag is set in every case, so the wait box is not
  closed over the prompt.
- **No parent.** Apply the document directly (8.3).

| Condition | Box type, title | Text | Buttons |
|---|---|---|---|
| `<p>` is exactly `item/generated` | Question, `Generated Model` | `The Model "<stem>" is generated in game by extruding its texture.` + LF + `To edit this model with Cubik, you can extrude its texture by importing the PNG.` + LF + LF + `Import aborted.` | `OK` (import stops) |
| `elements` is missing or empty | Question, `Inherited Model` | `"<stem>" is a child of "<p>.json" and does not contain any element.` + LF + `Importing the Parent is required to modify the model.` + LF + LF + `Do you want to import it anyway?` | `Import` (apply), `Abort` |
| `<p>` is `block/block` or `item/handheld` | Information, `Inherited Model` | `"<stem>" is using "<p>.json" as parent.` + LF + LF + `Merging the Parent can be required to get correct display settings.` | `OK` (apply), `Abort` |
| Any other `<p>` | Question, `Inherited Model` | `The Model "<stem>" is a child of "<p>.json".` + LF + `Importing the original Parent model can be required to modify it.` + LF + LF + then `There is one element in the model.` + LF when n <= 1, or `There are <n> elements in the model.` + LF otherwise, then `Do you want to import it anyway?` | `Import` (apply), `Abort` |

### 8.3 Apply order

The apply steps run in this order.

1. **Reset the type.** Set the model type to none.
2. **Folder heuristic.** If the model's folder path contains `item` and not `block`, the type becomes
   Item. If it contains `block` and not `item`, the type becomes Block. These are case-sensitive
   substring tests on the whole path. The heuristic applies only while the type is none.
3. **parent.** If present, store it as the model parent. Set "Automatic parent for vanilla display
   settings" to on, and set the type to Item when the parent is exactly `item/handheld`, otherwise to
   Block. This overrides step 2.
4. **Root fields.**
   - The model name becomes the stem.
   - When `__comment` is present, its string becomes the description.
   - When `ambientocclusion` is present, it becomes "Enable Ambient Occlusion". A non-boolean value
     reads as false.
5. **textures.** When present, visit the entries in document order.
   1. The value must be a string. A non-string value fails with an unhandled error in the reference;
      **[FIX-ALWAYS]** the port reports it as an import error.
   2. If the value contains `:`, split at the first `:` into a namespace and a path. Otherwise the
      namespace is `minecraft`.
   3. Resolve a file for the path (8.4).
   4. Register the entry. The texture entry gets the label `<key>`. Its image comes from the resolved
      file, or from the error image when nothing was found. Its Minecraft path is `<path>`, without the
      namespace.
      - An existing label is replaced.
      - A new label is added. The texture specification owns label sanitizing and its results.
   5. Map `<key>` to the resulting label.
   6. Apply the folder heuristic of step 2 to `<path>` and to the resolved file path, still only while
      the type is none.

   When `textures` is absent, show an information box titled `No texture found` and continue.
   - With a parent, the text is `This 3D model is loading textures from its parent "<parent>".` + LF +
     LF + `You can load correct textures with the Material Manager.`.
   - Otherwise it is `This 3D model doesn't have any texture.` + LF + LF + `You can add textures
     manually with the Material Manager.`.
6. **Default type.** If the type is still none, it becomes Block.
7. **elements.** Each entry creates one cube element (8.5).
8. **display.** If present, set "Include Display settings" on. For each of the eight slots (7.7, with
   the item-frame slot read from `fixed`):
   - A present slot becomes enabled with `rotation` (default (0,0,0)), `translation` (default (0,0,0))
     and `scale` (default (1,1,1)). Each vector needs at least three numbers, otherwise it takes its
     default.
   - An absent slot becomes disabled with default values.

   If `display` is absent, set "Include Display settings" off and reset all slots.
9. **overrides.** If present, set "Keep imported Overrides settings" on and clear the list. Each entry
   that has a `predicate` object adds a case.
   - The `model` string defaults to empty.
   - Each of the ten predicate keys is optional. Integers are truncated from the number.
   - An entry without `predicate` is skipped.

   If `overrides` is absent, set the option off and clear the list.
10. **Finish.** Deselect everything and refresh the texture list.

### 8.4 Texture file resolution

The inputs are a path `P` (for example `blocks/stone`), a namespace `ns`, and the model folder `F`,
normalized to `/` with a trailing `/`. The first candidate that exists wins.

1. If `F` contains `/<ns>/` at an index above 0, take the prefix up to and including that `<ns>` and
   try `<prefix>/textures/<P>.png`. The last occurrence is used.
2. Otherwise, if `F` contains `/minecraft/` at an index above 0, try the same with `minecraft`. This
   step is an alternative to step 1 and is never tried after it.
3. Let `D` be `F` with its trailing separator removed. Try `D/textures/<P>.png`.
4. Try `D/<P>.png`.
5. Try `D/<last path segment of P>.png`.
6. Otherwise nothing is found. The reference logs a "not found" line and uses the error image.

### 8.5 Elements

Each `elements` entry is processed as follows.

1. **Corners.** Read `from` and `to` as three numbers each. With fewer than three, the triple is
   (0,0,0).
2. **Size.** `size = to - from`, and each component equal to 0 becomes 0.001.
   - The element setter then raises every component to at least 0.001. An inverted box (`to < from`)
     therefore becomes 0.001 thick at `from`.
   - **[IMPORT-MODE]** In `modern`, the port sets invert = true and stores the absolute size.
3. **Position.** `position_E = (from.x, from.y, -from.z)`.
4. **Rotation.** When `rotation` is present:
   - The pivot is `(origin.x, origin.y, -origin.z)`.
   - `angle` defaults to 0, and `rescale` is read.
   - The Euler rotation is the editor Euler form of a rotation by `-angle` about +X for axis `x`,
     about +Y for `y`, and about -Z for `z`.
   - Any other axis string, including the 1.21.11 `{x, y, z}` form, gives no rotation, but the pivot
     is still the origin.

   Otherwise the pivot equals the position and there is no rotation.
5. **Creation.**
   - Create a cube element with a random colour. Each channel is uniform in [0.6, 1] and alpha is 1.
   - Set the pivot. A non-zero rotation is then assigned through the storage rule (2.2.1), which snaps
     to multiples of 22.5 when "Restrict rotations" is on (the default after a reset) and to multiples
     of 5 otherwise.
   - Store the rescale flag.
6. **Label and shade.**
   - The label is `__comment`, or `Element` when it is empty or missing.
   - `shade` defaults to true. A non-boolean value reads as false.
7. **Faces.** When `faces` is present:
   1. Reset every face's UV rotation to 0 and every UV to the element's automatic projection. The
      element specification defines that rule, the same one as "Apply Default UV Mapping".
   2. For each of the six face names:
      - **Absent face.** The face becomes disabled.
      - **uv.** When present, it is read as four numbers `u0, v0, u1, v1`, each divided by 16. Then
        `tl = (u0, 1-v1)` and `br = (u1, 1-v0)`, written raw without clamping. With fewer than four
        numbers all values are 0.
      - **texture.** The string with its first character removed, for example `#side` becomes
        `side`. A missing or empty texture string fails with an unhandled error in the reference;
        **[FIX-ALWAYS]** the port reports an import error.
        - The name is mapped through the key-to-label map.
        - If no texture entry with that label exists, a default texture entry is created.
      - **cullface.** The exact enum name. Any other text gives none, so the alias `bottom` is lost
        (**[IMPORT-MODE]**: `modern` maps `bottom` to down).
      - **rotation.** An integer, normalized by the UV-rotation setter.
      - **tintindex.** An integer. Missing values give -1.

   When `faces` is absent, every face keeps its defaults: enabled, the label `texture`, and the
   automatic UVs.

These keys are ignored: `gui_light`, `texture_size`, `credit`, element `name`, `light_emission`,
`render_type`, `groups`, and unknown keys. Section 31 lists what modern import adds.

### 8.6 Parser rules

The port's `cubik29` import mode MUST accept and reject the same inputs as the reference parser.

- **Root.** The first non-whitespace character must be `{`. Whitespace means Unicode white space.
- **Strings.**
  - A string ends at the first `"` that is not directly preceded by `\`. A string whose content ends
    with a backslash therefore swallows the quote.
  - Only `\uXXXX` escapes are decoded. Every other backslash sequence stays verbatim, so `\n` remains
    two characters. **[IMPORT-MODE]** `modern` decodes all JSON escapes.
- **Numbers.**
  - A number token starts with `-` or a digit and runs to the next `,`, `]` or `}`, so whitespace may
    be included.
  - The token is parsed as an invariant double with leading and trailing whitespace allowed, and with
    exponents.
  - Integer reads truncate the number toward zero.
- **Literals.** `true`, `false` and `null` are accepted.
- **Rejected and accepted input.** Comments are rejected. A trailing comma before `}` or `]` is
  accepted. A leading comma, or a number starting with `+`, is rejected.
- **Duplicate keys.** The last value wins.
- **Unexpected end of text.** It is a syntax error.

---

## 9. Minecraft resource pack (`.zip`), Cubik alpha (`.cub`) and display presets

### 9.1 Resource pack export

The pack targets pack format 3 (Minecraft 1.11 and 1.12). It replaces the vanilla `glass` and
`white_stained_glass` models with the current model.

Steps:

1. **Build the model.** Run the Minecraft JSON exporter in **forced** mode, with the output path
   `<temp>/cubik_model.json`. `<temp>` is the per-user temporary cache folder.
   - All its pre-checks and questions apply (7.2).
   - If it stops, the pack export stops silently. No zip and no `Exported` status follow.
   - Because of forced mode, every texture entry is also written as a PNG into `<temp>`.
2. **Write the model.** Write the JSON text to `<temp>/cubik_model.json`.
3. **Pack icon.** Render the 128x128 icon with the PNG render core (18.1). It uses GUI display mode,
   2x supersampling and orthographic size 8, and is written to `<temp>/pack.png`.
4. **Zip entries.** Build a ZIP archive with these entries, in this order.

   | Entry | Content |
   |---|---|
   | `pack.png` | The rendered icon. |
   | `pack.mcmeta` | The text in 9.1.1 below. |
   | `assets/minecraft/blockstates/glass.json` | The blockstate text in 9.1.1 below. |
   | `assets/minecraft/blockstates/white_stained_glass.json` | Same as above. |
   | `assets/minecraft/models/item/glass.json` | The item model text in 9.1.1 below. |
   | `assets/minecraft/models/item/white_stained_glass.json` | Same as above. |
   | `assets/minecraft/models/block/cubik_model.json` | The JSON from step 2. |
   | one entry per texture, in list order | See below. |

   The reference passes names with a leading `/`, and the ZIP library strips leading slashes from
   entry names (**UNVERIFIED** library behaviour). The port MUST write names without a leading slash.

   Texture entries depend on the texture's state.
   - **No usable file.** The entry has no source file, or the file is missing, or its Minecraft path is
     empty. Its image is written to `<temp>/<label>.png` and added under
     `assets/minecraft/textures/blocks/`, or `assets/minecraft/textures/items/` when the model type is
     Item.
     - The reference adds such an entry twice. The second add fails on the duplicate name and that
       error is swallowed.
     - **[FIX-ALWAYS]** The port adds it once.
   - **Usable file.** The image (re-encoded, not copied) is written to
     `<temp>/<source file stem>.png`. It is added under `assets/minecraft/textures/<text of the
     Minecraft path before its last "/">/`, or under `assets/minecraft/textures/` when the path has no
     `/` or its only `/` is the first character. A failure to add is ignored.
5. **Archive errors.** Any other archive error shows an error box titled `Error`. Its text is
   `An error occured while creating resource pack zip file.` + LF + LF + `<message>`, and the export
   stops.
6. **Success.** Show an information box titled `Model ResourcePack` with the text `Your model has been
   exported as a fully working Minecraft ResourcePack.` + LF + LF + `To use it in game, move the zip
   file in your Minecraft resourcepack folder and then get block "glass" or "white stained glass" to
   test the render.`. The status becomes `Exporting Minecraft ResourcePack finish...`, then the
   archive is saved to the chosen path.

#### 9.1.1 Fixed texts

In all three texts below, `<TAB>` is a tab character and newlines are LF.

- **`pack.mcmeta`.** The description is inserted unescaped.
  ```
  {
  <TAB>"pack": {
  <TAB><TAB>"pack_format": 3,
  <TAB><TAB>"description": "<description>"
  <TAB>}
  }
  ```
- **Blockstate.**
  ```
  {
  <TAB>"variants": {
  <TAB><TAB>"normal": { "model": "cubik_model" }
  <TAB>}
  }
  ```
- **Item model.**
  ```
  {
  <TAB>"parent": "block/cubik_model"
  }
  ```

**Known limitations, kept as reference behaviour.**

- Texture entries are re-encoded PNGs.
- A texture's file name inside the pack comes from its source file stem, while the model JSON
  references its Minecraft path. The two may differ.
- The temp files are never deleted.

### 9.2 Display settings presets

The display-settings panel ships preset files and stores user presets. Both use a JSON form with the
keys `name`, `canBeRemoved`, `enabled`, `rotation`, `translation` and `scale`, and are owned by the
display-settings specification. They are not an import or export format of the File menu.

### 9.3 Cubik alpha (`.cub`), load only

`.cub` is a legacy native format. Its decoder reuses the Minecraft JSON reader. The native format
specification is normative for it, so this is a summary.

1. **Decode.** Read the text BOM-aware as a sequence of UTF-16 code units. XOR each unit `i` with
   `"BDcraft Cubik - Encryption System Key"[i mod 37]` and with `"oBeeware"[i mod 8]`. The operation is
   its own inverse.
2. **Parse.** Parse the result with the rules of 8.6 and apply it as in 8.2 and 8.3. Error titles use
   `CUB` in place of `JSON`. The steps that differ are:
   - **Root fields.**
     - `name`: the model name, default empty.
     - `gridstep`, `gridmousestep`, `gridkeystep`: default 1.
     - `snaptogrid`: default true.
     - `modelOrigin`: default (0,0,0).
     - `type`: integer, default 1.
     - `usevanilladisplaysettings`: default true.
     - `usecustomdisplaysettings`, `useoverridessettings`, `preventbleedingtextures`,
       `excludehiddenelements`, `minifyexport`: default false.
     - `modelBox` with `xmin` -16, `ymin` -16, `zmin` -32, `xmax` 32, `ymax` 32, `zmax` 16,
       `isActive` true, `showHandles` true, `showBox` false.
     - The entity texture resolution is forced to 16.
   - **Textures** *(spot-checked)*. They come from `textures_path` (key to path) instead of
     `textures`, with their own file search and a "not found" summary.
   - **Voxels** *(spot-checked)*. `voxel.resolution` (default 16) re-creates the world, and each
     `voxel.voxels` entry `[x, y, z, colour]` is written raw.
   - **Element extras** *(spot-checked)*. `origin`, `label`, `color`, `uvsnap` and `uvsnapmirrored`.

## 10. Bedrock legacy geometry import (`.mcpe`)

The input is the legacy, pre-`format_version` Bedrock geometry document, as found in the old
`mobs.json`. Its root is an object that maps names to geometry objects.

### 10.1 Schema

The JSON serializer applies its standard tolerance: comments, single quotes and unquoted names are
accepted, and unknown keys are ignored. Every root member must be a geometry object. A root member of
any other shape (for example `"format_version": "1.8.0"` or `"minecraft:geometry": [...]`) fails the
whole import with an error.

| Object | Key | Type | Default |
|---|---|---|---|
| geometry | `texturewidth` | int | 64 |
| geometry | `textureheight` | int | 64 |
| geometry | `bones` | array of bone | empty |
| bone | `name` | string | null |
| bone | `parent` | string | null (read but unused) |
| bone | `pivot` | float[3] | [0,0,0] |
| bone | `rotation` | float[3] | [0,0,0] |
| bone | `mirror` | bool | false |
| bone | `neverRender` | bool | false |
| bone | `cubes` | array of cube | empty |
| cube | `name` | string | null (unused) |
| cube | `origin` | float[3] | [0,0,0] |
| cube | `size` | float[3] | [0,0,0] |
| cube | `uv` | float[2] | [0,0] |
| cube | `mirror` | bool | false |

The steps below run in order.

### 10.2 Read and inherit

1. Read the text BOM-aware and set the model name to the file stem. This happens before parsing.
2. Deserialize the text and clear the selection.
3. **Inheritance.** Take every key containing `:`. The text after the **last** `:` is a parent key. If
   a geometry with exactly that key exists, the child receives copies of the parent's bones it does
   not define. A bone counts as defined when the child has a bone with the same name, compared
   case-insensitively, that has at least one cube. The copies are placed before the child's own bones,
   in parent order.
4. Truncate every key at its **first** `:`. Two keys that become equal fail the import.
5. The default name is the first key in document order, with every occurrence of `geometry.` removed.

### 10.3 Name prompt

A text input dialog titled `Name of entity model to import` opens, pre-filled with the default name.
Confirming it runs the build (10.4) with the entered text `<name>`.

Lookup is case-insensitive. A `*` in the pattern matches any run of characters and a `?` matches one
character, so wildcards typed by the user also work.

1. Look for the first key matching the pattern `*<name>`, which means the key ends with the name.
2. Otherwise, look for the first key matching `*<name>*`, which means the key contains the name.
3. Otherwise use `<name>` itself.

When the chosen key does not exist, an error box titled `MCPE entity Import` opens with the text
`Entity name "<name>" not found in selected file.` + LF + LF + `Are you sure it's spelled correctly?`.
Its buttons are `Retry`, which reopens the prompt with the same text, and `Cancel`.

### 10.4 Build

1. **Reset.** If replace is set, reset the model with base textures.
2. **Texture entry.** Create and select a texture entry. Its label is `<name>` with `geometry.`
   removed. Its image is the **UV mask**:
   - a `W x H` RGBA image, where `W = texturewidth` and `H = textureheight`;
   - cleared to transparent;
   - for every cube of every bone (including `neverRender` bones), the box-net footprint is painted in
     one random opaque colour per cube;
   - with `u, v, w, h, d` = `roundHE` of the cube's `uv` and size x, y, z, the footprint covers image
     rows `[v+d, v+d+h)` (top-down) for columns `[u, u+2d+2w)`, and rows `[v, v+d)` for columns
     `[u+d, u+d+2w)`;
   - a footprint that falls outside the image fails the import. Horizontal overflow wraps into the
     next row instead of failing, because pixels are addressed linearly.
3. **Elements.** Visit bones in order. Skip a bone when it has no cubes or when `neverRender` is true.
   Each cube `i` of a bone creates one cube element.
   - **Position.** In E: `(8 - o.x - sz.x, o.y, -8 - o.z)`, where `o` is the origin and `sz` the raw
     size.
   - **Size.** `(max(0.001, sz.x), max(0.001, sz.y), max(0.001, sz.z))`.
   - **Faces.** All six faces get the texture label from step 2.
   - **Label.** The bone name when the bone has one cube, otherwise `<bone name>_c<i+1>`.
   - **Pivot.** In E: `(8 - p.x, p.y, -8 - p.z)`.
   - **UVs.** Assign the face rectangles of 10.5.
   - **Rotation.** The bone rotation `(r.x, r.y, r.z)`, used directly as the editor Euler angles,
     through the storage rule 2.2.1. "Restrict rotations" is still at its reset value (true) when
     replace is set, so rotations snap to **multiples of 22.5** at this step.
4. **Model settings.**
   - Model type: Entity.
   - Minimum texture size: `etw = ceil(W)` and `eth = ceil(H)`.
   - `esb` = false, `ecn` = true, `etx` = 0, `ety` = 0.
   - "Restrict rotations" = false. This is set only now.
   - Model origin: E (8, 0, -8).
5. **Finish.** Refresh the element list. The log line is `Elapsed time to load: <s>s`.

**Errors.** Any other failure shows `MCPE entity Import` with the text `An error occured while
importing MCPE entity:` + LF + `<message>` and the button `Abort`.

**[IMPORT-MODE] Rotation snapping.** In `modern` the port imports bone rotations without snapping.
To do so it disables the snap for the duration of the import.

### 10.5 Face UV rectangles

The rectangles are normalized `(x, y, w, h)` with Y measured from the top. They are stored with the
rule in 2.3. `T()` truncates toward zero and applies only to the sums shown. Size components may be
negative to mirror a face.

| Face | x (not mirrored) | y | w | h |
|---|---|---|---|---|
| down | T(u+d+2w)/W | v/H | -w/W | d/H |
| up | T(u+d+w)/W | (v+d)/H | -w/W | -d/H |
| north | T(u+d)/W | (v+d)/H | w/W | h/H |
| south | T(u+2d+w)/W | (v+d)/H | w/W | h/H |
| west | T(u+d+w)/W | (v+d)/H | d/W | h/H |
| east | T(u)/W | (v+d)/H | d/W | h/H |

Mirrored applies when the bone's or the cube's `mirror` is true. The y and h columns of mirrored faces
are as above, with these changes.

- down: `x = T(u+d+w)/W`, `w = w/W`.
- up: `x = T(u+d)/W`, `w = w/W`.
- north: `x = T(u+d+w)/W`, `w = -w/W`.
- south: `x = T(u+2d+2w)/W`, `w = -w/W`.
- west and east swap their rectangles. West takes the unmirrored east rectangle and east takes the
  unmirrored west rectangle.

---

## 11. Bedrock legacy entity export (`.js`)

### 11.1 Steps

1. **Pre-check.** Run the 6.5 check with the name `Minecraft PE Entity Model`.
2. **Status.** Set it to `Exporting...`.
3. **Atlas.** Build the entity atlas (section 12) from all elements. Hidden elements are excluded when
   "Exclude hidden elements" is on.
4. **Texture size.** Let `k = etr/16` as a float. Then `TW = ceil(atlas pixel width / k)` and
   `TH = ceil(atlas pixel height / k)`. The geometry is
   `{texturewidth: TW, textureheight: TH, bones: []}`.
5. **Elements.** Visit every element with index `i` (0-based, counting all elements). Skip it if it is
   hidden and "Exclude hidden elements" is on.
   - **Inputs.**
     - `rect` is the element's normalized atlas rectangle, measured from the bottom.
     - `uv = (rect.x·TW, TH - rect.yMax·TH)`.
     - `O = (origin.x, origin.y, -origin.z)` is the model origin in M.
     - `P = corner_M` is the **rotated** stored corner.
     - `Q = pivot_M`.
     - `s` is the element size.
   - **Bone mode** ("Group all elements as unique bone", the default).
     - When the first element is emitted, create one bone with:
       - name: the model name;
       - pivot: `O + (-8, 0, -8)`;
       - rotation: (0,0,0);
       - mirror: false;
       - parent: the attach name, omitted when empty.
     - Each element adds a cube to that bone:
       - origin: `(8 - P.x - s.x, P.y, P.z - 8)`;
       - size: `s`;
       - uv: as above;
       - name: `e<i+1>_<Sanitize(label)>` when "Use elements label for names" is on, otherwise omitted.
     - Element rotations are dropped.
     - If every element is skipped, `bones` stays empty. That is not an error.
   - **Per-element mode.** Each element becomes its own bone with:
     - name: `Trim(label)` when "Use elements label for names" is on, otherwise `e<i+1>`;
     - pivot: `(16 - Q.x, Q.y, Q.z) - O`;
     - rotation: the stored editor Euler angles `e`, unchanged (degrees in [0,360));
     - mirror: false;
     - parent: the attach name, omitted when empty;
     - one unnamed cube with origin `(16 - P.x - s.x, P.y, P.z) - O`, size `s` and the uv above.
6. **Serialize.** Use the JSON serializer.
   - Null members are omitted, numbers use NJ form, and primitive arrays are written compactly
     (`[8.0,0.0,-8.0]`) even in indented mode.
   - The output is indented, or compact when "Compress data" is on.
   - **Key order.**
     - geometry: `texturewidth`, `textureheight`, `bones`;
     - bone: `name`, `parent`, `pivot`, `rotation`, `mirror`, `neverRender`, `cubes`;
     - cube: `name`, `origin`, `size`, `uv`, `mirror`.
   - `mirror` and `neverRender` are always written, as `false`.
7. **Text.** The file text is a four-line header, then the JSON. Each header line ends in LF.
   ```
   // <app identity> Minecraft PE Entity exporter
   // <description>
   //
   // This export needs to be tweaked, copy and pasted in Mobs.json file to redefine any MCPE entity
   <empty line>
   <JSON>
   ```
   No final newline follows. Indented JSON uses NL (CRLF) between its own lines, so the file mixes LF
   and CRLF. A description that contains newlines breaks the header comment, as in the reference.
8. **Write.** Write the text to the chosen path, then write the atlas PNG to `<dir>/<stem>.png`.

### 11.2 Known reference defects (kept in `cubik29`)

- **[FIX-MODERN] Double rotation.** Rotated elements in per-element mode use the rotated corner `P`
  as the cube origin and also emit the bone rotation, so the game applies the rotation twice.
- **Mirrored X.** Positions mirror X around x = 8 (bone mode) or x = 16 minus origin (per-element
  mode). Rotation angles are written as stored, which is the inverse of the `.mcpe` import mapping
  (10.4). Whether this matches the game's handedness for rotated bones is **UNVERIFIED** (T-JS-1).

`Sanitize(t)` replaces every character outside `[A-Za-z0-9 ]` with a space, applies invariant title
case, then removes the spaces. Title case uppercases the first letter of each word and lowercases the
rest, and leaves words that are entirely uppercase unchanged. How a letter right after a digit is
treated is **UNVERIFIED**.

---

## 12. Entity texture atlas (shared by `.js` and `.java`)

### 12.1 Per-element nets

The input is the element list, with hidden elements dropped when "Exclude hidden elements" is on. Let
`k' = NextPowerOfTwo(etr)/16`. `etr` is always a power of two between 16 and 512, so `k' = etr/16`.

1. **Face images.** For each face, take its texture region, which the texture specification defines
   as a face image. When the face has none, use a transparent image.
2. **Resize.** Resize each face image with nearest-neighbour sampling to the sizes below.
   - down and up: `ceil(s.x·k') x ceil(s.z·k')`.
   - north and south: `ceil(s.x·k') x ceil(s.y·k')`.
   - west and east: `ceil(s.z·k') x ceil(s.y·k')`.
3. **Net layout.** Build the element **net**. Its width is `east.w + north.w + west.w + south.w` and
   its height is `up.h + north.h`.
   - The bottom band, from left to right: east, north, west, south.
   - The top band: up at `x = east.w`, then down at `x = east.w + north.w`.
   - In the usual top-down view, the top band (height = z depth) holds up then down, offset by the
     depth. The bottom band (height = y size) holds east, north, west, south.
4. **Dedup.** When "Use same element texture for doublons" is on, compare each new net with the nets
   already kept. Two nets match when they have the same size, format and filter mode and equal pixels.
   A matching element reuses the existing net. Otherwise the new net is appended.

### 12.2 Packing

1. **Pack.** Pack the nets with a rectangle packer, with padding 0 and a maximum size equal to the
   device's maximum texture size. The result is a packed image of `Pw x Ph` pixels and one normalized
   rectangle per net, measured from the bottom.
   - The reference packer is the engine's built-in packer. Its layout cannot be reproduced.
   - The port MUST use a deterministic packer.
   - Parity is defined per element: sampling the port's atlas through the port's UVs MUST give the
     same pixels as sampling the reference atlas through the reference UVs.
2. **Cells.** One cell is `trunc(etr/16)` pixels. Then `cw = trunc(Pw/cell)` and `ch = trunc(Ph/cell)`.
3. **Final atlas.** The final atlas is `AW x AH` cells, where `AW = max(etw, etx + cw)` and
   `AH = max(eth, ety + ch)`, so `AW·cell x AH·cell` pixels.
   - It is cleared to transparent.
   - The packed image is copied so that its top-left corner sits `etx` cells from the left and `ety`
     cells from the top.
4. **Rectangles.** They are remapped to the final atlas, still normalized and measured from the bottom.
   - `x' = (x·cw + etx)/AW`
   - `y' = (y·ch + AH - ch - ety)/AH`
   - `w' = w·cw/AW`
   - `h' = h·ch/AH`

Defaults: `etr` 16, offsets 0, minimum 64x64. A default atlas is therefore at least 64x64 pixels with
one cell per pixel.

---

## 13. Java entity model source (`.java`)

The target is a Forge or MCP-era `ModelBase` subclass, for Minecraft 1.7 to 1.13-era mods.

### 13.1 Steps

1. **Pre-check.** Run the 6.5 check with the name `Forge Java model`.
2. **Status.** Set it to `Exporting...`.
3. **Atlas and texture size.** Build the atlas (section 12) and compute `TW` and `TH` as in 11.1.
4. **Class name.** `Class = Sanitize(model name)`.
5. **Elements.** Visit every element with index `i` (counting all elements). Skip it if it is hidden
   and "Exclude hidden elements" is on.
   - **Field name.** `e<i+1>_<Sanitize(label)>` when "Use elements label for names" is on, otherwise
     `e<i+1>`.
   - **Geometry.** These values come from the unrotated corner.
     - `P = unrot_M` (2.2.2), `Q = pivot_M`, `O = (origin.x, origin.y, -origin.z)`, `s` = size.
     - `base = (P.x - 8, 24 + P.y - s.y, P.z - 8) - O`.
     - `d = (Q.x - P.x, s.y - (Q.y - P.y), Q.z - P.z)`.
     - Rotation point: `base + d`.
     - Box offset: `-d`.
     - Box size: `(ceil(s.x), ceil(s.y), ceil(s.z))`, as integers.
   - **Texture offset.** `U = rect.x·TW` and `V = TH - rect.yMax·TH`. Both are floats in G7 and are
     not rounded, so a fractional value produces invalid Java.
   - **Angles.** `e.x·(π/180)`, `e.y·(π/180)`, `e.z·(π/180)`, in single precision, using the raw
     stored editor angles. They are written in G7.
6. **Assemble.** Build the text from the template in 13.2.
7. **Write.** Write the text to the chosen path and the atlas PNG to `<dir>/<stem>.png`. The PNG is
   written even if the text write failed.

### 13.2 Text template

Every newline in the template is LF and there is no final newline. The template uses
`<A>` for the app identity and `<D>` for the description. Each line marked `<repeat>` is emitted once
per element, in element order, with `<f>` as the field name.

```
// <A> JAVA exporter
// <D>

import net.minecraft.client.model.ModelBase;
import net.minecraft.client.model.ModelRenderer;
import net.minecraft.entity.Entity;

public class <Class> extends ModelBase {

    //fields
    public ModelRenderer <f>;                                   <repeat>

    public <Class>()
    {
        textureWidth = <TW>;
        textureHeight = <TH>;

        <f> = new ModelRenderer(this, <U>, <V>);               <repeat block, 6 lines>
        <f>.setRotationPoint(<RPx>F, <RPy>F, <RPz>F);
        <f>.addBox(<BOx>F, <BOy>F, <BOz>F, <W>, <H>, <D>);
        <f>.setTextureSize(<TW>, <TH>);
        <f>.mirror = false;
        setRotation(<f>, <Rx>F, <Ry>F, <Rz>F);
    }

    public void render(Entity entity, float f, float f1, float f2, float f3, float f4, float f5)
    {
        super.render(entity, f, f1, f2, f3, f4, f5);
        setRotationAngles(f, f1, f2, f3, f4, f5, entity);

        <f>.render(f5);                                          <repeat>
    }

    private void setRotation(ModelRenderer model, float x, float y, float z)
    {
        model.rotateAngleX = x;
        model.rotateAngleY = y;
        model.rotateAngleZ = z;
    }
     
    public void setRotationAngles(float f, float f1, float f2, float f3, float f4, float f5, Entity entity)
    {
        super.setRotationAngles(f, f1, f2, f3, f4, f5, entity);
    }
 
}
```

Exact whitespace rules for the template:

- **Indentation.** It is four spaces.
- **Blank lines.** Each of these is present even when no element is emitted.
  - An empty line follows the class header line, before `//fields`.
  - An empty line separates the field list from `public <Class>()`.
  - An empty line follows `textureHeight = <TH>;`.
  - An empty line separates the constructor's `}` from `public void render`.
  - An empty line follows the `setRotationAngles(...)` call inside `render`.
  - One empty line precedes `private void setRotation`.
- **Whitespace-only lines.**
  - The line after the first method's closing brace holds exactly five spaces.
  - The line before the final `}` holds exactly one space.
- **Element blocks.** The 6-line per-element blocks follow each other without blank lines.
- **No elements.** An exporter run with zero emitted elements still writes the fixed lines.

### 13.3 Known reference behaviour

- **Y placement.** The rotation point Y is `24 + P.y - s.y + d.y`, and the box spans
  `[24 + P.y - s.y, 24 + P.y]` in Java model Y, which points down. A Cubik element whose bottom sits
  above y = 0 therefore lands below the ground line. **[FIX-MODERN]** Modern templates use
  `24 - P.y - s.y`. Whether this is a reference bug or an intended convention is **UNVERIFIED**, and
  it needs an in-game check.
- **Angles.** Angles are raw editor angles. Neither the sign nor the axis is converted.

---

## 14. OptiFine part model (`.jpm`)

Cubik writes OptiFine "custom entity model" part files only. It writes no JEM files, and it reads
neither format.

### 14.1 Steps

1. **Pre-check.** Run the 6.5 check with the name `Optifine Model Part`.
2. **Status.** Set it to `Exporting...`.
3. **Base name.** `base` is the stem of the target path.
4. **Atlas.** Pack every texture entry's image, in list order, with padding 0 and the device maximum
   size (same packer caveat as 12.2). The result is an image of `W x H` pixels and one normalized
   rectangle per texture label, measured from the bottom.
5. **Root object.**
   - `comment`: the description.
   - `createdWith`: the app identity.
   - `texture`: `base`.
   - `textureSize`: `[W, H]`.
   - `submodels`: built in step 6.
6. **Submodels.** Every element `j` adds a submodel. `j` is 0-based, and hidden elements are
   **included**.
   - `id`: `e<j>_<label>`. The label is raw.
   - `translate`: `origin_E - c_E`, the model origin minus the stored (rotated) corner, both in editor
     coordinates. It is omitted when it is approximately zero.
   - `rotate`: the stored editor Euler angles `e`. It is omitted when approximately zero.
   - `boxes`: a single box with `coordinates` set to `[-s.x, -s.y, 0, s.x, s.y, s.z]`.
   - **Face UVs.** For each enabled face, in the order down, up, north, south, west, east, write
     `uvDown`, `uvUp`, `uvNorth`, `uvSouth`, `uvWest` or `uvEast`. Disabled faces are omitted. The
     values are computed as follows.
     1. Let `r` be the atlas rectangle of the face's texture label, and `y' = (r.y + 1 - r.h) mod 1`,
        which converts the rectangle to top-down.
     2. Let `(a0, b0) = uv0/16` and `(a1, b1) = uv1/16`.
     3. Compute `x = (r.x + a0·r.w)·W` and `y = (y' + b0·r.h)·H`.
     4. Compute `w = max(1, (a1 - a0)·r.w·W)` and `h = max(1, (b1 - b0)·r.h·H)`. A mirrored UV
        therefore collapses to 1 pixel.
     5. The value is `[trunc(x), trunc(y), trunc(x + w), trunc(y + h)]`.
     6. A label missing from the atlas aborts the export with no file and no message (**UNVERIFIED**
        user-visible effect). **[FIX-ALWAYS]** The port reports an error.
   - **Progress.** Every 500 elements the status shows `Exporting Elements: <p>%` and the exporter
     yields one frame.
7. **Serialize.** The rules are those of 11.1 step 6.
   - **Key order.**
     - root: `comment`, `createdWith`, `texture`, `textureSize`, `submodels`;
     - submodel: `id`, `translate`, `rotate`, `boxes`;
     - box: `coordinates`, `uvDown`, `uvUp`, `uvNorth`, `uvSouth`, `uvWest`, `uvEast`.
   - Null members are omitted.
8. **Write.** Write the JSON to the chosen path and the atlas PNG to `<dir>/<base>.png`.

**[FIX-MODERN] Rotation pivot.** The rotation pivot is the rotated corner, not the element pivot.
`invertAxis` is not written. Section 31 describes the modern JEM and JPM writer.

## 15. Wavefront OBJ import (`.obj` with `.mtl`)

OBJ import is reached through File > Import, and through the mesh tool "Add custom mesh". The second
path opens a dialog titled `Add custom mesh` with the filter name `Mesh` and the extension `obj`. Both
routes use the same importer. The replace flag is ignored, so the import always adds meshes to the
current model.

### 15.1 Reading

- **Material file.**
  - Its path is the OBJ path with its **last four characters** replaced by `.mtl`. The `mtllib` record
    is ignored.
  - A missing material file means "no materials".
- **Texture folder.** Take the OBJ path with `\` replaced by `/`, cut after the last `/`. A path with
  no `/` keeps the whole path, which is a reference bug.
- **Texture files.** For every material with `map_Kd`, try the name as given, then
  `<folder><name>`, then `<folder>textures/<name>`.
  - The first existing file is decoded by the engine's image decoder, which handles PNG and JPEG
    only. Other formats silently produce no texture.
  - Textures use point filtering and are keyed by the raw `map_Kd` text.
- **Object name.** The default object name is the second-to-last token of the path split on `\`, `/`
  and `.`, which is normally the stem.

### 15.2 Parser

The text gets a trailing LF and is scanned once, line by line.

**Records.** A record is recognised only at the start of a line. Leading spaces and tabs are allowed.

| Record | Effect |
|---|---|
| `#`, `s`, `mtllib`, unknown | Ignored. |
| `o <name>` | Starts a new geometry group if the current one already has positions. The first name becomes the top-level name. |
| `g <name>` | Starts a new group when the current one is non-empty. Groups always become separate objects in this importer. |
| `usemtl <name>` | Reuses an existing submesh slot with that exact, case-sensitive name, or opens a new slot when the current one is non-empty, then names the slot. |
| `v x y z` | Position. |
| `vn x y z` | Normal. |
| `vt u v` | UV. A third value is ignored. |
| `f ...` | Polygon, triangulated (below). |

**Tokens and numbers.**
- Tokens are separated by **single spaces**. Runs of spaces collapse, but tabs are not separators.
- Numbers are parsed in the invariant culture. A failure yields 0 silently.
- Thousands separators are accepted, so `1,5` parses as 15.

**Face corners** *(spot-checked)*. A corner is `p`, `p/t`, `p//n` or `p/t/n`.
1. **Position index.** A positive value is 1-based. A negative value is relative to the count so far.
   The value 0 selects the first position. A corner out of range is dropped and logged as
   `Bad vertex index`.
2. **UV and normal indices.** They follow the same rules, with two quirks.
   - A **missing** component selects the **last** declared value. Without any declared value it
     defaults to (0,0) for a UV and (0,0,-1) for a normal.
   - An **empty** component, as in `1//3`, selects the **first** declared value.
3. **Vertex reuse.** Each position caches its last output vertex. The cached vertex is reused when the
   position, normal and UV are all approximately equal.
4. **Normal emission.** Normals are emitted only if at least one normal had been declared by the time
   the face is read.

**Triangulation** *(spot-checked)*. Three corners give one triangle. For n >= 4 the polygon is split along the diagonal
from corner 0 to corner `floor(n/2)` into the chain `0..floor(n/2)` and the chain
`floor(n/2)..n-1,0`, and each chain is split recursively. The first chain's triangles come first.

**MTL records.**

| Record | Effect |
|---|---|
| `newmtl <name>` | Starts a material. The name is trimmed. |
| `Kd r g b` | Diffuse colour, alpha 1. Materials created after the first default to white. |
| `map_Kd <file>` | Main texture name. |
| `map_d` | Never matches, because the reference compares a 7-character slice with a 6-character keyword. |

### 15.3 Object building

1. **Hierarchy.** One group produces one object. Several groups produce a parent object, named after
   the first geometry or else the default object name, with one child per group. Empty groups are not
   emitted.
2. **Right-hand flip.** Negate X on positions and normals, and swap the first two indices of every
   triangle. The importer is always called with identity rotation, unit scale and zero translation.
3. **Chunking.** A group is split into chunks of at most **65534** output vertices.
   - Each chunk becomes an object named after the group, or `<name>_part<n>` when the group was split.
   - The triangle that did not fit at a chunk boundary is **skipped**. That is a reference bug, and
     **[FIX-ALWAYS]** the port keeps the triangle.
   - Submeshes become materials named after their slot, or `mat<n>` when the slot has no name. Each
     material takes the MTL colour and texture.
4. **Mesh data.** Normals are recalculated when none were declared.
5. **Errors.**
   - A file without faces produces no object. The reference then fails.
   - Any failure shows an error box titled `Error while loading OBJ` with the text
     `Failed importing OBJ model at:` + LF + `<path>`.

### 15.4 Adoption as meshes

All imported objects are placed under the meshes node. Let `B` be the combined bounds of **all**
renderers under that node, including meshes that existed before. Every object with a renderer becomes
a mesh, is added to the mesh list and is selected.

1. **Import offset.** Store `(minX - pos.x, minY - pos.y, maxZ - pos.z)` of the object's own bounds.
2. **Normalization factor.** Store `1/mean(size)`. The block-size tool uses it.
3. **Scale.** Let `m = trunc(max(B.size))`.
   - When `m <= 3`, multiply the scale by 16.
   - Otherwise halve a factor, starting at 1, while `m·factor > 32`, and multiply the scale by the
     result.
   - The log lines are `Imported size: <dx>, <dy>, <dz>` and, when the factor is not 1,
     `Applied new scale to Mesh: x<factor>`.
4. **Centring.** When "Center model and origin on import" is on, set the pivot to `B.center·factor`,
   then move the pivot to E (8, 8, -8). The object moves with it.
5. **Label and flags.**
   - The label is the object name.
   - The mesh is flagged as imported, and its restrict flag is cleared, which excludes it from the
     22.5° rotation snap (2.2.1).

The reference behaviour is kept in `cubik29`. **[IMPORT-MODE]** In `modern` the importer:
- splits on any whitespace;
- treats index 0 as an error;
- keeps the empty-component rule consistent with the missing-component rule;
- reads `mtllib` and `map_d`;
- triangulates concave polygons by ear clipping;
- measures `B` on the new import only.

---

## 16. Wavefront OBJ export (`.obj`, `.mtl`, `.png`)

### 16.1 Options

| Option | Source |
|---|---|
| Include elements, meshes, voxels | not "Exclude elements", not "Exclude meshes", not "Exclude voxels" |
| Texture mode `T` | `obj.gmt` OR `obj.omt` |
| Optimized atlas (greedy merge) | `obj.omt` |
| Atlas padding | `obj.pt`: 2 px padding and a gutter fill when on, no padding when off (16.7.7) |
| Smooth voxels | `obj.sv` |
| No-weld | `obj.wv`. True means vertices are **not** de-duplicated. |
| Merge | `obj.mo`. True suppresses the `g` lines. |
| Vertex colours | on when `T` is false, for every part |
| Offset | `-origin_E` |

Notes on the options:

- **`obj.gmv`.** It is not read. "Detailed mesh with colored vertices" is simply the state in which
  both texture options are off.
- **Exclusive mesh types.** In Model Properties the three mesh-type options form one radio group, so
  exactly one of them is on. The exporter accepts any combination, and `gmt` with `omt` behaves as
  `omt`.
- **`obj.ao`.** The writer does not read it. The option sets the voxel world's ambient-occlusion flag
  when the world is created or the option changes. The viewport chunk meshes multiply their vertex
  colours by an occlusion factor while that flag is on, so detailed-mode exports carry AO-darkened
  vertex colours. Texture-mode output never contains AO (16.7).
- **"Exclude hidden elements".** It is not read by the OBJ exporter (16.2).

### 16.2 Parts

Parts are collected in this order. Every enumeration includes inactive objects, so **hidden elements,
meshes and voxel chunks are exported**. "Exclude hidden elements" has no effect here, unlike in the
entity exporters (11 to 13). Only the empty-model check (16.6) looks at active renderers alone.

1. **Element parts.** Every renderer under the elements node, in hierarchy order.
2. **Mesh parts.** Every renderer under the meshes node: primitives and imported meshes.
3. **Voxel parts.**
   - **Detailed mode** (T false, the default). Every chunk renderer under the voxel node becomes a
     part flagged for smoothing. Chunk meshes carry vertex colours (darkened by AO when it is on,
     16.1) and normals but no UVs, so their faces use the `v//vn` form (16.3).
   - **Texture mode** (T true, voxel world initialized). The voxel mesher of 16.7 builds rectangles
     (greedy-merged when optimized), chunk meshes and one atlas. The same branch also runs when the
     voxel node is missing, even with both texture options off, and then without greedy merge.
     - All chunk parts share one material named `voxel`. It is a copy of the **element** material
       (16.5), with colour white and the atlas as main texture. The atlas is named `voxelatlas`.
     - Chunk mesh `l` becomes the part `voxels_<l>`.
     - Its position is `(-16 + min.x, -16 + min.y, 16 - min.z) + offset`, where `min` is the
       write-region minimum in grid units. Its scale is `16/N`, and its rotation is Euler(-90, 0, 0).
     - **The position is correct only for N = 16.** The minimum is not multiplied by the scale, so
       for other N the part is displaced by `(1 - 16/N)·min` per axis. That is a reference defect,
       **[FIX-MODERN]**.
     - The part is flagged for smoothing only when the atlas is not optimized.

A part takes the object's name, a copy of its mesh, its materials, and its **local** position (plus the
offset), scale and rotation. Parent transforms are not accumulated. The mesh copy keeps positions,
normals, colours and UVs, but it puts **all triangles into one submesh**, following the engine's
documented triangle-list behaviour (consequences in 16.5).

Two reference crashes are unreachable in practice: a renderer under the voxel node without a mesh
filter, and a flagged vertex that no triangle uses (smoothing below). **[FIX-ALWAYS]** The port skips
such a renderer and leaves such a vertex unchanged.

**Smoothing** (when "Smooth voxels" is on) runs once, after all parts exist, over the parts flagged
for smoothing. Element and mesh parts are never flagged.

1. Build an adjacency map from the triangles of the flagged parts. The keys are vertex positions plus
   the part's position, compared exactly. Each key lists the other two corners of every triangle that
   uses it.
2. For each vertex `p` of a flagged part, take the six candidates `p ± s` along each axis. Keep the
   candidates that appear in `p`'s adjacency list, and move `p` to the mean of `p` and the kept
   candidates. Duplicated positions move together.
3. Write the positions back and recalculate the normals, so smoothed normals are no longer
   axis-aligned.

`s` is the voxel world's step, `16/N` in world units, set when the world is created. Detailed-mode
chunk vertices are in world units, so the step matches. Atlas vertices are in voxel units (16.7.4), so
candidates match only when N = 16. For any other N the pass leaves every position unchanged and only
recalculates the normals, which stay flat. Under `omt` no voxel part is flagged, so the option has no
effect on voxels at all. Both results are derived from the code (**UNVERIFIED** at runtime, T-OBJ-4).
**[FIX-MODERN]** Modern profiles smooth atlas parts in voxel units, for both texture modes.

### 16.3 Text layout

The OBJ text is built in this order. All `v`, `vn` and `vt` lines of all parts come before all `g`,
`usemtl` and `f` lines.

1. **Header** (NL line endings):
   ```
   # Export of Cubik Model: <model name>
   # <description>
   # from Cubik OBJ Exporter v1.0
   mtllib <base>.mtl
   ```
   `<base>` is the target stem with each whitespace run replaced by `_`.
2. **Materials.** For each part, each material whose name was not yet seen appends an MTL block (16.5)
   and then an empty line (NL).
3. **Empty parts.** A part whose mesh has no triangles writes no vertices, normals, UVs, group or
   faces. Its materials were already written in step 2.
4. **Vertices.** For each vertex `v`:
   - Compute `v' = R·(S ⊙ v) + T`, where `S` is the part scale, `R` the part rotation and `T` the part
     position. Then negate `v'.x`.
   - The line is `v x y z` LF, or `v x y z r g b` LF in vertex-colour mode. The colour is the mesh's
     vertex colour, or white `1 1 1`. Its alpha is not written.
   - All numbers use G7.
   - The arithmetic is the engine's single precision: `R` is the part's rotation quaternion and is
     applied with the engine's quaternion-vector product. Values that would be exact in real
     arithmetic can carry rounding residue, and G7 prints it. With the voxel parts' Euler(-90, 0, 0),
     for example, a coordinate that should be 0 can print as a value of the order of `1E-07`, and a
     unit normal component as `0.9999999` (step 5). Byte parity requires reproducing those
     single-precision operations exactly (**UNVERIFIED** values, T-OBJ-4).
   - **No-weld on** (the default): every vertex is emitted.
   - **No-weld off**: a vertex whose line text was already emitted reuses that index.
5. **Normals.** `n' = R·n`, then negate `n'.x`. The line is `vn x y z` LF. Normals are always
   de-duplicated by text.
6. **UVs.** The line is `vt u v` LF, unchanged. UVs are always de-duplicated by text.
7. **Group.** Unless merge is on, write `g <part name>` with NL.
8. **Faces.** For each submesh `n` (every part has exactly one, 16.2):
   - Write `usemtl <material name>` with NL. When the part has no material at that position, the name
     is `<part name>_sm<n>`. With one submesh this happens only for a part without any material slot.
   - For each triangle `(a, b, c)`, with indices plus the running count of raw vertices of earlier
     parts:
     - Each corner token is `<vi>/<ti>/<ni>` with 1-based indices into the emitted lists.
     - The token is `<vi>//<ni>` instead when the number of UVs recorded so far is **less than** any of
       the three raw indices.
     - An index equal to that count crashes the reference. **[FIX-ALWAYS]** The port uses the `//`
       form there.
     - **Global index alignment.** The UV and normal records run across all parts and are looked up
       with the raw vertex index. They line up only while every earlier part has exactly one UV and
       one normal per vertex. A part with fewer UVs, such as a mesh imported without `vt` or a
       detailed-mode voxel chunk, shifts every later part, whose faces then fall back to `//` or take
       the UV of another vertex. An imported mesh without UVs therefore breaks the UVs of later meshes
       and of texture-mode voxel parts. Kept in `cubik29`; **[FIX-MODERN]** modern profiles index per
       part. The normal lookup has no guard, which is harmless because every exported part kind has
       one normal per vertex.
     - Corner order: with `k = trunc(clamp(scale.x·scale.z, -1, 1))`, write `f a b c` when `k < 0`,
       otherwise `f c b a`. A negative product above -1 therefore truncates to 0 and keeps the
       reversed order.
     - Each face line ends with LF.
9. **Concatenation.** The faces text is appended after all the vertex data.

The output has mixed line endings on the reference Windows build. `cubik29` reproduces them exactly.

### 16.4 Weld polarity

The UI label reads "Weld vertices at exact same coordinates", and its default is on. On means **no**
welding. The `cubik29` profile keeps that polarity. **[FIX-MODERN]** The modern profile inverts the
stored meaning on export only, so "on" welds.

### 16.5 MTL block

Each line ends with NL.

```
newmtl <material name>
Kd <r> <g> <b>                 (only when the material has a main colour)
Tr <1-a>                       (only when alpha < 1)
d <a>                          (only when alpha < 1)
Ks <r> <g> <b>                 (only when the material has a specular colour)
map_Kd <file>                  (main texture present)
map_Ks <file>                  (specular map present)
map_Bump <file>                (bump map present)
illum 2
```

- **Number format.** Colours are floats in [0,1], written in G7.
- **Optional lines.** `Kd`, `Tr` and `d` need a main colour property on the material's shader, and
  `Ks`, `map_Ks` and `map_Bump` need the matching shader property and, for the maps, an assigned
  texture. Which properties the shipped shaders expose is asset data (**UNVERIFIED**). The writer has
  a variant that appends `-blendu off -blendv off` to map lines, but nothing enables it, so those
  options never appear.
- **Blocks are keyed by material name.** A material whose name was already written gets no second
  block, and its textures are not registered. Every part whose material shares that name therefore
  shows the first block's texture.
- **Texture file name.** It is `<base>_<texture name>`, with `.png` appended when missing. The texture
  is registered for writing under that name. A second texture with the same name is skipped, and the
  log line is `Texture with name <name> already exists!`. Its `map_Kd` line still names the file, which
  then holds the first texture of that name.
- **Texture copy failure.** The reference logs `Could not export texture : <name>: <message>` and then
  fails on the missing file name. **[FIX-ALWAYS]** The port logs the same line and omits that map line.
- **Empty material slots.** An empty slot makes the reference fail in both the MTL loop and the
  `usemtl` line. **[FIX-ALWAYS]** The port treats it as a missing slot: no block, and the
  `<part name>_sm<n>` name.
- **No sanitizing.** Material and texture names are written raw, except that `<base>` has its
  whitespace runs replaced. A name with spaces, such as an engine instance name, is split by readers
  that stop at whitespace.

#### 16.5.1 Names by source

The rules below come from the reference code. Items marked **UNVERIFIED** depend on asset data or on
engine naming conventions that were not executed (T-OBJ-3).

| Source | Group (`g`) | Material (`newmtl`, `usemtl`) | Main texture file |
|---|---|---|---|
| Element | `Element #<n>`. `n` is a creation counter that restarts at 1 when the element list is cleared, so deleted elements leave gaps. The element label is not used. | The shared element material, whose asset name is `Element` (**UNVERIFIED** at runtime). | The runtime element atlas. It is created without a name, so the file is `<base>_.png` (**UNVERIFIED**). |
| Primitive | Its kind label: `Cube`, `Sphere`, `Cylinder`, `Cone`, `Pyramid`, `Slope`, `Corner-in`, `Corner-out`, `Torus` or `Sprite`. | An instance of the prefab's material. The engine names instances `<material name> (Instance)` (**UNVERIFIED**). | Its own 128x128 paint texture, named after the kind: `<base>_<kind label>.png`. |
| Imported mesh | The imported object name (15.3), for example the group name or `<name>_part<n>`. | First slot: an instance of the imported material, `<slot name> (Instance)` (**UNVERIFIED**). Other slots keep their names (`mat<n>` when unnamed, 15.3); they get MTL blocks but no `usemtl` line. | Imported textures are created without a name, so `<base>_.png` (**UNVERIFIED**). |
| Voxel chunk, detailed mode | `Chunk_<cx>x<cy>x<cz>`: the chunk's indices in the chunk grid. | The chunk prefab's material. Its name is not visible in the analysed material (**UNVERIFIED**). | None expected; colours are per vertex. |
| Voxel part, texture modes | `voxels_<c>` | `voxel` | `<base>_voxelatlas.png` |

The element and primitive labels shown in the editor never reach the OBJ, because renaming an object
changes its label only, not the scene object name.

Consequences, all kept in `cubik29`:

- **Unnamed textures collapse.** The element atlas and every imported texture share `<base>_.png`,
  and only the first registered one is written. With elements present, imported meshes therefore
  reference the element atlas.
- **Shared material names.** Primitives whose prefabs share one material asset get a single MTL
  block, so every one of them shows the first primitive's texture. Two primitives of the same kind
  share the texture file name in any case.
- **One `usemtl` per scene part.** Because the part copy has one submesh (16.2), a multi-material
  imported mesh is written with all its faces in the first slot's material.

**[FIX-MODERN]** Modern profiles write unique sanitized names, one PNG per distinct texture and one
`usemtl` per submesh (31.9).

### 16.6 Dialogs and save

- **Empty model.** The check covers active renderers under the model root. With none, an error box
  titled `Empty model` appears with the text `Model not exported...` + LF + LF + `Please, make sure at
  least one element, one voxel, one mesh or one primitive exists before exporting.`, and the export
  stops. Hidden objects are normally inactive, so a model whose objects are all hidden is refused,
  while a model with one visible object also exports all its hidden ones (16.2). The check does not
  look at the voxel data itself.
- **Wait box.** Otherwise a wait box titled `Exporting to Wavefront OBJ` appears with the text `Your
  model is exported as a mesh with selected parameters.` + LF + LF + `That can take a few seconds or
  minutes depending on mesh complexity, your computer CPU and available memory.` + LF + LF +
  `Please wait...`. The exporter waits 0.1 s of real time, builds, then closes the box.
- **Silent mode.** The Sketchfab upload (section 29) runs the exporter silently, without the empty
  check and without the wait box.
- **Save order.**
  1. Write the OBJ text to the target. If that fails, stop.
  2. Write the MTL text to `<dir>/<base>.mtl`.
  3. Write every registered texture, in registration order, as `<dir>/<file name>` in PNG format.
  - A failed MTL or texture write shows its error box (6.4), and the remaining files are still
    written. A bare target name gives root-anchored sibling paths, as in 3.7.
- **Log.** `Elapsed time to serialize: <s>s`.

### 16.7 Voxel mesher for the texture modes

This mesher builds the texture-mode voxel parts of 16.2 ("Detailed mesh with texture palette" and
"Optimized mesh with texture atlas"). Only OBJ export uses it. Its inputs are the voxel world, the
greedy switch (`obj.omt`) and the padding switch (`obj.pt`). The reference also contains several
other mesher variants that no live path reaches; they are not part of this specification.

#### 16.7.1 Grid and cells

- The grid is the write region (2.4), not the occupied extent. Its size is `(X, Y, Z) = max - min`,
  and a cell is addressed by its offsets `(a, b, c)` from `min`. `b` is the vertical axis.
- Each cell's packed value is turned into a palette index: the first index that holds the value, or
  0 when no entry does. Index 0 means empty.
- Consequence: a non-zero voxel whose value is missing from the palette counts as empty. The normal
  write path adds only values with a non-zero alpha byte to the palette (2.5), so a voxel stored with
  alpha 0 is left out of this export unless the same value was force-added earlier.

#### 16.7.2 Exposed faces

Every non-empty cell gets six face flags. A face is exposed when the neighbour across it lies outside
the grid or is empty, so the outer shell of the grid is always exposed.

| Face index | Normal | Neighbour |
|---|---|---|
| 0 | +c | `(a, b, c+1)` |
| 1 | -c | `(a, b, c-1)` |
| 2 | -b | `(a, b-1, c)` |
| 3 | +b | `(a, b+1, c)` |
| 4 | -a | `(a-1, b, c)` |
| 5 | +a | `(a+1, b, c)` |

#### 16.7.3 Rectangles

- **Scan.** Visit `a` (outer), then `b`, then `c`, then the face index 0 to 5 (inner). A (cell, face)
  pair is a seed when the cell is non-empty, the face is exposed and the pair is not yet claimed.
  Every visited pair becomes claimed.
- **Without greedy merge** (`gmt`). Every seed becomes a 1x1 rectangle.
- **With greedy merge** (`omt`). The rectangle starts at the seed and grows by whole slices. The
  directions are tried in the fixed order +c, -c, -b, +b, -a, +a, skipping the two directions along
  the face's own normal axis.
  - A slice is accepted only if every one of its cells is inside the grid, non-empty, exposed through
    the same face and not yet claimed for that face. Accepted slices are claimed for that face.
  - The first refused slice ends growth in that direction. A finished direction is never tried
    again, so the result is not guaranteed to be maximal.
  - Colours are not compared. Cells of different colours merge into one rectangle, and the
    rectangle's texture carries the colours (16.7.6).
- **Record.** A rectangle is its minimum cell `(a0, b0, c0)`, its face, and its spans `w` and `h` along
  the face's two in-plane axes `U` and `V`:

  | Faces | `U` | `V` |
  |---|---|---|
  | ±c | `a` | `b` |
  | ±b | `a` | `c` |
  | ±a | `b` | `c` |

- **Order.** Rectangles keep their discovery order.

#### 16.7.4 Quads

- **Plane.** The quad lies at `P` along the face's normal axis: `c0 + 1` for +c, `c0` for -c,
  `b0 + 1` for +b, `b0` for -b, `a0 + 1` for +a and `a0` for -a.
- **Corners.** With `q0` the rectangle's minimum corner on that plane, the corners are `q0`,
  `q0 + w·U`, `q0 + h·V` and `q0 + w·U + h·V`, in that order. Every quad has four vertices of its own;
  none are shared.
- **Mesh frame.** A grid point `(a, b, c)` becomes the mesh vertex `(x, y, z) = (a, c, b)`, in voxel
  units, with no offset and no scale.
- **Triangles.** In local corner indices, faces +c, -b and +a use `(2, 1, 0)` and `(3, 1, 2)`. Faces -c,
  +b and -a use `(2, 0, 1)` and `(3, 2, 1)`. With these orders both triangles of every quad face
  outward in the engine's convention.

#### 16.7.5 Chunks

- Rectangles are cut, in discovery order, into chunks of at most 16200. A chunk therefore has at most
  64800 vertices and stays under the 16-bit index limit. The chunk count is `ceil(n / 16200)` for `n`
  rectangles.
- With no rectangle at all (an initialized but empty voxel world) there is no chunk, and the reference
  hands an empty list to the packer. If the engine accepts that, nothing voxel-related reaches the
  files, because the `voxel` material and the atlas are only written through a part (**UNVERIFIED**
  whether the engine accepts it). The port writes no voxel part and no atlas in this case.

#### 16.7.6 Textures per rectangle

- **Layout.** A rectangle of spans `w` by `h` gets a texture of `(w+2)` by `(h+2)` texels. Column `n`
  runs along `U` and row `m` along `V`, with row 0 at the bottom, as in the engine's texture
  addressing.
- **Interior.** Texel `(n+1, m+1)` holds the colour of the cell `(a0, b0, c0) + n·U + m·V`, which is a
  cell of the rectangle's own layer (the plane offset of 16.7.4 does not apply). The colour is the
  palette colour of that cell's index, with its stored alpha.
- **Border.** The one-texel border repeats the nearest interior texel: edge texels copy the adjacent
  interior edge texel, and the four corners copy the interior corner texels.
- **Solid rectangles.** When every cell of the rectangle has the same palette index, the rectangle
  uses a 2x2 texture of that colour instead. Within a chunk these swatches are shared per palette
  index: a swatch is created at its first use and reused afterwards. Non-solid textures are never
  shared.
- **Consequence for `gmt`.** Every rectangle is 1x1 and therefore solid, so each chunk holds exactly
  one 2x2 swatch per distinct colour it uses. That set of swatches is the "texture palette" of the UI
  label; no separate palette image is written.
- **Chunk lists.** Each chunk keeps its textures in creation order: a swatch at its first use, and a
  non-solid texture at its rectangle's position.
- **Settings.** 8-bit RGBA, clamp, point filter, no mipmaps.
- **Bytes.** Texel bytes equal the voxel colour bytes, because the byte-to-float-to-byte path is exact
  (2.6). For non-solid textures the engine performs that conversion (expected exact, **UNVERIFIED**).

#### 16.7.7 Atlas

- **Packing.** The texture lists of all chunks, concatenated in chunk order, go to the engine's
  rectangle packer. The padding is 2 texels when `obj.pt` is on and 0 otherwise, and the maximum size
  is the device's maximum texture size. The packer returns one normalized rectangle per texture,
  measured from the bottom, in input order. Its layout and atlas size carry the caveat of 12.2.
  Whether the padding lies outside the returned rectangles, whether sizes are powers of two, and the
  documented downscaling when the textures do not fit were not executed (**UNVERIFIED**).
- **Gutter fill** (`obj.pt` on only). One pass over the atlas texels in linear order, reading only the
  packed values:
  - A texel is filled when its alpha is 0 and its lower-left corner lies in no packed rectangle. The
    test is half-open: `xMin <= x < xMax` and `yMin <= y < yMax`, on normalized coordinates.
  - It takes the colour of the first of these neighbours whose alpha is above 0: linear index `i-1`
    (only when `i-1 > 0`), `i+1` (when below the texel count), `i-width` (only when `i-width > 0`),
    then `i+width` (when below the texel count).
  - Left and right are linear neighbours, so they wrap across row ends, and texel 0 is never a
    source.
- **Settings.** Clamp, point filter, no mipmaps, finalized after the gutter fill. The atlas is named
  `voxelatlas` and is written as `<base>_voxelatlas.png` (16.5.1).
- **Bleeding.** The one-texel inset of 16.7.8 and the copied border keep point sampling inside each
  rectangle. Without padding the rectangles touch. With padding the gutter receives neighbour colours,
  which helps viewers that sample linearly.

#### 16.7.8 UVs and normals

- For a quad whose texture has the packed rectangle `(xMin, yMin, xMax, yMax)`, with
  `e = (1/atlas width, 1/atlas height)`, the corners in 16.7.4 order get `(xMin+e.x, yMin+e.y)`,
  `(xMax-e.x, yMin+e.y)`, `(xMin+e.x, yMax-e.y)` and `(xMax-e.x, yMax-e.y)`. `u` follows `U` and `v`
  follows `V`. There is no flip.
- A 2x2 swatch maps all four corners to one point, its centre, so the whole quad shows one colour. A
  non-solid texture maps exactly its interior, and texel `(n, m)` lands on the cell it was taken from,
  on every face.
- Normals are recalculated per chunk after the UVs are set. With unshared vertices each vertex gets
  its quad's flat outward normal. Smoothing (16.2) may change them afterwards.

#### 16.7.9 Placement, smoothing and occlusion

- Parts, the material and the placement are specified in 16.2. A grid offset `(a, b, c)` therefore
  lands in the OBJ at `(-(s·a + p.x), s·b + p.y, -s·c + p.z)`, up to the rounding residue of 16.3. Here
  `s = 16/N` and `p` is the part position of 16.2, origin offset included.
- Smoothing applies to `gmt` parts only, with the unit mismatch described in 16.2.
- Ambient occlusion never applies (16.1).

#### 16.7.10 Port notes

- Reproducible from this section alone: the rectangle set and order, the corner and triangle orders,
  the texel contents, swatch sharing and texture order, the UV formula for given rectangles, and the
  part and material structure.
- Not reproducible without emulating the engine's packer: the rectangle positions and the atlas size,
  and with them the `vt` values and the PNG. As in 12.2, parity is defined per quad: sampling the
  port's atlas at the port's UVs MUST give the reference colours. A packer that emulates the reference
  is optional.
- Memory: the reference keeps a flag array per cell and a claimed array of `X·Y·Z·6` booleans. The
  port SHOULD use bit sets.
- `cubik29` keeps the per-chunk swatches, so a colour is repeated in every chunk that uses it.
  **[FIX-MODERN]** Modern profiles share swatches across chunks and may use a maximal greedy mesher.

---

## 17. STL export (`.stl`)

Binary STL with no dialogs.

1. **Header.** The 80-byte header is `STL - <app identity>`, right-padded with spaces to 80
   characters, in UTF-8. The reference does not truncate a longer header. **[FIX-ALWAYS]** The port
   truncates it to 80 bytes.
2. **Triangle count.** A uint32 LE. It is the number of triangles over all voxel chunk meshes, counted
   only when the voxel world is initialized, plus all element meshes. **Meshes and primitives are not
   exported.** Hidden elements are included, and the exclude options are ignored.
3. **Triangles.** Voxel triangles come first, in chunk order (x outer, then y, then z), followed by
   element triangles in list order. Each triangle is 50 bytes:
   - a normal: 3 float32, the **unnormalized mean** of the three vertex normals;
   - three vertices: 9 float32;
   - attribute: a uint16 equal to 0.
4. **Element transform.** Positions use `v' = R(e)·(s ⊙ v) + c`, the stored corner. Normals use
   `R(e)·n`. There is no axis flip, so coordinates are editor coordinates.
5. **Voxel chunks.** Chunk vertices are written **untransformed**. Chunk meshes are chunk-local, so
   every chunk lands at the chunk origin and the chunks overlap. That is a reference defect,
   **[FIX-MODERN]**. The modern profile applies each chunk's position.
6. **Statuses.** All of them use the name "Stereolithography STL". Each is shown together with the
   progress value in brackets.
   - `Exporting Stereolithography STL voxels...` (0.15), shown when the voxel world is initialized.
   - `... elements...` (0.3), shown when elements exist.
   - `... voxel triangles...` (0.6).
   - `... elements triangles...` (0.9).
   - `Exporting Stereolithography STL finish...` (1.0).
7. **Write.** The file is written with the binary writer (6.4). The log line is
   `STL Export: saved to <path>`.

---

## 18. PNG render export and PNG sprite import (`.png`)

### 18.1 PNG render export ("Model Render")

**Options.**

| Option | Value |
|---|---|
| Size | 1920x1080 when "Main view" is selected, otherwise 512x512 ("GUI display preview", the default). |
| Supersampling | 2 with "Anti-aliasing" on (the default), otherwise 1. |

**Steps.**

1. **Display mode.** Main view sets the display mode to none, and GUI preview sets it to GUI. The
   display-settings specification defines the modes.
2. **Camera.** Copy the position, rotation, field of view, orthographic flag and orthographic size of
   the main (bottom-right) view camera to the render camera. In GUI preview, set the orthographic
   size to 8. That only matters when the copied camera is orthographic.
3. **Hide editor overlays.**
   - Disable the four viewport cameras.
   - Turn off "use elements colour" if it is on.
   - Hide the gizmo root.
   - Wait one frame.
4. **Render.** Render into a `W·aa x H·aa` colour target with a 32-bit depth buffer, then read the
   pixels back.
5. **Restore.** Restore the settings in reverse order, wait one frame, and release the target.
6. **Downscale.** Downscale by `1/aa` with this box filter.
   - The output size is the source size divided by `aa` and rounded with `roundHE`, which gives
     `W x H`.
   - For output pixel `(i, j)`, counted from the bottom-left, with `cx = i·aa` and `cy = j·aa`, average
     source pixels `x` in `[max(0, floor(cx - aa/2)), min(Win, ceil(cx + aa/2)))` and `y` likewise,
     in float.
   - For aa = 2 the window is pixels `{2i-1, 2i}` clipped at 0. It is offset by half a pixel; that is
     reference behaviour.
   - With aa = 1 the image is copied.
7. **Finish.** The status is `Exporting PNG render finish...` with progress 1. The log lines are
   `Png render of Main view` or `Png render of GUI Preview`, then `PNG Export: done`, then
   `PNG Export: saved to <path>` after the write.

The PNG is 8-bit RGBA. Background transparency depends on the camera clear settings (view
specification). The UI promises "PNG 24bits + alpha". Pixel parity cannot be expected across
renderers. Parity tests compare the rendering pipeline settings, not the bytes.

### 18.2 PNG sprite import ("Sprite")

**Loading.** The image is loaded with point filtering.
- On failure the loader may show `Error while loading texture` with the text
  `An error occured while loading "<file>".` + LF + LF + `<message>` and the status
  `Failed loading texture: <path>`.
- The importer then shows an error box titled `Wrong texture` with the text
  `Imported file is not a valid texture to extrude`.

**Options dialog.** A dialog titled `PNG Import` opens, 420x224 in size. Escape cancels, and Enter
does not validate.

| Control | Default | Description |
|---|---|---|
| Disc "Fit in block" (group "Position / scale:") | on | "Defines position of imported sprite" |
| Disc "Fit in world" | off | |
| Toggle "Extrude sprite with optimized elements" | on | "If unchecked, the sprite will be imported as a billboard mesh" |

The buttons are `OK`, `Close` and the window close button. Close does nothing.

**Confirming with OK.**

1. **Ignored flag.** The replace flag is ignored.
2. **Model and name.** The model name becomes the original-case stem, and the selection is cleared.
   Let `n` be the lowercase stem.
3. **Layout.**

   | Mode | Origin `o` (E) | Extent X x Z |
   |---|---|---|
   | Fit in world | (-16, 8, 16) | 48 x 48 |
   | Fit in block | (0, 8, 0) | 16 x 16 |

   The Y extent is 1. The steps are `dx = extentX/W` and `dz = extentZ/H`.
4. **Pixel grid.** Cell `(x, y)` has `y = 0` at the top image row. A cell is opaque when its alpha
   byte is non-zero.
5. **Extrude mode.** Visit `x` = 0..W-1 (outer loop) and `y` = 0..H-1 (inner loop). Each opaque,
   unconsumed cell starts a rectangle.
   - **Width.** Scan right from `x` while cells are not transparent. `maxX` is the last scanned
     column whose cell is free (opaque and unconsumed).
   - **Height.** For each following row, stop when any cell in columns `x..maxX` is transparent.
     Otherwise, when the cell at `maxX` is free, set `maxY` to that row. Rows where it is not free are
     passed over. The rectangle can therefore overlap consumed cells.
   - **Consume.** Mark the rectangle `[x..maxX] x [y..maxY]` consumed. Its size is `w = maxX-x+1` by
     `h = maxY-y+1`.
   - **Element.** Each rectangle creates a cube element.
     - Position (E): `(o.x + x·dx, o.y, o.z - y·dz)`. Size: `(w·dx, 1, h·dz)`.
     - The pivot equals the position, there is no rotation, and the colour is random.
     - Every face gets the label `n`.
   - **Face UVs.** With `u0 = x/W`, `u1 = (x+w)/W`, `vLo = 1 - (y+h)/H`, `vHi = 1 - y/H`, `du = 1/W` and
     `dv = 1/H`:

     | Face | tl | br | UV rotation |
     |---|---|---|---|
     | down | (u0, vHi) | (u1, vLo) | 0 |
     | up | (u0, vLo) | (u1, vHi) | 0 |
     | north | (u1, vHi - dv) | (u0, vHi) | 0 |
     | south | (u0, vLo) | (u1, vLo + dv) | 0 |
     | west | (u0, vLo) | (u0 + du, vHi) | 270 |
     | east | (u1, vHi) | (u1 - du, vLo) | 270 |

   - **Afterwards.** Register a texture entry labelled `n` from the file. The status and log read
     `Successfuly extruded sprite "<n>" with <count> elements`.
6. **Billboard mode.** Create one sprite primitive mesh at the origin `o`, with the extent as its
   scale. A sprite primitive forces scale Y to 0.001.
   - Its label is `Sprite_<Sanitize(n)>` and its texture is a point-filtered copy of the image.
   - The mesh is selected. The status reads `Successfuly imported sprite "<n>"`.
   - No texture entry is registered.

The disc-to-field binding was inferred from the declaration order and the scene order. It is
**UNVERIFIED** at runtime.

## 19. MagicaVoxel (`.vox`)

### 19.1 Import

The model name is set to the stem before parsing. Messages use the name `MagicaVoxel VOX`. All
integers are little-endian.

1. **File header.** Read 4 bytes and require `VOX `, else fail with `Invalid MagicaVoxel VOX format`.
   The next 4 bytes are the version and are not checked.
2. **MAIN.** Read 4 bytes and require `MAIN`, else fail with `Corrupted MagicaVoxel VOX format`. Read
   its content size `n` and its children size, then skip `n` bytes.
3. **Chunk walk.** Walk the remaining chunks as one flat list. Each chunk is an id (4 bytes, UTF-8), a
   content size and a children size.
   - `SIZE`: read three integers `sx, sy, sz` in VOX order, where Z is up.
     - Let `N = max(sx, sy, sz)`, with no cap. When replace is set, re-create the voxel world at `N`.
     - Set the model box from E `(0,0,0)` to `GridToWorld(C+sx, C+sz, C+sy)`.
     - Skip the children.
     - Every `SIZE` chunk does all of this, so in a multi-model file only the last model survives.
   - `RGBA`: read content-size/4 colours `(r, g, b, a)`, then skip the children.
   - `XYZI`: keep the whole content as voxel data, including its leading count, then skip the
     children. A later `XYZI` replaces an earlier one.
   - Any other id (`PACK`, `nTRN`, `nGRP`, `nSHP`, `LAYR`, `MATL`, `rOBJ`, `rCAM`, `NOTE`, `IMAP`, ...):
     skip the content and the children.
   - The reference walk keeps an inaccurate byte counter and runs on past the end of the file, where
     every read returns zero bytes and zero values. The port MUST walk the chunk sizes exactly. The
     outcome is the same for all well-formed files.
4. **Required chunks.** Without a `SIZE` chunk, fail with `Size not found`. Without an `XYZI` chunk,
   fail with `Voxels data not found`.
5. **Palette.** With an `RGBA` chunk, force-add its first `min(255, count)` colours in order. Without
   one, load the built-in VOX palette (2.5). Either way the colours are **appended** to the current
   palette. On a fresh palette, VOX colour index `i` therefore maps to palette index `i`.
6. **Voxels.** Read the voxel data as 4-byte records `(x, y, z, i)`.
   - Each record is written with the bounded write to grid `(C + x, C + z, C + sy - 1 - y)`, with the
     colour at palette index `i`. An out-of-range index gives (160, 210, 255).
   - The reference starts at byte 0, so the count field is also decoded as a voxel record with index
     0. That record clears one cell, which is usually outside the write region.
   - **[FIX-ALWAYS]** The port starts at byte 4.
7. **Finish.** Restore batching and set the grid steps for `N` (view specification).

### 19.2 Export

1. **Prompt.** Run the voxel-only prompt (6.3) with the name `Magicavoxel VOX`.
2. **Status.** `Exporting VOX file...`. No progress is reported.
3. **Region.** The write region gives `min`, `max` and the size `(dx, dy, dz)`.
4. **Bytes.** Write `VOX `, int32 150, then the chunk `MAIN`.
   - Every chunk is a 4-character id (padded with spaces or truncated), int32 content size,
     int32 children size, the content, then the children.
   - `MAIN` has no content. Its children are `SIZE`, `RGBA` and `XYZI`, in that order.
5. **SIZE.** Three int32 values: `dx`, `dz`, `dy`.
6. **RGBA.** 256 entries of `(r, g, b, a)`. Entry `t` (0..255) is the palette colour at index `t+1`,
   with its stored alpha. Indices beyond the palette give opaque (160, 210, 255).
7. **XYZI.** An int32 count followed by the records.
   - Loop `gx` from `min.x` to `max.x-1` (outer), then `gy` from `min.y` to `max.y-1`, then `k` from
     `min.z` to `max.z-1` (inner).
   - The value read is the voxel at `(gx, gy, G-1-k)`.
   - The mirror is taken around the **world** centre. It matches the region only when
     `min.z + max.z = G`, which holds for the default centred box.
   - For a non-zero value, append the record `(gx-min.x, k-min.z, gy-min.y, palette index of the
     value)`. Each field is a byte, so values above 255 wrap. A colour missing from the palette gets
     index 0, which is empty in MagicaVoxel.
8. **Finish.** The log line is `Magicavoxel VOX Export: done`. The file is written with the binary
   writer.

**[FIX-MODERN]** Modern profiles mirror around the region, refuse sizes above 256, and remap the
palette to at most 255 entries.

---

## 20. Qubicle binary (`.qb`)

### 20.1 Import

Messages use the name `Qubicle Binary QB`. The whole file is read, and all integers are little-endian.

1. **Header.** Six uint32 values: version, colour format, z-axis orientation, compression, visibility
   mask flag, matrix count. There is no magic and no version check.
2. **Model flags.** "Use RGB color" is set to (colour format = 0). "Right handed axis" is set to
   (z orientation = 1).
3. **Matrices.** Each matrix is read as follows.
   - A uint8 name length, then the name, decoded as UTF-8 with lossy decoding.
   - int32 values `sizeX, sizeY, sizeZ, posX, posY, posZ`.
   - **Origin.** Matrix 0 defines the origin vector `o = (-posX, -posY, -posZ)`, using the raw
     positions. When the orientation is 1, `o.z = sizeZ + posZ`.
   - **Stored z.** A slice `z` is stored at index `z' = z` when the orientation is 0, and at
     `sizeZ-1-z` otherwise.
   - **Uncompressed data** (compression = 0). For each `z`, then `y`, then `x` (x fastest), read one
     uint32.
   - **RLE data** (compression != 0). For each `z` slice, read uint32 codes until code 6.
     - Code 2 is followed by a count and a value, and repeats the value count times.
     - Any other code is a literal value.
     - Cells fill in order, with `x = idx mod sizeX` and `y = idx div sizeX`.
4. **Normalize.** Subtract the componentwise minimum position over all matrices from every matrix.
5. **Parse errors.** A failure in steps 1-4 shows the error box `Qubicle Binary QB Import`, and then
   **continues** with whatever was parsed.
6. **World size.** The bounding size is `W = max over matrices of (pos + size)`. Let
   `N = min(128, max(W))`. When replace is set, re-create the world.
7. **Offsets.** The offsets are `x0 = (G - W.x) div 2`, `y0 = min(N, G - W.y)` and
   `z0 = (G - W.z) div 2`, where `G` is the live grid extent and `div` truncates.
8. **Origin and box.**
   - The model origin is moved to `GridToWorld(trunc(o + (x0, y0, z0)))`.
   - The model box is set to `GridToWorld(x0, y0, z0)` .. `GridToWorld(x0+W.x, y0+W.y, z0+W.z)`.
9. **Cells.** Matrices are written in file order, x outer, then y, then z. A later matrix overwrites an
   earlier one.
   - A raw value of 0 is skipped.
   - **RGBA format** (colour format 0). The bytes R, G, B, A are packed to ARGB. A zero alpha gives 0,
     which is still written and so clears the cell.
   - **BGRA format** (other values). The raw value is used as is.
   - **Target.** The cell is written to `(x + posX + x0, y + posY + y0, z' + posZ + z0)` with the
     bounded write.
10. **Finish.** Restore batching, set the model name to the stem, and set the grid steps for `N`.

The reference keeps these asymmetries. The data flip tests orientation != 0, while the origin and the
flag test orientation = 1. Alpha-0 raw BGRA values are stored as non-empty voxels. Positions in a
multi-matrix file shift the origin when matrix 0 is not at the minimum.

### 20.2 Export

1. **Prompt and status.** Run the voxel-only prompt with the name `Qubicle Binary`. The status is
   `Exporting Qubicle Binary file...`.
2. **Header.** Six uint32 values:
   - 257;
   - colour format: 0 if "Use RGB color" is on, else 1;
   - z orientation: 1 if "Right handed axis" is on, else 0;
   - 0;
   - 0;
   - 1.
3. **Matrix.**
   - A uint8 name length, which is the model name's length in UTF-16 units truncated to a byte.
   - The name bytes in the system ANSI code page.
   - int32 `dx, dy, dz` (the region size), then int32 `posX, posY, posZ`.
4. **Positions.** Let `og` be the model origin converted to grid indices.
   - `posX = min.x - og.x` and `posY = min.y - og.y`.
   - `posZ = min.z - og.z` without right-handed axis.
   - `posZ = (og.z - min.z) - dz` with right-handed axis.
5. **Data.** The data is uncompressed. Loop `j` over 0..dz-1 (outer), then `k` over 0..dy-1, then `l`
   over 0..dx-1 (x fastest).
   - The source z is `dz-1-j` with right-handed axis, and `j` otherwise.
   - The value `v` is the voxel at `min + (l, k, source z)`.
   - Write one int32 per cell. With RGB on it is `(A<<24)|(B<<16)|(G<<8)|R`, which gives the bytes R,
     G, B, A. With RGB off it is `v`, giving B, G, R, A. An empty cell is 0.
6. **Progress.** Every 262144 cells the status shows `Exporting Qubicle Binary file...`, and at the end
   `Exporting Qubicle Binary finish...` with progress 1.
7. **Size.** The file is `24 + 1 + name length + 24 + 4·dx·dy·dz` bytes, written with the binary
   writer.

**[FIX-ALWAYS]** The length byte must equal the number of name bytes written. The port writes the
name in Windows-1252, replacing unencodable characters with `?`. The length is the actual byte count,
capped at 255. Output for single-byte names is unchanged. The importer decodes names as UTF-8, so
non-ASCII names do not round-trip in the reference. `modern` writes and reads UTF-8.

---

## 21. Qubicle exchange (`.qef`)

### 21.1 Import

The model name is set to the stem. Messages use the name `Qubicle QEF`. The text is read
BOM-aware, line by line, with CR, LF or CRLF line endings.

1. **Line 1.** It must start with `Qubicle`, else fail with `Invalid Qubicle QEF format`. Lines 2 and 3
   are ignored.
2. **Line 4.** Split it on single spaces into integers; a failed parse gives 0. With fewer than three
   values, fail with `Can't read model size`.
   - Read `(sx, sy, sz)` and let `N = max`, with no cap. When replace is set, re-create the world.
   - The offsets are `x0 = (G-sx) div 2`, `y0 = min(N, G-sy)` and `z0 = (G-sz) div 2`.
   - Set the model box from `GridToWorld(x0, y0, z0)` to `GridToWorld(x0+sx, y0+sy, z0+sz)`.
3. **Line 5.** The colour count `c`. A failed parse gives 0.
4. **Lines 6 to 5+c.** Split each line on single spaces into floats; a failed parse gives 0. A line
   with at least three values force-adds the colour `(r, g, b)` with alpha 1. The channels are in
   [0,1] and are truncated to bytes.
5. **Remaining lines.** Split each line into integers. A line with at least four values writes grid
   `(x+x0, y+y0, z+z0)` with the colour at palette index `i + 1`, using the bounded write. The fifth
   value, the visibility mask, is ignored. Malformed lines are skipped.
6. **Finish.** Restore batching and set the grid steps for `N`. There is no success dialog beyond the
   pipeline status.

### 21.2 Export

1. **Prompt and status.** Run the voxel-only prompt with the name `Qubicle Exchange`. The status is
   `Exporting Qubicle Exchange file...`.
2. **Text.**
   ```
   Qubicle Exchange Format                 (LF)
   Version 0.2                             (LF)
   www.minddesk.com                        (LF)
   <dx> <dy> <dz>                          (LF)
   <palette count>                         (NL; the count includes the reserved entry 0)
   <r> <g> <b>                             (LF, one line per palette index from 0; G7 of channel/255; index 0 is "0 0 0")
   <x> <y> <z> <index> <mask>              (LF, one line per non-empty voxel)
   ```
3. **Voxel order.** x outer (0..dx-1), then y, then z inner. The coordinates are relative to `min`.
   `<index>` is the palette index of the voxel colour, or 0 when absent.
4. **Mask bits.** A bit is set when the neighbour on that side is empty: -X 2, +X 4, +Y 8, -Y 16,
   -Z 32, +Z 64. Bit 1 is never set.
5. **Neighbour test.** **Reference defect, kept in `cubik29`:** neighbours are probed at the
   *local* coordinates `(x, y, z) ± 1`, interpreted as grid coordinates, against the write region. A
   neighbour below the region minimum counts as empty. With the default centred box this marks every
   face visible, so the mask is 126 for typical models (**UNVERIFIED** for boxes larger than `N`).
   **[FIX-MODERN]** The modern profile probes `min + (x, y, z) ± 1`.
6. **Progress.** It is reported every 32768 visited cells, but only when the cell at that step is
   written.
7. **Finish.** The log line is `Qubicle Exchange Export: done`. The file is written with the text
   writer.

A round trip through the reference adds one black palette entry, because entry 0 is written and read
back as a colour.

---

## 22. binvox (`.binvox`)

Export only.

1. **Prompt and status.** Run the voxel-only prompt with the name `Binvox`. The status is
   `Exporting BINVOX file...`.
2. **Grid.** `S = max(dx, dy, dz)` over the region. Visit `i` (x) from 0 to S-1 (outer), then `j` (z)
   from 0 to S-1, then `k` (y) from 0 to S-1 (inner). The bit is `voxel(min.x+i, min.y+k, min.z+j) != 0`.
   Cells outside the region read 0.
3. **Runs.** Run-length pairs `(value, count)` are bytes, with value 0 or 1 and count 1 to 255. A new
   pair starts when the value changes or the count would exceed 255.
4. **File.** The ASCII lines `#binvox 1`, `dim S S S` and `data`, each followed by LF, then the pairs.
   There are no `translate` or `scale` lines, so readers that require them reject the file.
5. **Progress.** Every 262144 visited cells the status shows `Exporting Binvox file...`. The fraction
   is visited / (dx·dy·dz), which can exceed 1.
6. **Finish.** The log line is `Binvox Export: done`. The file is written with the binary writer.

---

## 23. MCEdit schematic (`.schematic`) and the colour-to-block map

### 23.1 Import

Messages use the name `MCEdit SCHEMATIC`.

1. **Read.** The file is gzip-compressed big-endian NBT. An uncompressed file fails.
2. **Root.** The root compound must be named `Schematic`, else fail with
   `Invalid MCEdit SCHEMATIC format`.
3. **Tags.** Read the short tags `Width` (W), `Height` (H), `Length` (L) and the byte-array tag
   `Blocks`. A missing tag fails. `Data`, `AddBlocks`, `Materials`, `Entities` and `TileEntities` are
   ignored.
4. **World.** Let `N = min(128, max(W, H, L))`. When replace is set, re-create the world.
   - The offsets are `x0 = (G-W) div 2`, `y0 = min(N, G-H)` and `z0 = (G-L) div 2`.
   - Set the model box to the footprint.
   - Append the Minecraft palette (2.5). It is not reset when replace is off.
5. **Cells.** Loop y (0..H-1, outer), then z (0..L-1), then x (0..W-1, inner). The array index is
   `(y·L + z)·W + x`.
   - A non-zero id writes grid `(x+x0, y+y0, z+z0)` with the colour at palette index `id`.
   - Distinct ids are recorded in first-seen order.
   - A `Blocks` array that is too short fails after the cells before it are written.
6. **Model.**
   - The name becomes the stem.
   - The type becomes **Map**, so a later JSON export is refused.
   - The map text becomes `empty = air` LF, `default = stone` LF, then one line per distinct id:
     `<hex of the palette colour at id> = <legacy name of id without "minecraft:", or "unknown">` LF.

Batching and the grid steps are not changed.

### 23.2 Export

1. **Prompt and status.** Run the voxel-only prompt with the name `MCEdit Schematic`. The status is
   `Exporting MCEdit Schematic file...`.
2. **Map parsing.** The map text is normalized in these steps.
   1. Trim it.
   2. Remove the rich-text decoration that the dialog adds: `<color=...>...</color> <i>X</i>` becomes
      `X`.
   3. Replace `empty` with `#00000000`.
   4. Replace each `=`, `|` and `/` with a space.
   5. Replace every line break with `%`. The forms LF CR, CR LF, LF and CR are handled in that order.
      Then append `%`.
   6. Rewrite every match of `([0-9a-zA-Z#]+)\s*?([a-z0-9_]+).*?%` as an entry `{c: group 1, b: group
      2}`.
   7. Wrap the entries in an object, remove the last character of the joined text (the last `,`),
      remove any remaining `%`, and parse the result as relaxed JSON.

   A parse failure shows an error box titled `Error` with the text `Map config: <message>`. The export
   then has no data, so the write fails and the pipeline shows `Error while saving`, followed by the
   final `Exported SCHEMATIC` status (6.2).
3. **Parser quirks.** Kept in `cubik29`.
   - Block names must match `[a-z0-9_]+`. An uppercase name produces a junk entry that never matches.
   - A namespaced value `minecraft:stone` keeps only `minecraft`, which maps to id 0.
   - Stray text on a line can break the JSON and therefore the export.
4. **Lookup.** Each voxel's colour is the hex text of its packed value, so an empty voxel gives
   `#00000000`. Entries are visited in order.
   - An entry matches when `c` equals the colour, or `c` equals the first 7 characters `#RRGGBB` of
     the colour. Both tests ignore case and run per entry, so an earlier prefix entry beats a later
     exact entry.
   - Without any match, the value of the first `default` entry applies, compared without case.
   - Otherwise the name is `unknown`.
5. **Block id.** Lowercase the name. Look it up in the legacy table as given, then as
   `minecraft:<name>`. Without a hit the id is 0 (air).
6. **NBT.** The root compound `Schematic` holds, in order:
   - short `Width = dx`;
   - short `Height = dy`;
   - short `Length = dz`;
   - byte array `Blocks`, filled y outer, z, x inner with the voxel at `min + (x, y, z)`.

   No other tags are written. The file is gzip-compressed.
7. **Progress.** Every 262144 cells the status shows `Exporting MCEdit Schematic file...`. At the end
   it shows `Exporting MCEdit Schematic finish...` with progress 1.

### 23.3 Map text tools (Model Properties > Map)

- **Auto list.** It requires an initialized voxel world.
  - It scans the region y, then z, then x for distinct non-zero colours.
  - More than 256 colours shows an error box titled `Too many colors` with the text `Your map contains
    <n> colors...` + LF + `Map can't have more than 256 colors!` + LF + LF + `Please use less different
    colors.`.
  - Otherwise the text becomes `empty = air` LF, `default = stone` LF, then one line per colour:
    `<hex> = color_<palette index>` LF.
- **Default list.** It restores the default map text (2.7).
- **Clear list.** It empties the text.
- **Display.** The dialog decorates hex codes and the words `empty` and `default` with rich text for
  display. The decoration is removed when the dialog is applied.

### 23.4 Legacy id tables

The data of all three tables is in the appendix (34.2, 34.3). Their lookup rules are:

- **Name table.** It maps the Minecraft 1.12 block registry names, each with the `minecraft:` prefix,
  to their numeric ids 0 to 255. There are 254 names: ids 253 and 254 are absent, and 255 is
  `minecraft:structure_block`.
  - **Name to id** (schematic export, 23.2): lowercase the input, try it as given, then with the
    `minecraft:` prefix. An unknown name gives 0 (air).
  - **Id to name** (the map text of 23.1): the name without its prefix by default. An unknown id gives
    `unknown`.
  - The structure importer does not use these helpers. It matches the `Name` tag exactly, with no case
    folding and no prefix retry (24).
- **Colour and variant property tables** (structure import only). The colour table maps white and
  black to shulker-box ids and the other colours to glazed-terracotta ids. The variant table maps the
  wood names to stair ids. These ids are used only as palette slots for colour approximation.

---

## 24. Minecraft structure (`.nbt`)

Import only. Messages use the name `MC Structure NBT`.

1. **Model.** The name becomes the stem and the type becomes **Map**. Both are set before parsing.
2. **Read.** The file is gzip big-endian NBT. The tags `author` (string), `size` (list of 3 ints),
   `palette` (list) and `blocks` (list) are all **required**. A missing tag fails with the error box.
3. **Description.** It becomes `Minecraft structure <stem> by <author>`.
4. **World.** Let `N = min(128, max(size))`. When replace is set, re-create the world.
   - The offsets are `x0 = (G-sx) div 2`, `y0 = min(N, G-sy)` and `z0 = (G-sz) div 2`.
   - **The model box is not changed**, so writes outside the current box are dropped.
   - Append the Minecraft palette.
5. **Palette ids.** For each palette entry:
   - The id comes from looking up `Name` in the legacy table (exact, with namespace). An unknown name
     gives 0.
   - The `Properties` string tags are visited in order. A `color` value found in the colour table, or
     a `variant` value found in the variant table, replaces the id, whatever `Name` said, so the last
     matching property wins. For example a planks entry with `variant` = `oak` gets the oak stairs id
     53 (34.3). The rule is read from the code; its effect on real structure files was not checked
     (T-NBT-1).
   - A non-string property ends the scan silently.
6. **Blocks.** Each entry's `state` index gives an id (0 when out of range). The block writes grid
   `pos + (x0, y0, z0)` with the colour at palette index `id`, using the bounded write. Air entries
   therefore clear cells.
7. **Finish.** Batching and the grid steps are not changed.

Modern structure files have no `author` tag, and they may carry `palettes` (plural) instead of
`palette`. The reference rejects them. Section 31 covers modern support.

---

## 25. Zoxel (`.zox`)

### 25.1 Import

The model name is set to the stem. Messages use the name `Zoxel ZOX`. The text is read BOM-aware. In
the reference a read failure is not caught. **[FIX-ALWAYS]** The port reports a read failure as an
import error.

1. **Parse.** Deserialize the keys `creator`, `width` (X), `height` (Y), `depth` (Z), `version`
   (default 1), `frames` (default 1) and `frame1`, an array of integer arrays.
2. **Checks.** `frames <= 0` fails with `No frame found`. A missing or empty `frame1` fails with
   `Frame 1 is empty`. Only `frame1` is read.
3. **World.** Let `N = max(width, height, depth)`, with no cap. When replace is set, re-create the
   world.
   - The offsets are `x0 = (G-W) div 2`, `y0 = min(N, G-H)` and `z0 = (G-D) div 2`.
   - Set the model box to the footprint.
4. **Entries.** Each entry with at least four numbers `[x, y, z, c]`:
   - The colour `c` is read as RGBA: `a = c & 255`, `r = (c >> 24) & 255`, `g = (c >> 16) & 255` and
     `b = (c >> 8) & 255`. These are packed to ARGB, and 0 when `a = 0`.
   - The entry writes grid `(x+x0, y+y0, z+z0)` with the bounded write. A value of 0 clears the cell.
5. **Finish.** Restore batching and set the grid steps.

### 25.2 Export

1. **Prompt and status.** Run the voxel-only prompt with the name `Zoxel ZOX`. The status is
   `Exporting Zoxel ZOX file...`.
2. **JSON.** Compact JSON from the serializer, with these keys in this order:
   - `creator`: the app identity;
   - `height`: dy;
   - `width`: dx;
   - `depth`: dz;
   - `version`: 1;
   - `frames`: 1;
   - `frame1`: the voxel list.

   Example: `{"creator":"<identity>","height":16,"width":16,"depth":16,"version":1,"frames":1,"frame1":[[0,0,0,-16777089]]}`.
3. **Voxel list.** Loop x outer, y, z inner over the region. Each non-empty voxel adds
   `[x, y, z, (r<<24)|(g<<16)|(b<<8)|a]`, with region-relative coordinates and the colour as a signed
   32-bit integer.
4. **Progress.** It is reported only when a non-empty voxel falls on a multiple of 262144 visited
   cells.
5. **Finish.** The log line is `Zoxel ZOX Export: done`. The file is written with the text writer.

---

## 26. Sproxel CSV (`.csv`)

Export only.

1. **Elements prompt.** When elements exist, show an information box titled `Sproxel CSV Export`.
   - Text: `Exporting to Sproxel CSV format only export voxels. Elements are not exported.` + LF + LF +
     `To export the elements, convert them first to voxels with Voxel Manager.`.
   - Buttons: `Continue` and `Cancel`.
   - Unlike 6.3, **Cancel aborts**. It sets the status `Cancelled CSV export...` and writes no file.
2. **Status.** `Exporting CSV file...`.
3. **Text.**
   - The first line is `<dx>,<dy>,<dz>` LF.
   - Then the layers, from y = dy-1 down to 0. Each layer holds rows z = 0..dz-1, separated by LF. Each
     row holds cells x = 0..dx-1, separated by `,`.
   - Each cell is the hex text `#RRGGBBAA` of the voxel at `min + (x, y, z)`, with its stored alpha.
     An empty cell is `#00000000`.
   - Layers are separated by LF LF. There is no final newline.
4. **Progress.** Every 32768 cells the status shows `Exporting Sproxel CSV file...` with the fraction.
5. **Finish.** Set the grid steps for `N` as a side effect. The log line is `Sproxel CSV Export: done`.
   The file is written with the text writer.

---

## 27. Stonehearth (`.stonehearth`) and Staxel (`.staxel`)

### 27.1 Stonehearth

The chosen path only supplies a folder and a stem. The `.stonehearth` file itself is never written.
The exporter has no voxel-only prompt and no dialogs.

1. **Qubicle part.** Run the Qubicle binary builder (20.2) for `<folder>/<stem>.qb`, with RGB on and
   right-handed axis on, whatever the model settings say. Its progress statuses apply.
2. **Entity document.** Build this document:
   ```
   {
     "type": "entity",
     "components": {
       "model_variants": { "default": { "models": [ "file(<stem>.qb)" ] } },
       "unit_info": { "display_name": "<model name>", "description": "<description>" },
       "stonehearth:entity_forms": { "placeable_on_ground": <pog>, "placeable_on_walls": <pow> },
       "region_collision_shape": { "region": [ { "min": { "x": 0.0, "y": 0.0, "z": 0.0 }, "max": { "x": 1.0, "y": 1.0, "z": 1.0 } } ] },
       "render_info": { "scale": 1.0 },
       "mob": { "model_origin": { "x": 0.0, "y": 0.0, "z": 0.0 }, "region_origin": { "x": 0.0, "y": 0.0, "z": 0.0 } }
     },
     "entity_data": {
       "stonehearth:net_worth": {
         "value_in_gold": <v>,
         "rarity": "<r>",
         "shop_info": { "buyable": <b>, "sellable": <s>, "shopkeeper_level": 1, "shopkeeper_type": "caravan" }
       }
     }
   }
   ```
   - The keys appear exactly in this order. The block above is shown compacted for reading. The real
     output follows the serializer's layout (3.3).
   - Null members are omitted: `rarity` when null, and `display_name` or `description` when null.
   - `.qb` is appended to the stem in `file(...)` only when it does not already end with `.qb`.
   - `mixins` is never written.
3. **Serialize.** Use the JSON serializer: indented (two spaces, NL), or compact when "Compress data" is
   on. Floats use NJ form, so `0.0` and `1.0`.
4. **Log.** `Elapsed time to serialize: <s>s`.
5. **Write.** Write `<folder>/<stem>.qb` with the binary writer, then `<folder>/<stem>.json` with the
   text writer.
6. **Status.** The pipeline then shows `Exported STONEHEARTH: <chosen path>`.

The model settings come from the Stonehearth tab of Model Properties: "Placeable on ground",
"Placeable on walls", "Base value of object in game (in gold):", "Rarity (in a shop keeper
inventory):", "Buyable" and "Sellable". The tab's info text reads `Stonehearth export is hugely in
development.` + LF + LF + `Exporting for Stonehearth generates a basic config file (json) along the Qb
file (model). Edit json to make the whole work in game.`.

### 27.2 Staxel

The Staxel exporter is a stub. It writes nothing, yet the pipeline still reports
`Exported STAXEL: <path>`. Its Model Properties tab offers a type (Tile, Item, Accessory, Other;
default Accessory). It shows the info text that starts `Staxel export is hugely in development.` and
links `http://forums.playstaxel.com/forums/modding.28`.

In `cubik29` the port MUST keep the registry entry and the visible behaviour. **[FIX-MODERN]** Modern
profiles hide the entry until a real exporter exists.

---

## 28. Windows shortcut (`.lnk`)

The entry is registered only while the built-in file browser is in use. Opening a `.lnk` does not
import a model. It navigates the built-in browser.

1. **Target path.** The target is resolved from a subset of the MS-SHLLINK layout.
   1. The extension must be `.lnk`, compared without case.
   2. Read the LinkFlags (uint32) at offset 0x14.
   3. If bit 0 is set, read a uint16 size at offset 0x4C and skip that many bytes.
   4. At the current position `L`, read LinkInfoSize (uint32), skip 12 bytes, and read
      LocalBasePathOffset (uint32).
   5. Seek to `L + LocalBasePathOffset` and read `(L + LinkInfoSize - position - 2)` **characters**,
      decoded as UTF-8.
   6. If the text contains a double NUL, keep the part before it and append the part after the next
      `\\` and its following NUL.
   7. Remove every invalid path character and `#`.
   8. Any error gives no path.
2. **Navigation** *(spot-checked)*.
   - **Existing directory.** On the next frame, open the browser at that directory.
   - **Existing file.** Open the browser at the file's parent directory.
   - **Otherwise.** On the next frame, show the error box `Path not found` with the text `Sorry but
     shortcut can't be followed:` and the quoted path, which may be empty. The buttons are `Continue`,
     which reopens the default folder, and `Cancel`.
3. **Pipeline.** It still reports `Imported LNK: <path>` and sets the model's last import/export path
   to the `.lnk` path (5.2 step 5).

Not handled: network targets, Unicode local paths, ID-list-only links, and non-ASCII paths, which are
mis-decoded. **[IMPORT-MODE]** `modern` uses a full shell-link parser.

---

## 29. Sketchfab upload

The menu path is Tools > Share on Sketchfab.

1. **Token check.** Without a token, show the box `Missing Sketchfab token` (Sketchfab icon) with the
   text `To share your model on Sketchfab, you must add your Sketchfab API token in Cubik Preferences >
   Formats > Sketchfab field...` + LF + LF + `Then retry.`.
2. **Wait box.** Show a wait box titled `Exporting to Sketchfab` with the text `Your model is compiled
   for Sketchfab.` + LF + LF + `Please wait...`, then wait one physics frame.
3. **Empty model.** With no active renderers under the model root, the failure callback receives
   `Model not exported...` + LF + LF + `Please, make sure at least one element, one voxel, one mesh or
   one primitive exists before exporting.`.
4. **Package.**
   1. Empty or create `<temp>/SketchfabExport/`.
   2. Run the OBJ exporter silently (16.6) to `<temp>/SketchfabExport/<model name without
      whitespace>.obj`, then write its files.
   3. Zip every file of the folder, at the archive root, into `<temp>/SketchfabExport/CubikModel.zip`.
   - Write failures show `Error while saving` with the text `An error occured while saving mesh to
     disk.` + LF + LF + `<message>`.
   - Packaging failures show `Error while packaging` with the text `An error occured while preparing
     archive to upload.` + LF + LF + `<message>`.
5. **Upload.** POST a multipart form to the Sketchfab Data API **v1** (`https://api.sketchfab.com/v1/models`)
   with these fields:
   - `fileModel`: the zip bytes, typed `application/zip`;
   - `token`;
   - `title`: the model name;
   - `description`: the description + LF + LF + `Made with <app identity>`;
   - `tags`: `cubik lowpoly voxel`;
   - `filenameModel`: `CubikModel.zip`;
   - `source`: `cubik-studio`;
   - `isPublished`: `1`.

   The status shows `Uploading Sketchfab export (<sent>/<total>)...`.
6. **Response** *(spot-checked)*.
   - A JSON response with `success` = true and `result.id` leads to the URL
     `https://sketchfab.com/show/<id>`, and the box `Model on Sketchfab` opens with the text `Your
     model is ready on your Sketchfab account!` + LF + LF + `Do you want to see it online now?`. Its
     buttons are `Show me`, which opens the URL, and `Later`.
   - Anything else opens the error box `Ohoh...` with the text `Unexpected Sketchfab answer: <body>`.
     Its buttons are `OK`, `Go to Sketchfab` (opens `https://sketchfab.com/dashboard`) and `Cancel`.

The v1 API is believed to be retired, so the reference flow cannot succeed today (**UNVERIFIED**, general knowledge). Section 31 specifies the
replacement.

## 30. Parity test plan

### 30.1 Fixtures and comparison rules

Reference fixtures come from Cubik 2.9.480 on Windows. Every fixture records the model, the settings
of 2.7 and 2.8, and the produced files.

| Output kind | Comparison rule |
|---|---|
| Text formats | Byte-exact after one normalization: the app identity string and the signature-derived description are replaced by placeholders. The port is configured with the reference identity in parity runs. |
| Binary formats | Byte-exact. |
| gzip and zip containers | Decompressed bytes and entry lists (3.6). |
| PNG | Decoded RGBA pixels. |
| Packed atlases (12.2, 14, 16.7) | Per-element, per-face or per-quad sampled pixels. In OBJ output, which takes element UVs from the runtime element atlas and voxel UVs from 16.7, the `vt` lines and the texture indices of `f` lines are compared this way instead of byte for byte. |
| Renders (18.1) | Settings only. Pixels are not compared. |

### 30.2 Test identifiers

The tests below are cited elsewhere in this document.

| Id | What it checks |
|---|---|
| T-NUM-1 | Float text forms. A rotated element whose unrotated corner is not a short decimal (X9), display values such as 0.1 and 1E-06 (G7), and `.js` floats (NJ). |
| T-JSON-1 | Minecraft JSON export of a model that exercises every header option, the texture rules with the PNG setting on and off, invert, rescale, shade, every face option, bleed inset, display (normal and forced), overrides, skipped elements and minify. |
| T-JSON-2 | Minecraft JSON import of vanilla 1.12 models: every parent prompt, `item/generated`, `textures` absent, namespaced textures, the cullface `bottom`, and a rotation with "Restrict rotations" on and off. It also rotates a primitive mesh with the option on, to settle the prefab flag of 2.2.1. |
| T-RP-1 | Resource pack entry list and texts. It includes textures with and without a source file, a duplicate label, and an Item model. |
| T-MCPE-1 | `.mcpe` import of a `mobs.json`: inheritance, wildcard lookup, mirror UVs, the UV mask, and the 22.5° snap. |
| T-JS-1, T-JAVA-1, T-JPM-1 | Bedrock, Java and OptiFine exports in both bone modes, with dedup, offsets and minimum size. T-JAVA-1 also needs an in-game check of the Y placement (13.3). |
| T-OBJ-1 | OBJ import corner cases of 15.2 and the scale rule of 15.4. |
| T-OBJ-2 | OBJ export with hidden elements, meshes and voxel chunks present (expected in the output), and with every object hidden (expected "Empty model"). |
| T-OBJ-3 | OBJ and MTL group, material and texture names for elements, primitives of one and of several kinds, imported meshes with one and several materials, voxels in detailed mode and voxels in atlas mode, at N = 16 and N = 32. It also checks the `<base>_.png` collapse and the material-name dedupe of 16.5. |
| T-OBJ-4 | Texture-mode voxel export: `gmt` and `omt` on a multi-colour model, padding on and off, smoothing at N = 16 and N = 32, a model above 16200 quads, an imported mesh without UVs before the voxels (16.3), and the single-precision residue in `v` and `vn` lines. |
| T-STL-1 | STL export of a voxel model spanning several chunks. |
| T-VOX-1, T-VOX-2 | VOX round trip with the default box, and with an asymmetric box. |
| T-QB-1 | QB round trip in RGBA/BGRA, with and without right-handed axis, multi-matrix input, and RLE input. |
| T-QEF-1 | QEF export masks with the default box and with a 3N box. |
| T-BINVOX-1 | binvox output. |
| T-SCHEM-1 | Schematic import, then export with the generated map text. Map text edge cases: uppercase names, namespaced names, `#RRGGBB` prefix entries, a missing `default`. |
| T-NBT-1 | Structure import of a 1.12 file. It also checks that a modern file is rejected. |
| T-ZOX-1, T-CSV-1, T-SH-1 | Zoxel, CSV and Stonehearth text output, including the CRLF of indented JSON. |
| T-PNG-1 | Render settings, and sprite import rectangle counts for patterned sprites. |
| T-LNK-1 | Shortcut targets: a local file, a folder, and a missing target. |
| T-UI-1 | Every dialog title, text, button label and status string in this document. |

---

## 31. Modernization

### 31.1 Principles

1. **Default profile.** `cubik29` stays the default for every exporter. A modern profile is opt-in per
   export, and the export dialog remembers the last choice per format.
2. **Version selection.** Modern profiles take an explicit target version. They write the smallest
   format that the used features need, and never a higher one. When a feature needs a newer target
   than the one selected, the port warns and either downgrades the feature or refuses the export.
3. **Importers.** Importers detect the input's dialect from its content, never from the extension
   alone, and accept both legacy and modern forms.
4. **Unknown keys.** JSON importers keep unknown keys in an extension map. Modern exporters write
   them back on round trip.
5. **Determinism.** Output is deterministic: ordered maps, explicit float formatting, a seeded RNG for
   jitter, and fixed archive timestamps and entry order.
6. **Snapshots.** Exporters run on an immutable model snapshot, off the UI thread, and report
   progress through a channel. The UI thread never blocks on file I/O.

### 31.2 Java Edition block and item models

The modern JSON writer is serializer-based. It uses tab or 2-space indentation (user option, default
tab, matching the reference), LF, and shortest round-trip floats limited to 6 decimals.

| Target | Changes from `cubik29` |
|---|---|
| `java-1.12` | Reference layout with the [FIX-MODERN] items fixed. Namespaces are written when they are not `minecraft`. |
| `java-1.13` (1.13 to 1.21.3) | Texture paths `block/` and `item/` (singular) instead of `blocks/` and `items/`. Namespaced ids are kept. `gui_light` (`front` or `side`) is written when set; the key exists from 1.15. |
| `java-1.21.4` (1.21.4 to 1.21.5) | Item models no longer carry `overrides`. Item overrides are exported as an **item definition** file `assets/<ns>/items/<id>.json` (31.3). |
| `java-1.21.6` (1.21.6 to 1.21.10) | Single-axis rotation with any angle in [-45, 45], according to Blockbench 5.2.2. That is not verified against the client jar (**UNVERIFIED**). |
| `java-1.21.11` (1.21.11 to 26.2) | Multi-axis element rotation `"rotation": {"x": a, "y": b, "z": c, "origin": [..]}`. The legacy single-axis form is still written whenever one axis is enough. `rescale` is allowed with both forms. |
| `java-26.3` | `shade_direction_override` replaces `shade` for per-element shading, according to Blockbench 5.2.2 (**UNVERIFIED** in vanilla). |

Optional element keys follow these rules.

- `light_emission` (0 to 15) is written only for targets that support it. The minimum version is
  **UNVERIFIED**.
- The texture object form `{"sprite": ..., "force_translucent": true}` follows the same rule, with an
  **UNVERIFIED** minimum version.
- The display slot `on_shelf` is offered for targets that have it. It is confirmed in the 26.3 game
  code, where display contexts are none, third person left and right hand, first person left and
  right hand, head, gui, ground, fixed and on_shelf.

**Import, `modern` mode.** The importer:

- keeps namespaces;
- reads `gui_light`, `texture_size` (UV scaling), `credit` and `render_type` into the extension map;
- reads `light_emission`;
- reads the multi-axis rotation object;
- maps the cullface alias `bottom` to down;
- decodes JSON escapes;
- optionally resolves parent chains when the resource-pack root is known. The resolution is limited to
  32 levels, merges `textures` and `display` per key, guards texture reference cycles, and stops at
  the builtin `item/generated` family.

### 31.3 Item definitions (1.21.4 and later)

A modern Item export with overrides writes `assets/<ns>/items/<id>.json`. Its `model` tree is built
from the override cases.

| Legacy predicate | Modern mapping |
|---|---|
| `custom_model_data` | `range_dispatch` on `custom_model_data` floats[0] |
| `damage` and `damaged` | `range_dispatch` and `condition` on damage |
| `pull` and `pulling` | Bow-style `range_dispatch` and `condition` |
| `angle` | Compass `range_dispatch` |
| `time` | Clock |
| `blocking`, `broken`, `cast`, `lefthanded` | `condition` or `select` cases |

The exact property names come from the client data and are **UNVERIFIED** here. The typed custom
model data component (floats, flags, strings, colours) is confirmed in the 26.3 game code. The legacy
overrides block stays available for targets below 1.21.4.

### 31.4 Resource pack

The modern pack writer applies these changes.

- **pack.mcmeta.** Resource-pack formats up to 64 use `pack_format`. Above 64, the game **requires**
  `min_format` and `max_format`. Each is a major integer or a `[major, minor]` pair, and
  `supported_formats` is rejected as deprecated. Packs that span both ranges must give `pack_format`,
  `supported_formats` and `min_format`/`max_format` consistently. All of this is confirmed in the 26.3
  game code, whose current resource-pack format is 97.1. The port ships a table that maps game
  version to format, maintained outside the code.
- **Flattened layout.** Blockstate variant key `""` instead of `normal`; texture folders `block/` and
  `item/`; item definitions for 1.21.4 and later.
- **Target choice.** The user chooses the replaced block and item; the default stays `glass` and
  `white_stained_glass`. Alternatively the user chooses a namespace and model id with no replacement.
- **Textures.** Unmodified source PNGs are copied byte for byte, and `.png.mcmeta` sidecars are carried
  along.
- **Archive.** Entry names have no leading slash, in a fixed order with fixed timestamps.

### 31.5 Bedrock

The modern export writes `<name>.geo.json`. It is pure JSON, with no comment header.

```
{
  "format_version": "<min version>",
  "minecraft:geometry": [ {
    "description": { "identifier": "geometry.<id>", "texture_width": W, "texture_height": H,
                     "visible_bounds_width": ..., "visible_bounds_height": ..., "visible_bounds_offset": [0, y, 0] },
    "bones": [ { "name", "parent", "pivot", "rotation", "mirror",
                 "cubes": [ { "origin", "size", "uv", "pivot", "rotation", "inflate", "mirror" } ] } ]
  } ]
}
```

**Version selection.** The rule follows Blockbench and is **UNVERIFIED** against Mojang's
documentation. It uses `1.21.110` when display transforms are written, `1.21.0` when a per-face UV has
a rotation, `1.16.0` when a bone binding is used, and `1.12.0` otherwise.

**Geometry rules.**

- Cubes carry their own pivot and rotation. This fixes the reference double rotation (11.2), so one
  bone per element is no longer needed.
- Signs relative to model space are taken from Blockbench. Bone pivot X is negated. Rotation X and Y
  are negated and Z is kept. A cube origin's X is `-(from.x + size.x)`. Cubik's mapping already
  mirrors X the same way.
- Box UV and per-face UV are both supported. Per-face UV uses `{ "north": {"uv": [u, v], "uv_size":
  [w, h]} ... }`, and up and down are flipped as Blockbench does.
- Visible bounds are computed per export. They do not accumulate.

**Import.** It accepts the legacy form (keys starting with `geometry.`, or `format_version` below
1.12.0) and the modern `minecraft:geometry` array. A multi-geometry file opens a picker list instead of
the wildcard prompt. In `modern` mode rotations are not snapped (10.4).

**Out of scope for the first modern release:** entity and attachable files, animations, block
geometry with material instances, and voxel shapes. The data model must still allow them later.

### 31.6 OptiFine CEM

The modern profile adds:

- JEM (whole entity model) **export and import**, and JPM **import**.
- **Parts.** Write `invertAxis: "xy"`. `translate` is the negated pivot, so the box rotates about the
  element pivot (this fixes 14.1). `rotate` is in degrees, `mirrorTexture` is supported, and
  `sizeAdd` holds the inflate.
- **UV forms.** Per-face `uvNorth..uvDown`, or box UV `textureOffset`.
- **Passthrough.** `attach`, `model`, `scale` and `animations` are kept on round trip.
- Mirrored UVs keep their direction instead of collapsing to 1 pixel.

### 31.7 Java entity code

The modern profile adds templates for:

- Forge and NeoForge 1.17+ with Mojang mappings: `LayerDefinition`, `CubeListBuilder`,
  `CubeDeformation` and `PartPose.offsetAndRotation`;
- Fabric 1.17+ with Yarn names: `ModelData`, `ModelPartBuilder` and `ModelTransform`.

The geometry follows the Blockbench conventions.

- Bone position X is negated. With flip-Y, Y is negated and root bones get +24.
- Rotations X and Y are negated, in radians with 4 decimals.
- Rotated boxes are placed in synthetic child parts `<name>_r1`.

The legacy `ModelBase` template stays in `cubik29`. A fixed variant `java-legacy-fixed` corrects the
Y placement (13.3), writes integer texture offsets, and validates identifiers. Invalid Java names,
such as an empty or digit-led class name, get the prefix `Model`.

### 31.8 Voxel formats

| Format | Modern additions |
|---|---|
| MagicaVoxel | Import the scene graph (`nTRN`, `nGRP`, `nSHP`) and merge models with their translations. Read `MATL` as metadata, and read `IMAP` palette remapping. On export, split models above 256 per axis and write a single-model scene graph. The chunk names come from the reader reports and the public format notes, and the chunk semantics are **UNVERIFIED** here. |
| Qubicle QB | Optional RLE compression on export, and the visibility-mask encoding flag. Export one matrix per voxel layer group when the port has layers. |
| Qubicle QEF | Correct visibility masks (21.2). Do not write the reserved palette entry, or write it and skip it on import. |
| binvox | Write `translate` and `scale` lines (`#binvox 1`, `dim`, `translate tx ty tz`, `scale s`, `data`). Readers that require them then accept the file. |
| MCEdit `.schematic` | Keep it for 1.12 and earlier. Add **Sponge Schematic** `.schem` (version 2 and 3) and **structure `.nbt` export** for 1.13 and later, with modern block states. The Sponge layout is general knowledge, **UNVERIFIED** here. |
| Structure `.nbt` import | Accept files without `author` and with `DataVersion`. Use the first entry of `palettes` when present. Map flattened block names to colours through a modern colour table. Using the game's block map colours is the proposed default, and the table is new data that the port must ship. Set the model box to the footprint, which fixes the dropped writes of 24. |
| Zoxel, CSV | Unchanged. Add Sproxel CSV import (`W,H,D`, then layers from top, `#RRGGBBAA` cells). |

### 31.9 Mesh and image formats

- **OBJ export, modern profile.**
  - LF only, and shortest round-trip floats in the invariant culture.
  - The weld option means what its label says.
  - Atlas-mode voxels are placed correctly for every N.
  - Optional unit scale: 1 block = 1 unit, or 16 units.
  - Material and texture names are sanitized. `map_Kd` paths are relative.
  - Normals come from transformed geometry.
  - Every scene part keeps its submeshes, with one `usemtl` per submesh, and every distinct texture
    gets its own file name (16.5).
  - UV and normal indices are resolved per part (16.3).
  - Texture-mode voxels share swatches across chunks, use a deterministic packer, and are smoothed in
    voxel units (16.2, 16.7).
  - "Exclude hidden elements" is honoured as an option, as in the entity exporters.
- **OBJ import.** Use a standards-compliant parser that handles whitespace, `mtllib`, `map_d`,
  concave polygons and negative indices, plus the 15.4 adoption rules.
- **STL.** ASCII or binary. Chunk transforms are applied, meshes and primitives are included, and the
  header is truncated. Optional Z-up remap and millimetre scale.
- **glTF 2.0 / GLB (new export).**
  - One node per element and mesh, or merged.
  - Embedded or external textures with a nearest-neighbour sampler. Textures are never resized to
    powers of two silently.
  - Optional per-bone hierarchy for entity models.
  - It is the recommended interchange format for DCC tools and the default Sketchfab payload.

### 31.10 Sketchfab

The v1 endpoint is replaced by the **Sketchfab Data API v3**: `POST https://api.sketchfab.com/v3/models`
with `Authorization: Token <token>` and a multipart `modelFile`, plus `name`, `description`, `tags`
and `isPublished`. The model is then polled for processing status. The endpoint details are general
knowledge, **UNVERIFIED** here.

- The payload is GLB by default, or the OBJ zip.
- The token stays in the user's settings and is never sent elsewhere.
- The reference dialog texts are kept for the visible flow.

### 31.11 Defects fixed in modern profiles

| Ref | Defect | Modern behaviour |
|---|---|---|
| 6.2 | `Exported` status after a failed write | Show a failure status and leave recent files unchanged. |
| 6.3 | Cancel in the voxel-only prompt does not abort | Cancel aborts. |
| 7.4 | Label and PNG name mismatch with the PNG setting on | One sanitized name for both. |
| 9.1 | Pack texture names differ from their model references | The entry name is derived from the Minecraft path. |
| 11.2 | Double rotation in `.js` | Use the unrotated corner and a cube pivot and rotation. |
| 13.3 | Java Y placement | Mirror Y. |
| 14.1 | JPM rotation pivot, collapsed mirrored UVs | 31.6 rules. |
| 16.2 | Atlas voxel placement for N other than 16 | Correct placement. |
| 16.2 | Voxel smoothing has no effect for N other than 16 and none under `omt` | Smooth atlas parts in voxel units. |
| 16.3 | UV indices shift after a part with fewer UVs than vertices | Per-part indices. |
| 16.4 | Weld polarity | As labelled. |
| 16.5 | Merged submeshes, name-keyed material and texture collisions | One `usemtl` per submesh, unique names. |
| 17 | STL chunk overlap | Apply chunk transforms. |
| 19.2 | VOX z mirror around the world centre | Mirror around the region. |
| 21.2 | QEF masks from local coordinates | Use grid coordinates. |
| 27.2 | Staxel stub reports success | Hide or implement. |

### 31.12 Implementation notes

- **Crates to evaluate.** These exist for the needed functionality and must be checked for licence,
  maintenance and safety before adoption:
  - `serde` and `serde_json` (with an order-preserving map);
  - `indexmap`;
  - `zip`;
  - `flate2`;
  - an NBT crate (`fastnbt` or `quartz_nbt`);
  - `dot_vox` for MagicaVoxel;
  - `tobj` for OBJ import;
  - `image` or `png`;
  - `gltf-json` for writing glTF.
- **Parity serializers.** The `cubik29` writers for Minecraft JSON, Java and OBJ are hand-written
  emitters. A generic serializer cannot reproduce their whitespace.
- **Float formatters.** G7, X9 and NJ (3.1) are small dedicated formatters with golden tests. The
  platform float formatting is never relied on.

---

## 32. Compatibility matrix

Legend: **R** = the reference reads it; **W** = the reference writes it; **P** = the port reproduces the
reference in `cubik29`; **M** = a modern profile or import mode adds or changes behaviour; **-** = not
applicable or not supported.

| Format | Reference | Port `cubik29` | Port modern | Target game or tool versions |
|---|---|---|---|---|
| Native `.cubik` | R W | P | (native specification) | Cubik 2.x |
| Legacy `.cub` | R | P | - | Cubik alpha |
| Java block/item `.json` | R W | P | M: namespaces, 1.13 paths, `gui_light`, multi-axis rotation, `shade_direction_override`, `light_emission`, parent resolution | Reference: 1.9 to 1.12. Modern: 1.13 to 26.x |
| Java item definition `items/*.json` | - | - | M: write | 1.21.4+ |
| Resource pack `.zip` | W | P | M: per-version `pack.mcmeta` (`pack_format`, or `min_format`/`max_format` above format 64), flattened layout | Reference: pack format 3 (1.11 to 1.12) |
| Bedrock legacy `.mcpe` | R | P | M: no snapping, picker | MCPE 1.0 to 1.7-era `mobs.json` |
| Bedrock legacy entity `.js` | W | P | - | MCPE legacy, manual paste into `mobs.json` |
| Bedrock modern `.geo.json` | - | - | M: read and write | `format_version` 1.12.0 to 1.21.110 |
| OptiFine `.jpm` | W | P | M: read, correct pivots | OptiFine CEM |
| OptiFine `.jem` | - | - | M: read and write | OptiFine CEM |
| Java entity `.java` | W | P | M: 1.17+ Mojang and Yarn templates, fixed legacy template | Reference: Forge 1.7 to 1.12 `ModelBase` |
| Wavefront `.obj`/`.mtl` | R W | P | M: standards mode, LF, weld fix, unique names, submeshes, voxel atlas fixes | Any DCC tool |
| STL `.stl` | W | P | M: ASCII, fixed chunks, meshes | Slicers, DCC tools |
| glTF `.gltf`/`.glb` | - | - | M: write | glTF 2.0 |
| PNG render `.png` | W | P (settings) | M: transparent background option | - |
| PNG sprite `.png` | R | P | - | - |
| MagicaVoxel `.vox` | R W | P | M: scene graph, more than 256 | MagicaVoxel 0.99+ |
| Qubicle `.qb` | R W | P | M: RLE export, UTF-8 names | Qubicle 1.x and 2.x, Stonehearth |
| Qubicle `.qef` | R W | P | M: correct masks | Qubicle |
| binvox `.binvox` | W | P | M: `translate`/`scale` | binvox tools |
| MCEdit `.schematic` | R W | P | M: kept for 1.12 | MCEdit and WorldEdit legacy (1.12 and earlier) |
| Sponge `.schem` | - | - | M: write (and read) | 1.13+ (**UNVERIFIED** layout) |
| Structure `.nbt` | R | P | M: modern files, export | Reference: 1.10 to 1.12. Modern: 1.13 to 26.x |
| Zoxel `.zox` | R W | P | - | Zoxel |
| Sproxel `.csv` | W | P | M: read | Sproxel |
| Stonehearth (`.qb` + `.json`) | W | P | - | Stonehearth alpha-era |
| Staxel | W (stub) | P (stub) | M: hidden | - |
| Windows `.lnk` | R | P | M: full parser | Windows |
| Sketchfab upload | W (API v1, dead) | P (dialog flow) | M: API v3 | Sketchfab |

---

## 33. Unverified items and open questions

### 33.1 Engine and runtime behaviour, not executed

1. **Float text.** The 7-significant-digit rounding of G7 and X9, and NJ's choice between 7 and 9
   digits, follow the runtime's documented number formatter. They have not been observed in a real
   export (T-NUM-1).
2. **Bounds test.** It is assumed inclusive at both ends (7.5).
3. **Cullface 63.** The enum name printed for cullface 63 may be `any` or `all` (7.6).
4. **Atlas packing.** The engine's packer layout, and its power-of-two sizing (12.2, 14, 16.7), cannot
   be reproduced. Parity is defined by sampled pixels. For the voxel atlas, padding placement,
   downscaling when the textures do not fit, and an empty texture list are also open (16.7.5,
   16.7.7).
5. **Face images.** The resize filter used for face images in the entity atlas (12.1) is
   nearest-neighbour by setting. Its exact sampling grid is unverified.
6. **Render output.** Background alpha, colour space and rounding when float colours are written to
   8-bit (18.1).
7. **Archive entry names.** The archive library strips leading `/` from entry names (9.1).
8. **Title case.** How it treats letters directly after digits (11.2 `Sanitize`).
9. **Texture image names.** Which internal name the texture image carries, which drives the sanitized
   PNG names of 7.4. The texture specification owns this.
10. **OBJ names.** The naming rules of 16.5.1 are read from the code. Still open are the asset names
    of the element material, the primitive prefab materials and the voxel chunk prefab material, the
    engine's instance suffix, and the empty names of the element atlas and of imported textures
    (T-OBJ-3). Hidden objects are exported according to the code; T-OBJ-2 confirms it at runtime.
11. **Single-precision residue.** The exact residue values in OBJ `v` and `vn` lines, which depend on
    the engine's quaternion arithmetic (16.3, T-OBJ-4).
12. **Submesh merge.** That the engine's triangle list of a multi-material mesh holds every submesh,
    as documented, so that the OBJ part copy has a single submesh (16.2, 16.5.1).
13. **Prefab restrict flag.** The per-object restrict flag stored in the element and primitive prefabs
    (2.2.1).
14. **Texel conversion.** The engine's float-to-byte conversion for non-solid voxel textures (16.7.6).

### 33.2 Reference behaviour that needs a runtime check

1. **QEF masks.** They are 126 for typical models, from code reading (21.2, T-QEF-1).
2. **STL chunks.** Voxel chunks overlap in STL output, from code reading (17, T-STL-1).
3. **Java Y placement.** Whether it is a reference bug or a convention (13.3, T-JAVA-1).
4. **PNG import dialog.** The binding of its discs to the options (18.2).
5. **JPM errors.** The user-visible effect when a face references a texture label missing from the
   atlas (14.1).
6. **Snapped rotations.** Snapping rotations to 5 degrees during JSON import when "Restrict
   rotations" is off is code-confirmed, but it is a surprising user-visible change (T-JSON-2).
7. **Voxel smoothing.** No effect for N other than 16, and none under `omt`, from code reading (16.2,
   T-OBJ-4).
8. **AO in detailed OBJ exports.** Detailed-mode vertex colours carry the viewport's occlusion factor
   when "Render fake ambient occlusion" is on (16.1, T-OBJ-3).
9. **Name collisions.** The `<base>_.png` collapse and the shared primitive material block (16.5.1,
   T-OBJ-3).

### 33.3 Data provenance and legal

Section 34 classifies every shipped table and palette. In summary:

1. **Regenerable data.** The legacy id table is factual Minecraft 1.12 data, and the two property
   tables are product mappings over it (34.2, 34.3). The VOX preset and the "HSL palette" picker
   follow generation rules given in 34.4.1 and 34.5, and those rules reproduce the shipped images
   exactly.
2. **Product-derived data.** The Minecraft preset colours and the values of the "RGB palette
   (default)" picker. Byte parity of schematic and structure imports needs the reference Minecraft
   colours. Whether to reuse them, redraw them or derive them from public data is an open decision,
   and this specification does not reproduce them.
3. **Third-party palettes.** The JMP, Generic16, Pico8, EDG32 and EDG16 pickers. The About box credits
   ENDESGA Studios for the EDG palettes and Arne Niklas Jansson for the Generic 16 palette. JMP and
   Pico8 are not credited. No licence was checked. The port ships each one only with a confirmed
   licence and credit, or replaces it.
4. **MagicaVoxel default.** That the VOX preset equals the published MagicaVoxel default palette was
   not checked against the published table (34.4.1).

### 33.4 Modern targets

These are taken from Blockbench 5.2.2 or general knowledge and still need confirmation against client
data or official documentation (31.2 to 31.10):

1. The 1.21.6 rotation range.
2. `shade_direction_override`.
3. The minimum version of `light_emission` and of the texture object form.
4. The property names of item definitions.
5. The Bedrock `format_version` tiers.
6. The Sponge schematic layout.
7. The Sketchfab v3 upload fields.
8. The game-version to pack-format table. Only 26.3, at 97.1, was confirmed.

### 33.5 Open design decisions

1. **QB names.** The encoding of non-ASCII Qubicle matrix names in `cubik29`. Proposed: Windows-1252
   with a corrected length byte.
2. **Message texts.** Whether the misspelled reference strings ("occured", "Successfuly", "don't have")
   stay in the port's UI. They are required for `cubik29` UI parity. A translation layer could fix
   them in other locales.
3. **Hidden prompt results.** Message boxes that replace each other without waiting (7.4). Should the
   port queue them in modern mode?
4. **Merge into an uninitialized world.** For voxel importers this writes nothing in the reference
   (5.3). It is reachable only through the Merge dialog with an import-type file, which the filter
   list does not offer.

---

## 34. Data appendix: legacy tables and palettes

This appendix holds the data tables that the reference ships and that the port needs. Each item says
whether it is public data that can be shipped or regenerated, a generation rule given here, or product
or third-party art that the port replaces or ships under a licence. Values derived from product or
third-party art are deliberately not reproduced (33.3).

Images are described with row 0 at the bottom, which is also the order in which the preset loader
reads them (2.5). Colours are given as `RRGGBB` hex.

### 34.1 Inventory

| Item | Size and format | Used by | Class | Port action |
|---|---|---|---|---|
| Legacy id-to-name table | 254 entries: ids 0 to 252 and 255 | 23.1, 23.2, 24 | Public game data | Ship as data (34.2). It can be regenerated from any public list of legacy block ids. |
| Colour property table | 16 entries | 24 | Product mapping over public ids | Ship as data (34.3). |
| Variant property table | 6 entries | 24 | Product mapping over public ids | Ship as data (34.3). |
| VOX preset | 16x16, opaque RGB, 256 texels | VOX import without an `RGBA` chunk (19.1) | Public format default, given as a rule | Generate with 34.4.1. |
| Minecraft preset | 16x16, opaque RGB, one texel per legacy id | 23.1, 24 | Derived from third-party game art by an unknown method | Ship an own table in the same layout (34.4.2), see 33.3. |
| "RGB palette (default)" picker | 16x16 RGBA, 1 empty cell, 255 colours | Colour picker | Generated layout with product-tuned values | Own generator in the same layout (34.5). |
| "HSL palette" picker | 12x22 RGBA, 4 empty cells, 256 colours | Colour picker | Generated: web-safe cube and four ramps | Generate with 34.5. |
| "JMP palette" picker | 4x4, opaque RGB | Colour picker | Third-party palette, not credited in the product | Replace, or ship with licence and credit. |
| "Generic16 palette" picker | 4x4, opaque RGB | Colour picker | Third-party palette, credited to Arne Niklas Jansson in the About box | Ship with licence and credit, or replace. |
| "Pico8 palette" picker | 4x4, opaque RGB | Colour picker | Resembles the PICO-8 palette, but several values differ from the published set as recalled (**UNVERIFIED**); not credited | Replace, or use the published palette with attribution. The label names a third-party product. |
| "EDG32 palette" picker | 6x9 RGBA, 32 colours, 22 empty cells | Colour picker | Third-party palette, credited to ENDESGA Studios in the About box | Ship with licence and credit, or replace. |
| "EDG16 palette" picker | 5x6 RGBA, 16 colours, 14 empty cells | Colour picker | Same as EDG32 | Same as EDG32. |

### 34.2 Legacy id-to-name table

The reference stores every name with the `minecraft:` prefix, omitted below. Ids 253 and 254 have no
entry. The lookup rules are in 23.4.

| Id | Name | Id | Name | Id | Name | Id | Name |
|---|---|---|---|---|---|---|---|
| 0 | `air` | 64 | `wooden_door` | 128 | `sandstone_stairs` | 192 | `acacia_fence` |
| 1 | `stone` | 65 | `ladder` | 129 | `emerald_ore` | 193 | `spruce_door` |
| 2 | `grass` | 66 | `rail` | 130 | `ender_chest` | 194 | `birch_door` |
| 3 | `dirt` | 67 | `stone_stairs` | 131 | `tripwire_hook` | 195 | `jungle_door` |
| 4 | `cobblestone` | 68 | `wall_sign` | 132 | `tripwire` | 196 | `acacia_door` |
| 5 | `planks` | 69 | `lever` | 133 | `emerald_block` | 197 | `dark_oak_door` |
| 6 | `sapling` | 70 | `stone_pressure_plate` | 134 | `spruce_stairs` | 198 | `end_rod` |
| 7 | `bedrock` | 71 | `iron_door` | 135 | `birch_stairs` | 199 | `chorus_plant` |
| 8 | `flowing_water` | 72 | `wooden_pressure_plate` | 136 | `jungle_stairs` | 200 | `chorus_flower` |
| 9 | `water` | 73 | `redstone_ore` | 137 | `command_block` | 201 | `purpur_block` |
| 10 | `flowing_lava` | 74 | `lit_redstone_ore` | 138 | `beacon` | 202 | `purpur_pillar` |
| 11 | `lava` | 75 | `unlit_redstone_torch` | 139 | `cobblestone_wall` | 203 | `purpur_stairs` |
| 12 | `sand` | 76 | `redstone_torch` | 140 | `flower_pot` | 204 | `purpur_double_slab` |
| 13 | `gravel` | 77 | `stone_button` | 141 | `carrots` | 205 | `purpur_slab` |
| 14 | `gold_ore` | 78 | `snow_layer` | 142 | `potatoes` | 206 | `end_bricks` |
| 15 | `iron_ore` | 79 | `ice` | 143 | `wooden_button` | 207 | `beetroots` |
| 16 | `coal_ore` | 80 | `snow` | 144 | `skull` | 208 | `grass_path` |
| 17 | `log` | 81 | `cactus` | 145 | `anvil` | 209 | `end_gateway` |
| 18 | `leaves` | 82 | `clay` | 146 | `trapped_chest` | 210 | `repeating_command_block` |
| 19 | `sponge` | 83 | `reeds` | 147 | `light_weighted_pressure_plate` | 211 | `chain_command_block` |
| 20 | `glass` | 84 | `jukebox` | 148 | `heavy_weighted_pressure_plate` | 212 | `frosted_ice` |
| 21 | `lapis_ore` | 85 | `fence` | 149 | `unpowered_comparator` | 213 | `magma` |
| 22 | `lapis_block` | 86 | `pumpkin` | 150 | `powered_comparator` | 214 | `nether_wart_block` |
| 23 | `dispenser` | 87 | `netherrack` | 151 | `daylight_detector` | 215 | `red_nether_brick` |
| 24 | `sandstone` | 88 | `soul_sand` | 152 | `redstone_block` | 216 | `bone_block` |
| 25 | `noteblock` | 89 | `glowstone` | 153 | `quartz_ore` | 217 | `structure_void` |
| 26 | `bed` | 90 | `portal` | 154 | `hopper` | 218 | `observer` |
| 27 | `golden_rail` | 91 | `lit_pumpkin` | 155 | `quartz_block` | 219 | `white_shulker_box` |
| 28 | `detector_rail` | 92 | `cake` | 156 | `quartz_stairs` | 220 | `orange_shulker_box` |
| 29 | `sticky_piston` | 93 | `unpowered_repeater` | 157 | `activator_rail` | 221 | `magenta_shulker_box` |
| 30 | `web` | 94 | `powered_repeater` | 158 | `dropper` | 222 | `light_blue_shulker_box` |
| 31 | `tallgrass` | 95 | `stained_glass` | 159 | `stained_hardened_clay` | 223 | `yellow_shulker_box` |
| 32 | `deadbush` | 96 | `trapdoor` | 160 | `stained_glass_pane` | 224 | `lime_shulker_box` |
| 33 | `piston` | 97 | `monster_egg` | 161 | `leaves2` | 225 | `pink_shulker_box` |
| 34 | `piston_head` | 98 | `stonebrick` | 162 | `log2` | 226 | `gray_shulker_box` |
| 35 | `wool` | 99 | `brown_mushroom_block` | 163 | `acacia_stairs` | 227 | `light_gray_shulker_box` |
| 36 | `piston_extension` | 100 | `red_mushroom_block` | 164 | `dark_oak_stairs` | 228 | `cyan_shulker_box` |
| 37 | `yellow_flower` | 101 | `iron_bars` | 165 | `slime` | 229 | `purple_shulker_box` |
| 38 | `red_flower` | 102 | `glass_pane` | 166 | `barrier` | 230 | `blue_shulker_box` |
| 39 | `brown_mushroom` | 103 | `melon_block` | 167 | `iron_trapdoor` | 231 | `brown_shulker_box` |
| 40 | `red_mushroom` | 104 | `pumpkin_stem` | 168 | `prismarine` | 232 | `green_shulker_box` |
| 41 | `gold_block` | 105 | `melon_stem` | 169 | `sea_lantern` | 233 | `red_shulker_box` |
| 42 | `iron_block` | 106 | `vine` | 170 | `hay_block` | 234 | `black_shulker_box` |
| 43 | `double_stone_slab` | 107 | `fence_gate` | 171 | `carpet` | 235 | `white_glazed_terracotta` |
| 44 | `stone_slab` | 108 | `brick_stairs` | 172 | `hardened_clay` | 236 | `orange_glazed_terracotta` |
| 45 | `brick_block` | 109 | `stone_brick_stairs` | 173 | `coal_block` | 237 | `magenta_glazed_terracotta` |
| 46 | `tnt` | 110 | `mycelium` | 174 | `packed_ice` | 238 | `light_blue_glazed_terracotta` |
| 47 | `bookshelf` | 111 | `waterlily` | 175 | `double_plant` | 239 | `yellow_glazed_terracotta` |
| 48 | `mossy_cobblestone` | 112 | `nether_brick` | 176 | `standing_banner` | 240 | `lime_glazed_terracotta` |
| 49 | `obsidian` | 113 | `nether_brick_fence` | 177 | `wall_banner` | 241 | `pink_glazed_terracotta` |
| 50 | `torch` | 114 | `nether_brick_stairs` | 178 | `daylight_detector_inverted` | 242 | `gray_glazed_terracotta` |
| 51 | `fire` | 115 | `nether_wart` | 179 | `red_sandstone` | 243 | `light_gray_glazed_terracotta` |
| 52 | `mob_spawner` | 116 | `enchanting_table` | 180 | `red_sandstone_stairs` | 244 | `cyan_glazed_terracotta` |
| 53 | `oak_stairs` | 117 | `brewing_stand` | 181 | `double_stone_slab2` | 245 | `purple_glazed_terracotta` |
| 54 | `chest` | 118 | `cauldron` | 182 | `stone_slab2` | 246 | `blue_glazed_terracotta` |
| 55 | `redstone_wire` | 119 | `end_portal` | 183 | `spruce_fence_gate` | 247 | `brown_glazed_terracotta` |
| 56 | `diamond_ore` | 120 | `end_portal_frame` | 184 | `birch_fence_gate` | 248 | `green_glazed_terracotta` |
| 57 | `diamond_block` | 121 | `end_stone` | 185 | `jungle_fence_gate` | 249 | `red_glazed_terracotta` |
| 58 | `crafting_table` | 122 | `dragon_egg` | 186 | `dark_oak_fence_gate` | 250 | `black_glazed_terracotta` |
| 59 | `wheat` | 123 | `redstone_lamp` | 187 | `acacia_fence_gate` | 251 | `concrete` |
| 60 | `farmland` | 124 | `lit_redstone_lamp` | 188 | `spruce_fence` | 252 | `concrete_powder` |
| 61 | `furnace` | 125 | `double_wooden_slab` | 189 | `birch_fence` | 255 | `structure_block` |
| 62 | `lit_furnace` | 126 | `wooden_slab` | 190 | `jungle_fence` |  |  |
| 63 | `standing_sign` | 127 | `cocoa` | 191 | `dark_oak_fence` |  |  |

### 34.3 Property tables

Colour table, for the structure property `color`:

| Value | Id | Block of that id |
|---|---|---|
| `white` | 219 | `white_shulker_box` |
| `orange` | 236 | `orange_glazed_terracotta` |
| `magenta` | 237 | `magenta_glazed_terracotta` |
| `light_blue` | 238 | `light_blue_glazed_terracotta` |
| `yellow` | 239 | `yellow_glazed_terracotta` |
| `lime` | 240 | `lime_glazed_terracotta` |
| `pink` | 241 | `pink_glazed_terracotta` |
| `gray` | 242 | `gray_glazed_terracotta` |
| `silver` | 243 | `light_gray_glazed_terracotta` |
| `cyan` | 244 | `cyan_glazed_terracotta` |
| `purple` | 245 | `purple_glazed_terracotta` |
| `blue` | 246 | `blue_glazed_terracotta` |
| `brown` | 247 | `brown_glazed_terracotta` |
| `green` | 248 | `green_glazed_terracotta` |
| `red` | 249 | `red_glazed_terracotta` |
| `black` | 234 | `black_shulker_box` |

Variant table, for the structure property `variant`:

| Value | Id | Block of that id |
|---|---|---|
| `oak` | 53 | `oak_stairs` |
| `spruce` | 134 | `spruce_stairs` |
| `birch` | 135 | `birch_stairs` |
| `jungle` | 136 | `jungle_stairs` |
| `acacia` | 163 | `acacia_stairs` |
| `dark_oak` | 164 | `dark_oak_stairs` |

Matching is exact and case-sensitive (24).

### 34.4 Preset palettes

Both presets are loaded with the rule of 2.5: texel `k` (0-based, bottom row first) becomes palette
index `k+1` on a fresh palette, and texel 255 is never loaded.

#### 34.4.1 VOX preset

- Let `L = (FF, CC, 99, 66, 33, 00)` and `D = (EE, DD, BB, AA, 88, 77, 55, 44, 22, 11)`.
- Texels 0 to 214 hold every colour `(R, G, B)` with each channel in `L`, except black. `R` is the
  outermost loop, then `G`, then `B`, and each channel runs through `L` in order.
- Texels 215 to 254 hold four ramps over `D`, in this order: red `(d, 00, 00)`, green `(00, d, 00)`,
  blue `(00, 00, d)` and grey `(d, d, d)`.
- Texel 255 is black and is never loaded.
- Every texel is opaque. Texel `k` is VOX colour index `k+1`, following the 1-based convention of the
  MagicaVoxel format.
- The rule reproduces the shipped image exactly. It matches the MagicaVoxel default palette as far as
  recalled, but it was not compared with the published table (**UNVERIFIED**).

#### 34.4.2 Minecraft preset

- Texel `k` is the colour of legacy id `k+1`, so texel row `r` holds ids `16r+1` to `16r+16`.
- Ids 253 to 256 hold one shared placeholder colour. Id 256, texel 255, is never loaded.
- A few ids share their colour with another id.
- Every texel is opaque. The values are product data derived from game art, by a method not visible
  in the analysed material, and they are not reproduced here. Two of them also appear in the default
  map text (2.7): id 2 is `5A9643` and id 3 is `A46939`.
- A replacement table MUST keep this layout, so that palette index `k` is the colour of legacy id `k`
  after a load into a fresh palette.

### 34.5 Picker palettes

The colour picker shows the chosen image as it is and takes a clicked cell's colour from it. Picker
palettes never enter the voxel palette.

- **"RGB palette (default)"**, 16x16.
  - Row 0 holds one empty cell followed by a 15-step grey ramp from black to white.
  - Rows 1 to 8 hold 16 hues each, starting at red and running through magenta, blue, cyan, green and
    yellow back towards red. They are shades whose strongest channel rises row by row, up to full
    intensity in row 8.
  - Rows 9 to 15 hold the same 16 hues as tints that get lighter row by row.
  - The port generates its own values in this layout.
- **"HSL palette"**, 12x22. Despite the name, the layout is not an HSL space.
  - Rows 0 to 17 hold the 216 web-safe colours over the levels `L` of 34.4.1. Row band `t` (rows `6t`
    to `6t+5`, `t` = 0 to 2) holds `R = L[2t]` in columns 0 to 5 and `R = L[2t+1]` in columns 6 to 11.
    Within a band, the row gives `G` and the column gives `B`, both running through `L` in order.
  - Rows 18 to 21 hold the ramps over `D` (34.4.1) for red, green, blue and grey, one ramp per row,
    each followed by one black cell and one empty cell.
  - The rule reproduces the shipped image exactly.
- **Third-party pickers.** JMP, Generic16 and Pico8 are 4x4 and fully opaque. EDG32 (6x9) and EDG16
  (5x6) leave some cells empty. Their values and cell arrangements are not reproduced here (34.1).

### 34.6 Picker palette file

- The current picker image persists as `palette.png` in the user data folder.
- Choosing a preset deletes that file, writes the preset image to it and shows it. At startup an
  existing file is shown; otherwise the RGB preset is written and shown.
- "Load new palette" accepts PNG and JPG. An image wider than 16 or taller than 32 pixels is first
  resized to `min(width, 16)` by `min(height, 32)` with point sampling, which stretches it without
  keeping the aspect ratio (read from the code, **UNVERIFIED** at runtime). The result is written to
  `palette.png`.
- "Save palette" writes the current image as PNG. The menus and dialog texts belong to the UI
  specification.
