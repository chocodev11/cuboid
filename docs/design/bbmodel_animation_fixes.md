# Blockbench 4 keyframe signs, looping Catmull-Rom, faces without a texture

Status: 2026-10-27, owner report: a ModelEngine `.bbmodel` (format 4.10) that plays right in Blockbench and in
game bends the wrong way in cuboid, and its parts without a texture show the first texture. Measured on that
private model with a probe of cuboid's `pose()` against Blockbench's rules (no content of it goes into the
repository).

## 1. Findings

- Blockbench 5 keeps keyframe values in its own convention and converts projects older than 5.0 when it loads
  them: the X value of position and rotation keyframes and the Y value of rotation keyframes change sign
  (numbers negated, Molang expressions inverted), and so do the Bezier handle values of those axes. Its legacy
  export (format 4.10) converts back; ModelEngine reads the 4.x convention. The load converts every animator of
  such a project, whatever object it animates. cuboid reads every project as if it were 5.0, so a 4.x animation
  turns about X and Y the other way: on the private model whole parts swing to the wrong side.
- Catmull-Rom of a looping animation: with at least three keyframes Blockbench takes the neighbour before the
  first keyframe from the second-to-last one and the neighbour after the last keyframe from the second one;
  cuboid repeats the end keyframes, which bends the curve near the ends of a loop. Outside its keyframes a
  looping channel holds its first or last value in both: Blockbench blends across the end of the loop only for a
  model format that asks for it, and none of its own formats does.
- Faces without a texture (the member missing, `false`, or naming no texture of the project): Blockbench draws
  them in the pastel shade of the cube's marker colour, from its ten marker colours (light blue, yellow, orange,
  red, purple, blue, green, lime, pink, silver), except in the formats of a single texture (Bedrock entity and
  block, the old Bedrock format, modded entity, skin, OptiFine, image), where they show the default texture. A
  face with `texture: null` is switched off. cuboid gave every face without a texture the default one, and its
  own marker table was in another order.

## 2. Fix

### 2.1 Signs

- cuboid's animations keep Blockbench 5's convention: the player, A3's keying and the Animate tab are unchanged.
- `.bbmodel` read, format version below 5.0: the X of position and rotation and the Y of rotation of every data
  point change sign, and the Bezier handle values of the same axes. A number is negated; an expression is
  inverted so that it still evaluates to the opposite value.
- `.bbmodel` write (cuboid writes 4.10): the same change back. A value that was read and not changed is written
  as the project had it, text and number alike, so an unchanged project keeps its animations exactly (the
  property test of A2 stays).
- The animators and animations the model keeps as JSON (an animator of an object that is not a group, an
  animation beyond the limits) are converted at read the same way when the project is 5.0 or later, so that all
  the writer emits has the signs of 4.10; those of an older project stay exactly as they were.
- `.cubik`: the animations of `cuboid.json` are in cuboid's convention. A section written before this fix is read
  as it is; the owner re-imports a 4.x project that was saved to `.cubik` before.

### 2.2 Looping Catmull-Rom

For an animation that loops, the neighbours of a Catmull-Rom segment wrap as in 1. Times outside the keyframes
keep the end values, as before. Animations that play once or hold are unchanged.

### 2.3 Faces without a texture

- A face that the project gives no texture has no texture in the model rather than the default one, except in
  the formats of a single texture; `texture: null` stays a face that is off. The element keeps the marker index
  of its project (`Element::marker`, none for an element that did not come from a project, which then has the
  marker nearest to its colour on export).
- The views draw such a face flat in the pastel shade of the element's marker colour, with the usual face
  shading; on an element without a marker it keeps the error texture. The marker table becomes Blockbench's ten
  colours with both shades.
- The writer writes such a face without a `texture` member, as Blockbench does, and the element's marker.
  Yesterday's marker index in `BbKept` becomes this field. `.cubik` does not keep the marker: `cuboid.json` has
  no section for elements.

## 3. Tests and gates

- Signs: a 4.10 and a 5.0 project with the same motion, each in its own convention, give the same pose; a 4.10
  project round-trips its animations exactly (numbers, numbers written as text, expressions, Bezier handles); a
  5.0 project written as 4.10 carries the converted values, the animators kept as JSON included.
- Loops: hand-computed values in the first and the last segment of a looping Catmull-Rom channel, and the end
  values held outside the keyframes.
- Faces: an untextured face imports untextured, draws in the pastel marker colour (headless extract test) and
  exports without a `texture` member with its marker; `null` stays off; the formats of a single texture keep the
  default texture.
- Gates: fmt, clippy `-D warnings`, the workspace tests, `cargo xtask layering`, `ledger`, `ui-style`, the seven
  private oracles. The reviewer re-runs the pose probe on the private model: cuboid must match Blockbench's rules
  at every time, loops included.
