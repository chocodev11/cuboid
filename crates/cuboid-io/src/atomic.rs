//! Atomic file replacement.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use tempfile::{Builder, NamedTempFile};

/// Start of the name of the temporary file of a write, so a leftover of a killed write can be
/// told from the user's files.
pub const TEMP_PREFIX: &str = ".cuboid-tmp-";

/// A temporary file this old belongs to a write that was cut off.
const STALE_AGE: Duration = Duration::from_secs(60 * 60);

/// The folders whose leftovers this run has looked for. A leaf lock: held for one insert, never
/// while calling anything else.
static CLEANED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// Writes `bytes` to `path` atomically: a temporary file in the target folder, flushed to disk,
/// then renamed over the target. A failure at any point leaves the previous file intact.
/// Documents, backups, settings and presets are written this way; the I/O thread calls it.
///
/// The replacing file keeps the permissions of the replaced one; a new file gets the default
/// permissions of a temporary file. At the first write into a folder in this run, temporary files
/// of cut-off writes (named with [`TEMP_PREFIX`], at least an hour older than the write's own
/// temporary file) are removed from it. On Unix the folder is synced after the rename, so a power
/// cut cannot bring the old file back.
///
/// spec: arch 15, app 0.2 (D8)
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // A bare file name has an empty parent, which means the current folder.
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = temp_in(dir)?;
    clean_once(dir, &temp);
    temp.write_all(bytes)?;
    let file = temp.as_file();
    file.sync_all()?;
    if let Ok(existing) = fs::metadata(path) {
        file.set_permissions(existing.permissions())?;
    }
    temp.persist(path).map_err(|failed| failed.error)?;
    // Makes the rename durable. The file is already replaced, so a folder that cannot be synced
    // (some network file systems refuse) is not a failed write.
    #[cfg(unix)]
    let _ = fs::File::open(dir).and_then(|folder| folder.sync_all());
    Ok(())
}

fn temp_in(dir: &Path) -> io::Result<NamedTempFile> {
    Builder::new().prefix(TEMP_PREFIX).tempfile_in(dir)
}

/// Looks for leftovers in `dir` the first time a write goes there. They only come from killed
/// processes, so one look per folder is enough, and many writes into one folder do not list it
/// again each time.
fn clean_once(dir: &Path, temp: &NamedTempFile) {
    // The age is measured on the clock of the file system: a file server stamps its files with its
    // own time, which may differ from this machine's by hours.
    let Some(reference) = temp
        .as_file()
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
    else {
        return;
    };
    let first = CLEANED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(dir.to_path_buf());
    if first && let Some(own) = temp.path().file_name() {
        remove_stale(dir, own, reference);
    }
}

/// Removes the temporary files that a killed process left in `dir`, except the one named `own`.
/// A file is stale when it is at least [`STALE_AGE`] older than `reference`, the time the file
/// system gave the write's own file; that keeps the files of writes that are running now, in this
/// or another process.
fn remove_stale(dir: &Path, own: &OsStr, reference: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == own || !name.to_string_lossy().starts_with(TEMP_PREFIX) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| reference.duration_since(time).ok())
            .is_some_and(|age| age >= STALE_AGE);
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests;
