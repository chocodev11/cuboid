//! Atomic file replacement.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

/// Writes `bytes` to `path` atomically: a temporary file in the target folder, flushed to disk,
/// then renamed over the target. A failure at any point leaves the previous file intact.
/// Documents, backups, settings and presets are written this way; the I/O thread calls it.
///
/// The replacing file keeps the permissions of the replaced one; a new file gets the default
/// permissions of a temporary file.
///
/// spec: arch 15, app 0.2 (D8)
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // A bare file name has an empty parent, which means the current folder.
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    let file = temp.as_file();
    file.sync_all()?;
    if let Ok(existing) = fs::metadata(path) {
        file.set_permissions(existing.permissions())?;
    }
    temp.persist(path).map_err(|failed| failed.error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
