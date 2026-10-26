# T1: animated textures (frame strips, `.mcmeta`, `.bbmodel` frames)

Status: 2026-10-26, refines `anim_texture_spec.md` 5.1 for the code after A3 (652a24b). Owner report: the
animated textures of a `.bbmodel` (vertical strips of square frames) render as stripes, because every frame of
the strip lands on the face. In Blockbench and in Minecraft a face shows one frame.

## 1. Today

- A texture entry is a label and an image. Face UVs are normalised over the image.
- The `.bbmodel` reader normalises UVs by `uv_width` and `uv_height`, the UV size of one frame; for a strip the
  whole strip is then mapped onto the face.
- The resource pack reader reads `pack.mcmeta` only; texture `.mcmeta` files are ignored and never written.
- The atlas gives each texture one rect of its whole image (`cuboid-scene/src/atlas_layout.rs`); the renderer
  uploads it once (`cuboid-render/src/atlas.rs`).

## 2. Data (cuboid-model)

- A texture entry gets `animation: Option<TextureAnimation>`:
  `TextureAnimation { frame_width, frame_height, frametime, interpolate, frames, extra }`. The image is a
  vertical strip of `frame_width` x `frame_height` tiles (square, the image width, unless a file says
  otherwise). `frametime` is in game ticks (20 per second, at least 1). `frames` is the play order,
  `None` for every tile once in order, else steps of a tile index and an optional time of their own.
  `extra` keeps fields of a file the model does not use, as JSON text, written back unchanged.
- Face UVs are relative to one frame (as in Minecraft and Blockbench), so no UV changes.
- Pure helpers with hand-computed tests: the number of tiles; the step and the tile at a tick, the next tile
  and the blend weight when interpolating; the cycle length in ticks. Bounds: at most 4 096 tiles and 4 096
  steps, times up to 72 000 ticks.

## 3. Formats

- `.bbmodel` read: a texture whose height is a whole multiple (at least 2) of its width is a strip of square
  frames, in the formats Blockbench animates textures in. `frame_time`, `frame_interpolate`,
  `frame_order_type` (loop, backwards, back_and_forth, custom) and `frame_order` become the animation; the
  raw fields are kept so that an unchanged file writes the same values. The Blockbench clone may be read to
  learn the fields, never copied.
- `.bbmodel` write: the strip and the frame fields.
- Resource pack read: `<texture>.png.mcmeta` beside a texture (`animation`: `frametime`, `interpolate`,
  `width`, `height`, `frames` as numbers or `{index, time}`). A damaged `.mcmeta` gives a notice and a still
  texture.
- Resource pack write: the strip and its `.png.mcmeta` (only the fields that differ from the defaults;
  `frames` written out for a Blockbench order that is not a plain loop).
- `.cubik`: Cubik cannot animate, so the payload texture keeps frame 0 only and Cubik shows a still frame.
  The `cuboid.json` entry stores, per animated texture, the strip as PNG data and the animation, with the
  hash of the frame 0 it wrote. A payload texture that no longer matches the hash (edited in Cubik) drops
  the strip with a notice. This replaces the `cuboid/tex/*.png` entries of the spec: the container keeps
  its two entries and the strips are compressed PNG already. The payload of an unchanged file stays byte
  for byte the same.
- Other writers: a still frame 0 where the format has no animation, with the usual note.

## 4. Showing frames

- The atlas holds one frame per animated texture: a rect of the frame size, filled from the tile the views
  show. A change of the tile uploads that rect only (a frame of a 16 x 16 texture is 1 KiB).
- What the views show is a texture tick kept by the app. By default it does not move, so every animated
  texture shows its first step. A play button in the Textures panel plays all animated textures at 20 ticks
  per second; while playing the app asks for frames at the tick rate, otherwise it does not repaint.
  `interpolate` blends the two tiles on the CPU into the rect.
- The UV editor shows the frame the views show. A paint tool on an animated texture paints into the tile
  shown; the core gets the tick from the app as A3 gives it the playhead.

## 5. Editing (Textures panel and the texture tab of the properties)

- The Textures panel marks an animated texture with its frame count and has the play button.
- A Frames section for the selected texture: the steps in order (virtualised list), the one shown, per-step
  time, the default `frametime`, `interpolate`; buttons to show a step, add a frame (a copy of the shown tile
  at the end of the strip), remove a step, and move a step up or down. "Animate" turns a still texture whose
  height is a multiple of its width into a strip; a still square texture becomes a strip of one frame that
  "Add frame" can grow.
- Commands, each one undo step in the texture category: `SetTextureAnimation` (the whole animation, or none),
  `AddTextureFrame` (copies a tile to a new tile at the end of the strip, which grows the image). Serde,
  guards, validation, bounds as the other commands.

## 6. Validation

A resource pack target checks animated textures: the strip height a multiple of the frame height, frame indices
inside the strip, times at least 1; non-square frames need a version that reads `width` and `height`.

## 7. Out of this round

Onion skin, interpolation on the GPU, frames of mesh textures, drawing tools beyond painting into the shown
tile.

## 8. Tests and gates

- Model: the helpers of 2 (loop, every Blockbench order, custom steps with times, interpolation weights,
  bounds).
- Formats: `.bbmodel` with a synthetic strip (round trip of the fields); `.mcmeta` variants (numbers, objects,
  missing, damaged, non-square); resource pack round trip; `.cubik` round trip (payload frame 0, the strip
  back from `cuboid.json`, a stale hash drops it with a notice, an unchanged file keeps its payload).
- Render (headless): a face on a two-tile strip of two solid colours shows the colour of the first tile; at a
  later tick, the second; only the frame rect is uploaded.
- UI: the frame count mark, the play button, the Frames section and its requests.
- App: playing asks for frames at the tick rate, pausing stops them; painting goes to the tile shown.
- The private `.bbmodel` oracle imports its animated textures as strips (counts only, never content). Gates:
  fmt, clippy `-D warnings`, the workspace tests, `cargo xtask layering`, `ledger`, `ui-style`, the seven
  private oracles. Opus reviews the formats (high risk).
