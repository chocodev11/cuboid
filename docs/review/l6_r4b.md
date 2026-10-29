# Review: L6 R4-B, the I/O and robustness fixes of the R4 audit

Status: 2026-10-28. Reviewer: Opus. Scope: the uncommitted R4-B fixes of `docs/review/l6_r4_audit.md` M5, M6 (steps 1
and 3), m5, m8, m10 (cuboid-settings) and m11, spec `docs/design/l6_release.md` 5: `crates/cuboid-app/Cargo.toml`,
`crates/cuboid-app/src/{io_thread,backup,ui_setup,flows}.rs`, the `RecentTimes` part of `frontend.rs` and the new
`frontend/recent.rs`, `crates/cuboid-io/src/{atomic.rs,atomic/tests.rs,lib.rs}`,
`crates/cuboid-settings/src/{lib.rs,store.rs,store/tests.rs}`,
`crates/cuboid-ui-kit/src/{widgets/paint.rs,widgets/mod.rs,lib.rs}`, `crates/cuboid-ui/src/properties.rs`,
`crates/cuboid-ui/tests/panels.rs`. Read against rayon-core 1.13.0 (`registry.rs:517-540`, `:785-842`), egui
0.37.0 (`context.rs:100-170`) and tempfile 3.27.0 (`file/imp/windows.rs`).
Run on 16 logical cores, so the background pool had 15 threads: `cargo clippy -p cuboid-app -p cuboid-io -p
cuboid-settings -p cuboid-ui-kit -p cuboid-ui --all-targets -j 4 -- -D warnings` (clean); `cargo test -p cuboid-io -p
cuboid-settings -p cuboid-ui-kit -j 4` (107, 57, 37 + 39 + 1 pass); `cargo test -p cuboid-app -j 4 -- io_thread recent
backup` (31 pass; cargo refuses three positional filters, so they went to libtest), plus the two other new app tests
`a_file_over_the_limit_is_refused_with_a_message` and `an_oversize_preferences_file_is_refused_and_gives_the_defaults`
(pass); `cargo test -p cuboid-ui -j 4 --test panels` (58 pass). Not run: the workspace tests, a machine with 2 logical
cores, a network share, the GUI.

Verdict: changes requested, two majors. Every listed finding is addressed, and the tests prove the main claims: a UI
loop no longer waits behind a busy background pool, a hanging lookup no longer holds the frame, oversize files are
refused, a 5 MB label is laid out as 257 characters. Two of the new mechanisms fail in setups the package must survive.
With one background thread (2 logical cores, which includes common budget laptops) the library thread's jobs block the
document thread's, the settings write at quit included. The leftover cleanup compares the local clock with times a file
server sets, so on a share whose clock is an hour behind it deletes the write's own temporary file and every save to
that folder fails.

## Major

1. `crates/cuboid-app/src/io_thread.rs:20-22`, with `:45-54`. `background_threads()` is `available_parallelism - 1`,
   at least 1. With one thread, the two `IoThread`s (documents, `lifecycle.rs:88`; library, `library.rs:321`) inject
   their tasks into one worker, which takes an injected task only after the one it runs has ended
   (`registry.rs:833-842`; a single worker never waits in a join, so it never picks up the other thread's task in
   between). An import or a thumbnail batch (`library.rs:1044`, one job for all items) then holds every save, autosave
   and open until it ends, the opposite of what `library.rs:2-3` promises ("a second `IoThread`, so a long import never
   delays a save"). At quit it loses data: `finish_quit` (`flows.rs:711-737`) queues the settings write and the
   autosave purge on the document thread and sets `quit`; `State` drops `io` first (`state.rs:127`), whose `Drop` waits
   3 s (`io_thread.rs:104-107`), then the process ends. Behind a library job longer than that, the session's settings
   (window, recent files) are not written and the purge does not run, so the next start offers autosaves of documents
   the user saved. `available_parallelism` is 2 on 2-core, 2-thread CPUs (Celeron N4020 or N4500 class laptops), in
   2-vCPU virtual machines and on the 2-vCPU CI runners private repositories get; there all I/O tests of a test binary
   also share one worker. Fix: `.max(2)` in `background_threads()`, with the reason in its comment. With two workers a
   library job body holds at most one, and the other takes the document task as soon as it has no stolen work left, so
   the wait is bounded by one parallel loop. Test in `io_thread::tests`: two `IoThread`s, a job on the first blocks on a
   gate, a job on the second returns within 5 s, then the gate opens; it fails with one worker on any machine.

2. `crates/cuboid-io/src/atomic.rs:33-34` and `:63-68`. The write creates its own `.cuboid-tmp-` file first and then
   runs `remove_stale`, which judges every prefixed file, its own included, by `modified().elapsed()`: the local clock
   against a time the file system sets. On SMB and NFS the server stamps a new file with its own clock. When that clock
   is an hour or more behind (a NAS without NTP, a router share after a power cut, a dead clock battery), the fresh
   temporary file counts as stale: `remove_stale` deletes it, `persist` then fails (Windows: `SetFileAttributesW` and
   `MoveFileExW` on a deleted or delete-pending name; Unix: `rename` gives `ENOENT`), and every save, export and backup
   into that folder fails with "Cannot save", where it worked before. The same skew deletes the temporary file that
   another cuboid process (a second window, `cuboid mcp`) is writing at that moment, which the age test was meant to
   rule out. The target file stays intact, but the folder becomes unwritable. Fix: take the reference time from the
   same clock and skip the own file. After `temp_in`, `let reference = temp.as_file().metadata().and_then(|m|
   m.modified()).ok();`; when it is `None`, skip the cleanup. Pass `temp.path()` and `reference` to `remove_stale`, skip
   `entry.path() == own`, and call a file stale when `reference.duration_since(mtime).is_ok_and(|age| age >=
   STALE_AGE)`. Test: create the own file with `temp_in`, set its time and one leftover's to now minus 2 h (a server
   2 h behind), another leftover to now minus 4 h, run `remove_stale`: the own file and the 2 h leftover stay, the 4 h
   one goes.

## Minor

3. `crates/cuboid-io/src/atomic.rs:34` and `:55-73`. Every write lists its whole folder, so writing many files into one
   folder is quadratic: a library import writes each file and its thumbnail into two shared folders
   (`cuboid-library/src/import.rs:384-398`), a resource pack export writes its files one by one
   (`cuboid-fmt-minecraft/src/resource_pack/mod.rs:107-122`), the recent thumbnails share one cache folder
   (`library.rs:1152`). Importing 500 files into a library of 5,000 lists about 5 million entries in each of the two
   folders: seconds on a local disk, minutes on a share, where each listing is a series of round trips. Leftovers only
   come from a killed process, so once per folder and run is enough. Fix: a `static CLEANED: Mutex<HashSet<PathBuf>>`
   that `write_atomic` checks and fills before `remove_stale`, taken with no other lock held and released before any
   other call (add it to the leaf locks of `architecture.md` 11.4 rule 4); or move the cleanup out of `write_atomic`
   into a `pub fn remove_stale_temps(dir)` that the document save and the library open call.

4. `crates/cuboid-app/src/frontend/recent.rs:41-43` and `:86`. The helper's `request_repaint` does not wake the window.
   Nothing in `crates/` installs `set_request_repaint_callback`, the shell learns about repaints only from
   `repaint_delay` at the end of a pass (`cuboid-shell/src/event_loop.rs:481-491`), and egui calls nothing else when no
   callback is set (`context.rs:133-170`). An answer that arrives after the frame ended, which is the slow share that
   M6 is about, shows only when input or another wake-up runs a frame; on an idle start screen the dates stay old. The
   doc comment says the opposite. Fix in R4-B's file: while a batch is out, end `fill` with
   `wake.request_repaint_after(Duration::from_secs(1))`, which the shell honours (one frame a second, only while a
   lookup is out), and correct the comment. The root fix, a repaint callback with an `EventLoopProxy` in cuboid-shell,
   which the same pattern in `cuboid-ui/src/export_dialog.rs:364-371` also needs, is for the shell's owner.

5. m11 covers the galleys and the inspector name only. These still take the whole label: the animation name field
   (`cuboid-ui/src/animations.rs:199-216`, refilled with `current.name` every frame, the text-field cost the audit
   names), the outliner's inline rename (`outliner.rs:212` and `:363` seed the editor with the whole `node.name`), and
   the plain labels of the bone name (`props_animate.rs:69`) and of the drag ghost (`outliner.rs:346-353`). Animation
   names (`cuboid-fmt-cubik/src/anim.rs:101`) and bone labels come from the same unbounded strings of the file. Fix:
   the inspector's pattern in `animations.rs` (`display_cut`, compare with the cut text); in the outliner, seed with
   `display_cut(&node.name)` and send `Rename` only when the buffer differs from that seed (`outliner.rs:203-207` sends
   it on any commit, so the seed alone would rename to the cut text on Enter); `display_cut` at the two labels. Tests
   like `a_very_long_name_is_cut_in_the_field...` for the animation field and the outliner rename.

## Nit

6. `crates/cuboid-settings/src/store.rs:263-273` keeps tempfile's `.tmp` prefix. A killed settings write leaves a
   `.tmpXXXXXX` in the config folder that nothing removes, although `cuboid_io::write_atomic` writes into that folder
   too (preferences, `ui_setup.rs:80`; pins, `library.rs:1487`) and would clean it. Fix:
   `tempfile::Builder::new().prefix(".cuboid-tmp-").tempfile_in(dir)` with a comment that it must equal
   `cuboid_io::TEMP_PREFIX`, and an app test that compares the two strings (the app sees both crates).

7. `crates/cuboid-app/src/backup.rs:281` no longer removes the `.tmp*` leftovers of earlier builds in the recovery
   folder. No release wrote them, so only development machines keep a few; accepting it is fine. `file_age` in the same
   file has the clock problem of 2 for the 120 s junk age; the recovery folder is local unless the profile is
   redirected to a share, and the reference time of 2 would serve it as well.

8. `crates/cuboid-library/src/doc.rs:61-70` keeps its own `read_within` with the metadata-then-`fs::read` pattern that
   `flows.rs` just left; the read is unbounded if the file grows between the two calls. Outside R4-B's files: a
   follow-up can call `cuboid_settings::read_capped` there (layer 5 may use layer 1). `read_capped` living in the
   settings crate because of the layers is acceptable.

9. `crates/cuboid-ui/src/properties.rs:100-114`. When the primary object or its name changes while the field keeps
   the focus, leaving the field renames the new state to the old buffer. This is older than R4-B, but with the cut it
   now writes object A's cut name into object B. Fix: remember `(primary, shown)` when the field gains the focus, and
   send `Rename` for that id only when the buffer differs from the remembered text.

## The reported deviations

1. One background pool shared by both I/O threads: sound apart from the single-thread case (finding 1). No deadlock:
   no job body waits on another job, on the UI thread or on a lock (no `recv` outside tests, no lock, no rayon `spawn`,
   `scope` or `join` anywhere in `crates/`). The UI thread never calls `install`, so its loops stay on the global pool;
   `a_parallel_loop_outside_does_not_wait_for_a_busy_background_pool` proves it and fails with a shared pool (one held
   item per global worker, so the 5 s receive times out). There is no nested `install`; one added later would run
   inline. A worker that waits in a join takes injected tasks (`registry.rs:796-800`), so a document task can run
   nested on the stack of a library job's loop; that is safe here: no thread-local state outside tests, at most two
   levels, and the inner task waits on nothing of the outer one. Panics: `run_isolated` catches on the worker, a
   panicking parallel loop resurfaces in the body and is caught there, `install` never unwinds, and the I/O thread
   goes on (`a_panicking_job_becomes_an_error_and_the_thread_goes_on` passes). The FIFO order of each `IoThread` holds,
   because `install` returns only when its task is done.
2. `RecentTimes` in `frontend/recent.rs`: sound, but the wake-up does not work (finding 4). One short-lived thread per
   batch and at most one alive; its handle is dropped, so nothing waits for it at exit, and the end of the process ends
   one that hangs in a lookup. A lookup that never returns freezes the dates and starts nothing more; a failed spawn
   waits 30 s before the next try. No file call is left on the UI thread for recent entries: `fill` does a `try_recv`
   and map lookups, `RecentEntry::from` and `recent_thumb_path` are pure, and the thumbnails go through a library job
   (`library.rs:1015-1036`). Step 3 left as is (`flows.rs:755`, one `exists()` right after a dialog, only when the
   extension was added): agreed, as the audit allowed.
3. `read_capped`: sound. It opens first and takes the size of the open handle, then reads through `take(limit + 1)`,
   so a file that grows, a device or a pipe cannot pass the cap, and the allocation is bounded by the checked size. An
   oversize settings file is kept aside and the first-run settings load (`Rejected { TooLarge }`, tested); oversize
   preferences give the defaults and the next preferences write replaces the file, which is right for a cosmetic file;
   an oversize recovery `.json` is neither read nor deleted, and its `.cubik` stays out of the orphan cleanup (tested).
   `read_within` keeps its message and its `NotFound` (tested). One change: preferences that are not UTF-8 are now
   parsed lossily instead of logged as unreadable, which is harmless.
4. Temporary files: the prefix, the cleanup and the Unix folder sync do what m8 asks, with the clock problem (finding 2)
   and the cost (finding 3). The cleanup cannot touch a user's file unless it is named `.cuboid-tmp-*`; a symbolic link
   is not followed (`DirEntry::metadata`) and `remove_file` removes the link only. A read-only folder fails at
   `temp_in`, before anything is listed; a folder that cannot be listed is skipped; a folder sync that fails after the
   rename is ignored, which is right. The age is the modification time (on Windows the last-write time of the directory
   entry, which for a file still being written is at least its creation time; on Unix `lstat`), so with one clock a
   running write, which takes seconds, is never an hour old. A write suspended for an hour (a laptop asleep mid-save)
   can lose its temporary file to another process; that save then fails with a message and the target stays, which is
   acceptable. `backup.rs` at 120 s behaves as before for the new prefix, and the FIFO of the document thread keeps the
   scan away from its own writes. The settings crate keeping its own writer is right for the layers; its prefix is
   Nit 6.
5. `display_cut`: sound. It returns the text borrowed up to 256 bytes, cuts on a character boundary after a bounded
   scan, and both galleys use it. The inspector: focus and leave sends no rename (tested). Undo restores the whole name,
   because `Edit::Rename` carries only the new name and the old one comes from the document. Typing or pasting renames
   to what the field holds, which is the cut text with the user's change, the ellipsis included and the rest of the
   label dropped; that is an explicit edit that undo reverts, and acceptable. The only path without an edit is Nit 9,
   which is older. The other label fields are finding 5.
6. m10 for cuboid-settings: sound. The block equals the one of the format crates, no `allow` or `expect` was added,
   and clippy is clean on all targets.

## Checked and sound

- `rayon.workspace = true` in `crates/cuboid-app/Cargo.toml`; the pool is built once, and a build error is kept in the
  `OnceLock` and reported by both `spawn` calls, which then fail as a failed thread spawn did before.
- The `frontend.rs` part of R4-B is the move of `RecentTimes` and the call `fill(&mut recent, state.now, ctx)`; the
  `rig.step` change in the same diff belongs to R4-A.
- `display_cut` keeps the cost of short labels: one length check, then the same `to_owned` as before.
- The new tests synchronize with bounded polls (10 s limits), never with fixed sleeps, and gate jobs end on a dropped
  sender, so a failing test does not hold a background worker.
- `write_atomic` still restores the replaced file's permissions, and a failed `persist` drops the temporary file.
- `ParseError::TooLarge` reaches the user through the existing `Rejected` notice with the cap in the message.

## Fixes

1. Fixed. `background_threads()` is `available_parallelism - 1` with `.max(2)` (comment gives the reason); test `a_long_job_of_one_io_thread_does_not_hold_the_jobs_of_another`.
2. Fixed. `write_atomic` takes the reference time from the own temporary file (`clean_once`; no cleanup when it is unreadable), `remove_stale` skips the own file and judges the age against that reference; test `the_age_is_measured_from_the_time_of_the_own_file_and_the_own_file_stays`.
3. Fixed. `static CLEANED: Mutex<BTreeSet<PathBuf>>` (leaf lock, one insert) makes the cleanup run once per folder per run; test `only_the_first_write_into_a_folder_looks_for_leftovers`. The leaf lock is not yet added to the list in `architecture.md` 11.4 rule 4 (outside R4-B files).
4. Fixed. `RecentTimes::fill` ends with `wake.request_repaint_after(POLL)` (1 s) while a batch is out, the comments are corrected; test `a_pass_with_a_batch_out_asks_to_come_back_and_an_idle_one_does_not`. The repaint callback in cuboid-shell stays with the shell owner.
5. Fixed. The animation name field shows `display_cut` and compares with the cut text; the outliner rename seeds with the cut name and sends `Rename` only when the buffer differs from the seed (`Rename` struct); the drag ghost and the bone name in `props_animate.rs` use `display_cut`. Tests: `a_very_long_animation_name_is_cut_in_the_field...` (animate.rs), `a_very_long_name_is_cut_in_the_outliner_rename...` (panels.rs).
6. Fixed. cuboid-settings writes with `TEMP_PREFIX` (`.cuboid-tmp-`); app test `the_settings_writer_names_its_temporary_files_like_the_atomic_writer` compares the two constants.
7. Not fixed. Accepted by the review (backup keeps the `.tmp` legacy prefix cleanup, which it does); the `file_age` clock skew of the recovery folder is left, as the folder is local.
8. Fixed. `cuboid-library/src/doc.rs` uses `cuboid_settings::read_capped`; `read_within` is removed.
9. Not fixed. Older than R4-B and not requested for this pass: remembering `(primary, shown)` when the inspector name field gains focus is left for a follow-up.
