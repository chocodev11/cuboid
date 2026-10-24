# UI spec: menus, shortcuts and dialogs

Status: implementation and parity-test reference for cuboid. Target behaviour: Cubik Studio 2.9.480 Beta (Windows x64). This document describes observable behaviour in our own words; it contains no original code.

Covered here: the menu bar and every menu row, every popup / context / dropdown menu and the list-row gestures that open them, menu-action semantics (guards, undo names, messages), the modified flag and what the unsaved-changes prompt covers, the complete default keyboard and mouse binding table, custom-binding storage, the dialog framework, the message box, every dialog (Preferences, Grid size, Voxel Model Setup, Voxelization, PNG Import, Model Properties, Change Faces Textures, Color Picker, generic text prompt, generic vector prompt, Progress, About, Register UI) and the built-in file browser.

Not covered (owned by other specs, referenced by name only): element / mesh / voxel / texture algorithms, file formats, the application shell (panels, status bar, tool palettes, views), theme colour values, licence and registration logic (only the Register dialog's UI is described), product branding (all "Cubik" / "BDcraft" wording, logos and credits must be replaced by cuboid's own).

## 0. Conventions

- **Units and coordinates.** UI units; 1 unit = 1 screen pixel at UI scale 1.0. Origin top-left, x to the right, y downwards. Dialog-local rectangles are written `x,y w×h` from the dialog's top-left corner. `W`, `H` = current dialog width and height (dialogs can grow, see 7.5).
- **UI scale.** `D` = floor(max(96, screen dpi) / 96 × 2) / 2 (1.0 for 96..143 dpi, 1.5 for 144..191, 2.0 for 192..239, ...). The canvas scale `S` = clamp(D, 1.0, 1.99) (shell spec). Several placement formulas below multiply by `S` or by `D`; which one is stated each time because the original mixes them.
- **Text.** OpenSans Regular, 11 px, line height 15, unless noted. Placeholders are italic. Strings are quoted exactly as shipped, including typos ("occured", "Chose", "successfuly", "dowscale", "took in account") and trailing line breaks. `\n` is a line break.
- **Theme tokens.** `btn.normal / btn.hover / btn.pressed / btn.disabled`, `fn.normal / fn.hover / fn.pressed` (32×32 panel "menu" header buttons), `field.normal / field.focus`, `text`. At theme application every text element takes the colour `text` (keeping its own alpha), except elements whose object name ends in `NoSkin`. Exact values for every theme are defined in the layout-and-theme spec (`ui_layout_theme.md`, 12.2); the Light values are repeated here for convenience:

  | Token | Light |
  |---|---|
  | btn.normal / hover / pressed / disabled | #ffffff / #c5e7ff / #a5cae5 / #dcdcdc78 |
  | field.normal / field.focus | #828282 / #0080ff |
  | text | #363636 (placeholders use it at 50 % alpha) |
  | close-X glyph normal / hover / pressed / disabled | #363636 / #e11a1a / #740000 / #72727280, each ×2 brightness multiplier |

- **Undo entries.** "Undo entry 'Name' (scope)" means: before the change, a snapshot named `Name` covering `scope` is pushed on the undo history; this also marks the document modified. Scopes: elements, meshes, objects (elements + meshes), voxels, textures, elementsAndTextures, elementsAndVoxels, objectsAndVoxels, viewProperties, modelProperties. When a guard records the entry, it is recorded even if the action then changes nothing. Recording an entry is the only way the document becomes modified: a change that records no entry leaves the document unmodified, so the unsaved-changes prompt does not cover it (5.1, "Modified flag").
- **Message boxes.** "Info box", "Question box", "Warning box", "Error box", "Check box", "Wait box", "Sketchfab box" name the icon kind of a message box (section 8). Buttons are listed in their on-screen order, left to right; the first listed is the primary button. Unless stated otherwise a box has a single "OK" button, and a Question box has "Yes", "No".
- **Status.** Statements are verified against code paths reached from real UI wiring unless marked **UNVERIFIED** (could not be confirmed) or **(inferred)** (strong evidence, one link missing). Section 11 lists all open points.

## 1. Menu system mechanics

### 1.1 Menu bar

- Full window width, 22 high, at the very top; background is the theme `label` sprite (plain white in Light).
- Buttons sit on a strip inset 1 unit from the left. Absolute positions (x from the window's left edge, all 22 high):

  | Label | x | width |
  |---|---|---|
  | File | 1 | 48 |
  | Edit | 49 | 48 |
  | Objects | 97 | 64 |
  | Recover | 161 | 64 |
  | Views | 225 | 48 |
  | Tools | 273 | 48 |
  | Window | 321 | 64 |
  | Help | 385 | 48 |

- Button visual: transparent at rest, `btn.hover` / `btn.pressed` / `btn.disabled` otherwise, 0.1 s colour fade. Label 11 px, horizontally centred, top-aligned in the box y 4..22.
- A ninth menu, labelled "Selection", exists in the original data at the "Objects" slot but is never shown (section 2.10).

### 1.2 Opening and closing bar menus

- Left click on a bar button: set the "menu bar engaged" flag, then open its panel one frame later. Clicking an open menu's button again re-opens it (no toggle).
- While engaged, pointer-enter on another bar button opens that menu immediately; the previously open menu closes because the pointer has left it.
- Pointer leaving a bar button closes its panel one frame later. The panel and all its child panels count as part of the button for hover purposes, so moving from the button into its panel (or deeper) keeps it open.
- The engaged flag is cleared when the pointer leaves the menu-bar region (inferred: a hover trigger on the bar whose target is not resolvable in the data).
- Bar menus do not use "close on outside press"; they close when the pointer is outside them or when a leaf row is clicked.

### 1.3 Panels, placement and closing

- A panel is a vertical list of rows. Each row has a minimum width equal to the menu's `minWidth` (listed per menu); the panel shrink-wraps its rows. Panel and row metrics (from the shipped templates; same as the layout-and-theme spec 3.2): panel frame `bg_panel` with padding 4 on all sides and 2 between rows; item row 20 high; icon 16×16 centred at x 12; label from x 32, 2 from the top, left-aligned; shortcut text right-aligned in the box x 24 .. w − 24, 4 from the top; submenu arrow 9×9 centred at x w − 12; separator row 2 high. The placement tests below use the original's own height estimate of `rows × 15 + 8` units when the panel has not been measured yet.
- **Bar drop-down:** panel top-left = bar button's bottom-left (22 below the button's top, left edges aligned).
- **Child submenu:** attached to its parent row's top-right corner and raised 4 units (child top = row top − 4, child left = row right). Each time a child panel becomes visible it is first measured (hidden for one frame), then clamped: if `left + max(220, width)·S > screen width`, it is moved to the left side of the parent row (`left = parent row left − width·S`); if its bottom would fall below the screen bottom, it is moved up so that its bottom touches the screen bottom.
- **Popup at cursor** (context menus, panel header menus): top-left at the pointer. If `pointer.x + max(minWidth + 8, width)·S > screen width`, shift left so the right edge touches the screen edge. If the panel (height `max(rows × 15 + 8, height)·S`) would extend below the screen bottom, it is shown above the pointer instead (its bottom edge at the pointer).
- **List dropdown under a control** (dropdown buttons, list fields): top-left at the control's bottom-left, panel width forced to the control's width. If `max(rows × 15 + 8, height)·D` would extend below the screen bottom, it is shown above the control (bottom edge at the control's top).
- **Close on outside press:** popups and list dropdowns close when any key or mouse button goes down while the pointer is not over the popup (including its open children).
- **Child open / close:** a child opens one frame after the pointer enters its parent row and closes one frame after the pointer leaves that row (the child counts as inside the row).
- Opening and closing of every panel is deferred by one frame, except the engaged-bar hover switch (immediate).

### 1.4 Rows

| Property | Meaning |
|---|---|
| label | Text. A label starting with "-" makes the row a separator (thin line from the separator sprite, 24×2 source); separators ignore clicks and any icon. |
| icon | Optional leading icon (source asset names are given in the tables for reference only; cuboid ships its own icons). Removed when absent. |
| shortcut | Display text in the shortcut column; also the binding definition (6.1). On macOS the displayed text replaces "Ctrl" with "⌘" and "Alt" with "⌥" (display only). |
| child | Child submenu; the row shows a submenu arrow only when it has a child. |
| action | Name of the action run on click (or by the shortcut). Empty = no action. |
| linked button | A toolbar button whose enabled state is kept identical to the row's. |
| always active (A) | Row and its shortcut stay enabled when the menu is disabled. |
| works in UI (U) | Shortcut also fires while a UI control has keyboard selection (6.2). |

- Row look: background transparent at rest; `btn.hover` / `btn.pressed` while hovered / pressed, switching instantly (no fade).
- Enabled look: row interactive, label and shortcut in `text`, icon normal.
- Disabled look (rows that are not always active): row not interactive (no hover, no click, no child opening), background clear, label and shortcut grey #808080, icon rendered greyscale.

### 1.5 Activating a row

Left click on a row (right click does nothing):

1. If the row has no child: close the whole cascade immediately (every panel from this row up to the root) and hide the owning panel.
2. Emit "row selected" with the row's label (consumed by list-style menus: recent files, theme list, field dropdowns).
3. If the row has an action, run it 0.05 s later (the same deferral is used when a shortcut fires).

Clicking a row that has a child only emits step 2 (rows with children have no action). An action whose name the owning handler does not implement does nothing (three such rows exist; they are flagged "dead" in the tables).

### 1.6 Enabling and disabling

- **Menu level** (a whole menu): applies the enabled / disabled look to every row except always-active rows, updates each row's linked button, enables / disables the menu's associated toolbar buttons and every keyboard binding registered by that menu (always-active bindings keep firing). The bar button itself stays clickable and the panel still opens. Re-enabling a menu re-enables all its rows, ignoring row-level states.
- **Row level** (by menu and row label): sets the row's enabled state and optionally replaces its label text; also updates the row's linked button. Rows are matched by their original label, so renamed rows (Undo, Redo, Paste) keep matching.
- **Initial state:** Objects disabled, Tools disabled, the "Open recent files" submenu disabled; all others enabled.
- A panel that is rebuilt (dynamic rows) does not re-apply its menu's disabled state to the new rows (quirk; affects only the recent-files submenu, whose new rows therefore look enabled).

### 1.7 Dynamic rows

| Row | Enabled when | Label | Refreshed |
|---|---|---|---|
| Edit > Undo | history can undo | "Undo <name of entry to undo>" or "Undo" | on history change |
| Edit > Redo | history can redo | "Redo <name of entry to redo>" or "Redo" | on history change |
| Edit > Paste | internal clipboard not empty | "Paste objects" (objects copied), "Paste voxels" (voxels copied), else "Paste" | after Copy, Cut and document reset; not after Paste or a selection change. Disabled "Paste" after start-up reset |
| Edit > Cut, Copy, Delete | selection count > 0 | unchanged | on every selection change (initial state before the first selection event: UNVERIFIED) |
| Objects (whole menu) | primary selected object is an element or a mesh | | on every selection change |
| File > Open recent files | recent list not empty | | see 2.1 |

## 2. Menu bar tree

Flags: A = always active, U = works in UI, L = has a linked toolbar button. Shortcuts are shown exactly as displayed. "minWidth" is the panel row minimum width. Degree signs are real (°).

### 2.1 File (minWidth 196, enabled)

| # | Label | Shortcut | Icon | Action | Flags |
|---|---|---|---|---|---|
| 0 | New file | Ctrl+N | page_white | NewFile | A |
| 1 | - | | | | |
| 2 | Open | Ctrl+O | folder_open | Open | A |
| 3 | Merge | Ctrl+M | folder_merge | Merge | A |
| 4 | Open recent files | | | child: Recent files | |
| 5 | - | | | | |
| 6 | Save | Ctrl+S | save | Save | A U |
| 7 | Save As... | | save_as | SaveAs | A |
| 8 | - | | | | |
| 9 | Import | Ctrl+I | import | Import | A |
| 10 | Export | Ctrl+E | export | Export | |
| 11 | - | | | | |
| 12 | Exit | | close_window | Exit | A |

**Recent files submenu** (child, minWidth 196, starts disabled). Initial content is a single disabled row "No recent files...". About 0.5 s after the settings file is loaded the submenu is rebuilt from the recent list (and again after every list change):

- List empty: the submenu is emptied and "Open recent files" is disabled.
- List not empty: "Open recent files" is enabled; one row per entry, newest at the top (the list is kept oldest-first and each entry is inserted at the top); then a separator; then "Clear recent file list" (action ClearRecentFiles). Recent rows have no icon, no shortcut, no action: they work through the "row selected" event (5.2).

Recent list rules (persisted in the settings file, settings spec): at most 10 entries `{name, path}`; `name` is the file name without folder; adding an entry first removes any entry with the same name, appends the new one at the end, then drops the oldest while more than 10. Entries are matched by name only (two files with the same name in different folders collide). Paths with extension ".lnk" are never added.

### 2.2 Edit (minWidth 280, enabled)

| # | Label | Shortcut | Icon | Action | Flags | Notes |
|---|---|---|---|---|---|---|
| 0 | Undo | Ctrl+Z | arrow_undo | Undo | | dynamic (1.7) |
| 1 | Redo | Ctrl+Y | arrow_redo | Redo | | dynamic |
| 2 | - | | | | | |
| 3 | Cut | Ctrl+X | cut | Cut | | enabled iff selection > 0 |
| 4 | Copy | Ctrl+C | page_copy | Copy | | enabled iff selection > 0 |
| 5 | Paste | Ctrl+V | page_paste | Paste | | dynamic |
| 6 | - | | | | | |
| 7 | Delete | Ctrl+Del | cross | Delete | L | enabled iff selection > 0; no confirmation |
| 8 | - | | | | | |
| 9 | Select by name | Ctrl+F | search | SelectName | | also F3 (hidden) |
| 10 | Select all | Ctrl+A | select_all | SelectAllElements | | |
| 11 | Deselect | Ctrl+D | | SelectNone | | |
| 12 | Select Inverse | Ctrl+Shift+I | | SelectInverse | | |
| 13 | - | | | | | |
| 14 | Model Properties | Ctrl+P | cog | ModelProperties | A L | |
| 15 | Set Model Origin | Ctrl+K | origin | ModelOrigin | L | |
| 16 | Set Model Blueprint | Ctrl+B | blueprint | ModelBlueprint | L | |
| 17 | - | | | | | |
| 18 | Rotate model +90° on X axis | | icon_rotateX | RotateModelX | | |
| 19 | Rotate model +90° on Y axis | | icon_rotateY2 | RotateModelY | | |
| 20 | Rotate model +90° on Z axis | | icon_rotateZ | RotateModelZ | | |

Hidden bindings owned by Edit (no row): `Del` → DeleteConfirm, `F3` → SelectName (neither always active nor works-in-UI).

Note: the Display Settings panel disables / re-enables five Edit rows by name while it is open ("Turn by +90° on Y axis", "Turn by -90° on Y axis", "Roll by 90° on X axis", "Optimize Faces", "Optimize Cullfaces"); none of these rows exist, so this has no visible effect. Do not invent the rows.

### 2.3 Objects (minWidth 256, starts disabled; enabled iff the primary selected object is an element or a mesh)

| # | Label | Shortcut | Icon | Action | Flags |
|---|---|---|---|---|---|
| 0 | Next | PgUp | bullet_go | Next | A |
| 1 | Previous | PgDown | bullet_back | Previous | A |
| 2 | - | | | | |
| 3 | Element Properties... | Shift+P | shape_square_wrench | ElementProperties | |
| 4 | Element Faces... | Shift+F | cube_faces | ElementFaces | |
| 5 | Element UV Mapping... | Shift+U | cube_uv | ElementUV | |
| 6 | - | | | | |
| 7 | Rename | Shift+N | textfield_rename | Rename (objects) | |
| 8 | Move | Shift+M | arrow_out | Move | |
| 9 | Scale | Shift+S | resize | Scale | |
| 10 | Clone | Shift+C | clone | Clone (objects) | |
| 11 | - | | | | |
| 12 | Toggle Visibility | Shift+V | lightbulb_off | ToggleVisibility (objects) | L |
| 13 | Toggle Lock | Shift+L | lock | ToggleLock (objects) | L |
| 14 | Focus Lock | Shift+K | lock_go | FocusLock (objects) | L |
| 15 | Reset Origin/Rotation | Shift+R | anchor | ResetRotation | L |

Hidden binding owned by Objects: `F2` → Rename (objects), not always active. Objects has 8 associated toolbar buttons (enabled with the menu); the data does not resolve which (inferred: the Selection toolbar panel buttons; UNVERIFIED).

### 2.4 Recover (minWidth 280, enabled)

| # | Label | Shortcut | Icon | Action | Flags |
|---|---|---|---|---|---|
| 0 | Show all Elements | Ctrl+Shift+S | lightbulb | ShowAll | L |
| 1 | Unlock all Elements | Ctrl+Shift+U | lock_open | UnlockAll | L |
| 2 | - | | | | |
| 3 | Reset Blueprint to default | Ctrl+Shift+P | blueprint | ResetBlueprint | L |
| 4 | Reset Views to default | Ctrl+Shift+C | camera_go | ResetCamera | L |
| 5 | - | | | | |
| 6 | Get back Far Elements | Ctrl+Shift+F | recover_elements | GetBackFarElements | L |

("Recover" is not file recovery; there is no backup-restore UI.)

### 2.5 Views (minWidth 296, enabled)

| # | Label | Shortcut | Icon | Action |
|---|---|---|---|---|
| 0 | Zoom+ | + | magnifier_zoom_in | ZoomPlus |
| 1 | Zoom- | - | magifier_zoom_out | ZoomMinus |
| 2 | - | | | |
| 3 | Wireframe mode | Shift+W | eye | WireframeMode |
| 4 | Grid settings | Shift+G | grid | GridSettings |
| 5 | - | | | |
| 6 | Load background for Up view... | | image_add | LoadBgTL |
| 7 | Load background for West view... | | image_add | LoadBgTR |
| 8 | Load background for South view... | | image_add | LoadBgBL |
| 9 | - | | | |
| 10 | Load background for all views... | Shift+A | images | LoadBgAll |
| 11 | - | | | |
| 12 | Toggle backgrounds size | Shift+T | images | ToggleBgSize |
| 13 | Delete backgrounds | Shift+B | images_delete | DeleteBgAll |

("+" and "-" bind the numeric-keypad plus / minus keys only, 6.1.)

### 2.6 Tools (minWidth 256, starts disabled)

Every row except separators is always active, so the menu's disabled state has no visible or functional effect. In the original the menu is enabled by the start-up licence check (out of scope); cuboid simply enables it at start-up.

| # | Label | Shortcut | Icon | Action | Flags |
|---|---|---|---|---|---|
| 0 | Element tools | F5 | cube | ElementTools | A |
| 1 | Voxel tools | F6 | voxel | VoxelTools | A |
| 2 | Paint tools | F7 | draw_pixel | PaintTools | A |
| 3 | Color tools | F8 | color_picker | ColorTools | A |
| 4 | Color Palette | F9 | color_palette | ColorPalette | A |
| 5 | Elements List | F10 | elements_manager | ElementsList | A |
| 6 | Elements Material | F11 | materials_manager | ElementsMaterial | A |
| 7 | Gizmos tools | F12 | bounding_box | GizmosTools | A |
| 8 | - | | | | |
| 9 | Share on Sketchfab | | link | ShareSketchfab | A |
| 10 | - | | | | |
| 11 | Browse Cubik Folder | | folder_open | CubikFolder | A |
| 12 | Browse Cubik Data Folder | | folder_open | CubikDataFolder | A |
| 13 | Browse last save/export Folder | | folder_open | LastExportFolder | A |
| 14 | - | | | | |
| 15 | Preferences... | | cog | Preferences | A |

### 2.7 Window (minWidth 380, enabled)

| # | Label | Shortcut | Icon | Action |
|---|---|---|---|---|
| 0 | Move Managers on Left | Ctrl+1 (Num. keypad) | move_managers_left | MoveManagersOnLeft |
| 1 | Move Managers on Right | Ctrl+3 (Num. keypad) | move_managers_right | MoveManagersOnRight |
| 2 | - | | | |
| 3 | Close Managers | Ctrl+2 (Num. keypad) | | HideManagers |
| 4 | Close Helpers | Ctrl+8 (Num. keypad) | | HideHelpers |
| 5 | - | | | |
| 6 | Close All | Ctrl+0 (Num. keypad) | close_window | HideAll |

### 2.8 Help (minWidth 200, enabled)

| # | Label (original) | Shortcut | Icon | Action |
|---|---|---|---|---|
| 0 | About BDcraft Cubik... | | information | About |
| 1 | Registration... | | key | Registration |
| 2 | - | | | |
| 3 | Cubik Support... | F1 | comments | Support |
| 4 | Cubik Details... | | cubik_logo | CubikPage |
| 5 | Cubik Changelog... | | changelog | Changelog |
| 6 | BDcraft.net | | bdcraft_logo | BDcraftPage |

Branding: cuboid keeps the structure (About, Registration-equivalent or none, separator, Support F1, Details, Changelog, project site) but uses its own names and URLs.

### 2.9 Panel header menu buttons

Several tool panels have a 32×32 "menu" button (sprite `tex_menu`, colours `fn.normal / fn.hover / fn.pressed`) at their header's top-right corner. A left click opens a popup at the cursor (1.3). Mapping by object name (the data does not resolve the targets; inferred):

| Panel | Popup |
|---|---|
| Elements List | Elements popup (3.2) |
| Elements Material | Material popup (3.4) |
| Meshes List | Meshes popup (3.1) |
| Voxel tools | Voxel popup (3.6) |
| Color Palette | Palette popup (3.7) |

The equivalent buttons on the Element tools, Mesh tools, Paint tools, Color tools and Gizmos tools headers exist but are hidden.

### 2.10 Dormant "Selection" menu (do not implement)

The data contains a second element menu labelled "Selection" (minWidth 256, disabled at start) occupying the Objects slot, but its object is inactive and nothing activates it, so it never appears and its bindings are never registered. For the record its rows were: Next (PgUp, A), Previous (PgDown, A), -, Edit Properties... (Shift+P), Edit Faces... (Shift+F), Edit UV Mapping... (Shift+U), -, Set Color... (Shift+C, L), Apply Random Color (Shift+R, L), Apply Default UV Mapping (Shift+D, L), Optimize faces, Optimize culling, Sort transparent elements (Shift+T), -, Toggle Visibility (Shift+V, L), Toggle Lock (Shift+L, L), Focus Lock (Shift+K, L). Its handler does not implement Next, Previous, Edit Properties, Edit Faces or Edit UV Mapping. Because it is dormant, Shift+C / Shift+R / Shift+T have no conflicts in practice.

## 3. Popup and dropdown menus

All popups open at the cursor (1.3) and close on an outside press. Submenus ("PopSub") are child panels (1.3). "elements" / "meshes" / "objects" in the Action column refer to the handler that runs the action (section 5).

### 3.1 Meshes popup (minWidth 180)

Opened by the Meshes List header menu button, or by a right click on a mesh row that is already selected (3.9). Not opened from the 3D views.

| # | Label | Icon | Action / child |
|---|---|---|---|
| 0 | Mesh | sphere | child: Mesh |
| 1 | Transform | arrow_out | child: Mesh transform |
| 2 | - | | |
| 3 | Convert to voxels... | export | StartVoxelization |
| 4 | - | | |
| 5 | Texture | image | child: Mesh texture |
| 6 | - | | |
| 7 | Copy | page_copy | child: Mesh copy |
| 8 | Paste | page_paste | child: Mesh paste |

- **Mesh** (minWidth 170, objects handler): Rename... (Rename), -, Set Color... (SelectMeshTint), Apply Color (ApplyMeshTint), -, Move to top (MoveToTop, meshes), Move to bottom (MoveToBottom, meshes), -, Hide (Hide), Show (Show), -, Lock (Lock), Unlock (Unlock), -, Clone (Clone), -, Delete (Delete). Icons: textfield_rename, color_wheel, color_wheel_go, arrow_up, arrow_down, lightbulb_off, lightbulb, lock, lock_open, clone, cross.
- **Mesh transform** (minWidth 160): Set Position... (SetPosition), Move... (Move), -, Set size... (SetSize), Scale... (Scale), Apply rescale (ApplyRescale), -, Rotate 90° around X (RotateX), Rotate 90° around Y (RotateY), Rotate 90° around Z (RotateZ), -, Randomize... (Randomize), -, Center on X axis (CenterX), Center on Y axis (CenterY), Center on Z axis (CenterZ).
- **Mesh texture** (minWidth 160): Load texture... (LoadMeshTexture), Save texture... (SaveMeshTexture), -, Remove alpha (RemoveAlpha), -, Clear texture (ClearMeshTexture).
- **Mesh copy** (minWidth 140): Texture (CopyMeshTexture), Mesh color (CopyMeshColor).
- **Mesh paste** (minWidth 140): Texture (PasteMeshTexture), Mesh color (PasteMeshColor).

### 3.2 Elements popup (minWidth 180)

Opened by the Elements List header menu button, or by a right click on an element row that is already selected (3.9).

| # | Label | Icon | Child |
|---|---|---|---|
| 0 | Element | cube | Element |
| 1 | Transform | arrow_out | Element transform |
| 2 | - | | |
| 3 | Faces | | Faces |
| 4 | Textures | | Textures |
| 5 | UV Mapping | cube_uv | UV Mapping |
| 6 | Tint | | Tint |
| 7 | Culling | | Culling |
| 8 | Render | | Render |
| 9 | - | | |
| 10 | Copy | page_copy | Element copy |
| 11 | Paste | page_paste | Element paste |

- **Element** (minWidth 170, elements handler): Rename... (Rename, elements), -, Set Color... (Color), Apply random Color (ColorRandom), -, Move to top (MoveToTop, elements), Move to bottom (MoveToBottom, elements), -, Hide, Show, -, Lock, Unlock, -, Clone (elements), -, Delete (elements). Icons as in the Mesh submenu.
- **Element transform** (minWidth 160, objects handler): same rows as Mesh transform, plus "Inflate..." (Inflate) inserted after "Apply rescale".
- **Faces** (minWidth 128): Enable (icon bullet_check, child Faces enable), Disable (icon bullet_uncheck, child Faces disable).
- **Faces enable / Faces disable** (minWidth 128 each): Top, Down, -, North, South, West, East, -, All (actions FacesEnableUp / FacesEnableDown / FacesEnableNorth / ... / FacesEnableAll and FacesDisable...; note "Top" is the Up face).
- **Textures** (minWidth 270): "texture (default)" (TexDefault), -, "side + up + down" (TexSUD), "side + cap" (TexSC), "front + side + cap" (TexFSC), "down + up + north + south + west + east" (TexDUNSWE), -, "Customize..." (TexCustom, icon bullet_wrench).
- **UV Mapping** (minWidth 196): Enable Snap (UVSnapEnable), Disable Snap (UVSnapDisable), -, Enable Symmetry (UVSnapMirrorEnable), Disable Symmetry (UVSnapMirrorDisable), -, Apply Default UV (dead: does nothing; the working action is the dormant menu's UVDefault, 5.5).
- **Tint** (minWidth 128): Top (TintUp), Down (TintDown), Sides (TintSides), -, All (TintAll), None (TintNone).
- **Culling** (minWidth 128): Top (CullingUp), Down (CullingDown), Sides (CullingSides), -, All (CullingAll), None (CullingNone).
- **Render** (minWidth 128): Enable shading (ShadeEnable), Disable shading (ShadeDisable), -, Outer normals (InvertFalse), Inner normals (InvertTrue).
- **Element copy** (minWidth 140): Faces visibility (CopyFaces), Textures (CopyTextures), UV Mapping (CopyUV), Tint (CopyTint), Culling (CopyCulling), Shading (CopyShade).
- **Element paste** (minWidth 140): Faces visibility (PasteFaces), Textures (PasteTextures), UV Mapping (PasteUV), Tint (dead: does nothing), Culling (PasteCulling), Shading (PasteShade).

### 3.3 Texture popup (minWidth 160)

Opened by a right click on a texture row of the Elements Material list; the click first selects that texture (3.9). Rows: Load texture... (LoadTexture), Save texture... (SaveTexture), -, Set as shortcut (SetAsShortcut, icon image_link), Edit label... (SetTextureLabel), Rename texture... (RenameTexture), -, Apply to selection (ApplyTextureToSelection), -, Edit texture... (EditTexture, icon draw_paint), Empty texture (EmptyTexture, icon image_delete), Delete texture (DeleteTexture, icon cross).

### 3.4 Material popup (minWidth 210)

Rows: Add missing textures (NewMat_Auto), Remove unused textures (NewMat_Optim), Delete all textures (NewMat_Empty), -, Replace textures list (icon materials_manager, child: New textures list), -, "Create atlas, blank textures" (CreateAtlasBlank), "Create atlas, keep textures" (CreateAtlasTextures).

New textures list (child, minWidth 300): "Texture (default)" (NewMat_Texture), "Side + Up + Down" (NewMat_SUD), "Side + Cap" (NewMat_SC), "Front + Side + Cap" (NewMat_FSC), "Down + Up + North + South + West + East" (NewMat_DUNSWE).

### 3.5 Texture label list (minWidth 212)

One shared list dropdown with one row per texture of the Elements Material list, labelled with the texture label, in list order.

- **Users.** Every field with list that picks a texture label: the six fields of Change Faces Textures (9.7) and the "Texture:" field of each face row in the element settings panel's Faces tab (model-and-tools spec 10.15). The binding is inferred: the data does not show which list each field references, but this is the only list that ever receives texture labels.
- **Refresh.** The rows are rebuilt only when the texture list changes (for example a texture added, deleted or relabelled, or the list reset with the document). Opening the list, showing a dialog or changing the selection does not refresh it. A rebuild while the list is open closes it.
- **Opening.** The field's arrow button shows the list under that field at the field's width (1.3) and makes that field the only receiver of the next choice.
- **Choosing a row** writes the label into the field's text, then notifies the field's owner:
  - Change Faces Textures: nothing else happens until OK (9.7).
  - Faces tab: the face rows are written back at once into the primary selected element only (not the rest of the selection). Its vertex colours and face UVs are recomputed; vertex positions do not change. No undo entry is recorded and the document is not marked modified (5.1). Typing in the field has the same effect on every keystroke.

### 3.6 Voxel popup (minWidth 180)

| # | Label | Icon | Action / child |
|---|---|---|---|
| 0 | Setup | cog | Setup |
| 1 | - | | |
| 2 | Convert to elements | export | child: Voxel convert |
| 3 | Transform | arrow_out | child: Voxel transform |
| 4 | Blueprint | blueprint | child: Voxel blueprint |
| 5 | - | | |
| 6 | Clear voxels | bullet_delete | ClearVoxels |
| 7 | Reset | cross | ResetVoxels |

- **Voxel convert** (minWidth 220): "1 element per voxel (cubes)" (ConvertToElementsCubes), "1+ elements per color (palette)" (ConvertToElements), "1+ elements per surface (atlas)" (ConvertToElementsAtlas).
- **Voxel transform** (minWidth 180): Upscale x2 (Upscale), Downscale /2 (Downscale), -, Flip on X axis (FlipX), Flip on Y axis (FlipY), Flip on Z axis (FlipZ), -, Rotate 90° around X axis (RotateX), Rotate 90° around Y axis (RotateY), Rotate 90° around Z axis (RotateZ), -, Color all (ResetColor), Fill volume (FillVolume), Hollow volume (HollowVolume), -, Crop voxels outside model box (CropVoxelsOutside).
- **Voxel blueprint** (minWidth 220): Create voxels (FillBlueprint), Delete voxels (ClearBlueprint), -, Copy voxels (CopyInBlueprint), Paste voxels (PasteAtBlueprint), -, Flip voxels on X axis (FlipXBlueprint), Flip voxels on Y axis (FlipYBlueprint), Flip voxels on Z axis (FlipZBlueprint).

### 3.7 Palette popup (minWidth 170)

"RGB palette (default)" (DefaultPalette), "HSL palette" (HslPalette), "JMP palette" (JmpPalette), "Generic16 palette" (Generic16Palette), "Pico8 palette" (Pico8Palette), "EDG32 palette" (Edg32Palette), "EDG16 palette" (Edg16Palette), -, "Load new palette" (LoadPalette), "Save palette" (SavePalette).

### 3.8 List dropdowns (no actions; consumers read the chosen label)

| Dropdown | minWidth | Rows | Used by |
|---|---|---|---|
| Theme list | 96 | Light, Dark | Preferences > Display > Skin theme (9.1). The theme set also contains "Rainbow", which is not offered. |
| Face culling list | 96 | none, -, down, up, north, south, west, east | element settings panel (other spec) |
| UV face list | 96 | Down, Up, North, South, West, East | element settings panel (other spec) |
| Display-settings preset lists (8: right hand 3rd, left hand 3rd, right hand 1st, left hand 1st, head, floor, wall, GUI) | 128 | filled at run time with preset names | display settings panel (other spec) |
| Texture label list | 212 | 3.5 | 9.7; element settings Faces tab "Texture:" fields |

Dropdown control (used by the theme selector and the face fields): a button showing the current label plus a down-arrow glyph; click opens the list under the control (1.3); choosing a row sets the label and notifies the owner.

### 3.9 List-row gestures (Elements list, Meshes list, texture list)

Each row of the Elements list, the Meshes list and the Elements Material texture list is one button covering the whole row. It is the row's only input receiver; there is no separate options button. Row visuals and list metrics: layout-and-theme spec 8.5 to 8.7. Element and mesh selection semantics: model-and-tools spec 8.9.

| Gesture | Element row | Mesh row | Texture row |
|---|---|---|---|
| Left click | Select only this element | Select only this mesh | Make this texture the current texture (single selection, separate from the object selection) |
| Ctrl + left click | Toggle this element in the selection | Toggle this mesh | As left click (modifiers ignored) |
| Shift + left click | Selection empty: as left click. Otherwise add the range (below); nothing is deselected | Same, over meshes | As left click |
| Second left click on the same row within 0.25 s | Handled as a normal click, nothing extra | Same | Left click, then the "Select texture file" dialog of Load texture (5.11) for this texture |
| Right click | Elements popup (3.2) if this element is selected; otherwise nothing, and the selection does not change | Meshes popup (3.1) if this mesh is selected; otherwise nothing | Left click (including the double-click test), then the Texture popup (3.3) |
| Middle click, hover | Nothing | Nothing | Nothing |
| Press and drag | Scrolls the list; never selects or opens a popup | Same | Same |
| Mouse wheel | Scrolls the list | Same | Same |

- **Clicks.** A click is a press and release of the same button over the same row without a drag. Ctrl means the ModifySelection binding and Shift the ModifySelectionMultiple binding (6.4), read when the click is handled.
- **Shift range.** lo and hi are the smallest and largest id numbers (the number shown at the row's right end) among the selected objects and the clicked row. Every element (on a mesh row: every mesh) whose id lies in [lo, hi] is added to the selection.
- **Shift range defect.** The range code assumes that every selected object is of the row's kind. A Shift-click on a mesh row while any other kind of object is selected (an element, the blueprint or the model-origin marker) fails with an internal error and changes nothing. The same happens on an element row while anything other than elements is selected. Ctrl toggles are not affected. cuboid: compute the range over the selected objects of the row's kind only (deliberate fix), or keep the no-op for strict parity.
- **Double-click timer.** Each visible row slot remembers the time of the last click it handled, left or right, and refreshes it on every click. Rows are recycled while the list scrolls, so the pairing follows the row slot, not the item, and clicks on two different slots never pair. Quirk: a right click on a texture row within 0.25 s of an earlier click on the same slot opens the file dialog and then the Texture popup.
- **Texture double click** differs from the popup's Load texture: it records no undo entry, so the replaced texture cannot be undone and the document is not marked modified (5.1).
- **Drag.** Pressing on a row and moving the pointer 5 screen pixels or more (the original's drag threshold) cancels the pending click. With the left button the list content follows the pointer (vertical, clamped, no inertia: layout-and-theme spec 16.10). With the right or middle button the drag only cancels the click (engine rule; UNVERIFIED at runtime).

## 4. Selection guards and shared action rules

### 4.1 Selection kind

Computed from the current selection, but only when a primary selected object exists (otherwise the kind is "none"): kind = "elements" if any selected object is an element, "meshes" if any is a mesh (imported mesh or primitive), "mixed" if both. The blueprint, the model-origin marker and other objects contribute nothing, so a selection holding only the blueprint is "none".

When an action passes an undo name to the kind check, the entry is recorded with scope elements, meshes or objects according to the kind (nothing is recorded for "none").

### 4.2 Guards

| Guard | Passes when | Message when failing |
|---|---|---|
| any (silent) | kind ≠ none | none |
| objects | kind ≠ none | Info box "No object selected" / "You must select at least one element, mesh or primitive to proceed." |
| elements | kind = elements or mixed | none selected: Info box "No element selected" / "You must select at least one element to proceed."; meshes only: Info box "No element selected" / "This feature doesn't work on meshes.\n\nSelect at least one element to proceed." |
| meshes | kind = meshes or mixed | none selected: Info box "No mesh selected" / "You must select at least one mesh or primitive to proceed."; elements only: Info box "No mesh selected" / "This feature doesn't work on elements.\n\nSelect at least one mesh or primitive to proceed." |
| primary element | a primary selected element exists | Info box "No element selected" / "You must select at least one element to proceed..." |
| primary mesh | a primary selected mesh exists | Info box "No mesh selected" / "You must select at least one mesh to proceed..." |

When the "elements" or "meshes" guard is given an undo name it records that entry with scope elements / meshes on success. Element-only actions skip meshes of a mixed selection (and vice versa).

## 5. Action reference

Handlers: "objects" actions are shared by the Objects menu, the Element transform and the Mesh submenus; "elements" actions belong to the Elements popup; menu-specific handlers otherwise. Every action is run 0.05 s after the click or key press (1.5).

### 5.1 File

| Action | Behaviour |
|---|---|
| NewFile | Question box "New 3D Model" / "Are you sure you want to create a new 3D Model?\n\nAny change will be lost if you've not saved them." buttons "New file", "Cancel". "New file" performs a full document reset. Always asks, whether or not the document is modified; no save prompt. |
| Open | If the document is modified, show the unsaved-changes prompt first (continuation: the dialog). Open dialog titled "Open a file", start folder = Preferences load/save path, formats = loadable formats. The file is loaded in replace mode; what replace mode does depends on the format (formats-interop spec 5.3): native files reset the document. |
| Merge | As Open, title "Merge a file with current model", loadable formats, result is merged into the current document (no reset). |
| Import | As Open, title "Import a file", start folder = Preferences import path, importable formats, loaded in replace mode. Replace mode depends on the importer (formats-interop spec 5.3): Minecraft JSON, `.cub` and `.mcpe` reset the model; the voxel importers re-create only the voxel world (elements and meshes stay); OBJ, PNG sprite and LNK always add to the current model. |
| Save | If the document has a save path, write it there; otherwise run Save As. Works while a UI control has focus. |
| SaveAs | Save dialog titled "Save Cubik model as...", start folder = Preferences load/save path, default name = model name, default extension "cubik"; if the document has a save path whose folder exists, the folder, name and extension are taken from it instead. |
| Export | Export dialog titled "Export Cubik model", start folder = Preferences export path (defaults and format list: files spec). In the original this is additionally gated by licence state (excluded); cuboid always allows it. |
| Exit | Request application quit. Quit always passes through the unsaved-changes prompt in "quit" mode. |
| ClearRecentFiles | Empty the recent list and rebuild the submenu. |

Format lists, extension validation messages, loading boxes and save errors: files spec.

**Unsaved-changes prompt** (only when the document is modified): Question box "Unsaved modifications" / "Do you want to save the changes you made to model <model name>?" buttons "Yes", "No", "Cancel".
- Yes: save as with Save. The pending continuation is not run afterwards; in quit mode the application quits once the save completes.
- No: mark the document unmodified; quit mode: quit; otherwise run the continuation.
- Cancel (also the X and Escape): nothing.

**Modified flag.** Only recording an undo entry marks the document modified (section 0). The flag is cleared by a document reset (New file, opening a native file), by a successful save in a savable format (exports and backups do not clear it) and by "No" in the prompt (application-behaviour spec 2.2). Undo and redo never change it, so undoing back to the saved state still prompts. A change that records no entry leaves the flag as it was, so **the prompt does not cover a session whose only changes are of these kinds**:

- Model Properties OK: every field, except switching "Restrict rotations" on (9.6);
- Grid size changes (9.2), although the display, mouse and keyboard steps are saved with the document;
- edits in the element settings panel (General, Faces and UV Mapping tabs, including texture labels picked in the Faces tab, 3.5);
- textures changed in the external editor ("Edit texture...", 5.11, or the element settings panel's equivalent), until the application regains focus and reloads the changed files (entry 'Reload textures from HDD');
- keyboard nudges of the selection with the arrow keys (6.4);
- model-box handle drags;
- the Display Settings buttons "Copy left hand" / "Copy right hand" (application-behaviour spec 9.6);
- a texture loaded by double-clicking its row (3.9).

The granularity rules themselves are in the model-and-tools spec (15.4, 15.5). Conversely, actions that record their entry before a dialog or confirmation box opens mark the document modified even when it is cancelled: the texture popup's Load texture, Set as shortcut, Rename texture, Edit label and Delete texture (5.11), and the Mesh tools button "Add custom mesh" (entry 'Add custom mesh', meshes). cuboid: keep these rules for parity, or mark every change that is saved with the document as modified (recommended deliberate fix, 11).

### 5.2 Recent files rows

Clicking a recent row (matched by label):
- Label not in the recent list (separator, "Clear recent file list" row): nothing.
- File exists: if modified, unsaved-changes prompt (normal mode, continuation = open); then open it in replace mode as File > Open does (5.1), and move the entry to the newest position.
- File missing: Info box "File not found" / "File <name> can't be found at:\n<path>"; the entry is removed and the submenu rebuilt.

### 5.3 Edit

| Action | Guard | Undo entry | Behaviour |
|---|---|---|---|
| Undo / Redo | | | Step the history. |
| Cut | see right | see right | If the primary selected object is the blueprint: when the model has voxels, entry 'Cut voxels' (voxels) and cut (the voxels inside the blueprint box are copied to the clipboard and cleared when the blueprint is the only selected object; otherwise the object cut applies); when there are no voxels, Info box "No voxel to cut" / "When only blueprint is selected, voxels are cut.\n\nYour model doesn't have any voxel currently.". Otherwise by kind: elements → 'Cut elements' (elements), meshes → 'Cut meshes' (meshes), mixed → 'Cut selection' (objects), then copy and delete the selected objects; none → Info box "No selection" / "You must select at least one element, mesh, primitive or blueprint to proceed.". |
| Copy | | none | No message. If exactly one object is selected and it is the blueprint, copy the voxels inside the blueprint box; otherwise copy the selected elements and meshes (nothing when the selection is empty). The clipboard is internal (never the OS clipboard) and is emptied before each copy. |
| Paste | | 'Paste' (objectsAndVoxels), always | Paste the clipboard: voxels are merged at the blueprint's current position; objects are added with offset = blueprint position now − blueprint position at copy time and become the selection (clipboard spec). |
| Delete | objects | 'Delete' (objects) | Delete every selected object immediately (no confirmation). |
| DeleteConfirm (Del key, toolbar Delete) | objects | (via Delete) | N = selection count. N > 1: text "Are you sure you want to delete N objects?". Otherwise the primary object must be deletable (if not: silently nothing) and the text is "Are you sure you want to delete \"<label>\"?". Append "\n\nClick \"Delete\" to confirm.". Question box titled "Confirm deletion of N object" plus "s" when N > 1, buttons "Delete", "Cancel"; Delete runs Delete. |
| SelectName (Ctrl+F, F3) | | 'Select <text trimmed>' (objects) on OK | Text prompt (9.9) titled "Select by name (wildcards supported)", initial text "*". OK: the selection is replaced by every element and every mesh whose label matches the pattern `*<text>*` (glob, `*` = any run, `?` = one character, case-insensitive, whole label). |
| SelectAllElements | | | Select every element and every mesh. |
| SelectNone | | | Clear the selection. |
| SelectInverse | | | Toggle the selection state of every element and every mesh. |
| ModelProperties | | | Open Model Properties (9.6). |
| ModelOrigin | | | Select only the model-origin marker. |
| ModelBlueprint | | | Select only the blueprint. |
| RotateModelX/Y/Z | | 'Rotate model on X' / 'on Y' / 'on Z' (elementsAndVoxels) | Rotate every unlocked element and every unlocked mesh +90° about that axis, and the whole voxel grid. |

### 5.4 Objects actions (Objects menu, Element transform, Mesh submenus)

| Action | Guard | Undo entry | Behaviour |
|---|---|---|---|
| Next / Previous | | | List = elements if the primary selection is an element, meshes if it is a mesh, otherwise the current edit mode's list. i = index of the first selected item, or the list length if none. Next selects only item (i + 1) mod n, Previous only item (i − 1 + n) mod n. Quirk: with nothing selected Next selects the second item and Previous the last one. |
| ElementProperties / ElementFaces / ElementUV | | | Switch the edit mode to Element and open the element settings panel on its General / Faces / UV Mapping tab. |
| Rename | any | on OK: 'Rename elements to <text>' / 'Rename meshes to <text>' / 'Rename selection to <text>' (scope by kind) | Text prompt titled "Rename elements", "Rename meshes" or "Rename selection" (by kind), initial = primary object's label. OK sets the label of every selected element and mesh to the text (empty allowed) and refreshes the lists. |
| Move | any | on OK: 'Move' (by kind) | Vector prompt (9.10) "Move" / "Offset (leave empty to not modify)", initial = last vector entered in this session (initially all fields empty). OK: remember the vector as entered; offset = (x, y, −z) with empty components = 0; move the selection by the offset (selection move routine, other spec). |
| SetPosition | any | on OK: 'Set position' | Prompt "Set position" / "New position (leave empty to not modify)", initial = primary's displayed position. OK: offset = (x − cx, y − cy, −(z − cz)) where (cx, cy, cz) is the displayed position and empty components give 0; move as Move. The displayed Z is the negated internal Z. |
| SetSize | any | on OK: 'Set size' | Prompt "Set size" / "New size (leave empty to not modify)", initial = primary's displayed size. OK: every selected object's scale becomes the entered components; empty components keep that object's own current value; elements rebuild their geometry. (Z is not negated.) |
| Scale | any | on OK: 'Scale' | Prompt "Scale" / "Scale (leave empty to not modify)", initial = last entered vector (initially 1, 1, 1; empty components stay empty next time). OK: factors f with empty = 1; pivot p = primary object's internal origin; for every selected object: scale ×= f, position = (position − p)·f + p, origin = (origin − p)·f + p (component-wise); elements rebuild. |
| Inflate (Element transform only) | elements (message) | on OK: 'Inflate' (elements) | Prompt "Inflate" / "Inflate (leave empty to not modify)", initial = last entered (initially 1, 1, 1). OK: f with empty = 1; for every selected element: clear its UV-snap flag; pivot = (from.x + size.x/2, from.y + size.y/2, from.z − size.z/2); scale, position and origin scaled by f about the pivot. Meshes are skipped. |
| ApplyRescale | any | 'Apply rescale' | For every selected object whose rotation angle is ±45°: multiply its scale by 1.4142 on each axis that is not the rotation axis; for ±22.5°: by 1.08264. |
| RotateX / RotateY / RotateZ | any | 'Rotate around X axis' / 'Y axis' / 'Z axis' | Rotate every selected element and mesh +90° about that axis. |
| Randomize | any | on OK: 'Randomize position' | Prompt "Randomize" / "Random position (leave empty to not modify)", initial (0, 0, 0). OK: v with empty = 0; every selected object gets an independent uniform random offset in [−v/2, +v/2] per axis (position and origin). |
| CenterX / CenterY / CenterZ | any | 'Center on X' / 'Center on Y' / 'Center on Z' | Align every selected object's centre on that axis with the primary's centre: X: c = from.x + size.x/2, object from.x = c − size.x/2; Y likewise; Z: c = from.z − size.z/2, object from.z = c + size.z/2. |
| Clone | any | 'Clone selection' | Clone the selection (selection spec). |
| ToggleVisibility / Hide / Show | any | 'Toggle visibility' / 'Hide' / 'Show' | Flip / set / clear the hidden flag of every selected element and mesh; lists refresh. |
| ToggleLock / Lock / Unlock | any | 'Toggle lock' / 'Lock' / 'Unlock' | Same with the locked flag. |
| FocusLock | any | 'Focus lock' | If the primary is an element (or, with no element/mesh primary, the edit mode is Element): lock every element, unlock the primary; mesh: the same with meshes. |
| ResetRotation | any | 'Reset selected objects' | For every selected object: rotation reset to zero; elements also get origin = their "from" corner; gizmos refresh. |
| Delete | objects | 'Delete' (objects) | Delete every selected object. |
| MoveToTop / MoveToBottom (Mesh submenu) | | 'Move meshes to top' / 'Move meshes to bottom' (recorded with scope elements, as shipped) | Reorder the selected meshes in the Meshes list. |
| SelectMeshTint (Set Color...) | meshes, then primary mesh | on OK: 'Set color' (meshes) | Color Picker (9.8) titled "Mesh Color" (one mesh selected) or "Meshes Color" (several), initial = primary mesh's material colour. OK: the colour, with R, G and B each raised to at least 0.25 (alpha kept), is applied to every selected mesh. |
| ApplyMeshTint (Apply Color) | meshes | 'Set mesh tint' (meshes) | Apply the colour tools' current left colour to every selected mesh. |
| LoadMeshTexture | | | Image open dialog titled "Select texture file" (filters: "Image" png, "Photo" jpg, "DirectX texture" dds, "Targa texture" tga) for the primary mesh (mesh spec). |
| SaveMeshTexture | | | Save dialog titled "Chose a path to save selected texture", filter "PNG Texture (*.png)" / png (mesh spec). |
| RemoveAlpha | | | Remove alpha from the selected meshes' textures (mesh spec). |
| ClearMeshTexture | meshes | on confirm: 'Clear meshes textures' (meshes) | Question box "Clear meshes textures" / "Are you sure you want to clear textures of selected meshes?" buttons "Clear", "Cancel". |
| CopyMeshTexture / CopyMeshColor | meshes | none | Store the primary mesh's main texture / material colour in a mesh clipboard (separate from the Edit clipboard). |
| PasteMeshTexture / PasteMeshColor | | none | If something was copied: the primary mesh receives a copy of the texture / the colour. No message when nothing was copied. |
| StartVoxelization (Convert to voxels...) | | | If the model has no meshes: Info box "No mesh to voxelize" / "Please add primitives or import meshes to start voxelization."; otherwise open Voxelization (9.4). |

### 5.5 Elements popup actions

Guard "elements" (4.2) unless noted; only elements are affected; each action ends with a selection refresh; undo scope is elements unless noted.

| Action | Undo entry | Behaviour |
|---|---|---|
| Rename | on OK: 'Rename <primary's old label>' | Guard: primary element. Text prompt "Rename selected element", initial = primary element's label. OK: the selected objects are labelled `text`, `text1`, `text2`, ... in selection order (first one without suffix). |
| Color (Set Color...) | on OK: 'Set color' | Guard: primary element. Color Picker titled "Element Color" (one selected) or "Elements Color" (several), initial = primary element's colour. OK: R, G, B each raised to at least 0.25 (alpha kept), applied to every selected element. |
| ColorRandom | 'Randomized color' | Guard: primary element. Every selected element gets a random colour with R, G, B uniform in [0.6, 1.0], alpha 1. |
| MoveToTop / MoveToBottom | 'Move elements to top' / 'Move elements to bottom' | Reorder the selected elements in the Elements list. |
| ToggleVisibility / Hide / Show | 'Toggle visibility' / 'Hide' / 'Show' | Hidden flag of selected elements. |
| ToggleLock / Lock / Unlock | 'Toggle lock' / 'Lock' / 'Unlock' | Locked flag of selected elements. |
| FocusLock | 'Focus lock' | Lock every element, unlock the primary element. |
| Clone | 'Clone element' | Duplicate every selected element in place; afterwards the original elements are the selection again (inferred from the re-select step). |
| Delete | 'Delete' (objects) | Guard "objects"; delete every selected object, then clear the selection. |
| FacesEnable<Face> / FacesEnableAll | 'Enable Down face', 'Enable Up face', 'Enable North face', 'Enable South face', 'Enable West face', 'Enable East face', 'Enable all faces' | Set the face's enabled flag (the row "Top" is the Up face); rebuild geometry. |
| FacesDisable<Face> / FacesDisableAll | 'Disable Down face' ... 'Disable all faces' | Clear the flag. |
| TexDefault | 'Set faces texture to default' | All six faces use texture slot "texture". |
| TexSUD | 'Change faces texture' | Down "down", Up "up", North / South / West / East "side". |
| TexSC | 'Change faces texture' | Down and Up "cap", sides "side". |
| TexFSC | 'Change faces texture' | Down and Up "cap", South "front", North / West / East "side". |
| TexDUNSWE | 'Change faces texture' | Each face named after itself: "down", "up", "north", "south", "west", "east". |
| TexCustom (Customize...) | | Open Change Faces Textures (9.7). |
| TintUp / TintDown / TintSides / TintAll / TintNone | 'Tint Up face' / 'Tint Down face' / 'Tint Sides faces' / 'Tint all faces' / 'Tint no face' | Tint index 0 on the face(s) (Sides = North, South, West, East); None sets −1 (no tint) on all six. |
| CullingUp / CullingDown / CullingSides / CullingAll / CullingNone | 'Cull Up face' / 'Cull Down face' / 'Cull Sides faces' / 'Cull All face' / 'Cull no face' | Each affected face's cull direction = its own direction; None sets "none" on all six. |
| ShadeEnable / ShadeDisable | 'Enable shade' / 'Disable shade' | Shading flag. |
| InvertFalse / InvertTrue | 'Outer normals' / 'Inner normals' | Inner-normals flag. |
| UVSnapEnable / UVSnapDisable | 'Enable UV-snap' / 'Disable UV-snap' | UV-snap flag (enabling also recomputes UVs). |
| UVSnapMirrorEnable / UVSnapMirrorDisable | 'Enable UV-snap mirror' / 'Disable UV-snap mirror' | UV-snap mirror flag (enabling also recomputes UVs). |
| CopyFaces / CopyTextures / CopyUV / CopyTint / CopyCulling | none | Copy that property of the primary element's six faces into the face clipboard. |
| CopyShade | none | Copy the primary element's shading flag. |
| PasteFaces / PasteTextures / PasteUV / PasteCulling / PasteShade | 'Paste faces' / 'Paste texture labels' / 'Paste UVs' / 'Paste culling' / 'Paste shade' | Apply the copied property to every selected element (locked elements are not skipped). PasteTextures also clears UV-snap. There is no working "paste tint". |
| UVDefault (dormant menu only) | on Yes: 'Apply default UV' | Guard: primary element. Question box "Redefine UV Data" / "This will replace UV Mapping of N selected Elements by new UV Mapping calculated with real coordinates (without rotation).\n\nDo you wish to continue?" (Yes, No). The popup row "Apply Default UV" does not reach this action. |

Element tools palette actions with messages (owned by the element spec, listed for string parity):
- Optimize faces: entry 'Optimize elements faces'. If elements end up without faces: Question box "Empty elements detected" / "N elements don't have any face.\n\nDo you want to remove them?" buttons "Remove", "Keep" (the X acts as Keep). Then Info box "Model faces optimization done": none removed → "No face has been removed..."; one → "1 face has been removed:\n\nBefore: X faces\nAfter: Y faces"; n → "n faces have been removed (p%):\n\nBefore: X faces\nAfter: Y faces" with p = floor(n × 100 / X).
- Optimize culling: entry 'Cull elements faces'; Info box "Model culling optimization done" / "n faces have been culled." when n > 1, otherwise "n face has been culled...".
- Sort transparent elements: entry 'Sort transparent elements'; Info box "Transparent elements sort" / 0: "No element with transparent texture to sort...", 1: "1 element with transparent texture has been sorted.", n: "n elements have been sorted.".

### 5.6 Recover

| Action | Undo entry | Behaviour |
|---|---|---|
| ShowAll | 'Show all' (objects) | Clear the hidden flag of every element and mesh. |
| UnlockAll | 'Unlock all' (objects) | Clear the locked flag of every element and mesh. |
| ResetBlueprint | 'Reset blueprint' (elements) | Blueprint position (−2, 0, 2), rotation zero, size (1, 1, 1). |
| ResetCamera | 'Reset camera' (viewProperties) | All views back to their default cameras (views spec). |
| GetBackFarElements | 'Get back far elements' (elements) | Constrain all elements inside the available 3D space (element spec). |

### 5.7 Views

| Action | Behaviour |
|---|---|
| ZoomPlus / ZoomMinus | Zoom all four views by one step of +0.5 / −0.5 (views spec). |
| WireframeMode | Toggle wireframe rendering (global). |
| GridSettings | Open Grid size (9.2). |
| LoadBgTL / LoadBgTR / LoadBgBL | Open the built-in file browser (never the native one) in Open mode; start folder = Preferences textures path as read at start-up; filters "PNG: Png image", "JPG: Jpg image", "All types"; the window title is not set, so it keeps whatever the previous browser use set (quirk). On choose: load the image and show it as the background of the Up / West / South view. |
| LoadBgAll | Same dialog; the image becomes the background of all three orthographic views and all three are shown. |
| ToggleBgSize | Toggle the background scale of all three between 16 and 48 units (starts at 16). |
| DeleteBgAll | Hide all three backgrounds. |

### 5.8 Tools

| Action | Behaviour |
|---|---|
| ElementTools, VoxelTools, PaintTools, ColorTools, ColorPalette, ElementsList, ElementsMaterial, GizmosTools | Toggle the visibility of the Element tools / Voxel tools / Paint tools / Color tools / Color palette / Elements list / Elements material / Gizmos tools panel. |
| ShareSketchfab | Empty Sketchfab token in Preferences: Sketchfab box "Missing Sketchfab token" / "To share your model on Sketchfab, you must add your Sketchfab API token in Cubik Preferences > Formats > Sketchfab field...\n\nThen retry.". Otherwise Wait box "Exporting to Sketchfab" / "Your model is compiled for Sketchfab.\n\nPlease wait..." (it has an "OK" button), then upload (model name, description, token; export spec). Success: Sketchfab box "Model on Sketchfab" / "Your model is ready on your Sketchfab account!\n\nDo you want to see it online now?" buttons "Show me" (open the returned URL), "Later". Failure: Error box "Ohoh..." / <error text> buttons "OK", "Go to Sketchfab" (open the Sketchfab site), "Cancel". |
| CubikFolder | Open the application install folder in the OS file manager. |
| CubikDataFolder | Open the per-user data folder. |
| LastExportFolder | Open the folder of the last file saved or exported in this session (behaviour when nothing was saved yet: UNVERIFIED). |
| Preferences | Open Preferences (9.1). |

OS file manager: Windows: the path (with backslashes) is handed to the shell, which opens a folder in Explorer and a file with its default application; macOS: `open "<folder>"`, or `open -R "<file>"` to reveal a file; Linux: the same with `xdg-open` (which does not understand `-R`, so files are not revealed; quirk); other systems: log "OpenInFileBrowser: OS not supported".

### 5.9 Window

| Action | Behaviour |
|---|---|
| MoveManagersOnLeft | Manager column (tool panels) to the left side, helper column (element settings, display settings) to the right. |
| MoveManagersOnRight | Managers right, helpers left (start-up layout). |
| HideManagers | Hide the ten manager panels: element tools, elements list, elements material, voxel tools, mesh tools, meshes list, paint tools, color tools, color palette, gizmos tools. |
| HideHelpers | Hide element settings and display settings. |
| HideAll | HideManagers then HideHelpers. |

### 5.10 Help

About opens the About box (9.12). Registration starts the licence flow (out of scope; cuboid omits the row and the dialog, 9.13). Support opens "https://cubik.studio/support", Details "https://cubik.studio/features", Changelog a URL supplied at run time by the licence/update service (UNVERIFIED), the last row "http://BDcraft.net". cuboid substitutes its own URLs.

### 5.11 Texture and Material popups (texture spec owns the semantics)

| Action | Undo entry (textures unless noted) | Behaviour |
|---|---|---|
| LoadTexture | 'Load texture' (recorded before the dialog, even if it is cancelled) | Image open dialog "Select texture file" (filters as LoadMeshTexture) for the selected texture slot; the chosen file is loaded into that texture under its existing label and its folder becomes the last texture folder. Start folder: the texture's own folder, else the last folder used for texture loading, else Preferences textures path, else Desktop, else "/". A double click on a texture row opens the same dialog but records no entry (3.9). |
| SaveTexture | none | Save dialog "Chose a path to save selected texture", filter "PNG Texture (*.png)", default name = texture label, extension png; start folder chain as above (with the last save folder). |
| SetAsShortcut | 'Set as shortcut' (recorded before the prompt, even if cancelled) | Text prompt "Set as shortcut", initial = the texture's tag without "#" when the tag starts with "#", else empty. OK sets the shortcut tag (may show Warning box "Too many unnamed textures" / "Rename or delete one or more textures to add new ones."). |
| RenameTexture | 'Rename texture' (before the prompt) | Prompt "Rename texture", initial = texture label. Clash: Warning box "Label already exists" / "A texture with the label \"<label>\" already exists, please try another.". |
| SetTextureLabel (Edit label...) | 'Edit texture label' (before the prompt) | Prompt "Edit texture label", initial = texture label; same clash warning. |
| ApplyTextureToSelection | 'Apply texture to selection' (elements) | Every face of every selected element uses this texture's label. |
| EditTexture | none | Open the texture in the external editor set in Preferences. |
| EmptyTexture | 'Empty texture' | Empty the texture slot. |
| DeleteTexture | 'Delete texture' (before the confirmation) | Question box "Delete Texture" / "Are you sure you want to delete texture named \"<label>\"?" (Yes, No). |
| NewMat_Auto | 'Add missing textures' | Add slots for every label used by element faces. |
| NewMat_Optim | 'Remove unused textures' | |
| NewMat_Empty | 'Clear textures' | |
| NewMat_Texture | 'New textures' | Default texture list. |
| NewMat_SUD / SC / FSC / DUNSWE | 'New textures' | Clear the list, then add slots: "particle", "down", "up", "side" / "particle", "cap", "side" / "particle", "cap", "side", "front" / "particle", "down", "up", "north", "south", "west", "east". |
| CreateAtlasBlank | on Continue: 'Create blank atlas' (elementsAndTextures) | Warning box "Create blank atlas" / "This will create a unique blank texture with random color per element face and will clear the Elements Material textures list.\n\nDo you wish to continue?" buttons "Continue", "Cancel". |
| CreateAtlasTextures | on Continue: 'Create full atlas' (elementsAndTextures) | Warning box "Create textures atlas" / "This will create a unique texture assembling all textures shown on elements faces and will clear the Elements Material textures list.\n\nDo you wish to continue?" buttons "Continue", "Cancel". |

(The Elements Material panel's "+" button runs AddTexture: entry 'Add new texture'.)

### 5.12 Voxel popup (voxel spec owns the algorithms)

Common guard "voxels set up": when no voxel grid exists, Info box "Setup Voxels first" / "<reason>, be sure to setup Voxels first.\n\nDo you want to setup now?" buttons "Setup" (opens Voxel Model Setup), "Cancel". The reason is "To use this function" unless listed.

| Action | Guard reason | Undo entry (voxels unless noted) | Behaviour / messages |
|---|---|---|---|
| Setup | (none) | on Apply: 'Setup voxels' | Open Voxel Model Setup (9.3). |
| ConvertToElementsCubes | "No voxel to convert to elements" | on Continue: 'Convert voxels to elements (cubes)' (elements, textures, voxels) | Resolution above 32: Error box "Voxel resolution too high" / "This method to convert voxels to elements is limited to resolution x32 or lower.\n\nPlease select another conversion method or decrease voxel resolution.". Else Question box "Confirm conversion to elements (cubes)" / "This conversion will create N elements and create M textures.\n\nDo you want to continue?" buttons "Continue", "Cancel". |
| ConvertToElements | same | on Convert: 'Convert voxels to elements (palette)' | Question box "Confirm conversion to elements (palette)" / "Are you sure you want to convert all voxels to elements?\n\nThis will delete voxels." buttons "Convert", "Cancel". |
| ConvertToElementsAtlas | same | on Convert: 'Convert voxels to elements (atlas)' | Same text with title "Confirm conversion to elements (atlas)"; uses the Preferences atlas options (2 px padding when "Add padding" is on; square option). |
| Upscale | default | 'Upscale voxels x2' | At resolution 128 or more: Warning box "Can't upscale above x128" / "Your model is already at the maximum allowed voxel size and can't be upscaled.". |
| Downscale | default | 'Downscale voxels x2' | At resolution 2 or less: Warning box "Can't downscale below x2" / "Your model would be a simple cube after dowscale...". |
| FlipX / FlipY / FlipZ | default | 'Flip voxels on X' / 'on Y' / 'on Z' | |
| RotateX / RotateY / RotateZ | default | 'Rotate voxels around X' / 'Y' / 'Z' | |
| ResetColor (Color all) | default | 'Reset voxels color' | Recolour every voxel with the current colour. |
| FillVolume | default | 'Fill voxels volume' | When done: status "Fill voxels success!" and Check box "Volume filled" / "Voxels volume has been filled with color successfuly". |
| HollowVolume | default | 'Hollow voxels' | Status "Hollow voxels volume calculation..."; when done status "Hollow voxels success!" and Check box "Volume hollowed" / "Voxels volume has been hollowed successfuly". |
| CropVoxelsOutside | "No voxel to crop" | on Clear: 'Clear voxels outside' | Question box "Confirm clear Voxels outside" / "Are you sure you want to clear all voxels outside the model box?\n\nThis will take effect immediately." buttons "Clear", "Cancel". |
| FillBlueprint (Create voxels) | default | 'Fill blueprint with voxels' | Fill the blueprint box with the current colour (opaque). |
| ClearBlueprint (Delete voxels) | default | 'Fill blueprint with voxels' (same name) | Fill the blueprint box with empty. |
| CopyInBlueprint | | none | Select only the blueprint, then Copy (5.3). |
| PasteAtBlueprint | | none | Select only the blueprint, then paste the clipboard (unlike Edit > Paste, no undo entry). |
| FlipXBlueprint / Y / Z | default | 'Flip voxels in blueprint on X' / 'Y' / 'Z' | |
| ClearVoxels | "No voxel to clear" | (voxel spec) | Clear all voxels. |
| ResetVoxels | "Nothing to reset" | on Reset: 'Reset voxels' | Question box "Confirm reset of Voxels" / "Are you sure you want to delete the current voxel model?" buttons "Reset", "Cancel"; removes the voxel grid. |

### 5.13 Palette popup

Each built-in row resets the palette and loads that palette (RGB 256, HSL 256, JMP 16, Generic 16, Pico-8, EDG 32, EDG 16). "Load new palette": open dialog "Select a picture to load as palette" (filters "PNG Texture (*.png)" / png, "JPG Picture (*.jpg)" / jpg). "Save palette": save dialog "Chose a path to save palette", default name "cubik_palette", png.

### 5.14 Toolbar buttons that share menu actions

These call the actions directly (no 0.05 s deferral). Tooltips are shown in the status bar and, when enabled in Preferences, as a classic tooltip.

| Panel / button | Tooltip | Action |
|---|---|---|
| Model side panel: Model settings | Edit Model Properties | ModelProperties |
| Model side panel: Model origin | Set Model Origin | ModelOrigin |
| Model side panel: Model blueprint | Set Model Blueprint | ModelBlueprint |
| Selection: Show/Hide | Hide/Show selection | ToggleVisibility (objects) |
| Selection: Lock/Unlock | Lock/Unlock selection | ToggleLock (objects) |
| Selection: Focus Lock | Lock all objects except selected one | FocusLock (objects) |
| Selection: Reset | Reset selection rotation&origin | ResetRotation |
| Selection: Delete | Delete selection | DeleteConfirm (asks, unlike the menu row) |
| Recover: Show All | Show all hidden objects | ShowAll |
| Recover: Unlock All | Unlock all locked objects | UnlockAll |
| Recover: Blueprint | Reset Blueprint to default | ResetBlueprint |
| Recover: Camera | Reset Views to default | ResetCamera |
| Recover: Far elements | Constrain all elements inside available 3D space | GetBackFarElements |
| Element create: Cube / Plane X / Plane Y / Plane Z | Add Cube / Add Plane on X axis / Add Plane on Y axis / Add Plane on Z axis | undo 'Create Cube element' / 'Create PlaneX element' / 'Create PlaneY element' / 'Create PlaneZ element' (elements); create the element with UV-snap on |
| Mesh create: Cube, Sphere, Cylinder, Cone, Pyramid, Slope, Corner-in, Corner-out, Torus | "Add <Kind> primitive" | undo 'Create <Kind> primitive' (meshes); create and select the primitive (inferred) |

Linked rows (inferred pairing by action): Edit > Delete ↔ Selection: Delete; Model Properties / Set Model Origin / Set Model Blueprint ↔ Model side panel; Objects visibility / lock / focus / reset rows ↔ Selection panel; Recover rows ↔ Recover panel. A linked button follows its row's enabled state (non-interactive with a greyscale icon when disabled).

## 6. Keyboard and mouse

### 6.1 Menu binding strings

A row's shortcut text (and each hidden binding) is converted once to a canonical string:

- A single character: "+" → "KeypadPlus", "-" → "KeypadMinus", anything else → the character upper-cased.
- Otherwise split on "+" and convert each token, joined again with "+": "Ctrl", "Shift", "Alt" unchanged; "Space" → "Space"; "PgUp" → "PageUp"; "PgDown" → "PageDown"; "Del" → "Delete"; "0 (Num. keypad)" .. "9 (Num. keypad)" → "Keypad0" .. "Keypad9"; any other token → the engine key whose name equals the upper-cased token (F1..F12, letters).

Consequences: "+" / "-" are the numeric-keypad keys only; "Ctrl+1 (Num. keypad)" is Ctrl + keypad 1; top-row digits would have to be written "Alpha1".

### 6.2 Dispatch algorithm (per frame)

1. Stop if the left, right or middle mouse button went down this frame, if the global modal flag is set (message box, modal dialog, About box or file browser open), or if no key or button went down this frame.
2. Remove from the held-key list every key no longer held.
3. Take the first key that went down this frame (in engine key-code order) and move it to the end of the held list.
4. Build the combo: held keys in press order joined with "+", each named "Ctrl" (either Control or either Command key), "Shift", "Alt", "Win" (Windows keys), otherwise the engine key name ("Z", "F5", "Delete", "PageUp", "Keypad1", "KeypadPlus", "Alpha1", "Mouse3", ...).
5. Every binding whose canonical string equals the combo fires if (no UI control currently holds keyboard selection, or the binding is works-in-UI) and (its menu is enabled, or the binding is always active). Firing runs the action 0.05 s later.

Consequences (keep for parity, flag as quirks): modifiers must be pressed before the key; any extra held key prevents a match; holding a key does not repeat; two keys pressed in the same frame register only one; macOS Command acts as Ctrl.

Recommended cuboid implementation: process each key-down event separately in arrival order through steps 2-5.

### 6.3 Default menu bindings (fixed, not customisable)

| Display | Canonical | Menu | Action | A | U |
|---|---|---|---|---|---|
| Ctrl+N | Ctrl+N | File | NewFile | A | |
| Ctrl+O | Ctrl+O | File | Open | A | |
| Ctrl+M | Ctrl+M | File | Merge | A | |
| Ctrl+S | Ctrl+S | File | Save | A | U |
| Ctrl+I | Ctrl+I | File | Import | A | |
| Ctrl+E | Ctrl+E | File | Export | | |
| Ctrl+Z | Ctrl+Z | Edit | Undo | | |
| Ctrl+Y | Ctrl+Y | Edit | Redo | | |
| Ctrl+X | Ctrl+X | Edit | Cut | | |
| Ctrl+C | Ctrl+C | Edit | Copy | | |
| Ctrl+V | Ctrl+V | Edit | Paste | | |
| Ctrl+Del | Ctrl+Delete | Edit | Delete (no confirmation) | | |
| (hidden) Del | Delete | Edit | DeleteConfirm | | |
| Ctrl+F | Ctrl+F | Edit | SelectName | | |
| (hidden) F3 | F3 | Edit | SelectName | | |
| Ctrl+A | Ctrl+A | Edit | SelectAllElements | | |
| Ctrl+D | Ctrl+D | Edit | SelectNone | | |
| Ctrl+Shift+I | Ctrl+Shift+I | Edit | SelectInverse | | |
| Ctrl+P | Ctrl+P | Edit | ModelProperties | A | |
| Ctrl+K | Ctrl+K | Edit | ModelOrigin | | |
| Ctrl+B | Ctrl+B | Edit | ModelBlueprint | | |
| PgUp | PageUp | Objects | Next | A | |
| PgDown | PageDown | Objects | Previous | A | |
| Shift+P | Shift+P | Objects | ElementProperties | | |
| Shift+F | Shift+F | Objects | ElementFaces | | |
| Shift+U | Shift+U | Objects | ElementUV | | |
| Shift+N | Shift+N | Objects | Rename | | |
| (hidden) F2 | F2 | Objects | Rename | | |
| Shift+M | Shift+M | Objects | Move | | |
| Shift+S | Shift+S | Objects | Scale | | |
| Shift+C | Shift+C | Objects | Clone | | |
| Shift+V | Shift+V | Objects | ToggleVisibility | | |
| Shift+L | Shift+L | Objects | ToggleLock | | |
| Shift+K | Shift+K | Objects | FocusLock | | |
| Shift+R | Shift+R | Objects | ResetRotation | | |
| Ctrl+Shift+S | Ctrl+Shift+S | Recover | ShowAll | | |
| Ctrl+Shift+U | Ctrl+Shift+U | Recover | UnlockAll | | |
| Ctrl+Shift+P | Ctrl+Shift+P | Recover | ResetBlueprint | | |
| Ctrl+Shift+C | Ctrl+Shift+C | Recover | ResetCamera | | |
| Ctrl+Shift+F | Ctrl+Shift+F | Recover | GetBackFarElements | | |
| + | KeypadPlus | Views | ZoomPlus | | |
| - | KeypadMinus | Views | ZoomMinus | | |
| Shift+W | Shift+W | Views | WireframeMode | | |
| Shift+G | Shift+G | Views | GridSettings | | |
| Shift+A | Shift+A | Views | LoadBgAll | | |
| Shift+T | Shift+T | Views | ToggleBgSize | | |
| Shift+B | Shift+B | Views | DeleteBgAll | | |
| F5 .. F12 | F5 .. F12 | Tools | ElementTools, VoxelTools, PaintTools, ColorTools, ColorPalette, ElementsList, ElementsMaterial, GizmosTools | A | |
| Ctrl+1 (Num. keypad) | Ctrl+Keypad1 | Window | MoveManagersOnLeft | | |
| Ctrl+3 (Num. keypad) | Ctrl+Keypad3 | Window | MoveManagersOnRight | | |
| Ctrl+2 (Num. keypad) | Ctrl+Keypad2 | Window | HideManagers | | |
| Ctrl+8 (Num. keypad) | Ctrl+Keypad8 | Window | HideHelpers | | |
| Ctrl+0 (Num. keypad) | Ctrl+Keypad0 | Window | HideAll | | |
| F1 | F1 | Help | Support | | |

58 row bindings + 3 hidden bindings. Objects-menu bindings that are not always active work only while the Objects menu is enabled (primary selection is an element or mesh). No two live bindings share a canonical string.

### 6.4 Held-key and mouse bindings (input map "CubikStudio")

| Binding | Default keys / buttons | Customisable | Consumer (other spec) |
|---|---|---|---|
| ModeRotate | 1 (top row), keypad 1 | Preferences "Rotate mode" | gizmo / selection tool mode while held |
| ModeScale | 2, keypad 2 | "Scale mode" | same |
| ModeClone | 3, keypad 3, Right Alt, Left Alt | "Clone mode" | same |
| PanView | middle mouse button, Space | "Pan view" | views: pan while held and dragging |
| Focus | F | "Focus view" | selection: focus the views on the selection |
| ModifySelection | Right Ctrl, Left Ctrl, Right Command, Left Command | no | selection: toggle-select on click (3D views and list rows, 3.9) |
| ModifySelectionMultiple | Right Shift, Left Shift | no | selection: add to selection; range select on list rows (3.9) |
| RotateView | Left Alt, Left Ctrl, Right Alt, Right Ctrl | no | views: orbit the perspective view while held and dragging |
| MoveRight / MoveLeft / MoveUp / MoveDown | Right / Left / Up / Down arrow | no | selection: keyboard nudge by the keyboard snap step; records no undo entry and does not mark the document modified (5.1) |
| Zoom | mouse wheel (sensitivity 0.1) | no | views: zoom the view under the pointer |
| Submit | Return, keypad Enter | no | file browser fields (10.7) |
| Cancel | Escape | no | (unused by the app code) |

A binding is "held" when any of its keys is held. Note that Alt is shared by ModeClone and RotateView, and Ctrl by ModifySelection and RotateView; resolution belongs to the selection / views specs.

### 6.5 Fixed keys outside the binding system

| Context | Key | Effect |
|---|---|---|
| Any dialog shown, non-modal, Escape-to-cancel set | Escape (on release) | Cancel the dialog (7.4). |
| Any dialog shown, Enter-to-validate set | Return or keypad Enter (on release) | Confirm (OK). |
| Global modal flag set | Escape (on release) | Activate the message-box close button (8.5; also happens when no message box is visible, quirk). |
| Anywhere | Tab / Shift+Tab | Move keyboard selection to the next selectable to the right-or-below / left-or-above (engine navigation); an input field reached this way starts editing. |
| Pointer over a numeric input field | Mouse wheel | Integer and decimal fields: ±1 per wheel event (up = increase), no clamping. |
| About box shown | mouse press and release outside the box | Close it. |
| File browser open | see 10.12 | |
| Application start-up | Left Alt held | Skip restoring the saved window position (shell spec). |

Other hard-wired modifiers (paint and voxel tools: Left Alt and others; element settings: Right Ctrl) belong to their own specs.

### 6.6 Custom binding storage

- Menu bindings are fixed data; nothing about them is stored.
- The five customisable held-key bindings are stored in the settings file as strings under `advKeysRotate`, `advKeysScale`, `advKeysClone`, `advKeysPan`, `advKeysFocus`. First-run settings store "Alpha1, Keypad1", "Alpha2, Keypad2", "Alpha3, Keypad3, LeftAlt, RightAlt", "Mouse2, Space", "F". When a stored string is empty or missing, Preferences shows the binding's current keys in canonical form instead (built-in: "Alpha1, Keypad1", "Alpha2, Keypad2", "Alpha3, Keypad3, RightAlt, LeftAlt", "Mouse2, Space", "F").
- String format: up to four engine key names separated by commas, filling the slots positive, negative, alt-positive, alt-negative in that order; names are trimmed and matched case-insensitively against engine key names (Alpha0..Alpha9, Keypad0..Keypad9, A..Z, F1..F15, Space, Return, Mouse0..Mouse6, LeftAlt, RightAlt, LeftControl, RightControl, LeftShift, RightShift, LeftCommand, RightCommand, ...); an unknown name gives "no key"; tokens after the fourth are ignored. An empty string means "built-in default".
- On Preferences OK: each field is trimmed and stored; then all five are applied (empty → built-in default) and re-stored in canonical form: the non-empty slots in slot order, separated by ", " (a leading ", " appears if the first slot is empty).
- In the original the stored strings are applied only when Preferences OK is pressed, not at start-up (bindings revert to defaults on every launch until Preferences is confirmed). cuboid applies them at start-up as well (deliberate fix; parity tests must not depend on the original defect).

### 6.7 Shortcut quirks to keep or decide

- Press-order matching, extra-key breakage, one key per frame, no auto-repeat (6.2).
- Bindings are disabled while the global modal flag is set, which includes modal dialogs, message boxes, the About box and the file browser, but not non-modal dialogs.
- Non-works-in-UI bindings are ignored while any UI control holds keyboard selection. Dialogs give their OK button or first field the selection when shown, and in the original the selection can linger after a dialog closes until the user clicks elsewhere (runtime behaviour UNVERIFIED).
- A mouse button press in the same frame cancels shortcut processing for that frame.

## 7. Dialog framework

All dialogs except the About box and the message box share the behaviour below. The message box (section 8) and the About box (9.12) reuse parts of it and state their differences.

### 7.1 Window chrome

| Part | Rect (dialog-local) | Notes |
|---|---|---|
| Frame | 0,0 W×H | `window` sprite, 9-slice borders L32 B64 R32 T56 (sprite px), so the visible panel is inset from the rect by its soft shadow. |
| Title | 24,16 (W−40)×30 | 11 px, middle-left, colour `text`. Set by the caller for the generic prompts. |
| Close button (X) | (W−40),16 24×24 | glyph `symb_close`; acts exactly like Cancel. |
| Drag strip | 16,16 (W−64)×32 | invisible; dragging it moves the dialog (7.5). |
| Resize grip | (W−40),(H−40) 24×24, or (W−32),(H−32) 16×16 in the 420-wide family and the message box | invisible; listed per dialog. |
| Direct buttons (most dialogs) | OK (W−250),(H−49) 110×26; Cancel (W−134),(H−49) 110×26 | label 11 px centred. |
| Button row (420-wide family: Grid size, Voxel Model Setup, Voxelization, PNG Import, Change Faces Textures) | row 24,(H−50) (W−48)×28 | a flexible spacer (weight 40) then OK and Cancel (preferred width 96, weight 1 each), 4 apart, all stretched to fill. At W = 420: OK 191.81..291.91, Cancel 295.91..396.00, both 28 high (≈100.1 wide, not 96; verify against the original, 11). |

In the 420-wide family the 16×16 grip lies above the right end of Cancel, so presses in the 8×10 overlap (x 388..396, y H−32..H−22) start a resize instead of pressing Cancel (quirk).

### 7.2 Placement

Every time a dialog is shown it is re-centred (a previous drag is forgotten); its size is kept from any earlier resize in the session. Screen-pixel top-left = (trunc(screen width − W·D) div 2, screen height − trunc(screen height + H·D) div 2), integer division, `D` from 0. Because the drawn size is W·S with S = min(D, 1.99), the dialog is very slightly off-centre at D ≥ 2.

### 7.3 Showing, focus and the global modal flag

- Showing sets the dialog's "shown" state and makes it visible. If the dialog is modal it also sets the **global modal flag**, which disables menu shortcuts (6.2), fades in the dimming layer (8.6) and enables the message-box Escape handling (8.5). The flag is a single boolean shared by modal dialogs, message boxes, the About box and the file browser; whoever clears it last wins (no counting).
- Keyboard focus on show: the dialog's default field when it has one (an input field starts editing), otherwise its OK button. Defaults: text prompt → its text field; vector prompt → X; Voxelization → "Keep inside empty"; all others → OK (Progress has none, so nothing).
- A dialog that has never been shown stays hidden.
- Pointer entering any dialog ends the current 3D-view hover state (views spec), so the views stop reacting under dialogs.

### 7.4 Closing, OK / Cancel and keyboard

- **OK** runs the dialog's own confirm logic, then the caller's confirm callback (if any), then hides. **Cancel** and the **X** run the dialog's own cancel logic, then the caller's cancel callback (if any), then hide. Hiding a modal dialog clears the global modal flag.
- **Escape**, on key release, cancels the dialog when it is shown, flagged "Escape cancels" and **not modal**. Modal dialogs ignore Escape (but see 8.5).
- **Return / keypad Enter**, on key release, confirms when the dialog is shown and flagged "Enter confirms" (modal or not).
- Both checks run for every shown dialog independently, and independently of message boxes: Escape released while a message box is open over a non-modal dialog closes both (quirk, from code; runtime UNVERIFIED).
- Tab / Shift+Tab move between controls (6.5).

### 7.5 Drag and resize

- **Drag:** pressing in the drag strip and moving shifts the dialog by the pointer delta (UI units); after each step the position is truncated to whole units and clamped so the dialog stays within the window, allowed to overhang each edge by at most 16 units.
- **Resize:** dragging the grip sets size = size at press + pointer delta (x right, y down), each axis clamped to [initial size, initial size + growth]. The top-left corner stays fixed; growth 0 means the axis is fixed. Contents follow their anchors (fields and text stretch horizontally; button rows stay bottom-right anchored).

### 7.6 Stacking order

Dialogs share one layer above the main UI, drawn in this order (later on top): Preferences, Grid size, Voxel Model Setup, Voxelization, PNG Import, Model Properties, text prompt, vector prompt, Color Picker, Change Faces Textures, About, message box, Register, Progress. Order does not change when a dialog is shown or focused, so e.g. a message box opened from Grid size appears above it, but Progress stays above a message box. Several non-modal dialogs can be open at once.

### 7.7 Standard controls

The layout-and-theme spec (section 16) is normative for widget visuals; the summary below covers what the dialogs rely on.

| Control | Look and behaviour |
|---|---|
| Push button | `bg_button` 9-slice (border 3), label 11 px centred in `text`; colours `btn.normal / hover / pressed / disabled`, 0.1 s fade. |
| Checkbox row (20 high) | Box 14×14 at (+3,+3): `bg_border` frame tinted `field.normal`, `field.focus` while hovered or pressed, `btn.disabled` when disabled; check glyph (`check`) in the same rect, colour `field.normal` (`field.focus` while hovered), fading 0.1 s. Label at (+23,+2), (w−28)×16, 11 px top-left. Clicking anywhere in the row toggles. A disabled row is greyed and hides its check mark when off. |
| Radio row (20 or 24 high) | 16×16 ring (`circle`) at (+2,+4) and 16×16 dot (`disc`) inside; label at +23. Radios of one group are exclusive and one is always selected. |
| Text field | `bg_border` frame (border 4) tinted like the checkbox frame; text inset 8 left / right, 4 top, 11 px; placeholder italic in `text` at 50 % alpha (shown when empty); caret #323232; selection colour `field.focus` at 50 % alpha. Content types used: standard, integer, decimal, alphanumeric (letters and digits only), name (letters, spaces and apostrophes; each word auto-capitalised), e-mail. Integer and decimal fields: the mouse wheel over the field adds / subtracts 1 per wheel event (the text is rewritten in plain form); text that does not parse counts as 0. |
| Slider | Track `bg_panel` tinted #f0f0f0 at 94 %; fill and handle solid #0080ff (handle darker while hovered or dragged, layout spec 16.7); whole numbers; click or drag anywhere on the box. |
| Group box | `bg_content` frame (1 px #d9d9d9 ring, white fill in Light); caption plate (`bg_label`) at (group x + 8, group y − 6), width = caption width + 8, 15 high, caption 11 px at +4, so it straddles the top border. Contents are inset 8 left / right, 12 top, 8 bottom. |
| Tab row | Tabs are exclusive; exactly one is selected and its page is the only one shown. The selected tab is drawn in `btn.normal`, unselected tabs in `btn.disabled` (faded); hover and press tint as buttons. Tab label 11 px at (+8,+4). The row is drawn after the page frame (on top of it). |
| Dropdown button | `bg_border_fill` box, label (left, 11 px, inset 8) and a 24×24 down-arrow glyph at its right end; click opens a list under the control (1.3); choosing a row sets the label. |
| Field with list | A text field with a 20×24 arrow button over its right end (x from field right − 20, y − 1); the arrow opens a list under the field; choosing a row puts its label into the field. |

### 7.8 Dialog summary

| Dialog | Title | W×H | Growth (W, H) | Grip | Modal | Enter confirms | Escape cancels | Buttons (left to right) |
|---|---|---|---|---|---|---|---|---|
| Preferences | Preferences | 498×538 | 200, 400 | 24 | no | no | yes | OK, Cancel |
| Grid size | Grid size | 420×314 | 200, 0 | 16 | no | no | yes | OK, Cancel |
| Voxel Model Setup | Voxel Model Setup | 420×246 | 200, 0 | 16 | no | no | yes | Apply, Cancel |
| Voxelization | Voxelization | 420×214 | 200, 0 | 16 | no | no | yes | OK, Close |
| PNG Import | PNG Import | 420×224 | 200, 0 | 16 | no | no | yes | OK, Close |
| Model Properties | Model Properties | 472×524 | 200, 200 | 24 | yes | no | (ignored) | OK, Cancel |
| Text prompt | caller ("Edit" in the data) | 400×172 | 200, 0 | 24 | yes | yes | (ignored) | OK, Cancel |
| Vector prompt | caller ("Edit") | 400×172 | 200, 0 | 24 | no | yes | yes | OK, Cancel |
| Color Picker | caller ("Color Picker") | 304×522 | none | none | no | yes | yes | OK, Cancel |
| Change Faces Textures | Change Faces Textures | 420×438 | 200, 0 | 16 | no | no | yes | OK, Close |
| Register (UI only) | (product name) | 520×295 | 200, 0 | 24 | yes | no | (ignored) | Trial Mode, OK, Quit |
| Progress | Progress | 400×172 | 200, 0 | 24 | yes | no | (ignored) | Abort |
| About | none | 640×370 | none | none | sets the flag | n/a | n/a | none (9.12) |
| Message box | caller | 360×180 minimum, automatic | 200, 200 | 16 | sets the flag | n/a | 8.5 | 1 to 3 (section 8) |

"Close" and "Abort" buttons are the Cancel button relabelled.

## 8. Message box

### 8.1 Layout

| Part | Rect (W×H default 360×180) | Notes |
|---|---|---|
| Title | 24,16 (W−40)×30 | middle-left |
| Icon | 32,56 48×48 | per kind (8.3) |
| Message | 96,54 (W−128)×(H−118) | 11 px, top-left, wraps; rich text allowed |
| Buttons | right-aligned row ending at x W−24, y H−50, cells 100×28, 4 apart | visible buttons in order button 1, 2, 3 from left to right; hidden buttons take no space. At W = 360: three buttons at x 28 / 132 / 236, two at 132 / 236, one at 236. |
| Close X | (W−40),16 24×24 | |
| Drag strip | 16,16 (W−64)×32 | |
| Resize grip | (W−32),(H−32) 16×16, growth +200 / +200 from 360×180 | |

### 8.2 Automatic size

On every show the size is recomputed from the message text (font size f = 11):

- c = f / 1.78 (assumed character width), l = 2·c (assumed line height).
- Split the text on line breaks into n parts; m = length of the longest part (in UTF-16 code units).
- W = clamp(trunc(m·c + 96), 360, 540).
- lines = n + Σ over parts of trunc(length·c / (W − 96)).
- H = max(trunc(lines·l + 140), 180).

Then the box is centred like a dialog (7.2) but with x = trunc((screen width − W·D) / 2) and y computed the same way (divide, then truncate).

### 8.3 Kinds

| Kind | Icon (source name) | Sound |
|---|---|---|
| Check | sign-check | pop_check |
| Question | sign-question | pop_question |
| Warning | sign-warning | pop_warning |
| Error | sign-error | pop_error |
| Information | sign-info | pop_info |
| Help | life-buoy | pop_info |
| Parameters | tools | pop |
| Idea | light-bulb | pop_info |
| Serial | key | pop_warning |
| System | cogs | pop |
| Lock | lock | pop |
| Unlock | lock-open | pop |
| Wait | time | none |
| Sketchfab | sketchfab | pop_info |

The sound plays once when the box opens, at volume 0.4. cuboid ships its own icons and sounds.

### 8.4 Buttons and callbacks

- Opening a box first closes the built-in file browser if it is open, then sets the global modal flag, sets title, text, icon and sound, and sets the three button labels; an empty or absent label hides that button. Button 1 receives keyboard selection.
- All previous handlers are removed; each of the three buttons and the X then get "close the box" (clears the global modal flag and hides the box), followed by the caller's handler if one is attached:
  - one handler: on button 1 and the X;
  - two handlers: the first on button 1, the second on button 2 and the X;
  - three handlers: on buttons 1, 2, 3, the third also on the X.
  So the X always behaves like the last button that has a handler, or simply closes.
- The box itself does not block other windows; whether the main UI can still be clicked while it is open depends on the dimming layer (8.6). Each opening is written to the log.

### 8.5 Keyboard, modal flag and quirks

- Escape, on key release, while the global modal flag is set, activates the message box's X (close plus the X's handlers), **even when no message box is visible**. Consequences in the original: Escape inside a modal dialog, the About box or the file browser re-runs the handlers attached to the X by the last message box and clears the global modal flag although the dialog stays open (menu shortcuts become live again). cuboid should restrict this to a visible message box (deliberate fix) unless strict parity is wanted.
- Return does nothing special; it activates the selected button through normal UI navigation (UNVERIFIED).
- Clicking a button closes the box first, then runs the handler; a handler may open another box immediately.

### 8.6 Dimming layer

While the global modal flag is set, a full-window black layer behind all dialogs fades towards 50 % opacity (each frame: colour = lerp(colour, target, 2·Δt)); when the flag clears it fades towards transparent four times faster and is removed below 2 % alpha. It covers the main UI but not the dialogs layer. Whether it blocks pointer input to the main UI: inferred yes, UNVERIFIED.

### 8.7 Standard calls

Info box = Information kind with "OK"; Error, Warning, Check, Wait, Sketchfab boxes likewise with a single "OK" unless other labels are given; Question box = Question kind with "Yes", "No". Callers override labels as listed in section 5.

## 9. Dialogs

Rects are dialog-local at the default size. Fields list: label (exact text), control, limits, initial value, and effect.

### 9.1 Preferences

Opened by Tools > Preferences... Non-modal. Every field is loaded from the current settings on each show; nothing is applied until OK. Cancel / X / Escape discard all edits.

**Layout.** Tabs row at 32,56, 25 high, fixed tab rects: General x 32..102.67, Display 102.67..173.33, Workspace 173.33..254, Paths 254..314.67, Formats 314.67..385.33, Advanced 385.33..466 (each tab = label width + 32.67 at 11 px; recompute from measured text if the font differs). General is selected at start; afterwards the last selected tab stays selected between shows for the rest of the session (nothing resets it; inferred). Page frame 32,80 (W−64)×(H−152); page area 40,92 (W−80)×(H−172) (418×366). Groups stack vertically at full page width with gaps of 14 (General, Workspace) or 12 (Display, Paths, Formats, Advanced). OK and Cancel are direct buttons (7.1); grip 24×24.

**General page**

| Group (y, h) | Control | Details |
|---|---|---|
| Signature (92, 94) | text field, full width, at y 104 (limit 256) | Settings key `signatureTemplate`. Below it at y 133, 359×46, a help text listing the keywords, one line each: "<keyword> will be replaced by <value>" for `%author` (account name), `%email` (account e-mail), `%cubik` (application name). cuboid: same mechanism with its own name; `%author` / `%email` take values cuboid stores (empty when unknown). |
| Backup (200, 68) | "Delay between backups (minutes):" (middle-left, 24 high at y 212) + integer field 64×24 at x 244 (limit 6, placeholder "Minutes") | Key `backupEveryMinutes`. OK stores max(5, value) (unparsable text gives 5). Under it at y 236, 9 px: "Backup folder:\n<backup folder path>". |
| File browser (282, 87) | three checkboxes at y 294 / 318 / 342 | "Use Native file browser if possible" (`useNativeFileBrowser`), "Show hidden files in Cubik file browser" (`showHiddenFiles`), "Re-add default favourites in Cubik file browser on launch" (`readdDefaultFavourites`). |

**Display page**

| Group (y, h) | Controls | Keys and effects on OK |
|---|---|---|
| Render (92, 208) | eight checkboxes, rows 24 apart from y 104 | "Use orthographic projection for main view" (`mainCamOrtho`, switches the main view projection), "Show elements color" (`useElementsColor`, refreshes elements), "Emulate elements tint colors" (`emulateElementTintColor`, refreshes elements), "Display marker at the center of 3D space" (`centerPoint`), "Show hidden elements as white transparent box" + line break (`renderHiddenElements`), "Show hidden elements in main view" (`renderHiddenElementsMainView`), "Enable anti-aliasing (fps impact)" (`renderAntialiasing`: 4× MSAA on / off), "Enable shadows for voxels (fps heavy)" (`renderShadows`). |
| Preview (312, 68) | checkbox "Maximize main view when entering Preview mode" (`maximize3dViewOnPreview`) at y 324; row at y 348: "Third person view with Minecraft skin:" + text field from x 258 to the right edge (limit 64, placeholder "Enter Player name...") | `minecraftSkin`. On OK, when the text differs from the stored value or is empty, it is stored and the preview skin is reloaded (display-settings spec). |
| UI (392, 44) | "Skin theme:" (64 wide) + dropdown 248×24 at x 122 | List rows "Light", "Dark" (3.8). Shown value: stored theme, or "Light" when none. On OK, if changed and the theme exists, it is stored (`appSkin`) and applied immediately. |

**Workspace page** (panels shown while no edit mode is active)

Hint line at y 92: "Check the panels to display when no edit mode is selected" (11 px). Two equal columns from y 122, 8 apart (each (418 − 8)/2 = 205 wide at the default size):

| Column | Group (y, h) | Checkboxes (settings key) |
|---|---|---|
| left | Element (122, 112) | "Create element (cuboid)" (`openElementCreate`), "Tools for elements" (`openElementTools`), "Elements list" (`openElementsList`), "Elements material" (`openElementsMaterial`) |
| left | Voxel (246, 64) | "Tools for voxels" (`openVoxelTools`), "Gizmos options" (`openGizmosTools`) |
| left | Mesh (322, 88) | "Create primitive (mesh)" (`openMeshCreate`), "Tools for meshes" (`openMeshTools`), "Meshes list" (`openMeshesList`) |
| right | Misc (122, 88) | "Tools to paint on surfaces" (`openPaintTools`), "Color selector" (`openColorTools`), "Color palette" (`openPalette`) |

On OK the panel visibility for the "no edit mode" state is re-evaluated at once.

**Paths page** (seven groups, 42 high, at y 92, 146, 200, 254, 308, 362, 416). Each holds a text field from the left edge to 32 units short of the right edge (24 high, at group y + 10, limit none) and a 32×24 "..." button at the right edge.

| Group caption | Key | Placeholder | "..." opens |
|---|---|---|---|
| Load/Save default path | `pathLoadSave` | "Enter path where .cub files are located..." | folder picker "Select default folder to load/save CUB" |
| Import default path | `pathImport` | "Enter path where files to import should be located..." | folder picker "Select default folder to import files" |
| Export default path | `pathExport` | "Enter path where files to export should be located..." | folder picker "Select default folder to export files" |
| Textures default path | `pathTextures` | "Enter path where textures are located..." | folder picker "Select default folder to load/save textures" |
| Meshes default path | `pathMeshes` | "Enter path where .obj files are located..." | folder picker "Select default folder for custom meshes" |
| Minecraft | `pathMinecraft` | "Enter path to Minecraft folder..." | folder picker "Select folder to Minecraft" |
| Texture editor | `pathTextureEditor` | "Enter path to Texture editor executable file..." | executable picker (below) |

- Folder pickers always use the built-in browser in folder mode (10.1), start in the folder typed in the field, or at "/" when it is empty; the chosen folder replaces the field text.
- The executable picker is titled "Select executable file" (the caption-specific title is not used), filter "Executable" (exe), starts in the folder part of the field text, and may use the native dialog (10.1).
- On OK each path is stored only if it names an existing folder or file; otherwise the stored value is silently kept (so a path cannot be cleared from this dialog; quirk).

**Formats page**

| Group (y, h) | Controls (key) |
|---|---|
| Minecraft (92, 38) | "Save textures as png on export (beside .json)" (`minecraftExportTextures`) |
| OBJ (Polygons) (142, 38) | "Center model and origin on import" (`objCenterOnImport`) |
| Sketchfab (192, 44) | text field, full width, alphanumeric only, placeholder "Your Sketchfab token..." (`sketchfabToken`) |
| Import/Export dialog (248, 136) | "Hide Minecraft formats" (`formatHideMinecraft`), "Hide Stonehearth formats" (`formatHideStonehearth`), "Hide Staxel formats" (`formatHideStaxel`), "Hide Voxel formats" (`formatHideVoxel`), "Hide Mesh formats" (`formatHideMesh`) |

**Advanced page**

| Group (y, h) | Controls (key) |
|---|---|
| Atlas (92, 64) | "Add padding between textures (bigger texture size)" (`advAtlasPadding`), "Always generate a square texture" (`advAtlasSquare`) |
| UI (168, 38) | "Show classic tooltips on mouse hover" (`showClassicTooltips`) |
| Keyboard shortcuts (218, 158) | five rows 24 high, 4 apart, from y 230: label 88 wide (middle-left) + text field to the right edge, placeholder "Key names separated by comma": "Rotate mode" (`advKeysRotate`), "Scale mode" (`advKeysScale`), "Clone mode" (`advKeysClone`), "Pan view" (`advKeysPan`), "Focus view" (`advKeysFocus`). Shown value: the stored string, or the binding's current keys in canonical form when the stored string is empty. Format and apply rules: 6.6. |

**OK, in order:** store every field (paths only when they exist, trimmed key strings), re-evaluate workspace panels, apply projection / colour / render flags as they are stored, reload the preview skin if needed, switch theme if changed, apply the five key bindings and re-store them canonically (6.6), write the settings file, hide.

**First-run settings** (no settings file or an unreadable one): `signatureTemplate` "Designed by %author with %cubik - https://cubik.studio" (cuboid: its own text), `backupEveryMinutes` 10, the three file-browser options off, `openElementCreate` on and the other workspace panels off, orthographic off, elements colour on, tint emulation on, centre marker off, both hidden-element options off, anti-aliasing off, shadows off, maximize preview on, classic tooltips on, Minecraft skin empty, theme "Light", load/save, import, export, textures and meshes paths = the user's Desktop, Minecraft path = the detected Minecraft folder (files spec), texture editor empty, Minecraft png export off, OBJ centring on, Sketchfab token empty, all "Hide ... formats" off, atlas padding off, square atlas on, key strings as in 6.6. When a settings file exists but lacks a key, the missing value takes the field's built-in default instead (settings spec), which differs for: hidden elements box (on), OBJ centring (off), square atlas (off), the key strings (empty).

### 9.2 Grid size

Opened by Views > Grid settings (Shift+G). Non-modal. Changes apply live while the dialog is open.

| Row (y) | Title (x 32, 16 high) | Slider (x 32, 292×20, at row y + 18) | Number field (48×24 at x 340, row y + 16) | Setting |
|---|---|---|---|---|
| 56 | "Displayed:" plus checkbox "Large" (64×20 at x 128, y 54) | 2..64 | integer | display grid step = 16 / value (view property `gd`); "Large" = `gl` |
| 104 | "Mouse snap:" | 16..512 | integer | mouse snap step = 16 / value (`gm`) |
| 152 | "Keyboard snap:" | 16..512 | integer | keyboard snap step = 16 / value (`gk`) |
| 200 | "UV mapping snap:" | 16..512 | integer | UV snap step = 16 / value (`gu`) |

- On show each slider = round(16 / step) (document defaults: display 1 → 16, mouse 0.5 → 32, keyboard 0.125 → 128, UV 0.5 → 32) and "Large" = stored flag; the four fields show the slider values.
- Moving a slider sets its step immediately and refreshes the fields. Typing a number and ending the edit sets the slider to the number clamped to its range. The fields have no placeholder.
- OK: keeps everything. Cancel / Close / Escape: restores the display, mouse and keyboard steps saved at show time; the UV step and "Large" are **not** restored (quirk; keep, or restore all as a deliberate fix).
- No undo entry is recorded and the document is not marked modified. The display, mouse and keyboard steps are saved with the document (formats-native spec 4.3), yet a session whose only change is a grid change closes without the unsaved-changes prompt and loses it (5.1). The keyboard step is also the arrow-key nudge distance (6.4).

### 9.3 Voxel Model Setup

Opened by the voxel popup "Setup" and by the "Setup" button of the "Setup Voxels first" box (5.12), with the model's voxel options.

- Group "Settings" 32,56 356×126. Row at y 68: "Number of voxels per axis per zone:" (208 wide, middle-left) + field 64×24 at x 250 (decimal-number input: digits, one ".", a leading "-"; at most 3 characters; placeholder "0"; read as an integer, so text that is not a whole number counts as 0). Ending an edit with a value outside 2..128 rewrites the field with the clamped value.
- Checkboxes (340×20) at y 94 / 114 / 134 / 154: "Voxel at center", "Floor", "Full block", "Adapt visible grid and snap steps". Their meaning is defined by the voxel spec (initial voxel content and whether grid and snap steps are adapted to the voxel size).
- Initial values = the model's voxel options (new model: resolution 16, the three content options off, adapt grid on).
- When the voxel grid is already set up, the field is disabled and the checkboxes are greyed and inactive (read-only view); Apply then only closes the dialog.
- **Apply** (not yet set up): resolution = clamp(integer, 2, 128); store the four flags; mark set up; run the setup (voxel spec; undo entry 'Setup voxels' (voxels)); close. **Cancel** / X / Escape: close, nothing changes.

### 9.4 Voxelization

Opened by "Convert to voxels..." (5.4) when the model has meshes. Non-modal: meshes can still be moved, scaled and rotated while it is open.

- "Resolution:" at 32,56; value label at x 102 ("x" + slider value, e.g. "x16"). Slider 32,74 356×24, 4..128, whole numbers (track 8 high, handle 8×20). "Number of voxels on one axis per zone" at y 104 (18 high). Checkbox "Keep inside empty" at y 124 (initial focus).
- On show: the status hint becomes "Set resolution, move/scale/rotate 3D models and press \"OK\"" (with the quotes); the slider takes the resolution used last in this session (16 at start).
- Changing the slider updates the label only. The checkbox has no immediate effect; it is read when the voxelization starts (passed as "keep inside empty", voxelize spec).
- **OK** (ignored while a voxelization is running, except that the dialog closes): remember the resolution r, then:
  - voxels exist with a different resolution R: Question box "Confirm replace of voxels" / "Selected resolution xr is different from existing voxels xR.\nVoxelization will erase current voxels.\n\nAre you sure you want to voxelize the model?." buttons "Erase and Start", "Cancel"; "Erase and Start" sets up a voxel grid at r and starts;
  - voxels exist with resolution r: start, adding to the existing voxels;
  - no voxels: set up a grid at r and start.
  The dialog closes in every case.
- **Close** / X / Escape: abort a running voxelization, otherwise leave voxelization mode (voxelize spec).

### 9.5 PNG Import

Opened by the PNG import flow (files spec). Non-modal.

- "Position / scale:" at 32,56. Radios (128×20): "Fit in block" at 32,74 (initially on) and "Fit in world" at 160,74. "Defines position of imported sprite" (description) at y 100. Checkbox "Extrude sprite with optimized elements" at y 120 (initially on). "If unchecked, the sprite will be imported as a billboard mesh" at y 144.
- The choices persist for the session (they are not reset when shown again).
- **OK**: passes { fit in world = "Fit in world" selected, extrude = checkbox } to the import, then closes. **Close** / X / Escape: closes without continuing the import.

### 9.6 Model Properties

Opened by Edit > Model Properties (Ctrl+P) and the Model side panel button. Modal. On show, a snapshot of the model properties is taken and every control is filled from it. OK writes all controls back; Cancel / X restore the snapshot (no change). OK records no undo entry and does not mark the document modified, except when it switches "Restrict rotations" on (OK step 1); every other Model Properties change is therefore not covered by the unsaved-changes prompt (5.1).

**Layout.** Tabs at 32,56, fixed rects: General 32..96, Minecraft 96..160, Stonehearth 160..240, Staxel 240..296, QB 296..344, Mesh 344..392, PNG 392..440; selected tab 25 high (y 56..81), unselected tabs 21 high and raised (y 59..80). Page frame 32,80 (W−64)×(H−152); page area 40,92 (W−80)×(H−172) (392×352). Groups stack with gaps of 14. OK and Cancel direct buttons; grip 24×24.

**General tab**

| Group (y, h) | Controls |
|---|---|
| Name (92, 44) | text field (limit 128), placeholder "Model name (required)"; no validation (empty is accepted). |
| Description (150, 44) | text field (limit 256), placeholder "Author, description...". |
| Global options (208, 110) | "Exclude elements", "Exclude voxels", "Exclude meshes", "Compress data" |
| Elements options (332, 110) | "Restrict rotations (required for Minecraft block/item model)", "Avoid z-fighting by applying random offsets", "Avoid bleeding textures by modifying UV mapping", "Exclude hidden elements" |

**Minecraft tab**

- Group "Type" (92, 44): five radios in a row: "Block" (label has a trailing line break), "Item", "Entity", "Optifine JPM", "Map" (x 48 / 118.4 / 188.8 / 259.2 / 361.6, widths 70.4 / 70.4 / 70.4 / 102.4 / 62.4).
- Below it (from y 150) exactly one block is shown depending on the type: Block or Item → Model options; Entity → Entity options; Optifine JPM → Optifine info; Map → Map options.
- **Model options** (150, 164): "Parent:" (44 wide) + text field (limit 128, placeholder "Relative path to parent (leave empty if no parent)"); checkboxes "Include Elements name", "Automatic parent for vanilla display settings", "Include Display settings", "Enable Ambient Occlusion (Block only)", "Keep imported Overrides settings (Item only)". Info (328, 50): "Elements only are exported in this format.\nBe sure to convert voxels and meshes to elements before export."
- **Entity options** (150, 198): "Attach:" + text field (limit 128, placeholder "Bone name (leave empty if not attached)"); integer field 48×24 (3 characters) + "Resolution of generated texture (16, 32, 64, 128, 256 or 512)"; two integer fields (X, Y) + "Offset of generated texture (in 16x resolution)"; two integer fields (width, height) + "Minimum size of entity texture (in 16x resolution)"; checkboxes "Group all elements as unique bone + Use Origin as pivot (MCPE)", "Use same element texture for doublons (smaller texture)", "Use elements label for names". Info (362, 50): same text as Model options.
- **Optifine info** (150, 80): "Elements only are exported in this format.\nBe sure to convert voxels and meshes to elements before export.\n\nRotation restrictions can be disabled in \"General\" tab."
- **Map options** (150, 230, grows with the dialog): multi-line text field 278×210 (placeholder "color code = block name") and buttons 94×24 at x 330: "Auto list" (y 162), "Default list" (y 190), "Clear list" (y 218). Info (394, 50): "Voxels only are exported in this format.\nBe sure to convert meshes to voxels before export."

Automatic parent: whenever the "Automatic parent for vanilla display settings" checkbox changes or the type changes, the Parent field is rewritten: checkbox on → "block/block" (Block), "item/handheld" (Item), empty (Entity), unchanged (other types); checkbox off → empty. Because this also runs when the dialog first initialises (first show in a session) and when showing changes the type radio, a custom parent can be wiped just by opening the dialog (quirk, from code order; runtime UNVERIFIED). cuboid: rewrite the field only on user changes.

Map table field: when not being edited it shows each colour code `#RRGGBB` or `#RRGGBBAA` as a coloured square glyph ("■" in that colour) followed by the code in italics, and the words `empty` / `default` preceded by a red "*" and in italics; while editing it shows plain text (and, when focused, six line breaks are appended to the text if it does not already end with them, quirk). Stored text is always plain.

- **Auto list**: only when a voxel grid exists. Collects the distinct non-empty voxel colours in scan order (y outer, z, x inner). More than 256: Error box "Too many colors" / "Your map contains N colors...\nMap can't have more than 256 colors!\n\nPlease use less different colors.". Otherwise the table becomes "empty = air\n", "default = stone\n", then one line "#RRGGBBAA = color_<palette index>\n" per colour (8 upper-case hex digits).
- **Default list**: "empty = air\ndefault = stone\n#5A9643 = grass\n#A46939 = dirt".
- **Clear list**: empty.

**Stonehearth tab**: Entity forms (92, 64): "Placeable on ground", "Placeable on walls". Values (170, 68): "Base value of object in game (in gold):" (200 wide) + integer field 48×24 at x 254 (no limit); "Rarity (in a shop keeper inventory):" + name-type field 96×24 at x 254 (limit 32; auto-capitalises words). Shop info (252, 64): "Buyable", "Sellable". Info (330, 82): "Stonehearth export is hugely in development.\n\nExporting for Stonehearth generates a basic config file (json) along the Qb file (model). Edit json to make the whole work in game."

**Staxel tab**: Type (92, 44): radios "Tile", "Item", "Accessory", "Other" (89.75 wide each). Info (150, 142): "Staxel export is hugely in development.\n\nSelecting a type will generate a default config file along the Qb file (actual model). You'll have to modify the config file in a text editor to make it work in game.\n\nPlease follow tutorials to properly mod Staxel:\nhttp://forums.playstaxel.com/forums/modding.28"

**QB tab**: Format options (92, 62): "Use RGB color (instead of BGR)", "Right handed axis (inverted Z)". Info (168, 96): "QB file is a binary format containing voxels only.\n\nIf you export for Stonehearth or Staxel, the format options are not took in account. If you export for anything else, be sure to use right format options the game is compatible with."

**Mesh tab**: Voxel options (92, 158), rows in this order: radio "Detailed mesh with colored vertices (no texture)", checkbox "Render fake ambient occlusion", radio "Detailed mesh with texture palette", checkbox "Smooth voxels vertices on export", radio "Optimized mesh with texture atlas", checkbox "Add padding to textures in voxel atlas on export" (the three radios form one group). Format options (264, 64): checkboxes "Weld vertices at exact same coordinates", "Merge all objects in same mesh". Info (342, 96): "Obj format with vertex colors requires a compatible importer (some 3D modeler software can't import such format).\n\nIn Unity and Unreal Engine, a mesh with vertex colors will need a specific Shader to properly render colors."

**PNG tab**: Render type (92, 68): radios "GUI display preview", "Main view". Options (174, 38): "Anti-aliasing". Info (226, 54): "Exporting to PNG will produce a render of the model saved to a picture file encoded in PNG 24bits + alpha."

**OK**, in order:
1. Restrict rotations: if the model's stored flag is off and the checkbox is on: N = number of elements with unrestricted rotation angles; N > 0 → Question box "Recalculate elements rotation" / "Your model contains N element" + ("s" when N > 1) + " with un-restricted rotation angles.\n\nDo you want to force restricted rotation?" buttons "Force", "Cancel" ("Force" turns the flag on and snaps those rotations, element spec; "Cancel" leaves it off); N = 0 → turn the flag on and snap at once. Turning the flag on records the undo entry 'Restrict elements rotation' (scope model properties + elements), so it marks the document modified; the entry is recorded after the flag is set and before the rotations are snapped. Quirk (from code order): undoing that entry restores the element rotations but leaves the flag on; when it is recorded without the question box (N = 0) its snapshot also predates step 2, so undoing it also reverts the other fields written by the same OK. **In every other case the flag is turned off silently**, including when it was on and the checkbox is still on (defect: every OK clears an already-set flag). cuboid: keep the flag on when it stays ticked (deliberate fix; parity tests must not depend on the defect).
2. Write every other control: name, description, exclusions, compress, z-fighting, bleeding, hidden-element exclusion, parent, type, the five model options, attach, entity resolution = nearest power of two of clamp(value, 16, 512) (a value exactly between two powers, e.g. 48 or 24, is assumed to go to the larger one: UNVERIFIED), UV offset X / Y and texture width / height = max(0, value), the three entity options, map table (plain text), Stonehearth values (gold as integer, rarity as text), Staxel type, QB options, mesh options, PNG options.
3. Refresh the model and the voxels (title, export state), close. The prompt from step 1 may still be open after the dialog has closed.

### 9.7 Change Faces Textures

Opened by Elements popup > Textures > Customize... (guard: elements). Non-modal.

- Six groups 32,y 356×42 at y 56 / 110 / 164 / 218 / 272 / 326, captioned "Down", "Up", "North", "South", "West", "East". Each holds a field with list 340×22 at (40, y + 12) (limit 64, placeholder "Texture label..."), its arrow opening the texture label list (3.5).
- On show each field = the primary selected element's texture label for that face. Typing, or choosing a label from the list, only changes the field (3.5).
- **OK**: undo entry 'Custom faces texture' (elements), recorded before any write. Every selected element (other selected objects are skipped) gets all six labels exactly as shown (empty allowed, no validation), including faces the user did not touch, so the other selected elements lose labels of their own that differed from the primary's. For each element the face UVs are recomputed from the new labels (vertex positions and colours unchanged; unknown or empty labels map to the "error" fallback, model-and-tools spec 4.6) and an element-changed notification is sent; one more notification without an element follows; close. **Close** / X / Escape: nothing.

### 9.8 Color Picker

Opened with a title and an initial colour by Set Color... (elements and meshes, 5.4 / 5.5). Non-modal; Enter confirms. Not resizable.

| Part | Rect | Behaviour |
|---|---|---|
| Swatch | 32,56 116×28 | current colour over a tiled checker (`bg_pattern_small` tinted #8c8c8c40) |
| Hex field | 156,56 116×28, placeholder "#000000" | shows `#RRGGBB` (no alpha). On end of edit accepts `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` (the "#" optional, hex digits either case); a 3- or 6-digit form sets alpha to 255; anything else is ignored. |
| Saturation / value square | 50.5,92 178×178 | x = saturation, y = value; 20×20 ring handle (`bullet`) |
| Hue strip | 247,97 20×168, vertical, bottom = 0 | 28×8 bar handle #828282 |
| Seven slider rows | 16 high each from y 278: R, G, B, gap 15, H, S, V, gap 15, A | letter at x 29 (15 wide), slider 49..229 (8 high gradient band, 8×16 handle #828282), value at x 234 (25 wide) = floor(component × 255) for every row, including H, S and V (so hue shows 0..255, not degrees). |
| Presets | row at 35,433 234×20 | first slot (18×18) is a transparent swatch; then a "+" button (20×18, shows the current colour with a "+" glyph); up to nine more swatches appear to its right as they are created. "+" (only when alpha > 0) fills the first unused slot with the current colour, but stops without adding when it meets a slot whose stored colour equals the current one (unused slots store white, so opaque white can never be added, quirk). Clicking a swatch sets the current colour (the transparent one sets fully transparent black). Presets last for the session. |
| OK / Cancel | 54,473 / 170,473 110×26 | |

**OK** returns the current colour as `#RRGGBBAA` (8 upper-case hex digits) to the caller. Cancel / X / Escape return nothing.

### 9.9 Text prompt

Used for rename, select-by-name, texture label and shortcut prompts (titles in section 5). Modal; Enter confirms (on key release); Escape does **not** cancel because the dialog is modal (8.5 applies).

- Group "Enter new name" 32,56 336×44 with a text field 40,68 320×24 (limit 128, placeholder "Enter text..."). Title and initial text come from the caller; the group caption never changes.
- **OK** passes the text unchanged (no trimming; empty allowed). Cancel / X: nothing.

### 9.10 Vector prompt

Used by Move, Set position, Set size, Scale, Inflate, Randomize (5.4). Non-modal; Enter confirms; Escape cancels.

- Title and group caption ("Enter new vector" in the data, replaced by the caller's description such as "Offset (leave empty to not modify)") come from the caller.
- Three decimal fields X, Y, Z at 40 / 148 / 256, y 68, 104×24 each (limit 128, placeholder "-"). On show each field is empty for an absent component, otherwise the number in plain invariant form (e.g. "0.5", "-2", "16").
- **OK** passes three components: empty field → "absent"; otherwise the parsed number (unparsable → 0). Cancel / X / Escape: nothing.

### 9.11 Progress

Shown automatically whenever a long task reports a progress value strictly between 0 and 0.5 (each report re-shows and re-centres it). Modal.

- Bar 32,56 272×20: track (`bg_panel` #f0f0f0, 10 high at y 61), fill #42e11a proportional to the value, no handle; value label at 328,58 48×16 = trunc(v × 100) + "%". Description 32,72 336×32 (middle-left) = the current status-bar text.
- Every frame while shown: description and value are refreshed; at value ≥ 1 it closes itself.
- **Abort** (the only button) / X: closes the dialog. No task registers an abort action, so the task keeps running and the dialog reappears with the next report below 50 % (from code; runtime UNVERIFIED). cuboid: either wire a real abort or keep the dialog informational.

### 9.12 About

Opened by Help > About. Not a framework dialog: no title bar, no buttons, no drag (its drag strip is disabled), no resize.

- 640×370 image card (original artwork tinted #ffffffe6); version line at 41,134 551×16, white: "Version <version> <x64|x86>", the version being the application version text (original "2.9.480 Beta").
- A scrolling text area 41,154 551×126 (clipped) containing a description block then a credits block (bold headings, centred). It stays at the top for 5 seconds after opening, then scrolls upwards at no less than 20 units per second; reaching the end it jumps back to just above the start and loops. Holding the left mouse button skips the initial wait; the area can also be dragged.
- Footer at 74,290 520×40, 9 px: copyright and a link line.
- While shown it sets the global modal flag and clears it when hidden. It closes when the left button is pressed outside the card and released outside it; Escape does not close it (8.5 applies).
- Placement: centred with fractional (not truncated) coordinates.
- Branding: cuboid uses its own artwork, version text, description, credits and footer; none of the original credit text is reused.

### 9.13 Register (UI only)

Licence handling is out of scope; cuboid has no registration and omits this dialog and the Help row (2.8). Described only for completeness.

- Modal, 520×295. A decorative column image at 17,17 81×261; title at 108,16 396×30; intro text 108,56 380×32; "Your ... account email address:" label 108,96 and e-mail field 108,114 380×23 (e-mail type, limit 255, placeholder "email"); serial label 108,146 and serial field 108,164 380×23 (limit 36, placeholder "serial"); a 16×16 valid / invalid mark (green check #42e11a / red cross #e11a1a) at x 468 inside each field; note text 108,200 380×32. Buttons 110×26 at y 246: "Trial Mode" x 104, "OK" x 270, "Quit" x 386.
- Fields are pre-filled (e-mail lower-cased, serial upper-cased); every edit re-validates both and swaps the marks; OK is enabled only when both are valid and "Trial Mode" is visible only when they are not. "Quit" and the X quit the application.

## 10. Built-in file browser

### 10.1 When it is used and how requests are configured

- **Open-file requests** (Open, Merge, Import, texture / mesh texture / palette loading, the Preferences executable picker): when Preferences "Use Native file browser if possible" is on, the native OS open dialog is tried first (filters from the request plus "All files"); the built-in browser is used only if the native dialog is unavailable. A native result that is empty or names a missing file counts as cancel.
- **Save requests** (Save As, Export, texture and palette saving): same rule with the native save dialog. Default name "Untitled" when the caller gives none. Defect: when the caller gives no default extension, the default **name** becomes "cubik" and no extension is set.
- **Always built-in:** folder pickers (Preferences paths) and the view-background loaders (5.7).
- Built-in request settings: mode Open or Save; files selectable and folders not (folder pickers: folders only, filter "folder"); hidden entries listed per Preferences "Show hidden files in Cubik file browser" (folder pickers: never); window title from the request (default "Cubik File Browser"; the background loaders set none, so the previous title stays); only one item may be submitted; in Save mode an extension is always required (no "All types" item).
- Filters: parallel lists of extensions and descriptions; extensions are lower-cased, a leading "." removed, empty entries dropped. A request with filters for an executable or image picker lists only files of those types under "All types"; other requests list every file under "All types".
- While the browser is open the global modal flag is set (7.3).
- Result delivery: on success the caller receives the first selected path. Whenever the browser closes, for any reason **including after a success**, the caller's cancel handler runs as well (quirk; cuboid callers must tolerate it or cuboid fixes it consistently).

### 10.2 Window

- Lives in its own layer above the main UI (its order relative to the dialogs layer: UNVERIFIED; message boxes close the browser before opening, 8.4), scaled with the UI scale. Created on first use at top-left (30, 30), 640×464; it is hidden, not destroyed, on close, so size and position persist for the session (not across sessions).
- Moves when dragged on any empty area of the window, only while the pointer is inside the application window. Resize handles lie on the visible frame (the window rect inset by 16): edge strips 12 thick centred on each edge and 16×16 squares centred on each corner; dragging an edge or corner resizes from that side (the opposite side stays fixed), minimum 450×250, maximum 1024×800 and never beyond the screen. Hovering a handle shows the matching resize cursor.

Window-local rects at the default size (W×H = 640×464); anchoring in brackets:

| Element | Rect | Notes |
|---|---|---|
| Title | 24,16 (W−88)×30 | 11 px middle-left |
| Close X | (W−40),16 24×24 | closes (cancel) |
| Back, Forward, Up | 24,48 / 54,48 / 84,48, 26×26 [top-left] | arrow glyph 16×16 centred |
| Path field | 110,48 (W−310)×26 | placeholder "Enter text..."; shows the current folder's full path |
| Reload | (W−200),48 26×26 [top-right] | refresh glyph |
| Drives | (W−171),48 26×26 [top-right] | drive glyph |
| Home | | exists but never shown |
| Filter field | (W−142),48 118×26 [top-right] | placeholder "Filter..." |
| Clear filter | (W−48),48 24×24 [top-right] | small cross over the filter field's right end |
| Locations panel | 24,78 (0.3·W − 26)×(H−142) | frame `bg_content`; vertical list with a 10-wide scroll bar at its right edge |
| File list | (0.3·W + 2),78 (0.7·W − 194)×(H−142) | frame `bg_content`; 10-wide scroll bar |
| Preview | (W−192),78 168×(H−142) [right] | image zone 168×128 at its top (backdrop #00000010); icon 64×64 at (W−140),110; info text at (W−184),212, 152×(H−284), 11 px top-left |
| Add to favourites, New folder, Delete, Open in OS | 24 / 54 / 84 / 114, (H−50), 26×26 [bottom-left] | glyph buttons |
| File name field | 144,(H−50) (W−414)×26 | Save mode only; placeholder "Enter filename..." (not shown) |
| Extension button | (W−266),(H−50) 74×26 [bottom-right] | `bg_content` box, label 11 px middle-left inset 7 |
| Open / Save | (W−188),(H−50) 80×26 [bottom-right] | "Open" in Open mode, "Save" in Save mode |
| Close | (W−104),(H−50) 80×26 [bottom-right] | "Close" |
| Prompt overlay | 16,16 (W−32)×(H−32) | #00000080 while any prompt is open; clicking it closes all prompts without action |
| Loading indicator | 50×50, centred | shown while a folder is being scanned (animated picture; look UNVERIFIED) |

Glyph buttons use `bg_button` with a 16×16 glyph in `text` colour; cuboid ships its own glyphs.

### 10.3 Opening

1. Store filters, configure the window for the mode (file-name field and Save button only in Save mode; Open button only in Open mode, initially disabled), set the title and apply the extension default (10.6).
2. Start folder = the requested folder. If it does not exist, show the warning prompt "Couldn't open directory: <path>" and list the file-system root ("/"; on Windows the drive list) instead.
3. Save mode: the file-name field = requested name (else "Untitled") and receives focus; the selected extension = the requested default extension, else the first filter.
4. Load the stored locations (10.10) and build the Locations panel.

### 10.4 Listing

- **Order.** Drives first, then folders, then files (drives only occur in the drive list). Within one kind, names are compared with the natural order below; entries that compare equal keep the order in which the OS listed them. The name is the last path segment including the extension; a drive's name is its letter, a colon and "/" (e.g. "C:/"), so drives sort by letter. The filters below remove entries without changing the order.
- Folders and drives are always listed; a drive is listed only if its root folder can be read. Files: with an extension selected, only files with that extension; with "All types", every file (or only filter-list files and extension-less files for executable / image pickers). Hidden entries are listed only when hidden files are shown, and then drawn at 55 % opacity.
- Search: when the filter text is non-empty, only entries whose name matches `*<text>*` are listed (whole-name, case-insensitive; `*` and `?` typed by the user act as wildcards). The filter applies when Return is pressed in the field (10.7) or on the next reload; "Clear filter" empties it and reloads.
- A folder whose modification time changed since it was scanned is rescanned. Reload keeps the scroll position; navigating to another folder scrolls to the top.
- Rows are 25 high. Background: `bg_button` frame whose base tint (files #ffffff82; folders, drives and the location headers #d8e7ff82) is multiplied by a state colour crossfading in 0.1 s: normal #ffffff00 (invisible), hover #c5e7ff, selected #2195dbc0, pressed #a5cae5 (header rows never highlight). Icon 16×16 at x 8, vertically centred (glyph colour #363636); name label from x 34, 11 px, colour #323232, middle-left, single line. Header rows read "Recent:" / "Favorite:" with the label at x 12 (their history / star glyph sits at x −12, outside the row, quirk). Hovering a row whose label is truncated for 500 ms shows a tooltip with the full name 10 units to the right of the pointer (a `bg_content` box sized to the text with padding 10 left / right and 5 top / bottom, 11 px #323232); it hides when the pointer leaves the row.
- Icons by category; for files the first category whose list contains "<extension>;" wins (substring test on the lower-case extension without the dot, so e.g. "ml" matches "xml;"; quirk). No match: the generic file glyph.

| Category | Extensions (as listed in the original) |
|---|---|
| Drive | (drives) |
| Folder | (folders) |
| Voxel | nbt vox qb qef schematic zox |
| Mesh | 3dm 3ds blend dxf fbx gltf max obj stl ply x3d |
| Boxes | cub cubx json jpm mcpe |
| Image | bmp dds gif jpg jpeg png psd tga thm tif tiff yuv |
| Audio | aif iff m3u m4a mid mp3 mpa ogg wav wma |
| Script | apk app bat cgi com gadget jar pif sh wsf java |
| Text | css doc docx "htm:html" (typo: neither htm nor html matches) js log msg odt pages pdf rtf tex txt wpd wps cfg ini xml |
| Video | 3g2 3gp asf avi flv m4v mov mp4 mpg rm srt swf vob wmv |
| System | cab cpl cur dll dmp drv icns ico sys |
| Archive | 7z cbr deb gz pkg rar rpm sitx tar zip zipx |
| Material | mtl mat |
| Windows executable | exe |
| Linux executable | x86 x86_64 x64 |
| Shortcut | lnk |
| Model (own format) | cubik |

**Natural name order.** Names a and b are compared as follows. Digits are all Unicode decimal digits. Text comparisons ignore case and use the collation of the invariant culture (the application runs with the invariant culture).

1. Split each name into alternating runs of digits and non-digits, and compare the runs pairwise from the left.
2. Two digit runs compare by numeric value, of any length, ignoring leading zeros. If the values are equal but the runs differ in length, the longer run (more leading zeros) sorts first, and this decides the comparison at once ("File 01.cub" before "File 1.cub", "a01c" before "a1b"). Equal values written in different digit scripts compare equal here; the first such case is remembered as a tie-break and replaces a remembered text tie-break.
3. Two non-digit runs that are equal: continue. If they differ in base characters (a difference that would still decide if different digits followed), the result is the collation's comparison of the rest of both names, from these runs on. If they differ only below that level (accents; in the Windows collation also hyphens and apostrophes, which it ignores at the base level), the first such difference is remembered as a tie-break and the scan continues.
4. A digit run against a non-digit run: the result is the collation's comparison of the rest of both names, so digits are not compared by value here ("a 10" before "a2", "File 1.cub" before "file1.cub": space sorts before digits).
5. If one name ends first, it sorts first. If both end together, the result is the remembered tie-break, or else equal.

Examples that hold under common collations: file2 < file10, a01 < a1, 00 < 0, "a" = "A", v1.9 < v1.10, "New Folder (2)" < "New Folder (10)", "a b" < "ab". Examples under the Windows collation only: e1 < é1 and "é 1" < "e 2" (accent deferred), ab < a-b (hyphen ignored at the base level). Sorted sample: folders "folder 1", "Folder 9", "Folder 10"; files "File 01.cub", "File 1.cub", "file1.cub", "file2.cub", "file10.cub", "notes", "readme".

Quirk: rule 4 compares characters while rule 2 compares values, so the order is not transitive for some names that mix digits with characters the collation ignores. With the Windows collation, 9 < 10, 10 < "-5" and "-5" < 9 form a cycle, and the displayed order of such names depends on the sort algorithm and on the OS listing order. The original runtime (Mono) may collate differently (UNVERIFIED), so the placement of hyphens, apostrophes and accents and these cycles are not parity targets. cuboid: implement rules 1-5 with one fixed case-insensitive collation in which punctuation is not ignorable (this removes the cycle above), break remaining ties by an ordinal comparison of the names so that the order is repeatable, and split and case-fold each name once per listing rather than in every comparison.

### 10.5 Selection and activation

- A click is the left button released on the row it was pressed on. Plain click: select only that row (clicking the only selected row deselects it) and clear the other panel's selection. Ctrl-click (Cmd on macOS): toggle the row and make it the range anchor. Shift-click: select the range from the anchor.
- A second click in the same panel within 300 ms activates the clicked row (it need not be the row clicked first): folder → open it; file → submit (Open or Save); drive → nothing (quirk). Location rows behave like their target (folder: open it; file locations: UNVERIFIED).
- Every click refreshes the preview. In Save mode, clicking a file puts its name without extension into the file-name field.
- Ctrl+A (Cmd+A) or F12: select every row of the focused panel.
- The Open button is enabled only while the selection would be accepted (10.8; exact refresh points UNVERIFIED).

**Preview.** Image files (png, jpg, jpeg, dds, tga) are decoded and shown in the image zone with preserved aspect; other files and folders show their category icon. Info text (rich text):

- file: `<b>name</b>`, blank line, `<size=10>Size:</size> <size>`, image only: `<size=10>Dimensions:</size> <w> x <h>`, blank line, `<size=10>Modified:</size> <date>`, `<size=10>Created:</size> <date>`;
- folder: name, blank line, Modified, Created.

Size: divide by 1024 (integer) while ≥ 1024, up to GB; written without decimals and without space, e.g. "512B", "1KB", "15MB". Dates are UTC as `dd/MM/yyyy HH:mm`. Repopulating the list resets the preview to the default icon and "Select a file...". (The original decodes the image on every click and never frees it; cuboid should cache or free.)

### 10.6 Extension list

- Items: one per filter, "<EXTENSION>: <description>" (extension upper-cased), then "All types" in Open mode. The button toggles the list only when there are two or more filters; every other browser action closes it.
- The list opens above the button, right-aligned with it, 238 wide, height min(items × 26, 320), scrollable (8-wide scroll bar). Items are 26 high: `bg_button` frame with base tint #ffffff82 and button state colours, label at x 5, 11 px #323232, middle-left.
- Choosing an item selects that extension ("All types" selects none) and reloads the listing.
- Button label: the selected extension in lower case; with none selected, "All types" when there are two or more filters, otherwise the only filter's extension.

### 10.7 Text fields

A field acts only when editing ends with Return or keypad Enter (the Submit binding); losing focus does nothing.

- Validation first. Path: after removing a leading drive prefix ("X:/" or "X:\"), reject control characters and `"` `<` `>` `|` `:` `*` `?` → warning "Path cannot include the following characters: " followed by those characters. File name and new-folder name: reject characters not allowed in file names → warning "Input cannot include the following characters: " followed by the list (on Windows `"<>|:*?\/`). Filter: always accepted.
- Path: backslashes become "/", aliases are expanded (10.11), then the browser opens that folder if it exists (otherwise nothing happens).
- File name: same as pressing Save.
- Filter: becomes the search text; the listing reloads.

### 10.8 Toolbar actions and submit rules

| Action | Effect |
|---|---|
| Back / Forward | move in the visit history, skipping folders that no longer exist (history is cleared when the browser closes) |
| Up | open the parent folder |
| Reload | rescan, keep the scroll position |
| Drives | Windows: drive list; macOS: /Volumes; Linux: /run/media/<user> if it exists, else /media |
| Add to favourites | add every selected row's path to Favourites |
| New folder | open the new-folder prompt |
| Delete | open the delete prompt when something is selected |
| Open in OS | show the first selected row, or else the current folder, in the OS file manager (5.8) |
| Close, X, Escape | close (cancel) |

**Open:** nothing selected → nothing happens; more than one item → warning "Please select only one item."; a folder when folders are not allowed → "Please select files only."; a file when files are not allowed → "Please select folders only."; otherwise deliver the full path, add the current folder to Recent (unless only locations were selected), close.

**Save:** empty name → warning "Please enter a file name". Path = current folder + "/" + name + "." + selected extension; the extension is always appended, even when the name already ends with it ("a.cubik" → "a.cubik.cubik"; quirk). If that file exists → overwrite prompt; "Overwrite" continues. Deliver the path, add the current folder to Recent, close.

**Close** (any path): save the stored locations, clear the global modal flag, run the caller's close handler, clear the history, hide.

### 10.9 Prompts

Prompts are separate small windows centred on the browser window (inferred), draggable by any empty area, not resizable, drawn above it; the prompt overlay (10.2) dims the browser while any prompt is open. They always use the dark frame and dark buttons with white text (also in the Light theme; placeholder white at 50 %, the new-folder field frame #32323280), and the confirming button of Overwrite and Delete is tinted #ffd8d8. Rects are prompt-local; buttons are 100×26 with their bottom 24 above the prompt's bottom; the right button ends 24 from the prompt's right edge and the left button ends 4 to the left of the right button.

| Prompt | Size | Title | Icon (32,y 48×48) | Text (96,56) | Buttons |
|---|---|---|---|---|---|
| Warning | 400×160 | "Error" | delete sign, y 48 | the message, 272×40 | "OK" 276,110 |
| Overwrite | 400×170 | "File already exists" | question sign, y 53 | "Selected file already exists.\n\nDo you want to overwrite?" 272×50 | "Overwrite" 172,120 (tinted #ffd8d8), "Cancel" 276,120 |
| New folder | 400×150 | "Chose a name for the folder to create" | none | field 24,54 352×26, placeholder "Enter the name... Leave blank for \"New Folder\"" | "Create" 172,100, "Cancel" 276,100 |
| Delete | 420×170 | "Delete selection" | warning sign, y 53 | see below, 300×50 | "Delete" 192,120 (tinted #ffd8d8), "Cancel" 296,120 |

- **New folder:** the field starts with "New Folder", or "New Folder (1)", "(2)", ... the first name not present in the current folder (search limited to 500 ms, then the plain name). "Create" validates the name, creates the folder in the current folder and reloads; an existing name gives the warning "Directory already exists." A blank name also gives that warning (the placeholder's promise is not implemented; quirk). The prompt closes after "Create" even when validation failed.
- **Delete:** text "Remove selected shortcut(s)?\n\nNote: this will not remove actual folder(s)" when the first selected item is a location, otherwise "Delete selected file(s)/folder(s)?\n\nWarning: this will permanently delete from disk.". On "Delete": if the first selected item is a location, only the selected locations are removed from Recent / Favourites; otherwise selected locations are removed and the selected files and folders are deleted from disk (no recycle bin; a folder is deleted with all its contents), and deleted paths are also removed from Recent / Favourites.

### 10.10 Locations panel and persistence

- Content: a "Recent:" header row (only when there are recent locations) followed by the recent locations, most recent first (by the stored last-access time; equal times keep stored order); then a "Favorite:" header row (only when there are favourites) followed by the favourites in stored order. The natural name order of 10.4 is not used here, and only stored entries appear (drives are never added automatically). Row label = the last path segment; tooltip = the full path when truncated.
- Recent: at most 5; adding a path that is already listed does nothing (its time is not refreshed); when full, the oldest entry is removed first. A folder is added on every successful Open or Save.
- Favourites: unlimited. Default favourites are added on the first run of the application (a one-time marker in the preference store) and on every launch while Preferences "Re-add default favourites in Cubik file browser on launch" is on: Desktop, Documents, Pictures, the Minecraft folder (Preferences path if it exists, else the detected one), the Staxel content folder, the Stonehearth folder, the application folder, the backup folder; each only if it exists.
- Storage: the location lists are saved on every close and at application exit, and loaded on every open (entries are merged, not replaced), as one text value in the user preference store under the key `CubikFileBrowser` (one-time marker: integer `cubikFilebrowser`, 0 = defaults not yet added). Text format (LF line ends):

```
SavedLocations {
SavedLocationEntry {
<Recent|Favorite>,
<path>,
<True|False>,
<Root|File|Folder|LogicalDrive>,
<date and time>
}
...
}
```

  Recent entries are written before favourites. The third field is the hidden flag; the date uses the invariant culture (`MM/dd/yyyy HH:mm:ss`). Reading removes all LF characters, takes each text between `SavedLocationEntry {` and `}`, splits it on "," and requires exactly 5 non-empty fields (otherwise logs "Not enough arguments" and skips it). Paths containing "," or "}" therefore cannot be stored. cuboid keeps these lists with its own settings (settings spec) and may import this value.
- Not persisted: window size and position, current folder, filter text, selected extension.
- The browser window, rows, lists and prompts are created on first use from their own templates and are not re-themed afterwards, so their colours are those given in this section in every theme (inferred; check the Dark theme).

### 10.11 Path aliases

Typed paths may contain `%minecraft%` (Minecraft folder), `%staxel%`, `%stonehearth%`, `%steam%` (Steam folder from the Windows registry; empty elsewhere), `%backup%` (backup folder), `%app%` (install folder), `%data%` (per-user data folder), `%desktop%`; afterwards OS environment variables (`%NAME%`) are expanded.

### 10.12 Keyboard and mouse buttons

While the browser is open (in addition to the field rules of 10.7):

| Input | Effect |
|---|---|
| Escape | close (cancel); the global Escape rule of 8.5 also fires |
| Mouse button 4 / mouse button 5 ("back" / "forward" buttons) | Back / Forward |
| Alt+Left / Alt+Right (either Alt or AltGr) | Back / Forward |
| Ctrl+A (Cmd+A on macOS), F12 | select all rows of the focused panel |
| Delete | open the delete prompt (when something is selected) |

These keys are read even while a text field or a prompt has focus (quirk: Delete typed in the path field opens the delete prompt when rows are selected; Escape closes the whole browser while a prompt is open). Menu shortcuts are inactive because the global modal flag is set (6.2).

## 11. Unverified / Open questions

1. **Theme colours** are taken from the layout-and-theme spec (12.2). Whether the dynamically created parts (menu rows, file browser, prompts) follow a theme change made in Preferences without a restart is not confirmed.
2. **Menu panel metrics** (1.3) come from the shipped templates; the submenu-arrow glyph and the exact separator colours follow the layout-and-theme spec.
3. **Header menu buttons, linked and associated buttons.** The popup opened by each panel header button (2.9), which toolbar buttons are linked to menu rows, and the eight buttons associated with the Objects menu are inferred from names (5.14).
4. **List-row gestures (3.9).** Right click opening the popups and the texture-row double click are confirmed from code and the row templates' bindings. Not observed at runtime: the engine-level drag rules (a drag of 5 px or more cancels the click; right or middle drags only cancel it).
5. **File browser template.** Geometry and colours come from the shipped templates (window, rows, prompts, resize handles), cross-checked with an older inactive copy in the main scene that differs only in the prompts' look (light there, dark in the shipped template). Open points: prompt placement (centred on the window, inferred), the loading indicator's picture, and whether the browser is re-themed in non-Light themes.
6. **Initial Edit row states.** Whether Cut / Copy / Delete start enabled or disabled before the first selection event.
7. **Escape and the global modal flag (8.5).** From code, Escape re-runs the last message box's X handlers whenever the flag is set (modal dialogs, About box, file browser) and clears the flag while the dialog stays open; Escape over a non-modal dialog also closes it together with a message box. Confirm at runtime and decide whether cuboid reproduces this.
8. **Shortcuts after dialogs.** Whether keyboard selection left on a dialog control keeps blocking non-works-in-UI shortcuts after the dialog closes (6.7).
9. **Model Properties defects.** (a) The Parent field is rewritten on the first opening in a session and whenever showing changes the type radio, wiping custom parents when "Automatic parent..." is off; (b) OK clears an already-set "Restrict rotations" flag. Both are derived from code order; confirm at runtime. cuboid fixes both.
10. **Changes the unsaved-changes prompt does not cover (5.1).** Settled from code: Model Properties OK (except switching "Restrict rotations" on), Grid size, element settings panel edits, keyboard nudges, model-box handle drags, the Display Settings copy buttons and texture loads by double click never mark the document modified, although most of them are saved with the document. Decision for cuboid: keep this for parity, or mark every saved change as modified (recommended deliberate fix).
11. **Help > Changelog URL** is supplied at run time by the licence / update service and is unknown.
12. **Custom key bindings at start-up.** The original applies the stored Preferences key strings only after Preferences OK, not at launch; cuboid applies them at launch (6.6).
13. **Button sizes in the 420-wide dialogs.** Computed from the layout rules as ≈100.1 units wide (7.1); confirm against a screenshot.
14. **Unselected tab geometry** in Model Properties (21 high, 1 unit above the frame) is computed from code and pivots; Preferences tabs keep 25. Verify.
15. **Dimming layer input blocking** (8.6) depends on a component setting not present in the data.
16. **Return in a message box** probably presses the selected button through standard UI navigation; not confirmed.
17. **Browse last save/export Folder** behaviour before anything was saved or exported in the session.
18. **Progress dialog.** No abort handler is registered, so Abort only hides the dialog, and every progress report below 50 % re-shows and re-centres it.
19. **File browser details.** Activation of file-type locations; exact refresh points of the Open button state; whether Return in the new-folder field also closes the prompt.
20. **Rounding of the Minecraft entity resolution** for values exactly between two powers of two (9.6).
21. **Message-box auto-size** uses an estimated character width (8.2), so the computed box can be too small or too large for the real text; cuboid should keep the same estimate for parity of box sizes.
22. **Native dialogs.** Platforms on which the native open / save dialogs exist (the built-in browser is the fallback) and how their filters look.
23. **Settings defaults when a key is missing** from an existing settings file differ from first-run defaults for a few options (9.1); the settings spec owns the final rule.
24. **Classic tooltips** (Preferences > Advanced) are drawn by the shell; their look is outside this document.
25. **Minor inferences** marked "(inferred)" in the text: the menu-bar engaged flag reset (1.2), the texture label list feeding the Change Faces Textures and Faces-tab fields (3.5), element clone re-selection (5.5), new primitives becoming the selection (5.14), the Preferences tab staying selected between shows (9.1).
26. **File-browser name collation (10.4).** The digit-run, leading-zero and kind rules are certain. How the original runtime (Mono) collates text is UNVERIFIED: the Windows behaviour described (hyphens and apostrophes ignored at the base level, accents deferred, the resulting cycles) was observed only with the Windows .NET collation. Also unverified: the OS listing order that decides ties, and which distinct names the collation treats as equal beyond case.
27. **Undo of 'Restrict elements rotation' (9.6).** That undoing it leaves the flag on, and with N = 0 also reverts the other fields of the same OK, follows from code order; confirm at runtime.
