# Agent toolkit for weapon workflows (design draft)

Status: draft, 2026-10-23, not reviewed. Owner decisions are listed in 12. Nothing here is implemented.

## 1. Goal and scope

cuboid gets a toolkit that lets an AI agent take one custom Minecraft Java weapon from a brief to an in-game
check: reference collection, blockout, modelling, UV and texture, display transforms, item states, validation,
sample renders, emulated in-game renders, resource pack export and a live test on a local Paper server that the agent sets up itself (10). The
agent calls typed tools only. There is no eval tool, and no step needs an agent-written script.

The toolkit is additive. Cubik parity (formats, UI, tools) is unchanged: `.cubik` files stay byte-compatible and
every new datum (references, workflow state, renders, provenance) lives in a workspace folder next to the model (3).

Out of scope: animation timelines, armatures, free meshes and Bedrock particles, which a Java item does not need.
Cubik's licence and registration code is never ported; cuboid is open source.

## 2. What the architecture already provides

| Need | Existing piece | Gap |
|---|---|---|
| Edit without a window | pure core, thin shells (architecture principle 6); M2 headless core | commands as data with schemas, ids, dry-run (14) |
| Render without a window | `Headless` offscreen target; W3.7; app 15 Render to PNG | camera presets, turntable, compare metrics |
| In-game preview scenes | app 9.5: player third person and head, first person, item-frame wall, GUI slot, floor | per-context metrics, shading check against the game (9) |
| Sprite to cubes | interop PNG sprite import, "Extrude sprite with optimized elements" | input from a pixel grid (7.2) |
| Resource pack, item definitions | interop 9.1, 31.3 (`items/*.json`, 1.21.4 and later), 31.4 | several targets per export, give snippets |
| Reference images | app 7 view background images (top, side, front), not persisted | persisted, tagged, scoped references (6.2) |
| Scripted sequences | testkit scenarios `tests/scenarios/*.ron` | the scenario op vocabulary becomes the public tool vocabulary |

## 3. Workspace

```
ink_blaster/
  cuboid.toml        manifest: id, namespace, targets, budgets, gates, stage state
  model.cubik        the model, readable by Cubik
  refs/              reference images and refs.toml (source, licence, tags, view, pin)
  renders/           tool outputs, named <kind>-<context>-r<revision>.png
  checkpoints/       named snapshots of model and textures
  build/pack/        exported resource pack (folder and zip)
  build/server/      give commands and Paper snippets per target
  journal.jsonl      one line per write: time, agent, tool, revision, summary
```

```toml
id = "ink_blaster"
namespace = "splat"
targets = ["1.21.6"]          # keys of the Minecraft version table
carrier = "minecraft:stick"   # item that carries the item model in give snippets

[budget]
elements = 64
texture = [32, 32]
colors = 16

[gates]
silhouette_iou = 0.8          # blockout against the pinned front reference
```

A user library (`<data dir>/cuboid/library/refs/`) holds references shared by every workspace, like the global
scope of Blockbench reference images. If `.cubik` carries no element ids, the workspace also keeps the id map
that makes ids stable across sessions.

## 4. Stages and gates

`ws_status` evaluates every gate and returns the first failing stage with suggested next calls. Gates are data in
the manifest, so the user can relax them or add more.

| # | Stage | Main tools | Gate |
|---|---|---|---|
| 0 | Brief | `ws_init`, `ws_set` | id, namespace, targets and budgets set |
| 1 | References | `ref_add`, `ref_board`, `ref_pin` | at least one front and one side reference, each with source and licence |
| 2 | Blockout | `ref_silhouette`, `model_edit` (`extrude_grid`, `add_cube`), `render_compare` | silhouette IoU against the pinned front reference at or above the gate |
| 3 | Detail | `model_edit`, `render_sheet` | `validate` reports no model errors; elements within budget |
| 4 | UV and texture | `texture_create`, `texture_edit`, `palette_set`, `texture_uv_sheet` | every visible face textured; colours within budget; one texel density |
| 5 | Display and states | `display_set`, `itemdef_set`, `render_ingame` | every context rendered; framing metrics pass |
| 6 | Validation | `validate` (all scopes, every target) | no errors |
| 7 | Review renders | `render_sheet`, `render_turntable`, `render_compare` | images exist for the current revision |
| 8 | Export | `export_pack` | the pack validates for every target; give snippets written |
| 9 | Live test | `game_rig_setup`, `game_test` | pack status `SUCCESSFULLY_LOADED`; every context captured; no model or texture errors in the client or server log |

## 5. Tool conventions

- Names are `<domain>_<verb>`. The catalogue stays near 30 tools so that it fits an agent's context.
- Inputs are JSON Schema 2020-12 with enums for presets and a default for every optional field. Units: model
  pixels (16 = one block), degrees, colours `#RRGGBB` or `#RRGGBBAA`.
- Elements are addressed by id (`e12`) or path (`blade/edge_l`). Selectors accept ids, paths, globs (`blade/**`)
  and filters such as `{"group": "guard", "inside": [[0, 0, 0], [16, 8, 16]]}`.
- Writes take an `ops` list, apply atomically, accept `expected_revision` (refused on mismatch) and `dry_run`
  (validate and preview on a copy). They return `{revision, changed, ids, warnings}`. In the GUI one call is one
  undo step.
- Errors carry a stable `code` (prefix `E_`, `W_` or `I_`), `message`, `hint` and, when the fix is mechanical,
  `fix_ops` the agent can send back unchanged.
- Results go in `structuredContent` with an `outputSchema` and are mirrored as compact text. Images are PNG image
  content plus the saved path under `renders/`. Lists page with `cursor` and `limit`.
- Renders are deterministic: the same revision, camera and size give the same bytes on the same adapter.
- Long jobs (turntables, exports for several targets) send progress notifications.
- Every tool maps onto core commands shared with the UI and the testkit scenarios. The toolkit adds no editing
  logic of its own.

## 6. Tool catalogue

### 6.1 Workspace

| Tool | Input | Output |
|---|---|---|
| `ws_init` | path, id, namespace, targets, template (`sword`, `axe`, `bow`, `gun`, `blank`) | created files, revision 0 |
| `ws_status` | - | stage, gates (pass, detail), next calls |
| `ws_set` | manifest patch | manifest, revision |

### 6.2 References

| Tool | Input | Output |
|---|---|---|
| `ref_add` | local files, scope (`workspace`, `library`), tags, view (`front`, `side`, `top`, `detail`, `mood`), source, licence, notes | ids, thumbnail sheet |
| `ref_list` | tags, view, scope | metadata |
| `ref_board` | ids or tags, columns, size | one labelled contact sheet |
| `ref_pin` | id, target (`view:front`, `view:side`, `view:top`, `uv`), offset, scale, opacity, visible | pin state |
| `ref_silhouette` | id, crop, threshold or alpha, grid size | pixel mask as rows (7.2), bounding box |

`ref_add` reads local paths only; the agent downloads images with its own tools. The licence field is required
(free text, `unknown` allowed) so that a published pack never ships traced third-party art by accident.

### 6.3 Model

| Tool | Input | Output |
|---|---|---|
| `model_outline` | depth, select | tree of groups and elements with ids, paths, bounds |
| `model_get` | select, fields | element records |
| `model_find` | name glob, bounds, texture, face filters | ids |
| `model_stats` | - | counts, bounds, texel densities, texture use, colour count |
| `model_edit` | ops, expected_revision, dry_run | revision, changed, ids, warnings |

`model_edit` ops: `add_group`, `add_cube`, `set` (from, to, rotation, pivot, name, visible), `delete`, `duplicate`,
`mirror` (axis, about), `array` (count, step), `align`, `extrude_grid` (rows, depth, scale; greedy merge into
cubes), `voxel_paint` (rows per layer), `voxels_to_cubes` (greedy), `set_face` (texture, uv, rotation, cull, tint),
`auto_uv` (`per_face` or `box`, density).

### 6.4 Texture

| Tool | Input | Output |
|---|---|---|
| `texture_create` | name, size, fill | texture id |
| `texture_edit` | texture, ops, expected_revision | revision, changed pixels, image of the changed region |
| `palette_set` | name, colours, or `from` (texture or reference, k) | palette |
| `texture_uv_sheet` | texture, scale | image with UV islands, labels and pixel grid |

`texture_edit` ops: `paint_grid` (7.2), `fill_rect`, `replace_colour`, `shade` (preset `bevel`, `edge_light`,
`gradient` or `noise` with a seed, limited to the palette), `quantize` (palette, no dither), `copy_region`,
`import_png`.

### 6.5 Display and item states

| Tool | Input | Output |
|---|---|---|
| `display_set` | context, rotation, translation, scale, or preset (`handheld`, `handheld_rod`, `bow`, `crossbow`, `two_handed`) | display table |
| `itemdef_set` | template or explicit tree | `items/<id>.json` preview per target |

`itemdef_set` templates: `static`; `gui_sprite_hand_model` (a `select` on display context: a flat sprite in the
GUI, on the ground and in item frames, the 3D model in hands and on the head); `pull_stages` (bow-style
`range_dispatch`); `charge` (crossbow); `variants` (`select` on custom model data strings); `cooldown`.

### 6.6 Validation

| Tool | Input | Output |
|---|---|---|
| `validate` | scope (`model`, `texture`, `display`, `itemdef`, `pack`, `all`), targets, self_test | counts, issues |

Issue families: element bounds and rotation rules per target (from the version table); coplanar overlapping faces
(z-fighting); faces without a texture; UV outside the texture; mixed texel density; semi-transparent pixels on
cutout faces; unused textures; colours over budget; elements over budget; first-person framing (clipped, covering
the crosshair); GUI fit; namespace and path rules; files missing from the pack. Each check ships a defect injector,
so `self_test` proves on the current model that the check still fires (the Blockbench MCP headless gates do this).

### 6.7 Render

| Tool | Input | Output |
|---|---|---|
| `render_view` | camera (`front`, `back`, `left`, `right`, `top`, `bottom`, `iso` or explicit), size, style (`flat`, `studio`), background, overlays (`grid`, `refs`, `bounds`, `pivots`) | image |
| `render_sheet` | views, size, labels | one image |
| `render_turntable` | frames, size, elevation, format (`gif`, `apng`, `sheet`) | file, preview frame |
| `render_ingame` | contexts, GUI scales, scene options (9) | one image per context, metrics |
| `render_compare` | a, b (render, reference or file), mode (`side_by_side`, `overlay`, `silhouette`) | image, metrics (IoU, bounding-box delta, palette distance) |

Renders never move the user's cameras; in the GUI they use their own offscreen targets.

### 6.8 Files

| Tool | Input | Output |
|---|---|---|
| `project_open` | path | revision, summary |
| `project_save` | path (default `model.cubik`) | path, bytes |
| `export_pack` | targets, layout (`folder`, `zip` or both), snippets | paths, SHA-1 per zip, pack format per target, give commands |
| `export_model` | format (any interop writer), path | path |

### 6.9 History

| Tool | Input | Output |
|---|---|---|
| `history` | action (`list`, `checkpoint`, `restore`, `undo`, `redo`), name, steps | revision, entries |
| `history_diff` | from, to (revisions or checkpoints) | added, removed and changed elements and textures |

### 6.10 Test rig (10)

| Tool | Input | Output |
|---|---|---|
| `game_rig_setup` | target, memory | server and client paths, versions, downloaded bytes |
| `game_rig` | action (`start`, `stop`, `status`, `reset_world`) | process states, ports, log tail |
| `game_test` | contexts, pack (default: the last export), delivery (`server`, `client_folder`), GUI scale | capture per context, pack status, log issues, diff against `render_ingame` |
| `game_capture` | camera (preset, or position and rotation), size, HUD on or off | image |
| `game_logs` | since, source (`server`, `client`) | parsed pack, model and texture issues |
| `game_baseline` | action (`list`, `propose`), contexts | baseline files; approval stays with the user (10.6) |

## 7. Formats by example

### 7.1 Batch edit

```json
{
  "expected_revision": 41,
  "ops": [
    {"op": "add_group", "path": "blade", "pivot": [8, 8, 8]},
    {"op": "add_cube", "path": "blade/core", "from": [7.5, 4, 7.5], "to": [8.5, 22, 8.5], "texture": "body"},
    {"op": "mirror", "select": "blade/edge_r", "axis": "x", "about": 8, "name": "edge_l"},
    {"op": "auto_uv", "select": "blade/**", "mode": "per_face", "density": 16}
  ]
}
```

```json
{"revision": 42, "changed": {"added": 3, "updated": 2},
 "ids": {"blade": "g3", "blade/core": "e7", "blade/edge_l": "e9"},
 "warnings": [{"code": "W_TEXEL_DENSITY", "ids": ["e9"], "hint": "edge_l maps 8 px per block; the model uses 16"}]}
```

### 7.2 Pixel grid

One character per pixel, `null` for transparent. The result returns an image of the painted region so the agent
can check its own grid.

```json
{"op": "paint_grid", "texture": "body", "at": [0, 0],
 "legend": {".": null, "k": "#1d1b24", "s": "#7b8496", "h": "#d6dde8"},
 "rows": ["..kk", ".ksk", "kshk", "kssk"]}
```

The same grid format feeds `extrude_grid` and comes out of `ref_silhouette`, so a silhouette taken from a reference
becomes a blockout in one call.

### 7.3 Issue

```json
{"code": "E_ROTATION_RULE", "severity": "error", "target": "1.21.6", "ids": ["e12"],
 "message": "guard_l rotates on two axes", "hint": "Java item models rotate each element on one axis",
 "fix_ops": [{"op": "set", "select": "e12", "rotation": [0, 0, 22.5]}]}
```

### 7.4 In-game render result

```json
{"images": [
  {"context": "gui", "scale": 2, "path": "renders/ingame-gui@2-r42.png",
   "metrics": {"slot_fill": 0.81, "clipped": false}},
  {"context": "firstperson_righthand", "path": "renders/ingame-fp_r-r42.png",
   "metrics": {"screen_cover": 0.12, "clipped_edges": [], "crosshair_overlap": false}}]}
```

## 8. Transports

- **stdio (default).** `cuboid mcp --root <dir>` runs the app binary without a window: headless core and renderer.
  One process per agent, so several agents can work on different workspaces at once. Writes are atomic (temporary
  file, then rename) and guarded by revisions and a lock file in the workspace, as in the Blockbench MCP headless
  mode. `cuboid-cli` stays free of UI and GPU.
- **Live GUI.** The app can expose Streamable HTTP on `127.0.0.1`: off by default, enabled in settings, token
  required, `Origin` and `Host` checked against DNS rebinding. Requests queue to the UI thread and run between
  frames as ordinary commands, so the user sees every change and can undo it; a status-bar item shows agent
  activity and a stop button. Responses are plain JSON (no SSE stream), which the transport allows.
- **No MCP client.** `cuboid call <tool> @args.json` runs one tool and prints the result. `cuboid run recipe.toml`
  runs a declarative list of tool calls (no conditions, no loops), for example "render every context, validate,
  export".
- **Resources.** `cuboid://ws/manifest`, `cuboid://model/outline`, `cuboid://ops` (op reference with examples),
  `cuboid://mc/<version>/rules`, `cuboid://refs/<id>`, `cuboid://renders/<name>`.
- **Prompts.** One per stage (`weapon_brief`, `weapon_refs`, `weapon_blockout`, `weapon_detail`, `weapon_texture`,
  `weapon_display`, `weapon_release`), each stating the goal, the gate and the tools to use.

## 9. Emulated in-game renders

`render_ingame` reuses the preview scenes of app 9.5 and the display transforms of the model.

- Contexts: `gui`, `firstperson_righthand`, `firstperson_lefthand`, `thirdperson_righthand`,
  `thirdperson_lefthand`, `ground`, `fixed`, `head`, plus any context the version table adds for a target.
- GUI renders are pixel-exact at slot sizes of 16 × GUI scale (16, 32, 48 px): orthographic, nearest sampling, with
  the model's `gui_light` mode.
- World contexts use Minecraft-style face shading at a fixed light level; first person uses the game's default
  field of view.
- Metrics: slot fill and clipping for the GUI; screen cover, clipped edges and crosshair overlap for first person;
  height above the floor for `ground`.
- Results are labelled "emulated" until the rig's calibration (10.4) shows the scene within tolerance.

## 10. Local test rig

The agent provisions and drives a local server and client through tools; nothing here needs an agent-written
script. A Minecraft server never loads resource packs, so only a client can show whether the model renders; the
rig therefore has both halves. It lives in a folder chosen in settings (it needs room for game files and Gradle
caches) and is reused across workspaces. Stage 9 is skipped while the rig is disabled.

### 10.1 Setup (`game_rig_setup`)

- **Server.** Paper for the target version, downloaded from the official PaperMC download API and checked against
  the SHA-256 in its build metadata. `server.properties`: `server-ip=127.0.0.1`, `online-mode=false`, flat world,
  spawn protection 0, RCON and query off. Game rules stop the daylight, weather and mob-spawning cycles. The
  `cuboid-bridge` plugin is installed.
- **EULA.** `eula=true` is written only after the user has accepted the Minecraft EULA once in cuboid's settings.
  No tool argument can set it.
- **Client.** `cuboid-capture` is a Fabric client game test (`FabricClientGameTest`) started by Loom's
  `ClientProductionRunTask` with `-Dfabric.client.gametest`. Each run boots the client, does one job and exits with
  a pass or fail code, so no client stays running. It uses an offline profile and a fixed window size, and the test
  sets FOV, GUI scale and view bobbing through the client options. `fabricApi.configureTests { eula }` is written
  from the same user setting as the server's EULA flag. No Microsoft account is used or stored; the rig is meant for
  people who own the game. Fallback (D-A3): the user's own launcher profile, captured from the OS (10.6).
- **Java.** The runtime that the version table names for the target.

### 10.2 Test run (`game_test`)

1. Write a job file (pack, item, contexts, delivery) and start one client game test run with it.
2. Render check (`delivery: client_folder`, the default): the test enables the pack from the client's
   `resourcepacks/` and creates a flat singleplayer world, so no server process runs. Delivery check
   (`delivery: server`): the rig starts Paper (10.5) and serves the zip on a loopback port, the test connects to
   127.0.0.1, and the bridge sends the pack with its SHA-1 and waits for `PlayerResourcePackStatusEvent`
   `SUCCESSFULLY_LOADED`.
3. Capture each context with `takeScreenshot` in a fixed setup: hands in first and third person (the item in the
   main hand, in the off hand for left-hand contexts; the test hides HUD layers but keeps the hand, because F1 hides
   the hand too); `gui` from a hotbar slot at the fixed GUI scale; `fixed` from a real item frame; `head` from an
   armour stand; `ground` from an `ItemDisplay` with the ground transform, because dropped items spin and bob.
4. Compare each capture with its approved baseline (`assertScreenshotEquals`) when one exists, and with
   `render_ingame`.
5. Collect pack, model and texture issues from the logs; return captures, issues and diffs.

### 10.3 Components

- **`cuboid-bridge` (Paper plugin, own repository).** A fixed action list, never console or RCON passthrough. Work
  arrives through a file inbox (`plugins/cuboid-bridge/inbox/*.json`, answers in `outbox/`), so it opens no port.
- **`cuboid-capture` (Fabric client game test, own repository).** Reads the job file, builds the scenes, takes
  the screenshots and writes a result file beside them (`build/run/clientGameTest/screenshots/`).

### 10.4 Calibration

Each context's pair of `render_ingame` and `game_capture` measures the emulation error. W7.6 uses the rig as its
oracle, and a scene drops its "emulated" label once the error stays within tolerance.

### 10.5 Process lifetime

Nothing in the rig runs on a schedule, so it needs no cron job. Setup runs once per target. A client test run is
one process that exits by itself; the tool waits for it with a timeout. Paper runs only for delivery checks: cuboid
starts it, waits for its `Done (` start-up line and the bridge's ready file, and stops it after the run or after an
idle timeout. Rig processes are children of the cuboid process (a job object on Windows), so they end with it. A
schedule only helps regression runs over every workspace, which belong in the nightly CI workflow.

### 10.6 Viewing captures

- **Agent.** `game_test` and `game_capture` return PNG image content together with numeric diffs, so the agent
  does not depend on its own reading of an image.
- **User.** The client window is visible during a local run; it must not be minimised, because a minimised window
  has no framebuffer to draw into. The Rig panel in the GUI shows capture, emulated render and diff per context,
  and only the user approves a capture as the new baseline.
- **CI.** Loom runs the client under XVFB (`useXVFB`) on Linux, and the workflow uploads the screenshots.
- **Without the mod.** For the user's own launcher profile, cuboid captures the game window through Windows
  Graphics Capture. The result depends on window size and display scaling, so it never becomes a baseline.
- **Motion.** A test can capture a frame sequence (an `ItemDisplay` turning, an animated texture) that cuboid packs
  into a GIF or APNG.

## 11. Security

- No eval, no scripting language, no shell access.
- Paths resolve inside the workspace roots, the library and the rig folder; symbolic links that leave them are
  refused. The rig folder comes from user settings, never from tool input.
- cuboid downloads only rig files from official sources (the PaperMC API with SHA-256, game files through Fabric
  Loom); references arrive as local files.
- The HTTP transport is off by default, loopback only, with a token and `Origin` and `Host` checks.
- The rig binds to 127.0.0.1 only, which is what makes `online-mode=false` safe. The bridge and the capture mod run
  a fixed action list and never execute commands taken from input.
- Provenance: `refs.toml` keeps source and licence, `journal.jsonl` keeps every write with the agent's name.

## 12. Owner decisions

| Id | Question | Recommendation |
|---|---|---|
| D-A1 | MCP implementation: own synchronous subset (initialize, tools, resources, prompts, notifications) or the official `rmcp` SDK, which needs tokio, confined to one crate | own subset: small, no async in the app, keeps the tokio ban; revisit if protocol churn costs more than the SDK would |
| D-A2 | Tool names: this catalogue, or aliases for Blockbench MCP names | this catalogue; aliases for a few common names only if existing prompts must run unchanged |
| D-A3 | Rig client: one Fabric client game test run per test, or the user's own launcher profile captured from the OS | client game test: deterministic, runs without the user, built-in screenshot comparison; own profile as the fallback |
| D-A4 | Also write reference pins into `.cubik` view background images | workspace only; Cubik does not persist view background images |

## 13. Work packages (proposal, milestone M7)

| WP | Scope | Needs |
|---|---|---|
| W7.1 Command schemas | JSON Schema generated from the core command enums; ids, selectors, dry-run, revisions | inside M2 (14) |
| W7.2 Protocol and CLI | `cuboid-mcp` library crate (L5): stdio transport, tool registry, resources, prompts; `cuboid mcp`, `call`, `run` | W7.1 |
| W7.3 Workspace and references | manifest, references, library, pins, journal, locks | W7.2 |
| W7.4 Validator | issue families of 6.6, per-target rules, defect injectors | M2, W4.x |
| W7.5 Render set | camera presets, sheets, turntables (GIF, APNG), compare metrics | W3.7 |
| W7.6 In-game emulation | `render_ingame` and its metrics | W7.5, app 9.5 scenes |
| W7.7 Texture ops | `paint_grid`, shade presets, quantize, UV sheet | M2 paint core |
| W7.8 Export targets | several targets per export, give and Paper snippets | W4.x Minecraft formats |
| W7.9 Gates and prompts | `ws_status`, stage prompts | W7.3 to W7.8 |
| W7.10 Live GUI transport | HTTP endpoint in the app, UI-thread queue, activity indicator | M6, W7.2 |
| W7.11 Test rig | `game_*` tools and the Rig panel; `cuboid-bridge` (Paper) and `cuboid-capture` (Fabric client game test) in their own repositories | W7.6, W7.8 |

## 14. Constraint on M2

The toolkit stays cheap only if the M2 core already has: commands as serde-serialisable data (no closures); ids
for elements and textures that stay stable for the life of a document; path addressing; transactions that group
commands into one undo step; dry-run on a cloned document; a revision counter; structured errors with stable codes.
Adding these after the UI exists costs far more than building them in. Recorded in `docs/review/integration_backlog.md`.
