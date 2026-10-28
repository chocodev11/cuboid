# Agent toolkit R1: an MCP server end users can plug in

Status: 2026-10-27, owner request ("Finish the agent toolkit, with an MCP that end users can use right away. Does
the project brief import a reference model folder yet? I think the agent needs it to work well."). Approved by the
owner on 2026-10-27 with the recommendations of 9, the user guide in `docs/mcp.md` and the order of 8; the test rig
is R2, right after R1. W-R1.1 (library and CLI) is implemented, review `docs/review/agent_toolkit_w1.md`; W-R1.2 is
implemented, review `docs/review/agent_toolkit_w2.md`; W-R1.3 is implemented, review `docs/review/agent_toolkit_w3.md`;
the Style references half of W-R1.5 is implemented, review `docs/review/agent_toolkit_w5a.md`; the other packages are
not. R1 cuts `agent_toolkit.md` (draft of
2026-10-23); what is not named here stays as drafted (10). Answer: no; the library imports folders, but nothing marks
a style example, and an agent sees counts only (4).

## 1. Scope

R1 lets a user add cuboid to Claude Code, Claude Desktop or Codex in one step, and an agent take a model from the
brief to validated, rendered, saved and exported in one project folder: `cuboid mcp --root <folder>`, a stdio
server in the app binary, which has the GPU (`cuboid-cli` stays free of UI and GPU), in both protocol eras (3);
style references (4); 18 tools over existing commands, library, validator and PNG render (5); connect text (6).

| Deferred | Why |
|---|---|
| Live GUI transport (draft 8) | Streamable HTTP, token and `Origin` checks, a UI-thread queue, an activity indicator; the frontend files are in flux (UV editor v2, mode layouts) |
| Test rig (draft 10) | downloads, EULA handling, two outside repositories; nothing in R1 needs it |
| In-game renders (draft 9) | the hand, frame and floor scenes are a stub (`crates/cuboid-render/src/preview.rs`); the GUI slot is nearly free and R1 takes it (5.2) |
| Manifest, stages, gates, picture pins (draft 3, 4, 6.2) | the brief already holds the targets, and `validate` and `model_stats` compare against it; pictures stay library entries |
| Resources, prompts, `cuboid call`, `cuboid run` (draft 8) | tools cover R1; a one-shot call loses unsaved edits, which live in the server process |

## 2. Ground truth: draft 14 against the code

| Need | Today | Where | R1 |
|---|---|---|---|
| Commands as serde data, no closures | yes: plain enums with serde; outcomes are not serde | `crates/cuboid-core/src/commands.rs` | none |
| Stable ids | `ObjId` is session-unique, kept through undo and redo, never persisted; commands name list positions (`Target::Element(usize)`); textures go by label | `crates/cuboid-model/src/{ids.rs,list.rs}`, `commands.rs` | ids `e<n>`, `g<n>`, `m<n>` from `ObjId`, turned into positions just before each command (`Core::target_of`, `object_of`) |
| Path addressing | none; labels repeat (`Rename` gives every target one name) | `crates/cuboid-model/src/groups.rs` (`parent_of`, `ancestors`) | resolver in the server; a path that matches twice is `E_AMBIGUOUS` |
| One undo step per transaction; dry run | `execute_merged` shares a step, but it snapshots only the first command's categories (`apply_fixes` works around it with two gestures); no dry-run API, but `Document` clones copy-on-write and `Core::with_document` exists | `crates/cuboid-core/src/core.rs` (`run_edit`), `crates/cuboid-app/src/validation.rs` | new batch API (W-R1.2); dry runs on a scratch core |
| Revision counter | `Document::revision` is a process-wide `Rev` stamp: new on every edit, undo, redo and `set_cameras`, never persisted, back to 1 in a new process | `crates/cuboid-image/src/contract.rs`, `core.rs` | token `<session>-<rev>` (5.1) |
| Stable error codes | `Refusal` has messages only, `Failed` a string; validator codes are stable | `commands.rs`, `crates/cuboid-validate/src/codes.rs` | `Refusal::code()` (W-R1.2) |
| Command schemas; scenario vocabulary | no schema generation (no schemars); the scenario DSL is a skeleton without a `Driver` or `tests/scenarios/*.ron` | `crates/cuboid-testkit/src/scenario.rs` | hand-written schemas for the R1 ops (5.3), not for raw `Command`; a scenario driver over them later |
| Headless render | `Headless` draws whole UI frames; `Renderer::start_png_render` needs no window, but its job is built in `Frontend` from the window's extractor and scene index | `crates/cuboid-shell/src/headless.rs`, `crates/cuboid-render/src/offscreen.rs`, `crates/cuboid-app/src/frontend/render_job.rs` | offscreen renderer in the app behind a trait (W-R1.4) |
| Check without the app | loader and check sit in the CLI binary; target, profile and fix mapping again in the app; presets are app-private | `crates/cuboid-cli/src/{doc.rs,check.rs}`, `crates/cuboid-app/src/{validation.rs,preset.rs}` | loader and check move to the library (W-R1.1), fix mapping to `cuboid-mcp` (W-R1.3); presets stay |
| Texture edits | whole pictures only (`CreateTexture`, `AddTexture`, `ReplaceTexture`); painting is an interactive tool | `commands.rs`, `crates/cuboid-core/src/tools/paint.rs` | `texture_paint` composes pixels and sends one `ReplaceTexture` |
| `cuboid mcp` entry | `Args::parse` takes any word as a file to open; no `windows_subsystem` attribute | `crates/cuboid-app/src/{lifecycle.rs,main.rs}` | subcommand before the window (W-R1.4) |
| Crate layer | the backlog puts `cuboid-mcp` on L5, where rule 1 forbids its use of `cuboid-library` (L5) | `xtask/src/layering.rs`, `docs/review/integration_backlog.md` 2 | `cuboid-mcp` on L6, `cuboid-app` on L7 (D-R3) |
| Reference models | no role; `Kind::Reference` means pictures; model metadata is counts and bounds; Minecraft JSON is read by hand, without groups or display | `crates/cuboid-library/src/{store.rs,kind.rs,meta.rs}` | 4 |

## 3. Protocol

Read on modelcontextprotocol.io (2026-07-28: versioning, base protocol, stdio, tools, `server/discover`,
cancellation, deprecated features; 2025-11-25 lifecycle; 2025-06-18 changelog). The current revision has no
handshake: requests carry protocol version and client capabilities in `_meta` (else -32602), `server/discover` is
mandatory, results carry `resultType`, and state across requests is named by an identifier the client passes. Up to
2025-11-25 sessions open with `initialize`; Claude Code has a legacy v1 runtime and a v2 one that negotiates
2026-07-28, so R1 serves both eras, which the revision allows.

| Item | R1 |
|---|---|
| Framing | one JSON-RPC message per line, UTF-8, no embedded newline; stdout carries messages only, logs go to stderr and the log file; end of stdin ends the process; a line over 16 MiB is refused (-32600) and skipped, so is a JSON array (batching was removed in 2025-06-18) |
| Modern | `server/discover`: `supportedVersions` 2026-07-28, 2025-11-25, 2025-06-18; capabilities `{"tools": {}}`; `serverInfo` in `_meta`; `instructions` (5.4); an unknown version gets -32022 with the list; the results of `server/discover` and `tools/list` carry `ttlMs` and `cacheScope` (required by the caching page) |
| Legacy | `initialize` answers the client's version when it is 2025-11-25 or 2025-06-18 (the first with structured tool output), else 2025-11-25; `notifications/initialized` is accepted; `ping` (the request list of 2026-07-28 has none, so a modern `ping` is -32601) |
| Both eras | `tools/list` in one page and fixed order; `tools/call` with `inputSchema`, `outputSchema`, `structuredContent` plus the same JSON as text, PNG image content; tool failures are results with `isError: true`; protocol errors are -32700, -32600, -32601, -32602 (also an unknown tool) and -32022 only |
| Left out | progress, resources, prompts, subscriptions, completion, sampling, elicitation, roots, logging (deprecated in 2026-07-28); `notifications/cancelled` is read and ignored, as a server may for what it cannot cancel (one thread, short calls) |

Implementation: an own synchronous subset in `cuboid-mcp` (`serde_json` values, `BufRead::read_line`, one thread
that also drives the GPU, about fifteen message shapes). `rmcp` 3.8.0 (crates.io, 2026-10-26) requires tokio; the
other server crates found (rust-mcp-sdk 2.0, pmcp, tiny_mcp_server, mcp_daemon, techne) say they are async on tokio
(descriptions read, code not audited). `rust-mcp-schema` 2.0.0, serde-only types generated from the official
schema, may check the output in tests (licence not checked). The tokio ban stays; `base64` is added for images.

## 4. Style references in the brief

4.1 Decision. R1 takes (b) below and remembers each folder as a style source with Rescan, which gives most of the
freshness of (a); Blockbench (behaviour only) keeps a reference picture's path while working and embeds it on save.
The role is `style`, since the kind `reference` already means pictures.

| | (a) linked folders, read in place | (b) library entries with a role |
|---|---|---|
| Project moved or shared | breaks: the paths point elsewhere | travels with `cuboid-library/` |
| Licence and source | one per folder | per entry, as today |
| What the server may read | grows past `--root` | stays inside the project |
| Changes in the source | seen at every read | seen after Rescan |
| Cost of a brief read | walk and parse every model each time (`cuboid-cli brief` writes nothing, so no cache) | parsed once, at import |

4.2 Schema 3 of `library.json`: `Entry.role` (`work`, the default, or `style`; models and textures), `ModelMeta.style`
(4.3), `Brief.style_sources` (at most 16 `{path, licence, tags, scanned}`, relative inside the project). Schema 1
and 2 read with role `work` and save as 3; older builds refuse 3 with `LibraryError::Newer`, as with 2, instead of
dropping the new fields on save. `ImportOptions` gains `role`. Rescan imports the folder again (SHA-256 skips
unchanged files) and drops the `style` entries whose source path is in the folder but no longer holds that
content. Removing a source removes its entries and copies, never the folder.

4.3 Per model. In the index, a few hundred bytes: groups and depth; longest element side (min, p25, median, p75,
max px); thin elements (a side of 1 px or less); rotated elements and angles; face densities (min, median, max); up
to 16 colours of embedded textures; up to 8 name tokens; the display preset the eight slots match, else `custom`;
a Minecraft JSON's parent. Minecraft JSON now goes through the `cuboid-fmt-minecraft` importer (groups, display; no
textures read, so no density). On demand, the outline (`refs_get`, `model_outline`, `cuboid-cli outline`): groups
(path, pivot, children), elements (path, from, to, rotation, pivot, enabled faces with texture, UV, UV rotation),
textures and the eight display transforms. Java model space (16 px a block, y up, z south), rounded to 1/1000; UVs in
texels of the texture with v down, as `texture_view` shows them and `set_face.uv` takes them; whole numbers without
`.0` and a face as `"north": [texture, u0, v0, u1, v1]` plus the UV rotation when it is not 0. 24 elements a page: a
page of the worst elements (six faces, a rotation, long names) is 9.3 KB, the first page with its groups 10.6 KB, about
3,500 tokens at 3 bytes a token (128 elements in the first form were 57 KB). The groups, with their children, come
whole on the first page, so `model_outline` cuts them with `depth`. Each item carries the node of its element or
group, which `model_outline` turns into `e<n>` and `g<n>`; one `outline(&Document)` in `cuboid-library` serves all
three.

4.4 Style summary, computed from the stored metadata at each brief read, at most 4 KB (invented values):

```json
"style": {"models": 14, "textures": 6, "elements": {"min": 6, "median": 24, "max": 61},
  "element_size": {"p10": 1, "median": 3, "p90": 9, "thin_share": 0.18}, "groups": {"median": 3, "depth": 2},
  "texel_density": {"p10": 16, "median": 16, "p90": 32, "near_median_share": 0.83},
  "texture_sizes": {"16": 3, "32": 9, "64": 2}, "palette": ["#2b2d42", "#8d99ae", "#edf2f4", "#ef233c"],
  "rotation": {"share": 0.07, "angles": [22.5, 45]}, "naming": {"case": "snake", "tokens": ["handle", "trim"]},
  "display": {"item/handheld": 11, "custom": 3}, "typical": ["3fa2c1d09b7e4410", "91c0e2aa5d3b7f02"]}
```

`typical` lists up to three references nearest the medians, to open first. The palette uses `shared_palette`, the
near-median share `DENSITY_TOLERANCE` (`brief.rs`); `model_stats` gives the same fields for the open model.

4.5 Command line. `cuboid-cli brief --json` output version 2: entries carry `role`; the `meta` of a model leaves
out its `style` (the summary says what the references share, the index keeps the style of each and `refs_get` gives
it), which cuts a reference from about 1.1 KB to 450 bytes; 300 references are still 133 KB, so `brief_get` pages
`entries` (W-R1.3, about 25 a page for a result under 8,000 tokens); new keys `style` and `style_sources`; a version 1
reader loses nothing. The `bounds` in `meta` are in document space, with z mirrored against the Java model space of
the outline. New `cuboid-cli outline [--json] [--limit N] <model>`: the outline (4.3) of any model file, stored
copies included, 24 elements by default and 0 for all; writes nothing. `brief.md` gains `## Style references` after
Rules: sources, up to 30 references (name, format, elements, density, texture size, display), the summary as pairs.

4.6 App. The Brief dialog (`crates/cuboid-ui/src/brief.rs`) gets a section Style references after Rules: per source
the path, "12 models, 4 textures", the licence, Rescan and Remove (which asks "Remove the 16 style references
imported from this folder? The folder itself is not touched."); then a Licence field (default `unknown`) and Add
folder (picker, then an import job). The library panel shows a Style chip, the entry menu "Use as style reference"
and "Use as own work", the start card "Style: 12 models from 2 folders". Project library only (D-R5).

## 5. Tools

5.1 Conventions (changes to draft 5).

- Each model tool takes `model`, a path relative to the root: the explicit handle the stateless revision asks for.
  It opens from disk on first use; at most 8 stay open. A model closes only when it has a file, no checkpoints and
  no unsaved edits, those without undo steps first; when none can close, `E_LIMIT` asks to save one.
- `revision` is `<session>-<rev>`, the session random per process: a token from another process is `E_SESSION`
  ("the server restarted; unsaved edits are lost"), an older one `E_REVISION`.
- Selectors: an id (`e12`, `g3`; for the process), a path (`head/trim_l`; across sessions), a glob (`head/**`) or a
  list; no match is `E_NO_MATCH`, never the selection. Writes take `ops`, `expected_revision` and `dry_run`, run as
  one batch and one undo step, and return `{revision, changed, ids, warnings}`; a dry run uses a scratch core.
- Errors: `isError: true` with `{code, message, hint}`; the codes of a command that was not done: the 21 of
  `Refusal::code()` (`E_NOTHING_SELECTED`, `E_NO_ELEMENTS`, `E_BAD_TARGET`, `E_NOTHING_TO_UNDO`, `E_NOTHING_TO_REDO`,
  `E_CLIPBOARD_EMPTY`, `E_NOT_DELETABLE`, `E_NOT_FINITE`, `E_OUT_OF_RANGE`, `E_ROTATION_RESTRICTED`, `E_NO_GROUPS`,
  `E_GROUP_TOO_MANY`, `E_GROUP_TOO_DEEP`, `E_GROUP_CYCLE`, `E_GROUP_MISSING`, `E_GROUP_ATTACHED`,
  `E_ANIMATION_TOO_MANY`, `E_ANIMATION_TOO_MANY_KEYFRAMES`, `E_ANIMATION_MISSING`, `E_ANIMATION_OUT_OF_RANGE`, `E_OP`),
  `E_NOT_YET` and `E_FAILED`; and the codes of the server: `E_INPUT` (arguments that do not fit the schema), `E_PATH`,
  `E_READ_ONLY`, `E_REVISION`, `E_SESSION`, `E_NO_MATCH`, `E_AMBIGUOUS`, `E_FILE_CHANGED`, `E_LIMIT`, `E_NO_GPU`, `E_IO`
  (a file or the library cannot be read or written) and `E_EXPORT` (the target does not take the model).
- Minecraft model space as in a Java model (16 px a block, y up, z south), degrees, `#RRGGBB(AA)`; z flips in core.
- Results stay under about 8,000 tokens (Claude Code warns at 10,000, cuts at 25,000), lists page with `cursor`;
  pictures are PNG content of at most 1024 px plus the file in `<root>/renders/` (image support per client not
  verified). Each write appends a line to `<root>/journal.jsonl` (time, client name, tool, model, revision).

5.2 Catalogue.

| Tool | Input | Output | Uses | New core work |
|---|---|---|---|---|
| `brief_get` | - | brief JSON version 2 (4.5) | `Library::brief_json` | no |
| `refs_import` | folder in the root, licence, tags | added, skipped, ids | `Library::import`, role `style` | no |
| `refs_get` | id, cursor, limit | outline of a stored reference; texture size, frames, palette | `outline` | no |
| `project_list` | glob, cursor, limit | model files: format, size, open, unsaved, revision | bounded walk with the import rules | no |
| `project_open` | model, `create` (`item`, `block`) | revision, counts, bounds, textures, read-only | library loader; a new model takes version and display preset from the brief | no |
| `project_save` | model, path, expected_revision, overwrite | path, bytes | `cuboid_fmt_cubik::save_path`; other formats save as `<stem>.cubik` | no |
| `project_export` | model, format (`java_model`, `resource_pack`), folder, version, overwrite (refused without it when a target exists) | paths, pack format, warnings | `JsonExport`, `export_pack` with the export dialog's options | no |
| `model_outline` | model, select, depth, cursor, limit | tree with ids and paths (4.3) | `outline` | no |
| `model_get` | model, select, fields | element records with faces | `Document` | no |
| `model_find` | model, name glob, group, inside box, texture, untextured faces | ids, paths | `Document` | no |
| `model_stats` | model | counts, bounds, the fields of 4.4, brief targets met or missed | library style functions | no |
| `model_edit` | model, ops (5.3), expected_revision, dry_run | revision, changed, ids, warnings | core commands in one batch | batch, `Refusal::code` |
| `texture_paint` | model, texture, ops (`paint_grid` of draft 7.2, `fill_rect`, `import_png`), expected_revision, dry_run | revision, picture of the changed region | `ReplaceTexture`, `AddTexture` | batch |
| `texture_view` | model, texture, scale, select (the faces' objects) | the texture enlarged with its faces' UV rectangles, fitted to 1024 px first, rectangles as data | CPU drawing in the server | no |
| `validate` | model, target (`block`, `item`, `pack`), self_test | counts; issues with code, severity, ids, paths, message, hint, fix | the check of `cuboid-cli check`, `validate_self_test` | no |
| `render_view` | model or ref, camera (`front`, `back`, `left`, `right`, `top`, `bottom`, `iso`, `gui`), size, background | picture, path, pixel bounds, coverage | PNG render path, `snap_view`, `frame_bounds`, `anchor_transform`, `ItemLight` | no |
| `render_sheet` | model or ref, views, size | the views in one grid picture, order as data | `render_view` | no |
| `history` | model, action (`list`, `undo`, `redo`, `checkpoint`, `restore`), name, steps | revision, entries | `Core::undo`, `redo`; checkpoints are snapshots in memory | restore as one step |

`camera: "gui"`: the `gui` slot, orthographic, GUI light, `emulated: true`. No adapter: render tools give `E_NO_GPU`.

5.3 `model_edit` ops.

| Op | Fields | Commands |
|---|---|---|
| `add_cube` | name, from, to, parent, texture | `AddElement(Cube)`, `SetPosition`, `SetSize`, `Rename`, `Reparent`, `AssignTexture` |
| `group` | select, name, pivot | `Group`, `SetGroup` |
| `set` | select; name, from, to, rotation, pivot, visible, locked | `Rename`, `SetPosition`, `SetSize`, `SetRotation`, `SetPivot`, `SetHidden`, `SetLocked` |
| `move`, `delete`, `duplicate`, `mirror`, `align`, `reparent` | select; by, axis, copy, edge, parent, index | `MoveBy`, `Delete`, `Duplicate`, `Mirror`, `Align`, `Reparent` |
| `set_face`, `auto_uv`, `flip_uv` | select, faces; texture, uv, uv_rotation, enabled, cull, tint, style, axis | `AssignTexture`, `SetUvRect`, `SetFaceEnabled`, `SetFaceCull`, `SetFaceTint`, `AutoUv`, `FlipUv` |
| `add_texture`, `remove_texture`, `set_display` | name, size, fill; context, rotation, translation, scale, preset | `CreateTexture`, `RemoveTexture`, `SetDisplaySlot` |
| `apply_fixes` | codes (default: every fixable issue) | the validator's fix ops, mapped as the Validator panel maps them |

A later op may name, by path, what an earlier op of the batch created. Not in R1, since the core has no command
for them: `array`, `extrude_grid`, `voxel_paint`, `voxels_to_cubes`, mirroring about a given plane.

5.4 Instructions (about 1 KB): read `brief_get`; study two or three `typical` references with `refs_get` and
`render_sheet`; edit in small batches; `validate` and `render_sheet` after each stage; save often; export last.

## 6. Connecting a client

6.1 `cuboid mcp --root <folder> [--read-only] [--check]`: `--root` is required and absolute (clients start servers
in different folders), a drive root is refused; `--read-only` refuses every file write; `--check` serves nothing,
reads the brief, validates and renders one cube, prints the result and the snippets below, and exits 0 or 2.

6.2 Snippets with example paths (read 2026-10-27 on code.claude.com, modelcontextprotocol.io and the Codex MCP
page): Claude Code, run in the project (local scope); Claude Desktop, one entry per project in `mcpServers` of
`%APPDATA%\Claude\claude_desktop_config.json`, then a restart; Codex, the command or `~/.codex/config.toml`.

```
claude mcp add --transport stdio cuboid -- "C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp"
"cuboid-lamp": {"command": "C:\\Program Files\\cuboid\\cuboid.exe", "args": ["mcp", "--root", "D:\\models\\lamp"]}
codex mcp add cuboid -- "C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp"

[mcp_servers.cuboid]
command = 'C:\Program Files\cuboid\cuboid.exe'
args = ['mcp', '--root', 'D:\models\lamp']
tool_timeout_sec = 120  # the default is 60
```

6.3 In the app. The Brief dialog gets a last section, AI clients: the project folder, each snippet in a read-only box
with a Copy button, and the `--check` line. It sits there because the root is the project folder and Preferences is
drawn from `ui.rs`, owned by the mode layouts package. cuboid never writes another program's files.

6.4 Release: `cuboid.exe` with the subcommand, `cuboid-cli.exe`, the user guide `docs/mcp.md` with the snippets (the
README links to it once the README changes of the logo work are committed). A Claude Desktop
bundle (`.mcpb`, a zip with a manifest of server type `binary` and a folder setting; details to check then) would
make Desktop a double click; it belongs to `cargo xtask package`, which prints "not implemented yet" (D-R4).

## 7. Security (draft 11 for R1)

- No eval, scripting, shell or network: no socket, only stdin.
- Paths are relative to the root; `..`, network, device and verbatim forms (`cuboid_io::texture_files::form`) and
  absolute paths are `E_PATH`; paths are canonicalised and must stay under the canonical root, so links and
  junctions out are refused; a new file is checked through its canonical parent.
- Writes: model, texture, export, render, journal and library files only, atomic (`write_atomic`); nothing is
  deleted but the copies of removed style entries; a save over a file changed on disk since opening is
  `E_FILE_CHANGED` unless `overwrite`, which also guards a file open in the GUI or in a second server.
- Style references are read-only: files under `cuboid-library/` cannot be saved.
- Limits: 16 MiB a message, 500 ops a batch, grids 512 x 512, textures 1024 px, renders 2048 px and the adapter
  limit, 9 views a sheet, 8 open models, 16 checkpoints, the format and import limits; inputs refuse unknown fields
  and numbers pass `Command::validate`. Provenance: licence and source per style entry, the journal.

## 8. Work packages

| WP | Scope | Files | Tests |
|---|---|---|---|
| W-R1.1 Library and CLI | role, schema 3, style sources, style metadata, outline, summary, brief JSON 2, `brief.md` section; loader and check move from the CLI into the library; `cuboid-cli outline` | `crates/cuboid-library/{Cargo.toml,src/**,tests/**}`, `crates/cuboid-cli/src/{main.rs,check.rs,doc.rs,outline.rs}`, `crates/cuboid-cli/tests/{brief.rs,check.rs,outline.rs}` | schema 2 reads as 3; `Newer(4)`; import with role; Rescan drops a changed file's old entry; source removal; summary under 4 KB for 1,000 synthetic references; outline golden of a synthetic model (groups, rotation, faces, display); Minecraft JSON through the importer; `brief.md` golden; `check` output unchanged |
| W-R1.2 Core batch | batch API: snapshot the union of the commands' categories, run them without own entries, push one entry with the tool's label, roll back on the first refusal; a restore step; `Refusal::code()` | `crates/cuboid-core/src/{core.rs,commands.rs,history.rs,lib.rs}`, `crates/cuboid-core/tests/batch.rs` (new) | a move plus a texture removal undoes and redoes as one step; a refusal mid-batch leaves document and history as they were; restore is one step; a dry run on a clone keeps the revision; codes distinct |
| W-R1.3 `cuboid-mcp` | new crate on L6: protocol (3), paths, model handles, the 18 tools, schemas, ops (5.3), the fix mapping taken from `validation.rs` (W-R1.4 drops the app's copy), the `Renderer` trait and the sheet, snippets, self-check | `crates/cuboid-mcp/**` (new), `Cargo.toml` (workspace entries `cuboid-mcp`, `base64`), `xtask/src/layering.rs` | legacy and modern transcripts from the spec examples; missing `_meta`, unknown version, arrays, long lines; every schema example deserialises; path escapes (`..`, absolute, junction, device names); revision, session, dry run; `--read-only`; `validate` equals `cuboid-cli check --json`; result sizes; render tools on a fake renderer |
| W-R1.4 App | `cuboid mcp` in `lifecycle.rs` before the window, nothing but messages on stdout; offscreen renderer that keeps its device and may use a software adapter; the framing of `render_job.rs` as a function both use; `validation.rs` on the moved mapping and the batch | `crates/cuboid-app/{Cargo.toml,src/main.rs,src/lifecycle.rs,src/validation.rs,src/frontend/render_job.rs}`, `src/mcp.rs`, `src/mcp_render.rs`, `tests/mcp_stdio.rs` (new) | the binary on piped stdio: legacy and modern runs, only JSON lines on stdout, exit well under 300 ms after the end of input (Claude Code on Windows ends the server 300 ms after closing stdin); a known cube renders the expected pixels (skipped without an adapter, like the render tests); `--check`; Validator fixes stay one undo step |
| W-R1.5 UI | Brief dialog sections Style references and AI clients; Style chip and role actions in the library panel; the card line; the app side of the new library requests | `crates/cuboid-ui/src/{brief.rs,library.rs}`, `crates/cuboid-ui/tests/{library_ui.rs,brief_ui.rs}`, `crates/cuboid-app/src/{library.rs,requests.rs}`, `crates/cuboid-app/src/frontend/library_tests.rs` | headless egui: add, rescan and remove send their requests, remove after the question; the snippets show the real paths and Copy outputs the exact text; role actions; review screenshots |
| W-R1.6 Documents | the user guide `docs/mcp.md` (connect, the tools, style references, limits); architecture crate list and layers; roadmap; backlog items 1 and 2 closed; status lines of `agent_toolkit.md` and `brief_v2.md` 7 | `docs/mcp.md` (new), `docs/design/{architecture.md,roadmap_lean.md,agent_toolkit.md,brief_v2.md}`, `docs/review/integration_backlog.md` | the snippets of `docs/mcp.md` equal the `--check` output |

Order: W-R1.1 after the UV editor v2 commit, beside the mode layouts package (they share no file); W-R1.2 after
W-R1.1; W-R1.3 after both; W-R1.4 after W-R1.3; W-R1.5 after W-R1.4 (snippets); W-R1.6 last. While two packages
run at once, each checks only its own crates (`cargo fmt -p`, `cargo clippy -p`, `cargo test -p`) and the full
gates run once both are done. `Cargo.lock`, the one shared file, changes
in W-R1.1, W-R1.3, W-R1.4 in that order. No package touches the `mode_layouts.md` files. Gates for each: fmt, clippy
`-D warnings`, workspace tests, `cargo xtask layering`, `ledger`, `ui-style`, the seven private oracles. Not verified
here: a whole model drawn from a fresh extract (render tests draw cubes so); stdio under `windows_subsystem`.

## 9. Owner decisions

| Id | Question | Recommendation |
|---|---|---|
| D-A1 | Own synchronous subset or `rmcp` with tokio | own dual-era subset (3); keep the tokio ban |
| D-A2 | Tool names | the R1 names of 5.2; no Blockbench MCP aliases |
| D-A3, D-A4 | Rig client; picture pins in `.cubik` | not needed for R1; decide with the rig and the picture tools |
| D-R1 | When | build R1 now, before the first release; R2, right after it, is the test rig (draft 10, W7.11) with what it needs of W7.6 and W7.8; the rest of the toolkit stays in L7 |
| D-R2 | How a project points at reference models | library entries with role `style` and remembered source folders with Rescan (4.1); no linked folders |
| D-R3 | Layers | `cuboid-mcp` on L6; `cuboid-app` moves to L7 |
| D-R4 | Claude Desktop bundle | a `.mcpb` with the release packaging; R1 itself gives the snippets |
| D-R5 | Global library | R1 reads the project library only, so the server never reads outside the root |

## 10. What R1 changes in agent_toolkit.md

Section 3: the project folder is the workspace, its brief the manifest; `renders/` and `journal.jsonl` stay, the
rest waits. 4 waits. 5 is amended by 5.1 (`apply_fixes` replaces `fix_ops`). 6 becomes 5.2 (`ws_*` to `brief_get`
and `project_list`, `ref_*` to `refs_*` for models, exports to `project_export`, `texture_uv_sheet` to
`texture_view`); 7.1 to 7.3 hold with these names. 8 is stdio in both eras; 9 the GUI slot; 10 waits; 11 to 14 give
way to 7, 9, 8 and 2; W7.6 and W7.9 to W7.11 stay. `brief_v2.md` 7 maps onto `brief_get`, `refs_get`, `validate`.
