//! MagicaVoxel `.vox` import and export (spec: interop 19, 2.4, 2.5, 31.8).
//!
//! The file is a RIFF-like list of chunks (id, content size, children size, content, children).
//! The importer reads `SIZE`, `XYZI`, `RGBA` and the scene graph (`nTRN`, `nGRP`, `nSHP`), skips
//! every other chunk and merges the models into one voxel world when they fit. The voxels become
//! a [`cuboid_model::apply::VoxelReplay`] in file order, so the apply path writes them through
//! the full writer. The exporter writes one model of at most 256 voxels per axis and at most
//! 255 colours.

mod export;
mod import;
mod scene;
#[cfg(test)]
mod tests;

use std::fmt;

use cuboid_model::apply::DEFAULT_VOXEL_BUDGET;
use thiserror::Error;

pub use export::{ExportOptions, VoxExport, export};
pub use import::{VoxImport, import, import_with};

/// The VOX colour of an index that the palette does not hold (interop 2.5), as `0xAARRGGBB`.
pub(crate) const MISSING_COLOUR: u32 = 0xFFA0_D2FF;

/// Limits checked while reading, before anything is allocated for the data they bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of the file.
    pub max_file: u64,
    /// Voxels along one axis of one model.
    pub max_dim: u32,
    /// Voxel records of all models, and of all placed instances together.
    pub max_voxels: u64,
    /// Models and scene graph nodes.
    pub max_nodes: u32,
    /// Nodes the scene graph walk visits (instances share models, so the walk can fan out).
    pub max_visits: u32,
    /// Memory of the voxel world in bytes (`cuboid_model::apply::voxel_world_bytes`).
    pub voxel_budget: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file: 1 << 28,
            max_dim: 4096,
            max_voxels: 1 << 24,
            max_nodes: 1 << 16,
            max_visits: 1 << 16,
            voxel_budget: DEFAULT_VOXEL_BUDGET,
        }
    }
}

/// Why a file is not imported or a world is not exported.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VoxError {
    /// The file does not start with `VOX `.
    #[error("Invalid MagicaVoxel VOX format")]
    NotVox,
    /// The `MAIN` chunk is missing or a chunk is damaged.
    #[error("Corrupted MagicaVoxel VOX format ({0})")]
    Corrupted(&'static str),
    /// A chunk is longer than the rest of the file.
    #[error("the file ends inside the chunk `{0}`")]
    Truncated(String),
    /// No model size before the voxels.
    #[error("Size not found")]
    SizeNotFound,
    /// No voxel data.
    #[error("Voxels data not found")]
    VoxelsNotFound,
    /// A model size of zero, or above the limit.
    #[error("a model of {0} x {1} x {2} voxels is empty or too large")]
    BadSize(u32, u32, u32),
    /// The file is larger than a limit allows, named here.
    #[error("the file exceeds the limit on {0}")]
    Limit(&'static str),
    /// Export: the world has no voxel to write.
    #[error("the voxel world is empty, nothing to export")]
    NoVoxels,
    /// Export: the model box is larger than MagicaVoxel allows.
    #[error(
        "the model box is {0} x {1} x {2} voxels, MagicaVoxel allows 256 per axis; shrink the model box"
    )]
    TooLarge(u32, u32, u32),
    /// Export: more colours than a VOX palette holds.
    #[error("the model uses {0} colours, a VOX palette holds 255; reduce the colours first")]
    TooManyColours(usize),
}

/// Something the importer or the exporter left out or changed, for the caller to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The file has several models and no scene graph to place them; the first was imported.
    NoSceneGraph {
        /// Models in the file.
        models: usize,
    },
    /// The scene graph cannot be followed (too deep, too large or without a root); the first
    /// model was imported.
    SceneGraphBroken {
        /// Models in the file.
        models: usize,
    },
    /// The placed models do not fit one voxel world within the memory budget; the first model was
    /// imported.
    ModelsDoNotFit {
        /// Models in the file.
        models: usize,
    },
    /// Voxels outside the size of their model were left out.
    VoxelsOutsideModel {
        /// How many.
        count: usize,
    },
    /// Export: the colours were reduced to the nearest of the 255 most used.
    Quantized {
        /// Colours of the world.
        from: usize,
        /// Colours written.
        to: usize,
    },
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::NoSceneGraph { models } => write!(
                f,
                "the file holds {models} models without a scene graph; only the first was imported"
            ),
            Notice::SceneGraphBroken { models } => write!(
                f,
                "the scene graph cannot be followed; only the first of {models} models was imported"
            ),
            Notice::ModelsDoNotFit { models } => write!(
                f,
                "the {models} models do not fit one voxel world; only the first was imported"
            ),
            Notice::VoxelsOutsideModel { count } => {
                write!(f, "{count} voxels outside their model were left out")
            }
            Notice::Quantized { from, to } => {
                write!(f, "{from} colours were reduced to the nearest of {to}")
            }
        }
    }
}

/// The MagicaVoxel default palette (interop 34.4.1): 255 opaque colours as `0xAARRGGBB`, where
/// entry `k` is VOX colour index `k + 1`.
pub(crate) fn default_palette() -> Vec<u32> {
    const LEVELS: [u32; 6] = [0xFF, 0xCC, 0x99, 0x66, 0x33, 0x00];
    const RAMP: [u32; 10] = [0xEE, 0xDD, 0xBB, 0xAA, 0x88, 0x77, 0x55, 0x44, 0x22, 0x11];
    let pack = |r: u32, g: u32, b: u32| 0xFF00_0000 | (r << 16) | (g << 8) | b;
    let mut colours = Vec::with_capacity(255);
    for r in LEVELS {
        for g in LEVELS {
            for b in LEVELS {
                colours.push(pack(r, g, b));
            }
        }
    }
    // The last cube entry is black, which the table leaves out.
    colours.pop();
    colours.extend(RAMP.iter().map(|&d| pack(d, 0, 0)));
    colours.extend(RAMP.iter().map(|&d| pack(0, d, 0)));
    colours.extend(RAMP.iter().map(|&d| pack(0, 0, d)));
    colours.extend(RAMP.iter().map(|&d| pack(d, d, d)));
    colours
}
