# Application behaviour: lifecycle, views, rendering look, preview, preferences

Functional specification for **cuboid**, a clean-room reimplementation of Cubik Studio 2.9.480 Beta
(Windows build). This document is the "views spec", "settings spec" and "display-settings spec" that the
sibling specifications refer to (name map: 0.3) and covers everything that is application-level rather than
model-level:

- start-up and shut-down, instances, command line, file association, main window behaviour;
- the four 3D views: cameras, projections, navigation, maximize, side swap, camera presets, persistence;
- grid overlay, guides and view background images;
- the rendering look (lights, shading, transparency, highlight outlines, overlays) precisely enough for a
  wgpu renderer to match it;
- the Minecraft display-settings preview (modes, preview scenes, presets, player skin);
- every preference (label, storage key, default, effect), other persisted state;
- recent files, automatic backup and the Recover menu, drag and drop, clipboard, render to PNG, Sketchfab
  upload, Minecraft/game folder integration, network and update policy;
- a feature parity checklist and the open questions.

Byte layouts of files are in the native format specification (`formats_native.md`); editing operations,
selection and gizmos in `model_and_tools.md`; panel/menu geometry and theme in `ui_layout_theme.md`.
This document repeats only what it needs.

Notation (shared with the sibling specifications):

| Term | Meaning |
|---|---|
| internal space | left-handed, +Y up frame the original stores in memory and in `.cubik` files; +X = east, +Z = north |
| MC space | Minecraft-style frame shown to users: `(x, y, -z)` of internal space |
| unit | one model unit = one Minecraft texel; 16 units = 1 block |
| central block | internal `x ∈ [0,16]`, `y ∈ [0,16]`, `z ∈ [-16,0]` |
| editing volume | internal `x ∈ [-16,32]`, `y ∈ [-16,32]`, `z ∈ [-32,16]`, centre `(8,8,-8)`, 48 units wide |
| `Euler(x,y,z)` | rotation that applies Z first, then X, then Y (degrees); rotating +Z about +X by +90° yields -Y |
| `{App}` | the product name. The original shows "Cubik Studio"; the port substitutes its own name in every string |
| [V] | verified in the original program logic and/or its shipped scene data |
| [I] | inferred from strong indirect evidence |
| UNVERIFIED | not confirmed; see section 21 |
| QUIRK | original behaviour that looks unintended; the port reproduces it unless section 0.2 lists a deviation |
| "text" | user-visible string, must match exactly (typos included) except for `{App}`/brand substitutions |

All colours are `#RRGGBBAA`. All arithmetic in the original is single precision.

---

## 0. Ground rules for the port

### 0.1 Parity principle

The port reproduces the observable behaviour of the original described here, including documented QUIRKs,
except for the deviations listed in 0.2. Branded texts (product name "Cubik Studio"/"Cubik", "BDcraft",
web addresses) are replaced by the port's own; layout and wording otherwise stay identical.

### 0.2 Deviations (port policy)

| Id | Original behaviour | Port behaviour | Reason |
|---|---|---|---|
| D1 | Online licence/registration check at every start, periodic re-check, device fingerprint upload, update prompts from the licence reply | none; no licence, registration, telemetry or account | out of scope, privacy |
| D2 | Downloads the player skin by name over plain HTTP from legacy Mojang/Amazon hosts whenever a name is set | skin lookup only after explicit opt-in, HTTPS, current skin service; local PNG file also accepted | privacy, obsolete hosts |
| D3 | Sketchfab upload to the retired v1 endpoint, token in the form body, local file path leaked as multipart file name | current Sketchfab upload API over HTTPS, token never logged, only the archive file name sent | obsolete API, privacy |
| D4 | Writes the `.cubik` association into `HKEY_CLASSES_ROOT` on every launch | association only through the installer or an explicit user action, per user | no silent system changes |
| D5 | Optional update check | off by default; when enabled by the user: at most once per launch, HTTPS, to the project's public release feed, prompt only (no automatic install) | "no phone home without consent" |
| D6 | Drag and drop replaces the document without asking about unsaved changes | drag and drop goes through the unsaved-changes prompt (2.2) | data loss fix (recommended) |
| D7 | In a session that started without a settings file the recent-files submenu does not react to clicks | always works | bug fix (recommended) |
| D8 | Writes `.cubik`, settings and backups in place | write to a temporary file and rename | crash safety (recommended) |
| D9 | Key bindings stored in the settings file are not applied at start-up; the built-in bindings stay active until the next Preferences OK (10.2) | the stored bindings are applied while the settings load | bug fix (recommended) |
| D10 | A settings file with `null` or a wrong type in any boolean or integer key is rejected as a whole (error box, all defaults, the file is overwritten later without a copy); a `null` or unknown theme name applies no theme at start-up; a `null` signature template breaks the default model signature (10.1) | keys are read one by one: an unusable value (`null` or a wrong type in a boolean or integer key, a `null` signature template, a `null` or unknown theme name) falls back to that key's default and the other keys are kept; a file that is not valid JSON is kept as `cubikdata.json.bad` before defaults are used | robustness (recommended) |
| D11 | Display Settings pages show control defaults after the first opening until their tab is clicked, so a copy button can copy defaults; a refresh that changes shown values records an empty undo step and marks the document dirty (9.3, 9.6) | pages always show the slot values; refreshing never records undo steps, never marks the document dirty and never sets the "use display settings" flag | bug fix (recommended) |

"Recommended" deviations need owner sign-off; until then they are implemented behind a compatibility switch
whose default is the port behaviour.

### 0.3 Document map and cross-reference names

The specifications refer to each other by short informal names. They resolve as follows ("this document" =
`app_behavior.md`):

| Name used in the specs | Document | Main sections |
|---|---|---|
| views spec, view specification | this document | 5 cameras and navigation, 6 grid and guides, 7 view backgrounds |
| settings spec | this document | 10.1 settings file, 10.2 every setting, 10.4 other persisted state |
| display-settings spec | this document | 9 (9.1 modes, 9.2 switching, 9.4 slot transform, 9.6 panel controls, 9.7 skin, 9.8 presets) |
| application specification | this document | 2.2 unsaved-changes prompt, 1 start-up, 2 shut-down |
| format spec, native format specification, file-format specification | `formats_native.md` | 4 document schema, 5 save, 6 load, 9 legacy `.cub` |
| files spec, import/export spec, import specification, export specification | `formats_interop.md` | 4 format registry and filter lists, 5 import pipeline, 6 export pipeline, 7-29 per format |
| model spec | `model_and_tools.md` | whole document (2 document model, 3 object base, 4 elements, 5 textures, 6 meshes, 15 undo) |
| selection spec | `model_and_tools.md` | 8 selection model (8.1 state, 8.6 modes, 8.9 list panels) |
| tools spec | `model_and_tools.md` | 8.7 custom mode, 9 gizmos and drags, 10 operations, 12 voxel tools, 13 paint and colour tools |
| clipboard spec | `model_and_tools.md` | 11 (application-level summary: section 14 here) |
| voxel spec | `model_and_tools.md` | 7 voxel world, 12 voxel tools, 14 conversions |
| voxelize spec | `model_and_tools.md` | 14.1 meshes to voxels (dialog: `ui_dialogs_menus.md` 9.4) |
| element spec, mesh spec | `model_and_tools.md` | 4 and 10.8 (elements), 6 and 10.14 (meshes) |
| layout spec, UI layout spec, shell spec | `ui_layout_theme.md` | 1 canvas, scale and window, 2 main window and panel sets, 3 menu bar, 4 status bar, 5 4-view zone, 6-9 strips and panels, 10 tooltips, 11 dialog chrome, 12 skins, 16 widgets |
| menus spec, dialogs spec | `ui_dialogs_menus.md` | 1-5 menus and actions, 6 keyboard and mouse, 7 dialog framework, 8 message box, 9 dialogs |
| file browser spec, file-dialog specs | `ui_dialogs_menus.md` | 10 built-in file browser (10.6 extension list, 10.10 locations, 10.11 path aliases); OS dialogs and validation: `formats_interop.md` 5 and 6 |
| UI spec | ambiguous (`ui_layout_theme.md` or `ui_dialogs_menus.md`) | this document always names the target section instead |
| texture spec | not yet written (planned) | until it exists: `model_and_tools.md` 5 and `ui_dialogs_menus.md` 5.11 |
| UV editor spec | not yet written (planned) | until it exists: `model_and_tools.md` 10.16 and 10.17, `ui_layout_theme.md` 9.1 |

Inside this document "model spec", "format spec", "layout spec" and "file browser spec" are used as in the
table.

---

## 1. Start-up

### 1.1 Window at launch [V]

- Windowed, client size 1000 x 700 on the very first launch, user-resizable, no resolution dialog, no splash
  screen. The engine restores the previous window size on later launches (engine-managed storage, not
  application code) [I]. The port stores the client size in its own settings.
- The application keeps running when it loses focus (backups and uploads continue).
- Alt+Enter toggles full screen (engine feature) [I]. See 4.4.

### 1.2 Initialisation order

The order below is observable (status texts, dialogs, which state wins); the port keeps it where it matters.

1. Number formatting and parsing become culture-invariant for the whole process (`.` decimal separator) [V].
2. The four view cameras are captured as the "default camera states" used by Reset Views (5.7); the Main
   view becomes the active view [V].
3. The UI scale is computed (4.3) and the minimum window size derived (4.2) [V].
4. Drag and drop is enabled (13) [V].
5. Settings are loaded (10.1) on first use, before any user interaction [V]:
   - file present and valid: values applied (property side effects included, 10.1), then the window is moved
     to the saved top-left position unless Left Alt is held, or `windowX` or `windowY` is 0, or either
     magnitude is 30000 or more; 0.5 s later the recent-files submenu is built and its click handler
     registered (11). QUIRK: the stored key bindings are not applied (10.2; port: D9);
   - file missing, empty, or containing only `null`: defaults, window not moved, log "CubikData: Fresh data
     created"; QUIRK: the recent-files click handler is not registered in this session (D7);
   - file not usable (malformed JSON, or `null` or a wrong type in a boolean or integer key, 10.1): an error box
     titled "Exception" showing the full exception text, then as "file missing" (port: D10).
6. The theme named by `appSkin` is applied; a `null` or unknown name applies none (10.1) [V].
7. View background planes are hidden; the background file dialog's start folder is fixed to the textures
   default path for the rest of the session (QUIRK: later preference changes do not affect it) [V].
8. The built-in file browser is created; on the first launch, or when "Re-add default favourites in Cubik
   file browser on launch" is on, the default favourites are added (17.3) [V].
9. Status bar details cell shows the empty-scene text "0 element (0 face)" (19.2) [V].
10. The backup loop starts (12.1) [V].
11. Full reset to a new document (1.3) [V].
12. File association is registered (3.3; port: D4) [V].
13. The preview player skin is loaded (9.7) [V].
14. A file given on the command line is opened (3.2) [V].
15. Side panels: managers 220 px wide, helpers 350 px wide, managers docked right (10.4) [V].

Original-only steps that the port omits: licence handshake and periodic gate (D1), integrity self-check of the
program files, native-helper self-test (no effect).

### 1.3 New document state ("full reset")

Used at start-up and by File > New file. After it [V]:

- texture list = the two default textures `particle` and `texture`;
- model properties = defaults (name "Cubik Model", empty save and export paths), model box = editing volume,
  model origin = (0,0,0), no elements, no meshes, voxel grid cleared;
- selection cleared, voxelisation state reset, display preview = Default mode (9.2), panel set = the custom
  set from the "open panel" preferences, paint state reset;
- undo history cleared, document clean (not dirty), internal clipboard emptied (14);
- grid reset (6.6); window title "{App} - unsaved" (4.1).

Not reset: camera poses and view anchors, maximize state, wireframe mode, background images, cinema frame,
preferences.

### 1.4 Debug mode

The original enables log capture when a marker file exists in its install folder or a build flag is set
(log written to the temporary cache folder, folder opened on the first error). The port replaces this with a
`--debug` switch / environment variable. Not a parity item.

---

## 2. Shut-down

### 2.1 Sequence [V]

Triggered by File > Exit, the window close button, Alt+F4 or the OS.

1. If the document is dirty, the quit is cancelled and the unsaved-changes prompt (2.2) is shown with
   "quit afterwards" semantics. If not dirty, continue.
2. (debug mode) the log report is written.
3. Windows: the settings file is written (10.1), including the current window top-left position as
   `windowX`/`windowY`. On other platforms the original only writes settings on Preferences OK (port: always
   write on quit).
4. The drag and drop hook is removed (log "Drag&Drop hook stopped") and the process exits. While quitting,
   closing panels does not trigger their normal side effects (for example the display preview does not
   switch back to Default).

### 2.2 Unsaved-changes prompt [V]

Shared by Exit/close, File > Open, Merge, Import and the recent-files menu.

- Shown only when the document is dirty. Question box: title "Unsaved modifications", text "Do you want to save
  the changes you made to model <model name>?", buttons "Yes", "No", "Cancel".
- **Yes**: run File > Save (Save As dialog when the document has no path). After a successful native save:
  if the prompt came from quitting, quit now. QUIRK: the pending action (open, merge, import, recent file) is
  not continued after saving; the user must repeat it. Cancelling the Save As dialog leaves the application
  running with nothing else done [I].
- **No**: the document is marked clean; then quit (when quitting) or run the pending action.
- **Cancel**: close the box, nothing else.
- Dirty = an undo step was recorded since the document was last reset (New file, native open) or saved
  natively. Exports and backups never clear it.

---

## 3. Instances, command line, file association

### 3.1 Instances [V]

Multiple instances are allowed; there is no single-instance lock and no inter-process hand-off. Opening a
`.cubik` from the file manager starts a new process. Instances share the settings file (last writer wins at
quit), the presets folder and the backup folder (backups of concurrent instances overwrite each other's
weekday file). The port keeps this behaviour; a single-instance mode is not part of parity.

### 3.2 Command line [V]

- If the first argument exists and does not start with `-`, it is opened like File > Open (replace semantics,
  extension validation, recent-files update) once the UI is ready. No unsaved-changes prompt (new session).
- Log "Loading file at launch: <path>"; the original shows status "Please wait..." while it waits for its
  licence handshake (port: no wait).
- All further arguments are ignored.

### 3.3 File association (Windows) [V]

Original, on every launch:

1. If `<data folder>/cubik_format.ico` does not exist it is written from the bundled icon (first launch shows
   the status "Successfuly saved file: <path>").
2. Registry values written under `HKEY_CLASSES_ROOT`:
   - `.cubik` default value = `CubikStudio`;
   - `CubikStudio` default value = `Cubik Studio Model`;
   - `CubikStudio\shell\open\command` default value = `"<install folder>\CubikStudio.exe" "%1"`;
   - `CubikStudio\DefaultIcon` default value = `<data folder>\cubik_format.ico` (backslashes).
3. The shell is notified that associations changed.
4. Without administrator rights the write fails silently (log warning "Can't associate .cubik files to Cubik:
   <message>").

Port (D4): same extension, an own ProgID and description, open command `"<exe>" "%1"`, written per user
(`HKCU\Software\Classes`) only by the installer or an explicit "Associate .cubik files" action.

---

## 4. Main window

### 4.1 Title [V]

- "{App} - <full path of the current native document>", or "{App} - unsaved" when the document has no native
  path. Original value of `{App}`: "Cubik Studio".
- Updated after a full reset, after a native file was opened (original: even if the import failed; port
  follows the format spec and only adopts the path on success), and when a native save starts (the title
  switches to the target path immediately). Imports and exports never change the title.

### 4.2 Size limits and position [V]

- Minimum client size: 900 x 700 multiplied by the UI scale (4.3). On Windows the original checks every frame
  and resizes the window back to max(minimum, current) when it is smaller.
- Position: top-left saved as `windowX`/`windowY` (10.2) on quit (Windows) and on Preferences OK; restored at
  start as in 1.2 step 5. Holding Left Alt during start skips the restore (rescue for an off-screen window).
- The original also writes two values `ScreenWidth`/`ScreenHeight` when the window becomes as wide as the
  monitor; nothing reads them. The port ignores them.

### 4.3 UI scale [V]

`s = floor(max(96, dpi) / 96 * 2) / 2` (steps of 0.5), clamped to `[1.0, 1.99]` when applied to the UI
(details in `ui_layout_theme.md` 1.2). Computed once at start-up. Applying a scale re-lays out the views.

### 4.4 Full screen [V]

On Windows the original's own full-screen handling does nothing beyond logging; the engine's Alt+Enter toggle
applies. (A macOS-only branch changes resolution and scale; not ported.) Port: Alt+Enter and F11 toggle
borderless full screen, restoring the previous windowed rectangle.

### 4.5 Frame pacing [V]

- Focus gained: target 60 frames/s and an immediate view re-layout. Focus lost: target 10 frames/s.
- The original's graphics quality preset enables vertical sync, which makes the engine ignore the 60 frames/s
  cap; focused redraw therefore runs at the display refresh rate [I].
- Port: redraw on demand (input, animation, background jobs), capped at the display refresh rate; while
  unfocused at most 10 redraws per second and only when something changed. Time-based effects use seconds.

### 4.6 Modal state [V]

A global modal flag is set while a modal dialog, the About box, a message box or the built-in file browser is
open (the browser sets it in every mode: open, save and folder picking); while it is set, keyboard shortcuts are
suppressed (shortcut rules: `ui_dialogs_menus.md` 6.2 and 7.3). Native OS dialogs block the application while
they are open. When the pointer enters any dialog window the hovered view is cleared (5.4), so the 3D views stop
receiving input under dialogs.

---

## 5. 3D views and cameras

### 5.1 The four views [V]

| View | Quadrant | Initial label | Projection | Default pose: position; `Euler` | Looks along | Second anchor (side swap) | Pan sign per anchor |
|---|---|---|---|---|---|---|---|
| Top | top-left | Up | orthographic, half-height 12 | (8, 72, -8); (90, 0, 0) | -Y | Down: (8, -54, -8); (270, 0, 0) | Up (1,1,1); Down (1,1,-1) |
| Side | top-right | West | orthographic, half-height 12 | (-54, 8, -8); (0, 90, 0) | +X | East: (46, 8, -8); (0, 270, 0) | West (1,1,1); East (1,1,-1) |
| Front | bottom-left | South | orthographic, half-height 12 | (8, 8, -54); (0, 0, 0) | +Z | North: (8, 8, 46); (0, 180, 0) | South (1,1,1); North (-1,1,1) |
| Main | bottom-right | Main | perspective, vertical field of view 64° | (-4, 24, -20); (45, 45, 0) | toward the pivot | none | n/a |

- Planar axes used for panning and keyboard nudges: Top = X and Z (normal Y), Side = Y and Z (normal X),
  Front = X and Y (normal Z), Main = none (free).
- Clip planes: orthographic views near 0.3, far 128; Main near 0.3, far 256.
- Each view clears to `#343434` (opaque for display purposes) with depth cleared; no HDR.
- Screen orientation of the default anchors: Top: right = +X, up = +Z (north up). Side (West): right = -Z,
  up = +Y. Front (South): right = +X, up = +Y.
- Orbit pivot ("focus point"): default (8, 8, -8). The Main camera starts 23.32 units from it. QUIRK: the
  default Main orientation looks along (0.5, -0.7071, 0.5), slightly above the pivot direction
  (0.5145, -0.686, 0.5145); the first action that re-aims the camera (5.5) snaps it onto the pivot.

### 5.2 Layout and maximize [V]

- The 4-view zone rectangle comes from `ui_layout_theme.md` 2 and 5.1. In 4-up mode it is split into four
  equal quadrants; each view's viewport is exactly its quadrant.
- **Maximize**: a left click on a view label toggles maximize for that view (all four, Main included). The
  maximized view's viewport becomes the whole zone; the other three views are neither drawn nor interactive.
  Maximize flags are mutually exclusive (maximizing a view clears the others). Maximizing also makes that view
  the pending active view (5.4). Clicking the label again returns to 4-up.
- The Display Settings panel maximizes the Main view automatically (9.3).
- The view rectangles are recomputed on window resize, on focus gain, on maximize changes, when the cinema
  frame toggles (5.10), when side panels change, and when the UI scale is applied.

### 5.3 View labels and side swap [V]

Label look and placement: `ui_layout_theme.md` 5.2 (11 px bold white, 31 % opacity at rest, full on hover).

- Left click: maximize toggle (5.2).
- Right click: **side swap** to the view's next camera anchor (cyclic: Up ↔ Down, West ↔ East,
  South ↔ North):
  1. For each axis on which the old and the new anchor positions are equal, the camera keeps its current
     coordinate; on the other axes it takes the new anchor's coordinate. (Up ↔ Down keeps the current X and Z,
     so panning survives the swap.)
  2. The camera rotation becomes the new anchor's `Euler` angles; the orthographic size is unchanged.
  3. The label text becomes the new anchor's name.
- Main has a single anchor. QUIRK: a right click on "Main" keeps the camera position and only resets the
  orientation to (45, 45, 0); the camera then no longer looks at the pivot until the next re-aim.
- QUIRK: Reset Views (5.7) does not reset the anchor index or the label; after cycling to "Down" and resetting,
  the label still reads "Down" and the next right click goes to "Up" (with no visible camera move).

### 5.4 Hovered and active view [V]

- A view is **hovered** while the pointer is inside its quadrant and not over any UI on top of it (menus,
  popups, panels, dialogs). Entering a dialog window explicitly clears the hover state.
- The hovered view becomes **pending**; every frame the pending view is copied to the **active view** unless
  a mouse button (left, right or middle) or the pan control is held. A drag therefore keeps its view until all
  buttons are released, even when the pointer leaves the view.
- The active view decides: which camera the mouse wheel zooms, which camera and plane panning uses, the plane
  of keyboard nudges and gizmo constraints (model spec), the picking camera.
- Hover-gated actions (nothing happens while no view is hovered): wheel zoom, menu and keypad zoom, starting a
  pan, picking/selection, gizmo hover, the Focus key.

### 5.5 Navigation [V]

Constants: pan speed `0.008`, zoom speed `6`.

**(a) Pan control.** Default bindings middle mouse button and Space (preference "Pan view", 10.2). Pressing
the control while a view is hovered arms panning. Panning runs every frame while the control stays held and no
object/gizmo transform is in progress; it ends when the control or the left mouse button is released. On the
first panning frame the original records the active camera, its planar axes, the pointer position `M0`, the
camera position `C0` and the pivot `P0`. Each frame `d = M - M0` in window pixels (x right, y up). Space needs
no mouse button: moving the mouse while Space is held pans.

**(b) Orthographic views (Top, Side, Front).** Absolute offset from the start, `s` = current orthographic
half-height, `⊙` = anchor pan sign (5.1):

| View | World offset `Δ` | Camera position |
|---|---|---|
| Top | `(-dx, 0, -dy) · s · 0.008 ⊙ sign` | `C0 + Δ` |
| Side | `(0, -dy, dx) · s · 0.008 ⊙ sign` | `C0 + Δ` |
| Front | `(-dx, -dy, 0) · s · 0.008 ⊙ sign` | `C0 + Δ` |

The content follows the pointer direction ("grab" feel) but not 1:1: per pointer pixel the content moves
`0.004 · H` screen pixels, H = viewport height in pixels. The UI scale is not taken into account.

**(c) Main view, pan control without the rotate modifier.** `v = R_cam · (dx, dy, 0) · 0.1` (camera rotation
applied to the screen delta, 0.1 units per pixel, independent of distance); camera = `C0 - v`;
pivot = `P0 - v`; the camera then re-aims at the pivot. Same rule when the Main view is orthographic.

**(d) Main view orbit.** Pan control held **and** the rotate modifier held (Left Alt, Right Alt, Left Ctrl or
Right Ctrl; not configurable). Per frame (incremental, `M0` reset to the current pointer every frame):

1. yaw: rotate the camera around the pivot about world +Y by `dx · 0.48°`;
2. pitch: rotate the camera around the pivot about the camera's (updated) right axis by `-dy · 0.32°`.

There is no pitch limit (the camera can roll over the poles). The pivot does not move.

**(e) Zoom.** Input value `z`: mouse wheel `+0.1` per notch toward the screen [I: engine wheel delta 1 per
notch times the configured 0.1 sensitivity, UNVERIFIED]; Views > Zoom+ / Zoom- (shortcuts: keypad `+` and
keypad `-` only) `+0.5` / `-0.5`.

- Orthographic camera: `size = clamp(size - 6·z, 1, 30)` (wheel notch ±0.6, menu ±3).
- Perspective camera: `candidate = position + forward · 12·z`; if `distance(candidate, pivot) < 2` and `z > 0`
  the camera does not move; otherwise it moves to `candidate` and re-aims at the pivot (wheel notch 1.2 units,
  menu 6 units). No maximum distance (the far plane at 256 clips).
- The wheel zooms the active view only; Zoom+ / Zoom- zoom all four views at once.
- QUIRK: every zoom requires a hovered view. Clicking Zoom+ / Zoom- in the menu therefore normally does nothing
  (the pointer is over the menu, not a view); the keypad shortcuts work while the pointer is over a view [I].
- Zoom is about the view centre (not the pointer).

**(f) Focus.** Pressing Focus (default F, preference "Focus view") while a view is hovered, no button held, and
the pointer over a pickable object: the pivot moves to the hit point and the Main camera turns to look at it
(position unchanged). Works from any view. The original polls this at 50 Hz, so very short key presses can be
missed (QUIRK, the port may poll every frame).

### 5.6 View input bindings (defaults) [V]

| Action | Default | Configurable |
|---|---|---|
| Pan view | middle mouse button, Space | yes ("Pan view") |
| Rotate-view modifier (Main orbit) | Left Alt, Right Alt, Left Ctrl, Right Ctrl | no |
| Zoom | mouse wheel; keypad `+` / keypad `-` (menu shortcuts, all views) | no |
| Focus | F | yes ("Focus view") |
| Maximize / side swap | left / right click on the view label | no |
| Wireframe mode | Shift+W | no |
| Reset Views | Ctrl+Shift+C | no |

The main-keyboard `+`/`-` keys do not zoom (the zoom input is wheel-only; the menu shortcuts map to the keypad).
Other bindings (rotate/scale/clone modes, selection modifiers, arrows): model spec.

### 5.7 Reset Views and camera persistence [V]

- Recover > "Reset Views to default" (Ctrl+Shift+C) records an undo step "Reset camera" (view properties) and
  restores the four cameras to the states captured at start-up (pose and projection, so the Main view becomes
  perspective with field of view 64 even when the orthographic preference is on: QUIRK) and the pivot to
  (8, 8, -8). Maximize, labels/anchor indices (QUIRK, 5.3), cinema frame and wireframe mode are unchanged.
- Native save stores the four camera states (position, rotation, orthographic flag, orthographic size, field of
  view; keys `ctl`, `ctr`, `cbl`, `cbr`, format spec). Native open **and merge** apply them. The pivot,
  anchors and labels are not stored. File > New file keeps the cameras.

### 5.8 Orthographic Main view preference [V]

"Use orthographic projection for main view" switches the Main camera to orthographic (half-height = its
stored orthographic size, initially 12) or back to perspective; applied when settings load and on Preferences
OK. While a display preview mode is active (9) the change is recorded for the Default mode's camera and takes
effect when the preview ends. Navigation in orthographic Main: pan 5.5(c), orbit 5.5(d), zoom by size.

### 5.9 Wireframe mode [V]

Views > "Wireframe mode" (Shift+W) toggles a session-only flag (off at start, not saved). While on, every mesh
drawn by the four views (elements, meshes, voxels, grid and background quads, preview decorations) is drawn
as wireframe of its triangles (each quad face shows one diagonal), keeping the colour the shading would give;
overlay lines (grid lines, guides) are unchanged. PNG renders are not affected.

### 5.10 Cinema frame [V]

Used by the first-person display modes (9.1). The Main viewport becomes the largest centred 16:9 rectangle in
its zone (height = width · 0.5625; when that exceeds the zone height, width = height / 0.5625). The remaining
area is covered by two opaque black bars: top and bottom when the zone is narrower than 16:9, left and right
when it is wider. The other views never use the frame.

### 5.11 What each view draws [V]

Scene content is tagged per view; a view draws only its tags:

| Content | Top | Side | Front | Main | PNG render |
|---|---|---|---|---|---|
| Model (elements, meshes, voxels), blueprint, model-origin markers, centre marker, preview player/item | yes | yes | yes | yes | model only (15) |
| Floor sub-grid plane (y ≈ 0) | yes | yes (edge-on) | yes (edge-on) | yes | no |
| Side sub-grid plane (x ≈ 0) | no | yes | no | no | no |
| Front sub-grid plane (z ≈ 0) | no | no | yes | no | no |
| Background image of the view | Top image | Side image | Front image | no | no |
| Move-gizmo variant | top | side | front | 3D | no |
| North pointer | yes | yes | no | yes | no |
| Display-preview decorations: GUI frame, first-person crosshair and sky | no | no | no | yes | no |
| Display-preview decorations: wall/item frame, floor ground and shadow | yes | yes | yes | yes | no |
| Hidden elements when "Show hidden elements in main view" is on | no | no | no | yes | no |
| Main grid lines and guides (6.4, 6.5) | own plane | own plane | own plane | floor plane | no |

In every display mode except Default (9.2) the workspace content is hidden in all views: sub-grid planes,
background images, gizmos, blueprint, north pointer and the centre and origin markers. Only the model, the
preview scene and, where the mode enables them, the main grid lines remain.

---

## 6. Grid, snap steps and guides

### 6.1 Settings [V]

Stored per document in the `.cubik` view properties (format spec 4.3). Steps are in units.

| Setting | Default | Key | Grid dialog control (slider range) | Dialog value N |
|---|---|---|---|---|
| Displayed sub-grid step | 1 | `gd` | "Displayed:" (2 to 64, integers) | step = 16 / N (default N = 16) |
| Large grid | off | `gl` | "Large" toggle | |
| Mouse snap step | 0.5 | `gm` | "Mouse snap:" (16 to 512) | step = 16 / N (N = 32) |
| Keyboard nudge step | 0.125 | `gk` | "Keyboard snap:" (16 to 512) | step = 16 / N (N = 128) |
| UV snap step | 0.5 | `gu` | "UV mapping snap:" (16 to 512) | step = 16 / N (N = 32) |
| Main line spacing | 16 | not stored | none | |
| Grid extent | 48 | not stored | none | |
| Grid visible | on | not stored | none (display modes only) | |
| Guides visible | on | not stored | none (display modes only) | |
| Snapping enabled | on | not stored | none; always on | |

N reads as "subdivisions per block". Uses of the steps: model spec (mouse snapping, keyboard nudges, UV snapping).

### 6.2 Grid size dialog [V]

Views > "Grid settings" (Shift+G). Header "Grid size". Non-modal; Escape cancels; Enter does nothing.
Layout: `ui_dialogs_menus.md` 9.2.

- Opening: remembers the displayed, mouse and keyboard steps; sets each slider to `round(16 / step)` and the
  "Large" toggle from the setting; each input box shows its slider value.
- Editing: a slider change applies `step = 16 / N` immediately (the grid redraws live); a typed number is
  committed on end of editing (integer parse, invalid text = 0, clamped to the slider range, then applied as a
  slider change); "Large" applies immediately.
- OK: closes, keeping the values. Cancel, the close button and Escape: restore the displayed, mouse and
  keyboard steps, then close. QUIRK: the UV step and "Large" are not restored.
- QUIRK: from the second opening in a session on, opening the dialog itself re-quantises the stored steps to
  `16 / round(16 / step)` (a non-matching step loaded from a file changes). Port: reproduce.

### 6.3 Sub-grid planes (fine grid) [V]

Three 48 x 48 unit squares, each centred on the editing volume:

| Plane | Position | Spans | Visible in |
|---|---|---|---|
| floor | y = -0.001 | x ∈ [-16,32], z ∈ [-32,16] | all four views |
| side | x = -0.001 | y ∈ [-16,32], z ∈ [-32,16] | Side view |
| front | z = +0.001 | x ∈ [-16,32], y ∈ [-16,32] | Front view |

Look:

- Lines every displayed step, aligned so that a line runs along each plane edge and every step from there
  (integer coordinates at step 1).
- Additive blending, both faces visible, depth tested against the scene, no depth write, drawn with the
  transparent objects (8.6).
- Each cell is one repeat of a 256 x 256 tile that is black with a 1-texel white row and a 1-texel white column
  on its edges; sampled with mip-mapping and bilinear filtering, so the lines thin out and fade when zoomed out.
  A fully covered line pixel adds `0.124` (about 32/255) to each colour channel of the framebuffer
  (tint alpha 0.031, the additive shader doubles colour and alpha).
- "Large": the planes become 16 times larger (768 units) with the same line spacing.
- Port guidance: an analytic grid shader drawing lines at multiples of the step, line width `step / 256` in
  world units, coverage-weighted, additive `0.124` white, plus a fade that imitates mip-map averaging
  (UNVERIFIED exact pixels; calibrate against screenshots).

### 6.4 Main grid lines [V]

Per view, 1-pixel lines every 16 units over a 48-unit square (4 x 4 lines, 3 x 3 blocks):

| View | Plane | Extent |
|---|---|---|
| Top, Main | y = 0 | x ∈ [-16,32], z ∈ [-32,16] |
| Side | x = 16 (QUIRK: not x = 0 like the side sub-grid) | y ∈ [-16,32], z ∈ [-32,16] |
| Front | z = 0 | x ∈ [-16,32], y ∈ [-16,32] |

Colour `#FFFFFF20`, normal alpha blending, depth tested (hidden behind geometry), no depth write, drawn after the
scene. Not affected by "Large" or by the displayed step.

### 6.5 Guides [V]

Drawn in the same pass as the main grid lines, only while grid **and** guides are visible (hiding the grid
also hides the guides):

- model box (the 12 edges of the model bounding box, model spec), colour `#FFC80040`, while the Gizmos panel's
  bounding-box display is on (default on);
- selection rectangle box (model spec), colour `#FF6400FF`, while shown.

1-pixel lines, depth tested.

### 6.6 Life cycle [V]

- New document (1.3): displayed step 1, mouse step 0.5, grid visible, snapping on, main spacing 16, extent 48.
  QUIRK: keyboard step, UV step and "Large" keep their values.
- Opening or merging a `.cubik`: mouse, keyboard and displayed steps are taken from the file; the UV step and
  "Large" are written with defaults and ignored on load (format spec 4.3).
- Display modes switch grid and guide visibility (9.1).
- Voxel setup "Adapt visible grid and snap steps": with voxel resolution `r` (voxels per block):
  `r' = r`, then while `r' > 64` divide `r'` by its smallest divisor ≥ 2; displayed step = `16 / r'`,
  mouse step = `16 / r`.

---

## 7. View background images [V]

- Three image planes, one per orthographic view, placed beyond the model as seen from the view's default
  anchor and facing it:

| View | Plane | Size | Image orientation on screen |
|---|---|---|---|
| Top | y = -32, facing up | 16 x 16 centred at (8, -32, -8) | right = +X, up = +Z |
| Side | x = 48, facing west | 16 x 16 centred at (48, 8, -8) | right = -Z, up = +Y |
| Front | z = 32, facing south | 16 x 16 centred at (8, 8, 32) | right = +X, up = +Y |

- Each plane is drawn only in its own view and only from the default side: after a side swap (Down, East,
  North) the plane faces away and is culled, so the image disappears.
- Drawn unlit, nearest-neighbour sampled, alpha blended (image alpha respected), depth tested, no depth write,
  with the transparent objects (8.6). The image is stretched to the square (no aspect correction).
- Views menu:
  - "Load background for Up view...", "Load background for West view...", "Load background for South
    view...": file dialog, then set that plane's image and show it;
  - "Load background for all views..." (Shift+A): one image for all three planes, all shown;
  - "Toggle backgrounds size" (Shift+T): all planes between 16 x 16 (start) and 48 x 48 units;
  - "Delete backgrounds" (Shift+B): hide all three; images stay in memory (a later per-view load shows only
    that view).
- The dialog is always the built-in file browser (even with the native-dialog preference), start folder = the
  textures default path captured at start-up (1.2), filters "Png image" (`.png`) and "Jpg image" (`.jpg`).
- Load failure: error box "Error while loading texture" / "An error occured while loading "<file name>".",
  blank line, message; status "Failed loading texture: <path>". QUIRK: the plane is still shown, as a plain
  white square.
- Not saved anywhere, not affected by New file, lost on exit.

---

## 8. Rendering look

### 8.1 Frame composition [V]

- Gamma colour space: textures, vertex colours and lights are used as stored 8-bit values, blending happens on
  those values (port: use non-sRGB render targets or equivalent maths).
- 8-bit colour target per view, cleared to `#343434`; MSAA 4x when "Enable anti-aliasing (fps impact)" is on,
  otherwise none; vertical sync on.
- Per view, in order: (1) alpha-tested queue (voxels); (2) transparent queue sorted back to front (elements,
  meshes, player and preview models, sub-grid planes, background planes); (3) overlay queue (gizmos, centre
  marker) drawn on top; (4) overlay lines and boxes (main grid, guides, hidden-element boxes, selection
  wireframes); (5) highlight outline composite (8.7).

### 8.2 Lights [V]

| Light | Type | Colour, intensity | Direction toward the light (internal space) | Shadows |
|---|---|---|---|---|
| main | directional | white, 0.8 | (-0.4056, 0.7071, -0.5792): from above, south-west | only when "Enable shadows for voxels (fps heavy)" is on: soft, strength 0.3, bias 0.1, normal bias 0, near plane 0.2 |
| secondary | directional | white, 0.5 | (0.5736, 0, 0.8192): horizontal, from north-east | none |

Ambient: flat colour (0.3953, 0.3953, 0.3953). No fog. Shadow settings when enabled: distance 128, two
cascades, medium resolution. The main light's `Euler` is (45, 35, 0), the secondary light's (0, 215, 0).

### 8.3 Elements, meshes and preview models [V]

All cuboid elements, imported/primitive meshes and the preview player use the same material: vertex-lit,
transparent with a depth pre-pass, back faces culled. For every vertex with normal `n` (constant per face for
cuboids):

```
lit      = A + Σ over lights i of min(2, I_i · max(0, n · l_i))      (per channel; A = ambient, I = intensity)
colour   = texture.rgb · vertexColour.rgb · lit                      (clamped by the 8-bit target)
alpha    = texture.a · vertexColour.a
```

Drawing of each object (objects in back-to-front order, 8.6):

1. depth-only pass: writes depth where `texture.a · vertexColour.a ≥ 0.5`, other texels write nothing;
2. colour pass: depth test "less or equal", no depth write, blend `src·a + dst·(1-a)`.

Textures are sampled from the editor's texture atlas with nearest filtering (model spec).

Resulting brightness of a white texel with a white vertex colour on axis-aligned faces:

| Face | Factor | 8-bit |
|---|---|---|
| up (+Y) | 0.961 | 245 |
| south (-Z) | 0.859 | 219 |
| north (+Z) | 0.805 | 205 |
| west (-X) | 0.720 | 184 |
| east (+X) | 0.682 | 174 |
| down (-Y) | 0.395 | 101 |

From the default Main camera the visible faces are up, south and west.

The lighting is evaluated per vertex in view space and interpolated; for cuboids this equals per-face shading.
The ambient and light scale factors come from the original's compiled shading program (light term halved and
doubled again, ambient term likewise) [V]; the absolute brightness must still be confirmed against a screenshot
(UNVERIFIED, section 21).

### 8.4 Element vertex colours [V]

The vertex colour carries the element colour and the tint-index preview:

- "Emulate elements tint colors" on (default): per face by its tint index: none → the element colour when "Show
  elements color" is on, else white; 0 → (106,170,64); 1 → (255,105,180); 2 → (100,149,237); 3 → (255,215,0);
  4 → (210,105,30); 5 → (72,209,204); any other index → (147,112,219). Tint colours are opaque.
- Off: every face = the element colour when "Show elements color" is on, else white; the alpha always comes
  from the element colour.
- Changing either preference recolours all elements immediately. PNG renders switch "Show elements color" off
  temporarily (15).

### 8.5 Voxels [V]

Voxel chunks use a per-pixel diffuse (Lambert) material in the alpha-tested queue (writes depth, drawn before
the transparent queue): main light per pixel with optional shadow, ambient plus the secondary light through
low-order spherical-harmonics lighting (a smooth approximation of the secondary light's diffuse term, converted
for gamma space); albedo = vertex colour (voxel colour times the ambient-occlusion level 1.0 / 0.9 / 0.8, voxel
spec). Port approximation: the 8.3 formula with the secondary light as a Lambert term (differences on side faces
of up to about 10 %, UNVERIFIED).

### 8.6 Transparency sorting [V]

Objects of the transparent queue are drawn back to front by the distance from the camera to the centre of each
object's bounding box (perspective: distance to the camera position; orthographic: distance along the view
direction). With the depth pre-pass of 8.3 the result is: texels with alpha ≥ 0.5 occlude correctly regardless
of order; texels with alpha < 0.5 blend correctly only over objects sorted behind them, so intersecting
semi-transparent elements can show order artefacts exactly like the original. The element list order (and the
Element menu's "Sort transparent elements") does not influence the viewport order; it only affects exports
(model spec).

### 8.7 Hover and selection outline [V]

- Hovered object (not selected): `#FFFFFF80`. Selected objects: `#FF6400FF`. The same in all themes.
- Look: a band about 2 pixels wide immediately outside the object's screen silhouette, drawn over everything
  (also when the object is hidden behind other geometry), no fill inside the silhouette. Adjacent highlighted
  objects show one outline around their union.
- Construction for the port (parameters taken from the original):
  1. render the silhouettes of all highlighted objects into a full-resolution RGBA mask with their highlight
     colour, ignoring depth, alpha-tested at 0.5 like 8.3;
  2. sample the mask at the four diagonal offsets `(±2, ±2)` pixels: colour = per-channel maximum of the four
     samples, alpha = sum of the four sample alphas (clamped to 1);
  3. clear the result wherever any highlighted silhouette covers the pixel;
  4. composite: `final = mix(scene, result.rgb, result.a)`.
- Exact pixel output (antialiasing of the band, corner shape) is UNVERIFIED.

### 8.8 Selection wireframe boxes [V]

When at most 4 objects are selected, each selected element and each selected primitive (not imported meshes)
also gets a wireframe of its box (12 edges plus one diagonal per face) at its position, rotation and size, drawn
on top in white at very low opacity (vertex alpha 5/255, about 2 %; UNVERIFIED visibility). Five or more selected
objects: no boxes. Not drawn during display preview.

### 8.9 Hidden elements [V]

| "Show hidden elements as white transparent box" | "Show hidden elements in main view" | Result |
|---|---|---|
| off | off | hidden elements are not drawn |
| on | either | a filled box of the element's size and rotation in faint white (same faint white as 8.8) in all four views (not during display preview) |
| either | on | hidden elements are drawn normally, but only in the Main view (8.3 shading) |

QUIRK [I]: toggling "Show hidden elements in main view" affects an element only the next time its visibility
changes (or when a file loads); already hidden elements keep their previous behaviour.

### 8.10 Markers [V]

- Centre marker ("Display marker at the center of 3D space", default off): a small axis marker at (8, 8, -8),
  translucent white (25 %), drawn in the overlay queue in all views.
- North pointer: a small arrow on the floor at (8, 0, 2), just north of the central block, pointing north;
  unlit white; drawn in Top, Side and Main.
- Blueprint: the reference cube (model spec), default at (-2, 0, 2), size 1, unlit translucent texture.
- Model-origin and bone-origin markers, gizmos and the voxel pointer: model spec. Gizmos draw in the overlay
  queue and do not write depth.

---

## 9. Minecraft display settings preview

### 9.1 Display modes [V]

The model is shown in one of nine modes. In every mode except Default the model root is attached under the
mode's **anchor** with local offset (-8, -8, 8), so the central block's centre sits on the anchor point; the
anchor receives the slot transform every frame (9.4).

| Id | Mode | Preview scene | Grid | Guides | Cinema 16:9 | Camera on first entry in a session | Anchor (internal space) |
|---|---|---|---|---|---|---|---|
| 0 | Default | normal workspace | on | on | off | the remembered default camera | world origin, offset (0,0,0) |
| 1 | Right hand 3rd | player | on | off | off | to (-20, 45, -36); pivot to the anchor; aim | (2, 12, -10) |
| 2 | Left hand 3rd | player | on | off | off | to (36, 45, -36); pivot to anchor; aim | (14, 12, -10) |
| 3 | Right hand 1st | first person | off | off | on | current camera kept | attached to the Main camera at camera-local (9, -8.35, 12) |
| 4 | Left hand 1st | first person | off | off | on | current camera kept | camera-local (-9, -8.35, 12) |
| 5 | Wall | item frame | off | off | off | to (0, 15, -25); pivot to anchor; aim | (8, 8, -9), turned 180° about Y, scale 0.5 |
| 6 | GUI | inventory slot | off | off | off | orthographic, at (8, 8, -32) looking north, orthographic size unchanged | (8, 8, -8) |
| 7 | Head | player | on | off | off | to (8, 45, -45); pivot to anchor; aim | (8, 28, -8), turned 180° about Y, scale 0.6 |
| 8 | Floor | ground | on | off | off | to (8, 45, -45); pivot to anchor; aim | (8, 8, -8), animated (9.5) |

Camera-local coordinates: x right, y up, z forward (first-person anchors share the camera's orientation).
Orientation of the two 3rd-person hand anchors: a 180° turn about the axis (0, 1, -1)/√2, i.e. anchor-local +Y
points south (-Z), local +Z points down (-Y) and local +X points west (-X); the anchors hang 10 units below the
shoulders, 2 units in front of the body (the player rig of 9.5). Head and wall anchors: 180° about +Y. GUI and
floor anchors: unrotated.

### 9.2 Switching modes [V]

Selecting a tab in the Display Settings panel sets the mode; closing the panel (or a new document) returns to
Default. Switching from mode A to mode B:

1. Save A's camera: Default stores the full Main camera state (pose and projection). Other modes keep a
   per-mode record, created on first save as a copy of the Main camera; the record's pose becomes the current
   Main camera pose with a perspective flag, except GUI, whose record always takes the fixed orthographic node
   pose (8, 8, -32) with the orthographic flag (so GUI camera changes are never remembered).
2. Switch to B.
3. Restore B's camera: Default applies its stored state. Other modes: on first entry, if the mode has a default
   camera position, the Main camera moves there, the pivot moves to the anchor and the camera aims at it; the
   record is then created as in step 1; finally the record is applied.
4. Show B's preview scene (hide the others), attach the model, set grid and guide visibility and the cinema
   frame per the table. Every mode except Default also hides the workspace root (gizmos, blueprint, markers,
   sub-grid planes and view background planes, 5.11).

A preview mode is active while the mode is not Default. Apart from the brief switch of a PNG render (15), only
a selected tab of the open panel enters one (9.3); the Preview edit mode by itself does not. While a preview
mode is active [V]:

- Blocked: all selection and manipulation input of the views: click, box and modifier selection, the rotate,
  scale and clone hold keys, move, rotate and scale drags, cloning, keyboard nudges, and the viewport clicks of
  the custom tools (the paint, voxel and colour-pick tools receive no 3D clicks; model spec 8.3, 8.7). The
  transform gizmos are hidden with the workspace root and cannot be grabbed.
- Still working: hover highlighting, camera pan, orbit and zoom, the Focus key, menu commands and keyboard
  shortcuts (no preview check was found for them; section 21).
- Not drawn: selection wireframes (8.8) and hidden-element boxes (8.9).
- A change of the orthographic Main preference is stored for Default (5.8).

Per-mode camera records live for the session only.

### 9.3 Panel open and close [V]

- The Display Settings panel belongs to the helpers group; the Preview edit mode shows it (panel sets:
  `ui_layout_theme.md` 2.6). Opening it refreshes all eight slot pages from the model, re-applies the stored
  pose angles (9.6) and, when a tab is selected, enters that tab's mode.
- Tabs: one exclusive group that cannot be switched off (clicking the selected tab keeps it selected). Tab to
  mode: Right hand 3rd 1, Left hand 3rd 2, Right hand 1st 3, Left hand 1st 4, Wall 5, GUI 6, Head 7, Floor 8
  (9.1). When the panel opens for the first time in a session no tab is selected [I]: the panel shows only its
  tab strip and the views stay in Default mode until the user clicks a tab. Afterwards the last selected tab
  stays selected across closing and reopening, and its mode is entered again at every opening.
- QUIRK [I]: at that first opening the pages are filled from the model and then reset to the control defaults
  (rotation 0, translation 0, scale 1, angle 0; export toggles keep the model's flag) by the panel's own
  initialisation. A page shows the model's values again when its tab is clicked; every later opening refreshes
  all pages correctly. Until then a hand page's copy button copies these defaults instead of the opposite slot's
  values (9.6). Port: D11.
- "Maximize main view when entering Preview mode" (default on): opening the panel maximizes the Main view if it
  was not maximized; closing it un-maximizes only if the panel had maximized it.
- Closing the panel: mode Default. The posed arms and head of the preview player keep their pose while hidden
  and are re-applied at the next opening.
- QUIRK: the original tries to disable five Edit-menu items ("Turn by +90° on Y axis", "Turn by -90° on Y
  axis", "Roll by 90° on X axis", "Optimize Faces", "Optimize Cullfaces") while the panel is open; no menu
  items carry those names (the Edit menu has "Rotate model +90° on X axis" and its Y and Z siblings), so
  nothing is disabled. Port: reproduce (no effect).

### 9.4 Slot transform [V]

Every frame the active mode's anchor gets the slot values of the current model (rotation `r` in degrees,
translation `t` in texels, scale `s`, Minecraft convention):

| Slots | Local position | Local rotation | Scale |
|---|---|---|---|
| right 3rd, right 1st, wall, GUI, head, floor | `(t.x, t.y, -t.z)` | `Rx(-r.x) · Ry(-r.y) · Rz(r.z)` | `s` |
| left 3rd, left 1st (mirrored) | `(-t.x, t.y, -t.z)` | `Rx(-r.x) · Ry(r.y) · Rz(-r.z)` | `s` |

Rotations are composed in the written order (X factor leftmost). Zero scale components count as 0.0001.
Slot mapping: right 3rd → `thirdperson_righthand`, left 3rd → `thirdperson_lefthand`, right 1st →
`firstperson_righthand`, left 1st → `firstperson_lefthand`, wall → `fixed` (item frame), GUI → `gui`,
head → `head`, floor → `ground`. The slot's export flag is ignored by the preview (disabled slots still preview).

### 9.5 Preview scenes [V]

- **Player (3rd person, head)**: classic Minecraft player of 64 x 64 skin layout with inner and outer layers,
  scale 2 (32 units tall), feet at y = 0, centred at x = 8, z = -8, facing south (toward the default
  third-person cameras). Shaded like elements (8.3), skin sampled nearest.
- **First person**: preview root attached to the Main camera at camera-local (0, 0, 12): the two hand anchors,
  a crosshair (sprite "cross", white 78 % opacity, rotated 45° about the view axis, scale 2, camera-local
  (0,0,12)) and a sky box around the camera (cube mesh scaled 100, 2048 x 1024 sky texture, unlit, tinted 0.588
  grey) that keeps a fixed world orientation while following the camera. Cinema frame on. All three are drawn in
  the Main view only.
- **Wall (item frame)**, all centred on x = 8, y = 8 and visible from the south: a 16-unit translucent white
  wall square at z = -8, a 10-unit translucent orange (25 %) frame square at z = -8.5 and the item-frame border
  texture (16 units) at z = -9; the item sits at the anchor (z = -9) at half scale.
- **GUI**: the inventory-slot frame (border sprite, 18.2 units wide) centred on (8, 8, -8), drawn in the Main
  view only; orthographic front camera.
- **Floor**: a 50 x 50 ground plane at y = -0.01 (soft blob texture, 50 % grey) and a black 25 % blob shadow at
  y = 0.01 under the item that keeps its world position (does not bob). The floor anchor animates at 50 Hz:
  `y = 8 + cos(2·t) - 7 + 4 · ground.scale.y`, x = 8, z = -8, rotation about +Y = `-60° · t` (t in seconds since
  start). With a default ground scale of 1 the item bobs between y = 4 and y = 6.

### 9.6 Display Settings panel controls [V]

Per slot (layout: `ui_layout_theme.md` 9.2): export toggle; rotation X/Y/Z sliders -180 to 180 (integers,
default 0); translation X/Y/Z -80 to 80 (continuous, default 0); scale X/Y/Z -4 to 4 (continuous, default 1);
preset list with add and remove buttons; copy buttons in the four hand panels; a pose angle on three pages
(below). Field and slider behaviour (values apply live, wheel steps of 1, typed values are not clamped, the
slider clamps itself): `ui_layout_theme.md` 16.6 and 16.7.

- Any edit opens one undo step "Edit display settings" per panel session, writes the panel values into the
  model slot (invalid number text = 0) and, when the export toggle is on, sets the model flag "use display
  settings" (never cleared here). A panel session ends when the panel is opened again.
- Copy (hand panels only): the slot receives the **opposite hand's panel values** (as currently shown, the arm
  angle included on the third-person pages), then the page refreshes. The copy itself records no undo step and
  sets no flag; when the refresh changes shown values the QUIRK below applies, and an undo step recorded that
  way already holds the copied values (undoing it does not revert the copy) [I].
- A refresh from the model never copies the page into the model. QUIRK [I]: when a refresh changes a value that
  a page shows (for example the first click on a tab of a model whose slot is not at the control defaults, 9.3,
  or reopening the panel after another file was loaded), the page's edit handler still runs: the panel
  session's undo step is recorded at that moment with unchanged values, the document becomes dirty although
  nothing was edited, and the "use display settings" flag is set when the page's export toggle is on. Port:
  D11.

**Pose angle ("Arm angle", "Head angle") [V].** A preview-only helper that poses the preview player (9.5); it
is not part of the slot transform of 9.4.

| Page | Label | Rotates | Axis |
|---|---|---|---|
| Right hand 3rd | "Arm angle" | the player's right arm | the arm's own X axis |
| Left hand 3rd | "Arm angle" | the player's left arm | the arm's own X axis |
| Head | "Head angle" | the player's head | the head's own Z axis |

The other five pages have no angle control.

- Range -180 to 180, whole numbers on the slider, default 0, degrees. The angle is absolute: the part is set
  to its rest pose and then rotated by the angle about the axis, so 0 is the rest pose. The rest pose is the
  part's pose at the first application in the session (the first panel opening). The part's outer skin layer
  and its slot anchor move with it, so the held item or the worn head model follows the arm or the head.
- Applied on every change of the field (typing, wheel, slider), at every panel opening and at every tab switch,
  each time with the angle stored for that page; nothing resets it to 0.
- Stored per slot in memory like the other values (edits write it). Choosing a preset sets it to 0 (presets
  carry no angle). It is never written to `.cubik` files (always 0 there, format spec 4.8), never exported,
  never read from any file and never saved in presets. QUIRK: New file and opening a file reset the slots
  field by field and leave the angle as it was [I for opening].
- The three third-person modes share one player, so the arm poses stay visible in the Head mode and the head
  pose in the hand modes. Closing the panel leaves the poses as they are (9.3).

### 9.7 Player skin [V, port: D2]

- Preference "Third person view with Minecraft skin:" (player name, up to 64 characters).
- At start: if `<data folder>/skin.png` exists it is used (even if the name changed since). Otherwise, empty
  name → the bundled default skin; non-empty name → download (original: `http://skins.minecraft.net/
  MinecraftSkins/<name>.png`, then `http://s3.amazonaws.com/MinecraftSkins/<name>.png`); success → cached to
  `skin.png`; both failing → status "Skin error: Not found", previous skin kept.
- Preferences OK with a changed or empty name: the cache file is deleted and the skin reloaded as above.
- Only square images are accepted; otherwise error box "Wrong Minecraft skin format" / "The Minecraft skin you
  want to use is not in right format.", blank line, "Be sure to use a skin in latest square format version."
  and the stored name is cleared. Accepted skins are copied without mip-maps, nearest filtering, and applied to
  every player mesh.
- Port: lookup only after opt-in (D2); additionally "Choose skin file..." for a local PNG; same cache file.

### 9.8 Presets [V]

- Folder `<data folder>/presets/`. When the panel is first shown in a session, the eleven bundled presets are
  copied there as `<name>.json` unless a file of that name exists (never overwritten), then every `*.json` in
  the folder is read. Log "DisplaySettings: Loaded <N> presets".
- Routing by **file name** prefix: `hand3rd` → both third-person panels; `hand1st` → both first-person
  panels; `head`, `floor`, `wall`, `gui` → their panel. Other prefixes are counted but unused. Within a panel
  presets are keyed by **preset name** (a later preset with the same name replaces an earlier one), sorted by
  name (stable), with a built-in "Default" first (identity values, export off, not removable).
- Bundled presets (all not removable; their files spell the export key `enable`, which is ignored, so they load
  with export on):

| File | Family | Name | Rotation | Translation | Scale |
|---|---|---|---|---|---|
| `hand3rd_block` | 3rd person | Vanilla block | 75, 45, 0 | 0, 2.5, 0 | 0.375 |
| `hand3rd_left_item` | 3rd person | Vanilla item in left hand | 0, 90, -55 | 0, 4.0, 0.5 | 0.85 |
| `hand3rd_right_item` | 3rd person | Vanilla item in right hand | 0, -90, 55 | 0, 4.0, 0.5 | 0.85 |
| `hand1st_left_block` | 1st person | Vanilla block | 0, 225, 0 | 0, 0, 0 | 0.40 |
| `hand1st_left_item` | 1st person | Vanilla item in left hand | 0, 90, -25 | 1.13, 3.2, 1.13 | 0.68 |
| `hand1st_right_block` | 1st person | Vanilla block | 0, 45, 0 | 0, 0, 0 | 0.40 |
| `hand1st_right_item` | 1st person | Vanilla item in right hand | 0, -90, 25 | 1.13, 3.2, 1.13 | 0.68 |
| `floor_block` | floor | Vanilla block | 0, 0, 0 | 0, 3, 0 | 0.25 |
| `floor_item` | floor | Vanilla item | 0, 0, 0 | 0, 2, 0 | 0.5 |
| `wall_block` | wall | Vanilla block | 0, 0, 0 | 0, 0, 0 | 0.5 |
| `gui_block` | GUI | Vanilla block | 30, 225, 0 | 0, 0, 0 | 0.625 |

  Scale values apply to all three axes. QUIRK [I]: the two first-person "Vanilla block" presets share a name,
  so both first-person panels list a single "Vanilla block", the one read last (the right-hand file on a
  normal alphabetical folder listing).
- Selecting a preset copies its values into the panel (export toggle = the preset's `enabled`), then behaves
  like an edit (9.6).
- Add: text prompt titled "New preset name", default "Custom". Saves the panel values as a removable preset
  with export on; replaces a same-named entry; selects it; writes `<prefix>_<sanitized>.json` where sanitized =
  name lower-cased, spaces turned into underscores, then every character outside `a-z0-9` removed (the
  underscores too). Collisions overwrite.
- Remove: nothing selected → nothing; protected → info box "<name> protected" / "Sorry but the preset
  "<name>" can't be removed.", blank line, "You can only delete presets you setup."; else confirmation
  "Delete confirmation" / "Are you sure you want to delete preset "<name>"?" with "Delete" and "Cancel"; Delete
  removes the entry, clears the selection and deletes the file.
- Preset file JSON (indented, 2 spaces): `name`, `canBeRemoved`, `enabled`, `rotation`, `translation`,
  `scale` (3-number arrays). Missing on read: name null, canBeRemoved true, enabled true, vectors zero (a
  missing scale becomes 0.0001 on all axes).
- Port: same folder layout under the port's data folder; on first run offer to import an existing Cubik presets
  folder.

---

## 10. Preferences and persisted state

### 10.1 Settings file [V]

- File `cubikdata.json` in the data folder (original: `%USERPROFILE%\AppData\LocalLow\BDcraft\Cubik\`).
  One compact (unindented) JSON object with the 54 keys of 10.2; strings that were never set are written as
  `null` (a file written before the first Preferences OK contains `"minecraftSkin":null`). Key order when
  written: `signatureTemplate`,
  `useNativeFileBrowser`, `showHiddenFiles`, `readdDefaultFavourites`, `backupEveryMinutes`, `windowX`,
  `windowY`, `openElementCreate`, `openElementTools`, `openElementsList`, `openElementsMaterial`,
  `openVoxelTools`, `openGizmosTools`, `openMeshCreate`, `openMeshTools`, `openMeshesList`, `openPaintTools`,
  `openColorTools`, `openPalette`, `renderHiddenElements`, `renderHiddenElementsMainView`,
  `maximize3dViewOnPreview`, `showClassicTooltips`, `minecraftSkin`, `appSkin`, `pathLoadSave`, `pathImport`,
  `pathExport`, `pathTextures`, `pathMeshes`, `pathMinecraft`, `pathTextureEditor`, `minecraftExportTextures`,
  `objCenterOnImport`, `sketchfabToken`, `formatHideMinecraft`, `formatHideStonehearth`, `formatHideStaxel`,
  `formatHideVoxel`, `formatHideMesh`, `advAtlasPadding`, `advAtlasSquare`, `advKeysRotate`, `advKeysScale`,
  `advKeysClone`, `advKeysPan`, `advKeysFocus`, `mainCamOrtho`, `useElementsColor`, `emulateElementTintColor`,
  `centerPoint`, `renderShadows`, `renderAntialiasing`, `recentFiles` (written as
  `{"max":10,"list":[{"name":...,"path":...}, ...]}`; a different stored `max` is honoured).
- Read once, on first use during start-up (1.2 step 5):
  - file missing, empty or the literal `null`: all defaults, no message;
  - a key missing from the file: that key's default (10.2); unknown keys are ignored;
  - malformed JSON, `null` in any of the 37 boolean and integer keys, or a value that cannot be converted to
    its key's type (for example `"abc"` for an integer; numbers written as strings are accepted): the whole
    file is rejected: error box titled "Exception" with the full exception text, all defaults, nothing partly
    loaded; the rejected file is overwritten without a copy at the next write (port: D10);
  - `null` in a string key or in `recentFiles`: accepted, see the table below.
- Effects of loading: the setters of six settings run as their keys are read: `mainCamOrtho` switches the Main
  projection (5.8); `useElementsColor` and `emulateElementTintColor` recolour all elements when the value
  changes; `centerPoint`, `renderShadows` and `renderAntialiasing` switch the centre marker, the voxel shadows
  and MSAA. The theme, the player skin and the panel set are applied by their own start-up steps (1.2 steps 6,
  13 and 15), the window position as in 1.2 step 5. QUIRK: the five key binding strings are only stored; the
  built-in bindings stay active until the next Preferences OK applies the strings (port: D9).
- `null` string values:

| Key(s) | Effect of `null` |
|---|---|
| `minecraftSkin` | as empty: bundled skin, no lookup (9.7); Preferences OK stores the field text (`""` when empty) |
| `appSkin` | no theme is applied at start-up (the original keeps the colours built into its interface; colour lookups such as outline colours use "Light"); Preferences shows "Light", and OK stores and applies it (port: D10, "Light") |
| `signatureTemplate` | Preferences shows an empty field and OK stores its text; until then a new document's default signature cannot be computed, so the first action that needs it (Model Properties, native save, exports that embed the signature, Sketchfab upload) fails with an internal error [I] (port: D10, default template) |
| `pathLoadSave`, `pathImport`, `pathExport`, `pathTextures`, `pathMeshes` | file dialogs and texture folder lookups start on the Desktop; Preferences shows an empty field (browsing starts at the root); stays `null` until OK with an existing path (10.3) |
| `pathMinecraft` | counts as a folder that does not exist: the file browser favourite uses the detected folder (17.3) |
| `pathTextureEditor`, `sketchfabToken` | as empty: box "Missing Texture editor" when a texture is sent to the external editor (model spec); missing-token box (16) |
| `advKeysRotate`, `advKeysScale`, `advKeysClone`, `advKeysPan`, `advKeysFocus` | Preferences shows the live bindings; OK stores and applies them |
| `recentFiles` | empty list with `max` 10 |

- Written: Preferences OK (all platforms) and quit (original: Windows only), always the whole object; the
  window position is read into `windowX`/`windowY` just before each write. Success shows the status
  "Successfuly saved file: <path>"; a write error shows the box "Error while saving file" / "An error occured
  while saving "cubikdata.json".", blank line, message, and the status "Failed saving file: <path>". Recent
  files are saved only then, so a crash loses recent-file changes of the session.
- Port: own settings location; on first run it imports an existing `cubikdata.json`; it writes its own file
  but keeps the key names and value formats for round trips. A `cubikdata.json` written for the original uses
  compact JSON, the key order above and `null` for strings that were never set.

### 10.2 Preferences dialog: every setting [V]

Tools > "Preferences...". Tabs "General", "Display", "Workspace", "Paths", "Formats", "Advanced"; buttons "OK"
and "Cancel" (layout: `ui_dialogs_menus.md` 9.1).

"Default" is the value of a fresh settings file; a key missing from an existing file takes the same value
(10.1).

| Tab / group | Label | Key | Type | Default | Effect | Applied |
|---|---|---|---|---|---|---|
| General / Signature | (template input, max 256) | `signatureTemplate` | string | "Designed by %author with %cubik - https://cubik.studio" | default model signature/description; `%author`, `%email`, `%cubik` replaced (help text lists "<key> will be replaced by <value>"). Port: `%author`/`%email` from an optional user profile setting (empty by default), `%cubik` = {App} | OK |
| General / Backup | Delay between backups (minutes): | `backupEveryMinutes` | int (max 6 digits) | 10 | backup interval (12.1); OK stores max(5, value); loading does not clamp | next cycle |
| General / Backup | Backup folder: (info) | — | — | `<data folder>/backup/` | | |
| General / File browser | Use Native file browser if possible | `useNativeFileBrowser` | bool | false | OS dialogs for open/save/export, built-in browser as fallback; also removes the "Windows Shortcut" import format | immediately |
| General / File browser | Show hidden files in Cubik file browser | `showHiddenFiles` | bool | false | built-in browser lists hidden entries | next dialog |
| General / File browser | Re-add default favourites in Cubik file browser on launch | `readdDefaultFavourites` | bool | false | default favourites re-added at every launch (17.3) | next launch |
| Display / Render | Use orthographic projection for main view | `mainCamOrtho` | bool | false | 5.8 | immediately |
| Display / Render | Show elements color | `useElementsColor` | bool | true | 8.4 | immediately |
| Display / Render | Emulate elements tint colors | `emulateElementTintColor` | bool | true | 8.4 | immediately |
| Display / Render | Display marker at the center of 3D space | `centerPoint` | bool | false | 8.10 | immediately |
| Display / Render | Show hidden elements as white transparent box | `renderHiddenElements` | bool | false | 8.9 | immediately |
| Display / Render | Show hidden elements in main view | `renderHiddenElementsMainView` | bool | false | 8.9 (QUIRK there) | on visibility change |
| Display / Render | Enable anti-aliasing (fps impact) | `renderAntialiasing` | bool | false | MSAA 4x | immediately |
| Display / Render | Enable shadows for voxels (fps heavy) | `renderShadows` | bool | false | main light soft shadows (8.2) | immediately |
| Display / Preview | Maximize main view when entering Preview mode | `maximize3dViewOnPreview` | bool | true | 9.3 | next panel open |
| Display / Preview | Third person view with Minecraft skin: | `minecraftSkin` | string (max 64) | null | 9.7 | OK |
| Display / UI | Skin theme: | `appSkin` | string | "Light" | theme; the list offers "Light" and "Dark" (a third theme "Rainbow" exists in the data and is accepted from the file); a `null` or unknown stored name applies no theme at start-up (10.1) | OK |
| Workspace | Create element (cuboid) | `openElementCreate` | bool | true | panels of the custom panel set (shown when no edit mode is selected; `ui_layout_theme.md` 2.6) | OK |
| Workspace | Tools for elements | `openElementTools` | bool | false | same | OK |
| Workspace | Elements list | `openElementsList` | bool | false | same | OK |
| Workspace | Elements material | `openElementsMaterial` | bool | false | same | OK |
| Workspace | Tools for voxels | `openVoxelTools` | bool | false | same | OK |
| Workspace | Gizmos options | `openGizmosTools` | bool | false | same | OK |
| Workspace | Create primitive (mesh) | `openMeshCreate` | bool | false | same | OK |
| Workspace | Tools for meshes | `openMeshTools` | bool | false | same | OK |
| Workspace | Meshes list | `openMeshesList` | bool | false | same | OK |
| Workspace | Tools to paint on surfaces | `openPaintTools` | bool | false | same | OK |
| Workspace | Color selector | `openColorTools` | bool | false | same | OK |
| Workspace | Color palette | `openPalette` | bool | false | same | OK |
| Paths | Load/Save default path | `pathLoadSave` | string | Desktop folder | start folder of Open, Merge, Save As | OK |
| Paths | Import default path | `pathImport` | string | Desktop | start folder of Import | OK |
| Paths | Export default path | `pathExport` | string | Desktop | start folder of Export | OK |
| Paths | Textures default path | `pathTextures` | string | Desktop | texture dialogs; background images (captured at start-up, 1.2) | OK |
| Paths | Meshes default path | `pathMeshes` | string | Desktop | custom mesh dialogs (model spec) | OK |
| Paths | Minecraft | `pathMinecraft` | string | detected Minecraft folder, or "" (17.1) | file browser favourite (17.3) | OK |
| Paths | Texture editor | `pathTextureEditor` | string | "" | external texture editor executable (model spec) | OK |
| Formats / Minecraft | Save textures as png on export (beside .json) | `minecraftExportTextures` | bool | false | Minecraft JSON export (format spec) | next export |
| Formats / OBJ (Polygons) | Center model and origin on import | `objCenterOnImport` | bool | true | OBJ import (format spec) | next import |
| Formats / Sketchfab | (token input, letters and digits only) | `sketchfabToken` | string | "" | 16 | OK |
| Formats / Import/Export dialog | Hide Minecraft formats | `formatHideMinecraft` | bool | false | removes `json`, `mcpe`, `js`, `jpm`, `java`, `zip` formats entirely | immediately |
| same | Hide Stonehearth formats | `formatHideStonehearth` | bool | false | removes `stonehearth` | immediately |
| same | Hide Staxel formats | `formatHideStaxel` | bool | false | removes `staxel` | immediately |
| same | Hide Voxel formats | `formatHideVoxel` | bool | false | removes `binvox`, `csv`, `qb`, `qef`, `schematic`, `nbt`, `vox`, `zox` | immediately |
| same | Hide Mesh formats | `formatHideMesh` | bool | false | hides `obj` and `stl` from dialog lists only (still opened by extension) | immediately |
| Advanced / Atlas | Add padding between textures (bigger texture size) | `advAtlasPadding` | bool | false | atlas generation (model spec) | next atlas |
| Advanced / Atlas | Always generate a square texture | `advAtlasSquare` | bool | true | atlas generation | next atlas |
| Advanced / UI | Show classic tooltips on mouse hover | `showClassicTooltips` | bool | true | tooltips (`ui_layout_theme.md` 10.1) | immediately |
| Advanced / Keyboard shortcuts | Rotate mode | `advKeysRotate` | string | "Alpha1, Keypad1" | hold keys for rotate mode (model spec) | OK |
| same | Scale mode | `advKeysScale` | string | "Alpha2, Keypad2" | hold keys for scale mode | OK |
| same | Clone mode | `advKeysClone` | string | "Alpha3, Keypad3, LeftAlt, RightAlt" | hold keys for clone mode | OK |
| same | Pan view | `advKeysPan` | string | "Mouse2, Space" | 5.5(a) | OK |
| same | Focus view | `advKeysFocus` | string | "F" | 5.5(f) | OK |
| (none) | window position | `windowX`, `windowY` | int | 0, 0 | 4.2 | quit/OK |
| (none) | recent files | `recentFiles` | object | `{ "max": 10, "list": [] }` | 11 | quit/OK |

Key strings: comma-separated key names using the engine's key code names (`Alpha1`, `Keypad1`, `LeftAlt`,
`Mouse2`, `Space`, `F`, ...), at most four keys (the first four are used), parsed case-insensitively, numeric
codes accepted, an unknown name leaves its slot empty; an empty string restores the binding's default. On OK
the stored string is re-written in canonical form (canonical names, unset slots skipped; QUIRK: when the first
slot is unset the text starts with ", "). Input placeholder "Key names separated by comma". The rotate-view
modifier, zoom and arrow keys are not configurable.

QUIRK: the stored key strings are applied only by Preferences OK, never at start-up. After every restart the
built-in bindings are active (the same keys as the default strings; the built-in clone binding lists its last
two keys as "RightAlt, LeftAlt") until the user presses OK in Preferences, which applies the strings the dialog
shows (port: D9).

Start folders of the file dialogs: Save As starts in the folder of the document's save path, and Export in the
folder of the document's export path, when that folder exists (with that file name); otherwise in the
preference folder; a `null` or empty preference folder means the Desktop (`formats_interop.md` 5 and 6 own the
dialogs).

### 10.3 Dialog behaviour [V]

- Non-modal; Escape cancels; Enter does nothing. Opening loads a snapshot of the live settings into the controls:
  `null` texts show an empty field, a `null` theme shows "Light", and an empty or `null` key string shows the
  binding's live keys in canonical form.
- Path browse buttons ("..."): folder pickers titled "Select default folder to load/save CUB", "Select default
  folder to import files", "Select default folder to export files", "Select default folder to load/save
  textures", "Select default folder for custom meshes" and "Select folder to Minecraft"; they always use the
  built-in browser in folder mode and start at the field's text (the root "/" when it is empty). The Texture
  editor button opens an executable picker that is always titled "Select executable file" (a caption naming the
  texture editor is not used), with the single filter "Executable" (`exe`), starting in the folder that contains
  the field's path; it uses the native dialog when that preference is on. The choice only fills the field.
- OK: copies all controls into the live settings in the table order, refreshes the custom panel set, applies
  side effects, key bindings, skin and theme, writes the settings file, closes. Path fields are stored only if
  the text names an existing file or folder; otherwise the old value stays silently.
- Cancel / close / Escape: closes; nothing changes.

### 10.4 Other persisted state [V]

| Storage | Key / file | Use |
|---|---|---|
| per-user preference store (Windows registry `HKCU\Software\BDcraft\Cubik`) | `cubikFilebrowser` (int) | set to 1 after the default favourites were added |
| same | `CubikFileBrowser` (string) | built-in file browser quick-access store (`ui_dialogs_menus.md` 10.10) |
| same | `minecraftPath` (string) | read-only override of the Minecraft folder detection (17.1); nothing in the app writes it |
| same | `managersWidth`, `helpersWidth` (int) | written on side-panel resize but read back as `ManagersWidth`/`HelpersWidth`; the store files each value under a name derived from the exact key spelling (the registry's case-insensitivity does not merge them), so the written values are never found and start-up always uses 220 / 350 (QUIRK, `ui_layout_theme.md` 2.7) |
| same | `ScreenWidth`, `ScreenHeight` | written, never read (4.2) |
| data folder | `cubikdata.json`, `skin.png`, `cubik_format.ico`, `presets/`, `backup/` | 10.1, 9.7, 3.3, 9.8, 12 |
| temporary cache folder | `SketchfabExport/`, `pack.png` | 16; resource-pack export (format spec) |

Not persisted: view background images, wireframe mode, maximize state, view anchors, display mode and its
camera records, the orbit pivot, panel visibility.

Port: one settings file in the port's configuration folder for all of the above that it keeps; the legacy
locations are read once for migration.

---

## 11. Recent files [V]

- Stored in the settings file as `recentFiles` = `{ "max": 10, "list": [ { "name": <file name>, "path":
  <full path> }, ... ] }`, oldest first.
- An entry's name is the file name with extension. Entries are identified by name: adding a file whose name
  matches an entry replaces that entry (and moves it to the end), even when the folders differ (QUIRK).
  Port: identify by full path, display the file name (and the parent folder for duplicates).
- Added (pushed to the end, oldest dropped beyond 10): when Open, Merge, Import, drag and drop, the command line
  or the recent menu start loading a file (before the load, so failed loads are listed too), except `.lnk`
  files; after a native save or an export in a format the application can also read. Never by backups.
- Menu File > "Open recent files": a submenu listing the names newest first (the reverse of the stored order,
  so the most recently added file is the top row), then a separator and "Clear recent file list"; disabled
  when the list is empty.
- Clicking an entry: if the file exists, the unsaved-changes prompt (2.2) then an open with replace semantics
  (as File > Open, without the dialog); the entry moves to the end of the list (the top of the submenu).
  If the file is missing: info box "File not found" / "File <name> can't be found at:", new line, "<path>";
  the entry is removed and the submenu rebuilt.
- "Clear recent file list": empties the list and rebuilds the submenu.
- QUIRK: in a session started without a settings file the entries do nothing when clicked (D7).

---

## 12. Automatic backup and the Recover menu

### 12.1 Backup loop [V]

- Starts with the application. Each cycle logs "Backup: Next backup in <N> minutes" (N = current setting, not
  clamped), waits `max(300, N · 60)` seconds, then writes a native `.cubik` copy of the current document to
  `<data folder>/backup/<Weekday>_backup.cubik` (`Monday` ... `Sunday`, English names), overwriting last week's
  file of that day.
- Runs regardless of the dirty state, also for empty documents. The usual save progress texts appear in the
  status line ("Saving properties..." ... "Saving..."), then "Backup: <path>".
- Does not change the document path, title, dirty state or recent files. Errors: "Error while saving" box with
  "Abort" (format spec).
- A changed interval takes effect after the current wait.

### 12.2 Recovery [V]

There is no crash-recovery prompt. Users recover by opening a backup file; the backup folder is a default
favourite of the built-in file browser and is shown in the Preferences (General / Backup).
Port: same, plus D8 atomic writes; optionally skip a backup when nothing changed (not parity, off by default).

### 12.3 Recover menu [V]

Menu "Recover" (also mirrored by the Recover tool panel, `ui_layout_theme.md` 7.3). Each item records an undo
step:

| Item | Shortcut | Undo label | Effect |
|---|---|---|---|
| Show all Elements | Ctrl+Shift+S | "Show all" | un-hide every element and mesh |
| Unlock all Elements | Ctrl+Shift+U | "Unlock all" | unlock every element and mesh |
| Reset Blueprint to default | Ctrl+Shift+P | "Reset blueprint" | blueprint position (-2, 0, 2) internal, rotation (0,0,0), size (1,1,1) |
| Reset Views to default | Ctrl+Shift+C | "Reset camera" | 5.7 |
| Get back Far Elements | Ctrl+Shift+F | "Get back far elements" | every element whose start corner `p` or opposite corner `(p.x+s.x, p.y+s.y, p.z-s.z)` lies outside the editing volume is moved by the smallest per-axis offset that puts its bounding box inside (per axis: below the minimum → shift up by the difference; else above the maximum → shift down). Meshes are not moved. |

---

## 13. Drag and drop [V, port: D6]

- Files dropped anywhere on the main window: only the first file is used (log "Dropped file: <path>"); it is
  opened with replace semantics through the normal open path: extension validation and its error boxes,
  recent-files update, "Loading..." box for large files.
- Original: no unsaved-changes prompt, so unsaved edits are lost (QUIRK; port D6 adds the prompt).
- Import formats are imported with the same replace flag as File > Import (format spec).
- The original supports this only on Windows; the port supports it on every platform.

---

## 14. Clipboard [V]

- Cut, Copy and Paste (Edit menu, model spec) use an internal in-process clipboard (objects, voxels, display
  data). It is not shared with the OS or other instances and is emptied by New file and replace-open.
- There is no image paste from the OS clipboard and no copy of renders to the OS clipboard (the bundled native
  helper contains clipboard-image functions that the application never calls). Port: same; an OS clipboard
  bridge would be an extension, not parity.

---

## 15. Render to PNG ("Model Render") [V, background UNVERIFIED]

- File > Export with the format "Model Render" (`.png`). Options in Model Properties, tab "PNG": "Render type"
  radio group "GUI display preview" (default) / "Main view"; "Options": "Anti-aliasing" (default on); info text
  "Exporting to PNG will produce a render of the model saved to a picture file encoded in PNG 24bits + alpha."
  (Stored model flags `png.gp`, `png.mv`, `png.aa`; only `mv` and `aa` influence the render.)
- **GUI display preview**: 512 x 512 pixels. The display mode is switched to GUI for the render (the model is
  posed by its `gui` slot under the GUI anchor and the Main camera takes the GUI orthographic pose, 9.2); an
  offscreen camera copies the Main camera's pose and projection and its orthographic half-height is then set
  to 8, so exactly the 16-unit slot fills the image.
- **Main view**: 1920 x 1080 pixels. The display mode is switched to Default for the render, so the offscreen
  camera copies the Default mode's Main camera (the current pose when no preview is active) with its
  projection (vertical field of view 64 or its orthographic size). The 16:9 aspect is used regardless of the
  viewport's aspect.
- The offscreen camera copies only position, rotation, field of view, the orthographic flag and the
  orthographic size from the Main camera; clip planes (near 0.3, far 128), culling and clear settings are its
  own.
- **Anti-aliasing**: rendered at twice the size, then downscaled by averaging 2 x 2 blocks of straight
  (non-premultiplied) RGBA. QUIRK: the blocks are shifted by one source pixel toward the image's left and bottom
  edges: output pixel (x, y), counted from the bottom-left, averages source columns 2x-1 and 2x and rows 2y-1
  and 2y; the first output column and row use only the samples that exist, and the last source column and row
  are never read (whether the half-pixel shift is visible is UNVERIFIED). Over an empty, transparent black
  background, silhouette edge pixels come out darker and more transparent.
- Content: model content only (elements, meshes, voxels); no grid, guides, gizmos, markers, blueprint,
  backgrounds, preview decorations or outlines: the workspace node is hidden for the capture and the offscreen
  camera only draws the scene layer that holds the model (the GUI slot frame and the other preview decorations
  are on other layers). "Show elements color" is switched off for the render (tint emulation still applies),
  then restored. Lights and shading as in the views. QUIRK: in Main view renders anything farther than 128
  units from the camera is clipped.
- Background UNVERIFIED: the offscreen camera clears only depth (its stored clear colour, magenta, is never
  used), and nothing else clears the freshly created render target, so pixels that no model geometry covers
  keep the target's initial contents. These are expected to be transparent black (0, 0, 0, 0) on common
  drivers. With such a background, because colour blending also blends alpha, a texel of alpha `a` ends with
  alpha `a²` (QUIRK [I]).
- The four views are not drawn while rendering (two frames); the previous display mode is restored afterwards
  (which re-applies that mode's grid, guide and cinema settings, 9.2).
- Output: 8-bit RGBA PNG; `.png` appended if missing. Status "Exporting PNG render finish...", then "Exported
  PNG: <path>". Write error: "Error while saving file" / "An error occured while saving texture "<file name>".",
  blank line, message. Not added to recent files.
- Also used by the Minecraft resource-pack export for `pack.png`: 128 x 128, GUI display preview, twice the size
  then downscaled (format spec).
- Port: offscreen wgpu target of the stated size (x2 for anti-aliasing), cleared explicitly to transparent black
  (0, 0, 0, 0) before drawing, same culling rules, straight (non-premultiplied) alpha in the PNG; the targets are
  released after each render (the original never releases them).

---

## 16. Sketchfab upload (high level) [V, port: D3]

1. Tools > "Share on Sketchfab". Without a token: a Sketchfab-icon message box (kind "Sketchfab",
   `ui_dialogs_menus.md` 8.3) "Missing Sketchfab token" / "To share your model on Sketchfab, you must add your
   Sketchfab API token in Cubik Preferences > Formats > Sketchfab field...", blank line, "Then retry." (port:
   own product name in the path text).
2. Wait box "Exporting to Sketchfab" / "Your model is compiled for Sketchfab.", blank line, "Please wait...".
3. If nothing visible exists: error "Model not exported..." / "Please, make sure at least one element, one
   voxel, one mesh or one primitive exists before exporting." (blank line between).
4. The model is exported as OBJ with MTL and PNG textures (current OBJ settings of the model, format spec) into
   `<temporary cache folder>/SketchfabExport/` (emptied first; base name = model name with all whitespace
   removed); all files are zipped into `CubikModel.zip` in that folder.
5. Upload: title = model name; description = model description + blank line + "Made with {App} <version>";
   tags "cubik lowpoly voxel"; source "cubik-studio"; published; not private. Status "Uploading Sketchfab
   export (<sent>/<total>)..." with sizes as whole numbers of B, KB, MB or GB (base 1024, rounded down) and the
   progress bar.
6. Success: Sketchfab-icon box "Model on Sketchfab" / "Your model is ready on your Sketchfab account!", blank
   line, "Do you want to see it online now?" with "Show me" (opens the model page) and "Later".
7. Failure: error box "Ohoh..." with the error text and buttons "OK", "Go to Sketchfab" (opens the Sketchfab
   dashboard) and "Cancel".

Packaging and write errors show "Error while packaging" / "An error occured while preparing archive to upload."
or "Error while saving" / "An error occured while saving mesh to disk." followed by the message.
Port: current Sketchfab upload API (D3), asynchronous with cancel and timeout, temporary files removed after
the upload; the token is stored in the OS credential store when available (otherwise in the settings file like
the original).

---

## 17. Minecraft and game folders

### 17.1 Minecraft folder detection [V]

Computed once per session, first match wins:

1. per-user preference value `minecraftPath` (as stored);
2. `%APPDATA%\.minecraft\`;
3. `<Program Files (x86)>\Minecraft\`;
4. `<Program Files>\Minecraft\`;
5. `<profile>\.minecraft\` (profile = parent of the Music folder, else the Documents folder);
6. `<profile>/Library/Application Support/minecraft/` (macOS layout);
7. none (empty).

The result is the default of the "Minecraft" path preference.

### 17.2 Other game folders [V]

- Steam folder: registry `HKCU\Software\Valve\Steam`, value `SteamPath` (Windows only), if it exists.
- Staxel: `%LOCALAPPDATA%\Staxel.Launcher\gamedata\content\staxel\`, else `<Steam>\steamapps\common\Staxel\`.
- Stonehearth: `<Steam>\steamapps\common\Stonehearth\`.

### 17.3 Uses [V]

- Default favourites of the built-in file browser (only existing folders, in this order): Desktop, Documents,
  Pictures, the Minecraft path preference (or the detected folder when the preference does not exist on disk),
  Staxel, Stonehearth, the install folder, the backup folder. Added at the first launch and at every launch while
  "Re-add default favourites..." is on.
- Path field placeholders of the built-in file browser: `%minecraft%` (detected folder, not the preference),
  `%staxel%`, `%stonehearth%`, `%steam%`, `%backup%`, `%app%` (install folder), `%data%` (data folder),
  `%desktop%`, and OS environment variables (`ui_dialogs_menus.md` 10.11) [I: reached through the path input
  field].
- A "browse resource packs folder" action exists in the original but is not wired to any menu; not ported.
- Port: same detection on Windows; on Linux `~/.minecraft/`, on macOS `~/Library/Application Support/minecraft/`.

### 17.4 Folder shortcuts in the Tools menu [V]

- "Browse Cubik Folder": opens the install folder in the OS file manager.
- "Browse Cubik Data Folder": opens the data folder.
- "Browse last save/export Folder": opens the folder of the path last handed to the save routine **in this
  session**: File > Save of a document that has a path, Save As, Export, and the "Yes" choice of the
  unsaved-changes prompt (2.2). Open, Merge, Import, recent files, drag and drop, the command line and
  automatic backups do not change it; a save or export that fails after its format was accepted still counts.
  Kept in memory only: lost at exit, not reset by New file.
- Before the first save or export no folder is known. The original then does nothing on Windows; on macOS and
  Linux the click ends in a logged internal error with nothing visible. Port: do nothing, without an error.

---

## 18. Network, updates, licensing

### 18.1 Original behaviour (reference only) [V]

- At start-up and again later, the original sends account e-mail, serial, a device fingerprint and a time
  stamp to the vendor's licence server; the reply includes the latest version number. When a newer version
  exists it offers "Update" (starts the bundled updater program from the install folder and quits), "Open page"
  (opens a server-provided address) and, depending on a server flag, "Quit". After an update a one-time "what's
  new" box appears. A failed time check quits the application.
- Other network use: skin download (9.7), Sketchfab upload (16), Help links in the browser.

### 18.2 Port policy

- No licence, registration, account, telemetry or update request (D1, D5). The Help menu's "Registration..."
  item is dropped.
- Network access only after explicit user action or opt-in: Sketchfab upload (the user action is the consent),
  skin lookup (opt-in, D2), optional update check (opt-in, D5: shows "A new version is available" with "Open
  page" and "Later").
- Help menu items open the port's own pages in the browser: "Support..." (F1), "Details...", "Changelog...",
  "About..." (dialog). Original targets for reference: `https://cubik.studio/support`,
  `https://cubik.studio/features`, a server-provided changelog address, `http://BDcraft.net`.

---

## 19. Status line and application messages

### 19.1 Info cell [V]

Actions write a message; the cell reverts to "Ready" 10 seconds (±1 s) after the last message (each new message
restarts the countdown). The scene's initial placeholder is "Loading..." (UNVERIFIED whether it is visible at
start-up before the first message).

### 19.2 Details cell [V]

With a selection: the primary object's name, followed by " #<n>" when its index is greater than 0. Without:
"<N> element" + "s" when N > 1, then " (<M> face" + ")" or "s)" when M > 1; for example "0 element (0 face)",
"3 elements (14 faces)". (The axis and rotation cells: model spec.)

### 19.3 Messages owned by this document

| Situation | Text |
|---|---|
| startup with a file argument | "Please wait..." (original) |
| backup written | "Backup: <path>" |
| background load failed | "Failed loading texture: <path>" |
| skin not found | "Skin error: Not found" |
| texture saved (icon on first run, PNG render) | "Successfuly saved file: <path>" / "Successfuly saved texture: <path>" |
| settings file written (Preferences OK, quit) | "Successfuly saved file: <path>"; on a write error "Failed saving file: <path>" (10.1) |
| PNG render | "Exporting PNG render finish...", "Exported PNG: <path>" |
| Sketchfab upload | "Uploading Sketchfab export (<sent>/<total>)..." |
| native save progress | "Saving properties...", "Saving elements...", "Saving textures...", "Saving voxels...", "Saving meshes...", "Saving model display...", "Saving..." |
| save/export start (shown briefly) | "Loading <EXT>: <path>" for native saves, "Importing <EXT>: <path>" for exports (QUIRK: wording inverted) |
| save/export done | "Saved <EXT>: <path>", "Exported <EXT>: <path>" |
| open done | "Loaded <EXT>: <path>", "Imported <EXT>: <path>" |

`<EXT>` is the extension in upper case without the dot. Message boxes for file open/save validation (missing
extension, unknown type, cannot read/write) and the "Loading..." box: `formats_interop.md` 5 and 6 and
`ui_dialogs_menus.md` 10.

---

## 20. Feature parity checklist

Tick each item when implemented and verified against the original (or against this spec where marked
UNVERIFIED).

**Start-up and shut-down**
- [ ] First launch: 1000 x 700 resizable window, empty document "Cubik Model", title "{App} - unsaved" (1.1, 4.1)
- [ ] Settings load: missing keys take the 10.2 defaults, `null` strings handled per 10.1, rejected-file error box
  (or D10), side effects on load (1.2, 10.1)
- [ ] Stored key bindings applied at start-up (D9; original behaviour behind the compatibility switch) (10.2)
- [ ] Window position restore with Left Alt bypass and 0/±30000 guards (1.2, 4.2)
- [ ] Full reset state of a new document; cameras/backgrounds/wireframe kept (1.3)
- [ ] Command-line file opens after start-up (3.2)
- [ ] Quit with unsaved-changes prompt Yes/No/Cancel semantics (2.1, 2.2)
- [ ] Settings written on quit and on Preferences OK; compact JSON in the original key order on export (2.1, 10.1)
- [ ] Multiple instances allowed (3.1)
- [ ] `.cubik` association via installer/explicit action (3.3, D4)

**Main window**
- [ ] Title rules (4.1)
- [ ] Minimum size 900 x 700 x UI scale (4.2)
- [ ] UI scale formula and clamp (4.3)
- [ ] Alt+Enter / F11 full screen (4.4)
- [ ] Reduced redraw when unfocused (4.5)
- [ ] Views blocked under dialogs; shortcuts suppressed while the modal flag is set, built-in browser included (4.6)

**Views and cameras**
- [ ] Four views with projections, poses, clip planes, clear colour (5.1)
- [ ] 4-up quadrants and per-view viewports (5.2)
- [ ] Label left click: maximize toggle, exclusive (5.2, 5.3)
- [ ] Label right click: side swap with axis-keeping rule; Main quirk (5.3)
- [ ] Hover and active-view latching during drags (5.4)
- [ ] Orthographic pan with anchor sign vectors (5.5 b)
- [ ] Main view pan 0.1 unit/px with pivot (5.5 c)
- [ ] Main view orbit with Alt/Ctrl modifier, 0.48°/0.32° per px (5.5 d)
- [ ] Wheel zoom (active view) and keypad/menu zoom (all views), limits, hover gate (5.5 e)
- [ ] Focus key moves pivot and aims Main camera (5.5 f)
- [ ] Reset Views with undo step and quirks (5.7)
- [ ] Camera states saved in and restored from `.cubik` (open and merge) (5.7)
- [ ] Orthographic Main view preference incl. preview interaction (5.8)
- [ ] Wireframe mode (5.9)
- [ ] Cinema 16:9 frame with black bars (5.10)
- [ ] Per-view content tags (5.11)

**Grid and guides**
- [ ] Grid settings defaults and storage keys (6.1)
- [ ] Grid size dialog: live edit, OK/Cancel restore rules, quantisation quirk (6.2)
- [ ] Sub-grid planes, additive look, Large mode (6.3)
- [ ] Main 16-unit lines per view incl. side-view x = 16 quirk (6.4)
- [ ] Model-box and selection-box guides (6.5)
- [ ] Grid reset on New, load rules, display-mode visibility, voxel adapt (6.6)

**View backgrounds**
- [ ] Load per view / all views, toggle size, delete (7)
- [ ] Placement, orientation, back-face culling after side swap, white-square on failure (7)

**Rendering look**
- [ ] Gamma pipeline, MSAA preference, draw order (8.1)
- [ ] Two directional lights and flat ambient with the stated vectors (8.2)
- [ ] Element shading formula, depth pre-pass with 0.5 alpha test, blending (8.3)
- [ ] Face brightness table matches (8.3)
- [ ] Element colour and tint-emulation vertex colours (8.4)
- [ ] Voxel shading and optional soft shadows (8.5, 8.2)
- [ ] Back-to-front transparency sorting (8.6)
- [ ] Hover/selection outline, see-through, colours (8.7)
- [ ] Selection wireframe boxes for up to 4 objects (8.8)
- [ ] Hidden-element display preferences (8.9)
- [ ] Centre marker, north pointer, blueprint (8.10)

**Display settings preview**
- [ ] Nine modes with anchors, grid/guides/cinema flags, first-entry cameras (9.1)
- [ ] Mode switching with per-mode camera records, GUI record reset (9.2)
- [ ] Preview lock: blocked selection and manipulation input, hidden workspace, working camera (9.2, 5.11)
- [ ] Panel open/close: maximize preference, return to Default, no tab selected at the first opening (9.3)
- [ ] Slot transforms incl. left-hand mirroring (9.4)
- [ ] Player, first-person (crosshair, sky), wall, GUI, floor bobbing scenes (9.5)
- [ ] Panel controls, ranges, undo step, export flag, copy opposite hand; pages always show slot values (9.6, D11)
- [ ] Arm and head pose angles: targets, axes, rest pose, session-only storage (9.6)
- [ ] Skin cache/default/lookup, square check, error texts (9.7, D2)
- [ ] Presets: bundled list, routing, add/remove/protected, file naming, JSON (9.8)

**Preferences**
- [ ] Every preference row of 10.2 with default, key and effect
- [ ] Preferences dialog snapshot, picker titles, OK order, path-existence rule, backup minimum 5, Cancel (10.3)
- [ ] Key binding strings parsing and normalisation (10.2)
- [ ] Theme selection Light/Dark (10.2)
- [ ] Legacy settings import (10.1, 10.4)

**Files and data**
- [ ] Recent files: newest-first submenu, name display, max 10, add rules, missing-file removal, clear (11)
- [ ] Weekly rotating automatic backups with interval rule (12.1)
- [ ] Recover menu: five actions with undo labels (12.3)
- [ ] Drag and drop of the first file (13, D6)
- [ ] Internal clipboard only, cleared by New/Open (14)
- [ ] Render to PNG: GUI 512² / Main 1920 x 1080, anti-aliasing x2 with the shifted 2 x 2 average, explicit
  transparent clear, content rules (15)
- [ ] Resource-pack `pack.png` render (15)
- [ ] Sketchfab upload flow and dialogs (16, D3)
- [ ] Minecraft/Staxel/Stonehearth/Steam folder detection (17.1, 17.2)
- [ ] File browser default favourites and path placeholders (17.3)
- [ ] Tools menu folder shortcuts; last save/export folder rules, no error before the first save (17.4)

**Network and messages**
- [ ] No network access without user action or opt-in (18.2)
- [ ] Optional opt-in update check (18.2, D5)
- [ ] Help menu links to the port's pages (18.2)
- [ ] Status line messages and 10 s "Ready" reset (19)

---

## 21. Unverified / open questions

1. **Absolute shading brightness.** The 8.3 factors follow from the original's compiled lighting programs
   (light and ambient terms halved, colour doubled). The engine's actual ambient value passed to that program
   is assumed to be half the configured flat ambient. Confirm with a screenshot of a white cube (expected 8-bit
   values 245 / 219 / 184 on the up / south / west faces from the default Main camera).
2. **Voxel shading** through spherical harmonics (8.5): exact values of the secondary light on voxel faces
   are not computed; calibrate against a screenshot.
3. **Mouse wheel step**: 0.1 zoom input per notch is assumed (engine wheel delta 1 per notch times 0.1).
4. **Zone pointer events**: that leaving a view clears the hover state (and therefore that menu-clicked Zoom+/-
   does nothing) is inferred from the preserved handler names; the event bindings themselves are not visible
   in the extracted scene data.
5. **Outline pixels** (8.7): tap pattern, alpha sum and cut-out are verified, the exact band shape with MSAA and
   the visual weight are not.
6. **Faint white boxes** (8.8, 8.9): the effective opacity of the vertex-alpha 5/255 boxes depends on an internal
   engine material; verify visually.
7. **Sub-grid line appearance** (6.3) under mip-mapping and the 0.124 additive weight: compare screenshots at
   several zoom levels before finalising the port shader.
8. **Engine-managed window size restore** (1.1) and Alt+Enter (4.4) are engine features not visible in the
   application code.
9. **Initial status text** (19.1): whether "Loading..." is visible after start-up.
10. **First-person preset collision** (9.8): which "Vanilla block" wins depends on the folder listing order.
11. **Display panel initial tab** (9.3): no tab is selected at the first opening. This rests on the serialized
    tab states (all off) and on the shipped interface library having no automatic selection for tab groups;
    confirm in the original.
12. **PNG background and alpha** (15): the contents of a freshly created render target (expected transparent
    black), the resulting `a²` alpha of semi-transparent texels and the alpha that voxels write. The offscreen
    camera's scene object is inactive; the render assumes the engine still renders it on request. Confirm with
    exports of a half-transparent texture and of a voxel model.
13. **Downscale offset** (15): whether the one-source-pixel shift of the 2 x 2 average is visible.
14. **Save As cancel inside the unsaved prompt** (2.2): assumed to leave the application running without
    further action.
15. **Path placeholders** (17.3): reached through the built-in browser's path field; exact field behaviour is
    owned by `ui_dialogs_menus.md` 10.11.
16. **Legacy skin hosts** (9.7): assumed unreachable today; irrelevant for the port (D2).
17. **Sketchfab API** (16): the current endpoint, authentication header and response shape must be taken from
    Sketchfab's current documentation at implementation time.
18. **Outlines in wireframe mode** (5.9, 8.7): the outline mask and its blur passes run while wireframe drawing
    is active, so outlines are probably broken or missing in wireframe mode in the original [I]. The port may
    keep normal outlines; confirm before claiming parity either way.
19. **Display preview interaction lock** (9.2): the blocked input set is verified. No preview check was found
    for menu commands and keyboard shortcuts (including those that act on the selection), so they are assumed
    to keep working; this was not traced command by command.
20. **Refresh-triggered edit handler** (9.3, 9.6, D11): relies on the engine raising value-change events for
    changes made by code. Confirm in the original: open the Display Settings panel on a saved model whose
    slots have non-default values, click a tab and check whether the document becomes modified; in a fresh
    session, also press "Copy left hand" before the left-hand tab was ever clicked.
21. **Pose angle after New file and Open** (9.6): New file keeps the in-memory angle (reset path read); that
    opening a file keeps it as well is inferred from the load copying the slots field by field.
22. **`null` signature template** (10.1): which actions fail and how the failure shows (error box or log only).
23. **`null` theme name** (10.1): how the interface looks when no theme is applied at start-up.
24. **Settings error box at start-up** (1.2): whether the "Exception" box can be displayed that early in the
    start-up sequence.
