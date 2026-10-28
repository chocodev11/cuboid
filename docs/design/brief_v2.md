# Project brief v2

Status: owner decision 2026-10-25 (brief before animation). It extends the library brief of L5
(`crates/cuboid-library/src/brief.rs`) so that an agent can read what a project wants and check its own work
against it, without the app. Review: the brief of L5 holds only a name, a game version and style tags; the
palette and the texel density are observed values, not targets; nothing outside the app can read the computed
brief or the suggestions.

## 1. Goals

1. One machine-readable view of a project: what it is for, its targets and rules, and what the library holds.
2. The same view as Markdown for language models, written next to `library.json`.
3. Targets that the validator checks, so a model can be judged against the project, by a person in the
   Validator panel or by an agent from the command line.

## 2. Brief fields (library.json schema 2)

The index `version` goes from 1 to 2. A schema 1 file reads with the new fields at their defaults; a schema 2
file is refused by older builds as today (`LibraryError::Newer`). All text is trimmed and bounded like the
schema 1 fields; lists are bounded.

| Field | Type | Meaning |
|---|---|---|
| `project_name`, `mc_version`, `style_tags` | as schema 1 | unchanged |
| `goal` | text, at most 2000 chars | what the project is for, in the user's words |
| `platform` | `vanilla`, `model_engine`, `items_adder`, `oraxen`, `geckolib`, `other` | where the models go |
| `namespace` | resource id namespace, at most 64 chars of `[a-z0-9_.-]` | for example `splatcraft` |
| `texel_density` | number or none | target texels per block |
| `texture_size` | 16, 32, 64, 128, 256, 512 or none | largest texture side the project allows |
| `palette` | at most 64 RGB colours | the project palette; empty means none |
| `palette_strict` | bool | texture colours outside the palette are errors instead of warnings |
| `max_elements` | number or none | element budget per model |
| `naming` | `case` (`snake`, `kebab`, `any`) and `prefix` (at most 32 chars) | names of model files, textures and elements |
| `display_preset` | `item/generated`, `item/handheld`, `block/block` or none | the default for new models |
| `rules` | at most 20 lines of at most 200 chars | do and don't rules in the user's words |

"Use observed values" in the brief dialog copies the observed palette (up to 16 colours) and the median
texel density into the targets; nothing does that silently.

## 3. Command line

`cuboid-cli brief [--json | --md] [--write] [<folder>]`

- `<folder>`: a project folder (it holds `cuboid-library/library.json`) or a library root; default: the
  current folder. A folder without an index is an error with exit code 2.
- `--json` (default): one JSON object, keys in a stable order, schema below. `--md`: the Markdown of 4.
  `--write`: writes `brief.md` next to `library.json` atomically and prints its path.
- Never writes anything else, never touches the network, and checks file sizes before reading like the
  library does.

`cuboid-cli check [--brief <folder>] [--target <block|item|pack>] [--json] <model>`

- Loads a `.cubik`, `.cub`, `.bbmodel` or Minecraft model JSON with the importers the app uses and runs the
  validator with the target's profile plus the project rules of 5 when `--brief` is given (or when the
  model's folder holds a project library). Prints the issues (code, severity, object, message, fix
  available); exit code 0 without errors, 4 with errors.

JSON of `brief` (version 1 of this output):

```json
{
  "format": "cuboid-brief", "version": 1,
  "brief": { "...": "the fields of 2" },
  "observed": { "palette": [[r, g, b]], "texel_density": 16.0, "models": 42, "textures": 118 },
  "entries": [ { "id": "...", "kind": "model", "name": "lantern.cubik", "file": "files/<id>-lantern.cubik",
                 "tags": [], "meta": { } } ],
  "lore": [ { "entry": "...", "title": "...", "names": ["Ink Blaster"] } ],
  "suggestions": [ { "kind": "lore_without_model", "entry": "...", "title": "...", "detail": "..." } ]
}
```

## 4. brief.md

Written by the app whenever the brief or the entries change (on the library's own I/O thread, atomic, only
when the text differs) and by `cuboid-cli brief --write`. First line: a comment that the file is generated
and that the brief is edited in cuboid. Sections, short tables, no prose beyond the user's own text:
Project (name, goal, platform, namespace, game version and pack format), Targets (density, texture size,
palette as hex codes, budget, naming, display preset), Rules (the user's lines), Library (models with
format, elements, density and texture size; textures with size, frames and alpha; references; lore titles
with their names and whether a model exists), Suggestions.

## 5. Validator rules

`cuboid-validate` stays free of the library: `Profile` gains plain fields, and the app fills them from the
brief of the document's project library (none when the document has no project library).

| Code | Severity | Rule |
|---|---|---|
| `BUDGET_ELEMENTS` | as today | now also fed by `max_elements` |
| `PRJ_DENSITY` | warning | a face's texel density differs from `texel_density` by more than `texel_ratio` |
| `PRJ_TEXTURE_SIZE` | warning | a texture side is above `texture_size` |
| `PRJ_PALETTE` | warning, error when strict | a texture uses colours farther than the palette reach (`PALETTE_REACH`) from every palette colour; the issue lists up to 8 of them |
| `PRJ_NAME` | info | the model file, a texture or an element name does not follow `naming` |
| `PRJ_NAMESPACE` | warning | a texture path or item id uses another namespace than `namespace` |

Each new code gets a self-test entry, a fix where one is safe (for `PRJ_NAME`: rename to the case), and an
entry in the Validator panel's filter.

## 6. App

- The brief dialog (`crates/cuboid-ui/src/brief.rs`) shows the fields of 2 in sections Project, Targets,
  Naming and Rules on PropertyGrid rows, with "Use observed values"; the start-screen card shows goal,
  platform, namespace and the targets that are set.
- New documents in a project take `mc_version` and `display_preset` from the brief.
- The suggestions use the targets when they are set (density against `texel_density`, palette against
  `palette`) and the observed values otherwise.

## 7. Agent toolkit

This section first named the tools `ws_brief` and `ref_*`, which return the JSON of 3, and `model_check`, which
runs the same check as `cuboid-cli check`. R1 of the agent toolkit (`agent_toolkit_r1.md`, user guide
`docs/mcp.md`) is built before the first release and maps them onto the tools of the MCP server `cuboid mcp`:

| Named here | R1 tool | What it gives |
|---|---|---|
| `ws_brief` | `brief_get` | the JSON of 3 as output version 2 (`agent_toolkit_r1.md` 4.5): `entries` in pages, the style summary of the references, no `style` inside each `meta` |
| `ref_*` | `refs_get`, `refs_import` | the outline of a stored reference model, or the size, frames and palette of a texture, with its style numbers; the import of a project folder as style references |
| `model_check` | `validate` | the issues of `cuboid-cli check` for the model as it is open in the server |

The picture boards and pins of the draft are not part of R1, and the rest of the toolkit stays in L7.
