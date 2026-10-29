//! When the recent files were last changed, for the start screen.
//!
//! A recent file can sit on a share that is offline, where one lookup waits tens of seconds. The
//! lookups run on a helper thread, one batch at a time, and a frame shows the last answer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use cuboid_ui::RecentEntry;

/// How long a modification time read from the disk is kept.
const KEEP: Duration = Duration::from_secs(30);

/// How often a frame looks for the answer of a batch that is out.
const POLL: Duration = Duration::from_secs(1);

type Answer = Vec<(PathBuf, Option<SystemTime>)>;

pub(super) struct RecentTimes {
    seen: HashMap<PathBuf, (Option<SystemTime>, Instant)>,
    /// The batch the helper thread is working on.
    batch: Option<Receiver<Answer>>,
    lookup: fn(&Path) -> Option<SystemTime>,
}

impl Default for RecentTimes {
    fn default() -> Self {
        Self::with_lookup(|path| std::fs::metadata(path).and_then(|m| m.modified()).ok())
    }
}

impl RecentTimes {
    fn with_lookup(lookup: fn(&Path) -> Option<SystemTime>) -> Self {
        Self {
            seen: HashMap::new(),
            batch: None,
            lookup,
        }
    }

    /// Sets `modified` of each entry to the last answer. The disk is asked again for the files
    /// whose answer is missing or older than 30 seconds, unless a batch is still out. The shell
    /// wakes an idle window only for the delay a frame reports, not for a repaint requested by
    /// another thread, so while a batch is out the frame asks to come back in a second.
    pub(super) fn fill(&mut self, entries: &mut [RecentEntry], now: Instant, wake: &egui::Context) {
        if let Some(batch) = &self.batch {
            match batch.try_recv() {
                Ok(answer) => {
                    self.batch = None;
                    for (path, time) in answer {
                        self.seen.insert(path, (time, now));
                    }
                }
                Err(TryRecvError::Disconnected) => self.batch = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.batch.is_none() {
            let expired: Vec<PathBuf> = entries
                .iter()
                .filter(|entry| {
                    let seen = self.seen.get(&entry.path);
                    seen.is_none_or(|(_, at)| now.saturating_duration_since(*at) >= KEEP)
                })
                .map(|entry| entry.path.clone())
                .collect();
            if !expired.is_empty() {
                self.ask(expired, now, wake);
            }
        }
        if self.batch.is_some() {
            wake.request_repaint_after(POLL);
        }
        for entry in entries {
            entry.modified = self.seen.get(&entry.path).and_then(|(time, _)| *time);
        }
    }

    fn ask(&mut self, paths: Vec<PathBuf>, now: Instant, wake: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (lookup, wake, batch) = (self.lookup, wake.clone(), paths.clone());
        let spawned = thread::Builder::new()
            .name("cuboid-recent".to_owned())
            .spawn(move || {
                let answer = batch.into_iter().map(|path| {
                    let time = lookup(&path);
                    (path, time)
                });
                let _ = tx.send(answer.collect());
                wake.request_repaint();
            });
        match spawned {
            Ok(_) => self.batch = Some(rx),
            Err(error) => {
                // Try again after the usual wait rather than on every frame.
                log::warn!("cannot start the thread for the recent files: {error}");
                for path in paths {
                    self.seen.insert(path, (None, now));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::thread::ThreadId;

    use super::*;

    fn entry(path: &str) -> RecentEntry {
        RecentEntry {
            path: PathBuf::from(path),
            name: String::new(),
            detail: String::new(),
            thumbnail: None,
            modified: None,
        }
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// Runs frames at `now` until `done` holds.
    fn frames_until(
        times: &mut RecentTimes,
        entries: &mut [RecentEntry],
        now: Instant,
        done: impl Fn(&[RecentEntry]) -> bool,
    ) {
        let wake = egui::Context::default();
        let start = Instant::now();
        loop {
            times.fill(entries, now, &wake);
            if done(entries) {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "no answer");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_lookup_that_hangs_never_holds_the_frame_and_the_old_value_stays() {
        static RELEASED: AtomicBool = AtomicBool::new(false);
        static LOOKUP_THREAD: Mutex<Option<ThreadId>> = Mutex::new(None);

        fn lookup(_: &Path) -> Option<SystemTime> {
            *LOOKUP_THREAD.lock().unwrap() = Some(thread::current().id());
            // The share is offline until the test says otherwise.
            let start = Instant::now();
            while !RELEASED.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(10) {
                thread::sleep(Duration::from_millis(1));
            }
            Some(at(200))
        }

        let wake = egui::Context::default();
        let mut times = RecentTimes::with_lookup(lookup);
        let base = Instant::now();
        times.seen.insert("a.cubik".into(), (Some(at(100)), base));
        let mut entries = [entry("a.cubik"), entry("b.cubik")];

        // The answer for `a` is old: the frame returns at once, still with the old value, and
        // has nothing for `b`.
        let now = base + KEEP * 2;
        let start = Instant::now();
        times.fill(&mut entries, now, &wake);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(entries[0].modified, Some(at(100)));
        assert_eq!(entries[1].modified, None);
        assert!(times.batch.is_some());

        // More frames while the batch is out neither block nor change the shown value.
        for _ in 0..3 {
            times.fill(&mut entries, now, &wake);
        }
        assert_eq!(entries[0].modified, Some(at(100)));

        RELEASED.store(true, Ordering::SeqCst);
        frames_until(&mut times, &mut entries, now, |entries| {
            entries.iter().all(|entry| entry.modified == Some(at(200)))
        });
        assert_ne!(*LOOKUP_THREAD.lock().unwrap(), Some(thread::current().id()));
    }

    /// Runs one egui pass with a fill and returns the delay the pass reports to the shell.
    fn pass(
        ctx: &egui::Context,
        times: &mut RecentTimes,
        entries: &mut [RecentEntry],
        now: Instant,
    ) -> Duration {
        let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
            times.fill(entries, now, ctx);
        });
        // Nothing is painted here, so nobody applies the texture changes.
        out.textures_delta.clear();
        out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
    }

    #[test]
    fn a_pass_with_a_batch_out_asks_to_come_back_and_an_idle_one_does_not() {
        static RELEASED: AtomicBool = AtomicBool::new(false);

        fn lookup(_: &Path) -> Option<SystemTime> {
            let start = Instant::now();
            while !RELEASED.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(10) {
                thread::sleep(Duration::from_millis(1));
            }
            Some(at(1))
        }

        let ctx = egui::Context::default();
        let mut times = RecentTimes::with_lookup(lookup);
        let mut entries = [entry("a.cubik")];
        let now = Instant::now();
        // egui repaints its first passes whatever is asked; then the batch decides.
        let mut delays: Vec<Duration> = Vec::new();
        while delays.last() != Some(&POLL) && delays.len() < 5 {
            delays.push(pass(&ctx, &mut times, &mut entries, now));
        }
        assert_eq!(delays.last(), Some(&POLL), "{delays:?}");

        RELEASED.store(true, Ordering::SeqCst);
        frames_until(&mut times, &mut entries, now, |entries| {
            entries[0].modified == Some(at(1))
        });
        // The helper's own repaint request is served by one more pass; then the frames rest.
        let start = Instant::now();
        while pass(&ctx, &mut times, &mut entries, now) != Duration::MAX {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "frames keep coming"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn an_answer_is_kept_for_thirty_seconds_and_then_asked_again() {
        static CALLS: AtomicU64 = AtomicU64::new(0);

        fn lookup(_: &Path) -> Option<SystemTime> {
            Some(at(CALLS.fetch_add(1, Ordering::SeqCst) + 1))
        }

        let wake = egui::Context::default();
        let mut times = RecentTimes::with_lookup(lookup);
        let mut entries = [entry("a.cubik")];
        let start = Instant::now();
        frames_until(&mut times, &mut entries, start, |entries| {
            entries[0].modified == Some(at(1))
        });

        // Within the wait, frames do not ask again.
        times.fill(&mut entries, start + KEEP / 2, &wake);
        assert!(times.batch.is_none());
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);

        // After it, the first frame still shows the old value; the new one arrives later.
        let later = start + KEEP * 2;
        times.fill(&mut entries, later, &wake);
        assert_eq!(entries[0].modified, Some(at(1)));
        frames_until(&mut times, &mut entries, later, |entries| {
            entries[0].modified == Some(at(2))
        });
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    }
}
