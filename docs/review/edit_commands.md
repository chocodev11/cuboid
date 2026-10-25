# Review: editing commands that replaced the "not available yet" toasts

Reviewer: Opus, 2026-10-25. Scope: `SetPivot`, `SetColor`, `SetShade`, `SetFaceEnabled`, `SetFaceTint`,
`SetFaceCull`, `ReplaceTexture` (with texture reload in the app) and `PaletteRemove`, plus the
`SetDisplaySlot` fix that turns on `include_display`.

Verdict: passes. Every command declares the undo categories of the data it changes, refuses non-finite
values, leaves no undo step for a no-op, and has scenario tests; the app tests cover the `.cubik` round trip
and the Minecraft JSON output of each field.

## Findings (none blocking)

1. `SetColor` writes alpha 1.0; tools 10.5 says the channels are clamped to at least 0.25 and alpha is left
   untouched. It only matters for an element colour that arrives with alpha below 1.
2. `AddTexture` and `ReplaceTexture` carry the pixels as `Vec<u8>` and the core clones them on execute, so a
   large texture is held twice for a moment. An `Arc<[u8]>` payload would remove the copy.
3. The Textures panel's Replace button still answers "not available yet"; it needs a file picker and can use
   `ReplaceTexture`.
4. Reload does not apply the slot downscale of spec 5.10 or the animation crop; that belongs with the
   animated-texture work (anim_texture_spec T1).
5. `PaletteRemove` is not undoable, like `PaletteAdd`: the palette is derived from the voxels, so a swatch the
   voxels still use returns on the next undo, redo or load.
