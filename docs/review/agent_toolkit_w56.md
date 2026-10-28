# Review: agent toolkit R1, W-R1.5 AI clients and W-R1.6 Documents

Status: 2026-10-28. Reviewer: Opus. Scope: the uncommitted second half of W-R1.5 of `docs/design/agent_toolkit_r1.md`
(the AI clients section: `crates/cuboid-ui/src/{brief.rs,library.rs,lib.rs}`, `crates/cuboid-app/src/library.rs`, the
tests in `crates/cuboid-ui/tests/{brief_ui.rs,screens.rs}` and the app, the screenshots `dialog_brief-clients*`) and
W-R1.6 (`docs/mcp.md`, `crates/cuboid-mcp/tests/guide.rs`, `docs/design/{architecture.md,roadmap_lean.md,
agent_toolkit.md,brief_v2.md}`, `docs/review/integration_backlog.md`, the Status of the spec); `connect.rs` of W-R1.3
as the section and the guide show its lines. Run: `brief_ui` (15 pass), the app test of the view, the guide test
(3 pass), clippy `-D warnings` on all targets of `cuboid-ui`, `cuboid-mcp` and `cuboid-app`, `cargo fmt --check` of the
three. Probes in the scratchpad (deleted after): the release binary's `--help`, `--check` and drive root with
`CUBOID_HOME` set, and Windows PowerShell 5.1 scripts that pass the lines to Git's `printf.exe`, directly and through a
copy of the npm shim's forwarding. Not run: workspace tests, the gates, `claude mcp add` (it writes the user's config).

Verdict, W-R1.5 AI clients: not ready; two majors, both small, both about Windows PowerShell, the shell Windows
Terminal opens by default: the Claude Code and Codex lines lose their `--` when the client is an npm install, and the
Try it line is a parse error. The lines are otherwise exact, Copy gives the exact text, and the section follows the
document. Verdict, W-R1.6: ready after the same two fixes reach the guide and one minor addition for first-time users;
every command, option, path, tool, code, limit and behaviour of `docs/mcp.md` matches the code, and the other doc
changes are true to the code and the history.

## Major

1. `crates/cuboid-mcp/src/connect.rs:58`, `:64`, shown by `crates/cuboid-ui/src/brief.rs:658` and `docs/mcp.md:63`,
   `:87`. On this machine `claude` resolves in PowerShell to `%APPDATA%\npm\claude.ps1` (Get-Command lists the script
   before `claude.cmd`), which forwards `$args`. PowerShell takes a bare `--` as the end of its own parameters when it
   calls a script, so it never reaches `$args`. Probed with a copy of that forwarding: the Claude Code line arrived as
   `mcp add --transport stdio cuboid C:\Program Files\cuboid\cuboid.exe mcp --root D:\models\lamp`; a direct call of a
   native program keeps the `--`. Claude's usage says subprocess flags go after `--`, so `--root` meets its option
   parser (what it then does was not run). Codex from npm has the same kind of shim. Fix: write the separator quoted,
   `"--"`, in both snippets: PowerShell passes a quoted `"--"` to the script (probed), and Command Prompt with the
   program's own argument parsing, and bash, give `--`. Change the example of spec 6.2 and the guide with it; the guide
   test then pins it. The snippets are W-R1.3 code; the spec example needs the owner's word.
2. `crates/cuboid-app/src/library.rs:1416`, captioned at `crates/cuboid-ui/src/brief.rs:676` "Run in a terminal".
   PowerShell reads a line that starts with a quoted string as an expression: the probe of
   `"C:\...\echo.exe" mcp --root "D:\models\lamp" --check` failed with "Unexpected token 'mcp' in expression or
   statement"; with `& ` in front it ran. So the Try it line fails for every user who pastes it into Windows Terminal.
   The guide's troubleshooting (`docs/mcp.md:264`, "Run the same line in a terminal with `--check` added") leads to the
   same error, and for a Claude Code line it reads as adding a server with `--check`. Fix: build the line once in
   `cuboid-mcp` as `Snippets::check` (see choice 3): on Windows `& "<program>" mcp --root "<folder>" --check` with the
   caption "Run in PowerShell", elsewhere `"<program>" mcp ...`. Guide 2.1 shows that line in place of its `cd` and
   `.\cuboid.exe` (whose `cd` does not change the drive in Command Prompt), the guide test compares it, and the
   troubleshooting row says "Run the check of 2.1 with the folder of the line".

## Minor

3. `crates/cuboid-app/src/library.rs:1239`, `:1407`. The section offers lines for folders that `cuboid mcp` refuses.
   A model saved as `D:\lamp.cubik` gives the folder `D:\` and `--root "D:\"`: the release binary answers "a drive root
   cannot be a project folder" with exit 2, and in bash, or in a program's parsing of a Command Prompt line, `\"`
   escapes the closing quote. A model on a share gives `--root "\\nas\models\lamp"`: "network paths are not read".
   A model opened from a terminal as `cuboid lamp.cubik` keeps its relative path (`lifecycle.rs:45`, the open flow
   does not change it), the project library is `cuboid-library`, whose parent is the empty path, so Folder is blank
   and the lines say `--root ""` (traced). The user learns it only when the client fails to start the server. Fix:
   give `cuboid-mcp` the text checks that `Root::new` runs before `canonicalize` (absolute, `form`, a path without a
   parent) as a function both use; give `ClientsView` a `refused: Option<String>`, shown as a caption in place of the
   lines ("cuboid mcp cannot serve this folder: {why}. Save the model in a folder of its own on a local drive."). No
   I/O on the UI thread. Make the file of the command line absolute with `std::path::absolute` in `Args::parse`.
   Tests: the app view for `D:\` and for a relative folder; the UI caption without Copy buttons.
4. `crates/cuboid-ui/src/brief.rs:382`. The box cannot be selected: the kit sets `selectable_labels = false`
   (`crates/cuboid-ui-kit/src/theme.rs:777`) and an egui 0.37 Label that says nothing falls back to it. The doc comment
   of `client_line` (`:359`) and the report say it can. A user who wants only the folder or the JSON value must take
   the whole line. Fix: `.selectable(true)` on that Label.
5. `crates/cuboid-ui/src/brief.rs:638`. The library opens in the Global scope (`crates/cuboid-app/src/library.rs:338`),
   where the dialog edits the global brief, which the server never reads (D-R5), and the section still offers lines.
   Scenario: with a saved model the user chooses Edit on the card, sets the game version and the rules, copies the
   Claude Code line; the agent's `brief_get` gives the project brief without them. Fix: when `!view.brief.project`, a
   caption above the lines says "The server reads the brief of the project library. Switch the library to Project to
   edit that brief.", as Style references does (`:695`). Test in `brief_ui` with a Global view.
6. `docs/mcp.md`, sections 1 and 2. A first-time user misses four things. Where the program comes from: there is no
   installer yet (`cargo xtask package` is not implemented); say to use the program of a release or build it
   (`cargo build --release -p cuboid-app`, then `target\release\cuboid.exe`), and that the AI clients section shows
   the path of the program that runs. What to say to the agent: one or two example requests ("Read the brief, study two
   typical references, then build a lantern block in `lantern.cubik`, validate it, show me a sheet and export it").
   How to stop, restart and remove: the client starts and ends the server; after changing a line, restart it in the
   client (Claude Code `/mcp`, Claude Desktop quit fully, Codex a new session) and remove it with
   `claude mcp remove cuboid` or `codex mcp remove cuboid` (check the menus and commands against the clients' docs); a
   running server keeps `cuboid.exe` in use, so an update or a rebuild of the program waits until the clients are
   closed. Claude Desktop: how to open the file (Settings, Developer, Edit Config in the MCP quickstart; verify), and
   that a file without `mcpServers` takes the whole JSON of 2.3.

## Nit

7. Lines in PowerShell expand `$` and the backtick inside double quotes: probed, `--root "D:\models\$lamp"` reached
   the program as `D:\models\`, so the client would serve the parent folder without a word. Rare; once the captions
   name PowerShell, escape both with a backtick there, or refuse such a folder in the section (finding 3).
8. `crates/cuboid-ui/src/brief.rs:684`. `copied` is an index, so when the lines change under the open dialog (scope,
   document closed and another saved) "Copied" stays on the new line of that place. Keep the copied text
   (`Option<String>`) and compare it with the line.
9. `crates/cuboid-ui/src/brief.rs:364`. Five buttons are all "Copy" for a screen reader; a hover text "Copy the line
   for Claude Code" (and so on) tells them apart.
10. `docs/design/architecture.md:212`: `cli` uses `bson` and `model` too; `:211`: the library's `fmt-*` is three of the
    five (no mesh, no voxel); `:197`: "the app fills the profile from the brief", and so does `cuboid_library::check`
    (`crates/cuboid-library/src/check.rs:192`) for the CLI and the server.
11. `docs/design/agent_toolkit_r1.md:8`. The Status should name this review for the AI clients and for W-R1.6.
12. `crates/cuboid-mcp/tests/guide.rs:91` knows one refusal code by hand, so naming another of the 21 in the guide
    fails the test; list the 21 of spec 5.1 there. The sample of 2.1 could be pinned by running `self_check` with the
    `Fake` renderer of `tests/common` and comparing the three `ok` lines up to their numbers.

## The reported choices

1. `ClientsView` on `LibraryView`, the folder the parent of the project's `cuboid-library`: accepted. The card is per
   scope and the folder is the scope-free part; the app test shows the lines in both scopes and gone after a close.
   With finding 5 for what the Global scope edits and finding 3 for the folders.
2. The box, a Label in a `Frame` with `bg_control` and radius 3: accepted as the look of a read-only field (inputs are
   control-filled, `ui_v2_spec.md` 4), and without a monospace face the lines stay readable; they wrap at spaces and
   fit at 100 and 150 % in both themes (screens). Not selectable, finding 4.
3. Two Codex boxes: accepted, spec 6.2 names both; Copy of the TOML keeps its final newline, which a file wants. The
   `--check` line formatted by the app: not accepted. Add `check` to `Snippets` (`connect.rs` builds `start` once), so
   the quoting lives in one place, finding 2 is written once and the guide test pins it; `clients_of` then only moves
   fields. "Copied": accepted, with nit 8.
4. Lines for a drive root: not accepted, finding 3. The lines cannot work, and in two shells they break their quoting.
5. The Claude Desktop caption without the path: not accepted, though cheap. The app's `DESKTOP_CONFIG`
   (`crates/cuboid-app/src/mcp.rs:21`) already holds the path per system; move it to `cuboid_mcp::connect`, pass it in
   `ClientsView` and name it in the caption, as `--check` and the guide do, with "inside `mcpServers`".
6. The guide's test: accepted. The guide quotes none of the labels that `--check` prints around the lines, so nothing
   is lost; comparing with `snippets` for the example paths is the honest form of the spec's test, since `--check`
   prints the path of the running program. See nit 12.
7. `architecture.md` beyond the request: accepted. `cuboid-validate`, `cuboid-fmt-bbmodel` and `cuboid-library` exist,
   every layer matches `LAYERS` in `xtask/src/layering.rs`, and the `cuboid-cli` commands are those of its `main.rs`
   (dump, roundtrip, brief, check, outline). Nit 10 for three lists.

## Checked and sound

- Lines: each box shows the `snippets` text for the program and the folder (app test), Copy puts exactly that text on
  the clipboard (five buttons, one "Copied" at a time; `brief_ui`), and no request reaches the app. The Desktop entry
  is serde_json, so backslashes and non-ASCII are right; TOML uses literal strings unless the text has `'` or a line
  break, else a basic string, which takes serde_json's escapes; a folder with an apostrophe or Vietnamese letters gives
  valid JSON and TOML, and the clipboard keeps UTF-8 through `copy_text`.
- The section: without a saved model a caption and no Copy (test); the ids (`brief_clients` as a section salt and
  as the grid's own salted id) do not clash; the layout at 100 and 150 % in both themes matches the screenshots.
- `docs/mcp.md` against the code: the three protocol revisions; options, the refusals of `--root`, exit codes and the
  usage (ran `--help`); the `--check` output, its exit and the `graphics:` line (ran: same shape, nothing written in
  the folder, the log under `CUBOID_HOME\data\logs`); the 18 tools in order and their effects, the ops, selectors,
  model space and the face of `front`; the read-only rule (`writes` in `tools.rs`, renders without a file); the
  journal's fields and the calls that add a line; the table of writes, atomic writes, the export and save overwrite
  rules; every limit and page size against its constant; the closing rule of the 8 models; the codes, messages and
  hints of troubleshooting; the style folder rules (16, no overlap, inside the project, not in the library, models and
  textures only); the log path, rotation, stderr level and process id; `CUBOID_HOME`, `CUBOID_LOG`, `WGPU_BACKEND`.
- Other documents: the backlog closures cite the right commits (`Core::scratch` and the batches in `87b0173`, the
  crate in `eded5f5`, the subcommand in `737950a`), tokio is still banned in `deny.toml`; the roadmap rows, the Status
  of `agent_toolkit.md` and `brief_v2.md` 7 match spec 4.5 and 10; architecture 15 matches `Dirs::detect`.

Not verified: what `claude` and `codex` do with a line whose `--` is gone; PowerShell 7 (not installed); the
config path of a Store install of Claude Desktop; Codex on native Windows against WSL; window widths below 1280 (no
screenshot; the dialog is 520 wide).

## Fixes

Status: 2026-10-28, Sonnet. One line per finding of this review; the findings above stay as written.

1. Fixed. `connect.rs` writes `"--"` in the Claude Code and Codex lines, with a comment that says why; spec 6.2, the guide (intro of 2, 2.2, 2.4) and the app tests carry it, and `guide.rs` pins it. Probed again in Windows PowerShell 5.1 with an echo script and Git's `printf.exe`: a bare `--` is dropped by the script, `"--"` arrives as `--` in both.
2. Fixed. `Snippets::check` is `& "<exe>" mcp --root "<folder>" --check` on Windows and the same without `& ` elsewhere; the app only moves it. The section captions it "Run in PowerShell." on Windows. Guide 2.1 shows the line and says to leave out `& ` in Command Prompt or bash (the guide has no cmd or bash line of its own); the troubleshooting row names the check of 2.1 and the shape of the line; `guide.rs` pins it and no other guide line may start with `& "`. Probed: without `& ` PowerShell answers "Unexpected token 'mcp'".
3. Fixed. `Root::refusal(path)` in `paths.rs` holds the text checks (absolute, `form`, no parent) and `Root::new` runs it first, so `cuboid mcp` and the section cannot disagree; `clients_of` calls it without I/O and `ClientsView.refused` makes the section show "cuboid mcp cannot serve this folder: ..." in place of the lines. Tests: `a_folder_is_refused_by_its_text_before_the_disk_is_asked` (also that `Root::new` ends with the same reason), the app test for `D:\`, an empty and a relative folder and a share, and the UI caption without Copy buttons. `Args::parse` (`lifecycle.rs`) makes the file argument absolute with `std::path::absolute`, so a model opened as `cuboid lamp.cubik` keeps an absolute path and the section offers lines; test `a_relative_file_is_made_absolute`. `paths.rs` is outside the list of my files; `Root::new` lives there and the shared function had to.
4. Fixed. `.selectable(true)` on the label of the box; a test drags across the first row, sends Copy and gets a part of the line (it fails with `.selectable(false)`). The Folder row stays unselectable: the finding names the box.
5. Fixed. In the Global scope a caption says the server reads the brief of the project library and to switch the library to Project; the lines stay. Test in `brief_ui` with a Global view.
6. Fixed in `docs/mcp.md`: where the program comes from (no installer; `cargo build --release -p cuboid-app`, `target\release\cuboid.exe`, open the model with the copy you want), 2.6 two example requests, 2.5 stop, restart, remove and update for each client (a running server keeps `cuboid.exe` in use), and in 2.3 how to open the file (Claude menu, Settings, Developer, Edit Config) and where the entry or the whole block goes. Checked against `code.claude.com/docs/en/mcp` (`claude mcp remove`, `/mcp` with Disable and Reconnect), the MCP quickstart for Desktop (Settings, Developer, Edit Config; quit completely) and the Codex source (`codex mcp remove`); the Codex MCP page does not say that a new session is needed after a change, so "start a new session" is an inference.
7. Documented, not escaped. The Claude Code, Codex and Desktop lines serve every shell, and a backtick escape would break them in bash and Command Prompt; the guide (intro of 2) says to write a backtick before a `$` or backtick in the path for PowerShell. A folder with such a character is valid for `cuboid mcp`, so the section does not refuse it.
8. Fixed. `copied` is `Option<String>`, the text of the line; test `copied_belongs_to_the_line_that_was_copied_not_to_its_place` changes the lines under the open dialog.
9. Fixed. Each Copy button has the tooltip "Copy the <name> line", which egui also gives the button as its accessible description (the name stays "Copy"); the section test reads the five descriptions.
10. Fixed. `architecture.md`: the profile is filled by the app and by `cuboid_library::check`; `library` lists `fmt-bbmodel, fmt-cubik, fmt-minecraft`; `cli` lists `model`, `bson` and its four `fmt-*` crates (cubik, mesh, minecraft, voxel).
11. Fixed. The Status of `agent_toolkit_r1.md` names this review for the AI clients and for W-R1.6; the lines of 6.2 changed as in 1 and 2.
12. Fixed. `guide.rs` builds the 21 `Refusal` values of spec 5.1 and takes `E_NOT_YET` and `E_FAILED` from `CmdOutcome`, instead of one code by hand. The sample of 2.1 is pinned by running `self_check` on the `Fake` renderer: steps and order for all three lines, the validate and render lines up to their numbers; the brief line says "game version not set" on an empty folder, so only its step name is compared.

Choice verdicts: `DESKTOP_CONFIG` moved from `cuboid-app` into `cuboid_mcp::connect` (the app's `--check` and the section use it) and the section names the path: "Add it inside mcpServers of <path>, then restart Claude Desktop." Choice 3 is `Snippets::check` as in 2. Choice 6: unchanged. The app test `mcp_stdio.rs` that looks for the lines in the `--check` output changed to the new Claude Code and Codex lines.
