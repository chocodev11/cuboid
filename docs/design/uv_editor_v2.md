# UV editor v2: zoom, pixel grid, faces in the view, numbers

Status: 2026-10-27, owner request ("a grid on the texture, UV editing closer to Blockbench: zoom in and out and
more"); refines `anim_texture_spec.md` 5.5. The box-UV template generator of 5.5 stays for a later round.

## 1. Today

`crates/cuboid-ui/src/uv.rs` fits the texture of the chosen face to the panel (no zoom, no pan, no grid), outlines
the other faces of the element dimly, and moves the chosen face by its body or a corner in whole texels. The face
is chosen in a menu; the toolbar has auto UV, a quarter turn, mirror and flip. Textures draw with nearest
filtering, so a zoomed texel stays sharp. An animated texture shows the frame the views show (T1).

## 2. View

- Zoom with the wheel about the pointer, from 25 % to 3 200 %, in steps that land on whole multiples of a texel
  where they can; `+` and `-` while the pointer is over the editor; a Fit button and a zoom readout in the
  toolbar (click it to fit). A new texture starts fitted; switching faces of the same texture keeps the view.
- Pan with the middle button, or with Space held and the left button (the Cubik way), or with the right
  button; the texture may leave the view, the view never leaves the texture entirely.
- Pixel grid: texel lines once a texel is at least 6 points on screen, fading in, with a stronger line every
  16 texels on larger textures. A toggle in the toolbar, on by default, kept in the preferences. Only visible
  lines are drawn.
- Under the picture: the texel under the pointer and the zoom.

## 3. Faces in the view

- Every face of the primary element that uses the shown texture is drawn with a short label (N, E, S, W, U, D);
  the chosen faces are filled with the accent, the others outlined. A click on a face chooses it; Shift or
  Ctrl adds or toggles; a click on empty texture keeps the choice. The menu of faces stays.
- "All UVs": a toggle that draws the faces of the other elements on the same texture faintly, to see overlaps.
- Dragging a chosen face moves all chosen faces together; a corner resizes the face it belongs to. Snap to whole
  texels, Shift to quarter texels, Alt for no snap. One drag is one undo step.
- Arrow keys move the chosen faces by a texel (Shift: by 4) while the pointer is over the editor.

## 4. Numbers

Below the view, for the primary chosen face: X, Y, width and height in texels (drag fields, a drag is one undo
step), the rotation (0, 90, 180, 270) and the texel density of the face in texels per block on each axis, which
turns the warning colour when the two axes differ.

## 5. Core

- `MoveFaceUv { faces, delta }`: moves the UV rectangles of several faces of the selection by the same offset in
  texture coordinates, keeping each face's size and flips; one undo step; serde, guard and validation as the
  other UV commands. A resize of one face stays `SetUvRect`.

## 6. Cost

No repaint while idle; the grid and the face labels are drawn from the visible range only; a zoomed view of a
large texture draws one image, not tiles.

## 7. Tests and gates

- UI (headless egui): wheel zoom keeps the texel under the pointer in place; pan; Fit; grid lines appear above
  the zoom threshold and only in view; click, Shift and Ctrl choose faces; a drag of two chosen faces sends one
  move with the snapped delta, Shift and Alt change the snap; arrow keys; the numbers send their edits; the
  density readout. A review screenshot zoomed in with the grid, the face labels and the numbers.
- Core: `MoveFaceUv` with undo and redo, flips kept, bounds.
- Gates: fmt, clippy `-D warnings`, the workspace tests, `cargo xtask layering`, `ledger`, `ui-style`, the seven
  private oracles.
