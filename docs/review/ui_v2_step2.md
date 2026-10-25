# UI v2 step 2 review (start screen, editor shell, familiarity presets)

Reviewer: Opus, 2026-10-25. Material: the 57 pictures of `crates/cuboid-ui/tests/screens.rs` in
`target/ui-screens/`, the ui-kit gallery, the step 2 report and the code paths named below.

Verdict: passes. The start screen follows the approved mockup, the shell follows spec 8 and 8.1, and the
first-run modal sets navigation, keymap and arrangement in one choice. The findings below move to step 3.

## Findings for step 3

1. Cubik arrangement: with the properties in the left area, their icon strip stands next to the tool strip, so
   two icon columns of different meaning touch at the window edge. The strip must face the viewport (spec 8).
2. The Texture tab repeats the texture list of the Textures panel; the Model tab shows the palette as a
   placeholder; the Display tab only says that the model has no display settings. Each tab needs its own
   content (step 3 brief).
3. Property tabs still put labels above the values; spec 6 wants PropertyGrid rows.
4. Top bar at 150 % below 1024 pt: the command field truncates to "Searc..."; show the icon and the chord only.
5. Recent cards: the fitted thumbnail sits at the top of its 4:3 frame and leaves a band below; centre it.
6. Viewport header: brush size and mirror axes disappear in narrow cells; they need an overflow menu.

## Decisions on the open questions of the report

- Right area width: 300 of content plus the 36 strip (336 in total) is accepted; spec 8 now says so.
- Suggestions without an app action get no button for now; the library work adds actions later.
- Latte keeps the official Catppuccin text colour. Its pressed step stays shallow, and the edge, the tick and
  the strong weight mark the state; the relaxations are documented in the contrast test.
- `Prefs::default().keymap` stays Blender: the first-run modal applies the chosen app's keymap on Continue and
  on Esc (ui.rs applies `coming_from` whenever the question closes), so new users never see the mix.
