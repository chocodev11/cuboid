# Layouts per mode: panels and properties tabs follow the mode

Status: 2026-10-27, owner request ("the Animate layout, and the UI in general, is more cluttered than Blockbench"),
then the owner's choice: filter the panels and the Properties tabs by mode, keep the layout still within each mode,
one New animation button, show the transform when an element is selected, fix the covered tab. Amends
`ui_v2_spec.md` 6 (tab rows), 8 and 8.1. Comes after the UV editor v2 package (`uv_editor_v2.md`), which changes
the same files.

## 1. Why

Every mode shows every panel and all seven Properties tabs today: the UV editor stays in Animate, the Animate and
Display tabs stay in Model, and a change of mode only brings a few tabs forward. Blockbench ties each panel to the
modes it serves; Blender shows the properties tabs that suit the active object. cuboid took Blender's tab strip and
Blockbench's areas without either filter, so everything is on screen all the time. In Animate with no animation,
three places offer the same New animation button: the Animations panel, the timeline and the Animate tab.

Not taken from Blockbench: its panels also come and go with the selection (the transform panel and the UV editor
show only while something is selected), so the panels below them move. cuboid keeps a panel in place for the
whole mode and changes only what is inside it.

## 2. Rules

1. What is on screen depends on the mode and the user's layout, never on the selection or the document. A panel
   with nothing to show keeps its place and shows a quiet empty state.
2. A change of mode is the one moment the layout changes: the panels of other modes leave, the panels of the mode
   come back where the user left them.
3. A mode never rewrites the stored layout: the filter is applied when the areas are drawn. Moving, resizing and
   splitting stay one layout for all modes, as today. The front tab of a group is the one thing a change of mode
   may write (section 3).

## 3. Panels per mode

| Panel | Model | Voxel | Paint | Display | Animate |
|---|---|---|---|---|---|
| Outliner | yes | yes | yes | - | yes |
| Properties | yes | yes | yes | yes | yes |
| UV editor | yes | - | yes | - | - |
| Textures | yes | - | yes | - | - |
| Palette | - | yes | yes | - | - |
| Library | yes | yes | yes | - | - |
| History | yes | yes | yes | - | - |
| Animations | - | - | - | - | yes |

- `PanelId::serves(mode)` holds the table as one match.
- A group shows the panels of its list that serve the mode, in their order. Its shown tab is its active panel
  when that one serves the mode, else the first that does. A click makes the clicked panel the active one of the
  group (stored), as today.
- A group with no panel for the mode is not drawn, and the other group of the area takes the whole height; the
  stored split is kept for when both show again.
- An area with no group left is not drawn and the viewports take its width; its stored width is kept.
- A panel that does not serve the mode is never shown: `Layout::is_shown` and what it feeds (the live check of
  the validator) take the mode. View > Panels and the command palette list such a panel disabled, with the tip
  "Not used in <mode>".
- The bring-forward rules of `Ui::set_mode` stay (Properties; the UV editor in Paint; the palette in Voxel and
  Paint; Animations in Animate). With the filter they only choose among the panels of the mode, and a group that
  already shows the panel in the mode is left alone: the tab the user chose there is the one the other modes read.
  The preferences are marked changed only when a tab moved. A choice that a rule does replace (History in Paint,
  where the palette comes forward) is not kept per mode, see section 11.

The default arrangements (`Layout::arranged`) do not change. With the filter, the Blockbench arrangement shows in
Animate the Animations panel alone in the left area, the Properties above the Outliner in the right area and the
timeline below the views; in Display, the Properties alone in the right area, and the left area, with nothing to
show, gives its width to the views.

The Animate tab stays in the Properties and does not become a panel of the left area: the Properties stay where
the user put them in every mode, and only their tab changes.

## 4. Properties tabs per mode

| Mode | Tabs, in strip order | Tab on entering the mode |
|---|---|---|
| Model | Element, Faces and UV, Texture, Model, Validator | Element |
| Voxel | Element, Model, Validator | Model |
| Paint | Texture, Validator | Texture |
| Display | Display, Validator | Display |
| Animate | Animate, Validator | Animate |

- `PropsTab::of_mode(mode)` gives the tabs; `PropsTab::for_mode` stays the tab on entering the mode.
- The strip shows only these tabs and never changes within a mode. The current tab is always one of them.
- The validator is in every mode, so its badge stays in sight and `Ui::show_validator` works everywhere.

## 5. The selection shows the transform

In Model and Voxel, when the primary selection becomes an object that was not the primary in the frame before (a
click, a box, the outliner, a new object, an undo), and the shown tab is Texture or Model, the tab becomes Element
(the object and its transform). Element, Faces and UV and Validator stay as they are, so an issue clicked in the
validator selects its object and keeps the list. A cleared selection changes nothing. The UI keeps the primary it
saw last to tell a new one.

## 6. One New animation button

- The Animations panel keeps its empty state with the New animation button. It is the only one.
- The timeline and the Animate tab without a current animation show one line in the caption style instead of an
  empty state, without a button: "No animation. Add one in the Animations panel." when the model has none,
  "Choose an animation in the Animations panel." when it has some. The timeline keeps its height.

## 7. Tab rows that do not fit

`group_tabs` (`ui/dock.rs`) fits the tabs into the room left of the header actions in three steps:

1. Every title fits: titles.
2. Else the other tabs as icons and the shown tab with its title, when that fits.
3. Else the shown tab's title is cut with an ellipsis to the room that is left, its tooltip and accessible name
   being the whole title; where not even an icon would fit, the shown tab is an icon too.

The tab row is clipped at the header actions, so nothing is drawn under the move button. The kit gets a text tab
with a largest width that cuts its title (egui's text truncation), beside `tab_button`.

## 8. Cost

The filter is a match per panel and frame. Nothing new is allocated per frame: the shown tabs of a group come from
an iterator over its list.

## 9. Tests and gates

- Layout: the table; a group shows its active panel when it serves the mode, else the first that does; a click
  stores the clicked panel; a group with no panel for the mode is not drawn and the other takes the height; an area
  with nothing to show is not drawn; drawing in every mode leaves the stored layout unchanged (prefs JSON before
  and after).
- UI (headless egui): Animate in the default arrangement shows Animations, Properties (Animate tab) and the
  Outliner, and no UV editor; Display shows the Properties alone; Model shows no Animations panel; the strip of
  each mode lists exactly the tabs of the table; View > Panels disables the panels of other modes; selecting an
  element on the Model or Texture tab shows Element, while Faces and UV and Validator stay; clearing the selection
  changes nothing; in Model with History in front, Animate and back to Model leave History in front, and a
  round trip through the modes moves no tab but the palette's; the Show actions of a panel the mode does not use
  are disabled in the command palette with the same tip; Animate with no animation has exactly one New animation button on screen; a tab row from 200 to
  340 points wide never reaches the move button, and a cut title keeps the whole title as its accessible name.
- Review screenshots: every mode in the default arrangement at 1280 x 800, and a narrow left area.
- Gates: fmt, clippy `-D warnings`, the workspace tests, `cargo xtask layering`, `ledger`, `ui-style`, the seven
  private oracles.

## 10. Changes to ui_v2_spec

- 6 (Components): the three-step fitting of tab rows.
- 8 (Editor layout): the panels and tabs per mode of 3 and 4, and the rules of 2.
- 8.1 (Familiarity presets): unchanged arrangements; the filter applies to every preset.

## 11. Not in this round

- A separate arrangement for each mode, and a front tab kept for each mode.
- Folding Faces and UV into the UV editor.
- The validator as a count in the status bar.
