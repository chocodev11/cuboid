# Review: the title bar

Reviewer: Opus, 2026-10-26. Scope: docs/design/title_bar.md as built: the input handling of the shell, the drag
area and the window buttons of the bar, the preference in the app. Gates and the seven private oracles were run
again after the agent. The real window (moving, snapping, resizing, the shadow and the corners) is for the owner
to try by hand: agents never drive a window.

Verdict: passes.

## Checked

- Shell: a press on an edge starts the system resize and does not reach egui; a move runs only while the
  primary button is down. When the system move or resize loop ends, winit posts a button-up
  (`WM_EXITSIZEMOVE`), so neither the shell nor egui keeps a button held. The resize cursor shows on hover
  only. There are no edges with the system frame, while maximized or in full screen.
- Bar: the drag area is one interaction over the bar, added before the widgets. A drag counts only when its
  press began where no widget of the previous pass senses a click or a drag, so a drag from a menu, a tab, a
  button or the search moves nothing. Labels are not selectable anywhere in the app, so the document name is
  empty space for a drag. Close runs Exit and asks about unsaved changes.
- App: the window starts decorated when the preference says so and always on macOS, where the row is hidden; a
  change of the preference sends one `SetDecorations`.
- Screenshots: one row at 1280 points; at 900 the name and the preset go, the search keeps its icon, the window
  buttons stay.

## Accepted deviations of the agent

The drag only counts from a press on empty space (see above); the Close glyph is the kit's `on_accent`, dark on
the pastel red of the dark themes (white is about 2.3:1 there; a theme test holds 4.5:1); the corner zones are
the edge band extended to 10 points along the other edge; short of room the preset button gives up its label,
then its place; the default window state is focused with the system title bar, so older tests stand.

## Known limits

- No Windows 11 snap-layout flyout on Maximize: it needs a hit test that winit 0.30 does not offer.
- winit's shadow for an undecorated window draws a thin line at the top edge.
- Alt+Space may not open the system menu without the system caption; Alt+F4 and Win+arrows work.

## Also in this set of changes

- The Goal field of the Project brief covered the rows below it: a property row is now as tall as its value,
  and a text area grows with its wrapped text up to 8 lines, then scrolls.
- `.bbmodel` marker colours tinted the textures: the importer turned Blockbench's marker index into the
  element colour, which multiplies the texture. Elements now import white; the marker index is kept with the
  uuids and written back on export.

## Process

The agent used `cat >>` once (a test block, redone with Edit) and `sed -i` once (a mutation probe, restored at
once).
