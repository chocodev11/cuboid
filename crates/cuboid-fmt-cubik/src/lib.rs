#![forbid(unsafe_code)]
//! Layer L4. The `.cubik` container, records, schema, retention and extension entry, plus
//! legacy `.cub`.
//!
//! One module per format with its fuzz target and goldens. Must not share mutable state, call
//! UI or touch the GPU.
//!
//! | Module | Content |
//! |---|---|
//! | [`container`] | the ZIP entry `cubik` (native 2) |
//! | [`records`] | the record types of native 4, one to one |
//! | [`decode`], [`encode`] | BSON payload to records and back (native 3, 8) |
//! | [`load`], [`save`] | records to the model snapshot and back, with the read-side and save-side rules (native 5.3, 6.3) |
//! | [`retain`] | the retention of a loaded file for a byte-identical re-save (native 8.3) |
//! | [`cub`] | legacy `.cub` (native 9) |
//! | [`dump`] | the canonical text dump (native 10.2) |

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod container;
pub mod cub;
pub mod decode;
pub mod dump;
pub mod encode;
mod extension;
pub mod load;
pub mod records;
pub mod retain;
pub mod save;

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{self, BufReader, Cursor};
use std::path::Path;

use cuboid_model::{LoadedDoc, OverrideCase, Retained, Snapshot};
use retain::Kept;
use thiserror::Error;

pub use container::ContainerError;
pub use decode::{PayloadError, PayloadErrorKind};
pub use extension::ProjectMeta;
pub use save::{SaveOptions, TexturePolicy};

/// Limits checked while reading, before anything is allocated for the data they bound.
///
/// spec: native 8.1 item 4, arch 12.3
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of the decompressed payload.
    pub max_payload: u64,
    /// Bytes of a `.cub` file; the text becomes a tree many times its size.
    pub max_text: u64,
    /// Nesting depth of the BSON documents.
    pub max_depth: u8,
    /// Items of one BSON array.
    pub max_array: u32,
    /// Elements, meshes and sub-meshes of the whole file together (a record takes far more memory
    /// than its bytes). For a `.cub` file: the elements.
    pub max_objects: u32,
    /// Items of the mesh data arrays (vertices, normals, UVs and triangle indices of all meshes and
    /// sub-meshes) of the whole file together.
    pub max_mesh_items: u32,
    /// Texture records. For a `.cub` file: the texture entries.
    pub max_textures: u32,
    /// Texels of all textures together.
    pub max_texels: u64,
    /// Memory of the voxel world, slot table and chunks (native 4.7: any resolution within a
    /// memory limit).
    pub voxel_budget: u64,
    /// A loaded payload is kept for a byte-identical re-save up to this size; 0 switches the
    /// retention off.
    pub retain_cap: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_payload: 1 << 30,
            max_text: 16 << 20,
            max_depth: 64,
            max_array: 1 << 24,
            max_objects: 1 << 18,
            max_mesh_items: 1 << 24,
            max_textures: 1 << 16,
            max_texels: 1 << 28,
            voxel_budget: load::default_voxel_budget(),
            retain_cap: 512 << 20,
        }
    }
}

impl Limits {
    pub(crate) fn bson(&self) -> cuboid_bson::Limits {
        cuboid_bson::Limits {
            max_doc: u32::try_from(self.max_payload).unwrap_or(i32::MAX.cast_unsigned()),
            max_depth: self.max_depth,
            max_array: self.max_array,
        }
    }
}

/// Why a `.cubik` file cannot be read or written.
#[derive(Debug, Error)]
pub enum CubikError {
    /// The container: not a ZIP, no `cubik` entry, or a corrupt entry.
    #[error(transparent)]
    Container(#[from] ContainerError),
    /// The payload is malformed.
    #[error("invalid Cubik model: {0}")]
    Payload(#[from] PayloadError),
    /// A record that the model cannot hold.
    #[error("invalid Cubik model: {0}")]
    Invalid(String),
    /// The file system.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A file read into the sections of the model.
#[derive(Debug)]
pub struct Loaded {
    /// The sections, ready for `cuboid_model::apply_document` with `ApplyMode::Open` or `Merge`.
    pub doc: LoadedDoc,
    /// Records that were skipped or read in a tolerant way.
    pub warnings: Vec<String>,
    /// The preset stored in the `cuboid.json` entry; `None` for a file without one, which is
    /// every file Cubik wrote.
    pub project: Option<ProjectMeta>,
}

/// Reads a `.cubik` file from memory.
///
/// spec: native 2.2, 6, 8.1
pub fn load_bytes(bytes: &[u8], limits: &Limits) -> Result<Loaded, CubikError> {
    load_from(Cursor::new(bytes), limits)
}

/// Reads a `.cubik` file from disk.
pub fn load_path(path: &Path, limits: &Limits) -> Result<Loaded, CubikError> {
    load_from(BufReader::new(File::open(path)?), limits)
}

fn load_from<R: io::Read + io::Seek>(reader: R, limits: &Limits) -> Result<Loaded, CubikError> {
    let container = container::read(reader, limits)?;
    let mut loaded = load::load_payload(container.payload, limits)?;
    if let Some(extension) = container.extension {
        let why = match extension::read(&extension) {
            extension::Read::Known(read) => {
                if read.project_damaged {
                    loaded.warnings.push(
                        "the project settings in the `cuboid.json` entry are damaged and ignored"
                            .to_owned(),
                    );
                }
                loaded.doc.overrides = Some(read.overrides);
                loaded.project = read.project;
                return Ok(loaded);
            }
            extension::Read::Newer(version) => format!("is of the newer version {version}"),
            extension::Read::Damaged(err) => format!("cannot be read ({err})"),
        };
        loaded.warnings.push(format!(
            "the `cuboid.json` entry {why}: its settings are ignored here and kept unchanged \
             when the file is saved"
        ));
        loaded.doc.retained = Some(Kept::keep_entry(loaded.doc.retained.take(), extension));
    }
    Ok(loaded)
}

/// Writes a `.cubik` container for a snapshot of all categories. `retained` is the
/// `Document::retained` of the document the snapshot was taken from; unchanged records are
/// copied from it. `overrides` and `SaveOptions::project` go into the `cuboid.json` entry when
/// the options allow it and there is something to write.
///
/// spec: native 2.4, 8.2, 8.3, arch 12.3
pub fn save_bytes(
    snapshot: &Snapshot,
    retained: Option<&Retained>,
    overrides: &[OverrideCase],
    options: &SaveOptions,
) -> Result<Vec<u8>, CubikError> {
    let payload = save::save_payload(snapshot, retained, options)?;
    // An entry this build cannot read goes back as it was, whatever the overrides and the preset
    // say now.
    let extension = if !options.extension {
        None
    } else if let Some(kept) = retained.and_then(Kept::entry_of) {
        Some(kept.to_vec())
    } else {
        (!overrides.is_empty() || options.project.is_some())
            .then(|| extension::write(overrides, options.project.as_ref()))
    };
    Ok(container::write(&payload, extension.as_deref())?)
}

/// Writes a `.cubik` file: the archive is built completely in memory, then replaces the target
/// atomically (native 2.4).
pub fn save_path(
    path: &Path,
    snapshot: &Snapshot,
    retained: Option<&Retained>,
    overrides: &[OverrideCase],
    options: &SaveOptions,
) -> Result<(), CubikError> {
    let bytes = save_bytes(snapshot, retained, overrides, options)?;
    cuboid_io::write_atomic(path, &bytes)?;
    Ok(())
}
