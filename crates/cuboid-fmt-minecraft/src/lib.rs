#![forbid(unsafe_code)]
//! Layer L4. Minecraft formats: Java JSON in and out, resource pack, display presets, `.mcpe`,
//! `.js`, `.java`, `.jpm` and the entity atlas.
//!
//! One module per format with its fuzz target and goldens. Must not share mutable state, call
//! UI or touch the GPU.
//!
//! The Java block and item model JSON ([`json_import`], [`json_export`], interop 7, 8, 31.2) and
//! the resource pack ([`resource_pack`], interop 9.1, 31.3, 31.4) are built. Coordinates, rotations
//! and face rectangles are mapped with [`cuboid_model::mc`]; the rules a model must meet for a game
//! version come from `cuboid-validate` and the version table of `cuboid-assets`.

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

pub mod bedrock_js;
pub mod entity_atlas;
pub mod geo;
pub mod java;
pub mod jem;
pub mod jpm;
pub mod json_export;
pub mod json_import;
mod jv;
pub mod mcpe;
pub mod modern;
pub mod presets;
pub mod resource_pack;

use std::fmt;

use cuboid_assets::mc_versions::mc_versions;
use cuboid_validate::Issue;
use thiserror::Error;

pub use cuboid_assets::mc_versions::{McVersion, PackFormat};
pub use cuboid_io::texture_files::{DiskFiles, Refusal, TextureFiles};
pub use json_export::{ExportFile, JsonExport, JsonOptions};
pub use json_import::{JsonImport, import, import_with};
pub use resource_pack::{PackExport, PackFile, PackOptions, export_pack};

/// Limits checked while reading, before anything is allocated for the data they bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of a model file; the text becomes a tree many times its size.
    pub max_text: u64,
    /// Separators (commas) of the whole text: the number of values that the tree holds.
    pub max_values: u64,
    /// Elements, including the ones an `item/generated` sprite is extruded into.
    pub max_elements: u32,
    /// Texture entries.
    pub max_textures: u32,
    /// Texels of all textures together.
    pub max_texels: u64,
    /// Item override cases.
    pub max_overrides: u32,
    /// Parent models followed, the model itself excluded (interop 31.2).
    pub max_parents: u32,
    /// Texels an `item/generated` sprite may have to be extruded pixel by pixel; a larger sprite
    /// becomes one flat slab.
    pub max_sprite_texels: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_text: 16 << 20,
            max_values: 1 << 22,
            max_elements: 1 << 16,
            max_textures: 1 << 12,
            max_texels: 1 << 26,
            max_overrides: 1 << 12,
            max_parents: 32,
            max_sprite_texels: 32 * 32,
        }
    }
}

/// Why a model is not imported or exported.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum McError {
    /// The text is not JSON, or its root is not an object.
    #[error("the file is not a valid JSON model")]
    Syntax,
    /// The root has none of `elements`, `textures` and `parent` (interop 8.2).
    #[error("the file is not a Minecraft model: it has no elements, textures or parent")]
    NotAModel,
    /// The file is larger than a limit of [`Limits`] allows, named here.
    #[error("the file exceeds the limit on {0}")]
    Limit(&'static str),
    /// A texture entry whose value is neither a string nor a `sprite` object (interop 8.3).
    #[error("the texture `{0}` is not a texture path")]
    TextureValue(String),
    /// A face without a texture name (interop 8.5).
    #[error("face `{face}` of element `{element}` names no texture")]
    FaceTexture {
        /// The element's label.
        element: String,
        /// The face.
        face: &'static str,
    },
    /// The model is neither a block nor an item model (interop 7.2).
    #[error("the model is not a block or an item model; set its type first")]
    Kind,
    /// The model has nothing to write.
    #[error("the model has no element to write")]
    Empty,
    /// The game version is not in the version table.
    #[error("the game version {0} is not in the version table")]
    UnknownVersion(String),
    /// The model breaks the rules of the target version.
    #[error("the model does not fit {version}: {first} ({count} problems)")]
    DoesNotFit {
        /// The target.
        version: String,
        /// The first problem.
        first: String,
        /// How many problems.
        count: usize,
    },
    /// No pack format is known for the version, and none was given.
    #[error("no pack format is known for {0}; give one")]
    PackFormat(String),
    /// A name that would write outside the pack or is not a path.
    #[error("the path `{0}` is not a safe file name")]
    UnsafePath(String),
    /// Two files of the pack would have the same path.
    #[error("two files of the pack are named `{0}`")]
    PathClash(String),
    /// A texture cannot be encoded.
    #[error("cannot encode the texture `{name}`: {reason}")]
    Texture {
        /// The texture's label.
        name: String,
        /// Why.
        reason: String,
    },
    /// The archive cannot be written.
    #[error("cannot write the archive: {0}")]
    Archive(String),
}

/// Something the importer or the exporter left out or changed, for the caller to show.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// Keys, slots or cases of one kind that the model cannot hold.
    Skipped {
        /// What, as the file names it.
        what: String,
        /// How many were left out.
        count: usize,
    },
    /// The parent model is neither in the folder tree nor built in; only the model itself was read.
    ParentNotFound {
        /// The `parent` value.
        parent: String,
    },
    /// The parent chain is longer than the limit or loops; it was cut there.
    ParentChain {
        /// The `parent` value where it was cut.
        parent: String,
    },
    /// A parent file was found but is not a model; it was not used.
    ParentUnreadable {
        /// The `parent` value.
        parent: String,
    },
    /// The parent is an entity model (`builtin/entity`); it is not merged.
    ParentNotMerged {
        /// The `parent` value.
        parent: String,
    },
    /// An `item/generated` family model was built as a sprite in elements.
    Extruded {
        /// How many elements the sprite became.
        elements: usize,
    },
    /// The sprite is too large to extrude pixel by pixel; it became one flat slab.
    SpriteSlab,
    /// A texture has no readable picture (no file, no layer0, a bad file); a checkerboard stands
    /// in for it.
    TextureMissing {
        /// The texture key.
        name: String,
    },
    /// A texture file was not read for the reason given.
    TextureRefused {
        /// The texture key.
        name: String,
        /// The path that was refused.
        path: String,
        /// Why.
        reason: Refusal,
    },
    /// The textures hold more texels than the limit; the rest are checkerboards.
    TexelBudget,
    /// Textures without a Minecraft path were given one on export.
    UnsavedTextures {
        /// How many.
        count: usize,
    },
    /// Two textures would be written to one file of the pack.
    TextureClash {
        /// The path.
        path: String,
    },
    /// Item overrides were not written: from 1.21.4 an item definition replaces them, and the
    /// names of its predicates are not confirmed (interop 31.3).
    OverridesNotConverted {
        /// How many cases.
        count: usize,
    },
    /// A finding of the validator about the pack.
    Validation(Issue),
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::Skipped { what, count } => write!(f, "skipped {count} {what}"),
            Notice::ParentNotFound { parent } => {
                write!(
                    f,
                    "the parent model `{parent}` was not found; it is not merged"
                )
            }
            Notice::ParentChain { parent } => {
                write!(f, "the chain of parent models is cut at `{parent}`")
            }
            Notice::ParentUnreadable { parent } => {
                write!(f, "the parent model `{parent}` is not a readable model")
            }
            Notice::ParentNotMerged { parent } => {
                write!(f, "the entity model `{parent}` is not merged")
            }
            Notice::Extruded { elements } => {
                write!(f, "the sprite was extruded into {elements} elements")
            }
            Notice::SpriteSlab => write!(f, "the sprite is large; it became one flat slab"),
            Notice::TextureMissing { name } => write!(f, "the texture `{name}` has no picture"),
            Notice::TextureRefused { name, path, reason } => {
                write!(f, "the texture `{name}` was not read from {path}: {reason}")
            }
            Notice::TexelBudget => write!(f, "the textures are too large; some were not read"),
            Notice::UnsavedTextures { count } => {
                write!(
                    f,
                    "{count} textures had no Minecraft path and were given one"
                )
            }
            Notice::TextureClash { path } => {
                write!(
                    f,
                    "two different textures use the path `{path}`; the first is kept"
                )
            }
            Notice::OverridesNotConverted { count } => {
                write!(f, "{count} item overrides are not written for this version")
            }
            Notice::Validation(issue) => write!(f, "{}: {}", issue.code, issue.message),
        }
    }
}

/// The newest game version the version table has a pack format for: the default target of an
/// export.
pub fn latest_version() -> McVersion {
    mc_versions()
        .pack_formats
        .last()
        .map_or(McVersion::new(26, 3, 0), |entry| entry.from)
}
