# Using cuboid with an AI agent

For people who want an AI agent to work on a cuboid project. The design and its decisions are in
`docs/design/agent_toolkit_r1.md`.

cuboid has a server for the Model Context Protocol (MCP). It lets an AI agent in Claude Code, Claude Desktop or
Codex work on one project folder: read the project brief and the style references, build and edit a model, paint
its textures, check it against the rules of Minecraft Java, look at renders, save it and export it. The agent
uses a fixed set of tools (4). cuboid gives it no shell, no scripting and no network, and it cannot reach a file
outside the project folder (6).

## 1. What the server is

`cuboid mcp --root <folder>` is a mode of the normal `cuboid` program. It opens no window. The AI client starts
it, sends requests on its standard input and reads the answers on its standard output, and the server ends when
the client closes the input. It speaks the MCP revisions 2026-07-28, 2025-11-25 and 2025-06-18.

- The server serves one folder, the project folder (the root). A project is the folder that holds your models
  and, once you use the library or the brief, a `cuboid-library` folder beside them.
- The models the agent works on live in the memory of the server. They reach the disk only when the agent saves
  them, and a server that stops loses what was not saved (7).
- The server and the cuboid window are separate programs. The agent does not edit what is open in the window.
  Save in the window and the agent can open the file; the agent saves a `.cubik` file and you open it in the
  window.
- Everything works without a graphics adapter except the two render tools (7).

## 2. Connect a client

There is no installer yet. The program is the release build of the repository: run
`cargo build --release -p cuboid-app` and take `target\release\cuboid.exe`, which you can copy to a folder of
your own. The examples use a program at `C:\Program Files\cuboid\cuboid.exe` and a project in
`D:\models\lamp`. Use the path of your own program and your own project folder, and quote both as shown. cuboid
never writes the settings of another program: you paste the line for your client yourself. With a saved model
open, the AI clients section at the end of the Project brief dialog shows these lines for its project and the
program that runs, each with a Copy button, so open the model with the copy of the program you want the agent
to use. For a folder that `cuboid mcp` refuses, such as a drive root or a network share, it shows the reason
instead of the lines.

The lines work in PowerShell (the default shell of Windows Terminal), Command Prompt and bash, except the line of
2.1, which is for PowerShell. The `"--"` in the Claude Code and Codex lines is quoted on purpose: every shell
turns it into a plain `--` for the program, but PowerShell drops a bare `--` before it reaches the `claude` and
`codex` of an npm install, which are PowerShell scripts. In PowerShell a `$` or a backtick inside double quotes
is read as PowerShell syntax, so if the path of your folder or program has one, write a backtick in front of
it.

### 2.1 Try it first

```
& "C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp" --check
```

This line is for PowerShell, which needs the `&` to run a quoted program path. In Command Prompt or bash, leave
out the `& `. The AI clients section shows it as Try it. `--check` serves nothing and writes nothing. It reads
the brief of the folder, validates one new cube and draws it, then prints the result of each step, the graphics
adapter in use, and the lines for each client below with the real path of the program that ran. It exits with 0
when every step worked and 2 when one failed. The first lines look like this:

```
ok   brief: 14 library entries, game version 1.21.6
ok   validate: one cube, 2 issues, 0 of them errors
ok   render: a cube drawn at 64 x 64 px
graphics: <adapter name> (Dx12, DiscreteGpu)
```

A line that starts with FAIL says what went wrong. Without a graphics adapter it is the render line, `--check`
exits with 2, and the lines for the clients are still printed: the server works without pictures (7).

### 2.2 Claude Code

Run this in the folder where you start Claude Code. It adds the server to that project, for you only (local
scope):

```
claude mcp add --transport stdio cuboid "--" "C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp"
```

Start Claude Code in that folder and open `/mcp` to see the server.

### 2.3 Claude Desktop

Add one entry per project to `mcpServers` in `%APPDATA%\Claude\claude_desktop_config.json`, then quit Claude
Desktop completely and start it again. The entry is named after the folder, so two projects do not clash.
`--check` prints the path of the file for your system.

To open the file, choose Settings in the Claude menu, then Developer, then Edit Config; it creates the file when
there is none. A file that has no `mcpServers` yet takes the whole block below, inside its outer braces. A file
that has `mcpServers` takes only the `"cuboid-lamp"` entry, inside it and after a comma.

```json
{
  "mcpServers": {
    "cuboid-lamp": {"command": "C:\\Program Files\\cuboid\\cuboid.exe", "args": ["mcp", "--root", "D:\\models\\lamp"]}
  }
}
```

### 2.4 Codex

Run this in a terminal:

```
codex mcp add cuboid "--" "C:\Program Files\cuboid\cuboid.exe" mcp --root "D:\models\lamp"
```

Or add this to `~/.codex/config.toml`. The single quotes make TOML literal strings, so the backslashes need no
escapes, and `tool_timeout_sec` gives slow calls, such as a render that opens the graphics device, 120 seconds
instead of Codex's default of 60.

```toml
[mcp_servers.cuboid]
command = 'C:\Program Files\cuboid\cuboid.exe'
args = ['mcp', '--root', 'D:\models\lamp']
tool_timeout_sec = 120  # the default is 60
```

The name `cuboid` is yours to change, for example to serve a second project.

### 2.5 Stop, restart, remove and update

The client starts the server when it starts and ends it when it closes, so there is nothing to start by hand.

| | Claude Code | Claude Desktop | Codex |
|---|---|---|---|
| Stop | End the session, or choose cuboid in `/mcp` and Disable it | Quit Claude Desktop completely | End the session |
| Restart, after you changed the line or the program | Start a new session; Reconnect in `/mcp` retries a server that stopped | Quit Claude Desktop completely and start it again | Start a new session |
| Remove | `claude mcp remove cuboid` | Delete the entry from `claude_desktop_config.json`, then restart | `codex mcp remove cuboid`, or delete the `[mcp_servers.cuboid]` table from `~/.codex/config.toml` |

A server that runs keeps `cuboid.exe` in use, and Windows does not replace a program that is running. To update
the program, close the clients that use it, copy or build the new `cuboid.exe` to the same path, and open the
clients again. A new path needs a new line.

### 2.6 What to ask

Open the client on the project folder and ask in plain words, for example:

- "Read the project brief, study two of the typical references, then build a lantern block in `lantern.cubik`.
  Validate it, show me a sheet of its views and export it."
- "Open `lamp.cubik`, make the shade two pixels wider, check it against the brief and save it."

## 3. Options

| Option | Meaning |
|---|---|
| `--root <folder>` | The project folder. Required, and given once. It must be an existing local folder with an absolute path; a drive root, a network path and a device path are refused. Every path the agent uses is relative to it. |
| `--read-only` | Refuses every call that would change something: saving, exporting, importing references, editing (a dry run still works), starting a new model, undo, redo and checkpoints. Reading and rendering still work, and a render draws its picture without saving a file under `renders/`. |
| `--check` | Serves nothing. Runs the check of 2.1 and prints the lines for the clients. |
| `--help`, `-h` | Prints the usage and exits with 0. |

Exit codes: 0 when the input ended, which is the normal end of a session, or a check passed; 2 for an option or a
folder that cannot be used, or a check that failed; 1 when reading or writing the streams broke. The message is
on standard error.

Without `--check` the server waits for messages on its input and prints nothing until it gets one, so do not
start it by hand.

## 4. What an agent can do

The server offers 18 tools, in this order.

| Tool | What it does | Changes |
|---|---|---|
| `brief_get` | Reads the project brief: game version, targets, naming and texture rules, the summary of the style references and the library entries, in pages. | nothing |
| `refs_import` | Remembers a folder of the project as style references (5). | `cuboid-library/` |
| `refs_get` | Reads a style reference: the outline of a model, or the size, frames and palette of a texture. | nothing |
| `project_list` | Lists the model files of the project folder, with which are open and unsaved. | nothing |
| `project_open` | Opens a model file, or with `create` starts a new item or block model with the version and display preset of the brief. | nothing on disk |
| `project_save` | Saves the model as a `.cubik` file. | the model file |
| `project_export` | Writes a Minecraft Java model (JSON with its textures) or a resource pack folder for a game version. | files below the export folder |
| `model_outline` | The groups, elements with their faces, textures and display slots of a model, 24 elements a page. | nothing |
| `model_get` | The records of selected elements and groups. | nothing |
| `model_find` | Finds objects by name, group, box, texture, or faces without a texture. | nothing |
| `model_stats` | Counts, bounds, the style numbers of the model, and whether it meets the targets of the brief. | nothing |
| `model_edit` | Runs a list of ops as one batch and one undo step. | the model in memory |
| `texture_paint` | Composes pixels and replaces the picture of a texture in one step. | the model in memory |
| `texture_view` | Shows a texture enlarged with the UV rectangle of every face that uses it. | nothing |
| `validate` | Lists the issues of the validator against the target and the rules of the brief. | nothing |
| `render_view` | A picture of the model, or of a style reference, from one camera. | a PNG in `renders/` |
| `render_sheet` | Up to 9 views in one grid picture. | a PNG in `renders/` |
| `history` | Lists undo steps, undoes, redoes, keeps named checkpoints and restores one. | the model in memory |

The ops of `model_edit` are `add_cube`, `group`, `set`, `move`, `delete`, `duplicate`, `mirror`, `align`,
`reparent`, `set_face`, `auto_uv`, `flip_uv`, `add_texture`, `remove_texture`, `set_display` and `apply_fixes`,
which does the mechanical fixes of the validator. A call that is refused halfway is undone as a whole. The agent
can try a call with `dry_run`, and pass `expected_revision` so that the call fails if something else changed the
model.

Objects are named by an id (`e12` for an element, `g3` for a group), by a path (`head/trim_l`) or by a glob
(`head/**`). Positions are in pixels of a Java model: 16 to a block, y up, z south. Angles are in degrees and
colours are `#RRGGBB` or `#RRGGBBAA`. The camera `front` looks at the north face, which the app calls its Back
view.

The server tells the client how to work, and a typical session goes like this:

1. `brief_get` for the game version, rules, targets and the style summary.
2. `refs_get` and `render_sheet` with `ref` for two or three of the references named in `typical`.
3. `project_open` on the model, or with `create` for a new one.
4. `model_edit` in small batches, then `validate` and `render_sheet` after each stage, looking at the pictures.
5. `texture_paint` and `texture_view` for textures, `project_save` often, `project_export` last.

## 5. Style references

A style reference is a model or a texture that shows the look you want, such as a few finished items of the same
set. The project library keeps them as read-only copies with a licence and tags, and the server tells the agent
what they have in common. Only the library of the project is used; the global library of the app is never read.

Add references in the app:

1. On the start screen, switch the library to Project. The project library sits next to a saved model.
2. In the Project brief card choose Edit, and find the section Style references in the dialog.
3. Type the licence of the files into Licence (the default is `unknown`) and choose Add folder. Pick a folder
   inside the project folder.

The models (`.cubik`, `.cub`, `.bbmodel` and Minecraft model JSON) and textures (`.png`) in the folder are copied
into `cuboid-library/`. The folder itself is not touched. A project keeps up to 16 style folders; they may not
overlap, and a folder inside `cuboid-library` is refused.

Each folder in the list has Rescan and Remove. Rescan reads the folder again: new files come in, unchanged files
are skipped, and a reference whose file changed or is gone is dropped (a changed file comes in again with its new
content). Remove deletes the references of the folder and their copies after a question; the folder is still
not touched. An agent can also import a folder of the project with `refs_import`, with a licence and tags, under
the same rules.

What the agent reads from them:

- `brief_get` gives a summary under `style`, at most 4 KB: how many models and textures there are, element
  counts, element sizes and the share of thin elements, groups, texel density, texture sizes, palette, rotations,
  naming and display presets. `typical` names up to three references nearest the medians, to study first.
- `refs_get` gives the licence, tags, source and style numbers of one reference, and for a model its outline in
  pages: groups, elements with box, rotation and faces, UV rectangles in texels, textures and display transforms.
  For a texture it gives size, frames and palette.
- `render_view` and `render_sheet` with `ref` draw a reference model.
- `model_stats` gives the same numbers for the open model, to compare.

## 6. Limits and safety

**The root.** The project folder is the whole world of the agent. Its paths are relative to the root. A path with
`..`, an absolute path, and a network, device or verbatim form (`\\host\share`, `NUL`, `\\?\C:\`) are refused
with `E_PATH`, and so is a link or junction that leads out of the folder. There is no tool that runs a program,
evaluates code or opens a connection.

**What the server writes.** Models, exports, pictures and the library index are written atomically (a
temporary file, then a rename), so an interrupted write leaves the old file.

| Where | What | Tool |
|---|---|---|
| the model file | a `.cubik` file; a model from another format is saved beside it as `<name>.cubik` | `project_save` |
| `exports/<model>/`, or the folder the agent names | the JSON model and textures, or a resource pack | `project_export` |
| `renders/` | PNG pictures | `render_view`, `render_sheet` |
| `cuboid-library/` | the index, the copies of the references and `brief.md` | `refs_import` |
| `journal.jsonl` | one line per write (below) | every call that writes |

The server deletes none of your files. The one thing it removes is the stored copy of a style reference whose
source file changed or is gone, inside `cuboid-library/`.

**No overwrite without `overwrite`.** `project_export` writes nothing at all when any file it would write is
already there, and answers `E_FILE_CHANGED`; the agent must pass `overwrite: true`, so a second export to the
same folder needs it. `project_save` replaces the model's own file only if that file is as the server last saw
it. A file that changed on disk since, such as one you saved from the window, or any other existing file, is
`E_FILE_CHANGED` unless `overwrite` is true. This also guards a file open in the window or in a second server.

**Read-only references.** The copies under `cuboid-library/` cannot be edited, saved over or exported into. The
agent can open and draw them, and make its own model from what it sees.

**Result size.** An answer stays under about 8,000 tokens, so the agent does not fill its context with one
call. Lists come in pages: the answer carries `next`, and the agent passes it back as `cursor`. A page holds, for
example, 25 library entries, 24 elements or 50 files. A picture sent to the agent is at most 1024 px a side; the
file in `renders/` keeps the size that was asked for.

| Limit | Value |
|---|---|
| A message | 16 MiB |
| Ops in one `model_edit` or `texture_paint` call | 500 |
| A texture | 1024 px a side |
| A `paint_grid` | 512 x 512 |
| A render | 2048 px a side, and no more than the adapter draws |
| Views on a sheet | 9, and the sheet at most 1024 px a side |
| Models open at once | 8 |
| Checkpoints of a model | 16 |
| Steps of one undo or redo call | 50 |
| Style folders of a project | 16 |
| A path | 1024 bytes |

A model with unsaved edits, a new model and a model with checkpoints stay open; a saved model may close to make
room, and loses its undo steps. If none of the 8 can close, opening another is `E_LIMIT` and asks the agent to
save one.

**The journal.** Every call that writes adds a line to `journal.jsonl` in the project folder: a save, an export,
an import of references, an edit that is not a dry run, a new model, an undo, a redo or a checkpoint, and a
render that saved a file. A line holds the time in seconds since 1970, the name the client gave itself, the
tool, the model (or the reference) and the revision. Nothing reads the journal back; it is yours, to see what an
agent did.

```
{"time":1793000000,"client":"claude-code","tool":"model_edit","model":"lamp.cubik","revision":"3fa2c1d0-17"}
```

## 7. Troubleshooting

| What you see | Why | What to do |
|---|---|---|
| The client says the server failed to start, closed, or is not connected | The server refused its command line, printed a message on standard error and exited with 2. Usual causes: a `--root` that is not absolute, no longer exists or is a drive root; a typing error in an option; the program was moved or deleted after the line was added. | Run the check of 2.1 with the folder of the line you added, `& "<program>" mcp --root "<folder>" --check` in PowerShell, read the message, and correct the path in the setting of the client. |
| `render_view` or `render_sheet` answers `E_NO_GPU` | The machine gives cuboid no graphics adapter, not even a software one. `--check` prints `graphics: none (...)` and a FAIL on the render line. | Install or update the graphics driver, or start the client in a session that has an adapter. Every other tool keeps working. On Windows the server draws with DirectX 12 and its software adapter (WARP); set `WGPU_BACKEND`, for example to `vulkan`, in the environment of the server to try another backend. |
| A render answers `E_LIMIT` with "the graphics adapter draws at most N px a side" | The size asked for is above what the adapter takes. | Ask for a smaller size. |
| `E_SESSION`: "the server restarted" | The client sent a revision from an earlier run of the server. Unsaved edits of that run are lost. | The agent opens the model again from its file. Ask it to save more often. |
| `E_REVISION` | The model changed since the agent read it. | The agent reads the model again and repeats the edit. |
| `E_FILE_CHANGED` | The file on disk is not the one the server read or wrote last, or an export would replace files. | Look at what changed the file. If the agent may replace it, tell it to pass `overwrite: true`; otherwise it saves to another path. |
| `E_READ_ONLY` | The server was started with `--read-only`, or the path is under `cuboid-library/`. | Start the server without `--read-only`, or use another path. |
| `E_PATH` | The path leaves the project folder or has a form that is never read. | Use a path relative to the project folder, with no `..`. |
| `E_LIMIT` | A limit of the table in 6 was reached, or 8 models are open and none can close. | Smaller calls, or save a model so that it can close. |
| The agent cannot see the pictures | A render returns the picture and saves the file; whether the client shows it to the agent depends on the client. | The file is in `renders/`, except with `--read-only`. |

The other codes are for the agent and come with a message and a hint: `E_INPUT` (arguments that do not fit the
tool), `E_NO_MATCH`, `E_AMBIGUOUS`, `E_IO` (a file or the library cannot be read or written), `E_EXPORT` (the
target does not take the model) and the codes of an edit that the core refused, such as `E_OUT_OF_RANGE`. A
refused edit leaves the model as it was.

**The log.** The server writes `cuboid.log` in the `logs` folder of the data folder of cuboid: on Windows
`%APPDATA%\cuboid\data\logs\cuboid.log`, on macOS and Linux the `logs` folder of the per-user data folder of
cuboid. Next to a portable install (a file `cuboid.portable` beside the program) it is `data\logs` beside the
program. The file rotates at 5 MiB and keeps three files. The window and every server append to the same file,
and each line of a server carries its process id in square brackets, so sessions can be told apart. The log
records when a server started, the folder it serves and whether it is read-only, the adapter it opened, and its
problems. Warnings and errors also go to standard error, which most clients keep in their own logs.

| Variable | Effect |
|---|---|
| `CUBOID_HOME` | Keeps the folders of a run below one folder: settings in `config/`, the log and recovery files in `data/`. A server with this variable writes its log to `<CUBOID_HOME>\data\logs\cuboid.log` and leaves yours alone. |
| `CUBOID_LOG` | The log level: `error`, `warn`, `info` (the default), `debug` or `trace`. |
| `WGPU_BACKEND` | The graphics backend for the render tools, such as `vulkan`. Without it Windows uses DirectX 12. |
