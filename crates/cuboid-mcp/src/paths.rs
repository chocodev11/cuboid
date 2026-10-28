//! The project folder and the one resolver every path of a tool goes through (spec: agent
//! toolkit R1, 7). A path is relative to the folder, has no `..`, none of the forms that reach the
//! network or a device, and once the links on it are followed it still lies under the folder.

use std::fs;
use std::path::{Path, PathBuf};

use cuboid_io::texture_files::{Form, form};

use crate::error::ToolError;

/// Longest path a tool takes, in bytes.
const MAX_PATH_TEXT: usize = 1024;

/// The project folder.
#[derive(Clone, Debug)]
pub struct Root {
    /// The folder as the system names it; every resolved path is compared with it.
    canonical: PathBuf,
    /// The same folder without the `\\?\` prefix of a canonical Windows path, the form that the
    /// library and the importers accept.
    plain: PathBuf,
}

/// A canonical Windows path without its verbatim prefix when the rest is an ordinary drive path.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_owned(),
    }
}

impl Root {
    /// The project folder at `path`: absolute, an existing folder, not a drive root.
    pub fn new(path: &Path) -> Result<Root, String> {
        let shown = path.display();
        if !path.is_absolute() {
            return Err(format!(
                "{shown}: the project folder must be an absolute path"
            ));
        }
        if let Form::Refused(why) = form(&path.to_string_lossy()) {
            return Err(format!("{shown}: {why}"));
        }
        let canonical = fs::canonicalize(path)
            .ok()
            .filter(|real| real.is_dir())
            .ok_or_else(|| format!("{shown}: not a folder that can be read"))?;
        if canonical.parent().is_none() {
            return Err(format!("{shown}: a drive root cannot be a project folder"));
        }
        Ok(Root {
            plain: plain(&canonical),
            canonical,
        })
    }

    /// The folder, in the form the library takes.
    pub fn path(&self) -> &Path {
        &self.plain
    }

    /// The file or folder `text` names under the project folder. It need not exist; its parent
    /// folders may be missing too, and `fs::create_dir_all` on the parent of the result makes
    /// them without leaving the folder, because every name below the deepest existing one is a
    /// plain name.
    pub fn resolve(&self, text: &str) -> Result<PathBuf, ToolError> {
        let bad = |why: &str| ToolError::path(format!("{text}: {why}"));
        if text.is_empty() || text.len() > MAX_PATH_TEXT || text.contains('\0') {
            return Err(bad("not a path of at most 1024 bytes"));
        }
        match form(text) {
            Form::Refused(why) => return Err(bad(&why.to_string())),
            Form::Local => return Err(bad("paths are relative to the project folder")),
            Form::Relative => {}
        }
        let mut candidate = self.canonical.clone();
        for name in text.split(['/', '\\']) {
            match name {
                "" | "." => {}
                ".." => return Err(bad("`..` is not allowed")),
                // Windows drops these, so the name would be another file.
                name if name.ends_with(['.', ' ']) => {
                    return Err(bad("a name ends in . or a space"));
                }
                name => candidate.push(name),
            }
        }
        // Links are followed on the part that exists; what lies below it is new.
        let mut existing = candidate.clone();
        let mut new = Vec::new();
        while fs::symlink_metadata(&existing).is_err() {
            let (Some(name), Some(parent)) = (
                existing.file_name().map(ToOwned::to_owned),
                existing.parent().map(Path::to_path_buf),
            ) else {
                return Err(bad("not a path under the project folder"));
            };
            new.push(name);
            existing = parent;
        }
        let mut real = fs::canonicalize(&existing)
            .map_err(|_| bad("a link on the path cannot be followed"))?;
        if !real.starts_with(&self.canonical) {
            return Err(bad("leaves the project folder"));
        }
        real.extend(new.iter().rev());
        Ok(plain(&real))
    }

    /// The path of something under the folder, relative to it and joined with `/`: the name a
    /// client uses for the file.
    pub fn relative(&self, path: &Path) -> String {
        let inside = path.strip_prefix(&self.plain).unwrap_or(path);
        let names: Vec<_> = inside
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect();
        names.join("/")
    }
}
