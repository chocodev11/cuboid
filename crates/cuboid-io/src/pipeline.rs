//! The texts of the import and export pipelines: validation errors, the loading box, status
//! lines, writer-helper messages and the voxel-only and element-only prompts.
//!
//! The texts are the original's, typos included. `{App}` stands for the product name; the
//! window code substitutes it where it shows a message (architecture 14.7). The flows own the
//! sequence of steps; this module only builds what they show.

use std::path::Path;

use cuboid_model::{MessageKind, UserMessage};

use crate::contract::{FormatEntry, ImportMode, Lookup, LookupMode, OutKind};
use crate::names::extension;
use crate::registry::Registry;

/// Files above this size show the loading box.
///
/// spec: interop 5.2 (step 3), native 6.1
pub const LOADING_BOX_MIN_BYTES: u64 = 307_200;

/// The first word of a pipeline status line.
///
/// spec: interop 5.2 (step 5), interop 6.2 (steps 2, 7)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Phase {
    /// An import finished.
    Imported,
    /// A native file finished loading.
    Loaded,
    /// An export started; the reference wording, replaced later.
    Importing,
    /// A save started.
    Loading,
    /// An export or save finished, also after write failures.
    Exported,
}

/// The kind of object an export says is missing in its element-only pre-check.
///
/// spec: interop 6.5
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NoElements {
    /// The voxel world is initialized.
    Voxels,
    /// At least one mesh exists.
    Meshes,
    /// Nothing at all.
    Empty,
}

fn message(
    kind: MessageKind,
    title: impl Into<String>,
    text: impl Into<String>,
    buttons: &[&str],
) -> UserMessage {
    UserMessage {
        kind,
        title: title.into(),
        text: text.into(),
        buttons: buttons.iter().map(|label| (*label).to_owned()).collect(),
    }
}

fn error(title: impl Into<String>, text: impl Into<String>) -> UserMessage {
    message(MessageKind::Error, title, text, &[])
}

/// Looks the path up and checks that the entry can do what the pipeline needs: it has an
/// importer in open mode and an exporter in save mode.
///
/// spec: interop 5.2 (step 1), interop 6.2 (step 1)
pub fn validate<'a>(
    registry: &'a Registry,
    path: &Path,
    mode: LookupMode,
    im: ImportMode,
) -> Result<&'a FormatEntry, UserMessage> {
    match registry.lookup(path, mode, im) {
        Lookup::NoExtension => Err(error(
            "Missing extension",
            "The selected file doesn't have any extension.

Operation aborted...",
        )),
        Lookup::Unknown(ext) => Err(error(
            format!("Unknown file type \"{ext}\""),
            "The selected file can't be opened in {App}.

Operation aborted...",
        )),
        Lookup::Entry(entry) => {
            let usable = match mode {
                LookupMode::Open => entry.can_read(),
                LookupMode::Save => entry.can_write(),
            };
            if usable {
                return Ok(entry);
            }
            let ext = extension(path);
            Err(match mode {
                LookupMode::Open => error(
                    format!("Can't read \"{ext}\""),
                    format!(
                        "{{App}} doesn't know how to read \"{ext}\" files yet.

However it knows how to write them!"
                    ),
                ),
                LookupMode::Save => error(
                    format!("Can't generate \"{ext}\""),
                    format!(
                        "{{App}} doesn't know how to write \"{ext}\" files yet.

However it knows how to read them!"
                    ),
                ),
            })
        }
    }
}

/// The byte count the loading box shows: divided by 1024 while it is at least 1024, up to GB,
/// printed without decimals and without a space before the unit.
///
/// spec: interop 5.2 (step 3)
pub fn size_text(bytes: u64) -> String {
    let mut size = bytes;
    let mut unit = "B";
    for next in ["KB", "MB", "GB"] {
        if size < 1024 {
            break;
        }
        size /= 1024;
        unit = next;
    }
    format!("{size}{unit}")
}

/// The wait box of a large file, or `None` when the file is small enough to load silently.
///
/// spec: interop 5.2 (step 3)
pub fn loading_box(file_name: &str, bytes: u64) -> Option<UserMessage> {
    if bytes <= LOADING_BOX_MIN_BYTES {
        return None;
    }
    Some(message(
        MessageKind::Wait,
        "Loading...",
        format!(
            "{{App}} is loading \"{file_name}\" ({})\n\nPlease wait...",
            size_text(bytes)
        ),
        &[],
    ))
}

/// A status line `<phase> <EXT>: <path>`; `EXT` is the extension of the path without its dot,
/// upper-cased.
///
/// spec: interop 5.2 (step 5), interop 6.2 (steps 2, 7)
pub fn status(phase: Phase, path: &Path) -> String {
    let word = match phase {
        Phase::Imported => "Imported",
        Phase::Loaded => "Loaded",
        Phase::Importing => "Importing",
        Phase::Loading => "Loading",
        Phase::Exported => "Exported",
    };
    let ext = extension(path).trim_start_matches('.').to_uppercase();
    format!("{word} {ext}: {}", path.display())
}

/// The status after the writer helper wrote a file or failed to. A PNG is a texture, every
/// other kind a file.
///
/// spec: interop 6.4
pub fn write_status(kind: OutKind, ok: bool, path: &Path) -> String {
    let what = if kind == OutKind::Png {
        "texture"
    } else {
        "file"
    };
    let outcome = if ok {
        "Successfuly saved"
    } else {
        "Failed saving"
    };
    format!("{outcome} {what}: {}", path.display())
}

/// The error box of a failed write; the export goes on and still reports `Exported`.
///
/// spec: interop 6.4
pub fn write_failed(kind: OutKind, file_name: &str, detail: &str) -> UserMessage {
    // The text writer names no noun.
    let noun = match kind {
        OutKind::Binary => "file ",
        OutKind::Text => "",
        OutKind::Png => "texture ",
    };
    error(
        "Error while saving file",
        format!("An error occured while saving {noun}\"{file_name}\".\n\n{detail}"),
    )
}

/// The error box of a failed import; `name` is the import name of the format.
///
/// spec: interop 5.4
pub fn import_failed(name: &str, detail: &str) -> UserMessage {
    message(
        MessageKind::Error,
        format!("{name} Import"),
        format!("An error occured while importing {name}:\n{detail}"),
        &["Abort"],
    )
}

/// The error box of an unexpected failure while an export writes; `name` is the export name.
///
/// spec: interop 6.2 (step 6)
pub fn export_failed(name: &str, detail: &str) -> UserMessage {
    message(
        MessageKind::Error,
        "Error while saving",
        format!("An error occured while saving {name} export:\n{detail}"),
        &["Abort"],
    )
}

/// The information box the voxel-only exporters show first when the model has elements.
/// `Cancel` changes only the status ([`cancelled_status`]); the export goes on in `cubik29`.
///
/// spec: interop 6.3
pub fn voxel_only_prompt(name: &str) -> UserMessage {
    message(
        MessageKind::Information,
        format!("{name} Export"),
        format!(
            "Exporting to {name} format only export voxels. Elements and Meshes are not exported.\n\nTo export elements or meshes, convert them first to voxels."
        ),
        &["Continue", "Cancel"],
    )
}

/// The status after `Cancel` in the voxel-only prompt; `key` is the registry key.
///
/// spec: interop 6.3
pub fn cancelled_status(key: &str) -> String {
    format!("Cancelled {} export...", key.to_uppercase())
}

/// The error box of the element-only pre-check of the `.js`, `.java` and `.jpm` exporters and,
/// with `json` set, of the Minecraft JSON exporter, which words the mesh case without the
/// article. `name` is the export name of the format.
///
/// spec: interop 6.5
pub fn no_elements(name: &str, what: NoElements, json: bool) -> UserMessage {
    let (title, lead, convert) = match what {
        NoElements::Voxels => (
            "Voxel model",
            "Your model does not contain any element but voxels.",
            "you must convert voxels to elements first.",
        ),
        NoElements::Meshes => (
            "Mesh model",
            "Your model does not contain any element but meshes.",
            if json {
                "you must convert meshes to elements first (via voxelization)."
            } else {
                "you must convert meshes to elements first (via a voxelization)."
            },
        ),
        NoElements::Empty => (
            "Empty model",
            "Your model is empty.",
            "you must create at least one element first.",
        ),
    };
    error(
        title,
        format!("{lead}\n\nTo export your model to {name} format, {convert}"),
    )
}

#[cfg(test)]
mod tests;
