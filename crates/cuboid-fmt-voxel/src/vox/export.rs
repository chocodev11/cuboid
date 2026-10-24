//! The `.vox` writer (interop 19.2 with the modern fixes: mirror around the region, sizes up to
//! 256, at most 255 colours).

use std::collections::{HashMap, HashSet};

use cuboid_voxel::{I3, VoxelWorld};

use super::{MISSING_COLOUR, Notice, VoxError};

/// The most voxels per axis that MagicaVoxel reads.
const MAX_SIZE: i32 = 256;
/// The colours of a palette; index 0 is empty.
const MAX_COLOURS: usize = 255;
/// The format version the writer states.
const VERSION: u32 = 150;

/// How to export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportOptions {
    /// With more than 255 colours, map the rarer ones to the nearest of the 255 most used instead
    /// of refusing.
    pub quantize: bool,
}

/// A written `.vox` file.
#[derive(Debug)]
pub struct VoxExport {
    /// The file.
    pub bytes: Vec<u8>,
    /// What the exporter changed.
    pub notices: Vec<Notice>,
}

/// The voxels of the write region as VOX `(x, y, z)` (Z up) and colour, with `x` outermost, then
/// `z`, then `y`. Grid `(gx, gy, gz)` is VOX `(gx - min.x, max.z - 1 - gz, gy - min.y)`: the
/// mirror is taken around the region, not around the world.
fn voxels(world: &VoxelWorld, dims: [i32; 3]) -> impl Iterator<Item = ([u8; 3], u32)> + '_ {
    let min = world.active_box.min;
    let top = world.active_box.max.z - 1;
    let [dx, dy, dz] = dims;
    (0..dx).flat_map(move |x| {
        (0..dy).flat_map(move |z| {
            (0..dz).filter_map(move |y| {
                let value = world.read(I3::new(min.x + x, min.y + z, top - y));
                if value >> 24 == 0 {
                    return None;
                }
                let at = [
                    u8::try_from(x).ok()?,
                    u8::try_from(y).ok()?,
                    u8::try_from(z).ok()?,
                ];
                Some((at, value))
            })
        })
    })
}

/// Squared distance of two colours over the four channels.
fn distance(a: u32, b: u32) -> u32 {
    a.to_be_bytes()
        .into_iter()
        .zip(b.to_be_bytes())
        .map(|(p, q)| u32::from(p.abs_diff(q)).pow(2))
        .sum()
}

/// The colours written, the index of every colour of the world and the number of voxels.
struct Palette {
    colours: Vec<u32>,
    index: HashMap<u32, u8>,
    voxels: usize,
}

/// The palette: colours in the order of the world's palette, then the rest in scan order, with
/// each colour's index (1-based). Over 255 colours, either an error or the nearest of the 255
/// most used.
fn palette(
    world: &VoxelWorld,
    dims: [i32; 3],
    options: &ExportOptions,
    notices: &mut Vec<Notice>,
) -> Result<Palette, VoxError> {
    let mut uses: HashMap<u32, usize> = HashMap::new();
    let mut scan_order = Vec::new();
    for (_, value) in voxels(world, dims) {
        let count = uses.entry(value).or_insert(0);
        if *count == 0 {
            scan_order.push(value);
        }
        *count += 1;
    }
    if uses.is_empty() {
        return Err(VoxError::NoVoxels);
    }
    let mut seen = HashSet::new();
    let ordered: Vec<u32> = world
        .palette
        .values()
        .iter()
        .copied()
        .chain(scan_order)
        .filter(|v| uses.contains_key(v) && seen.insert(*v))
        .collect();

    let mut kept = ordered.clone();
    if ordered.len() > MAX_COLOURS {
        if !options.quantize {
            return Err(VoxError::TooManyColours(ordered.len()));
        }
        // The most used colours stay, ties go to the earlier one; the palette keeps its order.
        let mut by_use: Vec<(usize, u32)> = ordered.iter().copied().enumerate().collect();
        by_use.sort_by_key(|&(at, v)| (std::cmp::Reverse(uses.get(&v).copied().unwrap_or(0)), at));
        by_use.truncate(MAX_COLOURS);
        by_use.sort_by_key(|&(at, _)| at);
        kept = by_use.into_iter().map(|(_, v)| v).collect();
        notices.push(Notice::Quantized {
            from: ordered.len(),
            to: kept.len(),
        });
    }

    let mut index: HashMap<u32, u8> = HashMap::new();
    for (at, &v) in kept.iter().enumerate() {
        index.insert(
            v,
            u8::try_from(at + 1).map_err(|_| VoxError::TooManyColours(at))?,
        );
    }
    for &v in &ordered {
        if index.contains_key(&v) {
            continue;
        }
        let nearest = kept
            .iter()
            .enumerate()
            .min_by_key(|&(at, &k)| (distance(v, k), at))
            .and_then(|(at, _)| u8::try_from(at + 1).ok())
            .ok_or(VoxError::NoVoxels)?;
        index.insert(v, nearest);
    }
    Ok(Palette {
        colours: kept,
        index,
        voxels: uses.values().sum(),
    })
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], content: u32, children: u32) {
    out.extend_from_slice(id);
    out.extend_from_slice(&content.to_le_bytes());
    out.extend_from_slice(&children.to_le_bytes());
}

/// Writes the write region of the world (its active box, the model box) as one `.vox` model. The
/// output is the same for the same world.
///
/// spec: interop 19.2
pub fn export(world: &VoxelWorld, options: &ExportOptions) -> Result<VoxExport, VoxError> {
    let (min, max) = (world.active_box.min, world.active_box.max);
    let size = max - min;
    let [dx, dy, dz] = size.to_array();
    if dx < 1 || dy < 1 || dz < 1 {
        return Err(VoxError::NoVoxels);
    }
    if [dx, dy, dz].iter().any(|&d| d > MAX_SIZE) {
        let over = |d: i32| u32::try_from(d).unwrap_or(0);
        return Err(VoxError::TooLarge(over(dx), over(dz), over(dy)));
    }
    let dims = [dx, dy, dz];
    let mut notices = Vec::new();
    let Palette {
        colours,
        index,
        voxels: count,
    } = palette(world, dims, options, &mut notices)?;

    let too_big = || VoxError::Limit("the file size");
    let xyzi = u32::try_from(count)
        .ok()
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(4))
        .ok_or_else(too_big)?;
    let (size_len, rgba_len) = (12_u32, 1024_u32);
    let children = 3 * 12 + size_len + rgba_len + xyzi;
    let total = children.checked_add(20).ok_or_else(too_big)?;

    let mut out = Vec::with_capacity(usize::try_from(total).map_err(|_| too_big())?);
    out.extend_from_slice(b"VOX ");
    out.extend_from_slice(&VERSION.to_le_bytes());
    chunk(&mut out, b"MAIN", 0, children);

    chunk(&mut out, b"SIZE", size_len, 0);
    // VOX has Z up: the sizes are the grid's x, z and y.
    for d in [dx, dz, dy] {
        out.extend_from_slice(&u32::try_from(d).unwrap_or(0).to_le_bytes());
    }

    chunk(&mut out, b"RGBA", rgba_len, 0);
    for slot in 0..256_usize {
        let value = colours.get(slot).copied().unwrap_or(MISSING_COLOUR);
        let [a, r, g, b] = value.to_be_bytes();
        out.extend_from_slice(&[r, g, b, a]);
    }

    chunk(&mut out, b"XYZI", xyzi, 0);
    out.extend_from_slice(&u32::try_from(count).unwrap_or(0).to_le_bytes());
    for ([x, y, z], value) in voxels(world, dims) {
        let at = index.get(&value).copied().unwrap_or(0);
        out.extend_from_slice(&[x, y, z, at]);
    }
    Ok(VoxExport {
        bytes: out,
        notices,
    })
}
