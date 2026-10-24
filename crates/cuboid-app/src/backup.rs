//! Autosave files in the recovery folder: atomic writes, a bounded number of files per document,
//! and the start-up scan that finds what a crash left behind (roadmap 7).
//!
//! Every autosave is a pair: `<session>_<n>.cubik` and a `.json` with the document it belongs to.
//! The JSON is written last and is the commit marker; a `.cubik` without one is junk.
//!
//! The folder is shared by every run and every open instance. A write only trims the files of its
//! own session and document, a running session holds `<session>.lock`, and the scan never deletes
//! on weak evidence: a pair it cannot judge stays and is offered.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cuboid_fmt_cubik::CubikError;
use cuboid_io::write_atomic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::editor::SaveInput;

/// Autosaves kept per document of a session, the newest first.
pub const KEEP: usize = 10;
/// Files without a partner this old are junk.
const JUNK_AGE: Duration = Duration::from_secs(120);
/// The newest `Meta` version this program knows; pairs of a newer one are kept untouched.
const META_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error(transparent)]
    Cubik(#[from] CubikError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("cannot write the description: {0}")]
    Json(#[from] serde_json::Error),
}

/// Why an autosave was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Timer,
    /// Before an operation that may fail or replace the document.
    Risky,
    /// After a panic in the frame logic.
    Panic,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Reason::Timer => "timer",
            Reason::Risky => "risky",
            Reason::Panic => "panic",
        }
    }
}

/// How a file looked when the session opened or saved it. Both values come from the same file
/// system, so a later difference means somebody wrote the file in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    modified_ms: i64,
    size: u64,
}

impl Stamp {
    pub fn of(path: &Path) -> Option<Stamp> {
        let meta = fs::metadata(path).ok()?;
        Some(Stamp {
            modified_ms: system_ms(meta.modified().ok()?),
            size: meta.len(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    session: String,
    /// The document of the session the autosave belongs to.
    #[serde(default)]
    doc_id: u64,
    /// The file the document was opened from or saved to.
    source: Option<String>,
    /// That file as the session last saw it.
    #[serde(default)]
    stamp: Option<Stamp>,
    title: String,
    saved_ms: i64,
    reason: String,
}

/// An autosave the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub cubik: PathBuf,
    meta: PathBuf,
    pub source: Option<PathBuf>,
    pub title: String,
    pub saved_ms: i64,
    pub reason: String,
    /// The file was written after the session saw it last; the autosave may not contain that.
    pub source_changed: bool,
    pub stamp: Option<Stamp>,
    session: String,
    doc_id: u64,
}

/// A name for this run of the program; the files of a session start with it.
pub fn session_id() -> String {
    format!("{}p{}", now_ms(), std::process::id())
}

fn now_ms() -> i64 {
    system_ms(SystemTime::now())
}

fn system_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
    })
}

/// Marks a session as running: an exclusive lock on `<session>.lock` until the value is dropped
/// or the process ends, however it ends.
pub struct SessionLock {
    _file: File,
}

impl SessionLock {
    pub fn acquire(dir: &Path, session: &str) -> io::Result<SessionLock> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(lock_path(dir, session))?;
        file.try_lock().map_err(io::Error::other)?;
        Ok(SessionLock { _file: file })
    }
}

fn lock_path(dir: &Path, session: &str) -> PathBuf {
    dir.join(format!("{session}.lock"))
}

/// True unless the session is known to be gone. A lock file that cannot be judged counts as held.
fn session_is_live(dir: &Path, session: &str) -> bool {
    match OpenOptions::new().write(true).open(lock_path(dir, session)) {
        Ok(file) => file.try_lock().is_err(),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// What an autosave records about its document.
pub struct Origin<'a> {
    pub session: &'a str,
    pub seq: u32,
    pub doc_id: u64,
    pub source: Option<&'a Path>,
    pub stamp: Option<Stamp>,
    pub title: &'a str,
    pub reason: Reason,
}

/// Writes one autosave pair into `dir` and trims the older ones of the same session and document.
/// Runs on the I/O thread.
pub fn write(dir: &Path, origin: &Origin<'_>, input: &SaveInput) -> Result<PathBuf, BackupError> {
    fs::create_dir_all(dir)?;
    let stem = format!("{}_{:04}", origin.session, origin.seq);
    let cubik = dir.join(format!("{stem}.cubik"));
    input.write(&cubik, false)?;
    let meta = Meta {
        version: META_VERSION,
        session: origin.session.to_owned(),
        doc_id: origin.doc_id,
        source: origin.source.map(|path| path.display().to_string()),
        stamp: origin.stamp,
        title: origin.title.to_owned(),
        saved_ms: now_ms(),
        reason: origin.reason.as_str().to_owned(),
    };
    write_atomic(
        &dir.join(format!("{stem}.json")),
        &serde_json::to_vec_pretty(&meta)?,
    )?;
    let own = list(dir, JUNK_AGE)
        .into_iter()
        .filter(|pair| pair.session == origin.session && pair.doc_id == origin.doc_id);
    for old in own.skip(KEEP) {
        remove_pair(&old);
    }
    Ok(cubik)
}

fn extension(path: &Path) -> Option<&str> {
    path.extension().and_then(|ext| ext.to_str())
}

fn file_age(path: &Path) -> Duration {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .unwrap_or_default()
}

/// A description that does not parse as the current `Meta` is junk, unless a newer version of the
/// program wrote it.
fn is_newer_version(bytes: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    serde_json::from_slice::<Version>(bytes).is_ok_and(|found| found.version > META_VERSION)
}

/// The pairs of the folder, newest first, with their descriptions. Removes only what is certainly
/// junk and at least `junk_age` old: a description that was read and is not valid, a pair half
/// missing, a temporary file of a cut-off write. A file that cannot be read stays.
fn list(dir: &Path, junk_age: Duration) -> Vec<Candidate> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .collect();
    paths.sort();
    let mut pairs = Vec::new();
    // The `.cubik` files that belong to a description that stays.
    let mut kept = HashSet::new();
    for path in &paths {
        if extension(path) != Some("json") {
            continue;
        }
        let cubik = path.with_extension("cubik");
        let Ok(bytes) = fs::read(path) else {
            kept.insert(cubik);
            continue;
        };
        match serde_json::from_slice::<Meta>(&bytes) {
            Ok(meta) if cubik.is_file() => {
                kept.insert(cubik.clone());
                pairs.push(Candidate {
                    cubik,
                    meta: path.clone(),
                    source: meta.source.map(PathBuf::from),
                    title: meta.title,
                    saved_ms: meta.saved_ms,
                    reason: meta.reason,
                    source_changed: false,
                    stamp: meta.stamp,
                    session: meta.session,
                    doc_id: meta.doc_id,
                });
            }
            Err(_) if is_newer_version(&bytes) => {
                kept.insert(cubik);
            }
            // A description without its file, or one that is not valid.
            _ if file_age(path) >= junk_age => {
                let _ = fs::remove_file(path);
                let _ = fs::remove_file(&cubik);
            }
            _ => {
                kept.insert(cubik);
            }
        }
    }
    for path in paths {
        let name = path.file_name().and_then(|name| name.to_str());
        let orphan = extension(&path) == Some("cubik") && !kept.contains(&path);
        let temporary = name.is_some_and(|name| name.starts_with(".tmp"));
        if (orphan || temporary) && file_age(&path) >= junk_age {
            let _ = fs::remove_file(&path);
        }
    }
    pairs.sort_by(|a, b| (b.saved_ms, &b.cubik).cmp(&(a.saved_ms, &a.cubik)));
    pairs
}

fn remove_pair(pair: &Candidate) {
    let _ = fs::remove_file(&pair.meta);
    let _ = fs::remove_file(&pair.cubik);
}

/// Removes the autosaves of the given documents of one session, after a save or a clean exit made
/// them redundant. Nothing of another session or document is touched.
pub fn purge_docs(dir: &Path, session: &str, doc_ids: &[u64]) {
    for pair in list(dir, JUNK_AGE) {
        if pair.session == session && doc_ids.contains(&pair.doc_id) {
            remove_pair(&pair);
        }
    }
}

/// Removes every autosave of the document the candidate stands for, because the user dropped it
/// or it was recovered.
pub fn discard(candidate: &Candidate) {
    if let Some(dir) = candidate.cubik.parent() {
        for pair in list(dir, JUNK_AGE) {
            if pair.session == candidate.session && pair.doc_id == candidate.doc_id {
                remove_pair(&pair);
            }
        }
    }
    remove_pair(candidate);
}

/// The autosaves worth offering: the newest of each document of every session that is not
/// running (`own` is the caller's session and is always listed). Deletes nothing but junk and the
/// lock files of dead sessions.
pub fn scan(dir: &Path, own: &str) -> Vec<Candidate> {
    let mut live: HashMap<String, bool> = HashMap::new();
    let mut seen = HashSet::new();
    let mut offered = Vec::new();
    for mut pair in list(dir, JUNK_AGE) {
        let running = pair.session != own
            && *live
                .entry(pair.session.clone())
                .or_insert_with(|| session_is_live(dir, &pair.session));
        if running {
            continue;
        }
        pair.source_changed = match (&pair.source, pair.stamp) {
            (Some(source), Some(stamp)) => Stamp::of(source).is_some_and(|now| now != stamp),
            _ => false,
        };
        // One entry per document: the list is newest first.
        let key = (pair.session.clone(), pair.doc_id, pair.source.clone());
        if seen.insert(key) {
            offered.push(pair);
        }
    }
    remove_dead_locks(dir, own);
    offered.truncate(KEEP);
    offered
}

/// Removes the lock files of sessions that are gone. A fresh file may belong to a session that has
/// not locked it yet.
fn remove_dead_locks(dir: &Path, own: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(|entry| entry.ok()).map(|e| e.path()) {
        let session = path.file_stem().and_then(|stem| stem.to_str());
        let Some(session) = session.filter(|_| extension(&path) == Some("lock")) else {
            continue;
        };
        if session != own && file_age(&path) >= JUNK_AGE && !session_is_live(dir, session) {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::editor::Editor;

    fn origin_doc<'a>(
        session: &'a str,
        seq: u32,
        doc_id: u64,
        source: Option<&'a Path>,
    ) -> Origin<'a> {
        Origin {
            session,
            seq,
            doc_id,
            source,
            stamp: None,
            title: "Model",
            reason: Reason::Timer,
        }
    }

    fn origin<'a>(session: &'a str, seq: u32, source: Option<&'a Path>) -> Origin<'a> {
        origin_doc(session, seq, 1, source)
    }

    fn input() -> SaveInput {
        Editor::new(1252).save_input(None)
    }

    fn count(dir: &Path, extension: &str) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|e| e == extension)
            })
            .count()
    }

    #[test]
    fn a_written_autosave_is_a_pair_that_loads_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &origin("s1", 1, None), &input()).unwrap();
        assert!(path.is_file());
        assert!(dir.path().join("s1_0001.json").is_file());
        cuboid_fmt_cubik::load_path(&path, &cuboid_fmt_cubik::Limits::default()).unwrap();
        // The atomic write leaves no temporary file behind.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn scan_offers_an_untitled_autosave() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &origin("s1", 1, None), &input()).unwrap();
        let found = scan(dir.path(), "");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Model");
        assert_eq!(found[0].source, None);
        assert_eq!(found[0].reason, "timer");
        assert!(!found[0].source_changed);
    }

    #[test]
    fn scan_keeps_and_flags_an_autosave_whose_file_changed_later() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("model.cubik");
        fs::write(&doc, b"as the session saw it").unwrap();
        let mut seen = origin("s1", 1, Some(&doc));
        seen.stamp = Stamp::of(&doc);
        write(dir.path(), &seen, &input()).unwrap();
        assert!(!scan(dir.path(), "")[0].source_changed);

        // Somebody else writes the file after the autosave: the autosave stays and says so.
        fs::write(&doc, b"written by another instance, a different size").unwrap();
        let found = scan(dir.path(), "");
        assert_eq!(found.len(), 1);
        assert!(found[0].source_changed);
        assert!(dir.path().join("s1_0001.cubik").is_file());
        assert!(dir.path().join("s1_0001.json").is_file());
    }

    #[test]
    fn scan_offers_a_document_whose_file_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone.cubik");
        let mut gone_origin = origin("s1", 1, Some(&gone));
        gone_origin.stamp = Some(Stamp {
            modified_ms: 1,
            size: 1,
        });
        write(dir.path(), &gone_origin, &input()).unwrap();
        let found = scan(dir.path(), "");
        assert_eq!(found.len(), 1);
        assert!(!found[0].source_changed);
    }

    #[test]
    fn scan_lists_the_newest_autosave_of_each_document_first() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &origin("s1", 1, None), &input()).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        write(dir.path(), &origin("s1", 2, None), &input()).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        write(dir.path(), &origin("s2", 1, None), &input()).unwrap();
        let found = scan(dir.path(), "");
        assert_eq!(found.len(), 2);
        assert!(found[0].cubik.ends_with("s2_0001.cubik"));
        assert!(found[1].cubik.ends_with("s1_0002.cubik"));
    }

    #[test]
    fn two_untitled_documents_of_a_session_are_both_offered() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &origin_doc("s1", 1, 1, None), &input()).unwrap();
        write(dir.path(), &origin_doc("s1", 2, 2, None), &input()).unwrap();
        assert_eq!(scan(dir.path(), "").len(), 2);
    }

    #[test]
    fn the_number_of_autosaves_of_a_document_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        for seq in 0..u32::try_from(KEEP).unwrap() + 3 {
            write(dir.path(), &origin("s1", seq, None), &input()).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(count(dir.path(), "cubik"), KEEP);
        assert_eq!(count(dir.path(), "json"), KEEP);
        assert!(!dir.path().join("s1_0000.cubik").exists());
    }

    #[test]
    fn a_write_never_removes_the_files_of_another_session() {
        let dir = tempfile::tempdir().unwrap();
        // An older run holds work the user postponed with "Decide later".
        write(dir.path(), &origin("old", 1, None), &input()).unwrap();
        for seq in 0..u32::try_from(KEEP).unwrap() * 2 {
            write(dir.path(), &origin("new", seq, None), &input()).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(dir.path().join("old_0001.cubik").is_file());
        assert!(dir.path().join("old_0001.json").is_file());
        assert_eq!(count(dir.path(), "json"), KEEP + 1);
    }

    #[test]
    fn a_write_only_trims_its_own_document() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &origin_doc("s1", 1, 1, None), &input()).unwrap();
        for seq in 2..u32::try_from(KEEP).unwrap() + 5 {
            write(dir.path(), &origin_doc("s1", seq, 2, None), &input()).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            dir.path().join("s1_0001.json").is_file(),
            "document 1 stays"
        );
        assert_eq!(count(dir.path(), "json"), KEEP + 1);
    }

    #[test]
    fn purge_removes_only_the_given_documents_of_the_session() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &origin_doc("s1", 1, 1, None), &input()).unwrap();
        write(dir.path(), &origin_doc("s1", 2, 2, None), &input()).unwrap();
        write(dir.path(), &origin_doc("s2", 1, 1, None), &input()).unwrap();
        purge_docs(dir.path(), "s1", &[1]);
        assert!(!dir.path().join("s1_0001.json").exists());
        assert!(dir.path().join("s1_0002.json").is_file());
        assert!(dir.path().join("s2_0001.json").is_file());
    }

    #[test]
    fn junk_files_are_removed_once_old_and_a_missing_folder_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bad.json"), b"{ not json").unwrap();
        fs::write(dir.path().join("orphan.cubik"), b"x").unwrap();
        fs::write(dir.path().join(".tmpAbC123"), b"cut off").unwrap();
        // Fresh files may belong to a write that is still running.
        assert!(scan(dir.path(), "").is_empty());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
        assert!(list(dir.path(), Duration::ZERO).is_empty());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(scan(&dir.path().join("missing"), "").is_empty());
    }

    #[test]
    fn a_description_that_cannot_be_read_or_is_from_a_newer_version_stays() {
        let dir = tempfile::tempdir().unwrap();
        // A folder in place of the description: the read fails like a sharing violation would.
        fs::create_dir(dir.path().join("s1_0001.json")).unwrap();
        fs::write(dir.path().join("s1_0001.cubik"), b"autosave").unwrap();
        fs::write(
            dir.path().join("s2_0001.json"),
            br#"{"version": 2, "other": "shape"}"#,
        )
        .unwrap();
        fs::write(dir.path().join("s2_0001.cubik"), b"autosave").unwrap();
        assert!(list(dir.path(), Duration::ZERO).is_empty());
        assert!(dir.path().join("s1_0001.cubik").is_file());
        assert!(dir.path().join("s2_0001.cubik").is_file());
        assert!(dir.path().join("s2_0001.json").is_file());
    }

    #[test]
    fn discard_removes_all_files_of_the_document() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &origin_doc("s1", 1, 1, None), &input()).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        write(dir.path(), &origin_doc("s1", 2, 1, None), &input()).unwrap();
        write(dir.path(), &origin_doc("s1", 3, 2, None), &input()).unwrap();
        let found = scan(dir.path(), "");
        let first = found.iter().find(|c| c.cubik.ends_with("s1_0002.cubik"));
        discard(first.unwrap());
        assert_eq!(count(dir.path(), "json"), 1);
        assert!(dir.path().join("s1_0003.json").is_file());
    }

    #[test]
    fn a_running_session_is_not_offered_and_a_dead_one_is() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &origin("alive", 1, None), &input()).unwrap();
        write(dir.path(), &origin("dead", 1, None), &input()).unwrap();
        let lock = SessionLock::acquire(dir.path(), "alive").unwrap();
        // A crashed run left its lock file behind; nobody holds the lock.
        drop(SessionLock::acquire(dir.path(), "dead").unwrap());
        let found = scan(dir.path(), "other");
        assert_eq!(found.len(), 1);
        assert!(found[0].cubik.ends_with("dead_0001.cubik"));
        // The running session sees its own files.
        assert_eq!(scan(dir.path(), "alive").len(), 2);
        drop(lock);
        assert_eq!(scan(dir.path(), "other").len(), 2);
    }

    #[test]
    fn a_second_lock_on_a_running_session_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _first = SessionLock::acquire(dir.path(), "s1").unwrap();
        assert!(SessionLock::acquire(dir.path(), "s1").is_err());
    }
}
