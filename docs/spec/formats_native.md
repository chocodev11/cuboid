# Native file formats: `.cubik` and legacy `.cub`

Status: specification for the Cuboid port, derived from a functional study of Cubik Studio 2.9.480 Beta.
Audience: implementers of the Rust reader/writer and of the parity test suite.

Conventions used in this document:

- **MUST / SHOULD / MAY** have their RFC 2119 meaning and apply to the Cuboid implementation.
- "Cubik" means the original application (2.9.480 Beta). "The port" means Cuboid.
- "f32" is an IEEE-754 single, "f64" a double, "i32" a signed 32-bit integer, all little-endian on disk.
- All coordinates are in the original editor space: Unity conventions, left-handed, Y up, 1 unit = 1 Minecraft
  pixel (16 units = 1 block). Section 4.11 summarises the conventions.
- Items marked **UNVERIFIED** could not be confirmed from the program's code or shipped binaries and need sample files
  or a run of the original application. Section 12 collects them.
- Items marked "verified (sample 1)" were confirmed against the first real `.cubik` file received for this study.
  Section 11.1 lists what that file proved and what it did not cover.

---

## 1. Overview

| Extension | Name shown by Cubik | Direction in Cubik | Physical format |
|---|---|---|---|
| `.cubik` | "Cubik model" | open and save | ZIP archive with one entry named `cubik`; the entry is a BSON document |
| `.cub` | "Cubik model alpha" | open only (no writer exists) | Text file: JSON obfuscated with a fixed repeating XOR |

Related facts:

- Automatic backups use the `.cubik` format unchanged (section 5.1).
- Cubik selects the format by the file extension only. The comparison is **case-sensitive** and ignores the dot:
  `model.cubik` is accepted, `model.CUBIK` is rejected with "Unknown file type". The port MAY accept any case, but MUST
  NOT rely on Cubik doing so. Cubik never sniffs file content.
- After opening a `.cub` file, Cubik remembers it as the current document path. A plain Save then fails with the message
  "Can't generate ".cub"" because no `.cub` writer exists; only Save As works.

---

## 2. The `.cubik` container

### 2.1 Structure

A `.cubik` file is a standard ZIP archive (PKWARE APPNOTE) containing exactly one file entry:

| Property | Value written by Cubik |
|---|---|
| Entry name | `cubik` (lower case, no extension, no directory), ASCII; flag bit 11 (UTF-8 names) clear. Verified (sample 1). |
| Compression | DEFLATE (method 8) at "best speed", which is zlib level 1. Verified (sample 1). Payloads larger than 524 288 bytes (512 KiB, the library's default parallel threshold) go through the library's parallel deflater: the input is cut into 64 KiB segments, each segment is compressed as an independent raw-deflate stream ended by a sync flush (an empty stored block, `00 00 FF FF`), and one empty final block (`03 00`) closes the stream. Verified (sample 1: a 953 962-byte payload in 15 segments). Smaller payloads are believed to be one ordinary stream (library code; no sample yet). Very small or incompressible payloads may be stored instead (method 0), because the ZIP library falls back to storing when compression does not help. **UNVERIFIED** for real files. |
| Encryption | none, no password. Verified (sample 1). |
| Header fields | version needed to extract 20; version made by `0x002D` (host 0 = MS-DOS, specification 4.5); general purpose flags 0, so there is no data descriptor and the local header carries CRC and sizes; internal and external attributes 0; no ZIP64 records. Verified (sample 1). |
| Archive comment, entry comment | none. Verified (sample 1). |
| Other entries | none. Verified (sample 1). |
| Timestamps / extra fields | DOS date and time in local time of the save, and one extra field `0x000A` (NTFS) with 32 data bytes: 4 reserved zero bytes, tag 1, size 24, then the modification, access and creation times as UTC FILETIME values, all three equal. Local and central header carry the same extra field. Verified (sample 1). Not meaningful for reading. |

The library that writes and reads the archive is DotNetZip 1.9.1.9 ("Ionic's Zip Library", identified from the
shipped binary). The entry payload is described in sections 3 and 4.

Container identity (informative): classic zlib 1.3.1 (raw deflate, level 1, 15-bit window, memory level 8, default
strategy), driven segment by segment as described above, reproduces the compressed stream of sample 1 byte for byte,
and with the header fields above and the original timestamps the whole file. zlib-ng at level 1 produces a different
and larger stream (243 500 instead of 173 757 bytes for sample 1 with the same segmenting), and classic zlib without
the segmenting produces a different one too (173 889 bytes). Byte identity of the container is not a requirement of
this specification; tests compare payloads (10.2).

### 2.2 How Cubik reads the container

1. The path must exist, otherwise the load stops with the import error dialog (step 3) showing "Invalid path".
2. The archive is opened, the entry `cubik` is looked up by name and decompressed into memory. The library's default
   name lookup is believed to be case-insensitive (**UNVERIFIED**).
3. Any failure in steps 1-2 (not a ZIP, truncated archive, entry missing, corrupt data) is logged as
   "Invalid Zip file: <message>" and the load continues with no payload, which then fails with the error dialog
   "Cubik model Import" / "An error occured while importing Cubik model:" followed by "Invalid Cubik model".
4. Other entries in the archive are ignored.

### 2.3 How Cubik writes the container

1. The BSON payload is produced completely in memory first.
2. A new archive with the single entry is written to the target path, replacing any existing file. Whether the
   library writes through a temporary file is **UNVERIFIED**.
3. Exceptions during the ZIP stage are caught inside the writer: Cubik shows "Error while compressing file" /
   "An error occured while compressing file "<name>".", followed by the exception text, and then continues as if the
   save had succeeded (status line "Saved CUBIK: <path>", dirty flag cleared). Exceptions during serialisation are
   reported as "Error while saving" / "An error occured while saving <format> export:" followed by the message, with
   an "Abort" button.

### 2.4 Requirements for the port

Reader:

- MUST accept stored (0) and deflate (8) entries, with or without data descriptors and with or without ZIP64 records.
- MUST locate the entry named exactly `cubik`. SHOULD fall back to a case-insensitive match. MUST ignore all other
  entries.
- MUST report "not a ZIP", "no `cubik` entry", and "corrupt entry" as distinct errors, and MUST NOT modify the open
  document when the container cannot be read.

Writer (Cubik-compatible output):

- MUST write a single entry named `cubik` using method 8 (deflate, any level) or method 0. MUST NOT encrypt. SHOULD NOT
  emit ZIP64 structures unless a size requires them. SHOULD NOT add other entries to a file intended for Cubik (Cubik
  ignores them, but drops them when it re-saves; see 8.4).
- MAY reproduce the segmenting and header fields of 2.1 when a byte-identical container is wanted; this needs a
  deflate implementation that matches classic zlib at level 1.
- MUST write to a temporary file in the same directory and rename it over the target only after the archive is
  complete. Unlike Cubik, the port MUST report a failed save as a failure.

---

## 3. Payload encoding

The `cubik` entry contains one BSON document (bsonspec.org, version 1.1 wire format) produced by Json.NET 8.0
(the Unity build shipped with the application, assembly version 8.0.0.0, "Json.NET .NET 3.5"). This section
describes exactly which BSON features occur and how program values map to them, so that the port can produce
byte-identical payloads.

### 3.1 BSON subset

| Type byte | BSON type | Used for |
|---|---|---|
| `0x01` | double (8-byte IEEE-754) | every floating-point value (all are f32 in the program) |
| `0x02` | UTF-8 string: i32 byte length including the trailing NUL, bytes, `0x00` | strings |
| `0x03` | embedded document | objects, including Unity vectors and quaternions |
| `0x04` | array (a document whose keys are `"0"`, `"1"`, ... in order) | lists and arrays |
| `0x05` | binary: i32 length, subtype byte, bytes | raw texture data (subtype `0x00`, "generic") |
| `0x08` | boolean (one byte, `0x00` or `0x01`) | flags |
| `0x0A` | null (no payload) | null object references and null strings |
| `0x10` | int32 | `int` fields, the one `byte` field, and every enumeration |

Never produced by Cubik: int64 (`0x12`), ObjectId, dates, regex, JavaScript, decimal128, timestamps, min/max keys,
undefined. The root is always a document, never an array. Verified (sample 1): all eight types of the table occur,
no other type occurs, and binary values use subtype `0x00` only.

General wire rules (standard BSON, repeated here because the port writes them by hand or through a raw builder):

- A document is: i32 total size in bytes (including the size field and the terminator), the elements, then `0x00`.
- An element is: type byte, key as a NUL-terminated UTF-8 string, value.
- Keys inside arrays are the decimal indices without leading zeros.
- The reader in Cubik relies on the size fields to find the end of each document, so they MUST be exact.
- Verified (sample 1): every size field is exact, every string length includes its terminator, and array keys are
  `"0"`, `"1"`, ... without gaps.

### 3.2 Value mapping rules

| Program value | BSON | Notes |
|---|---|---|
| f32 | double | The f32 is widened exactly to f64 (no rounding, no decimal conversion). Example: f32 0.1 is stored as f64 0.100000001490116119384765625 (`0x3FB99999A0000000`). A writer MUST use `f64::from(x)`. A reader converts back with round-to-nearest-even, which restores the original f32 exactly. Verified (sample 1): every double is an exact widening (for example 3.4781999588012695, the f32 of 3.4782), and -0.0 keeps its sign bit (4.3.2). |
| i32 | int32 | |
| byte (only the root key `v`) | int32 | |
| enumeration | int32 | The numeric value of the enumeration member; names are never written. |
| bool | boolean | |
| string | string, or null | Encoded as UTF-8 without BOM. |
| byte array | binary subtype `0x00`, or null | |
| object reference | document, or null | |
| list or array | array, or null | Empty lists are written as empty arrays (5-byte document). |
| Unity vector or quaternion | document (section 3.4) | Never null. |

### 3.3 Member order

Within every document, keys appear in the order given by the schema tables of section 4. The rule behind that order
(useful when checking a table) is: all serialised fields of the type in declaration order, followed by all
serialised properties in declaration order. The port MUST write keys in the documented order to produce
byte-identical payloads; Cubik itself reads keys in any order.

Verified (sample 1) for every record type that file contains: the root, ModelProperties with all its groups,
ViewProperties, CameraState, Quaternion, Vector3, the blueprint ElementRecord, TextureRecord, RawTexture, VoxelData
with its world, chunk and voxel records, ModelDisplay and DisplaySlot. A writer that takes only the values from the
file and writes them with the tables of section 4 reproduced the whole payload byte for byte (11.1). Not covered:
Vector2, Box, FaceSet, FaceRecord, FaceUv, element items of `el`, and the mesh records.

### 3.4 Unity value types

These program types are written as plain documents. The member lists below were checked against the Unity 5.5.3
engine assembly shipped with the application.

| Type | Document keys (in order) | Size of the document |
|---|---|---|
| Vector2 | `x`, `y` (doubles) | 27 bytes |
| Vector3 | `x`, `y`, `z` (doubles) | 38 bytes; verified (sample 1) |
| Quaternion | `x`, `y`, `z`, `w` (doubles), then `eulerAngles` (a Vector3 document, degrees) | 100 bytes; verified (sample 1) |

Notes:

- Read-only members of these types (length, normalised vector) are never written. Verified (sample 1).
- The stored components are the engine's single-precision results, not exact values: sample 1 holds quaternions whose
  length differs from 1 by up to 7e-8, components that are not the nearest f32 of the exact value, Euler angles one
  f32 step away from whole degrees, and -0.0 components. Section 4.3.2 gives the consequences.
- Quaternions carry a redundant Euler-angle copy. When Cubik reads a quaternion it first sets `x`, `y`, `z`, `w`, and
  then, because `eulerAngles` comes later in the document, replaces the whole rotation with the rotation built from
  those Euler angles. The effective value is therefore `Euler(eulerAngles)`, not the stored components. A port reader
  MUST apply the members in document order with the same override; a port writer MUST write `eulerAngles` consistent
  with `x`, `y`, `z`, `w` (section 4.3.2 gives the conventions).
- Colours, bounds and rectangles of the engine never appear in the payload. Colours are stored as packed integers
  (section 4.10) and bounds through a dedicated record (section 4.6.2).

### 3.5 Worked byte examples

All values below were computed from the rules in this section; they are test vectors for the codec.

Empty document (used for the groups `ht` and `tr`, section 4.2):

```
05 00 00 00 00
```

Vector3 (1, 2, 3):

```
26 00 00 00
01 78 00  00 00 00 00 00 00 F0 3F      x = 1.0
01 79 00  00 00 00 00 00 00 00 40      y = 2.0
01 7A 00  00 00 00 00 00 00 08 40      z = 3.0
00
```

Vector2 (0.1f, 0.5f), showing the exact f32 widening:

```
1B 00 00 00
01 78 00  00 00 00 A0 99 99 B9 3F      x = 0.1f widened
01 79 00  00 00 00 00 00 00 E0 3F      y = 0.5
00
```

Face record for the bottom face (name "down", enabled, no tint, no cull, texture "texture", rotation 90,
UV (0,0)-(1,1)); 129 bytes:

```
81 00 00 00
02 6E 00 05 00 00 00 64 6F 77 6E 00                 n  = "down"
08 61 00 01                                          a  = true
10 74 69 00 FF FF FF FF                              ti = -1
10 63 00 00 00 00 00                                 c  = 0
02 74 00 08 00 00 00 74 65 78 74 75 72 65 00         t  = "texture"
03 75 76 00 4A 00 00 00                              uv = {
   10 72 00 5A 00 00 00                                r  = 90
   03 74 6C 00 1B 00 00 00 01 78 00 00 00 00 00 00 00 00 00
                           01 79 00 00 00 00 00 00 00 00 00 00   tl = (0,0)
   03 62 72 00 1B 00 00 00 01 78 00 00 00 00 00 00 00 F0 3F
                           01 79 00 00 00 00 00 00 00 F0 3F 00   br = (1,1)
   00                                                }
00
```

Voxel record x=1, y=2, z=3, colour 0xFF5A9643 (stored as the signed value -10840509):

```
21 00 00 00 10 78 00 01 00 00 00 10 79 00 02 00 00 00 10 7A 00 03 00 00 00 10 63 00 43 96 5A FF 00
```

Every voxel record of sample 1 has exactly this 33-byte layout. Verified (sample 1).

### 3.6 Type conversions performed by Cubik's reader

Cubik's reader is lenient. The port reader SHOULD accept the same inputs so that hand-edited or third-party files
behave the same way. Cubik never writes the alternative forms.

| Target | Accepted BSON values | Conversion |
|---|---|---|
| f32 | double, int32, int64 | integers converted; doubles rounded to nearest f32 |
| i32 | int32, int64, double | doubles rounded half-to-even; out-of-range values make the load fail |
| `v` (byte) | as i32 | values outside 0..255 make the load fail |
| bool | boolean, int32, int64, double | non-zero is true |
| enumeration | int32, int64, string | integers are taken as-is (undefined values are kept, not rejected); strings are matched to member names case-insensitively |
| string | string, null | other scalar types are converted to text (rare; the port MAY reject) |
| byte array | binary (any subtype), string, null | a string is decoded as base64; subtype `0x02` carries an extra inner length prefix |
| object, list | document / array, or null | |
| Unity vector / quaternion | document | **null makes the whole load fail** in Cubik |

Additional rules:

- Unknown keys are ignored at every level.
- Missing keys keep the value that a freshly constructed record has ("If absent" columns in section 4).
- If a key occurs twice, the last occurrence wins.
- For keys of value type (vectors, numbers, booleans) an explicit null makes Cubik's load fail. The port reader SHOULD
  instead treat it as absent and record a warning.

---

## 4. Document schema

Notation for the tables:

- **Key**: the key exactly as stored.
- **BSON**: element type. "doc X" means an embedded document of record X; "arr X" an array of X; "/null" means Cubik
  can write null here.
- **Cubik writes**: the value written for a fresh model (after start-up or File > New), or the rule that produces the
  value.
- **If absent**: the value used by Cubik when the key is missing from the file.
- Rows appear in the exact on-disk order.

### 4.1 Root document

| Key | BSON | Meaning | Cubik writes | If absent |
|---|---|---|---|---|
| `v` | int32 | format version | always `1` | 1 (the value is never checked) |
| `mp` | doc ModelProperties | model-level settings | always present | model settings are not touched |
| `vp` | doc ViewProperties | grid steps and the four view cameras | always present | view state is not touched |
| `bp` | doc ElementRecord | the blueprint (the reference box used by the creation and voxel tools) | always present | blueprint not touched |
| `el` | arr ElementRecord | all elements in list order | always present, may be empty | no elements are added |
| `tx` | arr TextureRecord | the texture list in list order | always present | texture list not touched (an empty array behaves the same) |
| `vx` | doc VoxelData /null | the voxel model | null when no voxel grid has been set up | voxel grid is cleared if one exists |
| `me` | arr MeshRecord | all meshes in list order | always present, may be empty | no meshes are added |
| `md` | doc ModelDisplay | Minecraft display settings | always present | display settings not touched |

There is no "save parts" mask on disk: every save writes every section.

Verified (sample 1): the key order of the table, `v` = 1, `vx` a document, and empty `el` and `me` written as empty
arrays (5-byte documents).

### 4.2 ModelProperties (`mp`)

| Key | BSON | Meaning | Cubik writes | If absent |
|---|---|---|---|---|
| `sp` | string | absolute path of the document | for a normal save: the target path of this save; for a backup: the document's previous save path (or "") | "" |
| `ep` | string | last import or export path | path, or "" | "" |
| `g` | doc GeneralSettings | general model settings | | defaults below |
| `mc` | doc MinecraftSettings | Minecraft export settings | | defaults below |
| `ht` | doc (empty) | an option group without members (purpose unknown) | always `{}` | |
| `sh` | doc StonehearthSettings | Stonehearth export settings | | defaults below |
| `st` | doc StaxelSettings | Staxel export settings | | defaults below |
| `tr` | doc (empty) | an option group without members (purpose unknown) | always `{}` | |
| `qb` | doc QubicleSettings | Qubicle export settings | | defaults below |
| `obj` | doc MeshExportSettings | mesh (OBJ and similar) export settings | | defaults below |
| `png` | doc PngRenderSettings | PNG render export settings | | defaults below |

Paths are absolute and use `/` as the separator, also on Windows (sample 1: a drive-letter path of the form
`X:/folder/name.cubik`); readers MUST accept `\` as well. They make files non-relocatable and may disclose user
names; the port SHOULD treat them as hints only. In sample 1 the file name in `sp` differed from the name of the file
itself, which had been renamed or copied after saving.

Verified (sample 1): the key order and types of 4.2 to 4.2.3, empty `ht` and `tr` groups, and, for a model whose
properties had apparently never been edited, every fresh value listed in 4.2.1 to 4.2.3 (including `rer` = true, the
`mt` map text and `mb` = null).

None of the sub-documents may be null: Cubik copies each group field by field when it applies the model properties,
and a null group aborts the load.

#### 4.2.1 GeneralSettings (`mp.g`)

| Key | BSON | Meaning | Cubik writes / fresh value |
|---|---|---|---|
| `o` | doc Vector3 | model origin (pivot marker) | (0, 0, 0) |
| `mb` | doc Box /null | model box bounds | null until the user drags the model box handles; then the current box |
| `ee` | boolean | exclude elements from exports | false |
| `ev` | boolean | exclude voxels from exports | false |
| `em` | boolean | exclude meshes from exports | false |
| `me` | boolean | minify (compact) exported text ("Compress data") | false |
| `rer` | boolean | restrict element rotations to Minecraft rules (single axis, 22.5 degree steps) | **true** |
| `ehe` | boolean | exclude hidden elements from exports | false |
| `pbt` | boolean | prevent texture bleeding by adjusting UVs | false |
| `pzf` | boolean | avoid z-fighting with small random offsets | false |
| `d` | string | description / signature | never null; when the user never set one, the expansion of the user's signature template (default template `Designed by %author with %cubik - https://cubik.studio`, where `%author` and `%email` come from the user's registration and `%cubik` is the application name). Verified (sample 1): `%cubik` expands to `Cubik Studio`, and an empty author leaves two spaces: `Designed by  with Cubik Studio - https://cubik.studio` |
| `n` | string | model name | never null; default `Cubik Model` |

Rules for `d` and `n` (also for `mc.p`):

- Whenever Cubik assigns these strings, including while reading a file, every ASCII double quote `"` (U+0022) is
  replaced by U+201D `”`. Files written by Cubik therefore never contain `"` in them. The port reader MUST apply the
  same replacement; the port writer MUST NOT write `"`.
- Cubik never writes null for them, and a null in a file makes Cubik's load fail. The port writer MUST write a string.

If absent, the values listed above apply (`d` falls back to the template expansion, `n` to `Cubik Model`).

Box (`mp.g.mb`):

| Key | BSON | Meaning |
|---|---|---|
| `min` | doc Vector3 | minimum corner |
| `max` | doc Vector3 | maximum corner |

The editor clamps the box interactively to x in [-16, 32], y in [-16, 32], z in [-32, 16]; the default box is exactly
those limits. On load the stored values are applied without clamping. A null box leaves the current box unchanged.

#### 4.2.2 MinecraftSettings (`mp.mc`)

| Key | BSON | Meaning | Fresh value |
|---|---|---|---|
| `t` | int32 | model kind (enumeration ModelKind, section 4.9) | 1 (block) |
| `ao` | boolean | Minecraft ambient occlusion flag | true |
| `uen` | boolean | include element names in the export | true |
| `uvds` | boolean | automatic parent for vanilla display settings | false |
| `uds` | boolean | include display settings in the export | false |
| `uos` | boolean | keep imported item overrides | false |
| `eab` | string | entity: attach bone name | "" |
| `etr` | int32 | entity: generated texture resolution | 16 |
| `etx` | int32 | entity: texture offset X (16x units) | 0 |
| `ety` | int32 | entity: texture offset Y | 0 |
| `etw` | int32 | entity: minimum texture width | 64 |
| `eth` | int32 | entity: minimum texture height | 64 |
| `esb` | boolean | entity: group all elements as one bone, origin as pivot | true |
| `est` | boolean | entity: reuse the texture for duplicate elements | true |
| `ecn` | boolean | entity: use element labels as cube names | false |
| `mt` | string | voxel colour to block mapping table | the four-line text `empty = air`, `default = stone`, `#5A9643 = grass`, `#A46939 = dirt`, joined with `\n` |
| `p` | string | Minecraft parent model | "" (never null; quote rule of 4.2.1 applies) |

Not stored in this group: the model display (stored at the root under `md`) and the Minecraft item overrides
(predicate cases). **Item overrides imported from Minecraft JSON are lost when Cubik saves a `.cubik` file.**

#### 4.2.3 Other option groups

StonehearthSettings (`mp.sh`):

| Key | BSON | Meaning | Fresh value |
|---|---|---|---|
| `pog` | boolean | placeable on ground | false |
| `pow` | boolean | placeable on walls | false |
| `v` | int32 | value in gold | 10 |
| `r` | string | rarity | `common` |
| `b` | boolean | buyable | true |
| `s` | boolean | sellable | true |

StaxelSettings (`mp.st`):

| Key | BSON | Meaning | Fresh value |
|---|---|---|---|
| `t` | int32 | Staxel object kind (enumeration StaxelKind) | 3 (accessory) |

QubicleSettings (`mp.qb`):

| Key | BSON | Meaning | Fresh value |
|---|---|---|---|
| `rgb` | boolean | write RGB instead of BGR colour order | false |
| `aiz` | boolean | right-handed axes (inverted Z) | false |

MeshExportSettings (`mp.obj`):

| Key | BSON | Meaning | Fresh value |
|---|---|---|---|
| `gmv` | boolean | voxels: detailed mesh with coloured vertices | true |
| `gmt` | boolean | voxels: detailed mesh with texture palette | false |
| `omt` | boolean | voxels: optimised mesh with texture atlas | false |
| `wv` | boolean | weld vertices at identical positions | true |
| `mo` | boolean | merge all objects into one mesh | false |
| `pt` | boolean | add padding to voxel atlas textures | false |
| `ao` | boolean | render fake ambient occlusion; **also drives voxel ambient occlusion inside the editor** | true |
| `sv` | boolean | smooth voxel vertices on export | false |

PngRenderSettings (`mp.png`):

| Key | BSON | Meaning | Fresh value |
|---|---|---|---|
| `gp` | boolean | render type "GUI display preview" | true |
| `mv` | boolean | render type "Main view" | false |
| `aa` | boolean | anti-aliasing | true |

For all option groups the "If absent" value equals the fresh value.

### 4.3 ViewProperties (`vp`)

| Key | BSON | Meaning | Cubik writes | Applied on load |
|---|---|---|---|---|
| `ctl` | doc CameraState /null | top-left view camera (top orthographic view) | captured from the camera just before saving | yes, if not null |
| `ctr` | doc CameraState /null | top-right view camera (side orthographic view) | captured | yes, if not null |
| `cbl` | doc CameraState /null | bottom-left view camera (front orthographic view) | captured | yes, if not null |
| `cbr` | doc CameraState /null | bottom-right view camera (main perspective view) | captured | yes, if not null |
| `gm` | double | grid step for mouse moves | current value (fresh 0.5) | yes |
| `gk` | double | grid step for keyboard moves | current value (fresh 0.125) | yes |
| `gu` | double | grid step for UV editing | **always 0.5** (the current value is not copied into the saved record) | **no** |
| `gd` | double | displayed sub-grid step | current value (fresh 1.0) | yes (refreshes the grid) |
| `gl` | boolean | large grid mode | **always false** (not copied) | **no** |

If a key is absent the defaults are: cameras null, `gm` 0.5, `gk` 0.125, `gu` 0.5, `gd` 1.0, `gl` false.
Loading a file also forces "snap to grid" on, because that flag is not stored and the loader copies it from the
freshly constructed record, whose default is on.

Sample 1: key order and types as in the table, all four cameras present, `gu` 0.5 and `gl` false, and `gm` and `gd`
with values other than the fresh ones (the current values are saved). This is consistent with the table but does not
prove the "always" rules for `gu` and `gl`.

The camera documents are null only in hand-made files; Cubik always captures all four.

#### 4.3.1 CameraState

| Key | BSON | Meaning |
|---|---|---|
| `p` | doc Vector3 | camera world position |
| `r` | doc Quaternion | camera world rotation (includes the `eulerAngles` copy, see 3.4) |
| `o` | boolean | orthographic projection |
| `os` | double | orthographic size (half of the vertical view extent, world units) |
| `fv` | double | vertical field of view in degrees |

Camera states found in sample 1 (key order verified (sample 1)):

| Key | `p` | `r.eulerAngles` | `o` | `os` | `fv` |
|---|---|---|---|---|---|
| `ctl` (top) | (8, 72, -8) | (90, 0, 0) | true | 12 | 60 |
| `ctr` (side) | (-54, 8, -8) | (-0.0, 90.00000762939453, 0) | true | 12 | 60 |
| `cbl` (front) | (8, 8, -54) | (-0.0, 0, 0) | true | 12 | 60 |
| `cbr` (perspective) | (-4, 24, -20) | (45.000003814697266, 45.000003814697266, 0) | false | 12 | 64 |

`os` is written for the perspective camera as well, and `fv` is 60 for the orthographic ones. The values are round
and aimed at the centre of the model zone (8, 8, -8), so they are believed to be the start-up view cameras; this
stays UNVERIFIED until request 2 of 11.2 (the sample's views may have been reset rather than never moved). The
orthographic size 12 and the perspective field of view 64 of the earlier scene reading agree with the sample.

#### 4.3.2 Rotation conventions for `eulerAngles`

- `Euler(x, y, z)` (degrees) builds the rotation that applies a rotation about Z by z, then about X by x, then about Y
  by y, in a left-handed, Y-up frame. As a quaternion product: `q = qY(y) * qX(x) * qZ(z)`.
- The stored `eulerAngles` is the inverse decomposition of the quaternion in the same order, converted to degrees and
  then made positive per component: a component below -0.0057295779 (that is -0.0001 rad expressed in degrees) gets
  360 added, a component above 359.99427 gets 360 subtracted.
- The engine computes the decomposition natively in single precision, and a port implementation will differ in the
  last bits. Sample 1 shows the size of the effect: 45.000003814697266 and 90.00000762939453 are one f32 step above
  45 and 90, and the components of the 90-degree rotations are 0.7071068286895752 and 0.7071067094802856, not the
  nearest f32 of the exact value. Recomputing the Euler angles from the stored quaternions in double precision agrees
  with the stored angles within 1e-6 degrees. Tests MUST therefore compare with a tolerance of 1e-4 degrees (enough
  for every value of sample 1); byte-identical camera states need the retained values (8.3).
- Negative zero: the x component comes from the arcsine of a negated matrix term (`asin(-2(yz - wx))` in quaternion
  components). When that term is exactly +0, as for the identity and for a pure Y rotation, the stored x is -0.0,
  and the positive-making step leaves -0.0 unchanged. Verified (sample 1: the side and front cameras). A writer MUST
  keep the sign bit of retained values and SHOULD use the same formula for new values.
- Gimbal lock: at x = 90 or -90 degrees only the difference (or sum) of y and z is defined. Sample 1 stores the top
  camera, a rotation of 90 degrees about X, as (90, 0, 0). A plain decomposition of the same stored quaternion yields
  (90, 180, 180), because rounding turns two matrix terms into tiny negative numbers. The port MUST handle this case
  with a singular branch that sets one of y and z to 0 (which one the engine zeroes is UNVERIFIED; both are 0 in the
  sample), and tests SHOULD compare rotations (for example the angle between quaternions) rather than Euler
  components.

### 4.4 ElementRecord (`bp` and the items of `el`)

| Key | BSON | Meaning | Notes |
|---|---|---|---|
| `p` | doc Vector3 | local position of the element (the min-x, min-y, max-z corner of the unrotated box) | already includes the effect of rotating about the pivot |
| `s` | doc Vector3 | size (local scale) | the editor keeps each component >= 0.001 |
| `r` | doc Vector3 | rotation as Euler angles in degrees (convention of 4.3.2) | quantised, see 6.3 |
| `o` | doc Vector3 | pivot (rotation origin) | |
| `c` | int32 | element colour, packed (section 4.10) | 0 when the colour is fully transparent |
| `is` | boolean | selected at save time | restored as selection on load |
| `ih` | boolean | hidden | |
| `il` | boolean | locked | |
| `l` | string | label | |
| `f` | doc FaceSet /null | the six faces | null only for `bp` |
| `rs` | boolean | Minecraft "rescale" for rotated elements | |
| `suv` | boolean | automatic box UV mapping ("UV snap") | |
| `suvm` | boolean | automatic UV mapping, mirrored variant | |
| `sh` | boolean | Minecraft "shade" | |
| `in` | boolean | inverted normals | |
| `t` | int32 | element type (enumeration ElementType) | |

If absent (records created by the reader): vectors (0,0,0), `c` 0, flags false, `l` null, `f` null, `t` 0.

The blueprint record (`bp`) is written from the blueprint object, which is not an element: Cubik writes its `p`, `s`,
`r`, `is`, `ih`, `il`, `l` normally, writes `o` as the blueprint's **world** position, and writes `c` = 0,
`f` = null, `rs`/`suv`/`suvm`/`sh`/`in` = false and `t` = 0. On load only `p`, `s`, `r`, `ih`, `il` and `l` have a
visible effect. Verified (sample 1): key order, `o` equal to `p` (blueprint at top level), `c` 0, `f` null, the five
flags false, `t` 0, and the label `Blueprint` (believed to be the default label).

For items of `el` the port writer MUST write a non-null `f` with all six faces, and every face MUST have a non-null
`uv` and `t`: Cubik dereferences them without checks.

#### 4.4.1 FaceSet (`f`)

| Key | BSON | Face |
|---|---|---|
| `d` | doc FaceRecord | down (-Y) |
| `u` | doc FaceRecord | up (+Y) |
| `n` | doc FaceRecord | north |
| `s` | doc FaceRecord | south |
| `w` | doc FaceRecord | west |
| `e` | doc FaceRecord | east |

#### 4.4.2 FaceRecord

| Key | BSON | Meaning | Value for a new element / if absent |
|---|---|---|---|
| `n` | string | face name | the face's own name: `down`, `up`, `north`, `south`, `west`, `east` |
| `a` | boolean | face enabled (drawn and exported) | true |
| `ti` | int32 | Minecraft tint index, -1 for none | -1 |
| `c` | int32 | Minecraft cull face (FaceType flags, section 4.9) | 0 |
| `t` | string | texture label (key into the texture list) | if absent: `texture`; a new element created in the editor gets the first texture of the list that is not `particle` |
| `uv` | doc FaceUv | UV rectangle | see below |

#### 4.4.3 FaceUv (`uv`)

| Key | BSON | Meaning |
|---|---|---|
| `r` | int32 | texture rotation in degrees |
| `tl` | doc Vector2 | first corner (u, v) |
| `br` | doc Vector2 | second corner (u, v) |

Meaning and rules:

- Coordinates are normalised to the face's own texture (the one named by `t`), with the origin at the bottom-left
  of the image. They are not relative to the packed atlas.
- Conversion from Minecraft pixel UVs `[x1, y1, x2, y2]` (16 units per texture side):
  `tl = (x1/16, 1 - y2/16)` and `br = (x2/16, 1 - y1/16)`; and back:
  `x1 = 16*tl.x`, `y1 = 16*(1 - br.y)`, `x2 = 16*br.x`, `y2 = 16*(1 - tl.y)`.
- `r` is normalised whenever it is assigned (also on load): divide by 90, round half-to-even, multiply by 90, add 360,
  take the remainder modulo 360. Results are 0, 90, 180 or 270 for inputs above -360.
- Corner values assigned through the record (editor edits and `.cubik` loading) are clamped per component to
  [-1, 2]. The `.cub` importer (and the Minecraft JSON importer, which shares its code) stores imported corners
  directly, without this clamp (9.5).
- **Values written to the file are "folded" into [0, 1].** For each axis independently, with `a` = stored first
  corner component and `b` = stored second corner component:
  - first corner: if `a < 0` and `b > 0` write 0; else if `a < 0` write `a + 1`; else if `a > 1`, or `a == 1` and
    `b > 1`, write `a - 1`; otherwise write `a`.
  - second corner: if `a < 1` and `b > 1` write 1; else if `b > 1` write `b - 1`; else if `b < 0`, or `b == 0` and
    `a < 0`, write `b + 1`; otherwise write `b`.
  The arithmetic is done in f32. For corners within [-1, 2] the written values are always within [0, 1] and a
  load-then-save cycle is stable after the first save. Unclamped imported corners outside [-1, 2] can be written
  outside [0, 1]; the next `.cubik` load clamps them, and they settle after one or two more save and load cycles.
- A FaceUv created by the reader without keys has both corners at (0, 0) and rotation 0. (The editor's "reset UV"
  action sets (0, 0) and (1, 1).)

### 4.5 TextureRecord (items of `tx`)

| Key | BSON | Meaning | Cubik writes |
|---|---|---|---|
| `lb` | string | texture label, the unique key used by faces (the rename dialog restricts it to `A-Z a-z 0-9 - _`; imported labels may contain other characters) | non-empty label; an empty or null label makes Cubik report an "Invalid Label" error and is expected to abort the rest of the load (UNVERIFIED) |
| `fp` | string /null | absolute source file path with `/` separators | null when the entry's image was assigned in memory during the session (default textures, missing-file placeholders, shortcuts, generated images, and every texture after a paint stroke, 4.5.3): that assignment clears the path. After a reopen the same entries have "" (6.3) |
| `fn` | string /null | file name without extension | taken from `fp` when `fp` is not empty, otherwise a stored name (null for the default textures) |
| `fm` | string | Minecraft-style texture id or `#label` shortcut | when never set explicitly: the part of `fp` after `/textures/` without extension, or the bare file name without extension, or "" |
| `tx` | doc RawTexture | the image itself | never null |
| `cs` | string /null | content signature of the source file | null when never computed; also cleared (null) whenever an image is assigned in memory, which includes every paint stroke (4.5.3) |

The texture list of a fresh model holds two entries, `particle` and `texture`. Section 4.5.2 gives their exact
records. Verified (sample 1): the key order of TextureRecord and RawTexture, and null `fp`, `fn` and `cs` written as
BSON null. A label of the default list can hold any image: in sample 1 the entry `texture` held a user image
(non-square, not a power of two, with partial transparency) whose `fm` still held a file-name id while `fp` was
null, so `fm` survives the loss of the path (4.5.3).

About `cs`: Cubik computes the MD5 digest of the source file and turns the 16 digest bytes into text with the
operating system's ANSI code page (for example Windows-1252), not as hexadecimal. The result is machine-dependent
and is only ever compared for equality: when the file at `fp` exists and its current signature differs from `cs`,
Cubik reloads the texture from disk when the window regains focus (recorded as the undo step
"Reload textures from HDD"). An empty or null `cs` disables that check. Rules for the port:

- MUST treat `cs` as opaque text and preserve it unchanged for textures that are not reloaded.
- SHOULD, for textures it loads from disk itself, compute the same value (MD5 bytes decoded with the platform ANSI
  code page) so that Cubik does not reload them needlessly; if that is not possible it MAY write null.

On load, every record is added to (or replaces the image of) the texture with label `lb`, and then `fp`, `fn`, `fm`
and `cs` are copied onto it; the path setter turns a null `fp` into "". If a label occurs twice, the second record
replaces the first. A record labelled `error` replaces the image of Cubik's internal missing-texture entry instead
of creating a list entry, so it is not listed and the next save drops it (code reading). The port SHOULD load such a
record as a normal texture and warn.

#### 4.5.1 RawTexture (`tx.tx`, `me[].tx`, `me[].sm[].tx`)

| Key | BSON | Meaning |
|---|---|---|
| `n` | string | image name (informational) |
| `w` | int32 | width in pixels |
| `h` | int32 | height in pixels |
| `f` | int32 | pixel format code (engine texture format, table below) |
| `r` | binary subtype 0 | raw pixel data exactly as held by the engine |

Pixel data rules:

- Rows run **bottom to top** (the first row in the data is the bottom row of the image as displayed); pixels within a
  row run left to right; rows are tightly packed. Verified (sample 1, through the level-0 hash of 4.5.2). Sizes need
  not be powers of two or square (sample 1).
- The data MAY contain a full mip chain after level 0 (level sizes `max(1, w >> i)` by `max(1, h >> i)` until 1x1,
  largest first, each level tightly packed with the same row rule). Cubik writes a chain (believed, 4.5.2) for images
  it copied in memory during the session: the two default textures of a new model and missing-file placeholders.
  Textures loaded directly from image files normally have no chain, but resized ones may. Cubik's loader only uses
  level 0 and creates the texture without mipmaps, so a chain disappears when Cubik re-saves a reopened file (6.3).
- For data shorter than level 0 the engine reports a "not enough data provided" error; whether Cubik's load then
  fails or continues with a blank image is UNVERIFIED.

| `f` | Format | Bytes per pixel / block | Byte order of one pixel | Expected in Cubik files |
|---|---|---|---|---|
| 1 | Alpha8 | 1 | A | rare |
| 2 | ARGB4444 | 2 (16-bit little-endian word) | A in bits 12-15, R 8-11, G 4-7, B 0-3 (UNVERIFIED) | rare |
| 3 | RGB24 | 3 | R, G, B | JPEG sources; the default textures and Cubik's other built-in pictures (4.5.2) |
| 4 | RGBA32 | 4 | R, G, B, A | primitives' generated textures; probably PNG sources |
| 5 | ARGB32 | 4 | A, R, G, B (alpha first: verified (sample 1)) | every texture after a paint stroke (4.5.3, verified (sample 1)); TGA sources, atlas images; possibly PNG sources (UNVERIFIED which of 4/5) |
| 7 | RGB565 | 2 (16-bit LE word) | R in bits 11-15, G 5-10, B 0-4 | rare |
| 10 | DXT1 (BC1) | 8 bytes per 4x4 block | block rows bottom to top | DDS sources |
| 12 | DXT5 (BC3) | 16 bytes per 4x4 block | block rows bottom to top | DDS sources |
| 13 | RGBA4444 | 2 (16-bit LE word) | R in bits 12-15, G 8-11, B 4-7, A 0-3 (UNVERIFIED) | rare |
| 14 | BGRA32 | 4 | B, G, R, A | rare |

Other engine format codes exist (for example 9, 15-21, 24-61); the port reader MUST NOT fail on them: it SHOULD
substitute a placeholder image, keep the raw bytes for re-saving, and warn.

DDS sources are loaded by skipping the 128-byte DDS header and handing the remaining bytes to the engine as level-0
blocks without flipping (only DXT1 and DXT5 are recognised). DXT data from DDS files is therefore believed to be stored,
and displayed by Cubik, upside down relative to the DDS convention (UNVERIFIED). The port MUST interpret the stored
blocks with the bottom-to-top rule above so that it displays what Cubik displays.

The port writer, for images it creates or edits, SHOULD write `f` = 4 (RGBA32) with exactly `w * h * 4` bytes and no
mip chain. The exception is the two default textures of a new model, written as RGB24 (4.5.2). For images it read
from a Cubik file and did not modify, it SHOULD write the original `f` and `r` unchanged, which keeps the payload
byte-identical (verified (sample 1)). After a paint stroke, Cubik writes `f` = 5 for every texture the list held
at that moment, until the texture is replaced (4.5.3).

#### 4.5.2 Default textures of a new model

A new model, made at application start-up or by File > New (menu item "New file", confirmed in the "New 3D Model"
dialog), gets a texture list with exactly two entries, added in this order to the emptied list: `particle`, then
`texture`. At the same moment Cubik registers its internal missing-texture image `error`, but only in the atlas, as
the fallback image for unknown labels: it is not a list entry and is never written.

Each default entry holds an in-memory copy of a built-in 16 x 16 picture of the application. The copy keeps the
source's size, pixel format and image name, receives the source's pixels, and is always created with mipmaps (the
copy routine asks for them whenever the source has at least one level, which every texture has). The engine then
rebuilds the lower levels from level 0.

Records written by the first save of a new model, provided no paint stroke happened before it (4.5.3):

| Key | First record | Second record | Status |
|---|---|---|---|
| `lb` | `particle` | `texture` | verified (code) |
| `fp` | null | null | verified (code) |
| `fn` | null | null | verified (code) |
| `fm` | "" | "" | verified (code) |
| `tx.n` | `e_particle` | `e_texture` | verified (code and shipped data) |
| `tx.w` x `tx.h` | 16 x 16 | 16 x 16 | verified (shipped data) |
| `tx.f` | 3 (RGB24) | 3 (RGB24) | verified (shipped data; the copy keeps the format) |
| `tx.r` | 1023 bytes | 1023 bytes | believed; UNVERIFIED (768 if the engine hands out level 0 only) |
| `cs` | null | null | verified (code) |

The 1023 bytes are five levels, largest first: 16 x 16 (768 bytes), 8 x 8 (192), 4 x 4 (48), 2 x 2 (12) and 1 x 1 (3),
each with rows bottom to top (4.5.1).

Sample 1 does not show this form. Its two records carry `tx.f` = 5, `tx.r` of `w * h * 4` bytes without a mip
chain (1024 bytes for the 16 x 16 `particle`) and `tx.n` = "", with `fp`, `fn` and `cs` null and `fm` as before. This
is the state that the atlas recut of 4.5.3 produces at the end of every paint stroke, and the sample's voxel model is
consistent with painting. The sample therefore neither confirms nor refutes the table above, which remains the
expected form of a save made before any paint stroke (12, item 16). Nothing in the file suggests another Cubik
version: the observed records follow from the same code. The file itself carries no version marker (7).

- Level 0 is the built-in picture. It has two colours only, white `#FFFFFF` and grey `#C1C1C1`: a 1-pixel grey frame
  around a grey letter P (`particle`) or T (`texture`). For recognising these records in samples, the SHA-256 of the
  768 level-0 bytes in file order (bottom row first) is:
  - `particle`: `6a1ff12896d4f4b8bf250a65968dc4a145382fbc06a8d152c970acb92b8c31b1`
  - `texture`: `9a901787373a1c23ce64b8fec993df167fee95c1a54c97c031b66b67d29753db`

  The pictures are not symmetric top to bottom, so the row order changes these values. Level 0 holds exactly these
  pixels only if the engine copies 32-bit colours into an RGB24 image, which is believed but UNVERIFIED for this
  engine version (12, item 16). Sample 1 supports it: the level-0 pixels of its recut `particle` record, reduced
  from ARGB32 to R, G, B in file order, have exactly the `particle` hash above. This verifies the row order, and,
  because the recut reads the pixels back from the atlas, it shows that the in-memory copy held the built-in pixels.
- Levels 1 to 4 (255 bytes) come from the engine's mipmap rebuild, believed to be a 2 x 2 average computed on the
  memory copy that the save reads. The exact values (filter and rounding) are UNVERIFIED. Request 1 of 11.2
  settles them, together with the length of `r`.
- Reopening the file changes both records (6.3): the images come back without mipmaps and the null `fp` becomes "".
  A Cubik save after a reopen therefore writes 768 bytes (level 0 only) and `fp` = "". The port reader MUST accept
  all three forms: `r` of 1023 or 768 bytes with `f` = 3, the recut form of 4.5.3 (`f` = 5, 1024 bytes, `n` = ""),
  and `fp` null or "". It MUST NOT recognise default textures by label, format or name: any label can hold any
  image (sample 1).

Related cases, which do not occur in a fresh save (the built-in pictures are all 16 x 16 RGB24):

| Case | Image of the entry | `tx.n` | Mip chain | Other keys |
|---|---|---|---|---|
| A texture added by path (imports, 9.5) whose file does not exist | copy of the built-in missing-texture picture (checkerboard) | `e_error` | yes, as above (1023 bytes believed) | `fp` null (the searched path is not kept), `fn` = last path segment of `fm`, `fm` as given |
| "Set as shortcut" to a label that does not exist | the built-in shortcut picture itself (a `#` sign), not a copy | `e_shortcut` | no (768 bytes) | `fm` = `#<label>`, `fp` null, `cs` null |
| "Set as shortcut" with an empty name | the built-in `texture` picture itself | `e_texture` | no (768 bytes) | `fm` "", `fp` null, `cs` null |
| "Set as shortcut" to an existing label | the target texture's own image, shared | the target's | as the target | `fm` = `#<label>`, `fp` null, `cs` null |

Rules for the port:

- The port MUST NOT ship or embed Cubik's built-in pictures (art policy, `ui_layout_theme.md` section 15.1). Its
  default textures use its own 16 x 16 pictures with the same roles.
- For its own new models the port writer SHOULD write the two default records as in the table above, including
  `tx.n` = `e_particle` / `e_texture` (an informational name), except `tx.r`: RGB24 without a mip chain, exactly 768
  bytes of the port's own pixels. This is the form in which Cubik itself writes these images once the file has been
  reopened, so the image bytes stay identical when Cubik re-saves the port's file (only `fp` changes, 6.3). It is
  the exception to the RGBA32 writer rule of 4.5.1.
- The port MUST NOT replace default textures found in a file by its own pictures. Like any other record they are kept
  as read (8.3), including their mip tail and the null or "" form of `fp`.
- Whether the optional local skin pack of the art policy may also supply these two pictures is an open decision
  (12, item 17), because it would write Cubik's pictures into the user's own files.

#### 4.5.3 Atlas recut after paint strokes

The trigger is verified from the code; the records it produces are verified (sample 1), which matches this state in
every key, though the file cannot show which stroke caused it. While a paint tool is active, releasing the mouse button that started a stroke in a
view (left button: primary colour, right button: secondary colour) ends the stroke. At that moment Cubik replaces the
image of **every** texture in the list, whatever the stroke painted (texture, voxels, or nothing), with a copy cut
out of the packed atlas:

- the new image has the atlas pixel format, ARGB32 (`f` = 5), the size of the entry's atlas rectangle, no mipmaps and
  an empty name (`n` = "");
- assigning it in memory sets `fp` and `cs` to null (4.5); `fn` then falls back to the stored name (null for the
  default textures), and `fm` keeps its value;
- the pixels are those of the atlas, so the colours of 8-bit sources are preserved (sample 1: the `particle` picture
  survives exactly, 4.5.2).

Consequences for files written by Cubik: after any paint stroke all texture records have the form above (the two
default records included: `f` = 5, `r` = 1024 bytes, `n` = ""), the source paths are gone, and the "Reload textures
from HDD" check is disabled for these textures because `cs` is null. Textures added after the last stroke keep their
own form until the next stroke. UNVERIFIED: the behaviour when every texture is block-compressed (the atlas is then
believed to be compressed too), and whether textures can shrink when the atlas exceeds its maximum size.

Rules for the port:

- The reader MUST accept these records like any other (4.5.1); the recut form occurs in real files (sample 1).
- The writer keeps retained records unchanged (8.3). A port that wants Cubik's payload after a paint stroke, for
  example in a strict comparison mode, MUST apply the same transformation to every list texture at the end of each
  stroke: level-0 pixels converted to ARGB32 without a mip chain, `n` = "", `fp` and `cs` null, `fn` and `fm` kept.

### 4.6 MeshRecord (items of `me`)

| Key | BSON | Meaning | Notes |
|---|---|---|---|
| `p` | doc Vector3 | local position | |
| `s` | doc Vector3 | local scale | applied as-is on load (no clamping) |
| `r` | doc Vector3 | rotation, Euler degrees | quantised on load, see 6.3 |
| `o` | doc Vector3 | pivot | |
| `is` | boolean | selected | restored on load |
| `ih` | boolean | hidden | |
| `il` | boolean | locked | |
| `l` | string | label | must not be null for `tp` = 0 |
| `tp` | int32 | mesh kind (enumeration MeshKind) | unknown values load as a cube primitive |
| `tx` | doc RawTexture /null | main texture | primitives get a generated 128x128 RGBA32 texture filled with the primary colour |
| `mb` | doc MeshBounds | bounds recorded at import time | must not be null for `tp` = 0 |
| `me` | doc MeshData /null | geometry | written only when `tp` = 0 (custom mesh); null otherwise |
| `sm` | arr SubmeshRecord /null | sub-meshes | written only when `tp` = 0 and the mesh has at least one sub-mesh |

For primitive kinds (1-10) the geometry is not stored; Cubik recreates the primitive from its built-in prefab and
applies the stored transform, label, flags, texture and bounds. **UNVERIFIED:** whether the `sprite` kind (10), whose
geometry comes from a PNG import, survives a save and reload unchanged (its geometry is not stored).

#### 4.6.1 MeshData (`me.me`)

| Key | BSON | Meaning |
|---|---|---|
| `v` | arr Vector3 | vertex positions (mesh-local) |
| `n` | arr Vector3 /null | vertex normals; Cubik writes an empty array when the mesh has none |
| `t` | arr int32 | triangle vertex indices for the whole mesh (all sub-meshes concatenated, believed; UNVERIFIED) |
| `u` | arr Vector2 /null | texture coordinates; empty array when none |

On load, a null `n` makes Cubik recompute normals. An empty or mismatched `n` or `u` is passed to the engine as-is;
the engine is expected to reject it with a logged error and keep no normals or UVs (UNVERIFIED). The port writer SHOULD
therefore write `n` and `u` with one entry per vertex, or null for `n`. The engine of Cubik limits a mesh to 65 000
vertices (UNVERIFIED exact limit), so the port writer MUST NOT emit larger meshes for Cubik.

#### 4.6.2 MeshBounds (`me.mb`)

| Key | BSON | Meaning |
|---|---|---|
| `c` | doc Vector3 | centre |
| `s` | doc Vector3 | size (full extent) |

For primitives the bounds are whatever the primitive object holds; they are not computed at creation (believed to be
zero, UNVERIFIED).

#### 4.6.3 SubmeshRecord (items of `me.sm`)

| Key | BSON | Meaning |
|---|---|---|
| `t` | arr int32 | triangle indices of this sub-mesh |
| `c` | int32 | material colour, packed ARGB (section 4.10) |
| `tx` | doc RawTexture /null | material texture |

On load the sub-mesh count is set to the array length, each sub-mesh gets its triangles and a new material with the
decoded colour and the texture.

### 4.7 VoxelData (`vx`)

| Key | BSON | Meaning |
|---|---|---|
| `w` | arr VoxelWorldRecord | voxel worlds; Cubik writes exactly one and reads only the first |

VoxelWorldRecord:

| Key | BSON | Meaning |
|---|---|---|
| `r` | int32 | resolution R: voxels per axis across the 16-unit model zone |
| `c` | arr VoxelChunkRecord | non-empty chunks |

VoxelChunkRecord:

| Key | BSON | Meaning |
|---|---|---|
| `cx` | int32 | chunk index along X |
| `cy` | int32 | chunk index along Y |
| `cz` | int32 | chunk index along Z |
| `v` | arr VoxelRecord | voxels of the chunk (may be empty) |

VoxelRecord:

| Key | BSON | Meaning |
|---|---|---|
| `x` | int32 | local X inside the chunk, 0..15 |
| `y` | int32 | local Y, 0..15 |
| `z` | int32 | local Z, 0..15 |
| `c` | int32 | packed colour (section 4.10); never 0 in files written by Cubik |

Grid geometry:

- The voxel grid has N = 3R cells per axis; the model zone occupies the middle third. Cell size is 16 / R units.
- Chunks are 16 x 16 x 16 cells; there are C = ceil(3R / 16) chunks per axis. Valid chunk indices are 0..C-1.
- Global cell index: `(gi, gj, gk) = (16*cx + x, 16*cy + y, 16*cz + z)`.
- Cell (gi, gj, gk) covers X in [gi*cell - 16, (gi+1)*cell - 16], Y in [gj*cell - 16, (gj+1)*cell - 16] and
  Z in [16 - (gk+1)*cell, 16 - gk*cell]. Z decreases as gk grows.
- The editor's setup dialog allows R = 2..128; upscaling can reach higher values (up to 254). Readers MUST accept any
  R >= 1 within a configurable memory limit.

Write order used by Cubik (needed for byte-identical output): chunks are scanned with cx outermost, then cy, then
cz; a chunk is written if it has been allocated and has not been cleared by "Clear voxels" since (a chunk whose
voxels were all erased one by one is still written, with an empty `v`). Inside a chunk, voxels are scanned x
outermost, then y, then z, and zero cells are skipped.

On load (section 6.3): records with an empty `v` are skipped; chunk indices outside 0..C-1 or local coordinates
outside 0..15 make Cubik's load fail; the voxel palette is rebuilt from the colours in file order. The port reader
SHOULD skip invalid records with a warning instead of failing.

Verified (sample 1, R = 128, so C = 24): one world; key order of all four record types; chunks in cx, cy, cz order
without duplicates and with indices within 0..C-1; voxels in x, y, z order inside each chunk with local coordinates
0..15; no zero colours. About a quarter of the voxels lie outside the model zone, so stored voxels can occupy the
whole 3R grid. Not covered: chunks with an empty `v`, resolutions other than 128.

### 4.8 ModelDisplay (`md`)

| Key | BSON | Minecraft display slot |
|---|---|---|
| `tr` | doc DisplaySlot | `thirdperson_righthand` |
| `tl` | doc DisplaySlot | `thirdperson_lefthand` |
| `fr` | doc DisplaySlot | `firstperson_righthand` |
| `fl` | doc DisplaySlot | `firstperson_lefthand` |
| `u` | doc DisplaySlot | `gui` |
| `h` | doc DisplaySlot | `head` |
| `g` | doc DisplaySlot | `ground` |
| `i` | doc DisplaySlot | `fixed` (item frame) |

None of the slots may be null (Cubik copies each slot without checks).

DisplaySlot:

| Key | BSON | Meaning | Fresh value |
|---|---|---|---|
| `n` | string | name of the preset the slot was set from | `Default` |
| `c` | boolean | the preset can be removed (user preset) | true |
| `e` | boolean | slot enabled (exported) | false |
| `r` | doc Vector3 | rotation in degrees (Minecraft axis order) | (0, 0, 0) |
| `t` | doc Vector3 | translation (1/16 block units) | (0, 0, 0) |
| `a` | double | angle of the preview helper | **always 0** (not copied into the saved record) |
| `s` | doc Vector3 | scale | (1, 1, 1); every component equal to 0 is written as f32 0.0001 |

On load `n`, `c`, `e`, `r`, `t` and `s` are copied into the model; `a` is ignored.

Verified (sample 1): slot and key order, `a` = 0 in every slot, and the fresh values in the slots that had not been
edited. One slot held a non-zero rotation and translation while `e` was false and `n` was still `Default`, so the
values of a disabled slot are saved as they are.

### 4.9 Enumerations

ModelKind (`mp.mc.t`): 0 none, 1 block, 2 item, 3 entity, 4 optifine (Optifine JPM), 5 map.

StaxelKind (`mp.st.t`): 0 none, 1 tile, 2 item, 3 accessory, 4 other.

ElementType (`el[].t`): 0 cube, 1 plane X (zero thickness along X; only west and east faces enabled on creation),
2 plane Y (down and up), 3 plane Z (north and south).

MeshKind (`me[].tp`): 0 custom mesh, 1 cube, 2 sphere, 3 cylinder, 4 cone, 5 pyramid, 6 slope, 7 corner-in,
8 corner-out, 9 torus, 10 sprite.

FaceType (`f.*.c`), bit flags: 0 none, 1 down, 2 up, 4 north, 8 south, 16 west, 32 east, 63 any/all. A single
direction is expected for Minecraft cull faces.

Pixel formats (`f` of RawTexture): see 4.5.1.

### 4.10 Packed colours

All packed colours are i32 values laid out as 0xAARRGGBB (alpha in bits 24-31, blue in bits 0-7). Values with
alpha 0x80 or more are negative as signed integers.

| Where | Packing used by Cubik when saving | Unpacking used by Cubik when loading |
|---|---|---|
| element colour `el[].c` | channels converted from 0..1 floats to bytes by clamping to [0,1], multiplying by 255 and truncating; if the alpha byte is 0 the value is 0, otherwise alpha is forced to 255 | RGB from the low 24 bits, alpha always 255 (so 0 loads as opaque black) |
| sub-mesh colour `me[].sm[].c` | each channel multiplied by 255, truncated to an integer and kept modulo 256 (no clamping); if the alpha integer is 0 the value is 0, otherwise the alpha byte is kept as computed | all four bytes, including alpha |
| voxel colour `vx...c` | written as stored (editor colours always have alpha 255; verified (sample 1): every colour has alpha 0xFF and is therefore negative, opaque black being -16777216) | stored as-is; the alpha byte is ignored for display, 0 means empty |

### 4.11 Coordinate conventions (summary)

- Units are Minecraft pixels; Unity axes; left-handed; Y up. Minecraft model coordinates relate to stored coordinates
  by negating Z: a Minecraft element `from`/`to` maps to `p = (from.x, from.y, -from.z)` and
  `s = to - from` (zero components replaced by 0.001).
- An unrotated element spans X in [p.x, p.x + s.x], Y in [p.y, p.y + s.y] and Z in [p.z - s.z, p.z].
- Rotations are Euler angles with the convention of 4.3.2. Element and mesh rotations are applied about `p` by the
  engine transform; rotation about the pivot `o` is already folded into `p`.
- Geometry details beyond what is needed to read and write the file belong to the element and voxel specifications.

---

## 5. Save semantics

### 5.1 Triggers and paths

- **Save** (Ctrl+S): if the document has a save path, save there; otherwise behave like Save As.
- **Save As**: file dialog titled "Save Cubik model as...". The initial folder is the "load/save" folder setting
  (Desktop when empty) and the default file name is the model name with extension `cubik`; when the document already
  has a path in an existing folder, that folder, file name and extension are proposed instead (after opening a `.cub`
  file this proposes `.cub`, which then fails as described in section 1).
- Before taking the snapshot, a normal save sets the document path `mp.sp` to the target path, so the written `sp`
  equals the file's own location.
- **Automatic backup**: starts after application start-up and repeats every max(300 s, backup-interval-minutes * 60 s)
  (default setting 10 minutes). It writes `<user data folder>/backup/<Weekday>_backup.cubik`, where `<Weekday>` is the
  English day name (`Monday` ... `Sunday`), so seven files rotate weekly. On Windows the user data folder is
  `%USERPROFILE%\AppData\LocalLow\BDcraft\Cubik`. Backups run unconditionally (no dirty check), do not change the
  document path, and report "Backup: <path>" in the status line.

### 5.2 Snapshot

The save first builds an in-memory snapshot, one section per frame, with these status texts: "Saving properties...",
"Saving elements...", "Saving textures...", "Saving voxels...", "Saving meshes...", "Saving model display...",
"Saving...". Then it serialises and writes. Snapshot content, in order:

1. `mp`: a copy of the model properties.
2. `vp`: the four view cameras are captured into the live view state, then the view state is copied; the copy keeps
   the cameras, `gm`, `gk` and `gd` but not `gu` or `gl` (written with their defaults, 5.3).
3. `bp`: the blueprint; `el`: every element of the element list, in list order.
4. `tx`: every entry of the texture list, from the first entry, in list order.
5. `vx`: only if the voxel grid has been set up; otherwise null.
6. `me`: every mesh, in list order.
7. `md`: a copy of the model display slots (without the helper angle).

### 5.3 Save-time normalisations

To reproduce Cubik's payload byte for byte, the port writer MUST apply these rules (most of them follow from how
values are stored in the editor):

| Field | Rule |
|---|---|
| `mp.g.d`, `mp.g.n`, `mp.mc.p` | never null; defaults of 4.2; no ASCII double quotes |
| `vp.gu` | 0.5 |
| `vp.gl` | false |
| `vp.ctl`..`vp.cbr` | captured camera states; quaternion with consistent `eulerAngles` |
| `bp.o`, `bp.c`, `bp.f`, `bp.rs`, `bp.suv`, `bp.suvm`, `bp.sh`, `bp.in`, `bp.t` | blueprint world position, 0, null, false x5, 0 |
| `el[].c` | packing of 4.10 |
| `el[].s` | as held by the editor (components >= 0.001 for editor-produced data) |
| `el[].r`, `me[].r` | already quantised by the editor (6.3) |
| `el[].f.*.uv.r` | 0, 90, 180 or 270 |
| `el[].f.*.uv.tl`, `.br` | folded values of 4.4.3 |
| `tx[].fp`, `fn`, `fm`, `cs` | derivations of 4.5; `fp` and `cs` null for images assigned in memory during the session |
| `tx[].tx` | as held in memory: level 0 only for images restored from a file (6.3); default textures of a new model as in 4.5.2; after a paint stroke every texture in the recut form of 4.5.3 |
| `me[].me`, `me[].sm` | null unless `tp` = 0 |
| `md.*.a` | 0 |
| `md.*.s` | zero components as f32 0.0001 |
| `vx` | null when no voxel grid; one world; scan order of 4.7 |
| all floats | exact f32 to f64 widening |

Sample 1 satisfies every rule of this table that applies to its content (`vp.gu`, `vp.gl`, the blueprint keys,
`md.*.a`, `vx`, the texture forms and the float widening). Verified (sample 1).

### 5.4 What a `.cubik` file does not contain

- Minecraft item overrides (predicate cases).
- `gu` (UV grid step) and `gl` (large grid) as live values, "snap to grid", grid and guide visibility.
- The display helper angle (`a` is always 0).
- Selection order, the primary selection, element and mesh display ids (renumbered in file order on load), undo
  history.
- The voxel palette (rebuilt from voxel colours), voxel setup options other than the resolution.
- The model box when it was only set by an import and never dragged (`mb` stays null).
- The packed texture atlas (rebuilt on load) and texture file sizes.
- Background images of the views, panel layout and application settings (stored elsewhere).

---

## 6. Load semantics

### 6.1 Open versus merge

- **Open** (File > Open, recent files, drag and drop, command line): the scene is reset first. The texture list is
  emptied (the atlas keeps only the internal `error` image), and the model (properties, model box, origin, elements,
  meshes, voxels), selection, voxelisation state, display preview, panel layout, user-interface state, paint state and
  undo history are reset.
- **Merge** (File > Merge): no reset. The loaded document is applied on top of the current scene with the same steps,
  which means model properties, view properties and display settings are overwritten, the texture list is
  **replaced** whenever the file has at least one texture record, the voxel model is replaced (or deleted when the file
  has none), and elements and meshes are appended.
- For files larger than 307 200 bytes Cubik first shows a "Loading..." message ("<application> is loading "<file>"
  (<size>)", blank line, "Please wait...").
- After the importer returns, Cubik shows "Loaded CUBIK: <path>" in the status line, makes the opened file the
  document path and updates the window title. It does this **even when the importer reported an error** (the import
  error dialog with its "Abort" button is shown in that case, and the "Loading..." message is not closed by the
  loader). The port MUST NOT adopt the path of a file that failed to load.

### 6.2 Apply order

The selection is cleared, then the sections are applied in this order: `mp`, `vp`, `bp` and `el`, `tx`, `vx`, `me`,
`md`. Textures are therefore applied after the elements that reference them; elements are refreshed when their
textures arrive.

### 6.3 Per-section rules

| Section | Behaviour in Cubik |
|---|---|
| `mp` | Every group is copied field by field into the live model, including `sp` and `ep` (the open step then replaces `sp` with the opened path). The model box from `mb` is applied when not null. |
| `vp` | Cameras that are not null are applied (world position, rotation, projection). `gm`, `gk`, `gd` and the grid snap flag (always on, see 4.3) are copied; `gu` and `gl` are ignored. |
| `bp` | Position, size, rotation, pivot (no visible effect), hidden, locked and label are applied to the blueprint. |
| `el` | For each record an element of type `t` is created with colour `c` (alpha forced to 255), then position, size, rotation, pivot, hidden, locked, label, faces and flags are applied, then the element is refreshed. Records with `is` true become selected. |
| rotations (`el`, `me`, `bp`) | Each component is normalised with `(angle + 360) mod 360` (C remainder semantics, so values below -360 stay negative) and then quantised: to multiples of 22.5 degrees when `mp.g.rer` is true and the object is an element or the blueprint; to multiples of 5 degrees otherwise. Meshes always use 5 degrees, whatever `rer` says: imported custom meshes are created without the restriction, and the built-in templates of all ten primitive kinds carry it switched off. Rounding is half-to-even. A rotation equal to the object's current rotation (for a new object: (0,0,0)) is not normalised at all. |
| sizes (`el`, `bp`) | Element sizes are applied as stored: a cube element keeps even components below 0.001, because the creation step assigns the stored size directly. Plane elements first get their zero-thickness axis set to 0.001 by the creation step; the blueprint size goes through the editor setter, which raises components below 0.001 to 0.001. Files written by Cubik already respect the minimum. |
| `tx` | Skipped when the array is missing or empty. Otherwise the whole texture list is cleared and each record is added in order (a repeated label replaces the earlier image; a label `error` is special, see 4.5). Each image is recreated without mipmaps from the start of `r`, so a mip tail is ignored and a later Cubik save writes level 0 only; a null `fp` becomes "". Both rules change the default textures of a file saved from a new model (4.5.2). |
| `vx` | When missing, null, or with no world: the voxel grid is reset if it exists. Otherwise, if no grid exists or its resolution differs from `w[0].r`, a new grid of that resolution is created (voxel ambient occlusion taken from `mp.obj.ao`, active box from the model box); then all voxels are cleared and the chunks of `w[0]` are written. |
| `me` | For `tp` = 0 a mesh object is rebuilt from `me`, `mb` and `l`; for other kinds the primitive is created. Then transform (scale unclamped), flags, label, kind, texture, bounds and sub-meshes are applied. Records with `is` true become selected. |
| `md` | Each slot copies name, removable flag, enabled flag, rotation, translation and scale. |

### 6.4 Failure behaviour

Any exception during the apply phase aborts the remaining steps but keeps everything applied so far (there is no
rollback) and shows the import error dialog. The port MUST instead parse and validate the complete document before
changing the open model, and MUST apply it atomically (all or nothing). It SHOULD reproduce Cubik's error texts.

---

## 7. Version handling and tolerance

- `v` is always written as 1 and never read. There is no other version marker. Verified (sample 1: `v` = 1, and
  nothing else in the file names the producing version). The port MUST NOT reject files with
  other `v` values (0..255) and SHOULD log them.
- Missing keys and missing sections are normal (older or hand-made files) and take the "If absent" values.
- Unknown keys are ignored by Cubik at every level. This makes additive extensions safe for Cubik (see 8.4).
- Older Cubik releases may have written variants of this format (fewer keys, other structures, or plain JSON inside
  the archive); none were available for this study. Cubik 2.9.480 only accepts the ZIP + BSON form. The port MUST
  accept everything described here and SHOULD accept a JSON text payload in the `cubik` entry as an extension
  (UNVERIFIED that such files exist).

---

## 8. Requirements for the Rust implementation

### 8.1 Reader

1. Read the container (2.4), then decode the BSON payload completely before touching the open document.
2. Accept the type conversions of 3.6, ignore unknown keys, use last-wins for duplicate keys, and apply quaternion
   members in document order (3.4).
3. Apply the read-side normalisations of Cubik so that the in-app state matches Cubik's: quote replacement in
   `mp.g.n`, `mp.g.d`, `mp.mc.p`; UV rotation normalisation and corner clamping; the size handling and rotation
   quantisation of 6.3; the grid snap reset; ignoring `gu`, `gl` and `md.*.a`.
4. Treat structures that make Cubik fail (nulls in value-type keys, a null `f` in an element, invalid voxel indices,
   short texture data) as recoverable errors with a clear message, either rejecting the file or skipping the record;
   never panic. Enforce limits on document size, array lengths, texture dimensions (`w * h` and `r` length) and voxel
   resolution before allocating.
5. Keep, per record, enough original information to re-save untouched data unchanged (8.3).

### 8.2 Writer (Cubik-compatible profile)

1. Produce exactly the key order, element types and values of section 4 with the normalisations of 5.3.
2. Never write null where the "BSON" column does not say "/null".
3. Floats: f32 widened to f64. Integers and enumerations: int32. `v` = 1.
4. Strings: valid UTF-8 (Rust strings always are).
5. Container: 2.4.
6. Recommended output check in debug builds: decode the produced payload with the port's own reader and compare.

### 8.3 Lossless round trip

To keep files byte-identical when the user changes nothing, the port SHOULD retain for each loaded section the
values exactly as read (including `cs`, the null or "" form of `fp`, raw texture bytes with mip chains, and the camera
quaternion together with its Euler copy) and write them back unchanged unless the corresponding object was modified. Unknown keys found in a
file SHOULD be preserved at their position and re-emitted.

Verified (sample 1): decoding into the records of section 4, keeping floats as f32 values, and writing them back with
the rules of section 3 reproduces the payload byte for byte. Keeping the f32 values suffices; the original double bit
patterns need not be stored separately, because every double is an exact widening.

### 8.4 Port extensions

Port-only data (for example Minecraft item overrides, which Cubik does not store) MAY be written either as extra keys
in the BSON documents or as additional ZIP entries. Both are ignored by Cubik when it reads the file and both are
lost when Cubik saves it. Recommendation: one additional entry `cuboid.json` (UTF-8 JSON with its own version
number) and no extra BSON keys, so the `cubik` payload stays byte-comparable with Cubik's output. The user SHOULD be
able to disable extensions for maximum compatibility.

### 8.5 Implementation notes

- Payload sizes: each voxel costs about 37-40 bytes of BSON (37.8 on average in sample 1) and each vertex about 45
  bytes, so payloads of tens of megabytes are realistic. Cubik's level-1 deflate took sample 1 from 953 962 to
  173 757 bytes. Decode with a zero-copy, streaming approach (iterate raw elements), and encode in one pass by
  reserving the 4-byte size field of every document and patching it when the document closes.
- Crates: the `zip` crate with its deflate feature for the container; the `bson` crate's raw document API for
  reading and appending typed elements in a fixed order, or a small purpose-built encoder if profiling shows the
  copying of nested raw documents to be a bottleneck; `md-5` for the texture signature; `encoding_rs` for the code-page
  decoding of `cs`. Check current versions and APIs when implementing.
- The codec MUST be independent of the scene representation: decode into plain record structs mirroring section 4,
  then map them to the editor model; encode from the same structs.

---

## 9. Legacy `.cub` ("Cubik model alpha")

### 9.1 Identification

Extension `.cub`, open only. The file is UTF-8 text (with optional byte order mark) whose characters are a Minecraft
model JSON extended with Cubik keys, obfuscated with a repeating XOR. No writer exists in Cubik 2.9.480; the port
MUST NOT offer one unless a later decision requires it.

### 9.2 Text decoding

1. Read the whole file and decode it as text the way a .NET text reader with byte-order-mark detection does:
   `EF BB BF` means UTF-8 (BOM skipped), `FF FE` UTF-16 little-endian, `FE FF` UTF-16 big-endian; otherwise UTF-8.
   Invalid UTF-8 sequences become U+FFFD. (.NET also recognises UTF-32 byte order marks; supporting them is optional.)
2. Work on the resulting sequence of UTF-16 code units (characters outside the Basic Multilingual Plane count as two
   units).

### 9.3 De-obfuscation

With `K1 = "oBeeware"` (8 characters) and `K2 = "BDcraft Cubik - Encryption System Key"` (37 characters), the decoded
code unit at index i is:

```
plain[i] = file[i] XOR K1[i mod 8] XOR K2[i mod 37]
```

The operation is its own inverse; the combined key stream repeats every 296 units and its first 16 values are
`2D 06 06 17 16 07 06 45 2C 37 07 0C 1C 41 5F 45`. These are fixed public constants of the file format, used for
obfuscation only; they protect nothing.

Test vector: the plain text `{"name":"Box","type":1}` corresponds to the file bytes
`56 24 68 76 7B 62 24 7F 0E 75 68 74 3E 6D 7D 31 53 5C 63 35 34 20 7B`.

Because both keys are ASCII, an ASCII character always encodes to an ASCII character, and the file stays valid
UTF-8 whenever the plain text was.

### 9.4 JSON dialect

Cubik parses the decoded text with a permissive parser whose behaviour differs from standard JSON. To read every
file Cubik can read, the port MUST implement these rules instead of using a strict JSON parser:

- The top level must be an object; whitespace (Unicode white space) between tokens is skipped.
- Strings run from a double quote to the next double quote that is not immediately preceded by a backslash. **No
  escape sequence is decoded except `\uXXXX`** (four hexadecimal digits, replaced by that code unit); `\"`, `\\`, `\/`,
  `\n` and the others are kept literally, backslash included. A string that ends with an escaped backslash therefore
  does not terminate where standard JSON would.
- A number token starts with a digit or `-` and runs up to (not including) the next `,`, `]` or `}`; the token,
  including any trailing white space, is parsed as a double with the invariant culture (exponents allowed). Integer
  readers truncate toward zero; float readers convert to f32.
- `true`, `false` and `null` are recognised by their first letter and checked for the full word.
- Duplicate keys: the last value wins.
- Typed lookups used by the importer: a missing key yields the documented default. A value of another type reads as
  0 (numbers), false (booleans) or null (strings); a null string aborts the import wherever the importer uses it (for
  example a numeric `name` or `texture`). Vectors require arrays of at least
  3 (Vector3) or 4 (Vector4, colour) numbers, otherwise the default is used. Enumeration names (cull faces) are
  matched case-sensitively, numeric text and comma-separated flag names are also accepted, and anything else yields
  the default.
- The `\uXXXX` replacement is applied to the raw string content after the string has been delimited, so an escaped
  backslash followed by `u` and four hexadecimal digits is also replaced.
- Any syntax error aborts the import with the dialog "Invalid CUB" / ""<name>" is not a valid CUB file:" followed
  by a blank line and the parser message, which is always "Invalid data found" because the parser runs in its silent
  mode. `CUB` is the upper-cased format key `cub` (the Minecraft JSON importer, which shares this code, shows `JSON`
  instead), and `<name>` is the file name without extension.

### 9.5 Import pipeline and keys

The import uses the scene-reset rules of 6.1 for Open. After a successful parse:

1. If the root has none of the keys `elements`, `textures`, `parent`: error "Invalid CUB Model" (""<name>" is not a
   valid Model", blank line, "Please check the file or contact the author.").
2. If `parent` is present: `item/generated` aborts with "Generated Model"; a model with a parent but no elements, or
   with a parent other than `block/block` and `item/handheld`, asks for confirmation ("Inherited Model",
   Import/Abort); `block/block` and `item/handheld` show an informational "Inherited Model" (OK/Abort). These dialogs
   are shared with Minecraft JSON import.
3. The model is then built in this order. Keys not listed are ignored (including the standard Minecraft `textures`
   object, which `.cub` files do not use).

Root keys:

| Key | Type | Default | Effect |
|---|---|---|---|
| `parent` | string | - | `mp.mc.p` = value; kind = item for exactly `item/handheld`, else block; `uvds` = true. Overridden by `type` and `usevanilladisplaysettings` below. |
| (file name) | - | - | the model name is first set to the file name without extension |
| `__comment` | string | - | description `mp.g.d` |
| `ambientocclusion` | bool | - | `mp.mc.ao` |
| `gridstep` | number | 1.0 | displayed grid step (`gd`) |
| `gridmousestep` | number | 1.0 | mouse grid step (`gm`) |
| `gridkeystep` | number | 1.0 | keyboard grid step (`gk`) |
| `snaptogrid` | bool | true | grid snapping |
| `modelOrigin` | [x, y, z] | (0,0,0) | model origin marker (and `mp.g.o`) |
| `name` | string | "" | model name `mp.g.n`; **replaces the file name, so a missing key gives an empty name** |
| `type` | int | 1 | `mp.mc.t` (ModelKind) |
| - | - | - | `mp.mc.etr` is forced to 16 |
| `usevanilladisplaysettings` | bool | true | `mp.mc.uvds` |
| `usecustomdisplaysettings` | bool | false | `mp.mc.uds`, but **overwritten later** by the presence of `display` |
| `useoverridessettings` | bool | false | `mp.mc.uos`, but **overwritten later** by the presence of `overrides` |
| `preventbleedingtextures` | bool | false | `mp.g.pbt` |
| `excludehiddenelements` | bool | false | `mp.g.ehe` |
| `minifyexport` | bool | false | `mp.g.me` |
| `modelBox` | object | - | `xmin` (-16), `ymin` (-16), `zmin` (-32), `xmax` (32), `ymax` (32), `zmax` (16) set the model box directly (not `mp.g.mb`, so a later `.cubik` save writes `mb` = null); `showHandles` (true) and `showBox` (false) set the box gizmo visibility; `isActive` (true) enables the box |
| `textures_path` | object | - | texture table, see below |
| `elements` | array | - | elements, see below |
| `display` | object | - | display slots, see below |
| `overrides` | array | - | item overrides, see below |
| `voxel` | object | - | voxel model, see below |

Textures (`textures_path`, an object mapping a texture key to a path string):

- A path starting with `:` names a Minecraft texture id; the id (text after the colon) is looked up as
  `<...>/minecraft/textures/<id>.png` relative to a `minecraft` folder found in the `.cub` file's path, then
  `textures/<id>.png` and `<id>.png` next to the file.
- Otherwise the file is searched in this order: `<cub folder>/<file name of the path>`, `<cub folder>/<key>.png`,
  `<cub folder>/textures/<file name>`, the path itself. If none exists the texture gets Cubik's missing-texture
  picture and no path (4.5.2).
- Each key becomes a texture with label = key and Minecraft id = the id derived from the path. Face texture references
  `#key` resolve through this table.
- When `textures_path` is absent Cubik shows "No texture found" ("This 3D model doesn't have any texture.", "You can add
  textures manually with the Material Manager."). The "<N> texture(s) not found" message exists but cannot trigger,
  because missing files are replaced by an empty path before the check.

Elements (each item of `elements`):

| Key | Type | Effect |
|---|---|---|
| `from`, `to` | [x, y, z] | `p = (from.x, from.y, -from.z)`, `s = to - from` with zero components replaced by 0.001 |
| `rotation` | object | `origin` [x, y, z] (Z negated) becomes the pivot (without `rotation` the pivot is the position); the element rotation is the Euler form (4.3.2) of a rotation by -`angle` degrees about +X for `axis` `x`, about +Y for `y`, or about -Z for `z` (any other axis gives no rotation); `rescale` (bool) sets `rs` |
| `__comment` | string | label (default `Element`), but see `label` |
| `shade` | bool (true) | `sh` |
| `faces` | object | the automatic UV unwrap is computed first; then each of `down`, `up`, `north`, `south`, `west`, `east` is read; a missing face is disabled |
| `origin` | [x, y, z] | pivot, only when the element has no rotation |
| `label` | string ("") | **replaces the label in all cases** (missing key gives an empty label) |
| `color` | [r, g, b, a] in 0..1 | element colour (missing gives transparent black, saved later as `c` = 0) |
| `uvsnap` | bool (false) | `suv` |
| `uvsnapmirrored` | bool (false) | `suvm` |

Faces (each face object): `uv` [x1, y1, x2, y2] in pixels, converted with the formula of 4.4.3 but stored without the
[-1, 2] clamp, so out-of-range values reach the save-time folding unclamped; `texture` (the first character, normally
`#`, is removed, and the rest is mapped through the texture table; a missing `texture` aborts the import);
`cullface` (FaceType name, default none); `rotation` (int, default 0, normalised); `tintindex` (int, default -1). A face
texture label that does not exist yet creates a default texture entry (built like the defaults of 4.5.2: a copy of the
`particle` picture for the label `particle`, of the `texture` picture for any other label).

Display (`display`): if present, `uds` = true and each of the slots `thirdperson_righthand`, `thirdperson_lefthand`,
`firstperson_righthand`, `firstperson_lefthand`, `gui`, `head`, `ground`, `fixed` is enabled with `rotation`
(default 0,0,0), `translation` (0,0,0) and `scale` (1,1,1) when present, or disabled and reset when absent. If
`display` is absent, `uds` = false and all slots are reset.

Overrides (`overrides`): if present, `uos` = true and every item with a `predicate` object adds a case with `model` and
the optional predicates `angle`, `blocking`, `broken`, `cast`, `damage`, `damaged`, `lefthanded`, `pull`, `pulling`,
`time`. Otherwise `uos` = false and the list is cleared. These cases are not stored in `.cubik` files (5.4).

Voxels (`voxel`): `resolution` (int, default 16) creates a new grid; each item of `voxels` is an array
`[x, y, z, colour]` of numbers where x, y, z are **global** cell indices (0..3R-1; values outside the grid are ignored)
and colour is the packed colour of 4.10. A missing `voxels` array aborts the import. Colours stored as positive numbers
above 2147483647 convert to an undefined value in Cubik (UNVERIFIED which form alpha-era files use).

Finally the selection is cleared and the last texture of the list is selected.

### 9.6 Quirks to keep or decide on

- The model name and element labels are empty unless the `name` / `label` keys exist (see tables).
- `uds` and `uos` follow the presence of `display` / `overrides`, not their explicit flags.
- Grid steps default to 1.0 here, unlike the native defaults (0.5 / 0.125 / 1.0).
- The model box from `modelBox` is not recorded in the model properties and is lost on the next `.cubik` save.
- When opening (not merging), Cubik resets the scene **before** parsing, so a `.cub` file that fails to parse still
  clears the open model. Errors after parsing (for example a face without `texture`) leave a partially imported model.
  The port MUST import atomically and leave the open model untouched on failure.
- The parser's own text writer (which an alpha-era writer may have used) emits strings without any escaping and
  numbers in the current culture; files written on a system with a decimal comma would contain numbers such as
  `0,5`, which this dialect cannot read back (UNVERIFIED whether such files exist).

---

## 10. Test plan

### 10.1 Codec unit tests

- BSON primitives: the vectors of 3.5 (empty document, Vector3, Vector2 with f32 widening, face record, voxel record);
  sizes of 3.4; exact f32 to f64 to f32 round trips for random values including subnormals, -0.0, infinities and NaN.
- Each record type of section 4: encode a populated instance and compare with a hand-computed byte string; decode
  with every lenient form of 3.6.
- Container: archives with stored and deflated entries, with data descriptors, ZIP64 records, extra entries,
  upper-case entry names, a missing entry, a truncated archive, a non-ZIP file.
- `.cub`: the test vector of 9.3, a file with a UTF-8 BOM, a UTF-16 file, non-ASCII and astral characters, and the
  string and number quirks of 9.4.

### 10.2 Golden files

Sample files received from users (section 11) are private: they MUST NOT be committed to the repository. Tests read
them from a private location named by the environment and report "skipped (oracle absent)" when it is not set. Only
fixtures that the port generates itself, and files whose authors agreed to publication, are stored under
`tests/golden/`. For every sample, the golden set (private or public) holds:

1. The original file.
2. The decompressed `cubik` payload.
3. A canonical text dump of the payload produced by a port tool (key path, BSON type, value; doubles printed as hex
   bit patterns; binary values printed as their length and SHA-256, and RawTexture `r` in addition as the level-0
   length and SHA-256 plus the length of the remaining tail) for readable diffs.
4. An expected summary (counts of elements, faces enabled, textures with formats, sizes, `r` lengths and level-0
   SHA-256, voxel count per colour, meshes per kind, display slots, key model properties).

Tests:

- **Payload identity**: read, then write without modification; the new payload MUST equal the original byte for byte
  (8.3). This covers default textures too: the reader MUST NOT substitute the port's own pictures (4.5.2). Holds for
  sample 1 with a schema-driven research encoder (11.1).
- **Semantic round trip**: read into the editor model, discard retained originals, write; the payload MUST equal the
  original except for the documented normalisations (6.3), which the test lists explicitly. Because the writer then
  re-encodes every image (4.5.1), the list also contains a texture rule: texture records are compared on
  `lb`, `fp`, `fn`, `fm`, `cs`, `n`, `w`, `h` and the level-0 pixels decoded to RGBA, while `f`, the length of `r`
  and any mip tail are excluded.
- **Summary**: the decoded model MUST match the expected summary.
- **Fresh model** (needs request 1 of 11.2; sample 1 is not a fresh save): the payload the port writes for a new
  model, compared through the canonical dump, MUST equal that file except for `mp.sp`, `mp.g.d`, the camera states
  `vp.ctl` to `vp.cbr` (until their defaults are known, 12 item 10) and the `r` values of the two default textures
  (the port's own pixels without a chain, 4.5.2).
- **Container** (optional): with classic zlib available, re-deflating the payload as described in 2.1 and writing the
  original header fields and timestamps MUST reproduce the original file (holds for sample 1).

### 10.3 Cross-application tests (Windows with Cubik 2.9.480 installed, manual or UI-automated)

1. Port writes file A from a scripted scene; Cubik opens A without any dialog other than the normal status, the scene
   matches (counts, a screenshot of the four views compared against the port's render within a tolerance); Cubik
   saves it as B; the port reads B and its payload MUST equal A's payload apart from the documented normalisations and
   the capture of cameras (for textures: a null `fp` comes back as "", 6.3).
2. Cubik authors file C; the port reads C; the port's summary MUST match what Cubik shows; the port writes C';
   Cubik opens C' and re-saves C''; C'' payload MUST equal C payload apart from documented normalisations (for
   textures: mip chains dropped and null `fp` written as "", 6.3).
3. Repeat 1-2 for each feature area: elements of all four types, all face options and rotations, textures of each
   pixel format, voxels at R = 2, 16, 32, 128 and an upscaled grid, each mesh kind and an OBJ with several
   sub-meshes, display slots, every model properties field, camera changes.
4. Negative tests in Cubik: confirm that the error behaviours listed in this document (null value-type keys, missing
   faces, short texture data) reproduce, to validate the port's error mapping.

### 10.4 Robustness and performance

- Fuzz the container, BSON and `.cub` readers (coverage-guided fuzzing); no panics, bounded memory and time.
- Load and save benchmarks on large documents (R = 128 voxel models, meshes near the vertex limit, 4096 x 4096
  textures); targets to be set against Cubik's own timings measured with the same files.

---

## 11. Sample files

### 11.1 Sample evidence

Sample 1 is the first real `.cubik` file received. It is private and stays outside the repository (10.2). Content in
generic terms: a voxel model (one world at R = 128, 53 chunks, about 18 000 voxels with several hundred colours),
the two texture labels of a new model (`texture` holding a user image), no elements, no meshes; file 173 937 bytes,
payload 953 962 bytes. The producing version is unknown: the file has no version marker, and nothing in it requires a
version other than 2.9.480.

Proved by sample 1 (each item is marked "verified (sample 1)" in its section):

- Container (2.1): one deflated entry `cubik`, DotNetZip header fields, NTFS extra field, no data descriptor, no
  ZIP64, no comments; the parallel 64 KiB segmenting with sync flushes; classic zlib level 1 reproduces the
  compressed stream, and with the original timestamps the whole file, byte for byte.
- Encoding (3): the BSON subset, exact sizes, exact f32 widening of every double, -0.0 kept, and the member order of
  every record type present. A research encoder that decodes the payload into the records of section 4 and writes
  them back with the rules of section 3 reproduced the 953 962-byte payload byte for byte.
- Values: root (`v` = 1), the fresh values of every model property group, view grid keys, the blueprint record, the
  camera states of 4.3.1, the display slots, the voxel layout, scan orders and colour packing (4.7, 4.10).
- Rotations (4.3.2): single-precision artifacts of the engine (one f32 step above whole degrees), -0.0 Euler
  components, and the gimbal-lock form of the top camera; the 1e-4 degree tolerance suffices.
- Textures (4.5): record layout, bottom-to-top rows, ARGB32 with alpha first, non-power-of-two sizes, and the atlas
  recut of 4.5.3, which explains why the default `particle` record appears as `f` = 5, 1024 bytes, `n` = "" instead
  of the fresh form of 4.5.2, and why the user image in `texture` has neither path nor signature.

Corrected because of sample 1: path separators (4.2), the expectation that the default textures of a saved file are
RGB24 with the names `e_particle` / `e_texture` (true only before any paint stroke, 4.5.2, 4.5.3), the tolerance and
gimbal-lock rules of 4.3.2, and the storage of user samples (10.2).

Not covered by sample 1: elements and faces, meshes, Box, Vector2, a fresh save, reopen effects, small or stored
payloads, other resolutions, empty voxel chunks, `cs` values, non-ASCII strings.

### 11.2 Samples still requested

All files written by Cubik Studio 2.9.480 Beta unless stated otherwise; one feature per file where possible, plus
screenshots of the four views and of the Model Properties dialog for each. Note the Cubik version with every file.

1. A model saved immediately after start-up (File > New, then Save) **without using any paint tool**, and the same
   file reopened in Cubik and saved again. Inspect `tx.f`, `tx.n`, the type of `fp`, the length of `tx[0..1].tx.r`
   and its tail after the first 768 bytes (4.5.2). Still open: sample 1 is not a fresh save. In addition, the same
   new model saved after a single paint stroke with any tool (confirms 4.5.3 on a minimal file), and a model with
   PNG and JPEG textures saved before and after one paint stroke (settles 12 item 3 and the recut of other formats).
2. The same empty model saved twice in a row, and once after moving each view camera (confirms the camera states
   of 4.3.1 as start-up defaults).
3. Elements: one cube; one of each plane type; rotated elements on each axis with "restrict rotations" on and off;
   an element with every face option changed (disabled faces, cull faces, tint index, UV rotation, UV rectangles
   crossing the texture border); hidden and locked elements; a selection of several elements.
4. Textures: loaded from PNG (with and without alpha), JPG, TGA, DDS DXT1 and DXT5, an animated texture with `.mcmeta`,
   a shortcut texture (`#label`), the texture atlas created by "Create textures atlas", and a model after
   "Remove unused textures". Each saved without any paint stroke in the session, because a stroke recuts every
   texture to ARGB32 (4.5.3).
5. Voxels: grids at resolutions 2, 16, 32 and 128 (128 covered by sample 1); one after "Upscale x2" from 128; one
   after "Clear voxels"; one after erasing all voxels manually.
6. Meshes: every primitive kind (cube, sphere, cylinder, cone, pyramid, slope, corner-in, corner-out, torus), a
   PNG sprite, an imported OBJ with one material and one with several materials and textures, a tinted mesh.
7. Display settings: every slot edited, presets applied, zero scale values.
8. Model properties: every tab with non-default values (including a description containing a double quote and
   non-ASCII text, a parent, entity settings, map table).
9. A model with the model box dragged; a model imported from Minecraft JSON with item overrides, then saved.
10. A merge result (File > Merge of two files).
11. Large files: more than 300 KiB and more than 50 MB if available (sample 1 has a 954 KB payload in a 174 KB file,
    so it does not trigger the loading message of 6.1). Also a small model whose payload is below 512 KiB, to check
    the single-stream deflate of 2.1.
12. The automatic backup files from `%USERPROFILE%\AppData\LocalLow\BDcraft\Cubik\backup\`.
13. Files written on a Windows installation with a non-Western ANSI code page (for example Vietnamese), to check the
    texture signature encoding.
14. `.cubik` files from older Cubik Studio versions (any 1.x or 2.x release), with the version noted.
15. Legacy `.cub` files from Cubik alpha versions, together with their texture folders.
16. If possible, the same scene saved by Cubik and exported as Minecraft JSON, to cross-check coordinates.

---

## 12. Unverified / Open questions

1. ZIP details of Cubik's writer: whether incompressible payloads are stored instead of deflated, whether payloads
   up to 512 KiB are one ordinary deflate stream (library code says so), and whether the archive is written through
   a temporary file. Resolved by sample 1: the NTFS time extra field is written, header fields and the segmented
   level-1 deflate of larger payloads (2.1).
2. Whether Cubik's entry lookup is case-insensitive (believed yes).
3. Pixel format of textures loaded from PNG files: RGBA32 (4) or ARGB32 (5). Readers must support both regardless.
   Sample 1 cannot settle it: its textures went through the recut of 4.5.3, which yields ARGB32 for every source.
4. Exact 16-bit layouts of ARGB4444 and RGBA4444 data.
5. Behaviour of the engine when raw texture data is longer than level 0 (assumed accepted, with only level 0 used:
   Cubik reopens its own fresh files, whose default textures are believed to carry mip chains), when it is shorter
   (load failure or blank image), and when data has a format the GPU does not support.
6. Whether `me.t` holds the concatenated triangles of all sub-meshes and the exact vertex limit (65 000) of meshes in
   Cubik's engine.
7. Whether sprite meshes (kind 10) keep their extruded geometry after a save and reload.
8. Numerical agreement between the port's quaternion/Euler conversion and the engine's native one. Sample 1 shows
   deviations of one f32 step and confirms that 1e-4 degrees suffices for its four cameras; still open: the engine's
   gimbal-lock threshold and which of y and z it zeroes there (4.3.2), and agreement for arbitrary rotations.
9. Code page decoding of the texture signature `cs` (Mono's tables for undefined bytes may differ from Windows').
10. Fresh-model camera states (`vp.c*`) and the exact default view camera positions. Sample 1 gives believed defaults
    (4.3.1); request 2 of 11.2 confirms them.
11. Existence and layout of older `.cubik` variants (other keys, JSON payloads).
12. The text encoding used by the alpha-era `.cub` writer (UTF-8 or ANSI) and how voxel colours above 2^31 were
    written there.
13. The meaning of the empty option groups `ht` and `tr`.
14. Whether undo and clipboard data, which reuse the document shape in memory, ever reach the disk (not observed).
15. The visible effect of `mp.g.o` when loading a `.cubik` file (the origin marker may only update when re-enabled).
16. Payload of the default textures in a fresh save (4.5.2): whether `r` holds the full mip chain (1023 bytes,
    believed) or level 0 only (768); the values of mip levels 1 to 4 (filter and rounding of the engine's rebuild);
    whether level 0 equals the built-in pixels (the engine's copy of 32-bit colours into an RGB24 image). Request 1
    of 11.2 settles all three; the same questions apply to missing-file placeholders. Sample 1 only supports the
    third point (4.5.2): its default textures are in the recut form of 4.5.3.
17. Art decision: whether the optional local skin pack of the art policy (`ui_layout_theme.md` 15.1) may also supply
    the two default document textures, which would write Cubik's pictures into the user's own files. Until decided,
    the port uses its own pictures (4.5.2).
18. Atlas recut (4.5.3): the result when every texture is block-compressed, whether textures larger than the
    atlas limit come back shrunk, and whether actions other than the end of a paint stroke trigger it (none found in
    the code).
19. The Cubik version that wrote sample 1 (no marker in the file; 2.9.480 assumed).
