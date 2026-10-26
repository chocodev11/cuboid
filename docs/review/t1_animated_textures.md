# Review: T1 animated textures

Reviewer: Opus, 2026-10-26. Scope: docs/design/animated_textures_t1.md as built. The model and the formats
were read by the reviewer, the atlas, the core, the UI and the app by a second Opus reader. Gates and the seven
private oracles were run again after the agent.

Verdict: passes after the fixes below.

## Checked

- Model: the tile of a tick for every order and for steps with times of their own; a pass is at most
  4 096 x 72 000 ticks, so no sum overflows; interpolation blends a step into the next one over its time, as the
  game does; tile rows count from the bottom of the data, like every image of the model.
- `.cubik`: the payload of an animated texture holds the tile of the first step, and the record of an
  unchanged file is reused when its pixels match, so the payload stays byte for byte the same. The strip in
  `cuboid.json` is taken only when the payload texture matches its digest (SHA-256, 8 bytes) and the first
  frame of the strip matches it too; otherwise the still picture stays, with a warning. The base64 text and the
  texels of all strips are bounded, and the image decoder limits sides and allocations.
- `.bbmodel`: a strip is found with Blockbench's rule (a frame is as high as the width times the UV height over
  the UV width), at least two whole frames, in the formats that animate textures, so a 64 x 128 skin stays a
  picture. The four frame fields are written back as they were while they still say what the animation says.
- `.mcmeta`: the frame size follows the game (`width` alone keeps the picture height, `height` alone the picture
  width, neither gives square frames of the shorter side); a damaged member gives a notice and a still
  picture; unknown members are kept and written back; keys are escaped.
- base64 moved from `cuboid-fmt-bbmodel` to `cuboid-io` unchanged in behaviour, with an encoder for the
  `.cubik` strips.
- Screenshots: the frame count in the Textures panel, the play row, the Frames section with steps and times.
- Second reader: no panic, no lost edit, no broken undo; tile math, uploads of the frame rect only, the clock
  (no repaint while paused, wake-ups at the tick rate), the two commands, painting into the shown tile, the UV
  editor, the Frames section and the three validator rules checked.

## Fixed in review

1. An animated texture that found no room in the atlas was placed again on every tick: the views rebuilt
   every element twenty times a second and the log filled with warnings. It is now left alone until its
   picture changes, as a still texture is.
2. The atlas compared the whole state of a tick, so a tick that showed the same picture (a frame that blends
   into itself, a tile held over several steps) uploaded it again. It now compares the picture: the tile, and
   the mix while a blend is under way.
3. The Frames section was listed again on every frame, up to 4 096 steps per animated texture. It is now kept
   while the animation and the step shown stay the same.
4. "1 frames" in the texture tab reads "1 frame".

## Known limits

- Blockbench counts a partial last frame (it rounds the frame count up); cuboid takes only a whole number of
  frames, so such a picture stays still.
- A `.mcmeta` whose frames sit side by side (a grid, which the game reads) is treated as damaged.
- Mesh exporters write the first frame with a notice; the UV editor shows the tile, not the blend.
