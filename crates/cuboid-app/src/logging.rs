//! File logger with size-based rotation and a ring buffer of the last lines for crash reports
//! (spec: arch 16).

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Name of the active log file; rotated files are `cuboid.1.log`, `cuboid.2.log`, ...
const ACTIVE: &str = "cuboid.log";
/// Files kept, the active one included.
const KEEP: usize = 3;
/// Size at which the active file rotates.
const MAX_BYTES: u64 = 5 << 20;
/// Lines kept for the crash report.
const RING_LINES: usize = 2000;

static LOGGER: OnceLock<Logger> = OnceLock::new();

struct Sink {
    dir: PathBuf,
    file: Option<File>,
    written: u64,
    max_bytes: u64,
    ring: VecDeque<String>,
}

pub struct Logger {
    level: LevelFilter,
    echo: bool,
    sink: Mutex<Sink>,
}

/// Installs the global logger. A second call keeps the first logger.
pub fn init(dir: &Path, level: LevelFilter, echo: bool) {
    let logger = LOGGER.get_or_init(|| Logger::new(dir, level, echo, MAX_BYTES));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
}

/// The last `count` lines, oldest first. Gives nothing when the sink is locked, so that the
/// panic hook cannot deadlock on a panic inside the logger.
pub fn recent_lines(count: usize) -> Vec<String> {
    let Some(logger) = LOGGER.get() else {
        return Vec::new();
    };
    let sink = match logger.sink.try_lock() {
        Ok(sink) => sink,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return Vec::new(),
    };
    let skip = sink.ring.len().saturating_sub(count);
    sink.ring.iter().skip(skip).cloned().collect()
}

/// The level from `--debug` and `CUBOID_LOG` (`error`, `warn`, `info`, `debug`, `trace`).
pub fn level(debug_flag: bool) -> LevelFilter {
    let from_env = std::env::var("CUBOID_LOG")
        .ok()
        .and_then(|text| text.trim().parse::<LevelFilter>().ok());
    from_env.unwrap_or(if debug_flag {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    })
}

impl Logger {
    fn new(dir: &Path, level: LevelFilter, echo: bool, max_bytes: u64) -> Logger {
        let mut sink = Sink {
            dir: dir.to_owned(),
            file: None,
            written: 0,
            max_bytes,
            ring: VecDeque::with_capacity(RING_LINES),
        };
        sink.open();
        Logger {
            level,
            echo,
            sink: Mutex::new(sink),
        }
    }

    fn sink(&self) -> MutexGuard<'_, Sink> {
        self.sink.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Sink {
    fn path(&self, index: usize) -> PathBuf {
        if index == 0 {
            self.dir.join(ACTIVE)
        } else {
            self.dir.join(format!("cuboid.{index}.log"))
        }
    }

    /// Opens the active file for appending. A failure leaves the file logger off; the ring
    /// buffer keeps working.
    fn open(&mut self) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(0));
        if let Ok(file) = file {
            self.written = file.metadata().map_or(0, |meta| meta.len());
            self.file = Some(file);
        }
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = fs::remove_file(self.path(KEEP - 1));
        for index in (0..KEEP - 1).rev() {
            let _ = fs::rename(self.path(index), self.path(index + 1));
        }
        self.open();
    }

    fn push(&mut self, line: String) {
        let len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if self.file.is_some() && self.written + len > self.max_bytes {
            self.rotate();
        }
        if let Some(file) = &mut self.file {
            if file.write_all(line.as_bytes()).is_ok() {
                self.written += len;
            } else {
                self.file = None;
            }
        }
        if self.ring.len() == RING_LINES {
            self.ring.pop_front();
        }
        self.ring.push_back(line);
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let stamp = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!(
            "{stamp} {:5} {} {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        if self.echo || record.level() <= Level::Warn {
            eprint!("{line}");
        }
        self.sink().push(line);
    }

    fn flush(&self) {
        if let Some(file) = &mut self.sink().file {
            let _ = file.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(logger: &Logger, level: Level, text: &str) {
        logger.log(
            &Record::builder()
                .level(level)
                .target("test")
                .args(format_args!("{text}"))
                .build(),
        );
    }

    #[test]
    fn lines_reach_the_file_and_the_ring() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path(), LevelFilter::Info, false, MAX_BYTES);
        write(&logger, Level::Info, "hello");
        write(&logger, Level::Debug, "filtered out");
        logger.flush();
        let text = fs::read_to_string(dir.path().join(ACTIVE)).unwrap();
        assert!(text.contains("INFO  test hello"), "{text}");
        assert!(!text.contains("filtered out"));
        assert_eq!(logger.sink().ring.len(), 1);
    }

    #[test]
    fn the_file_rotates_by_size_and_keeps_three_files() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path(), LevelFilter::Info, false, 200);
        for index in 0..40 {
            write(&logger, Level::Info, &format!("line number {index:03}"));
        }
        logger.flush();
        for name in [ACTIVE, "cuboid.1.log", "cuboid.2.log"] {
            assert!(dir.path().join(name).is_file(), "{name} is missing");
        }
        assert!(!dir.path().join("cuboid.3.log").exists());
        let active = fs::metadata(dir.path().join(ACTIVE)).unwrap().len();
        assert!(active <= 200, "active file has {active} bytes");
        let newest = fs::read_to_string(dir.path().join(ACTIVE)).unwrap();
        assert!(newest.contains("line number 039"));
    }

    #[test]
    fn the_ring_keeps_only_the_newest_lines() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path(), LevelFilter::Info, false, MAX_BYTES);
        for index in 0..RING_LINES + 5 {
            write(&logger, Level::Info, &format!("n{index}"));
        }
        let sink = logger.sink();
        assert_eq!(sink.ring.len(), RING_LINES);
        assert!(sink.ring.front().unwrap().ends_with("n5\n"));
    }

    #[test]
    fn an_unwritable_folder_keeps_the_ring_working() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        fs::write(&blocker, b"x").unwrap();
        let logger = Logger::new(&blocker.join("logs"), LevelFilter::Info, false, MAX_BYTES);
        write(&logger, Level::Info, "still here");
        assert_eq!(logger.sink().ring.len(), 1);
    }
}
