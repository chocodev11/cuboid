# UI v2 specification

Status: approved direction 2026-10-24 (owner). It replaces the ad-hoc look of the first UI pass and refines
`roadmap_lean.md` section 6. Sizes are in egui points at 100 % UI scale.

## 1. Principles

- One system. Every screen is built from ui-kit components. Code in `cuboid-ui` never sets a font size, a colour, a
  margin, a radius or a spacing literal; `cargo xtask ui-style` enforces it (12).
- Quiet chrome, loud content. Surfaces are neutral; the viewport and the user's model carry the colour. The accent
  marks only the one primary action of a view, the current selection, the active tool and progress.
- Alignment. Everything sits on a 4 pt grid; label and value columns line up in every property list.
- Density of Blender, clarity of Blockbench: 24 pt controls, compact panels, readable labels, sentence case.
- Own identity: no Blockbench or Blender theme, art or icon is copied (roadmap_lean 6).

## 2. Typography

Font: Inter (SIL OFL 1.1) in Regular 400, Medium 500 and SemiBold 600, embedded; the system UI font is the fallback
for scripts Inter lacks. Numbers use the same family.

| Role | Size | Weight | Colour token | Use |
|---|---|---|---|---|
| caption | 11 | 400 | text_weak | metadata, hints, badges, shortcut chips |
| body | 12 | 400 | text | default UI text, values, list rows |
| label | 12 | 400 | text_weak | property labels |
| strong | 12 | 500 | text_strong | emphasised values, selected row titles |
| title | 13 | 600 | text_strong | panel headers, section titles, card titles |
| heading | 16 | 600 | text_strong | dialog titles, empty-state headlines |
| display | 20 | 600 | text_strong | the start screen headline only |

No other sizes exist. Line height is 1.35 times the size. Sentence case everywhere; no all caps.

## 3. Spacing and sizes

| Token | Value |
|---|---|
| space | 2, 4, 8, 12, 16, 24, 32 |
| control height | 24 (fields, buttons, rows, tabs), 28 (panel headers, toolbars), 32 (primary actions in dialogs and the start screen) |
| list row | 28; 44 with a thumbnail |
| icon | 16 in controls, 20 in toolbars, 40 in empty states |
| radius | 3 controls, 4 cards and panels, 6 modals |
| gutter | 8 at window edges and inside panels; 12 between sections; 24 between page groups |

## 4. Surfaces and borders

- Levels from back to front: app background, region (panels), content (cards, list areas), control fill, hover,
  pressed. Each step is a surface token of the active theme family.
- Borders are separators between regions and focus rings only. Cards and inputs have no outline; inputs are
  filled (control fill) and show a 1 pt focus ring.
- Contrast, by the WCAG 2.1 formula, in every theme: body text 4.5:1 against its surface; the parts that show
  a control's state (checkbox box, toggle track and knob, slider track and handle) 3:1 against the surface
  they sit on (WCAG 1.4.11), drawn as a 1 pt edge where the fill alone is too faint; field fill 1.3:1 against
  its surface. A theme unit test asserts these ratios on the token pairs.
- Modals: 40 % backdrop, content surface, one strong border, radius 6.

## 5. Colour roles

- Accent: the primary button (at most one per view), the selection tint, the active tool, the active tab indicator
  (2 pt underline), progress. Segmented controls, toggles of settings and chips use the neutral selected state
  (pressed surface plus strong text), never the accent.
- Status colours (danger, warning, success) only for status: validator badges, errors, toasts.
- Axis colours (X red, Y green, Z blue, fitted per theme) only for axis labels and the gizmo.

## 6. Components (ui-kit)

| Component | Anatomy |
|---|---|
| PanelFrame | header 28 (icon 16, title, right-aligned icon buttons, optional collapse chevron), body padding 8, scroll area |
| Section | title row (title style, chevron when collapsible) and content; 12 between sections |
| PropertyGrid | rows of 24; label column 96 (min 72, max 128, label style, left aligned, ellipsis), value column fills |
| Fields | DragValue (number, unit suffix, drag and type), TextField, Dropdown, Checkbox, Toggle, Segmented (neutral selected), ColorSwatch, Slider, VectorField (three DragValues with axis captions) |
| Buttons | Primary (accent fill, on_accent text), Secondary (control fill), Ghost (transparent, hover fill), Danger; heights 24 or 32 |
| IconButton | 24 square (28 in toolbars), tooltip with shortcut chip |
| ListRow | 28 (44 with thumbnail): leading icon or thumbnail, title, trailing caption; hover and selected states; context menu |
| Tree | ListRow with 12 indent per level, chevron, hover-revealed visibility and lock buttons |
| Tabs | 28 high, title style, active tab with a 2 pt accent underline |
| Toolbar | horizontal or vertical strip of IconButtons with group separators |
| Card | thumbnail 4:3 with the content fitted, title, caption, optional badge; hover lifts one surface step |
| EmptyState | 40 icon, heading, one body line, one or two buttons |
| Modal | heading, body, footer with buttons on the right (primary last); Esc cancels, Enter confirms |
| Toast | bottom right, width 320, stack of at most 3, caption plus optional action |
| Badge | caption on the pressed surface, radius 3, padding 2 by 6 |

## 7. Icons

Phosphor Regular (MIT) as an icon font, mapped once in an `Icon` enum. Colour: text_weak; active: text_strong or
the accent for the active tool. The painter-drawn icons are removed, except the app mark and the view gizmo.

## 8. Editor layout

- Top bar 32: menus (File, Edit, View, Add, Tools, Help), mode tabs (Model, Voxel, Paint, Display), and on the right
  the preset indicator and the command search.
- Left: a vertical tool strip of 36 per mode, then the left area (240, resizable 180 to 400) with the UV editor
  and textures stacked; library and history are further tabs there.
- Centre: viewports; each has a 28 header inside its top edge (view name, shading menu, camera preset), as in
  Blender's viewport header.
- Right: the right area (300 of content plus the 36 icon strip, resizable): the outliner on top (a resizable
  split, 40 % of the height by default) and below it the properties with a vertical icon tab strip by context:
  Element, Faces and UV, Texture, Model, Display, Validator; content uses PropertyGrid. The icon strip sits on
  the side that faces the viewport, so it never stands next to the tool strip. Cubik Studio, Blockbench and Blender all keep the outliner on
  the right, so cuboid does too.
- Bottom: status bar 24: viewport hints only while a viewport is hovered, job progress in the middle, counts and
  version on the right.
- Every panel can move between the left and right areas.
- Each mode has a default arrangement (Paint: UV editor and textures in front; Voxel: palette in front).

### 8.1 Familiarity presets

The first run asks which app the user knows. The answer sets three things at once, each changeable later in
Preferences; the look and the theme never change with it.

| Coming from | Navigation | Keymap | Arrangement |
|---|---|---|---|
| Blockbench (preselected) | left drag orbits, right drag pans | Blockbench-style | as in 8 |
| Cubik Studio | middle drag (or Space held) pans, Alt or Ctrl with it orbits (app_behavior 5.5) | Cubik's menu shortcuts from the `cuboid-actions` tables | properties in the left area, outliner in the right area, as in Cubik |
| Blender | middle drag orbits, Shift + middle drag pans | Blender-style | as in 8 |

The command palette matches cuboid's names, Cubik's original menu text (kept in the action registry) and a
curated table of common Blockbench and Blender terms (for example cube, bone, Elements list, Element
Settings). A row found through another app's term shows that term as its caption.

## 9. Start screen

As approved in the v2 mockup (`mockups/start_screen_v2.html`; its Vietnamese text and colours are illustrative,
the sizes of this file win): sidebar 240 (Open as the one primary button, New as a list of rows including Cubik
classic; the voxel size is asked when Voxel is chosen; library counts and Import at the bottom), main area with the
recent grid (cards with fitted thumbnails, format badge, relative time with correct plurals, pin on hover, filter)
and actionable suggestions, right column 260 with a read-only project brief (Edit opens a dialog; the Minecraft
version is chosen from the version table). No hero header and no settings strip; the status bar shows "Ready" and
the version. Empty state: an EmptyState with New block, Open file and Import a folder into the library. First run:
a Modal over the start screen that asks "Which app are you used to?" with three choice cards (Blockbench, Cubik
Studio, Blender, each with a small drawing of its orbit and pan gestures) that set the familiarity preset of
8.1, and named accent swatches; nothing moves behind it.

## 10. Dialogs

Widths 360, 520 or 720; padding 16; heading title; body with PropertyGrid where fields are involved; actions on
the right. Dialogs never resize while open.

## 11. Responsive rules

At 100 % scale: 1280 and wider shows all three start-screen columns and both editor side areas; from 1024 to 1279
the right column folds into the main area (start) or the properties area narrows to its icon strip plus 260
(editor); below 1024 the sidebar shows icons only. UI scale multiplies every size.

## 12. Enforcement and review

- `cargo xtask ui-style` fails when files under crates/cuboid-ui/src use `FontId::`, `RichText::size`, colour
  constructors, numeric `add_space`, `Margin::` or `CornerRadius::` literals, or `Color32` constants; the kit
  exposes tokens and components for all of them.
- A screenshot set (ignored test) renders every screen and state: start (data, empty, first run), editor in each
  mode with a selection, every dialog; at 1280 x 800 and 1920 x 1080; Mocha and Latte; 100 % and 150 %. Opus
  reviews the set against this spec after each UI package; fixes loop until it passes.

## 13. Work order

1. ui-kit v2: Inter and Phosphor assets, typography roles, tokens, every component of 6, the lint, a component
   gallery screenshot.
2. Start screen and editor shell (top bar, regions, status bar) on the components.
3. Panels: outliner, properties (all tabs), UV editor, textures, library, validator, history.
4. Dialogs: project settings, preferences, export, PNG render, brief edit, recovery, unsaved changes.
5. Screenshot review after 2 and after 4.
