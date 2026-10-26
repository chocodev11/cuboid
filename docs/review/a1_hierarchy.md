# Review: A1 groups and hierarchy

Reviewer: Opus, 2026-10-26. Scope: docs/design/hierarchy_a1.md as built (model, commands, `.bbmodel`,
`.cubik` sidecar, outliner). Gates and the seven private oracles were run again after the agent: 2152 tests
pass, payload identity of unchanged `.cubik` files holds.

Verdict: passes.

## Undo and apply

- `Categories::HIERARCHY` (bit 128) restores the tree and the selection of the moment (`GroupsSnap` carries
  the selection, because a tree edit holds no object list to take the flags from).
- A merge leaves the hierarchy of the merged file out: its groups name the ids of that file, and the merged
  objects get new ids. The objects arrive flat; acceptable until merge learns to remap groups.
- After an open or a restore, links to objects that no longer exist are pruned (`retain_live`), so an undo of
  an objects-only step cannot leave a dangling child.

## Formats

- `.bbmodel`: local transforms are inverted in f64 on write; `tidy` snaps values within 2e-5 of the 4-decimal
  grid. Accepted: the change is below the precision Blockbench writes. Kept sections (animations,
  controllers, placeholders, unknown keys) are bounded at 64 MiB and written back verbatim.
- `.cubik`: the groups live in `cuboid.json` with record signatures and a digest; a stale digest keeps the
  links that still match and warns.

## Decisions

- `Reparent.index` stays "position among the children that stay"; a parent that is not a group is refused.
- The note that a save leaves the kept animations out comes once per document (`Editor::kept_warned`), not on
  every save; the app test checks the second save stays quiet.

## Known limitations

- Undo of an objects-only command does not re-select a group that the command deselected.
- Merge does not bring groups along (see above).
