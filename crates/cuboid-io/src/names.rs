//! The file name helpers every exporter shares: extension, stem, sibling files and the PNG
//! extension rule.
//!
//! They work on the text of the path with `/`, `\` and `:` as separators on every platform, as
//! the reference does on Windows, so a path typed in a dialog means the same everywhere.
//!
//! spec: interop 3.7, interop 4.3

use std::path::{Path, PathBuf};

/// The file name: the text after the last `/`, `\` or `:`.
fn file_name(text: &str) -> &str {
    let start = text.rfind(['/', '\\', ':']).map_or(0, |at| at + 1);
    text.get(start..).unwrap_or_default()
}

/// The extension of a path with its dot, or an empty string when there is none: the text from
/// the last `.` of the file name. A trailing dot gives no extension, and a name that starts
/// with a dot is all extension.
///
/// spec: interop 4.3
pub fn extension(path: &Path) -> String {
    let text = path.to_string_lossy();
    let name = file_name(&text);
    match name.rfind('.').and_then(|dot| name.get(dot..)) {
        Some(ext) if ext.len() > 1 => ext.to_owned(),
        _ => String::new(),
    }
}

/// The file name without its last extension.
///
/// spec: interop 3.7
pub fn stem(path: &Path) -> String {
    let text = path.to_string_lossy();
    let name = file_name(&text);
    let kept = name.len() - extension(path).len();
    name.get(..kept).unwrap_or_default().to_owned()
}

/// The path of a file in the folder of `path`: the folder with `/` separators, then `name`.
/// A bare target name has no folder and resolves against the current folder; the reference
/// would anchor it at the file system root.
///
/// spec: interop 3.7
pub fn sibling(path: &Path, name: &str) -> PathBuf {
    let text = path.to_string_lossy().replace('\\', "/");
    match text.rfind('/') {
        Some(at) => PathBuf::from(format!("{}{name}", text.get(..=at).unwrap_or_default())),
        None => PathBuf::from(name),
    }
}

/// The path a PNG is written to: `.png` is appended unless the path already ends with it in
/// any case.
///
/// spec: interop 3.7
pub fn png_path(path: &Path) -> PathBuf {
    let mut text = path.as_os_str().to_owned();
    if !path.to_string_lossy().to_lowercase().ends_with(".png") {
        text.push(".png");
    }
    PathBuf::from(text)
}

#[cfg(test)]
mod tests;
