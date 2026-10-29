# The title bar: one row for the window and the top bar

Status: 2026-10-26, owner request. Today the system title bar ("lantern - Cuboid" and the window buttons)
sits above cuboid's top bar; the two become one row, as in Blockbench and VS Code, which gives the views about
30 pixels of height back.

## 1. Today

- `cuboid-shell` opens a decorated winit 0.30.13 window; the OS draws the title and the window buttons. The top
  bar (`cuboid-ui/src/topbar.rs`, 32 points: brand, menus, mode tabs, the preset button, the command search)
  is the first panel of the window.
- winit 0.30.13 on Windows: an undecorated window has no caption, no border and no resize edges (the client
  area covers the whole frame). It offers `drag_window`, `drag_resize_window`, `set_maximized`,
  `set_minimized`, `set_decorations`, `with_undecorated_shadow` and `with_corner_preference`.

## 2. Behaviour

- Windows and Linux (X11, Wayland) open the window undecorated, on Windows with the system shadow and round
  corners. The top bar is the title bar:
  - Left to right: the brand, the menus and the mode tabs as today, the document name centred in the free
    space (with the unsaved marker of the window title), the preset button, the command search, then the
    window buttons Minimize, Maximize (Restore while maximized) and Close.
  - The window buttons are 46 by 32 points, the full height of the bar, flush with the right edge, with kit
    icons and accessible names. Hover fills: the control fill for Minimize and Maximize, the danger colour
    with a light glyph for Close; the contrast rules of `ui_v2_spec.md` 4 apply.
  - Empty space of the bar: a drag moves the window through the system move loop (so Aero Snap and dragging a
    maximized window off its place work); a double-click maximizes or restores. A press on a menu, a tab, a
    button or the search does not move the window.
  - Close runs the Exit action, so unsaved changes are asked about as with the system close button.
  - Resize: within 5 points of an edge of the window (10 at the corners) the cursor is the resize cursor and a
    left press starts the system resize, over every panel; not while maximized or in full screen.
  - The OS window title stays set (taskbar, Alt+Tab). Alt+F4 and Win+arrow keep working.
  - An inactive window shows the brand and the document name in the weak text colour.
  - Full screen (F11) hides the window buttons and the drag area; the bar keeps its menus.
  - A narrow window drops the document name first, then the search follows its present compact rules.
- macOS keeps the system title bar in this round.
- Preference "Use the system title bar" (Preferences, Interface), off by default: when on, the window is
  decorated and the bar has no window buttons and no drag area. It applies at once (`set_decorations`).
  It is the way back if a window manager handles the frameless window badly.

## 3. Shell

- `ShellOptions.decorations`; `WindowState` gains `maximized`, `decorated` and `focused`; `FrameRequest`
  gains window commands (start a move, toggle maximize, minimize, set the decorations), applied after the
  frame. A move starts only while the primary button is still down.
- Edge resize lives in the shell, before egui sees the pointer: a pure function from the pointer position,
  the window size and the band to a `ResizeDirection` or none; on an edge the shell shows the resize cursor
  for that frame and keeps the press from egui.
- The headless shell records the window commands for the tests and has no edges.

## 4. UI and app

- `UiCx` gets the window state, `UiOutput` the window requests of the bar; the app maps them to the shell's
  window commands and stores the preference with the others.
- The drag area is one interaction over the whole bar, added before the widgets so that they win.

## 5. Tests and gates

- UI (headless egui): each window button sends its request and Close sends Exit; a double-click and a drag on
  empty space; a press on a menu, a tab or the search moves nothing; no window buttons when decorated or in
  full screen; the Restore icon while maximized; the weak colours of an inactive window; screenshots of the
  editor and the start screen with the bar, Mocha and Latte.
- Shell: the edge function over edges, corners, the inside and a maximized window; the window commands of the
  headless shell.
- App: Close with unsaved changes asks; the preference switches the decorations.
- Manual, by the owner on Windows 11: moving, snapping, resizing, maximizing, the shadow and the corners.
  Agents never drive the real window.
- Gates: fmt, clippy `-D warnings`, the workspace tests, `cargo xtask layering`, `ledger`, `ui-style`, the seven
  private oracles.

## 6. Decisions of the build

Where the text above did not hold, the smallest sound change was made:

- **Drag area.** egui gives a drag to the interaction behind a button as soon as the pointer has moved, so a press
  on a menu or a tab and a small movement would have moved the window. The area is still one interaction over the
  whole bar, added first; a drag moves the window only when the press began where no widget of the bar senses a
  click or a drag (`topbar::pressed_on_empty_space`).
- **Close glyph.** The glyph on the danger fill is the kit's `on_accent`, as for the Danger button: it is dark on
  the pastel red of the dark themes and light in the light themes. A white glyph on Mocha's red is about 2.3:1. A theme test
  holds `on_accent` on `danger` at 4.5:1 (7:1 in high contrast).
- **Edge zones.** The 10 point corner zone is the 5 point edge zone extended to 10 points along the other edge,
  not a 10 point square: a square would cover the interior next to the corner.
- **Tight bars.** Near the minimum width of 900 points the three window buttons leave little room. Things give way
  in this order: the document name, the search words, the label of the preset button, the word "cuboid" below 906
  points (the logo stays, `logo.md` 5), then the preset button's place (File > Project settings stays).
- **Window state.** `WindowState::default()` is a focused window with the system title bar, so a test window shows
  the bar of today; `WindowView::default()` in the UI is the same.
