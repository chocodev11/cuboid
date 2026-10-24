# L5 smoke review

Date: 2026-10-24. Scope: milestone L5 (formats and workspace), as listed in `docs/design/roadmap_lean.md`
section 4. This is a short smoke review for high-risk problems only. No code was changed.

## Summary

Two findings are blocking: a crafted `.bbmodel` can make the importer allocate without bound and abort the
process, and the Minecraft JSON export overwrites PNG files next to the target without asking.

The rest of L5 looks solid. The importers check sizes before they allocate, refuse network and device paths,
cap parent chains, scene graphs and group depth, and turn bad data into notices. The library copies files and
never moves them. Saves and exports are atomic and run on the I/O thread. Worker panics come back as errors.
Primitive mesh geometry does not affect payload identity.

What was run:

- Oracle tests with the private samples: `cuboid-fmt-cubik` oracle 4 of 4 pass, `cuboid-fmt-bbmodel` oracle
  1 of 1 pass, `cuboid-cli` oracle 1 of 1 pass, `cuboid-app` e2e `every_private_sample_opens_and_renders`
  passes.
- Payload identity of a file saved by cuboid: the sample was saved with `cuboid-cli roundtrip --out`. The
  saved copy passes all 4 payload-identity tests. It also passes with a `cuboid.json` entry added, and a second
  round trip of the copy is identical.
- Release start-up, measured from a portable copy of `target/release/cuboid.exe`. With a warm file cache, the
  time from launch to the renderer being ready was 0.82 to 0.85 s over 3 runs. The first frame follows right
  after. The first launch of a freshly copied exe took 1.5 s. Most of that came before `main` ran, probably
  a virus scan. GPU set-up takes about 620 ms of the warm start. Target: 1 s or less.
- Working set (WS) is the RAM the process holds. With an empty document at idle it was 156 to 167 MB
  (private bytes about 377 MB). Target: 150 MB or less. With the `.cubik` sample open it was 166 MB (private
  386 MB). It rose to 236 MB (private 453 MB) on a run that drew the sample's thumbnail (see finding 10).
- One hostile file, a 226 KB `.bbmodel`, used 1.65 GB of private memory (see finding 1).

## Findings

### 1. Blocking: a long group name in a `.bbmodel` is copied into every element label

- Where: `crates/cuboid-fmt-bbmodel/src/import.rs:448`, `:480`, `:650`; `crates/cuboid-fmt-bbmodel/src/mesh.rs:33`.
- What happens: each group appends its name to a prefix string. Every element below it gets its own copy of
  that prefix in its label, and the outliner walk stores one more copy per element. Nothing limits the length
  of a name. Memory grows as name length times element count. The limits on the text size and the number of
  values do not stop this.
- Scenario: measured with the release build. A 226 KB file holds one group with a 100 000-character name and
  2 000 cubes. Opening it took 1.65 GB of private memory and 1.0 GB peak working set. The default limits allow
  a 256 MB file and 262 144 elements. A 2 MB name over 20 000 cubes asks for about 80 GB. The allocation fails,
  and Rust aborts the process. The panic boundary cannot catch an abort, so all work since the last autosave
  is lost. The same file in the library import does the same on the library thread.
- Fix: cap names when they are read (for example 256 characters). Keep the prefix bounded, for example only
  the innermost groups or a truncated form. Share it as `Arc<str>` instead of cloning a `Ctx` per element.
  Count label bytes against a limit in `Limits`.

### 2. Blocking: the Minecraft JSON export overwrites PNG files next to the target without asking

- Where: `crates/cuboid-app/src/flows.rs:205-208`; names from
  `crates/cuboid-fmt-minecraft/src/json_export/mod.rs:155-166` and `:252-262`.
- What happens: a texture without a Minecraft path is written as `<label>.png` into the folder of the JSON
  file. The overwrite question, the system dialog and the export dialog's warning only cover the JSON file.
  Existing PNG files with the same name are replaced silently.
- Scenario: a new document has the default textures `particle` and `texture`, and neither has a path. Paint
  block A and export it as JSON into folder X. Cuboid writes `A.json`, `particle.png` and `texture.png`. Then
  export block B into X. Only `B.json` is asked about. `texture.png` is replaced, and block A now shows B's
  picture. A user's own PNG that shares a texture label is lost the same way.
- Fix: before writing, check the sibling files and include them in the overwrite question and the dialog
  warning. Or name them after the model (`<stem>_<label>.png`). Never replace an existing PNG without consent.

### 3. Nonblocking: one cell of coincident faces can make the z-fight rule allocate gigabytes

- Where: `crates/cuboid-validate/src/zfight.rs:102-131`.
- What happens: `MAX_PAIRS` is checked only after a whole cell run has been paired. Inside one run, all faces
  are paired with each other first, so n coincident faces push n(n-1)/2 pairs.
- Scenario: 10 000 elements duplicated in place (for example, Ctrl+D held, or an import of stacked copies).
  One run pushes about 50 M pairs, about 600 MB, on the validator thread. 20 000 elements push about 2.4 GB,
  which can abort the process. The live check starts on its own while the Validator panel is visible.
- Fix: check the cap inside the inner loop, stop the rule and report a capped summary.

### 4. Nonblocking: two import paths allow very large memory spikes from one file

- Where: `crates/cuboid-app/src/flows.rs:328` (`.cub`) and `:343` (`.bbmodel`). The `.json` and `.vox` branches
  check the size first at `:351` and `:368`. Texel budget: `crates/cuboid-fmt-bbmodel/src/lib.rs:69`,
  `crates/cuboid-fmt-minecraft/src/lib.rs:79`.
- What happens: a `.bbmodel` or `.cub` file is read whole before any limit applies. The 256 MB text limit
  of `.bbmodel` is checked only after the read. Separately, the texel budget of every importer is 2^28 texels,
  which is 1 GiB of RGBA before the GPU copies. That is far above the RAM targets of roadmap 8.
- Scenario: a 3 GB file renamed to `.bbmodel` is read in full on the I/O thread. Saves and autosaves wait
  behind it, and a small machine may run out of memory. In another case, a 1 MB project embeds four PNGs of
  8192 x 8192 pixels, which compress well. Opening it takes more than 1 GiB of RAM plus the same again in
  textures on the GPU.
- Fix: check the file size first in every branch. Lower the texel budget (for example to 2^26) and count it
  against one budget for the whole document, refusing with a message as roadmap 7 asks.

### 5. Nonblocking: the export and render dialogs check whether a file exists on the UI thread

- Where: `crates/cuboid-ui/src/export_dialog.rs:311` (the `Location` shared by both dialogs).
- What happens: `path.exists()` runs on the UI thread each time the folder or name text changes. On Windows,
  checking a network path or an offline mapped drive can block for seconds until the SMB timeout. Roadmap 6
  says the UI thread never blocks.
- Scenario: the user types a network folder such as `//nas/models` into the folder field. Each key after the
  host name starts a new check, and the window freezes on every key. A saved export folder on a sleeping NAS
  freezes the dialog when it opens.
- Fix: run the check as a small job on a worker thread, debounced. Show the warning when the answer arrives.

### 6. Nonblocking: a resource pack exported as a folder overwrites the pack files already there

- Where: `crates/cuboid-app/src/flows.rs:236-251`; warning text at `crates/cuboid-ui/src/export_dialog.rs:314-315`.
- What happens: if the target folder exists, the export writes its files into it. It replaces `pack.mcmeta`
  and every model, item and blockstate file at the same paths, and leaves the other files alone. The dialog
  only says "This will replace the existing file", which sounds like a single file. If one write fails, for
  example on a full disk, the folder holds a mix of new and old files.
- Scenario: the user keeps one pack folder and exports a second model into it. Their `pack.mcmeta` is
  replaced, and its description, overlays and supported formats are lost.
- Fix: list the files that already exist and ask before writing. Keep an existing `pack.mcmeta` unless the
  user chooses to replace it.

### 7. Nonblocking: `cuboid.json` has no version check, and a damaged entry is dropped on the next save

- Where: `crates/cuboid-fmt-cubik/src/extension.rs:118-129`, `crates/cuboid-fmt-cubik/src/container.rs:91`,
  `crates/cuboid-fmt-cubik/src/lib.rs:157-166`.
- What happens: the entry's `version` is never read, and unknown members are ignored. On save the entry is
  written again as version 1 with only what this build knows. If an entry cannot be read, the user sees a log
  warning and the next save drops the item overrides. A damaged `project` member silently falls back to the
  Cubik preset, and the next save stores that preset.
- Scenario: a file saved by a newer cuboid build is opened and saved here. The newer members are gone, and
  nothing tells the user.
- Fix: when the version is above 1 or the entry could not be read, keep the raw entry bytes and write them
  back unchanged, or refuse to overwrite the entry. Show a toast when the file opens, not only a log line.

### 8. Nonblocking (privacy): texture paths in a `.bbmodel` can read any local image

- Where: `crates/cuboid-fmt-bbmodel/src/textures.rs:154-163`; `crates/cuboid-io/src/texture_files.rs:44-81`.
- What happens: `relative_path` may climb out of the project folder with `..`, and `path` may be absolute.
  Both are read on any local drive, and symbolic links and junctions are followed. A relative `path` is
  resolved against the process's working folder, not the project folder. A notice appears only when a read
  fails.
- Scenario: a downloaded project names `../../Pictures/Screenshots/shot.png` or an absolute path. The picture
  is pulled into the document without notice and travels with the next save or export. A junction inside a
  shared folder can lead to a network share, which the text check cannot see.
- Fix: by default, read only below the project folder. Name any reads outside it in the notices, or ask
  first. Resolve a relative `path` against the project folder. Refuse reparse points, as the library walk
  already does.

### 9. Nonblocking: "Remove from the library" deletes files at once, with no question and no undo

- Where: `crates/cuboid-ui/src/library.rs:373-374`; `crates/cuboid-library/src/store.rs:336-358`.
- What happens: one click in the context menu deletes the stored copy, its `.mcmeta` copy, the thumbnail and
  the entry's tags and licence.
- Scenario: the user misclicks in the context menu after the original file was moved or deleted. The only
  copy is gone.
- Fix: ask first (roadmap 6 keeps modal dialogs for destructive confirmations). Or move the files to a
  `trash/` folder of the library until the session ends.

### 10. Nonblocking (RAM): the thumbnail renderer is not released while the app is idle

- Where: `crates/cuboid-app/src/thumbs.rs:24` and `:133-137`; `crates/cuboid-app/src/frontend.rs:176-179`.
- What happens: the thumbnail renderer (the "rig") keeps a second `Renderer` until 120 frames pass with no
  work. But the app draws frames only on input or while the rig is busy. After the last thumbnail, frames
  stop and the renderer stays.
- Scenario: measured with the `.cubik` sample open. On the run that drew a thumbnail, the working set was
  236 MB (private 453 MB) and still there 10 s later. On a run with the thumbnail cached, it was 166 MB
  (private 386 MB).
- Fix: drop the renderer by time, with one requested repaint, or as soon as the queue is empty. Or let the
  rig use the pipelines of the live renderer.

### 11. Nonblocking (performance): start-up meets its target narrowly, and idle RAM is over its target

- Where: `crates/cuboid-shell/src/event_loop.rs:220-234`.
- What happens: warm start-up is 0.82 to 0.85 s against the 1 s target. About 620 ms of it is GPU set-up.
  The log shows the Vulkan adapter probed twice (two "cooperative matrix" lines about 170 ms apart), and
  egui-wgpu lists all 4 adapters across backends. Idle working set is 156 to 167 MB against the 150 MB target.
- Scenario: a slower machine, or a cold disk, will miss 1 s.
- Fix: measure with an instance limited to the primary backend (Vulkan or DX12) and without the adapter
  listing. Look at what makes up the idle working set. Add the CI benchmark of roadmap 8 for both numbers.

### 12. Nonblocking: the new loaders have no fuzz targets

- Where: `fuzz/fuzz_targets` (bson, cub, cubik, dxt, image, jsonlite, lnk, mcpe_json).
- What happens: `.bbmodel`, Minecraft JSON and resource packs, VOX and the library's classifier have hostile
  unit tests but no fuzz targets. Roadmap 7 asks for nightly loader fuzzing. Finding 1 is the kind of bug a
  fuzz run with a memory limit finds.
- Fix: add targets for `cuboid_fmt_bbmodel::import`, `cuboid_fmt_minecraft::import` (with an in-memory
  `TextureFiles`), `cuboid_fmt_voxel::vox::import` and the library's `classify` and `model_meta`. Run them with
  `-rss_limit_mb=2048`.
