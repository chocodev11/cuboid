#![forbid(unsafe_code)]
//! Layer L4. Blockbench projects (`.bbmodel`): import into the model and export from it.
//!
//! A project is a JSON tree of elements, groups, textures and display settings. The importer
//! reads cubes and free-format meshes, bakes the group hierarchy into the elements (the model
//! has none), keeps the nearest group names, cut to a bounded length, as name prefixes and reads the textures from embedded PNG
//! data URLs or from files next to the project. Everything it cannot hold is skipped with a
//! [`Notice`]. The exporter writes a `java_block` project when every element obeys the Minecraft
//! rules and a `free` project otherwise; both are written as format version 4.10, which
//! Blockbench 4 and 5 open.
//!
//! The coordinates, rotations and face rectangles are mapped with [`cuboid_model::mc`], the rules
//! the Minecraft Java JSON uses (interop 2.1-2.3, 8.5). The layout of the file is that of the
//! editor that writes it; this crate has its own code for it.

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

pub mod data_url;
mod export;
mod import;
mod json;
mod mesh;
mod textures;

use std::fmt;

use cuboid_io::texture_files::Refusal;
use thiserror::Error;

pub use cuboid_io::texture_files::{DiskFiles, TextureFiles};
pub use export::{BbExport, export};
pub use import::{BbImport, import, import_with};

/// Limits checked while reading, before anything is allocated for the data they bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of the project text; the text becomes a tree many times its size.
    pub max_text: u64,
    /// Separators (commas) of the whole text: the number of values that the tree holds.
    pub max_values: u64,
    /// Elements of all kinds, and the meshes that a large mesh is split into.
    pub max_objects: u32,
    /// Texture entries.
    pub max_textures: u32,
    /// Texels of all textures together.
    pub max_texels: u64,
    /// Vertices and triangle indices of all meshes together.
    pub max_mesh_items: u32,
    /// Nesting depth of the groups.
    pub max_depth: u32,
    /// Bytes of element labels (group names and own names, already shortened) of the whole
    /// project.
    pub max_label_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_text: 256 << 20,
            max_values: 1 << 23,
            max_objects: 1 << 18,
            max_textures: 1 << 12,
            max_texels: 1 << 26,
            max_mesh_items: 1 << 24,
            max_depth: 64,
            max_label_bytes: 64 << 20,
        }
    }
}

/// Why a project is not imported or exported.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BbError {
    /// The text is not JSON.
    #[error("the file is not valid JSON ({0})")]
    Syntax(String),
    /// The JSON has no `meta` object: it is no Blockbench project.
    #[error("the file is not a Blockbench project")]
    NotAProject,
    /// A compressed backup (`<lz>`), which Blockbench only writes for its own recovery.
    #[error("the file is a compressed Blockbench backup; open it in Blockbench and save it again")]
    Compressed,
    /// The file is larger than a limit of [`Limits`] allows, named here.
    #[error("the file exceeds the limit on {0}")]
    Limit(&'static str),
    /// The model cannot be written.
    #[error("cannot write the project: {0}")]
    Write(String),
}

/// Something the importer or the exporter left out or changed, for the caller to show.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// The project is of a newer format version than this crate knows. It is read as well as
    /// possible.
    NewerVersion(String),
    /// Elements, animations or other data of one kind that the model cannot hold.
    Skipped {
        /// The kind, as the project names it.
        what: String,
        /// How many were left out.
        count: usize,
    },
    /// A texture has no readable picture (no data, a bad data URL, a missing file or a picture
    /// that does not decode); a checkerboard stands in for it.
    TextureMissing {
        /// The name of the texture.
        name: String,
    },
    /// A texture file was not read for the reason given.
    TextureRefused {
        /// The name of the texture.
        name: String,
        /// The path that was refused.
        path: String,
        /// Why.
        reason: Refusal,
    },
    /// The textures hold more texels than the limit; the rest are checkerboards.
    TexelBudget,
    /// Element names and group names that were too long are cut with an ellipsis.
    LabelsShortened {
        /// How many elements got a shortened label.
        count: usize,
    },
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::NewerVersion(version) => {
                write!(f, "the project is of the newer format version {version}")
            }
            Notice::Skipped { what, count } => write!(f, "skipped {count} {what}"),
            Notice::TextureMissing { name } => write!(f, "the texture `{name}` has no picture"),
            Notice::TextureRefused { name, path, reason } => {
                write!(f, "the texture `{name}` was not read from {path}: {reason}")
            }
            Notice::TexelBudget => write!(f, "the textures are too large; some were not read"),
            Notice::LabelsShortened { count } => {
                write!(f, "shortened the long names of {count} elements")
            }
        }
    }
}
