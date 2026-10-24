# Cuboid UI Specification: Layout, Theme and Widgets

Status: draft 1, 2026-10-18. Audience: implementers and parity testers of the cuboid UI.

This document defines the main window of cuboid so that it reproduces the
original Cubik Studio 2.9 window pixel for pixel at UI scale 1: canvas and scale
rules, the geometry of every region and panel, the docking and resizing of the
side zones, the skin (theme) system with its exact colour tables, the 9-slice
frames, fonts, the icon inventory, and the look and behaviour of every widget
kind. Menu contents, dialog contents, the file browser, the 3D camera behaviour
and the edit-mode logic have their own specs; this document only covers the
parts of them that shape the window (bars, frames, chrome, shared widgets).

Evidence tags used throughout:

- **[V]** verified against the original program logic and its shipped data.
- **[D]** derived by applying the layout rules of section 0.4 to verified data.
- **[U]** unverified; see section 18.

---

## 0. Conventions

### 0.1 Units and coordinates

- `px` means one logical UI pixel ("canvas pixel"). At UI scale `s` (section 1.2)
  one canvas pixel covers `s` physical pixels. Every size in this document is in
  canvas pixels unless stated otherwise.
- Coordinates: origin at the top-left of the window client area (or of the
  named parent), x to the right, y downward.
- `[x0, x1) x [y0, y1)` is a half-open rectangle; `(x, y, w, h)` is the same
  rectangle as position plus size; "at (dx, dy) in P" means an offset from the
  top-left of P.
- `W`, `H`: window client width and height in canvas pixels.
- Colours are `#RRGGBB` or `#RRGGBBAA` (alpha last, `ff` opaque).

### 0.2 Fractional geometry

The original keeps positions and sizes as floating-point numbers (for example
a 239.22 px wide list button or a status cell 220.74 px wide) and does not snap
the UI to the pixel grid. The port must keep floats in layout. When drawing,
snap the outer edges of frames to whole physical pixels with the same rounding
rule everywhere (round half away from zero is recommended). Exact
sub-pixel rasterization of the original is [U].

### 0.3 Draw order

Siblings are drawn in list order; later siblings are drawn on top and receive
the pointer first. Nothing in the main window clips its children except the
scroll views (lists, texture view). A panel that overflows its region is simply
drawn past it and is then covered by whatever is drawn later (status bar, menu
bar, dialogs).

### 0.4 Layout rules (normative)

Most sizes in this spec are fixed numbers. A few regions are computed with the
following rules, which reproduce the original exactly; implement them once and
reuse them.

**Anchored rectangles.** An element is placed inside its parent by a pair of
anchor points (fractions of the parent size), an offset and a size. When both
anchors of an axis coincide, the element has a fixed size on that axis; when
they differ, the element stretches with the parent and the "size" is a margin
(negative = inset). Every stretching region of the main window is listed with
its insets in section 2, so the port does not need a general anchor engine.

**Stack (row or column).** Children are laid out in order along the main axis;
inactive children are skipped. Each child reports `min`, `preferred` (never
below `min`) and `flexible` (a weight, 0 = rigid) along that axis. If the stack
has "expand children" enabled, each child's flexible weight is raised to at
least 1. Let `A` = available length minus padding minus spacing x (n - 1).

1. If `A >= sum(preferred)`: `size_i = pref_i + (A - sum(pref)) * flex_i / sum(flex)`.
   If `sum(flex) = 0`, every child gets its preferred size and the block is
   aligned inside the stack by the stack's alignment.
2. Else if `A >= sum(min)`: `size_i = min_i + (pref_i - min_i) * t` with
   `t = (A - sum(min)) / (sum(pref) - sum(min))`.
3. Else `size_i = min_i`; the stack overflows its far edge (not clipped).

On the cross axis a controlled child gets `clamp(available, min, flex > 0 ?
available : preferred)` and is placed by the stack's alignment. A stack's own
`min`/`preferred` along its main axis is padding + spacing + the sum of the
children's values; across, it is padding + the largest child value.

**Grid.** Fixed cell size `(cw, ch)`, spacing `(sx, sy)`, filled row by row from
the top-left. Columns = `max(1, floor((inner_width + sx + 0.001) / (cw + sx)))`.
The grid's min and preferred height is `padding_v + rows * (ch + sy) - sy`
(an empty grid therefore reports `padding_v - sy`, which may be negative).

**Default contributions.** Unless an explicit layout hint overrides it, a
9-sliced background contributes a preferred size equal to the sum of its two
borders on that axis (min 0, flexible 0); a plain picture contributes its pixel
size; a text label contributes its natural single-line width and height (Open
Sans 11 px: height 15 px, see section 14). Explicit hints (min, preferred,
flexible) given in the tables below take precedence.

**9-slice squash.** When a 9-sliced rectangle is smaller than the sum of its two
borders on an axis, both borders on that axis are scaled by
`rect_size / (border_a + border_b)` before drawing. This matters for the status
bar cells (section 4).

### 0.5 Colour state model (normative)

Every interactive widget has a background graphic (its "target") with a base
colour, plus a state colour. The displayed colour of each texel is
`texel x base_colour x state_colour x multiplier` (component-wise, including
alpha; results are clamped to 1 when drawn). The state colour comes from a
4-entry table (normal, hover, pressed, disabled) and changes with a linear
cross-fade (0.1 s unless stated; some widgets switch instantly). State priority:
disabled > pressed > hover > normal, where "hover" also covers "has keyboard
focus" for widgets that can take focus (section 16.1).

---

## 1. Canvas, scale and window

### 1.1 Canvas [V]

- One overlay canvas covers the whole client area. There is no reference
  resolution scaling: 1 canvas px = 1 physical px at scale 1, and nothing
  scales with the window size. The canvas origin follows the window.
- All 9-slice borders in this spec are in canvas px (the source art has
  100 pixels per unit, which maps 1:1 to the canvas).

### 1.2 UI scale [V]

- `scale = floor(max(96, dpi) / 96 * 2) / 2` (steps of 0.5), then clamped to
  `[1.0, 1.99]` for the canvas. Examples: 96 dpi -> 1.0, 120 dpi -> 1.0,
  144 dpi -> 1.5, 168 dpi -> 1.5, 192 dpi -> 1.99 (not 2.0).
- The scale is computed once at start-up from the display dpi. There is no user
  setting and it is not persisted.
- Users of the scale: the whole canvas, the minimum window size (section 1.3),
  the 3D viewport rectangles, the tooltip gap (section 10.1), the popup
  edge-flip tests (section 3.4), the dialog centring (section 11) and the
  tool-cursor offset (section 10.2).
- Port note: implement this formula for parity and also offer an explicit
  scale override. Whether the original reports the real dpi on every Windows
  configuration is [U].
- On macOS the original also re-derives the scale when entering full screen;
  on Windows it does not. The port need not reproduce the macOS path.

### 1.3 Window [V]

- Default client size on first launch: 1000 x 700 (physical px).
- Minimum client size: 900 x 700 multiplied by the UI scale; on Windows the
  original actively resizes the window back to the minimum when it is made
  smaller.
- The original stores the last windowed size when the window becomes as large
  as the monitor (keys `ScreenWidth`, `ScreenHeight`); restoring it on start-up
  is [U] and belongs to the settings spec.
- While the window has focus the UI is refreshed at 60 Hz, when unfocused at
  10 Hz. Time-based UI effects (fades, the 10 s status reset) are defined in
  seconds and must not depend on the refresh rate. The port should redraw on
  demand instead.

---

## 2. Main window geometry

### 2.1 Overview

```
x=0                                                                          x=W
+-----------------------------------------------------------------------------+ y=0
| File Edit Objects Recover Views Tools Window Help              menu bar 22  |
+------------+----+-----------------------------------------+------+----------+ y=22
| helpers    |    |  +-----------------+-----------------+  |      | managers |
| group      |left|  | Up    (top)     | West  (side)    |  |right | group    |
| (optional, |tool|  |                 |                 |  |tool  |(optional,|
|  width L)  |strip  +-----------------+-----------------+  |strip | width R) |
|            | 64 |  | South (front)   | Main  (persp.)  |  | 112  |          |
|            |    |  |                 |                 |  |      |          |
|            |    |  +-----------------+-----------------+  |      |          |
+------------+----+-----------------------------------------+------+----------+ y=H-24
| Info (flexible)            | Details 220 |Face| X  | Y  | Z  |Rotation       |
+-----------------------------------------------------------------------------+ y=H
```

Top-level regions, back to front: app area, status bar, menu bar, dialogs,
popup menus, file browser, overlay (tooltip and tool cursor). [V]

| Region | Rectangle | Notes |
|---|---|---|
| Menu bar | `[0, W) x [0, 22)` | section 3 |
| App area | `[0, W) x [22, H-24)` | height `H - 46` |
| Status bar | `[0, W) x [H-24, H)` | section 4 |
| Left side zone | `[0, L) x [22, H-24)` | absent when `L = 0` |
| Main zone | `[L, W-R) x [22, H-24)` | |
| Left tool strip | `[L, L+64) x [22, H-24)` | section 6 |
| 4-view zone | `[L+64, W-R-112) x [22, H-24)` | section 5 |
| Right tool strip | `[W-R-112, W-R) x [22, H-24)` | section 7 |
| Right side zone | `[W-R, W) x [22, H-24)` | absent when `R = 0` |

`L` and `R` are the widths of the side zones that are currently visible
(section 2.2). [V]

Worked examples [D] (window px, scale 1; quadrant = one view in 4-up mode):

| Window | Side zones | 4-view zone x-range | Zone size | Quadrant | Right strip x |
|---|---|---|---|---|---|
| 900x700 | none | [64, 788) | 724x654 | 362x327 | [788, 900) |
| 900x700 | R=220 | [64, 568) | 504x654 | 252x327 | [568, 680) |
| 900x700 | L=350, R=220 | [414, 568) | 154x654 | 77x327 | [568, 680) |
| 1000x700 | none | [64, 888) | 824x654 | 412x327 | [888, 1000) |
| 1000x700 | R=220 | [64, 668) | 604x654 | 302x327 | [668, 780) |
| 1280x720 | none | [64, 1168) | 1104x674 | 552x337 | [1168, 1280) |
| 1280x720 | R=220 | [64, 948) | 884x674 | 442x337 | [948, 1060) |
| 1280x720 | L=350, R=220 | [414, 948) | 534x674 | 267x337 | [948, 1060) |
| 1920x1080 | L=350, R=220 | [414, 1588) | 1174x1034 | 587x517 | [1588, 1700) |

The original does not prevent the 4-view zone from becoming narrower than
zero (for example 900 px wide with a 640 px helpers zone and a 300 px managers
zone). The port must clamp the zone width at 0 and keep the strips visible. [U]

### 2.2 Side zones and panel groups [V]

Panels on the sides belong to one of two groups:

- **Managers group**: Element tools, Voxel tools, Mesh tools, Elements list,
  Elements material, Meshes list, Paint tools, Color tools, Palette, Gizmos
  (stacked in this order, section 8).
- **Helpers group**: Element Settings, Display Settings (section 9).

The create panels (left tool strip) and the right tool strip panels are not in
either group and never affect the side zones.

Docking state:

| State | Default | Range | Meaning |
|---|---|---|---|
| `managers_on_left` | false | bool | managers group docked in the left zone, helpers in the right zone |
| `managers_width` | 220 | 180 .. 300 | width of the zone holding the managers group |
| `helpers_width` | 350 | 350 .. 640 | width of the zone holding the helpers group |

Each group is visible while at least one of its panels is shown. The zone that
hosts a visible group is shown with that group's width; a zone whose group is
empty is hidden (width 0). So with the default docking, `R = managers_width`
when any manager panel is shown, and `L = helpers_width` when a helper is shown.

Start-up: the widths are read from the saved preferences (section 2.7), the
managers group is docked right, both zones start hidden, and then the panels
of the start-up panel set (section 2.6) are shown.

Each zone stacks its group with the stack rules of section 0.4 (no padding,
no spacing, children expanded on both axes, so the panels always fill the zone
height; see sections 8.1 and 9 for the panel sizes).

### 2.3 Side zone grips [V]

Each side zone has a grip on its inner edge:

- Size 6 x 96, centred on the inner edge of the zone (it straddles the edge by
  3 px on each side) and vertically centred in the app area.
- Look: sliced skin `panel` frame tinted with the skin button state colours
  (`buttonNormal`, `buttonHover`, `buttonPressed`, `buttonDisabled`), fade 0.1 s.
- Hover shows the horizontal-resize cursor (hot spot at the centre of the
  cursor image); leaving restores the default cursor unless a drag is running.
- Drag: `new_width = start_width + dx` for the left zone's grip and
  `start_width - dx` for the right zone's grip, where `dx` is the pointer
  movement since the press; the result is clamped to the range of the group
  that the zone currently holds (managers 180 .. 300, helpers 350 .. 640). The
  width is applied live on every drag step: the zone, the main zone and the 3D
  viewports re-layout immediately, and the value is written to the preferences
  on every change (as an integer, truncated).
- Release ends the drag and restores the default cursor.
- The grip is hidden together with its zone.

Port note: write the preference on release instead of on every step, and
coalesce re-layout to once per frame; the visible result is identical.

### 2.4 Window menu layout commands [V]

| Command | Effect |
|---|---|
| Move Managers on Left (Ctrl+1 on keypad) | managers group into the left zone, helpers into the right zone |
| Move Managers on Right (Ctrl+3 on keypad) | managers group into the right zone, helpers into the left zone (default) |
| Close Managers (Ctrl+2 on keypad) | hide all ten manager panels |
| Close Helpers (Ctrl+8 on keypad) | hide both helper panels |
| Close All (Ctrl+0 on keypad) | both of the above |

Moving a group keeps the group's own width; the grip ranges follow the group.

### 2.5 Showing and hiding panels [V]

- Showing a panel adds it to its group (once), re-evaluates the zones and makes
  the panel visible. Hiding removes it from its group; when the group becomes
  empty its zone disappears and the main zone widens.
- A panel's requested visibility is authoritative: a panel that was asked to be
  hidden stays hidden even if its parent becomes visible again.
- Panels keep their order of section 8 / 9 regardless of the order in which
  they were shown.
- If both helpers were ever visible at the same time they would share the zone
  height equally; the original never does this on purpose. [U]

### 2.6 Panel sets [V]

Entering an edit mode hides every manager panel, both helpers and all three
create panels, then shows the set below. The edit mode buttons are in the right
tool strip (section 7.1); turning the active mode button off returns to "no
mode".

| Edit mode | Left tool strip | Managers group | Helpers group |
|---|---|---|---|
| no mode (start-up) | Element panel if preference `openElementCreate` (default on), Primitive panel if `openMeshCreate`, Model panel (always) | each manager panel whose preference `open...` flag is on (all off by default) | none |
| Element | Element, Model | Element tools, Elements list, Elements material | none until requested |
| Voxel | Model | Voxel tools, Paint tools, Color tools, Palette, Gizmos | none |
| Mesh | Primitive, Model | Mesh tools, Meshes list, Paint tools, Color tools | none |
| Paint | Model | Paint tools, Color tools, Palette | none |
| Preview | Model | none | Display Settings |

The "no mode" preference flags are `openElementCreate`, `openElementTools`,
`openElementsList`, `openElementsMaterial`, `openVoxelTools`, `openGizmosTools`,
`openMeshCreate`, `openMeshTools`, `openMeshesList`, `openPaintTools`,
`openColorTools`, `openPalette`; they are edited in Preferences only.

So the default first-launch window shows no side zones: the left strip holds
the Element and Model create panels and the 4-view zone spans
`[64, W-112)`.

Element Settings appears when one of the Element tools toggles "settings",
"faces" or "UV" (or the matching Objects menu command) asks for it; it then
opens on the matching tab and hides Display Settings. The Tools menu items
F5 .. F12 toggle single manager panels on and off.

When Display Settings is shown and the preference `maximize3dViewOnPreview`
(default on) is set, the Main view is maximized (section 5.2) unless it already
was; hiding Display Settings returns to 4-up if the maximize was caused by
showing it. [V]

### 2.7 Persistence of the layout [V]

| Key | Type | Default | Notes |
|---|---|---|---|
| `ManagersWidth` (read) / `managersWidth` (written) | int | 220 | the original reads and writes keys that differ only in letter case |
| `HelpersWidth` (read) / `helpersWidth` (written) | int | 350 | same |

Because of the case difference the widths saved by the original are very
likely never read back, so the original effectively starts with 220 / 350
every time. [U] The port persists `managers_width`, `helpers_width` and
`managers_on_left` in its own settings file and, when importing Cubik
preferences, accepts either spelling. Panel visibility, the UI scale and the
docking side are not persisted by the original.

---

## 3. Menu bar

### 3.1 Bar [V]

- Rect `[0, W) x [0, 22)`. Background: the skin's `label` sprite stretched
  (plain, not sliced) at full opacity: white in Light, `#2b2b2b` in Dark.
- Buttons are placed at fixed offsets from x = 1 (the bar content is inset 1 px
  left and right); all are 22 px tall:

| Menu | x | Width | Label |
|---|---|---|---|
| File | 1 | 48 | `File` |
| Edit | 49 | 48 | `Edit` |
| Objects | 97 | 64 | `Objects` (an alternative menu labelled `Selection` uses the same slot; it is hidden in 2.9) |
| Recover | 161 | 64 | `Recover` |
| Views | 225 | 48 | `Views` |
| Tools | 273 | 48 | `Tools` |
| Window | 321 | 64 | `Window` |
| Help | 385 | 48 | `Help` |

- Button look: sliced `button` frame; state colours transparent white (rest,
  invisible), skin `buttonHover`, `buttonPressed`, `buttonDisabled`; fade 0.1 s.
- Label: 11 px, centred horizontally, top-aligned in the box
  `(0, 4, w, 18)` of the button, skin text colour.
- The Objects and Tools menus start disabled (when they enable: menus spec).
  Disabling a menu gives all of its items the disabled look of section 3.2 and
  also disables the toolbar buttons linked to it (they show the disabled tint
  and a greyscale icon, section 16.2). Whether the bar button itself changes is
  [U]; the port keeps the bar button unchanged.
- The bar reports "pointer inside" while the pointer is over it (used by the
  menu hover logic).

### 3.2 Dropdown panel [V]

```
+--------------------------------------------+   panel: sliced `panel` frame
|  +--------------------------------------+  |   padding 4 on all sides
|  |(i) Label text              Shortcut >|  |   row: 20 px, spacing 2
|  +--------------------------------------+  |
|  ======================================== |   separator: 2 px
|  +--------------------------------------+  |
|  |(i) Label text              Shortcut  |  |
|  +--------------------------------------+  |
+--------------------------------------------+
```

- Panel width = the menu's minimum width + 8; height = 8 + sum of row heights +
  2 per gap. Rows fill the inner width.
- Minimum widths: File 196, Edit 280, Objects 256, Selection 256, Recover 280,
  Views 296, Tools 256, Window 380, Help 200, recent-files submenu 196;
  context popups and submenus: see the menus spec (values 96 .. 300).
- Item row (20 px): sliced `button` frame, transparent at rest, `buttonHover`
  on hover, `buttonPressed` while pressed, instant change (no fade).
  - Icon: 16 x 16, centred at x = 12, vertically centred; omitted when the item
    has none.
  - Label: box from x = 32 to the right edge, top 2 px; 11 px, left-aligned,
    skin text colour.
  - Shortcut: box from x = 24 to `w - 24`, top 4 px; 11 px, right-aligned (its
    right edge is 24 px from the row's right edge), skin text colour. On macOS
    `Ctrl` is displayed as the command glyph and `Alt` as the option glyph.
  - Submenu arrow (only for items with a submenu): `symb_arrow` glyph 9 x 9,
    centred at x = `w - 12`, vertically centred, skin text colour.
- Separator row: the `separator` sprite (24 x 2) stretched to the inner width,
  2 px tall (a 1 px line in the skin's separator colour over a 1 px highlight).
- Disabled item: no background at all, label and shortcut `#808080`, icon drawn
  in greyscale; it does not react to the pointer. Items flagged "always active"
  never take this look.
- Panel position: a main menu opens with its top-left at the bottom-left of its
  bar button (y = 22). A submenu opens with its top-left 4 px above the
  top-right corner of its row, so its first row lines up with the parent row.
  A context popup opens with its top-left at the pointer; it shifts left by the
  amount it would overflow the window's right edge and opens upward (bottom at
  the pointer) when it would overflow the bottom edge.
- A dropdown list (section 16.8) opens its panel directly below the list
  button, with the button's width; it opens upward when there is no room below.
- Open and close requests take effect on the next frame. While a main menu is
  open, moving onto another bar button switches to that menu. Details: menus
  spec.

### 3.3 Menu contents

Menu items, shortcuts and commands are defined in the menus spec.

### 3.4 Popup flip tests [V]

The overflow tests of context popups use the panel's real size (or an estimate
of 15 px per row + 8 when it has never been laid out) multiplied by the UI
scale. The port uses the real size.

---

## 4. Status bar [V]

- Rect `[0, W) x [H-24, H)`. A row of seven cells, no padding or spacing,
  children expanded on both axes.

| Cell | Preferred width | Flexible | Content |
|---|---|---|---|
| Info | 4 | 500 | status message; `Loading...` before start-up, `Ready` when idle |
| Details | 220 | 1 | label and id of the focus object, or the model summary (4.1) |
| Face | 60 | 1 | face of the element under the pointer (4.1) |
| X | 60 | 1 | `X: ` + position x of the focus object (4.1) |
| Y | 60 | 1 | `Y: ` + position y |
| Z | 60 | 1 | `Z: ` + position z |
| Rotation | 60 | 1 | rotation axis and angle of the focus object (4.1) |

- Widths follow rule 1 of the stack algorithm: `extra = W - 524`, Info =
  `4 + extra * 500/506`, the others `pref + extra/506`. At W = 900: Info
  375.5, Details 220.74, others 60.74 each. At W = 1280: Info 751.0, Details
  221.49, others 61.49. [D]
- Each cell: sliced `menu` frame (the panel-header sprite). Because the cell is
  only 24 px tall, the 24 px top border and 2 px bottom border are squashed by
  24/26 (section 0.4), so each cell shows the header gradient over almost its
  full height with the 1 px dark top line.
- Each cell's text: inset 8 px left and right, full height, 11 px, vertically
  centred, left-aligned, skin text colour. Labels follow the text rule of
  section 14: an over-long message wraps at a word boundary and the lines that
  do not fit the 24 px cell are dropped, so only the words that fit on the
  first line are shown.
- Info behaviour: any status message (including the tooltip text of every
  hovered control) replaces Info immediately; 10 s after the last message
  (counted in whole seconds, restarting on each new message) Info shows
  `Ready`.
- Until their first refresh (4.2) the six right-hand cells show `...`.

### 4.1 Cell contents [V]

Details, X, Y, Z and Rotation describe one *focus object* (4.2) or, when there
is none, the whole model. Face describes only the object under the pointer.

**Details**

- Focus object: its label, followed by ` #<id>` only when its display id is
  above 0 (the per-kind creation counter of model spec 2.1). Example:
  `Box3 #3`. The blueprint shows `Blueprint` and the model origin
  `Model Origin`, both without an id (they never get one).
- No focus object: `<N> element` (`elements` when N > 1), then ` (<M> face)`
  (` (<M> faces)` when M > 1). N counts every element, hidden ones included; M
  is the number of enabled faces over all elements. Examples:
  `0 element (0 face)`, `1 element (1 face)`, `3 elements (14 faces)`.

**X, Y, Z**

- `X: `, `Y: `, `Z: ` followed by the focus object's position relative to its
  parent in UI space `(x, y, -z)` (model spec 1.2), in model units (16 per
  block, no division: `X: 32` is two blocks). For an unrotated element this is
  the export `from` corner (model spec 1.3). The pivot (origin) is never shown.
- Numbers use the default float text of model spec 1.6 (invariant culture:
  `.` as decimal separator, no grouping, up to 7 significant digits, trailing
  zeros dropped). Exponent notation for very large or very small values and a
  possible `-0` are [U].
- Elements and meshes are children of the model's element node, which sits at
  the origin with unit scale, as do its parents; the values are therefore model
  coordinates provided those nodes are unrotated (their rotation is not in the
  scene data; expected zero) [U].
- No focus object: the three cells are empty.

**Rotation**

- Read from the stored Euler triple `e` (model spec 1.3), not from the drawn
  transform. Axis letter: the first non-zero component in the order x, y, z,
  written `x`, `y` or `z`; when all three are 0 the cell shows `-` alone.
- Angle: wrap the component (`a - 360` when `a > 180`), negate it for x and y
  (not for z), round to 0.1 with half-to-even. Text: the letter, then `: `, the
  angle and `°` (U+00B0); this suffix is added only when the rounded angle is
  not 0, otherwise the letter stands alone.
- Examples: stored x 270 gives `x: 90°`; stored y 337.5 gives `y: 22.5°`;
  stored z 45 gives `z: 45°`; stored x 22.5 with z 45 gives `x: -22.5°` (the
  z part is not shown).
- Stored angles are multiples of 22.5 when "Restrict rotations" is on, else
  multiples of 5 (model spec 3.4). With "Restrict rotations" on, whole quarter
  turns of an element are moved into its faces when a rotation drag ends
  (model spec 3.5), so a 90° turn shows the raw angle during the drag and `-`
  after release (timing [U]).
- No focus object: empty.

**Face**

- Only for the hover target (4.2), never for the selection, and only when that
  object is an element. Empty for meshes, the blueprint, the model origin and
  when nothing is hovered (so a selected element that is not hovered shows an
  empty Face cell).
- Rule: take the surface normal (world space) recorded at the last hover hit,
  rotate it by the inverse of the element's current world rotation (no
  position, no scale) and compare it with the six unit axes using approximate
  equality (squared distance below 1e-10): -Y `Down`, +Y `Up`, +Z `North`,
  -Z `South`, +X `East`, -X `West`, anything else `-`. These are internal axes
  without the Z flip, which agree with the face table of model spec 1.2, so
  the cell names the face that was hit.
- The recorded normal is renewed only when the pointer moves (4.2). Rotating
  the hovered element under a still pointer therefore re-reads the old normal
  in the new orientation, and the name can change or become `-` until the
  pointer moves [U]. Element hit normals are axis-aligned, so `-` should be
  rare [U].

**Fit.** Only about 44 px of a 60 px cell hold text. By the overflow rule of
section 14 a value too long for one line wraps after the `X:` (or `x:`) word
and the second line is dropped, so a long number such as `X: -12.5625` shows
as `X:` alone [D]; the exact limit depends on the glyph advances [U].

### 4.2 When the cells change [V]

**Hover target (drives Face).** Face is rewritten from the hover target every
physics step (50 Hz). The hover target itself is updated in the same step only
when the window has focus, the pointer is inside a 3D view (the flag of
section 5.3: set on entering a quadrant, cleared on leaving it or entering a
dialog) and no mouse button (left, right, middle) and no pan key is held; so
a drag freezes hover and Face, and while the pointer is outside the views the
target keeps its last value (it is not cleared on leaving a view) [U].
The ray through the pointer is cast again only when the pointer has moved
since the last cast; otherwise the previous hit is reused, so scene changes
under a still pointer go unnoticed. Only objects on the default physics layer
are hit (hidden objects are on another layer; mapping [U]). A miss clears the
hover target. A hit sets it to the object hit, except while a voxel tool is
active or the selection mode is custom: then a hit leaves the target unchanged
(a miss still clears it).

**Focus object (drives Details, X, Y, Z, Rotation).** These cells are not
polled; they are rewritten when one of these events happens, each naming the
object to show (none = model summary):

- hover starts on an object: that object;
- hover ends: the primary selection, or the summary when there is none (when
  hover moves straight from one object to another, the new object wins);
- the primary selection changes: the new primary, or the summary when it is
  cleared. Click-select sets the clicked object; removing the primary from the
  selection moves it to the first remaining selected object or clears it; box
  selection makes the first object of its list primary; the main element
  creation path selects the new element when its caller asks for it (which
  callers do: [U]);
- a selected object's position or rotation changes, including every frame of
  a drag and while nudge keys are held: that object. Scale changes do not
  refresh the cells;
- start-up: the summary.

In the usual case the focus object is the hovered object, else the primary
selection, else none; but the last event wins (for example nudging the
selection while another object is hovered shows the selection).

**Paths that leave the cells stale** until a later event (read from the
program logic; runtime [U]): renaming an element in Element Settings (Details
keeps the old label), typing a position in Element Settings (X, Y, Z lag),
dragging the model origin (its moves raise no refresh), the helper that
re-selects a list of objects (used for example after Clone; it changes the
primary without the change event, so the previous focus or summary stays),
name-search selection (keeps an existing primary even when that object is no
longer selected), and the summary counts (computed only when the summary is
shown, so after File > New or Open with nothing selected, or after elements
are added or removed without a primary change, the counts stay stale until
the next hover end or primary change).

### 4.3 Per object kind and multi-selection [V]

| Focus | Details | Face | X / Y / Z | Rotation |
|---|---|---|---|---|
| Element | label, plus ` #id` if id > 0 | hovered face or `-`; empty when not hovered | UI position | axis and angle, or `-` |
| Mesh | label, plus ` #id` if id > 0 | empty | UI position | axis and angle, or `-` |
| Blueprint | `Blueprint`, no id | empty | UI position of the object itself (its separate pivot is not used) | axis and angle, or `-` |
| Model origin | `Model Origin`, no id | empty | the marker's position (equals the model origin property) | normally `-` |
| none | element and face summary | empty | empty | empty |

- Mesh ids come from the mesh counter; some mesh creation and restore paths
  reuse the current counter value without advancing it, so ids can repeat
  (reachability [U]).
- The model origin has no rotation gizmo, but a rotated origin (through the
  keyboard rotation path) cannot be ruled out [U].
- Multi-selection: only one object is shown, never a count of selected
  objects. Hover overrides the primary while the pointer is over an object.
  When the selection moves or rotates, the primary is the last object to
  refresh the cells, so they end on the primary; another selected object is
  shown only once it becomes primary.

---

## 5. 4-view zone [V]

### 5.1 Quadrants

- The 4-view zone is split into four equal quadrants (no gutter): Up = top-left
  (orthographic top view), West = top-right (orthographic side view), South =
  bottom-left (orthographic front view), Main = bottom-right (perspective
  view). Halves are fractional when the zone size is odd.
- Each 3D view is cleared to `#343434` and drawn into its quadrant.
- Quadrant frame: the `bg_border` sprite (1 px opaque line at the outer edge,
  transparent inside) tinted `#00000080`, drawn hollow. The frame is drawn at
  half scale, so the line is 0.5 px wide at 50 % black; on screen this shows as
  a faint dark hairline on the quadrant edges. Port: draw a 1 px line in black
  at 25 % opacity on each quadrant edge. [U] (exact rasterization)

### 5.2 View labels

- One label per quadrant: `Up`, `West`, `South`, `Main`; 11 px, bold
  (synthesized), white, top-left aligned in a 34 x 16 box.
- Position: (8, 4) from the quadrant's top-left in 4-up mode; (16, 8) from the
  zone's top-left when that view is maximized.
- Labels are not affected by the skin. Their state colours (applied to the
  white text): rest `#ffffff50`, hover `#ffffffff`, pressed `#ffffffa0`,
  disabled `#d2d2d250`, fade 0.1 s.
- Left click toggles "maximize" for that view (any of the four, including
  Main): the maximized view fills the whole 4-view zone and the other three
  views and frames are hidden; clicking again returns to 4-up. Maximizing one
  view un-maximizes the others.
- Right click cycles the view's camera preset (Up/Down, West/East,
  South/North; Main has a single preset) and the label text shows the preset
  name. Details: views spec.

### 5.3 Hover

Entering a quadrant makes its view the active 3D view (for zoom, pan,
selection); leaving any quadrant, or entering a dialog, clears it. Details:
views spec.

### 5.4 Cinema frame (Main view only)

When a display-settings preset requests a 16:9 frame, the Main view is
letterboxed (or pillarboxed) to 16:9, centred, and the remaining area of its
quadrant is covered with two opaque black bars (`#000000`), one anchored to the
top-left and one to the bottom-right of the quadrant. Details: views spec.

---

## 6. Left tool strip [V]

### 6.1 Strip

- Rect `[L, L+64) x [22, H-24)`. Background: sliced `panel` frame over the full
  strip height.
- The visible create panels are stacked from the top in this fixed order:
  Element, Primitive, Model. A hidden spacer with min height 1000 follows them;
  its only purpose is to keep the panels at their natural height (stack rule 3
  in practice). Panel heights: Element 212, Primitive 442, Model 180.
- Stacked y-ranges (window y, scale 1) [D]:

| Visible panels | Element | Primitive | Model |
|---|---|---|---|
| Element + Model (Element mode, default) | [22, 234) | - | [234, 414) |
| Primitive + Model (Mesh mode) | - | [22, 464) | [464, 644) |
| Model only | - | - | [22, 202) |
| all three (no-mode preference) | [22, 234) | [234, 676) | [676, 856) |

- Panels below `H - 24` are hidden behind the status bar; nothing is clipped
  or scrolled. With all three panels at H = 700 the Model panel is fully hidden.
- Very tall windows: when `H - 46` exceeds the total including the 1000 px
  spacer (for example Element + Model: H > 1438), the surplus is shared
  equally by every stacked item including the spacer; each panel grows and,
  inside it, the title and the button area each take half of the growth, so
  the buttons move down. Port: reproduce for parity. [D]

### 6.2 Create panel chrome

- Background: sliced `menu` frame (header gradient in the top 24 px).
- Padding: left 7, right 7, top 6, bottom 8 (Model: bottom 6).
- Title: at (7, 6), 50 x 16, 11 px, left/top aligned.
- Buttons: one column, x = 7, width 48.

### 6.3 Element panel (title `Element`, 4 buttons)

Buttons 48 x 44 at y = 22 + 46k (k = 0..3); icon 38 x 38 centred.

| k | Icon (tint) | Tooltip | Command |
|---|---|---|---|
| 0 | `primitive_cube` | `Add Cube` | add cube element |
| 1 | `element_planey` (`#b4b4ff`) | `Add Plane on Y axis` | add Y plane |
| 2 | `element_planex` (`#ffa0a0`) | `Add Plane on X axis` | add X plane |
| 3 | `element_planez` (`#96ff96`) | `Add Plane on Z axis` | add Z plane |

### 6.4 Primitive panel (title `Primitive`, 9 buttons)

Buttons 48 x 44 at y = 22 + 46k (k = 0..8); icon 38 x 38 centred, untinted.

| k | Icon | Tooltip |
|---|---|---|
| 0 | `primitive_cube` | `Add Cube primitive` |
| 1 | `primitive_sphere` | `Add Sphere primitive` |
| 2 | `primitive_cylinder` | `Add Cylinder primitive` |
| 3 | `primitive_cone` | `Add Cone primitive` |
| 4 | `primitive_pyramid` | `Add Pyramid primitive` |
| 5 | `primitive_slope` | `Add Slope primitive` |
| 6 | `primitive_cornerin` | `Add Corner-in primitive` |
| 7 | `primitive_cornerout` | `Add Corner-out primitive` |
| 8 | `primitive_torus` | `Add Torus primitive` |

### 6.5 Model panel (title `Model`, 3 buttons)

Buttons 48 x 48 at y = 24 + 50k (k = 0..2). Inside each: icon 24 x 24 centred
horizontally with its centre 18 px below the button top (box y 6..30); caption
in the box y 30..48, 11 px, centred horizontally, top-aligned.

| k | Icon | Caption | Tooltip |
|---|---|---|---|
| 0 | `cog` | `Settings` | `Edit Model Properties` |
| 1 | `origin` | `Origin` | `Set Model Origin` |
| 2 | `blueprint` | `Blueprint` | `Set Model Blueprint` |

All create buttons are push buttons (section 16.2) with the skin button colours.

---

## 7. Right tool strip [V]

- Rect `[W-R-112, W-R) x [22, H-24)`. Background: sliced `panel` frame.
- Three panels of fixed height stacked from the top: Edit mode 198
  (window y [22, 220)), Selection 198 ([220, 418)), Recover 202 ([418, 620)).
  The rest of the strip shows only its background.
- Panel chrome: sliced `menu` frame, padding 6; title at (6, 6), 100 x 18,
  11 px; content at y = 24 (Recover: 24.5, because its 2 px of spare height are
  shared by its four layout items; the port may snap to 24) [D].
- Buttons: 100 x 32 at (6, 24 + 34k). Inside: icon 24 x 24 at x = 4, vertically
  centred; label box (32, 8, 68, 20), 11 px, left/top aligned.

### 7.1 Edit mode panel (title `Edit mode`)

Five toggle buttons in one exclusive group that may be switched all off.

| k | Icon | Label | Tooltip |
|---|---|---|---|
| 0 | `cube` | `Element` | `Enter Element edit mode` |
| 1 | `voxel` | `Voxel` | `Enter Voxel edit mode` |
| 2 | `sphere` | `Mesh` | `Enter Mesh edit mode` |
| 3 | `draw_paint` | `Paint` | `Enter Paint edit mode` |
| 4 | `eye` | `Preview` | `Enter Display Settings edit mode` |

### 7.2 Selection panel (title `Selection`)

| k | Icon | Label | Tooltip | Command |
|---|---|---|---|---|
| 0 | `lightbulb_off` | `Show/Hide` | `Hide/Show selection` | toggle visibility |
| 1 | `lock` | `Lock/Unlock` | `Lock/Unlock selection` | toggle lock |
| 2 | `lock_go` | `Focus Lock` | `Lock all objects except selected one` | focus lock |
| 3 | `anchor` | `Reset` | `Reset selection rotation&origin` | reset rotation and origin |
| 4 | `cross` | `Delete` | `Delete selection` | delete (with confirmation) |

### 7.3 Recover panel (title `Recover`)

| k | Icon | Label | Tooltip | Command |
|---|---|---|---|---|
| 0 | `lightbulb` | `Show All` | `Show all hidden objects` | show all |
| 1 | `lock_open` | `Unlock All` | `Unlock all locked objects` | unlock all |
| 2 | `blueprint` | `Blueprint` | `Reset Blueprint to default` | reset blueprint |
| 3 | `camera_go` | `Views` | `Reset Views to default` | reset views |
| 4 | `recover_elements` | `Elements` | `Constrain all elements inside available 3D space` | get back far elements |

---

## 8. Managers group panels

### 8.1 Common rules [V]

- The managers zone stacks its visible panels top to bottom in this order:
  Element tools, Voxel tools, Mesh tools, Elements list, Elements material,
  Meshes list, Paint tools, Color tools, Palette, Gizmos. No padding, no
  spacing; every panel spans the zone width `Z` (default 220) and the panels
  together fill the zone height (stack rules of section 0.4 with children
  expanded, so rigid panels get flexible weight 1).
- Panel layout values:

| Panel | Min height | Preferred | Flexible |
|---|---|---|---|
| Tool panels (Element, Voxel, Mesh, Paint tools, Gizmos) | `24 + grid_height + 4` | = min | 1 |
| Color tools | 98 | 98 | 1 |
| Elements list, Meshes list | 0 | 26 | 1000 |
| Elements material | 122 | 122 | 400 |
| Palette | 48 | 48 | 500 |

- Tool grids: cells 32 x 32, spacing 2, columns `floor((Z - 12 + 2.001) / 34)`
  (5 at Z = 180, 6 at Z = 220, 8 at Z = 300), `grid_height = rows * 34 - 2`.
  The min height of a tool panel is recomputed every frame from its content,
  so tool panels grow or shrink as the zone width changes.
- Resulting heights at Z = 220 [D]: Element tools 94, Voxel tools 94, Mesh
  tools 60, Paint tools 128, Gizmos 60, Color tools 98. At Z = 180 Voxel tools
  is 128; at Z = 300 Paint tools is 94.
- Example distributions, window 1280 x 720 (zone height 674), Z = 220 [D]
  (offset in the zone : height):

| Mode | Panels |
|---|---|
| Element | Element tools 0 : 94.31; Elements list 94.31 : 334.35; Elements material 428.66 : 245.34 |
| Voxel | Voxel tools 0 : 94.49; Paint tools 94.49 : 128.49; Color tools 222.98 : 98.49; Palette 321.46 : 292.05; Gizmos 613.51 : 60.49 |
| Mesh | Mesh tools 0 : 60.36; Meshes list 60.36 : 386.92; Paint tools 447.28 : 128.36; Color tools 575.64 : 98.36 |
| Paint | Paint tools 0 : 128.80; Color tools 128.80 : 98.80; Palette 227.59 : 446.41 |

- Panel chrome (all ten): sliced `menu` frame. Tool panels have a 24 px
  header with the title box from (6, 6) to the right edge, 18 px tall; list
  panels place the title box at (6, 6), 188 x 16. Titles: 11 px, left/top
  aligned.
- Panel menu marker: some panels carry a 32 x 32 marker in their top-right
  corner (`tex_menu`: a light triangle with three dots), drawn as a button with
  the skin "functions" colours (Light: `#363636` rest, `#0080fff0` hover,
  `#0080ff` pressed). Clicking it opens the panel's context popup at the
  pointer. Present on: Voxel tools (voxel popup), Elements list (elements
  popup), Elements material (material popup), Meshes list (meshes popup),
  Palette (colour popup). The markers of the other panels are hidden.
- Tool panel content: x 6 .. `w - 6`, from y = 24; the grid is its only child.
- Tool exclusivity: the 3 toggles of Element tools, the 9 tool toggles of Voxel
  tools, the 9 tool toggles of Paint tools and the colour-picker toggle of
  Color tools form one shared exclusive group that may be switched all off; at
  most one of these 22 tools is active at any time. The Gizmos toggles are
  independent.
- Tool cells: push buttons and toggle buttons are 32 x 32 sliced `button`
  frames with a 24 x 24 centred icon; numeric fields in the grids are 32 x 32
  input fields (section 16.6) with centred text.

### 8.2 Element tools (title `Element`) [V]

| # | Kind | Icon | Tooltip |
|---|---|---|---|
| 1 | toggle | `shape_square_wrench` | `Edit element general properties` |
| 2 | toggle | `cube_faces` | `Edit element faces properties` |
| 3 | toggle | `cube_uv` | `Edit element UV mapping (texture position on faces)` |
| 4 | button | `color_wheel_go` | `Apply random color to selected elements` |
| 5 | button | `color_wheel` | `Set color of selected elements` |
| 6 | button | `cube_uv_apply` | `Apply UV mapping with world coordinates to selected elements` |
| 7 | button | `lightning_delete` | `Automatically removes hidden faces of all elements` |
| 8 | button | `lightning_hide` | `Automatically apply new culling properties to all elements` |
| 9 | button | `sort` | `Automatically sort transparent elements` |

Toggles 1 to 3 open Element Settings on the General, Faces or Textures UV tab.

### 8.3 Voxel tools (title `Voxel`, marker opens the voxel popup) [V]

| # | Kind | Icon | Tooltip | Field |
|---|---|---|---|---|
| 1 | toggle | `voxel_alone` | `Dot: Add/Remove one voxel` | |
| 2 | toggle | `voxel_line` | `Line: Add/Remove line of voxels` | |
| 3 | toggle | `voxel_cuboid` | `Cuboid: Add/Remove cuboid of voxels (like a selection)` | |
| 4 | toggle | `voxel_sphere` | `Sphere brush: Add/Remove sphere of voxels` | |
| 5 | field | | `Sphere brush diameter` | decimal, max 2 chars |
| 6 | toggle | `voxel_cube` | `Cube brush: Add/Remove cube of voxels` | |
| 7 | field | | `Cube brush diameter` | decimal, max 2 chars, shows `7` |
| 8 | toggle | `voxel_laser` | `Laser: Paint or cut voxels in depth` | |
| 9 | toggle | `voxel_extrusion` | `Extrusion: Add/Remove thickness to voxels face` | |
| 10 | toggle | `voxel_shell` | `Shell: Add/Remove layer around collateral voxels` | |
| 11 | toggle | `voxel_select` | `Wrap: Set blueprint around group of collateral voxels` | |

### 8.4 Mesh tools (title `Mesh`) [V]

| # | Kind | Icon | Tooltip |
|---|---|---|---|
| 1 | button | `import` | `Add custom mesh` |
| 2 | button | `resize` | `Resize mesh to fit middle zone` |
| 3 | button | `proportions` | `Resize mesh to stay proportional` |
| 4 | button | `color_wheel_go` | `Apply left color to selected meshes material` |
| 5 | button | `color_wheel` | `Set material color of selected meshes` |

### 8.5 Elements list (title `Elements list`, marker opens the elements popup) [V]

- Content box: x 6 .. `w - 6`, y 26 .. `h - 6`.
- List view: x 0 .. `content_w - 8` of the content box; sliced `content` frame
  background; clips its rows. Vertical scrollbar 8 px wide on the right
  (section 16.10), always shown.
- Rows: 26 px tall, full list width, no gaps, virtualized (only visible rows
  exist). Row anatomy:
  - background: sliced `panel` frame; base colour skin `buttonNormal`, or skin
    `buttonPressed` when the element is selected; push-button state colours
    without fade.
  - icon 16 x 16 at x = 8, vertically centred: hidden elements show
    `lightbulb_off`, locked elements show `lock` (both untinted), otherwise the
    element-type icon (`primitive_cube`, `element_planex`, `element_planey`,
    `element_planez`) tinted with the element colour (opaque).
  - name: box x 32 .. `w - 52`, top 6, 11 px, left/top aligned, skin text
    colour; skin `textError` when the model format is a block or item model and
    the element breaks that format's limits.
  - index: box x 8 .. `w - 8`, top 6, 11 px, right-aligned.
- Scrolling: vertical only, clamped at both ends, no inertia, 28 px per wheel
  step.

### 8.6 Elements material (title `Elements Material`, marker opens the material popup) [V]

- Content box: x 6 .. `w - 6`, y 26 .. `h - 6`.
- Header (top 96 px of the content box):
  - preview square 96 x 96 at (0, 0): a checkerboard (`bg_pattern_small` tiled,
    8 px squares, tinted `#8c8c8c40`) under the texture preview.
  - text lines at x = 104 .. right edge, 16 px tall, at y = 0, 20, 40, 60, 80:
    texture label (bold), file name, path, size (for example `16x16`), file
    weight (for example `1KB`); 11 px, left/top aligned. A line for the image
    format exists at y = 60 but is hidden.
- Texture list: from y = 102 to the content bottom, same chrome as 8.5, rows
  22 px tall; row anatomy: sliced `panel` background with the same colour rules
  as 8.5, label box x 8 .. `w - 8`, top 4, 11 px.
- Add button: 32 x 18 at the bottom-right corner inside the list view,
  sliced `button` frame, text `+` 13 px bold centred, tooltip `Add texture`.

### 8.7 Meshes list (title `Meshes list`, marker opens the meshes popup) [V]

Same as 8.5 with 28 px rows. Row icons: hidden `lightbulb_off`, locked `lock`,
otherwise by mesh type: custom mesh `symb_file_3d`, then `primitive_cube`,
`primitive_sphere`, `primitive_cylinder`, `primitive_cone`,
`primitive_pyramid`, `primitive_slope`, `primitive_cornerin`,
`primitive_cornerout`, `primitive_torus`, extruded mesh `element_planey`.

### 8.8 Paint tools (title `Paint`) [V]

| # | Kind | Icon | Tooltip | Field |
|---|---|---|---|---|
| 1 | toggle | `draw_pixel` | `Pen: Set color of one pixel/voxel` | |
| 2 | toggle | `draw_line` | `Line: Draw a line of pixels/voxels` | |
| 3 | field | | `Line diameter` | decimal, max 2 chars, shows `1` |
| 4 | toggle | `draw_rectangle` | `Rectangle: Create a rectangle of pixels or voxels` | |
| 5 | toggle | `draw_brush` | `Round brush: Change color in round area` | |
| 6 | field | | `Round brush diameter` | decimal, max 2, shows `7` |
| 7 | toggle | `draw_paint` | `Square brush: Change color in square area` | |
| 8 | field | | `Square brush diameter` | decimal, max 2, shows `7` |
| 9 | toggle | `draw_fill` | `Bucket: Flood zone with color` | |
| 10 | toggle | `draw_sprayer` | `Sprayer: Change first clicked color in sprayer area` | |
| 11 | field | | `Sprayer diameter` | decimal, max 2, shows `7` |
| 12 | toggle | `draw_magic` | `Magic: Replace clicked color with new color` | |
| 13 | toggle | `draw_fillnoise` | `Noise: Flood zone with random shades of color` | |
| 14 | field | | `Noise amount (0-255)` | decimal, max 3, shows `32` |

### 8.9 Color tools (title `Color`) [V]

- Content: x 6 .. `w - 6` from y = 24; two rows stacked with 2 px spacing; each
  row is 34 px tall (2 px top padding + 32), children vertically centred.
- Row layout (left to right, spacing 2): colour field (min width 94, flexible
  100, 24 px tall), button 32 x 32 (flexible 1), button 32 x 32 (flexible 1).
  At Z = 220 the field is 139.1 px wide and each button 32.45 px [D].

| Row | Field (tooltip, max chars, initial text) | Button 2 | Button 3 |
|---|---|---|---|
| Left colour | `Left RGBA color`, 9, `#808080` | `color_palette`, `Set left color` | toggle `color_picker`, `Color Picker: Get clicked color` (shared tool group) |
| Right colour | `Right RGBA color`, 9, `#00000000` | `color_palette`, `Set right color` | `replace`, `Switch left and right colors` |

- Colour field: a text field of at most 9 characters with no character filter
  (`#RRGGBBAA` fits), text box inset 8 left and right and 4 from the top;
  placeholder `#000000`. At its right end two glyphs share one centre, 13 px in
  from the field's right edge and vertically centred: first a small cross
  (`cross_small`, 22 x 22, tinted `#363636` in every skin, because the skin
  pass only recolours `symb_` glyphs), then, drawn above it, the colour swatch
  (`disc` drawn 26 x 26, a disc of about 13 px, tinted with the field's colour,
  alpha included).
- The cross has no behaviour: it is not a button and nothing shows, hides or
  reads it. It is seen only where the swatch is see-through. Measured on the
  original art, the cross reaches 5.85 px from the centre and the disc is solid
  out to 6.0 px, so an opaque colour hides the cross completely, a partly
  transparent colour lets it show faintly, and alpha 0 (for example the default
  right colour `#00000000`) shows the bare cross.
- Display format: `#` followed by red, green, blue and alpha as two uppercase
  hex digits each, always 9 characters with alpha present. Whenever a colour is
  assigned from elsewhere (palette pick, colour dialog, switch button, colour
  picker tool) the field text is rewritten in this form. While the user types,
  the text is only converted to upper case, not reformatted.
- Parsing runs on every change of the text, so typing applies live; empty text
  is ignored (colour, swatch and palette markers keep their state):
  1. upper-case the text and drop every character that is not `0`-`9` or
     `A`-`F` (`#`, spaces, `x` and other letters disappear);
  2. if fewer than 6 digits remain, pad with `0` on the right to 6 digits (no
     `#RGB` shorthand expansion);
  3. red, green and blue are the first three digit pairs; alpha is the fourth
     pair only when exactly 8 digits remain, otherwise 255 (a 7th digit and any
     digits beyond the 8th are ignored).

  The colour is always the parsed 8-bit value, also after an assignment from
  elsewhere; the swatch follows every parse, and each parse re-places both
  palette markers (8.10).

| Typed | Resulting colour (the text stays as typed, upper-cased) |
|---|---|
| `#FF8800` | `#FF8800FF` |
| `FF880080` | `#FF880080` |
| `F80` | `#F80000FF` |
| `#FFF` | `#FFF000FF` |
| `0xFF8800` | `#0FF880FF` |
| `1234567` | `#123456FF` |
| `12345678` | `#12345678` |
| `#` or `xyz` | `#000000FF` |
| (empty) | unchanged |

- Colour buttons (`color_palette`): open the Color Picker dialog (dialogs spec)
  titled `Left color` or `Right color` with the field's current hex text; the
  dialog's result is parsed with the rules above. The switch button exchanges
  the two colours (both fields are rewritten). The picker toggle is the colour
  picker tool (model spec 13.4); it switches itself off whenever a new primary
  object is selected and when the Color tools panel is hidden.
- The colours are not saved. Every start shows the authored texts `#808080`
  (left, parsed as `#808080FF`) and `#00000000` (right).

### 8.10 Palette (title `Palette`, marker opens the colour popup) [V]

- The palette picture fills x 6 .. `w - 6`, y 26 .. `h - 6`, stretched without
  keeping its aspect ratio (cells are rectangles when the panel is not in
  proportion), untinted, nearest-texel sampling. Loaded pictures are reduced
  to at most 16 x 32 texels (below); only a `palette.png` replaced outside the
  program can be larger.
- The panel starts hidden, but its picture and markers are kept up to date
  while it is hidden.

Cell positions below are written (column, row) with row 0 at the **bottom** of
the picture.

**Presets.** The colour popup (menus spec) lists seven built-in palettes, each
with its own picture as the 16 px row icon, then a separator, `Load new
palette` and `Save palette`.

| Row | Picture | Size (w x h) | Opaque colours | Transparent cells |
|---|---|---|---|---|
| `RGB palette (default)` | `rgb256` | 16 x 16 | 255, all distinct | 1, at (0, 0) |
| `HSL palette` | `hsl256` | 12 x 22 | 256 distinct; black `#000000` occurs 5 times, at (11, 17) and (10, 18 .. 21) | 4, column 11, rows 18 .. 21 |
| `JMP palette` | `jmp_arne_niklas_jansson` | 4 x 4 | 16 distinct | none (no alpha channel) |
| `Generic16 palette` | `generic16_v20_arne_niklas_jansson` | 4 x 4 | 16 distinct | none (no alpha channel) |
| `Pico8 palette` | `pico8` | 4 x 4 | 16 distinct | none (no alpha channel) |
| `EDG32 palette` | `edg32` | 6 x 9 | 32 distinct | 22, which keep non-zero RGB values |
| `EDG16 palette` | `edg16` | 5 x 6 | 16 distinct | 14, likewise |

The presets are point-filtered and clamp at their edges. Marker placement
depends on their exact cell layout, so the port must reproduce each preset
cell for cell; how cuboid obtains the colour values (rebuilt from the
published palettes of the same names, or extracted locally as in section 15.1)
is a packaging decision [U].

**Load new palette.**

1. A file dialog (dialogs spec; the native dialog only when the preference
   "Use Native file browser if possible" is on, default off) titled
   `Select a picture to load as palette`, filters `PNG Texture (*.png)` and
   `JPG Picture (*.jpg)`. Start folder: the folder of the last palette load in
   this session (not saved), else the textures path of Preferences, else the
   Desktop, else `/`.
2. The chosen file's folder is remembered, then the file is decoded with
   nearest-texel filtering. JPEG has no alpha, so its cells are opaque. A read
   failure shows the error box `Error while loading texture` and the status
   `Failed loading texture: <path>`, and nothing else changes. A readable file
   that is not a valid image raises no error: the palette becomes whatever the
   failed decode leaves (a 1 x 1 or small placeholder picture [U]) and is
   written to `palette.png` like any loaded picture.
3. If the picture is wider than 16 or taller than 32 texels it is resampled to
   `min(w, 16)` x `min(h, 32)`: each axis is limited on its own, the aspect
   ratio is not kept, nothing is cropped or letterboxed, and sampling is
   nearest-texel. A picture within the limit is used as it is. The original
   resamples by drawing into an offscreen target; whether this changes
   semi-transparent texels or the RGB of fully transparent texels is [U], and
   the orientation is assumed to be kept [U]. (The original never frees that
   offscreen target; the port must not leak it.)
4. The result becomes the palette (markers re-searched, see below; the colours
   do not change) and is written to `palette.png`.

**`palette.png`.** The current palette is kept as `palette.png` in the user
data folder (original: `%USERPROFILE%\AppData\LocalLow\BDcraft\Cubik\`).

- Start-up: if the file exists it is used as it is (no size check, no
  resampling, not rewritten); otherwise the RGB preset is shown and written to
  the file. If the file exists but cannot be read, the error box above appears
  and, in the original, the rest of the colour tools' start-up is skipped (the
  hex fields, colour buttons, picker and switch stay unconnected; runtime
  effect [U]); the port falls back to the RGB preset instead. A readable but
  undecodable file gives the placeholder picture of step 2 without rewriting
  the file [U].
- Choosing a preset deletes the file, shows the preset and writes it to the
  file. If that write fails, the preset stays on screen and the file stays
  missing until the next start writes the RGB default.
- A successful load writes the (resampled) picture. If the write fails, the new
  palette stays on screen.
- A successful write shows the status `Successfuly saved texture: <path>`
  (sic); a failed write shows the error box `Error while saving file` and the
  status `Failed saving texture: <path>`.
- Never written by Save palette, a failed load, colour changes or the picker
  tool.

**Save palette.** A save dialog titled `Chose a path to save palette` (sic),
filter `PNG Texture (*.png)`, default name `cubik_palette`, extension `png`.
Start folder: the folder of the last palette save in this session, initially
`/` (the drive root); the textures path, Desktop and `/` fallbacks apply only
when that folder no longer exists. It writes the palette picture as currently
shown (preset, or loaded and resampled picture) as PNG, appending `.png`
unless the name already ends with `.png` in any letter case. It does not touch
`palette.png` or the palette. The folder is remembered even when the write
fails. Success and failure messages as for `palette.png` above.

**Picking.** Pointer down and every drag step pick a texel:

- The pointer is converted to the palette's local space (origin at the
  top-left corner, y negative downward) and both coordinates are truncated
  toward zero; then `column = trunc(x * W / rect_width)` and
  `row = trunc(y * H / rect_height) + H - 1` (W x H = picture size).
- The texel is read; alpha 0 gives fully transparent black. The left button
  sets the left colour and the right button the right colour (the field is
  rewritten, 8.9); other buttons only re-place the markers.
- The exact right edge maps to column W and the exact bottom edge to row -1,
  both outside the picture, and a drag that leaves the palette keeps picking
  outside it. Outside the picture
  the read follows the picture's wrap mode: presets clamp to the edge texel,
  pictures loaded from a file (including `palette.png` at start-up) wrap
  around [D]. The port clamps to the picture in both cases.

**Markers.** Two markers, each one cell in size: a hollow 1 px frame
(`bg_border`) with a centred 8 px letter `L` (left colour) or `R` (right
colour).

- A marker shows where its colour is **found first** in the picture, not where
  it was clicked: a pick assigns the colour to its field, and the field's
  update searches again for both colours (8.9). Colour dialog, picker tool and
  switch button go through the same search.
- Search: rows from the bottom up, each row left to right; the first texel
  equal to the colour wins (approximate equality, squared distance below
  1e-10, which is exact for 8-bit colours). A colour with alpha 0 matches the
  first texel with alpha 0 whatever its RGB.
- Duplicates: only the first occurrence is ever marked. In the HSL preset every
  black pick marks (11, 17); the black cells (10, 18 .. 21) are never marked.
- Colour not in the picture: that marker (frame and letter) is hidden; the
  colour stays in its field and swatch.
- Both colours on one cell: both markers are drawn there, `R` above `L`.
- Changing the palette (preset or load) searches again with the current
  colours: markers may move or disappear, the colours never change.
- Placement: a cell is `rect_width / W` by `rect_height / H`; the marker of
  (column, row) has its top-left corner at
  `(column * cell_w, (H - 1 - row) * cell_h)` from the palette's top-left and
  the size of one cell. Markers are re-placed whenever the palette rectangle
  changes size.
- Frame and letter are black when the marked colour is light
  (`r + g + b > 1.5`) or mostly transparent (alpha < 0.5), white otherwise. The
  letters are not skinned.
- With the default colours (`#808080FF`, `#00000000`): `L` is hidden in every
  preset (that grey is in none of them); `R` marks the first transparent cell,
  (0, 0) in RGB, (11, 18) in HSL, (5, 0) in EDG32, (1, 0) in EDG16, and is
  hidden in JMP, Generic16 and Pico8, which have no transparent cell.

**Parity notes.** Keep first-match marking (marking the clicked cell would
differ from the original), the per-axis stretch to at most 16 x 32 with
nearest sampling, and the start-up order (an existing `palette.png` is used
without validation, otherwise the RGB preset is written). A readable but
invalid picture chosen with Load new palette replaces the palette and
`palette.png` in the original; reproduce this or reject the file, but decide
deliberately.

### 8.11 Gizmos (title `Gizmos`) [V]

| # | Kind | Icon | Tooltip |
|---|---|---|---|
| 1 | toggle | `bounding_box` | `Show / Hide Bounding Box` |
| 2 | toggle | `bounding_box_handles` | `Show / Hide Bounding Box Handles` |

---

## 9. Helpers group panels

Both helpers fill the helpers zone (width `Lh`, default 350; height `H - 46`).
In this section `y` is measured from the top of the helper panel
(window y = 22 + y).

### 9.1 Element Settings [V]

```
y=0  +----------------------------------------------------+
     | Element Settings                            title  |   sliced `menu` frame
y=26 | +--------+-------+-------------+                   |
     | |General | Faces | Textures UV |      tabs 25 px    |
y=50 | +========+---------------------------------------+ |
y=52 | |  +-Label------------------------------------+  | |   page: `content` frame
y=58 | |  | [Enter text...                         ] |  | |
     | |  +------------------------------------------+  | |
     | |  +-Position---------------------------------+  | |
     | |  |  X: [      ]   Y: [      ]   Z: [      ] |  | |
     | |  +------------------------------------------+  | |
     | |    Size, Origin, UV Snap, Rendering ...        | |
     | +------------------------------------------------+ |
     +----------------------------------------------------+
```

- Frame: sliced `menu`. Title `Element Settings` at (6, 6), 188 x 18, 11 px.
- Content box: x 6 .. `Lh - 6`, y 26 .. `h - 6`.
- Tab strip: x 6 .. `Lh - 6`, y 26 .. 51. Tabs use the sliced `tab` frame (no
  bottom border); one exclusive group that cannot be switched off:

| Tab | x | Width | Default |
|---|---|---|---|
| `General` | 6 | 64 | active |
| `Faces` | 70 | 64 | |
| `Textures UV` | 134 | 96 | |

  Active tab: 25 px tall (y 26 .. 51), base colour skin `buttonNormal`.
  Inactive tab: 21 px tall with its bottom raised 1 px (y 29 .. 50), base
  colour skin `buttonDisabled`. Tab text: inset 8 left and 4 top, 11 px,
  left/top aligned. Tabs have no tooltip. The active tab overlaps the page
  frame's top border by 1 px, so it joins the page visually.
- Page frame: sliced `content`, x 6 .. `Lh - 6`, y 50 .. `h - 6`. Page content
  is inset 8 on every side (x 14 .. `Lh - 14`, from y = 58); its width is
  `Pw = Lh - 28` (322 at the default width).
- Disabled overlay: while no element is selected an overlay covers the whole
  helper panel: sliced `panel` frame tinted white whose alpha moves toward 0.9,
  and the centred italic text `Select an element to access its settings`
  (11 px) whose alpha moves toward 1. The animation runs at 50 Hz:
  `alpha += (target - alpha) * 0.2` per step while appearing,
  `alpha += (0 - alpha) * 0.4` per step while disappearing; the overlay is
  removed when its alpha is 0.01 or less (about 0.18 s). It blocks the pointer
  while present.

**Group template** (used by every group of both helpers and by the generic
dialogs): sliced `content` frame; a caption chip whose top-left is at (8, -6)
relative to the group, so it straddles the group's top border like a fieldset
legend: 15 px tall, width = caption text width + 8, solid skin `label` colour,
caption text at x + 4, 11 px; group content box x 8 .. `w - 8`, y 12 .. `h - 8`
(the vector groups of Display Settings use y 10 .. `h - 8`).

**General tab** (vertical stack, spacing 14; groups keep their preferred
heights; y from the page content top at y = 58):

| Group | y | Height | Content |
|---|---|---|---|
| `Label` | 0 | 44 | text field over the full width, 24 px tall, vertically centred; placeholder `Enter text...`; max 64 chars |
| `Position` | 58 | 44 | `X:` field `Y:` field `Z:` field |
| `Size` | 116 | 44 | same |
| `Origin` | 174 | 44 | same |
| `UV Snap` | 232 | 128 | checkbox `UV Snap`; hint; checkbox `UV Snap Symmetry`; hint |
| `Rendering` | 374 | 78 | checkboxes `Disable shading`, `Rescale element based on rotation`, `Invert faces normals` |

- X/Y/Z rows: one row of six cells over the group content width `Cw = Pw - 16`
  (306): three labels (`X:`, `Y:`, `Z:`, 11 px, centred) with flexible weight 1
  and three decimal fields (24 px tall) with flexible weight 2. With empty
  fields the cells are 40, 62, 40, 62, 40, 62 px at Cw = 306; in general
  `label = 9 + (Cw - 27) / 9` and `field = 2 * (Cw - 27) / 9` [D]. Fields:
  decimal, no length limit, placeholder `0`.
- UV Snap content (108 px, stacked, children expanded): checkbox row 22,
  hint 32, checkbox row 22, hint 32. Hints are 11 px, wrapped, left/top
  aligned:
  `Automatically modify textures coordinates of all faces with the real Element coordinates`
  and `Makes the East and North textures flipped horizontally to face opposite side.`
- Rendering content (58 px): three checkbox rows of 20 px at y 0, 20, 40 (the
  third overflows by 2 px; keep it).

**Faces tab** (stack, spacing 12): six face blocks `Down`, `Up`, `North`,
`South`, `West`, `East`, 70 px tall, at y = 0, 82, 164, 246, 328, 410.

- Face block: a 20 px wide toggle stripe on the left over the full block
  height (sliced `button` frame, toggle-button colours, on = face enabled) and,
  from x = 20 to the right edge, a group with the face name as caption. Group
  content (stack, spacing 4): two lines of 23 px.
- Line 1 (row, spacing 8): `Tint index:` label (59 px, vertically centred),
  integer field (40 px, max 6 chars, placeholder `0`, tooltip
  `Tint channel (used for grass, spawn eggs, potions, ...)`), `Culling:` label
  (43 px), culling dropdown (120 px at the default width, flexible, shows
  `none`, tooltip `Direction to not render face when hidden by a block`).
  Text in these 23 px fields starts 5 px below the top.
- Line 2 (row, spacing 8): `Texture:` label (45.65 px), texture field with a
  list button (232.35 px, flexible; text, max 64 chars, placeholder
  `Texture label...`); the list button is 20 px wide at the right end inside
  the field and shows a down-arrow glyph (24 px).
- The culling dropdown lists `none`, separator, `down`, `up`, `north`,
  `south`, `west`, `east` (popup minimum width 96).

**Textures UV tab** (stack, spacing 28):

1. `Fast UV` group (44 px): one row of four buttons (spacing 4, each
   `(Cw - 12) / 4` = 73.5 px wide, 24 px tall); field-style buttons (sliced
   `bg_border_fill`, a 1 px frame over a 15 % white fill, field colours):

| Button | Icon (size, tint) | Text | Tooltip |
|---|---|---|---|
| Full | `arrow_out` (16) | `Full` | `UV Mapping Full face` |
| Up | `element_planey` (20, `#b4b4ff`) | `Up` | `UV Mapping by projection from Up to Down (Y axis)` |
| West | `element_planex` (20, `#ffa0a0`) | `West` | `UV Mapping by projection from West to East (X axis)` |
| South | `element_planez` (20, `#96ff96`) | `South` | `UV Mapping by projection from South to North (Z axis)` |

   Icon at x = 4, vertically centred; text from x = 28, vertically centred,
   left-aligned.

2. Texture editor group: min height 312, takes all remaining page height
   (510 px at window height 700). Parts:
   - Face tabs: one row of six tabs whose top edge is 24 px above the group's
     top edge and whose bottom overlaps it by 1 px; tabs are bottom-aligned,
     spacing -1, sharing the group width (min widths 18, 4, 8, 10, 8, 8 plus an
     equal share of the rest: 63.17, 49.17, 53.17, 55.17, 53.17, 53.17 px at
     the default width). Active tab 25 px tall, inactive 21 px. Each tab shows
     a 16 px icon left of its centred caption (`Down`/`Up` with
     `element_planey` `#b4b4ff`, `North`/`South` with `element_planez`
     `#96ff96`, `West`/`East` with `element_planex` `#ffa0a0`); caption box
     from x = 16, top 4, 11 px, centred. `Down` is active by default.
   - Tools grid (at the group content top): cells 70 x 24, spacing 2, four per
     row: `Edit` (`draw_paint`, 20 px icon), `Swap` (`swap`), `Copy`
     (`page_copy`), `Paste` (`page_paste`); then `Flip X`
     (`shape_flip_horizontal`), `Flip Y` (`shape_flip_vertical`), `+90°`
     (`shape_rotate_clockwise`), `-90°` (`shape_rotate_anticlockwise`), 16 px
     icons. Same field-style buttons as Fast UV; icon at x = 4 (Edit) or 6; text
     from x = 28 (30 for the rotate buttons). Tooltips in order:
     `Open texture in picture editor`, `Swap UV with opposite face`,
     `Copy current UV Mapping`, `Paste UV Mapping from clipboard`,
     `Flip UV Mapping Horizontally`, `Flip UV Mapping Vertically`,
     `Rotate UV Mapping by +90°`, `Rotate UV Mapping by -90°`.
   - Texture view: from 54 px below the content top to the content bottom,
     opaque `#363636` background, clips its content. Inside, at half scale, a
     512 x 512 zone (256 x 256 on screen) with a 32 px (16 px on screen) margin
     shows a checkerboard (`bg_pattern_big` tiled, 8 px squares on screen,
     tinted `#ffffff10`) under the face texture (nearest texel), and the UV
     selection rectangle (`bg_border_fill` frame) with rotation and origin
     markers (`cross`, `circle`, 24 px, kept at a constant screen size).
     Behaviour (wheel zoom 0.5 .. 4 while hovered, middle-button pan,
     selection drag, double-click recentre): UV editor spec.
   - Scrollbars: horizontal at the bottom and vertical at the right, 10 px
     thick, invisible track, sliced `button` handle (section 16.10).

### 9.2 Display Settings [V]

- Frame: sliced `menu`. Title `Display Settings` at (6, 6), 18 px tall.
- Content box: x 6 .. `Lh - 6`, y 26 .. `h - 6`.
- Tab strip (48 px, y 26 .. 74): two rows of four tabs, one exclusive group
  that cannot be switched off:

| Row | Tabs (min width) |
|---|---|
| A | `Right hand 3rd` (96), `Left hand 3rd` (96), `Head` (48), `Floor` (48) |
| B | `Right hand 1st` (96), `Left hand 1st` (96), `Wall` (48), `GUI` (48) |

  Each row shares the strip width with -1 px spacing (at the default width
  109.25, 109.25, 61.25, 61.25 px at x 0, 108.25, 216.5, 276.75). Rows are
  21 px tall and bottom-aligned in the strip (y 6 .. 27 and 27 .. 48 of the
  strip). The row that contains the active tab is always the lower row (the
  rows swap places when a tab of the upper row is chosen); the active tab is
  25 px tall, rises 4 px into the upper row and is drawn over it. Inactive tabs
  are 21 px tall with the skin `buttonDisabled` base colour. Tab text inset 8
  left and 4 top, 11 px. Which tab is active when the helper first opens is [U].
- Page frame: sliced `content`, from y = 73 (overlapping the tab strip by
  1 px) to the content bottom; page content inset 8 (x 14 .. `Lh - 14`, from
  y = 81), width `Pw = Lh - 28`.
- All eight pages share this template (stack, spacing 14; y from the page
  content top):

| Item | y | Height | Pages |
|---|---|---|---|
| Options row | 0 | 22 | all |
| `Rotation` group | 36 | 106 | all |
| `Translation` group | 156 | 106 | all |
| `Scale` group | 276 | 106 | all |
| `Presets` group | 396 | 44 | all |
| `Arm angle` / `Head angle` group | 454 | 44 | Right hand 3rd and Left hand 3rd (`Arm angle`), Head (`Head angle`) |
| filler | rest | flexible | all |

- Options row: checkbox `Export settings` on the left and, on the four hand
  pages, a push button on the right with centred text: `Copy left hand` on
  Right hand 3rd and Right hand 1st, `Copy right hand` on Left hand 3rd and
  Left hand 1st. Widths: checkbox `(Pw - 6) / 2`, button `6 + (Pw - 6) / 2`
  (158 and 164 at Pw = 322) [D]. Head, Floor, Wall and GUI show the checkbox
  at full width and no button.
- Vector groups (Rotation, Translation, Scale): content box x 8 .. `w - 8`,
  y 10 .. `h - 8`; rows X, Y, Z at y = 0, 32, 64 of the content box, each
  24 px tall:
  - label `X:`, `Y:`, `Z:`: 16 x 24 at x = 0, right-aligned, vertically centred;
  - slider at x = 24 .. `Cw - 52` (230 px at Cw = 306), y 2 .. 22;
  - decimal field 44 x 24 at the right end (x `Cw - 44` .. `Cw`), max 6 chars,
    text inset 8 left and right and 6 from the top; its placeholder is never
    shown.
- Ranges and defaults: Rotation -180 .. 180 whole numbers, default 0;
  Translation -80 .. 80 continuous, default 0; Scale -4 .. 4 continuous,
  default 1; Angle -180 .. 180 whole numbers, default 0.
- Presets group: content box from y = 10; one row with spacing 6: preset
  dropdown (flexible 100, 25 px tall, shows `none`), `+` and `-` buttons (each
  25 px plus their share, sliced `panel` frames, centred text). At Cw = 306:
  239.22, 27.39, 27.39 px [D].
- Angle group: content box from y = 12; one row `A:` with a slider and a field
  laid out like the vector rows.
- The vector, presets and angle groups take their frame and chip from the
  skin `content` and `label` slots (the source scene mixes light and dark
  variants; the skin normalizes them, section 12.4).
- Behaviour of values, presets and export flags: display settings spec.

---

## 10. Overlay layer

### 10.1 Classic tooltip [V]

```
            +------------------------------+
  target  <|  Tooltip text (12 px, white) |      body: #000000c8, no frame
  button   +------------------------------+      arrow: triangle, same colour
```

- Shown immediately (no delay) when the pointer enters a control that has a
  tooltip, if the preference `showClassicTooltips` is on (default on); hidden
  when the pointer leaves it. Entering also writes the tooltip text to the
  status bar Info cell (section 4), regardless of the preference.
- Controls with tooltips: push buttons, toggle buttons, text fields and
  field-style buttons. Tabs, checkboxes, sliders and menu rows have none.
- Body: solid `#000000c8` rectangle (no sprite, no border). Text: 12 px, white,
  vertically centred, left-aligned, wrapping, inset 6 left and right and 2 top
  and bottom; never recoloured by the skin.
- Size: let `p` = the text's single-line width (integer). Width
  `= min(200, p) + 16`; height `= (floor(p / 200) + 1) * 16 + 16` (one line:
  32 px). The line count estimate can differ from the real wrap; keep the
  formula for parity. [U] (visible effect when it differs)
- Placement uses the target's anchor point `(tx, ty)` (its pivot; for almost
  every control this is its top-left corner) and its size `(tw, th)`:
  - if `tx > window_width / 2` (integer division): the tooltip goes to the
    left: its right edge at `tx - 8 * scale`, the arrow on its right edge
    pointing right;
  - otherwise it goes to the right: its right edge at
    `tx + (tooltip_width + tw + 8) * scale` (left edge 8 px right of the
    target), the arrow on its left edge pointing left;
  - vertically the tooltip is centred on `ty - th * scale / 2`;
  - both coordinates are truncated to even pixels (`floor(v / 2) * 2`).
- Arrow: the `arrow_right` triangle glyph drawn 16 x 16, tinted `#000000c8`,
  attached outside the body edge facing the target (the visible triangle is
  about 7.5 x 15 px, so it ends about 0.5 px short of the target).

### 10.2 Tool cursor [V]

A 16 x 16 symbol follows the pointer, offset 16 px right and 16 px down
(times the UI scale), showing the current manipulation tool: move
(`bulletmove`), rotate (`bullet`), scale (`bulletscale`), clone (`bullet+`),
selection (`bulletsquare`), custom (`bullet`). When it is shown: tools spec.

---

## 11. Dialog chrome [V]

All dialogs share one frame; their contents are in the dialogs spec.

```
 0                                                    w
 +----------------------------------------------------+  0
 |   (16 px soft shadow ring, outside the frame line)   |
 |  +----------------------------------------------+  |  16
 |  | Title                        drag strip  [X] |  |  16..48: title row
 |  |                                              |  |
 |  |   content area x 32..w-32, y 56..h-72        |  |
 |  |                                              |  |
 |  |----------------------------------------------|  |  h-56: separator line
 |  |                       [  OK  ]  [ Cancel ]   |  |  footer band
 |  +----------------------------------------------+  |  h-16
 +----------------------------------------------------+  h
```

- Background: sliced `window` frame over the whole dialog rectangle. The
  sprite carries a 16 px soft shadow outside a 1 px outline, a light header
  gradient, a white body, and a footer band 38 px tall above the bottom outline
  separated by a 1 px line (section 13). So the visible panel is inset 16 px
  from the dialog rectangle on every side.
- Title: at (24, 16), width `w - 40`, height 30, 11 px, vertically centred,
  left-aligned.
- Close button: 24 x 24 at (`w - 40`, 16), `symb_close` glyph (a disc with an
  X cut out) with base colour `#ffffff96` and state colours rest = skin text
  colour, hover `#e11a1a`, pressed `#740000`, disabled `#72727280`, all with
  multiplier 2. Effective Light colours: rest about `#6c6c6c`, hover about
  `#ff3434`, pressed about `#e80000`, opaque. [D]
- Drag strip: transparent, x 16 .. `w - 64`, y 16 .. 48. Dragging it moves the
  dialog with the pointer; the dialog may overhang the window by at most 16 px
  on any side; positions are truncated to whole pixels.
- Resize grip (on resizable dialogs): a transparent square (24 or 16 px, see
  the table) whose bottom-right corner is 16 px left of and 16 px above the
  dialog's bottom-right corner. Hover shows the diagonal resize cursor;
  dragging sets `size = start + (dx, dy)` (y downward), clamped per axis between
  the dialog's initial size and initial size + "max extra".
- OK and Cancel (when present): push buttons 110 x 26, labels `OK` and `Cancel`
  centred; Cancel's right edge at `w - 24`, OK's right edge at `w - 140`; both
  bottoms 23 px above the dialog bottom.
- Content area: x 32 .. `w - 32`, y 56 .. `h - 72`; groups inside use the group
  template of section 9.1 (stack spacing 14).
- Opening: the dialog is centred in the window (its size times the UI scale,
  integer-truncated) every time it opens; focus goes to the dialog's default
  control or else to its OK button. Keys act on key release: Enter (main or
  keypad) confirms when the dialog has "Enter confirms"; Escape cancels when
  the dialog has "Escape cancels" and is not modal. Close and Cancel cancel, OK
  confirms. Moving the pointer onto any dialog clears the active 3D view. How a modal
  dialog blocks the rest of the window (the original sets an application-wide
  modal flag that other input handlers consult) is defined in the dialogs
  spec. [U]

| Dialog | Size | Modal | Enter confirms | Esc cancels | Grip | Max extra (w, h) |
|---|---|---|---|---|---|---|
| Preferences | 498 x 538 | no | no | yes | 24 | 200, 400 |
| Grid settings | 420 x 314 | no | no | yes | 16 | 200, 0 |
| Voxel settings | 420 x 246 | no | no | yes | 16 | 200, 0 |
| Voxelization | 420 x 214 | no | no | yes | 16 | 200, 0 |
| Extrusion | 420 x 224 | no | no | yes | 16 | 200, 0 |
| Model properties | 472 x 524 | yes | no | (yes, but modal) | 24 | 200, 200 |
| Rename | 400 x 172 | yes | yes | (yes, but modal) | 24 | 200, 0 |
| Vector input | 400 x 172 | no | yes | yes | 24 | 200, 0 |
| Color picker | 304 x 522 | no | yes | yes | none | - |
| Faces textures | 420 x 438 | no | no | yes | 16 | 200, 0 |
| Progress | 400 x 172 | yes | no | (yes, but modal) | 24 | 200, 0 |
| Message box | 360 x 180 | see dialogs spec | | | 16 | 200, 200 |
| About box | 640 x 370 | own picture frame, no chrome | | | none | - |

Message box layout: title as above; icon 48 x 48 at (32, 56); message text box
x 96 .. `w - 32`, y 54 .. `h - 64`, 11 px, left/top aligned, wrapping; up to
three push buttons 100 x 28, right-aligned with 4 px spacing, in the strip
x 24 .. `w - 24` whose bottom is 22 px above the dialog bottom.

---

## 12. Skin system

### 12.1 Model [V]

A skin has a name, eight frame pictures ("slots") and a colour table.

| Slot | Role |
|---|---|
| `button` | button faces, menu bar buttons, menu rows, toggle stripes, scrollbar handles |
| `content` | group frames, page frames, list backgrounds |
| `label` | caption chips, menu bar background |
| `tab` | tabs |
| `menu` | panel frames with the 24 px header band, status bar cells |
| `panel` | tool strip backgrounds, popup and dropdown panels, list rows, side grips, slider tracks, `+`/`-` buttons, disabled overlay |
| `separator` | menu separators |
| `window` | dialog frames |

Colour table tokens:

| Token | Used for |
|---|---|
| `buttonNormal` | rest colour of buttons, toggle buttons, tabs (active), side grips; base colour of unselected list rows |
| `buttonHover` | hover colour of the same widgets and of menu rows |
| `buttonPressed` | pressed colour; base colour of "on" toggle buttons and selected list rows |
| `buttonDisabled` | disabled colour of every widget kind; base colour of inactive tabs |
| `functionsNormal` | panel menu markers, rest |
| `functionsHover` | panel menu markers, hover |
| `functionsPressed` | panel menu markers, pressed |
| `text` | all skinned text, symbol glyphs, dialog close glyph rest colour |
| `textError` | invalid entries in lists (red) |
| `fieldNormal` | rest colour of text fields, field-style buttons, checkbox frames and check marks |
| `fieldFocus` | hover, pressed and focus colour of the same; selection highlight (at 50 % alpha); hover and drag colour of the Display Settings slider handles |
| `objectHover` | 3D: highlight outline of the object under the pointer |
| `objectSelect` | 3D: highlight outline of selected objects; lines of the selection bounding box |
| `modelBox` | 3D: model box guide lines |
| `objectLinesWidth` | stored with the skin but used by no live code path (the hover and selection wire boxes that would use it are never created); the port may ignore it |

### 12.2 Colour tables [V]

Values are exact; floats are given where the 8-bit hex is rounded.

| Token | Light | Dark | Rainbow |
|---|---|---|---|
| `buttonNormal` | `#ffffffff` | `#ffffffff` | `#ffffffff` |
| `buttonHover` | `#c5e7ffff` (0.772059, 0.905680, 1, 1) | `#cceaffff` (0.801471, 0.917850, 1, 1) | `#ff96f4ff` (1, 0.588235, 0.956078, 1) |
| `buttonPressed` | `#a5cae5ff` (0.646410, 0.793342, 0.897059, 1) | `#9ad4ffff` (0.602941, 0.832491, 1, 1) | `#ff6ff0ff` (1, 0.433824, 0.939608, 1) |
| `buttonDisabled` | `#dcdcdc78` (0.862745 x3, 0.470588) | `#b4b4b48c` (0.705882 x3, 0.549020) | `#7171718c` (0.441176 x3, 0.549020) |
| `functionsNormal` | `#363636ff` | `#ffffff96` (alpha 0.588235) | `#000000c8` (alpha 0.784314) |
| `functionsHover` | `#0080fff0` (0, 0.501961, 1, 0.941176) | `#0080ffdc` (alpha 0.862745) | `#000000ff` |
| `functionsPressed` | `#0080ffff` | `#0080ffff` | `#000000ff` |
| `text` | `#363636ff` | `#ffffffff` | `#000000ff` |
| `textError` | `#ff0000ff` | `#ff0000ff` | `#ff0000ff` |
| `fieldNormal` | `#828282ff` | `#828282ff` | `#828282ff` |
| `fieldFocus` | `#0080ffff` | `#0080ffff` | `#0080ffff` |
| `objectHover` | `#ffffff80` (alpha 0.501961) | same | same |
| `objectSelect` | `#ff6400ff` | same | same |
| `modelBox` | `#ffc80040` (1, 0.784314, 0, 0.250980) | same | same |
| `objectLinesWidth` | 0.04 | 0.04 | 0.04 |

### 12.3 Slot pictures per skin [V]

| Slot | Light | Dark | Rainbow |
|---|---|---|---|
| `button` | `bg_button` | `bg_button_dark` | `bg_button_rainbow` |
| `content` | `bg_content` | `bg_content_dark` | `bg_content` |
| `label` | `bg_label` | `bg_label_dark` | `bg_label` |
| `tab` | `bg_tab` | `bg_tab_dark` | `bg_tab` |
| `menu` | `bg_menu` | `bg_menu_dark` | `bg_menu_rainbow` |
| `panel` | `bg_panel` | `bg_panel_dark` | `bg_panel` |
| `separator` | `bg_separator` | `bg_separator_dark` | `bg_separator` |
| `window` | `window` | `window_dark` | `window_rainbow` |

Skin names are matched exactly (case-sensitive): `Light`, `Dark`, `Rainbow`.

### 12.4 Applying a skin [V]

Applying a skin walks every picture and every text of the UI, including hidden
panels and the templates from which rows and menus are created later:

1. Frame pictures are reassigned by the start of their current picture name,
   tested in this order: `bg_button` -> button slot, `bg_content` -> content,
   `bg_label` -> label, `bg_tab` -> tab, `bg_menu` -> menu, `bg_panel` -> panel,
   `bg_separator` -> separator, `window` -> window. Because dark and rainbow
   names start with the same prefixes, any mix of variants authored in the
   scene is normalized to the chosen skin (for example the dark-looking groups
   of Display Settings render with the light `content` frame in Light).
2. Symbol glyphs (names starting with `symb_`): when the glyph is the
   background of a button, that button's rest colour becomes `text`;
   otherwise the glyph's colour becomes `text` (alpha included).
3. The panel menu marker picture (`tex_menu`): its button gets
   `functionsNormal`, `functionsHover`, `functionsPressed` as rest, hover and
   pressed colours (disabled colour unchanged).
4. Every text label takes the skin `text` colour, keeping its own alpha,
   except the "unskinned" labels: the four view labels, the tooltip text, the
   palette marker letters and the about-box footer. Consequence: hint texts that
   are authored grey (`#8c8c8c`) and dialog titles render in the plain text
   colour (`#363636` in Light); placeholders keep 50 % alpha (`#36363680` in
   Light, `#ffffff80` in Dark). [D]

Widgets read their state colours from the skin when they are created
(section 16 lists which tokens each widget uses). Code that changes colours
later (list rows, menu rows, disabled menu items, element names in error)
reads the active skin at that moment.

Pictures, colours and the 3D background (`#343434`) not listed in this
section are skin-independent: view frames and labels, tooltip, letterbox
bars, slider fill and handle (`#0080ff`), texture view background
(`#363636`), checkerboards, icons, cursors.

### 12.5 Selecting a skin [V]

- Setting key `appSkin` (string), default `Light`. Preferences offers a
  dropdown with `Light` and `Dark`. The choice is saved and applied when the
  dialog is confirmed with OK, and only if a skin of that name exists.
- At start-up the saved skin is applied once all widgets exist.
- `Rainbow` exists in the data but is not offered in the UI; it can only be
  reached by writing `Rainbow` into the setting.
- If the saved name is missing or unknown, the original applies no skin at
  start-up (the authored mix of light and dark frames stays visible). The port
  falls back to `Light` instead.
- Original quirk: widgets created before a runtime skin switch keep the
  previous skin's state colours until restart (only their pictures and texts
  change). The port re-resolves every colour from the active skin at draw
  time, so a switch is complete and immediate. Parity tests compare a freshly
  started session.

### 12.6 Derived effective colours [D]

Rest colours as drawn (sprite texel x base x state), useful for screenshot
tests:

| Element | Light | Dark |
|---|---|---|
| Menu bar background | `#ffffff` | `#2b2b2b` |
| Text | `#363636` | `#ffffff` |
| Button face (rest) | `#fbfbfb`, 1 px `#ffffff` inner line, 1 px `#d9d9d9` outer line | `#464646`, `#4f4f4f` inner, `#3b3b3b` outer |
| Button face (hover) | `#c2e3fb` face | `#384046` face |
| Button face (pressed, or toggle on) | `#a2c7e1` face | `#2a3a46` face |
| Tool strip background | `#f6f6f6` fill, `#ffffff` inner line, `#d5d5d5` outer line | `#222222` fill, `#292929` line |
| Group frame | `#ffffff` fill, `#d9d9d9` line | `#2b2b2b` fill, `#3b3b3b` line |
| Panel header band | `#d8d8d8` top line, gradient `#e6e6e6` to `#fefefe`, white body | `#4f4f4f` top line, gradient `#464646` to `#2b2b2b`, `#2b2b2b` body |
| Field frame (rest / hover or focus) | `#828282` / `#0080ff`, 1 px | same |
| Selection highlight in fields | `#0080ff` at 50 % | same |
| Slider track | `#f6f6f6`-ish frame at 94 % (`bg_panel` x `#f0f0f0f0`) | `#222222` frame x `#f0f0f0f0` |
| Display Settings slider fill and handle | `#0080ff`; handle `#0040ff` while hovered or dragged | same |

---

## 13. Frames, textures and 9-slicing

### 13.1 Drawing rules [V]

- A sliced picture is split into 9 parts by its borders (left, top, right,
  bottom in px); corners are drawn unscaled, edges stretched along one axis,
  the centre stretched on both (or skipped when drawn hollow). Borders map 1:1
  to canvas px. Apply the squash rule of section 0.4 when the rectangle is
  smaller than two opposing borders.
- Plain pictures are stretched to their rectangle. Tiled pictures repeat at
  their pixel size (times the parent scale).
- Every picture is multiplied by its base colour and its state colour.

### 13.2 Frame inventory [V]

Borders are listed as left, top, right, bottom. Pixel recipes describe the
original look so that look-alike art can be drawn; they are not to be copied
as files.

| Picture | Size | Borders | Light recipe |
|---|---|---|---|
| `bg_button` | 32 x 32 | 3, 3, 3, 3 | 1 px outer line `#d9d9d9`, 1 px inner line `#ffffff`, fill `#fbfbfb` |
| `bg_content` | 32 x 32 | 2, 2, 2, 2 | 1 px line `#d9d9d9`, fill `#ffffff` |
| `bg_label` | 4 x 4 | none (plain) | solid `#ffffff` |
| `bg_tab` | 32 x 28 | 2, 2, 2, 0 | 1 px line `#d9d9d9` on top, left and right, open bottom, fill `#ffffff` |
| `bg_menu` | 32 x 32 | 2, 24, 2, 2 | top line `#d8d8d8`; rows 1 .. 23 a vertical gradient `#e6e6e6` to `#fefefe` with side pixels about 14 levels darker; body `#ffffff` with `#f0f0f0` side pixels; bottom line `#f0f0f0` |
| `bg_panel` | 32 x 32 | 3, 3, 3, 3 | 1 px outer line `#d5d5d5`, 1 px inner line `#ffffff`, fill `#f6f6f6` |
| `bg_separator` | 24 x 2 | none (plain) | top row `#d5d5d5`, bottom row `#ffffff` |
| `bg_border` | 32 x 32 | 4, 4, 4, 4 | 1 px white outer line, transparent inside (tinted at runtime) |
| `bg_border_fill` | 32 x 32 | 2, 2, 2, 2 | 1 px white outer line, inside white at 15 % (`#ffffff26`) |
| `window` | 128 x 128 | 32, 56, 32, 64 | 16 px soft black shadow (alpha from about 0.01 at the outside to 0.33 next to the frame); 1 px outline `#a0a0a0`; header gradient `#e6e6e6` (just under the outline) to `#fefefe` 22 px lower; white body; 1 px line `#d7d7d7` 56 px above the picture bottom; footer band `#efefef` below it down to the bottom outline |
| `tex_menu` | 32 x 32 | none (plain) | white upper-right triangle (cut along the top-left to bottom-right diagonal) at about 70 % alpha, with three small round holes in a row near the top; tinted at runtime |

| Picture | Dark recipe |
|---|---|
| `bg_button_dark` | outer `#3b3b3b`, inner `#4f4f4f`, fill `#464646` |
| `bg_content_dark` | line `#3b3b3b`, fill `#2b2b2b` |
| `bg_label_dark` | solid `#2b2b2b` |
| `bg_tab_dark` | line `#3b3b3b`, fill `#2b2b2b` |
| `bg_menu_dark` | top line `#4f4f4f`; gradient `#464646` to `#2b2b2b` over rows 1 .. 19; body `#2b2b2b`; side pixels darkening to `#262626`; bottom line `#252525` |
| `bg_panel_dark` | line `#292929`, fill `#222222` |
| `bg_separator_dark` | rows `#292929`, `#222222` |
| `window_dark` | same shadow; outline `#4f4f4f` (top) to `#252525` (bottom); header gradient `#464646` to `#2b2b2b`; body `#2b2b2b`; line `#323232`; footer `#272727` |

| Picture | Rainbow recipe |
|---|---|
| `bg_button_rainbow` | borders 4 on all sides; a 2 px outer band of six colour segments running around the edge (red `#ff0000`, orange `#ff9900`, yellow `#ffff00`, green `#33ff00`, blue `#0099ff`, purple `#6633ff`); fill `#fbfbfb` |
| `bg_menu_rainbow` | borders 2, 18, 2, 2; horizontal colour bands from the top: red `#fa2e2e` (3 rows), orange `#faa92e` (3), yellow `#fbfb2f` (4), green `#59fc30` (3), blue `#31abfd` (3), purple `#835afd` to `#855cff` (rest) |
| `window_rainbow` | same shadow; header and footer made of red, orange, yellow, green, blue and purple bands |

### 13.3 Patterns and glyphs [V]

| Picture | Size | Content | Use |
|---|---|---|---|
| `bg_pattern_big` | 32 x 32 | 16 px white/black checker | texture view backdrop, tiled, tint `#ffffff10` |
| `bg_pattern_small` | 16 x 16 | 8 px white/black checker | texture preview backdrop, tiled, tint `#8c8c8c40` |
| `check` | 32 x 32 | white tick (glyph box x 3 .. 29, y 7 .. 25) | checkbox mark (drawn 14 x 14) |
| `symb_arrow_down` | 32 x 32 | white down chevron (box x 9 .. 23, y 12 .. 21) | dropdown arrows (drawn 24 x 24) |
| `symb_arrow` | 32 x 32 | white right chevron | submenu arrows (9 x 9) |
| `arrow_right` | 32 x 32 | white right-pointing triangle in the left half | tooltip arrow (16 x 16) |
| `disc` | 32 x 32 | white disc, 16 px, centred | colour swatch, radio dot |
| `circle` | 32 x 32 | white ring | UV origin marker |
| `cross` (32 px variant), `cross_small` | 32 x 32 | white X (small: 14 px) | `cross`: UV rotation marker, registration marks; `cross_small`: file-browser filter clear button, decorative cross under the colour-field swatch (8.9) |
| `symb_close` | 64 x 64 | white disc with an X cut out | dialog close |

Cursors (32 x 32, white with black outline, hot spot at the centre):
horizontal resize (side grips), diagonal resize (dialog grips), plus vertical
and the other diagonal (file browser). Tool cursor symbols: section 10.2.

---

## 14. Fonts and text [V]

- One family: Open Sans Regular. Bold and italic are synthesized from the
  regular face. The port may ship Open Sans Bold and Italic for better
  quality; parity tests then accept the small difference.
- Sizes: 11 px for all regular UI text; 12 px tooltip; 13 px bold for the `+`
  of the texture list; 8 px for the palette marker letters. Dialogs also use
  9 px (dialogs spec).
- Styles: bold for the view labels and the texture label of the material
  header; italic for placeholders and the Element Settings disabled overlay.
- Colours: section 12.4 (skin text colour for almost everything; placeholders
  at 50 % alpha).
- Line height at 11 px: 15 px (single-line labels are 15 px tall; a two-line
  hint is 30 px). With top alignment the first baseline sits about 12 px below
  the box top. [U] (exact baseline)
- Alignment: each label has a box and one of nine alignments (top, middle,
  bottom x left, centre, right).
- Overflow rule for labels (almost all of them): text wraps at word boundaries
  to the box width, and whole lines that do not fit the box height are not
  drawn (no partial lines, no ellipsis). A label too narrow for its text thus
  shows only the words of the first line. Exceptions: the two UV Snap hints may
  overflow their box downward; a few numeric labels in dialogs never wrap.
- Single-line text fields never wrap: when the text is longer than the field's
  text box, only the part that fits is drawn and it scrolls horizontally to
  keep the caret visible.
- Rich text is off for field contents and placeholders (markup characters
  appear literally).
- Text fields: caret 1 px wide, coloured like the field's text, blinking with
  a period of 1 / 0.85 s (visible for the first half of each period).

---

## 15. Icon inventory and art policy

### 15.1 Policy

- cuboid ships its own original look-alike icons and frames with the same
  sizes, borders, roles and colour logic as listed here; it never ships
  pictures taken from Cubik Studio.
- Optional skin pack: a user may point cuboid to their own Cubik Studio
  installation; cuboid then extracts the pictures locally, keyed by the
  original picture names used in this spec, and uses them instead of the
  built-in art. Nothing extracted is redistributed. Brand marks (BDcraft, Cubik
  and Sketchfab logos, operating-system logos) are never used by default.
- Style of the original set: flat, mostly two-tone icons on a 48 px canvas in
  saturated material-style colours (blue `#2196f3`, indigo `#3f51b5`, cyan
  `#00bcd4`, green `#4caf50`, light green `#8bc34a`, red `#f44336`, amber
  `#ffc107`, orange `#ff9800`, deep orange `#ff5722`, pink `#e91e63`, blue grey
  `#607d8b`, with tints `#bbdefb` and `#90caf9`); shaded grey 3D renders for the
  primitives; white silhouettes for symbols that are tinted at runtime.

### 15.2 Inventory by role [V]

Source size / drawn size in px.

| Role | Pictures | Size | Drawn |
|---|---|---|---|
| Create element | `primitive_cube`, `element_planex`, `element_planey`, `element_planez` (planes are light grey and tinted per axis: X `#ffa0a0`, Y `#b4b4ff`, Z `#96ff96`) | 96 | 38 |
| Create primitive | `primitive_cube`, `_sphere`, `_cylinder`, `_cone`, `_pyramid`, `_slope`, `_cornerin`, `_cornerout`, `_torus` (shaded grey renders) | 96 | 38 |
| Model | `cog` (32), `origin`, `blueprint` | 48 | 24 |
| Edit modes | `cube`, `voxel`, `sphere`, `draw_paint`, `eye` | 48 | 24 |
| Selection | `lightbulb_off`, `lock`, `lock_go`, `anchor`, `cross` (the 48 px icon variant; a white 32 px glyph of the same name also exists, and the look of the 48 px variant is [U] because it was not extracted) | 48 | 24 |
| Recover | `lightbulb`, `lock_open` (32), `blueprint`, `camera_go`, `recover_elements` | 48 | 24 |
| Element tools | `shape_square_wrench`, `cube_faces`, `cube_uv`, `color_wheel_go`, `color_wheel`, `cube_uv_apply`, `lightning_delete`, `lightning_hide`, `sort` | 48 | 24 |
| Voxel tools | `voxel_alone`, `voxel_line`, `voxel_cuboid`, `voxel_sphere`, `voxel_cube`, `voxel_laser`, `voxel_extrusion`, `voxel_shell`, `voxel_select` | 48 | 24 |
| Mesh tools | `import`, `resize`, `proportions`, `color_wheel_go`, `color_wheel` | 48 | 24 |
| Paint tools | `draw_pixel`, `draw_line`, `draw_rectangle`, `draw_brush`, `draw_paint`, `draw_fill`, `draw_sprayer`, `draw_magic`, `draw_fillnoise` | 48 | 24 |
| Color tools | `color_palette`, `color_picker`, `replace` | 48 | 24 |
| Gizmos | `bounding_box`, `bounding_box_handles` | 48 | 24 |
| UV tools | `arrow_out`, `swap`, `page_copy`, `page_paste`, `shape_flip_horizontal`, `shape_flip_vertical`, `shape_rotate_clockwise`, `shape_rotate_anticlockwise`, `draw_paint` | 48 | 16 or 20 |
| List rows | `lightbulb_off`, `lock`, element and primitive icons above, `symb_file_3d` | 48-96 | 16 |
| Menu rows | for example `page_white`, `folder_open`, `folder_merge`, `arrow_undo`, `arrow_redo`, `cut`, `icon_rotateZ`, `eye`, `magnifier_zoom_in`, `magifier_zoom_out` (sic), `move_managers_left`, `move_managers_right`, `information`, `key`, `comments`, `textfield_rename`, `bullet_add`, `bullet_check`, `bullet_uncheck`, `bullet_delete`, `bullet_go`, `bullet_back`, `bigger`, `smaller`, `image`, `image_link`, `image_save`, `folder_open_image`, `export`; full list in the menus spec | 48 | 16 |
| Message box | `sign-check`, `sign-question`, `sign-warning`, `sign-error`, `sign-info`, `life-buoy`, `tools`, `light-bulb`, `key` | 96 | 48 |
| Symbols (white, tinted) | `symb_arrow`, `symb_arrow_down`, `symb_close`, `check`, `disc`, `circle`, `cross`, `cross_small`, `arrow_right`, file-browser glyphs (`symb_folder`, `symb_file`, `symb_drive`, ...) | 32-64 | 9-24 |
| Cursors | `bullet`, `bullet+`, `bulletmove`, `bulletscale`, `bulletsquare`; resize cursors | 32 | 16 / native |
| Frames | section 13 | | |

Pictures that exist but are unused in the main window (for example
`clone`, `crop`, `grid`, `select_all`, `save`, `save_as`, `search`, `link`,
`voxel_ok`, `voxel_delete`, `icon_rotateX`, `icon_rotateY2`) need no
counterpart unless another spec uses them.

---

## 16. Widget catalogue

### 16.1 Shared behaviour [V]

- Colour model: section 0.5. Unless stated, the state fade is 0.1 s and the
  multiplier 1.
- Focus: text fields, dialog buttons and the checkboxes of Element Settings
  and of dialogs take keyboard focus when clicked and then keep showing their
  hover colour until another control takes focus (a clicked checkbox frame
  stays blue). Tool buttons, panel buttons, menu and list rows, tabs, sliders,
  toggle buttons and the Display Settings checkboxes never take focus, so they
  never stay highlighted after a click.
- Hover on any control with a tooltip writes the tooltip to the status bar and
  shows the classic tooltip (section 10.1); leaving hides the tooltip (the
  status text stays until it times out).
- Right click on a push button is reported separately and does nothing in the
  main window; only the view labels use right click.

### 16.2 Push button [V]

- Target: its frame (usually sliced `button`; some use `panel`), base colour
  white. States: `buttonNormal` (or fully transparent for "transparent"
  buttons such as menu bar buttons), `buttonHover`, `buttonPressed`,
  `buttonDisabled`. All labels inside take the skin text colour.
- Disabled: the disabled tint on the frame and the button's icon drawn in
  greyscale (the icon is the first picture inside the button).
- Fires on release inside (left button only). Inside a menu panel a click
  closes the whole chain of open menus.

### 16.3 Toggle button [V]

- Like a push button (same tokens), plus: base colour = `buttonPressed` while
  on and the rest colour while off, set instantly when the value changes; the
  state tint multiplies on top (so an "on" button hovered is darker still).
- In an exclusive group: switching one on switches the others off; groups that
  allow "all off" let a click on the active button switch it off.
- Disabled: disabled tint and greyscale icon.

### 16.4 Checkbox [V]

```
 +--+  Label text          box 14 x 14 at (3, 3); label from x = 23
 |v |                      row height 20 (min) or as laid out
 +--+
```

- Box: `bg_border` 1 px frame, 14 x 14 at (3, 3) from the row's top-left; state
  colours `fieldNormal`, `fieldFocus` (hover and pressed), `buttonDisabled`.
- Mark: `check` glyph filling the box, colour `fieldNormal`, switching to
  `fieldFocus` while the pointer is over the row; it fades in or out over 0.1 s
  when the value changes.
- Label: box from x = 23 to `w - 5`, 2 px from the top and bottom, 11 px,
  left/top aligned, skin text colour. The whole row is clickable.

### 16.5 Tab [V]

- Sliced `tab` frame; state colours `buttonNormal`, `buttonHover`,
  `buttonPressed`, `buttonDisabled`; base colour `buttonNormal` when active,
  `buttonDisabled` when inactive.
- Size change: free-placed tabs (Element Settings) are 25 px tall when active
  and 21 px tall with the bottom raised 1 px when inactive; tabs in a tab row
  (texture face tabs, Display Settings) are 25 px when active and 21 px when
  inactive, bottom-aligned, the active one rising 4 px above its row and its
  row being drawn on top (and, for the two-row strip, moved to the lower row).
- Selecting a tab shows its page and hides the others; tab groups never allow
  "none".

### 16.6 Text field [V]

- Frame: `bg_border` (1 px), sliced; state colours `fieldNormal` (rest),
  `fieldFocus` (hover, pressed, focused), `buttonDisabled`; the inside is
  transparent (the parent background shows through).
- Text box: inset 8 left and right; top inset 4 (24 px fields), 5 (23 px
  fields) or 6 (Display Settings fields); 11 px, left/top aligned (fields in
  the 32 x 32 tool cells: centred). Placeholder: italic, text colour at 50 %
  alpha, shown only when the field is empty.
- Content kinds:

| Kind | Accepts |
|---|---|
| text | anything, single line, optional length limit |
| decimal | digits, one decimal point, a leading minus |
| integer | digits and a leading minus |

- Focus: clicking an unfocused field focuses it and selects all of its text;
  clicking a focused field places the caret. Double-click selects the word
  under the caret, words being separated by space, comma, full stop, colon,
  semicolon and line breaks. Multi-line fields (dialogs only) move the caret to
  the start on focus instead; the original also appends blank lines to their
  text on focus, which the port does not reproduce (dialogs spec). [U]
- Selection highlight: `fieldFocus` at 50 % alpha.
- Values apply live: consumers react to every change of the text (each typed
  character, each paste, each wheel step), not only to Enter or loss of focus.
  Text that does not parse as a number reads as 0.
- Mouse wheel over the field (no focus needed): decimal fields add or subtract
  1 per wheel step (up = increase), integer fields 1, multi-line text fields
  move the caret one line up or down; other fields ignore the wheel. There is
  no clamping; a linked slider clamps its own value.
- Tab moves the focus to the next control to the right (or, if none, below);
  Shift+Tab to the left (or above). Order follows the visual reading order.
  Moving focus into a field selects its text.
- Numbers are printed and parsed with the invariant culture on every system:
  `.` as the decimal separator and no digit grouping (the original switches to
  this culture at start-up; model spec 1.6).
- No drag-to-scrub: the original has no drag gesture on numeric fields or
  their labels. cuboid may offer scrubbing as an opt-in setting, off by
  default, so the default behaviour matches the original.

### 16.7 Slider [V]

```
 X: [========|===============]  [ 0    ]
     track 6 px, fill from the left, handle 8 x 16
```

- Box (for example 230 x 20); track: sliced `panel` frame tinted `#f0f0f0f0`,
  full width, 6 px tall, vertically centred; fill: solid `#0080ff` from the
  track's left end to the handle centre; handle: solid `#0080ff`, 8 x 16,
  centred on the value position; the handle centre travels over the full box
  width (it overhangs the track ends by 4 px).
- State colours on the handle of the Display Settings sliders (the only
  skinned sliders, 75 in all): rest white (handle shows `#0080ff`), hover and
  pressed `fieldFocus` (handle shows `#0040ff`), disabled `buttonDisabled`.
  The sliders inside dialogs (Grid size, Voxelization, Color Picker) are plain
  sliders with fixed, unskinned state colours: rest `#ffffff`, hover
  `#f5f5f5`, pressed `#c8c8c8`, disabled `#c8c8c880`; the progress bar and the
  Color Picker's saturation/value box have no state colours. Their geometry
  and linking are in the dialogs spec; the other bullets of this section are
  about the Display Settings sliders.
- Click or drag anywhere on the box moves the value; whole-number sliders snap
  to integers.
- Linked field: a slider and the numeric field in the same row stay in sync.
  Editing the field moves the slider without notifying anyone (empty text is
  ignored); moving the slider writes the value into the field, which then
  notifies its consumers. When a slider is shown again it re-reads the field
  one frame later.

### 16.8 Dropdown button and field with list [V]

- Dropdown button (culling, presets, preferences): a field-style button over
  the full width (`bg_border_fill`, field colours) with the current value
  (11 px, inset 8 left, vertically centred or 5 px from the top) and a down
  chevron (`symb_arrow_down` 24 x 24, skin text colour) at the right end.
  Clicking opens a dropdown panel (section 3.2) directly below the button with
  the button's width; choosing an item replaces the shown value and notifies
  the owner. Setting the value from code does not notify.
- Field with list (texture field): a text field with a 20 px transparent list
  button at its right end (chevron 24 x 24 centred); the list button opens the
  dropdown below the field; choosing an item writes it into the field and
  notifies.

### 16.9 Field-style button [V]

`bg_border_fill` frame (1 px frame over 15 % white); state colours
`fieldNormal` (or fully transparent when "transparent"), `fieldFocus`,
`fieldFocus`, `buttonDisabled`. Icon and text positions per use (sections 9.1,
16.8).

### 16.10 Scroll views and scrollbars [V]

- Lists: vertical scrolling only; clamped (no overscroll); no inertia; 28 px
  per wheel step; dragging the content with the mouse is allowed.
- Texture view: both directions; clamped; wheel zooms instead of scrolling;
  panning only with the middle mouse button (or the pan key); dragging with
  the left button does not move the content.
- Scrollbar: 8 px (lists) or 10 px (texture view) wide; the track is
  invisible; the handle is a sliced `button` frame spanning the bar width, with
  a minimum length of 20 px and state colours rest white, hover `#f5f5f5`,
  pressed `#c8c8c8`, disabled `#c8c8c880` (not skinned). List scrollbars are
  always visible.

### 16.11 Text link (view labels) [V]

Text whose own colour is the state target: white text x state colour
(`#ffffff50` rest, `#ffffffff` hover, `#ffffffa0` pressed, `#d2d2d250`
disabled), fade 0.1 s. Left click and right click are separate actions
(section 5.2).

### 16.12 Grips [V]

Side grips: section 2.3. Dialog grips: section 11. Both change the cursor on
hover and keep the resize cursor while dragging.

### 16.13 Status hints and tooltips

Section 4 (Info cell) and section 10.1 (classic tooltip).

---

## 17. Parity test checklist

At UI scale 1, Light skin, fresh start, compare against reference captures of
the original:

1. 1000 x 700 first launch: menu bar buttons x positions (1, 49, 97, 161, 225,
   273, 321, 385); left strip panels at y 22 .. 234 and 234 .. 414; right
   strip panels at 22, 220, 418; 4-view zone `[64, 888)`; status cell widths
   (Info 474.36 at W = 1000: `4 + 476 * 500/506`; Details 220.94).
2. Element mode at 1280 x 720: managers zone `[1060, 1280)`, panel heights of
   section 8.1; open Element Settings: helpers zone `[0, 350)`, tab geometry
   of 9.1.
3. Drag the right grip to 180 and 300; check the tool grid columns (5 and 8)
   and the panel heights.
4. Hover each control with a tooltip: tooltip size and placement (10.1).
5. Open each main menu: panel width = min width + 8, row pitch 22,
   separators 2 px.
6. Switch to Dark, restart, repeat 1 and 2 against Dark captures.
7. Maximize each view by clicking its label; label moves to (16, 8).
8. Palette (8.10): with the HSL preset, left-click each of the five black
   cells; `L` always lands on (11, 17). Load a 40 x 10 picture; the palette
   becomes 16 x 10 and `palette.png` is rewritten. Type `F80` into the left
   field; the colour is `#F80000FF` and the text stays `F80`.
9. Status bar (section 4): hover each face of an unrotated cube and check
   `Down` .. `East`; rotate an element to a stored x of 337.5 and check
   `x: 22.5°`; with nothing selected check the element and face summary.

---

## 18. Unverified / open questions

1. Text rasterization: exact baselines and glyph advances of Open Sans at
   11 px in the original renderer; line height 15 px is inferred from layout
   sizes. Needs side-by-side captures.
2. Sub-pixel rendering: the original does not snap; fractional edges (Recover
   panel content at 24.5, list button widths, quadrant halves) may look
   blurred in the original. The rounding rule of section 0.2 is a decision,
   not an observation.
3. View quadrant frames are drawn at half scale (0.5 px at 50 % black); the
   1 px at 25 % approximation needs a capture.
4. Saved side widths are read and written under keys that differ in case;
   whether the original ever restores a dragged width after restart was not
   observed.
5. UI scale on Windows depends on the dpi the original reports; behaviour on
   125 % and 175 % displays (scale stays 1.0 and 1.5) was not observed.
6. Runtime skin switch: the original's partial update (old state colours on
   existing widgets) is deduced from the creation-time colour reads; its
   visibility was not observed.
7. Hint texts and dialog titles authored grey are recoloured to the skin text
   colour by the skin pass; deduced, not observed.
8. Palette and colour fields (8.9, 8.10): the engine's float-to-byte colour
   conversion (truncation or rounding; 8-bit values round-trip exactly either
   way, so only non-8-bit colours from elsewhere are affected); what a readable
   but undecodable picture leaves as palette (1 x 1 or a small placeholder,
   and its texels); whether the resampling of large pictures alters
   semi-transparent texels or the RGB of transparent ones, and keeps the
   orientation; the runtime effect of an unreadable `palette.png` at start-up;
   whether the field's own upper-casing re-enters its change handler (the
   result is the same either way); the out-of-picture reads at the palette
   edges (wrap or clamp per picture) at runtime; how the port sources the
   preset colour layouts.
9. Which Display Settings tab is active when the helper first opens (no tab is
   active in the source scene).
10. Some submenus (voxel convert, voxel transform, voxel blueprint, material
    new-textures) are attached to the top-left of their row instead of the
    top-right in the source data; whether they really open over the parent row
    needs a capture (menus spec).
11. The tooltip line-count estimate versus the real wrap of long tooltips.
12. Whether the menu bar button of a disabled menu changes appearance.
13. Very tall windows: growth of the create panels and the downward shift of
    their buttons (section 6.1) is derived from the layout rules, not observed.
14. Negative 4-view zone width when both side zones are wide on a small window.
15. Element list names: the original only resets the name colour for some
    model formats, so a recycled row may keep a red name; treat as a bug and
    always set the colour.
16. Wheel step size per notch (affects list scrolling at 28 px per step and
    texture view zoom) depends on the platform's wheel delta.
17. Status bar cells (section 4): float text edge cases (exponent form, `-0`);
    whether the model's parent nodes are unrotated; when the quarter-turn bake
    of elements happens relative to the cell refresh; the stale-cell paths of
    4.2 and the Face cell under a rotated element with a still pointer, at
    runtime; the hover target staying on an object after the pointer leaves
    a view; which callers make element creation select the new element;
    repeated mesh ids; the layer of hidden objects; the exact width at which a
    value collapses to its label.
18. The provenance and licence of the original icon style (resembles a
    recoloured public icon set); cuboid draws its own icons regardless.
