//! Crash reports and the frame-level panic boundary (spec: arch 11.4, 16; roadmap 7).

use std::any::Any;
use std::backtrace::Backtrace;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, TryLockError};
use std::time::{Duration, Instant};

use crate::logging;

/// Crash reports kept in the folder; older ones are deleted when a new one is written.
const KEEP_REPORTS: usize = 20;
/// Log lines copied into a report.
const REPORT_LOG_LINES: usize = 200;

static REPORT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static HOOK: Once = Once::new();

/// Installs the panic hook once and points it at `dir`; a second call only changes the folder.
/// The hook writes a report and then runs the previous hook, which prints to stderr.
pub fn install_hook(dir: &Path) {
    if let Ok(mut slot) = REPORT_DIR.lock() {
        *slot = Some(dir.to_owned());
    }
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            write_from_hook(info);
            previous(info);
        }));
    });
}

/// Best effort and panic free: the hook runs while the process is already panicking.
fn write_from_hook(info: &PanicHookInfo<'_>) {
    let dir = match REPORT_DIR.try_lock() {
        Ok(slot) => slot.clone(),
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().clone(),
        Err(TryLockError::WouldBlock) => None,
    };
    let Some(dir) = dir else { return };
    let message = payload_message(info.payload());
    let location = info
        .location()
        .map_or_else(|| "unknown".to_owned(), ToString::to_string);
    let thread = std::thread::current();
    let report = format_report(
        &message,
        &location,
        thread.name().unwrap_or("unnamed"),
        &Backtrace::force_capture().to_string(),
        &logging::recent_lines(REPORT_LOG_LINES),
    );
    if let Some(path) = write_report(&dir, &report) {
        log::error!("panic: {message} at {location}; report {}", path.display());
    }
}

/// The text of a crash report: version, OS, the panic, the backtrace and the last log lines.
pub fn format_report(
    message: &str,
    location: &str,
    thread: &str,
    backtrace: &str,
    log_lines: &[String],
) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "cuboid {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        text,
        "os: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(text, "time: {}", jiff::Zoned::now());
    let _ = writeln!(text, "thread: {thread}");
    let _ = writeln!(text, "panic: {message}");
    let _ = writeln!(text, "location: {location}");
    let _ = writeln!(text, "\nbacktrace:\n{backtrace}");
    let _ = writeln!(text, "\nlast log lines:");
    for line in log_lines {
        text.push_str(line);
        if !line.ends_with('\n') {
            text.push('\n');
        }
    }
    text
}

/// Writes `crash_<time>.txt` into `dir` and trims old reports. Returns the path on success.
pub fn write_report(dir: &Path, report: &str) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S%.3f").to_string();
    let path = dir.join(format!("crash_{}.txt", stamp.replace('.', "_")));
    fs::write(&path, report).ok()?;
    trim_reports(dir);
    Some(path)
}

fn trim_reports(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut reports: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("crash_") && name.ends_with(".txt"))
        })
        .collect();
    // The names start with the time, so the order of the names is the order of the reports.
    reports.sort();
    let excess = reports.len().saturating_sub(KEEP_REPORTS);
    for old in reports.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
}

/// The text of a panic payload.
pub fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic with a non-text payload".to_owned()
    }
}

/// Runs one step of a recovery. A panic is logged and becomes `None`, so the recovery can go on
/// with its other steps instead of escaping.
pub fn catch<T>(what: &str, body: impl FnOnce() -> T) -> Option<T> {
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(payload) => {
            log::error!("panic while {what}: {}", payload_message(payload.as_ref()));
            None
        }
    }
}

/// A panic caught by [`PanicGuard::run`].
#[derive(Debug)]
pub struct Panicked {
    pub message: String,
    /// Several panics came in a short time: recover through the dialog instead of looping.
    pub repeated: bool,
}

/// Runs the app logic of one frame under `catch_unwind` and counts the panics.
pub struct PanicGuard {
    recent: VecDeque<Instant>,
    limit: usize,
    span: Duration,
}

impl Default for PanicGuard {
    /// Three panics within ten seconds count as repeated.
    fn default() -> Self {
        Self::new(3, Duration::from_secs(10))
    }
}

impl PanicGuard {
    pub fn new(limit: usize, span: Duration) -> Self {
        Self {
            recent: VecDeque::new(),
            limit,
            span,
        }
    }

    /// Runs `body`. A panic is caught and returned; the caller restores its state.
    pub fn run<T>(&mut self, body: impl FnOnce() -> T) -> Result<T, Panicked> {
        match panic::catch_unwind(AssertUnwindSafe(body)) {
            Ok(value) => Ok(value),
            Err(payload) => Err(Panicked {
                message: payload_message(payload.as_ref()),
                repeated: self.note_panic(Instant::now()),
            }),
        }
    }

    /// Records a panic at `now`; true when the recent ones reach the limit.
    fn note_panic(&mut self, now: Instant) -> bool {
        while self
            .recent
            .front()
            .is_some_and(|first| now.duration_since(*first) > self.span)
        {
            self.recent.pop_front();
        }
        self.recent.push_back(now);
        self.recent.len() >= self.limit
    }

    /// Forgets the panics seen so far, after the user dealt with the recovery dialog.
    pub fn reset(&mut self) {
        self.recent.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_is_caught_and_its_text_returned() {
        let mut guard = PanicGuard::default();
        let result = guard.run(|| -> u32 { panic!("boom {}", 7) });
        let caught = result.unwrap_err();
        assert_eq!(caught.message, "boom 7");
        assert!(!caught.repeated);
        assert_eq!(guard.run(|| 5).unwrap(), 5);
    }

    #[test]
    fn repeated_panics_in_a_short_time_escalate() {
        let mut guard = PanicGuard::new(3, Duration::from_secs(10));
        let first = Instant::now();
        assert!(!guard.note_panic(first));
        assert!(!guard.note_panic(first + Duration::from_secs(1)));
        assert!(guard.note_panic(first + Duration::from_secs(2)));
        // Panics spread over a long time do not.
        guard.reset();
        assert!(!guard.note_panic(first));
        assert!(!guard.note_panic(first + Duration::from_secs(20)));
        assert!(!guard.note_panic(first + Duration::from_secs(40)));
    }

    #[test]
    fn the_report_names_version_os_backtrace_and_log_lines() {
        let lines = vec!["a line\n".to_owned(), "b line".to_owned()];
        let text = format_report("bad", "src/x.rs:1:2", "main", "frame 0", &lines);
        assert!(text.starts_with(&format!("cuboid {}", env!("CARGO_PKG_VERSION"))));
        for part in [
            std::env::consts::OS,
            "panic: bad",
            "src/x.rs:1:2",
            "frame 0",
            "a line\nb line\n",
        ] {
            assert!(text.contains(part), "missing {part:?} in {text}");
        }
    }

    #[test]
    fn the_hook_writes_a_report_for_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        install_hook(dir.path());
        let _ = panic::catch_unwind(|| panic!("hook test marker 4711"));
        let found = fs::read_dir(dir.path()).unwrap().any(|entry| {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
            text.contains("hook test marker 4711") && text.contains("backtrace:")
        });
        // Later panics of other tests must not recreate the folder.
        *REPORT_DIR.lock().unwrap() = None;
        assert!(found, "no crash report in {}", dir.path().display());
    }

    #[test]
    fn old_reports_are_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..KEEP_REPORTS + 5 {
            fs::write(dir.path().join(format!("crash_{index:04}.txt")), "x").unwrap();
        }
        fs::write(dir.path().join("other.txt"), "x").unwrap();
        trim_reports(dir.path());
        let count = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, KEEP_REPORTS + 1);
        assert!(!dir.path().join("crash_0000.txt").exists());
    }
}
