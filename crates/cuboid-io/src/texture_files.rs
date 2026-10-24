//! Reading texture files that a model file names: the refusals of paths that reach the network or
//! a device, a size cap and the check that only regular files are opened. The importers of the
//! `.cub` and `.bbmodel` formats share it.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use thiserror::Error;

/// Why a texture file named by a model file is not read. The import continues without the
/// texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Refusal {
    /// A network path (`\\host\share` or `//host/share`).
    #[error("network paths are not read")]
    Network,
    /// A device or namespace path (`\\.\`, `\\?\`), a reserved device name (`CON`, `NUL`,
    /// `COM1`, ...), a stream name or a drive-relative path.
    #[error("device paths and special names are not read")]
    Device,
    /// The path leads to something that is not a regular file.
    #[error("not a regular file")]
    NotRegular,
    /// The file is larger than the limit, or the textures together are.
    #[error("too large")]
    TooLarge,
}

/// Finds texture files for the importers.
pub trait TextureFiles {
    /// The bytes of the file at `path`: `Ok(None)` when it does not exist, an error when the
    /// path or the file is not one to read.
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, Refusal>;
}

/// The largest texture file [`DiskFiles`] reads.
pub const MAX_TEXTURE_FILE: u64 = 64 << 20;

/// The file system: regular local files only, read up to [`MAX_TEXTURE_FILE`] bytes. Network and
/// device paths are refused by their form, before the system is asked anything about them.
pub struct DiskFiles;

impl TextureFiles for DiskFiles {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        if let Form::Refused(reason) = form(&path.to_string_lossy()) {
            return Err(reason);
        }
        // `metadata` does not open the file, so a pipe or a device cannot block it.
        let Ok(meta) = std::fs::metadata(path) else {
            return Ok(None);
        };
        if meta.is_dir() {
            return Ok(None);
        }
        if !meta.is_file() {
            return Err(Refusal::NotRegular);
        }
        if meta.len() > MAX_TEXTURE_FILE {
            return Err(Refusal::TooLarge);
        }
        let Ok(file) = File::open(path) else {
            return Ok(None);
        };
        // The file may have been replaced since the check above.
        if !file.metadata().is_ok_and(|meta| meta.is_file()) {
            return Err(Refusal::NotRegular);
        }
        let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
        if file
            .take(MAX_TEXTURE_FILE + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Ok(None);
        }
        if bytes.len() as u64 > MAX_TEXTURE_FILE {
            return Err(Refusal::TooLarge);
        }
        Ok(Some(bytes))
    }
}

/// How a path named by a file is rooted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Joined to the folder of the model file.
    Relative,
    /// A rooted path on a local drive or file system.
    Local,
    /// A path that is never read.
    Refused(Refusal),
}

/// Looks at the text of a path only, with `\` and `/` alike: the forms that reach the network
/// or a device are refused before the system sees them.
pub fn form(path: &str) -> Form {
    let path = path.replace('\\', "/");
    if let Some(rest) = path.strip_prefix("//") {
        // `//./` and `//?/` are the device and verbatim namespaces, anything else a host.
        let namespace =
            rest.starts_with(['.', '?']) && matches!(rest.as_bytes().get(1), None | Some(&b'/'));
        return Form::Refused(if namespace {
            Refusal::Device
        } else {
            Refusal::Network
        });
    }
    if path.starts_with("/??/") || path == "/dev" || path.starts_with("/dev/") {
        return Form::Refused(Refusal::Device);
    }
    let bytes = path.as_bytes();
    let drive = bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':');
    if drive && bytes.get(2) != Some(&b'/') {
        // `C:name` is relative to the current folder of a drive.
        return Form::Refused(Refusal::Device);
    }
    let body = if drive { path.get(2..) } else { Some(&*path) };
    for component in body.unwrap_or_default().split('/') {
        // A colon names a stream (`name:stream`) or a device (`NUL:`).
        if component.contains(':') || reserved(component) {
            return Form::Refused(Refusal::Device);
        }
    }
    if drive || path.starts_with('/') {
        Form::Local
    } else {
        Form::Relative
    }
}

/// A Windows device name, with any extension (`nul.png` is the device too).
fn reserved(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or_default();
    let upper = stem.trim_end_matches(' ').to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) {
        return true;
    }
    // COM0..COM9 and LPT0..LPT9, also with the superscript digits 1, 2 and 3.
    let mut chars = upper.chars();
    let head: String = chars.by_ref().take(3).collect();
    (head == "COM" || head == "LPT")
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_digit() || matches!(u32::from(c), 0xB9 | 0xB2 | 0xB3))
        && chars.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_and_device_forms_are_refused_by_their_text() {
        let refused = |path: &str| match form(path) {
            Form::Refused(reason) => Some(reason),
            _ => None,
        };
        assert_eq!(refused(r"\\host\share\a.png"), Some(Refusal::Network));
        assert_eq!(refused("//host/share/a.png"), Some(Refusal::Network));
        assert_eq!(refused(r"\\.\COM1"), Some(Refusal::Device));
        assert_eq!(refused(r"\\?\C:\a.png"), Some(Refusal::Device));
        assert_eq!(refused("C:a.png"), Some(Refusal::Device));
        assert_eq!(refused("dir/nul.png"), Some(Refusal::Device));
        assert_eq!(refused("a.png:stream"), Some(Refusal::Device));
        assert_eq!(refused("/dev/zero"), Some(Refusal::Device));
    }

    #[test]
    fn ordinary_paths_are_relative_or_local() {
        assert_eq!(form("textures/a.png"), Form::Relative);
        assert_eq!(form("../a.png"), Form::Relative);
        assert_eq!(form("C:/a/b.png"), Form::Local);
        assert_eq!(form("/home/a.png"), Form::Local);
    }

    #[test]
    fn a_missing_file_is_none_and_a_folder_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(DiskFiles.read(&dir.path().join("none.png")), Ok(None));
        assert_eq!(DiskFiles.read(dir.path()), Ok(None));
        let file = dir.path().join("a.png");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(DiskFiles.read(&file), Ok(Some(b"abc".to_vec())));
    }
}
