# L4-B review (fail)

L4-B delivers the application shell of `cuboid-app`: command line, start-up and shut-down, one FIFO I/O thread
with job handles, the file flows with the unsaved-changes question, timer and risky autosaves in a recovery
folder, the start-up scan, the panic hook with crash reports, the frame-level panic boundary with the last-good
restore and the safe mode, and the rotating log. The building blocks are sound. Every write goes through
`write_atomic` (temporary file in the same folder, then a replacing rename), so no partial file ever replaces
a good one, and an autosave pair uses its JSON as the commit marker. The I/O thread cannot die, because every
job runs under `catch_unwind`. The UI thread never blocks on it. The only locks are leaf locks, and the hook
takes them with `try_lock`. Exit waits at most 3 s. Memory is bounded: the last-good copy, the autosave inputs
and the snapshots are pointer copies (shared items, chunks and images), and toasts, crash reports, log files
and the log ring are capped. `cargo clippy -p cuboid-app --all-targets --no-deps -- -D warnings`,
`cargo test -p cuboid-app` (63 passed), `cargo fmt -p cuboid-app -- --check` and `cargo xtask layering` pass.
The package fails on the lifetime of recovery files and on the panic recovery path. Three paths delete
autosaves the user still needs: the folder-wide prune, the session-wide purge and the start-up stale check.
The last good state can be the very state that panics, and each further panic writes one more autosave of it.
The recovery code runs outside the boundary and repeats the capture that may have panicked. Save As can
replace an existing file without a question, because the extension is added after the dialog.

## Blocking

1. `crates/cuboid-app/src/backup.rs:120` and `:192-196` (prune across sessions).
   - What is wrong: `write` calls `prune(dir, KEEP)` after every autosave. `prune` keeps the 10 newest pairs
     of the whole folder, whatever session wrote them. The folder is shared by all runs and by concurrent
     instances (app 3.1 allows several instances). So the autosaves of one run push out those of another.
   - Scenario A: cuboid crashes with an hour of unsaved work in an untitled model. The next start shows the
     recovery dialog. The user clicks "Decide later", which promises to keep the files for the next start,
     and works on another model. With the default 10-minute timer, plus the risky autosaves before New and
     Open, the tenth autosave of this run comes after about 100 minutes of editing, sooner with risky ones.
     That write deletes the crashed run's pair. The next start offers nothing; the hour of work is gone.
   - Scenario B: two instances. Instance A holds edits that have not changed since its last autosave, so its
     timer writes nothing more (`unchanged`). Instance B autosaves 10 times, and A's only pair is deleted.
     A crashes later; its work is lost. The same sharing lets B's start-up scan offer A's live autosaves as
     "did not close cleanly", and "Discard" there deletes them.
   - Fix: prune per session. In `write`, keep the newest N pairs of the writing session and never touch
     another session's pairs. Remove other sessions' pairs only by age (for example 14 days) or by an explicit
     user decision. To tell a live session from a dead one, hold a lock file per session (`<session>.lock`,
     opened without sharing on Windows, `flock` elsewhere). `scan` and every prune skip sessions whose lock is
     held.

2. `crates/cuboid-app/src/flows.rs:499-511`, `:292-309`, `:551-565` and `:462-471` (purge too wide, recovery
   without the question).
   - What is wrong: after a clean save, and at quit, `purge_session` deletes every pair of this session. But
     the pairs of a session are not all of the document being saved. `finish_open` writes a risky autosave of
     the current document and replaces it without asking in two cases: an accepted recovery
     (`accept_recovery` calls `start_open` directly, not through `request`), and edits made while a file was
     loading. The next save of the new document, or the next quit, deletes that autosave. The user was never
     asked about that work.
   - Scenario: three panics put the app into safe mode while model X (unsaved) is open. The recovery dialog
     lists X's panic autosave and model Y from an older crash. The user recovers Y. X is replaced without a
     question and survives only as this session's risky and panic autosaves. The dialog comes back with X;
     the user clicks "Decide later". The user saves Y. `finish_save` purges the session, and both X pairs are
     deleted. The next start offers nothing; X is lost. Quitting instead of saving ends the same way (the
     "Don't save" answer was about Y only).
   - Fix: give each document a recovery id, new at every `Editor::replace`, and store it in `Meta`. Purge only
     the pairs of the saved document's id after a save, and only the ids the user discarded on purpose at
     quit. Send `RecoveryAccept` through `request`, so a dirty document gets the unsaved-changes question
     first. In `finish_open`, when the document became dirty during the load, ask instead of replacing, or keep
     its autosave out of the purge. Keyed by the id, `scan` also stops hiding all but one of several untitled
     documents of a session (the key is now `(session, source)`, `backup.rs:228`).

3. `crates/cuboid-app/src/backup.rs:214-226`, `:154` and `:169-172` (the scan deletes on weak evidence).
   - What is wrong: `scan` deletes a pair when the source file was modified at or after the autosave's
     `saved_ms`. That compares the file system's clock with this PC's clock. It also treats any later write,
     by anyone, as proof that the autosave is contained in the file. Separately, `list` deletes a pair whose
     `.json` cannot be read once it is older than 120 s, also when the read failed for a passing reason: a
     sharing violation from a virus scanner, or a `Meta` of a newer cuboid version.
   - Scenario: instance A has `M.cubik` open with edits, and its autosave is written at 10:00. Instance B, a
     sync client (OneDrive, Dropbox) or a git checkout writes `M.cubik` at 10:05. A crashes at 10:06. The next
     start deletes A's autosave without asking, although its edits are in no file. With a file on a network
     share whose server clock runs ahead, every autosave of that file is deleted at each start, so recovery
     never works for it.
   - Fix: do not delete in `scan` on this test. Store the source file's modification time and size in `Meta`,
     as they were when this session opened or last saved it. At the scan, offer the pair; when the file
     changed since then, say so in the dialog ("the file was changed after this autosave"). In `list`, delete
     a `.json` only when it was read and does not parse; skip it on I/O errors, and keep files whose `version`
     is higher than the known one.

4. `crates/cuboid-app/src/state.rs:243`, `crates/cuboid-app/src/app.rs:108-115` and
   `crates/cuboid-app/src/state.rs:404-408` (the good state is taken before the UI ran; one autosave per
   panic).
   - What is wrong: `Editor::commit_frame` refreshes the last good document at the end of `State::step`. That
     runs before `placeholder_ui::show` in the same guarded closure, and it will run before the scene extract
     once that is hooked up. So a state becomes "good" before any code has drawn it. If the UI or the extract
     panics on that state, `restore_last_good` puts the same state back. In addition, `recover_from_panic`
     queues a new full autosave for every panic, also in safe mode, because the swapped copy always gets a
     new revision.
   - Scenario: after a pause of more than 250 ms the user runs one command. It leaves a state that the
     outliner or the chunk extract panics on, for example a NaN transform. In that frame `commit_frame` stores
     the state as good, then the drawing panics. The restore brings back the same state, and the next frame
     panics again. Each panic writes an autosave of the broken state. In safe mode the panic repeats as long as
     the panicking code still runs, and while an autosave is pending a frame comes every 50 ms, so autosaves
     keep coming. Within about ten writes the prune deletes every older pair, including the last one before
     the command. `scan_when_idle` waits for `autosaves` to be empty, so the recovery dialog does not open.
     When it opens, it offers only the newest pair per session and file, which is the broken state.
   - Fix: refresh the good copy only after a whole frame finished without a panic. Call `commit_frame` in
     `App::frame` after `guarded` returned true: after the UI now, and after extract and render sync later.
     After a panic, autosave only when the restored content differs from the last autosave: remember the
     revision of the good document itself and compare that, not the revision of the swapped copy. Write no
     autosave from the panic path in safe mode. Let the scan wait for the job in flight, not for an empty
     list.

5. `crates/cuboid-app/src/app.rs:45-48`, `crates/cuboid-app/src/state.rs:404-416`, `:371-375` and
   `crates/cuboid-app/src/editor.rs:190-212` (the recovery runs outside the boundary).
   - What is wrong: `App::guarded` calls `recover_from_panic` outside `catch_unwind`. That function swaps the
     document, prunes the selection and calls `save_input`, which runs `capture` over the whole document on
     the UI thread. The same `save_input` runs inside the boundary for every timer autosave, risky autosave,
     save and export. A panic that comes from `save_input` (capture, normal form, voxel snapshot) or from the
     selection therefore repeats in the recovery and escapes. The shell unwinds out of the event loop and the
     process ends. Nothing is written; the hook writes only a report.
   - Scenario: a model holds a value that makes `capture` panic. The timer autosave panics inside the
     boundary. `recover_from_panic` restores the good copy, which holds the same value, and calls
     `save_input` again. The second panic ends the program. Because every autosave of this document panics,
     the session has no autosave at all, and all unsaved work is lost.
   - Fix: run the recovery steps under their own `catch_unwind`; on a second panic enter safe mode without
     the autosave and log it. Move `capture` into the job: give the worker the `Arc<DocSnapshot>` and a copy
     of the selection, and capture there under `run_isolated`; the UI thread then only clones pointers. Also
     add the crash backup of architecture 16: keep the last good save input in a slot, and write it
     synchronously from the panic hook (taken with `try_lock`) or from a `Drop` of `App` while
     `std::thread::panicking()` is true. That also covers panics outside the boundary, in the shell's egui
     tessellation and paint.

6. `crates/cuboid-app/src/flows.rs:179`, `:181` and `:88-98` (the extension is added after the dialog).
   - What is wrong: Save As and Export copy append `.cubik` to the path the system dialog returned. The
     dialog asked about overwriting the name the user typed, not the name that is written. rfd's portal
     backend, the default on Linux, does not add the extension. On Windows rfd sets a default extension, but
     the dialog adds it only when the typed name has no extension of its own.
   - Scenario: on Linux the user saves a new model as "chair" in a folder that already holds `chair.cubik`.
     The dialog sees no file named "chair" and asks nothing. The port writes `chair.cubik` and replaces the
     old model. On Windows a dotted name does the same: "chair.v2" comes back as typed, and an existing
     `chair.v2.cubik` is replaced without a question.
   - Fix: when `ensure_cubik_extension` changed the path and the new path exists, ask before writing (a
     confirm dialog through `Dialog`), or show the save dialog again with the corrected name. Every path the
     dialog did not confirm gets the overwrite question.

## Nonblocking

- Saves are not verified in release: `SaveOptions::default()` turns `verify` on only with debug assertions
  (`editor.rs:46-48`), and a clean save then purges all autosaves. A writer bug in a release build would leave
  an unreadable file and no other copy. For user saves set `verify: true` (one payload decode on the I/O
  thread), and keep the newest autosave until the saved file was read back, or keep the previous file as
  `<name>.cubik.bak`. Cubik's weekday backups (app 12.1) were that safety net; the port has none.
- The restore keeps the undo history (`SwapKind::Import`, `editor.rs:147`). Entries recorded after the good
  copy was taken stay, and so does an entry pushed by the command that panicked. Undo then applies snapshots
  of a newer state onto the older document and mixes categories into a state that never existed. Store
  `history().undo_len()` with the good copy and truncate to it on restore (a small `Core` API), or clear the
  history.
- At quit (`finish_quit`) queued autosaves run before the settings write and the purge, and `IoThread::drop`
  waits only 3 s. With a large model the settings (recent files, window) may not be written and the purge may
  not run, so the next start offers a recovery after a clean exit. Cancel the queued autosaves at quit (the
  purge removes them anyway) and queue the settings first.
- A write cut off at exit or by a crash leaves its `.tmpXXXXXX` file in the recovery folder, and `list` never
  removes it. Each one can be as big as the payload. Remove temporary files older than `JUNK_AGE` in `list`.
- `RecoveryDiscard` and an accepted recovery remove only the newest pair of the document. The older pairs of
  the same crashed session stay and are offered at the next start as if they were new. Remove all pairs of the
  same session and document.
- `accept_recovery` closes the dialog and moves the rest into `recovery_rest` before `start_open` can refuse
  with "Please wait"; the dialog then does not come back. A failed recovery open also drops `recovery_rest`
  silently. Check `doc_job` first, and show the rest after a failure too.
- `fail_doc_job` applies its `NotFound` branch to saves too (`flows.rs:529-538`). Saving into a folder that is
  gone (an unplugged drive) says "it does not exist" and removes the target from the recent files. Limit the
  branch to opens.
- "Don't save" marks the document clean before the open runs (`flows.rs:280`). When the open then fails, the
  old document stays on screen marked clean, and quitting does not ask. Mark it clean only when the replacement
  succeeded; the risky autosave covers the time between.
- The panic toast says "an autosave was written" before the job ran, and also when it fails
  (`state.rs:409-416`). Say "is being written", or toast from `poll_autosaves`.
- `poll_autosaves` reports only the last result when several finish in one frame, so a failure followed by a
  success in the same frame is not shown.
- A Quit while the recovery dialog is open replaces it with the unsaved-changes dialog, and the candidates are
  gone for the session. Keep the recovery dialog queued behind the question.
- Privacy: a crash report copies 200 log lines, which hold full paths with the user name (open and save
  failures, load warnings, the settings notice). Users attach these reports to bug reports. Replace the home
  folder with `~` when the report is written. Logs and reports get default permissions (0644 under the usual
  umask on Linux and macOS); create `<data>` with 0700. The fallback `temp_dir()/cuboid` (`platform.rs:36`) is
  shared between users on Unix; prefer a per-user folder.
- `write_from_hook` logs through the blocking sink lock (`crash.rs:59`). Nothing in `Sink::push` can panic
  today, so this is safe, but a future panic under that lock would deadlock the hook. Use the same `try_lock`
  as `recent_lines` for that line.
- The logger writes each line unbuffered on the calling thread, so the UI thread does one file write per line;
  at debug level that costs frame time. A `BufWriter` flushed at warn and above, or a log thread, avoids it.
  Ring lines have no length cap; cap them (for example at 4 KiB).
- Memory: the last good copy shares items, chunks and images, so its direct cost is the copied `Vec`s and the
  `ObjList` index map (O(elements)) every 250 ms. The larger cost is copy on write: a paint stroke on a
  4096 x 4096 texture copies the 64 MB image once after each refresh, up to four times a second. Measure it in
  the L3 and L6 benchmarks; if needed, refresh at the end of a stroke instead of on the timer.
- Every panic marks the document dirty, even when nothing changed, so the quit then asks. Harmless.
- Spec differences to list in app 0.2: after "Save" in the unsaved-changes question the pending action
  continues (app 2.2 QUIRK: it does not); exported copies are not added to the recent files (app 11 adds
  readable exports); a recent entry is removed on any open of a missing file, not only from the menu; the
  autosave interval can go down to 1 minute (spec: at least 5).
- An exported copy keeps `mp.sp`, the full path of the document (`save_input(None)`, `flows.rs:412`). Exports
  are meant to be shared; write an empty save path there if the format spec allows it.

## Integration notes

- Replace `placeholder_ui::show` by the `cuboid-ui` entry point and keep the intents. In safe mode hide every
  panel and the viewport that read the document, not only the main panels, and skip the scene extract and the
  render sync, so a panic in drawing cannot repeat.
- Run the scene extract, `Renderer::sync` and the UI inside the same guarded closure, and refresh the good copy
  after it (item 4). After `restore_last_good` force a full rebuild: the restored voxel world carries the
  dirty set of the time it was copied, and `swap_document` marks only lists, selection and textures in the
  journal, so chunk meshes and display and view caches would stay stale.
- Call `Core::set_cameras` before every `save_input` (save, export, autosave). Today the files carry the
  camera poses that were read from the file.
- Send recent files, drag and drop and recovery through `request` in the real UI too. Block edit commands while
  an open runs, or ask when it finishes (item 2).
- The boundary does not cover the shell's egui tessellation and paint, `RenderGlue::new` or
  `placeholder_ui::init`. The crash backup of item 5 is the safety net for those.
- After a caught panic egui holds a half-built pass. Call `ctx.request_discard` or skip painting that pass, so
  the broken frame is not shown.
- Hook `engine_key` up to `cuboid-actions`; key dispatch pushes intents and never edits the state.
- Device loss: roadmap 7 asks to recreate the device and its resources; today there is only a toast and an
  autosave.
- Pass the memory budgets of roadmap 7 into the `Limits` of the open instead of `Limits::default()`.
- Add `#![windows_subsystem = "windows"]` for release builds, so no console window opens; warnings still reach
  the log.
- macOS: check that `rfd::AsyncFileDialog`, driven by `pollster` on the `cuboid-dialog` thread, works with
  winit on the main thread; rfd needs the main run loop there.
