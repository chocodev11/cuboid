//! Writes to the voxel world: setup, cells with brush and mirror, flood fill, colour replacement
//! and palette.

use cuboid_model::apply::{DEFAULT_VOXEL_BUDGET, voxel_world_bytes};
use cuboid_model::{DocFlags, Document, EditCtx, Rev};
use cuboid_num::pack::argb;
use cuboid_voxel::{ChunkSlot, I3, VoxelWorld, WriteKind, mapping};
use rustc_hash::FxHashSet;

use super::shape::{brush_span, mirror_images};
use super::{MAX_BRUSH, MAX_STROKE_CELLS, SETUP_RES, VoxelError};

/// The six face neighbours of a cell.
const NEIGHBOURS: [I3; 6] = [
    I3::new(1, 0, 0),
    I3::new(-1, 0, 0),
    I3::new(0, 1, 0),
    I3::new(0, -1, 0),
    I3::new(0, 0, 1),
    I3::new(0, 0, -1),
];

/// The world of the document.
pub fn world(doc: &Document) -> Result<&VoxelWorld, VoxelError> {
    doc.voxels.as_ref().ok_or(VoxelError::NoWorld)
}

/// The world of the document, for writing.
pub fn world_mut(doc: &mut Document) -> Result<&mut VoxelWorld, VoxelError> {
    doc.voxels.as_mut().ok_or(VoxelError::NoWorld)
}

/// Tells the scene that voxel chunks changed.
fn touched(doc: &mut Document) {
    doc.journal.mark_doc(DocFlags::VOXEL_CHUNKS);
    doc.revision = Rev::next();
}

/// Creates the voxel world at `res` voxels per block edge with the model box as its active box.
///
/// spec: tools 7.7 (the presets and the grid adaption belong to the caller)
pub fn setup(cx: &mut EditCtx<'_>, res: u32) -> Result<(), VoxelError> {
    if cx.doc.voxels.is_some() {
        return Err(VoxelError::AlreadySetUp);
    }
    if !SETUP_RES.contains(&res) {
        return Err(VoxelError::Resolution(res));
    }
    let budget = DEFAULT_VOXEL_BUDGET;
    let bytes = voxel_world_bytes(res, usize::MAX).ok_or(VoxelError::Resolution(res))?;
    if bytes > budget {
        return Err(VoxelError::Budget { res, bytes, budget });
    }
    let active = mapping::active_box(cx.doc.model_box, res);
    let mut world = VoxelWorld::new(res, cx.doc.props.mesh_export.ambient_occlusion, active);
    world.setup_corners = cx.doc.model_box;
    cx.doc.voxels = Some(world);
    touched(cx.doc);
    Ok(())
}

/// The brush of a write: a cube of `size` voxels around every cell and the mirror planes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Brush {
    /// Edge of the cube in voxels, 1 to [`MAX_BRUSH`].
    pub size: i32,
    /// Mirror across the planes through the middle of the active box, per axis.
    pub mirror: [bool; 3],
}

impl Brush {
    /// A single voxel, no mirror.
    pub const DOT: Brush = Brush {
        size: 1,
        mirror: [false; 3],
    };
}

/// What a write puts into the cells.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Put {
    /// The packed colour; 0 erases.
    pub value: u32,
    /// Leave empty cells alone: the right button recolours what exists.
    pub existing_only: bool,
}

/// Writes one cell if it lies in the active box and holds another value; logs the old value.
fn put_one(world: &mut VoxelWorld, cell: I3, put: Put, log: &mut Vec<(I3, u32)>) -> bool {
    if !world.active_box.contains(cell) {
        return false;
    }
    let old = world.read(cell);
    if old == put.value || (put.existing_only && old == 0) {
        return false;
    }
    world.write_full(cell, put.value);
    log.push((cell, old));
    true
}

/// Writes `put` into the brush stamp of every cell, mirrored. Cells that hold the value already
/// are skipped; every cell that changes is logged with its old value in `log`, so a preview can
/// be rolled back. Returns how many cells changed.
///
/// spec: tools 12.3
pub fn write_cells(
    cx: &mut EditCtx<'_>,
    cells: &[I3],
    brush: Brush,
    put: Put,
    log: &mut Vec<(I3, u32)>,
) -> Result<usize, VoxelError> {
    let world = world_mut(cx.doc)?;
    let size = brush.size.clamp(1, MAX_BRUSH);
    let edge = usize::try_from(size).unwrap_or(1);
    if cells.len().saturating_mul(edge * edge * edge) > MAX_STROKE_CELLS {
        return Err(VoxelError::TooLarge(MAX_STROKE_CELLS));
    }
    let active = world.active_box;
    let mut changed = 0;
    for &cell in cells {
        for z in brush_span(cell.z, size) {
            for y in brush_span(cell.y, size) {
                for x in brush_span(cell.x, size) {
                    for image in mirror_images(I3::new(x, y, z), brush.mirror, active) {
                        changed += usize::from(put_one(world, image, put, log));
                    }
                }
            }
        }
    }
    if changed > 0 {
        touched(cx.doc);
    }
    Ok(changed)
}

/// Puts the logged cells back to their old values, newest first, and empties the log: the
/// rubber band of a line or a box moves.
pub fn rollback(cx: &mut EditCtx<'_>, log: &mut Vec<(I3, u32)>) -> Result<(), VoxelError> {
    let world = world_mut(cx.doc)?;
    for (cell, old) in log.drain(..).rev() {
        world.write_full(cell, old);
    }
    touched(cx.doc);
    Ok(())
}

/// Flood fill: the cells connected to `seed` through shared faces that hold the seed's value
/// (inside the active box) take `value`, logged in `log`. A region of more than `limit` cells
/// changes nothing and gives [`VoxelError::TooLarge`]. Returns how many cells changed.
///
/// spec: tools 13.3 (Fill voxels)
pub fn fill_region(
    cx: &mut EditCtx<'_>,
    seed: I3,
    value: u32,
    limit: usize,
    log: &mut Vec<(I3, u32)>,
) -> Result<usize, VoxelError> {
    let world = world_mut(cx.doc)?;
    if !world.active_box.contains(seed) {
        return Ok(0);
    }
    let target = world.read(seed);
    if target == value {
        return Ok(0);
    }
    let mut seen = FxHashSet::from_iter([seed]);
    let mut region = vec![seed];
    let mut next = 0;
    while let Some(&cell) = region.get(next) {
        next += 1;
        for step in NEIGHBOURS {
            let neighbour = cell + step;
            if !world.active_box.contains(neighbour)
                || world.read(neighbour) != target
                || !seen.insert(neighbour)
            {
                continue;
            }
            if region.len() >= limit {
                return Err(VoxelError::TooLarge(limit));
            }
            region.push(neighbour);
        }
    }
    let put = Put {
        value,
        existing_only: false,
    };
    let changed = region
        .into_iter()
        .filter(|&cell| put_one(world, cell, put, log))
        .count();
    if changed > 0 {
        touched(cx.doc);
    }
    Ok(changed)
}

/// Gives every voxel of the colour `from` the colour `to` and renames its palette entry, in
/// the whole grid whatever the active box. Both colours must be opaque. Returns how many voxels
/// changed.
///
/// spec: tools 7.4 (global colour replacement), tools 13.3 (Replace voxels color)
pub fn replace_color(
    cx: &mut EditCtx<'_>,
    from: [u8; 4],
    to: [u8; 4],
) -> Result<usize, VoxelError> {
    let (from, to) = (argb(from), argb(to));
    if from == 0 || to == 0 {
        return Err(VoxelError::Transparent);
    }
    let world = world_mut(cx.doc)?;
    if from == to {
        return Ok(0);
    }
    let n = world.chunks_per_axis() as usize;
    let mut cells = Vec::new();
    for (at, slot) in world.slots().iter().enumerate() {
        let ChunkSlot::Data(chunk) = slot else {
            continue;
        };
        // The slot index is (cx * n + cy) * n + cz, the local index x + 16 y + 256 z.
        let base = I3::new(
            i32::try_from(at / (n * n)).unwrap_or(0),
            i32::try_from(at / n % n).unwrap_or(0),
            i32::try_from(at % n).unwrap_or(0),
        ) * 16;
        cells.extend(
            chunk
                .scan()
                .filter(|&(_, value)| value == from)
                .map(|(local, _)| {
                    let local = i32::try_from(local).unwrap_or(0);
                    base + I3::new(local & 15, (local >> 4) & 15, local >> 8)
                }),
        );
    }
    if cells.is_empty() {
        return Ok(0);
    }
    world.palette.rename(from, to);
    world.write_many(
        &mut cells.iter().map(|&cell| (cell, to)),
        WriteKind::Unchecked,
    );
    touched(cx.doc);
    Ok(cells.len())
}

/// Adds a colour to the palette of the world. Returns whether it was new.
///
/// The palette is derived from the voxels (it is rebuilt by undo, redo and load), so an entry
/// without a voxel lasts only until then.
///
/// spec: tools 7.4
pub fn palette_add(doc: &mut Document, color: [u8; 4]) -> Result<bool, VoxelError> {
    let value = argb(color);
    if value == 0 {
        return Err(VoxelError::Transparent);
    }
    let world = world_mut(doc)?;
    let fresh = world.palette.position(value).is_none();
    world.palette.insert(value);
    Ok(fresh)
}
