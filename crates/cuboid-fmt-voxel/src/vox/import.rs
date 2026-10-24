//! The `.vox` reader (interop 19.1 with the scene graph of 31.8).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use cuboid_model::apply::{VoxelReplay, voxel_world_bytes};
use cuboid_model::{Categories, LoadedDoc, ModelBox, ModelProps, Snapshot};
use cuboid_voxel::I3;
use cuboid_voxel::mapping::cell_size;
use glam::Vec3;

use super::scene::{self, Cursor, IDENTITY, Node, Placed, length};
use super::{Limits, MISSING_COLOUR, Notice, VoxError, default_palette};

/// An imported `.vox` file.
#[derive(Debug)]
pub struct VoxImport {
    /// The sections, to apply with `ApplyMode::Open`.
    pub doc: LoadedDoc,
    /// What the importer left out or changed.
    pub notices: Vec<Notice>,
}

struct Model {
    size: [u32; 3],
    /// `(x, y, z, colour index)` records.
    voxels: Vec<[u8; 4]>,
}

struct Parsed {
    models: Vec<Model>,
    /// Colour `i + 1` of the file, as `0xAARRGGBB`; 0 for a transparent colour.
    palette: Option<Vec<u32>>,
    nodes: HashMap<i32, Node>,
    any_voxels: bool,
}

/// The placed models as one voxel world.
struct Built {
    res: u32,
    /// The extent in VOX axes (Z up).
    extent: [i64; 3],
    cells: Vec<(I3, u32)>,
    outside: usize,
}

/// Imports a `.vox` file with the default [`Limits`]; see [`import_with`].
pub fn import(bytes: &[u8], path: &Path) -> Result<VoxImport, VoxError> {
    import_with(bytes, path, &Limits::default())
}

/// Imports a `.vox` file. `path` gives the model name. With several models the scene graph places
/// them by their transforms when the result fits the voxel budget; otherwise the first model
/// is imported alone and a [`Notice`] says so.
///
/// spec: interop 19.1, 31.8
pub fn import_with(bytes: &[u8], path: &Path, limits: &Limits) -> Result<VoxImport, VoxError> {
    if bytes.len() as u64 > limits.max_file {
        return Err(VoxError::Limit("the file size"));
    }
    let file = parse(bytes, limits)?;
    let palette = file.palette.unwrap_or_else(default_palette);
    let count = file.models.len();
    let mut notices = Vec::new();

    let first = [Placed {
        model: 0,
        rot: IDENTITY,
        t: [0; 3],
    }];
    let placed = if count == 1 {
        None
    } else if file.nodes.is_empty() {
        notices.push(Notice::NoSceneGraph { models: count });
        None
    } else {
        let found = scene::instances(&file.nodes, count, limits).filter(|p| !p.is_empty());
        if found.is_none() {
            notices.push(Notice::SceneGraphBroken { models: count });
        }
        found
    };
    let built = match placed {
        Some(all) => build(&file.models, &palette, &all, limits).or_else(|| {
            notices.push(Notice::ModelsDoNotFit { models: count });
            build(&file.models, &palette, &first, limits)
        }),
        None => build(&file.models, &palette, &first, limits),
    };
    let built = built.ok_or(VoxError::Limit("voxel memory"))?;
    if built.outside > 0 {
        notices.push(Notice::VoxelsOutsideModel {
            count: built.outside,
        });
    }

    // The model box of interop 19.1: from the origin to the far corner of the model.
    let cell = cell_size(built.res);
    let [ex, ey, ez] = built.extent;
    let model_box = ModelBox {
        min: Vec3::new(0.0, 0.0, -(ey as f32) * cell),
        max: Vec3::new(ex as f32 * cell, ez as f32 * cell, 0.0),
    };
    let mut props = ModelProps::default();
    props.general.name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    props.general.model_box = Some(model_box);

    let sections = Snapshot {
        categories: Categories::MODEL_PROPERTIES | Categories::VOXELS,
        props: Some(Arc::new(props)),
        view: None,
        elements: None,
        textures: None,
        voxels: Some(None),
        meshes: None,
        display: None,
    };
    Ok(VoxImport {
        doc: LoadedDoc {
            sections,
            voxel_replay: Some(VoxelReplay {
                res: built.res,
                cells: built.cells,
            }),
            overrides: None,
            retained: None,
            keep_exact_rotations: false,
        },
        notices,
    })
}

fn truncated(id: [u8; 4]) -> VoxError {
    VoxError::Truncated(String::from_utf8_lossy(&id).into_owned())
}

/// Walks the chunks exactly by their sizes and keeps the ones that matter.
fn parse(bytes: &[u8], limits: &Limits) -> Result<Parsed, VoxError> {
    let mut file = Cursor::new(bytes);
    if file.id() != Some(*b"VOX ") || file.u32().is_none() {
        return Err(VoxError::NotVox);
    }
    // MAIN: its own content is skipped and the other chunks follow it as one flat list.
    let corrupt = || VoxError::Corrupted("the MAIN chunk");
    if file.id() != Some(*b"MAIN") {
        return Err(corrupt());
    }
    let content = file.u32().ok_or_else(corrupt)?;
    file.u32().ok_or_else(corrupt)?;
    file.take(length(content))
        .ok_or_else(|| truncated(*b"MAIN"))?;

    let mut parsed = Parsed {
        models: Vec::new(),
        palette: None,
        nodes: HashMap::new(),
        any_voxels: false,
    };
    let mut total = 0_u64;
    while file.remaining() > 0 {
        let header = (file.id(), file.u32(), file.u32());
        let (Some(id), Some(content), Some(children)) = header else {
            return Err(VoxError::Truncated("a chunk header".to_owned()));
        };
        let body = file.take(length(content)).ok_or_else(|| truncated(id))?;
        file.take(length(children)).ok_or_else(|| truncated(id))?;
        match &id {
            b"SIZE" => {
                let mut c = Cursor::new(body);
                let (Some(x), Some(y), Some(z)) = (c.u32(), c.u32(), c.u32()) else {
                    return Err(VoxError::Corrupted("the SIZE chunk"));
                };
                if [x, y, z].iter().any(|&d| d == 0 || d > limits.max_dim) {
                    return Err(VoxError::BadSize(x, y, z));
                }
                if parsed.models.len() >= length(limits.max_nodes) {
                    return Err(VoxError::Limit("models"));
                }
                parsed.models.push(Model {
                    size: [x, y, z],
                    voxels: Vec::new(),
                });
            }
            b"XYZI" => {
                let model = parsed.models.last_mut().ok_or(VoxError::SizeNotFound)?;
                let mut c = Cursor::new(body);
                let corrupt = || VoxError::Corrupted("the XYZI chunk");
                let count = c.u32().ok_or_else(corrupt)?;
                let data = length(count)
                    .checked_mul(4)
                    .and_then(|n| c.take(n))
                    .ok_or_else(corrupt)?;
                total += u64::from(count);
                if total > limits.max_voxels {
                    return Err(VoxError::Limit("voxel records"));
                }
                model.voxels = data
                    .chunks_exact(4)
                    .filter_map(|v| <[u8; 4]>::try_from(v).ok())
                    .collect();
                parsed.any_voxels = true;
            }
            b"RGBA" => {
                // Colour `j` of the chunk is VOX colour index `j + 1`; the 256th is never used.
                let colours = body.chunks_exact(4).take(255).map(|c| match *c {
                    [r, g, b, a] if a != 0 => u32::from_be_bytes([a, r, g, b]),
                    _ => 0,
                });
                parsed.palette = Some(colours.collect());
            }
            b"nTRN" | b"nGRP" | b"nSHP" => {
                let (node_id, node) =
                    scene::parse_node(&id, body).ok_or(VoxError::Corrupted("a scene node"))?;
                if parsed.nodes.len() >= length(limits.max_nodes) {
                    return Err(VoxError::Limit("scene nodes"));
                }
                parsed.nodes.insert(node_id, node);
            }
            _ => {}
        }
    }
    if parsed.models.is_empty() {
        return Err(VoxError::SizeNotFound);
    }
    if !parsed.any_voxels {
        return Err(VoxError::VoxelsNotFound);
    }
    Ok(parsed)
}

/// Places the models, shifts them to the origin and turns the voxels into grid cells. `None` when
/// the result is over a limit or does not fit the voxel budget.
fn build(models: &[Model], palette: &[u32], placed: &[Placed], limits: &Limits) -> Option<Built> {
    let mut voxels = 0_u64;
    let mut low = [i64::MAX; 3];
    let mut high = [i64::MIN; 3];
    for p in placed {
        let model = models.get(p.model)?;
        voxels += model.voxels.len() as u64;
        if voxels > limits.max_voxels {
            return None;
        }
        // The cells at the corners of the model span its box, whatever the rotation.
        let last = model.size.map(|s| i64::from(s) - 1);
        for corner in 0..8_u32 {
            let cell: [i64; 3] = std::array::from_fn(|axis| {
                let at = if (corner >> axis) & 1 == 1 {
                    last
                } else {
                    [0; 3]
                };
                at.get(axis).copied().unwrap_or(0)
            });
            let at = cell_of(p, model.size, cell);
            for axis in 0..3 {
                let (lo, hi) = (low.get_mut(axis)?, high.get_mut(axis)?);
                *lo = (*lo).min(*at.get(axis)?);
                *hi = (*hi).max(*at.get(axis)?);
            }
        }
    }
    let mut extent = [0_i64; 3];
    for (axis, e) in extent.iter_mut().enumerate() {
        *e = high.get(axis)? - low.get(axis)? + 1;
    }
    let size = u32::try_from(extent.into_iter().max()?).ok()?;
    // The grid is three blocks wide; the editor needs at least two voxels per block.
    let res = size.max(2);
    if voxel_world_bytes(res, 0)? > limits.voxel_budget {
        return None;
    }
    let ey = i32::try_from(*extent.get(1)?).ok()?;
    let c = i32::try_from(res).ok()?;

    let lookup = |index: u8| match index {
        0 => 0,
        i => palette
            .get(usize::from(i) - 1)
            .copied()
            .unwrap_or(MISSING_COLOUR),
    };
    let mut cells = Vec::new();
    let mut chunks = HashSet::new();
    let mut outside = 0;
    for p in placed {
        let model = models.get(p.model)?;
        for &[x, y, z, index] in &model.voxels {
            let value = lookup(index);
            if value == 0 {
                continue;
            }
            let inside = u32::from(x) < model.size[0]
                && u32::from(y) < model.size[1]
                && u32::from(z) < model.size[2];
            if !inside {
                outside += 1;
                continue;
            }
            let at = cell_of(p, model.size, [i64::from(x), i64::from(y), i64::from(z)]);
            let shifted = |axis: usize| i32::try_from(at.get(axis)? - low.get(axis)?).ok();
            // VOX (x, y, z) with Z up is grid (x, z, flipped y).
            let grid = I3::new(c + shifted(0)?, c + shifted(2)?, c + ey - 1 - shifted(1)?);
            chunks.insert(grid >> 4);
            cells.push((grid, value));
        }
    }
    if voxel_world_bytes(res, chunks.len())? > limits.voxel_budget {
        return None;
    }
    Some(Built {
        res,
        extent,
        cells,
        outside,
    })
}

/// The cell of the voxel `at` of a placed model: centred on the model, rotated, translated.
fn cell_of(p: &Placed, size: [u32; 3], at: [i64; 3]) -> [i64; 3] {
    let doubled: [i64; 3] = std::array::from_fn(|axis| {
        let s = size.get(axis).copied().unwrap_or(0);
        2 * at.get(axis).copied().unwrap_or(0) + 1 - i64::from(s)
    });
    let turned = scene::apply(&p.rot, doubled);
    std::array::from_fn(|axis| {
        turned.get(axis).copied().unwrap_or(0).div_euclid(2) + p.t.get(axis).copied().unwrap_or(0)
    })
}
