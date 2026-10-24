//! Windows shortcuts (`.lnk`): the target path of a subset of the MS-SHLLINK layout, and what
//! the built-in file browser does with it.
//!
//! Opening a `.lnk` imports no model, it navigates the browser. Network targets, Unicode local
//! paths and ID-list-only links are not handled, and non-ASCII paths are mis-decoded, as in the
//! reference.
//!
//! spec: interop 28

use std::path::{Path, PathBuf};

use cuboid_model::{MessageKind, UserMessage};

use crate::names::extension;

/// Where the LinkFlags are.
const FLAGS_AT: usize = 0x14;
/// Bit 0 of the flags: a link target ID list follows the header.
const HAS_ID_LIST: u32 = 1;
/// Where the size of the ID list is, at the end of the 76-byte header.
const ID_LIST_SIZE_AT: usize = 0x4C;

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let quad = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(<[u8; 4]>::try_from(quad).ok()?))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let pair = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes(<[u8; 2]>::try_from(pair).ok()?))
}

/// Resolves the target of a shortcut file. `path` is the shortcut's own path, whose extension
/// must be `.lnk` (any case), and `bytes` its content. Any problem gives `None`; a shortcut
/// without a local path gives an empty string.
///
/// The target is the local base path of the LinkInfo structure, read as UTF-8 text, with the
/// trailing part of a UNC-style suffix folded in and the characters that are invalid in paths,
/// and `#`, removed.
///
/// spec: interop 28 (step 1)
pub fn target(path: &Path, bytes: &[u8]) -> Option<String> {
    if !extension(path).eq_ignore_ascii_case(".lnk") {
        return None;
    }
    let flags = u32_at(bytes, FLAGS_AT)?;
    // Without an ID list the position stays behind the flags that were just read.
    let mut info_at = FLAGS_AT + 4;
    if flags & HAS_ID_LIST != 0 {
        let id_list_size = u16_at(bytes, ID_LIST_SIZE_AT)?;
        info_at = ID_LIST_SIZE_AT + 2 + usize::from(id_list_size);
    }
    let info_size = usize::try_from(u32_at(bytes, info_at)?).ok()?;
    // LinkInfoSize, then the header size, the flags and the volume ID offset (12 bytes), then
    // the offset of the local base path.
    let base_offset = usize::try_from(u32_at(bytes, info_at.checked_add(16)?)?).ok()?;
    // The text runs to two characters before the end of the LinkInfo structure.
    let count = info_size.checked_sub(base_offset)?.checked_sub(2)?;

    let start = info_at.checked_add(base_offset)?;
    let tail = bytes.get(start..)?;
    // A character takes at most four bytes.
    let tail = tail.get(..count.saturating_mul(4)).unwrap_or(tail);
    let text: String = String::from_utf8_lossy(tail).chars().take(count).collect();
    Some(clean(&fold_suffix(&text)))
}

/// A text with a double NUL keeps the part before it and appends what follows the next `\\`
/// and the NUL after that.
fn fold_suffix(text: &str) -> String {
    let Some(double_nul) = text.find("\0\0") else {
        return text.to_owned();
    };
    let head = text.get(..double_nul).unwrap_or_default();
    let tail = text
        .get(double_nul + 2..)
        .and_then(|after| after.split_once("\\\\"))
        .and_then(|(_, after)| after.split_once('\0'))
        .map_or("", |(_, rest)| rest);
    format!("{head}{tail}")
}

/// Removes the characters that are invalid in a Windows path, and `#`.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|&c| !(c < ' ' || matches!(c, '"' | '<' | '>' | '|' | '#')))
        .collect()
}

/// What the built-in browser does with a shortcut.
///
/// spec: interop 28 (step 2)
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Follow {
    /// Open the browser at this folder.
    Browse(PathBuf),
    /// Show this box; `Continue` reopens the default folder, `Cancel` closes the browser.
    NotFound(UserMessage),
}

/// Decides what to do with a resolved target: an existing folder is opened, an existing file
/// opens its parent folder, anything else is the `Path not found` box with the quoted path,
/// which may be empty. The checks read the file system, so the caller runs this off the UI
/// thread.
///
/// spec: interop 28 (step 2)
pub fn follow(target: Option<&str>) -> Follow {
    let target = target.unwrap_or_default();
    let path = Path::new(target);
    if path.is_dir() {
        return Follow::Browse(path.to_owned());
    }
    if path.is_file() {
        let folder = path.parent().unwrap_or(path);
        return Follow::Browse(folder.to_owned());
    }
    Follow::NotFound(UserMessage {
        kind: MessageKind::Error,
        title: "Path not found".to_owned(),
        text: format!("Sorry but shortcut can't be followed:\n\"{target}\""),
        buttons: vec!["Continue".to_owned(), "Cancel".to_owned()],
    })
}

#[cfg(test)]
mod tests;
