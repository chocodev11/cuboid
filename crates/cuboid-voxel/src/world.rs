//! `VoxelWorld`: resolution, chunk slot table, active box, setup corners, dirty set and revision
//! (spec: tools 7.1-7.2; arch 8.4).

use std::sync::Arc;

use fixedbitset::FixedBitSet;
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::chunk::Chunk;
use crate::contract::{ChunkSlot, I3, IBox3, Rev, WBox3};
use crate::palette::Palette;

/// Cells per chunk edge, as the divisor of the chunk count.
const CHUNK_EDGE_U32: u32 = 16;

/// Cells per chunk edge as a signed coordinate step, and its binary logarithm.
const CHUNK_EDGE_I32: i32 = 16;
const CHUNK_SHIFT: i32 = 4;

/// The sparse voxel grid of a model: `3 * res` cells per axis in 16-cell chunks, allocated on
/// the first non-zero write and never by a read. Chunks are shared (`Arc`) between clones and
/// snapshots; a write copies a shared chunk first.
///
/// The slot table follows the persistence order of the file (`cx` outermost, then `cy`, then
/// `cz`), so a save walks memory linearly and parallel results merge in file order.
///
/// A slot is `Data` only while the chunk holds a non-zero cell. Erasing the last one turns it
/// into `EmptyWritten`: the file still lists the chunk (tools 7.10, the original's cleared
/// empty flag). A slot that never received a non-zero write stays `Unallocated`.
///
/// spec: tools 7.1-7.5, arch 6 (cuboid-voxel), arch 8.4
#[derive(Clone, Debug)]
pub struct VoxelWorld {
    /// Voxels per block edge; any `R >= 1` (native 4.7), the editor uses 2..=254.
    pub res: u32,
    pub(crate) n: u32,
    pub(crate) slots: Vec<ChunkSlot>,
    /// Values used so far, in first-use order.
    pub palette: Palette,
    /// Cells readable and writable by the checked writers; half-open and inside the grid. Change
    /// it with [`VoxelWorld::set_active_box`], which dirties the affected chunks.
    pub active_box: IBox3,
    /// The model box at setup time, which up- and downscale restore (QUIRK, tools 7.2).
    pub setup_corners: WBox3,
    /// Ambient occlusion in the chunk meshes (`obj.ao`).
    pub ao: bool,
    pub(crate) dirty: FixedBitSet,
    /// Changes with every write that passes its box check.
    pub rev: Rev,
}

/// The slot table of a world without the settings that a restore keeps from the live world (the
/// active box and the AO flag). Cheap to take: one pointer copy per chunk.
///
/// A restore turns it back into a world: `EmptyWritten` slots become `Unallocated`, cells
/// outside the active box are cleared and the palette is rebuilt in scan order (tools 7.10). That
/// equals the full-writer load of native 6.3 only for records in slot and scan order without
/// repeats, which is what an undo snapshot and a canonical file hold. A snapshot has no record
/// order, so a loader of other files must replay the records in file order through
/// `write_many(.., WriteKind::Full)` instead of building a snapshot.
///
/// spec: arch 9.1 (Voxels), arch 8.4, tools 7.10
#[derive(Clone, Debug)]
pub struct VoxelSnap {
    /// Voxels per block edge.
    pub res: u32,
    /// `n * n * n` slots in persistence order, `n = ceil(3 * res / 16)`.
    pub slots: Arc<[ChunkSlot]>,
}

/// The slot index of chunk `c` in a table of `n` chunks per axis.
fn slot_index(c: I3, n: i32) -> usize {
    ((c.x * n + c.y) * n + c.z) as usize
}

impl VoxelWorld {
    /// An empty world of resolution `res` with the given active box (the whole grid for a
    /// fresh world).
    ///
    /// spec: tools 7.1, tools 7.7
    pub fn new(res: u32, ao: bool, active: IBox3) -> Self {
        let n = (3 * res).div_ceil(CHUNK_EDGE_U32);
        let count = (n as usize).pow(3);
        Self {
            res,
            n,
            slots: vec![ChunkSlot::Unallocated; count],
            palette: Palette::default(),
            active_box: active,
            setup_corners: WBox3::EDITING_VOLUME,
            ao,
            dirty: FixedBitSet::with_capacity(count),
            rev: Rev::next(),
        }
    }

    /// Chunks per axis, `ceil(3 * res / 16)`.
    pub fn chunks_per_axis(&self) -> u32 {
        self.n
    }

    /// The chunk table in persistence order: slot `(cx * n + cy) * n + cz`.
    pub fn slots(&self) -> &[ChunkSlot] {
        &self.slots
    }

    /// Whether any chunk is dirty: the world-dirty flag that starts a rebuild pass.
    ///
    /// spec: tools 7.5, tools 7.6 (Scheduling)
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_clear()
    }

    /// Takes the set of dirty chunks (bit = slot index) and leaves an empty one.
    ///
    /// spec: tools 7.5, tools 7.6 (Scheduling)
    pub fn take_dirty(&mut self) -> FixedBitSet {
        let empty = FixedBitSet::with_capacity(self.slots.len());
        std::mem::replace(&mut self.dirty, empty)
    }

    /// The slot table as an undo or clipboard snapshot.
    pub fn snapshot(&self) -> VoxelSnap {
        VoxelSnap {
            res: self.res,
            slots: Arc::from(self.slots.as_slice()),
        }
    }

    /// Restores a snapshot into a fresh world with the given settings. The result equals a new
    /// world into which every voxel of the snapshot was written with the full writer, in file
    /// order:
    ///
    /// - `EmptyWritten` slots become `Unallocated`, so an erased chunk is not listed again;
    /// - cells outside `active` are dropped. Chunks inside the box are shared, only chunks on
    ///   its boundary are copied, and a chunk left without cells becomes `Unallocated`;
    /// - the palette holds the values with alpha in scan order (chunks in slot order, voxels
    ///   `x`, `y`, `z`), so values that were erased before the snapshot are gone;
    /// - every slot is dirty, so the whole world is meshed again, chunks that the restore emptied
    ///   included (meshing an unallocated slot is free). The setup corners are the editing volume;
    ///   the caller sets them.
    ///
    /// The equality with the file-order load of native 6.3 holds for records in slot and scan
    /// order without repeats; see [`VoxelSnap`].
    ///
    /// spec: arch 9.1 (Voxels), tools 7.10, native 6.3
    pub fn from_snapshot(snap: &VoxelSnap, ao: bool, active: IBox3) -> Self {
        let mut world = Self::new(snap.res, ao, active);
        let n = world.n as i32;
        let coords = (0..n)
            .flat_map(|cx| (0..n).flat_map(move |cy| (0..n).map(move |cz| I3::new(cx, cy, cz))));
        let kept = world.slots.iter_mut().zip(snap.slots.iter()).zip(coords);
        for ((dst, src), c) in kept {
            let ChunkSlot::Data(chunk) = src else {
                continue;
            };
            *dst = restore_chunk(chunk, c << CHUNK_SHIFT, active);
        }
        world.dirty.insert_range(..);

        // Distinct values per chunk in parallel, merged in slot order.
        let colours: Vec<Vec<u32>> = world
            .slots
            .par_iter()
            .map_init(FxHashSet::default, |seen, slot| {
                let ChunkSlot::Data(chunk) = slot else {
                    return Vec::new();
                };
                seen.clear();
                chunk
                    .scan()
                    .map(|(_, value)| value)
                    .filter(|&value| value >> 24 != 0 && seen.insert(value))
                    .collect()
            })
            .collect();
        for value in colours.into_iter().flatten() {
            world.palette.insert(value);
        }
        world
    }

    /// Replaces the active box, as when the model box changes. The cells that became readable
    /// or unreadable change what the mesher sees, and so do the cells next to them (neighbour
    /// reads, ambient occlusion), so every chunk within one voxel of the changed cells is dirty.
    /// The revision changes, so jobs started on the old box are dropped. An unchanged box does
    /// nothing.
    ///
    /// Port deviation: the original processes the first changed side of the box only
    /// (QUIRK, tools 7.2); the port remeshes every affected chunk (architecture 17.2).
    ///
    /// spec: tools 7.2, arch 8.4
    pub fn set_active_box(&mut self, active: IBox3) {
        let old = std::mem::replace(&mut self.active_box, active);
        if old == active {
            return;
        }
        for slab in box_difference(old, active)
            .into_iter()
            .chain(box_difference(active, old))
            .flatten()
        {
            self.mark_dirty_cells(slab.min, slab.max - I3::ONE);
        }
        self.rev = Rev::next();
    }

    /// The slot index and the local cell index of cell `i`; `None` outside the chunk grid.
    pub(crate) fn locate(&self, i: I3) -> Option<(usize, usize)> {
        let c = i >> CHUNK_SHIFT;
        let n = self.n as i32;
        if c.cmplt(I3::ZERO).any() || c.cmpge(I3::splat(n)).any() {
            return None;
        }
        let l = i & (CHUNK_EDGE_I32 - 1);
        let local = Chunk::local_index(l.x as usize, l.y as usize, l.z as usize);
        Some((slot_index(c, n), local))
    }

    /// The stored value of cell `i`, whatever the active box says; 0 outside the grid.
    pub(crate) fn cell_at(&self, i: I3) -> u32 {
        let Some((slot, local)) = self.locate(i) else {
            return 0;
        };
        match self.slots.get(slot) {
            Some(ChunkSlot::Data(chunk)) => chunk.cell(local),
            _ => 0,
        }
    }

    /// Marks the chunk of cell `i` and every chunk within one voxel of it as dirty: the meshes
    /// of all of them can change, diagonal neighbours included. `i` must be inside the grid.
    ///
    /// spec: tools 7.5 (port note)
    pub(crate) fn mark_dirty(&mut self, i: I3) {
        self.mark_dirty_cells(i, i);
    }

    /// Marks every chunk within one voxel of the cells `first..=last` as dirty, clipped to the
    /// chunk grid.
    fn mark_dirty_cells(&mut self, first: I3, last: I3) {
        let n = self.n as i32;
        let lo = ((first - I3::ONE) >> CHUNK_SHIFT).max(I3::ZERO);
        let hi = ((last + I3::ONE) >> CHUNK_SHIFT).min(I3::splat(n - 1));
        for cx in lo.x..=hi.x {
            for cy in lo.y..=hi.y {
                for cz in lo.z..=hi.z {
                    self.dirty.insert(slot_index(I3::new(cx, cy, cz), n));
                }
            }
        }
    }
}

/// The part of `a` outside `b` as up to six disjoint non-empty boxes: the x slabs of `a`, then
/// the y slabs of what is left inside the x range of `b`, then the z slabs.
fn box_difference(a: IBox3, b: IBox3) -> [Option<IBox3>; 6] {
    let both = IBox3 {
        min: a.min.max(b.min),
        max: a.max.min(b.max),
    };
    let non_empty = |s: IBox3| s.min.cmplt(s.max).all().then_some(s);
    if both.min.cmpge(both.max).any() {
        return [non_empty(a), None, None, None, None, None];
    }
    let slab = |axis: usize, lo: i32, hi: i32, within: IBox3| {
        let mut s = within;
        s.min[axis] = lo;
        s.max[axis] = hi;
        non_empty(s)
    };
    let x_part = IBox3 {
        min: I3::new(both.min.x, a.min.y, a.min.z),
        max: I3::new(both.max.x, a.max.y, a.max.z),
    };
    let y_part = IBox3 {
        min: I3::new(both.min.x, both.min.y, a.min.z),
        max: I3::new(both.max.x, both.max.y, a.max.z),
    };
    [
        slab(0, a.min.x, both.min.x, a),
        slab(0, both.max.x, a.max.x, a),
        slab(1, a.min.y, both.min.y, x_part),
        slab(1, both.max.y, a.max.y, x_part),
        slab(2, a.min.z, both.min.z, y_part),
        slab(2, both.max.z, a.max.z, y_part),
    ]
}

/// What a restore keeps of `chunk`, whose first cell is `lo`: the shared chunk when it lies
/// inside the active box, a copy without the cells outside the box on its boundary, and nothing
/// when no cell survives.
fn restore_chunk(chunk: &Arc<Chunk>, lo: I3, active: IBox3) -> ChunkSlot {
    let hi = lo + I3::splat(CHUNK_EDGE_I32);
    let kept = if lo.cmpge(active.min).all() && hi.cmple(active.max).all() {
        Arc::clone(chunk)
    } else if lo.cmpge(active.max).any() || hi.cmple(active.min).any() {
        return ChunkSlot::Unallocated;
    } else {
        // The part of the active box inside the chunk, in local coordinates.
        let edge = I3::splat(CHUNK_EDGE_I32);
        let keep_min = (active.min - lo).clamp(I3::ZERO, edge);
        let keep_max = (active.max - lo).clamp(I3::ZERO, edge);
        Arc::new(clipped(chunk, keep_min, keep_max))
    };
    if kept.nonzero() == 0 {
        return ChunkSlot::Unallocated;
    }
    ChunkSlot::Data(kept)
}

/// A copy of `chunk` with every cell outside the local box `[min, max)` cleared.
fn clipped(chunk: &Chunk, min: I3, max: I3) -> Chunk {
    let mut copy = Chunk::default();
    for z in min.z..max.z {
        for y in min.y..max.y {
            for x in min.x..max.x {
                let local = Chunk::local_index(x as usize, y as usize, z as usize);
                copy.set(local, chunk.cell(local));
            }
        }
    }
    copy
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use proptest::prelude::*;

    use super::*;
    use crate::mapping::grid_box;

    const RED: u32 = 0xFFFF_0000;
    const GREEN: u32 = 0xFF00_FF00;
    const BLUE: u32 = 0xFF00_00FF;

    fn world(res: u32) -> VoxelWorld {
        VoxelWorld::new(res, true, grid_box(res))
    }

    fn data(slot: &ChunkSlot) -> &Arc<Chunk> {
        match slot {
            ChunkSlot::Data(chunk) => chunk,
            other => panic!("expected a data slot, found {other:?}"),
        }
    }

    #[test]
    fn the_chunk_table_has_ceil_3r_over_16_chunks_per_axis() {
        for (res, per_axis) in [(2, 1), (16, 3), (24, 5), (128, 24), (254, 48)] {
            let world = world(res);
            assert_eq!(world.chunks_per_axis(), per_axis, "R = {res}");
            assert_eq!(world.slots.len(), (per_axis as usize).pow(3), "R = {res}");
            assert_eq!(world.snapshot().slots.len(), world.slots.len());
        }
    }

    #[test]
    fn taking_the_dirty_set_leaves_an_empty_one_of_the_same_size() {
        let mut world = world(16);
        world.dirty.insert(5);
        assert!(world.is_dirty());
        let taken = world.take_dirty();
        assert!(taken.contains(5));
        assert_eq!(world.dirty.count_ones(..), 0);
        assert_eq!(world.dirty.len(), taken.len());
        assert!(!world.is_dirty());
    }

    #[test]
    fn a_new_world_has_no_chunk_and_the_palette_starts_with_zero() {
        let world = world(16);
        assert!(
            world
                .slots
                .iter()
                .all(|s| matches!(s, ChunkSlot::Unallocated))
        );
        assert_eq!(world.palette.values(), [0]);
    }

    #[test]
    fn locate_maps_cells_to_slots_in_persistence_order() {
        let world = world(16);
        // n = 3: slot (cx * 3 + cy) * 3 + cz.
        assert_eq!(world.locate(I3::new(0, 0, 0)), Some((0, 0)));
        assert_eq!(
            world.locate(I3::new(1, 2, 3)),
            Some((0, 1 + 16 * 2 + 256 * 3))
        );
        assert_eq!(world.locate(I3::new(16, 0, 0)), Some((9, 0)));
        assert_eq!(world.locate(I3::new(0, 16, 0)), Some((3, 0)));
        assert_eq!(world.locate(I3::new(0, 0, 16)), Some((1, 0)));
        assert_eq!(world.locate(I3::new(47, 47, 47)), Some((26, 4095)));
        // No axis wraps into the next row of the table.
        for outside in [
            I3::new(-1, 0, 0),
            I3::new(0, -1, 0),
            I3::new(0, 0, -1),
            I3::new(48, 0, 0),
            I3::new(0, 48, 0),
            I3::new(0, 0, 48),
        ] {
            assert_eq!(world.locate(outside), None, "{outside}");
        }
    }

    #[test]
    fn a_snapshot_is_untouched_by_later_writes() {
        let mut world = world(16);
        world.write_full(I3::new(3, 4, 5), RED);
        let snap = world.snapshot();
        world.write_full(I3::new(3, 4, 5), GREEN);
        world.write_full(I3::new(20, 0, 0), BLUE);

        assert_eq!(world.read(I3::new(3, 4, 5)), GREEN);
        let kept = data(&snap.slots[0]);
        assert_eq!(kept.cell(Chunk::local_index(3, 4, 5)), RED);
        assert!(matches!(snap.slots[9], ChunkSlot::Unallocated));
    }

    #[test]
    fn a_restore_forgets_the_erased_chunk_flag() {
        let mut world = world(16);
        world.write_full(I3::new(1, 1, 1), RED);
        world.write_full(I3::new(1, 1, 1), 0);
        world.write_full(I3::new(40, 40, 40), GREEN);
        assert!(matches!(world.slots[0], ChunkSlot::EmptyWritten));
        let snap = world.snapshot();
        assert!(
            matches!(snap.slots[0], ChunkSlot::EmptyWritten),
            "a save lists the erased chunk"
        );

        let restored = VoxelWorld::from_snapshot(&snap, true, grid_box(16));
        assert!(matches!(restored.slots[0], ChunkSlot::Unallocated));
        assert!(matches!(restored.slots[26], ChunkSlot::Data(_)));
        // The palette is rebuilt from the cells: the erased value is gone.
        assert_eq!(world.palette.values(), [0, RED, GREEN]);
        assert_eq!(restored.palette.values(), [0, GREEN]);
    }

    #[test]
    fn a_restore_of_an_unchanged_snapshot_shares_the_chunks() {
        let mut world = world(16);
        world.write_full(I3::new(1, 1, 1), RED);
        let snap = world.snapshot();
        let restored = VoxelWorld::from_snapshot(&snap, true, grid_box(16));
        assert!(Arc::ptr_eq(data(&snap.slots[0]), data(&restored.slots[0])));
    }

    #[test]
    fn a_restore_clears_cells_outside_the_active_box() {
        let mut world = world(16);
        let inside = I3::new(20, 20, 20);
        let outside = I3::new(30, 20, 20);
        let other_chunk = I3::new(2, 40, 40);
        for i in [inside, outside, other_chunk] {
            world.write_full(i, RED);
        }
        // The box cuts chunk (1, 1, 1) at x = 24 and excludes chunk (0, 2, 2).
        let active = IBox3 {
            min: I3::new(16, 16, 16),
            max: I3::new(24, 48, 32),
        };
        let restored = VoxelWorld::from_snapshot(&world.snapshot(), true, active);
        assert_eq!(restored.read(inside), RED);
        assert_eq!(restored.cell_at(outside), 0);
        assert_eq!(restored.cell_at(other_chunk), 0);
        let (boundary, _) = restored.locate(inside).unwrap();
        let (dropped, _) = restored.locate(other_chunk).unwrap();
        assert_eq!(data(&restored.slots[boundary]).nonzero(), 1);
        assert!(matches!(restored.slots[dropped], ChunkSlot::Unallocated));
        // The live world is untouched.
        assert_eq!(world.cell_at(outside), RED);
    }

    #[test]
    fn a_restore_rebuilds_the_palette_in_scan_order() {
        let mut world = world(16);
        // Chunk (0, 0, 0): the file order inside the chunk is b, c, a, not memory order a, c, b.
        let a = I3::new(1, 0, 0);
        let b = I3::new(0, 0, 1);
        let c = I3::new(0, 1, 0);
        // Chunk (0, 0, 1) comes after chunk (0, 0, 0), though it is written first.
        world.write_full(I3::new(0, 0, 16), 0xFF00_0004);
        world.write_full(a, 0xFF00_0001);
        world.write_full(c, 0xFF00_0003);
        world.write_full(b, 0xFF00_0002);
        assert_eq!(
            world.palette.values(),
            [0, 0xFF00_0004, 0xFF00_0001, 0xFF00_0003, 0xFF00_0002]
        );
        let restored = VoxelWorld::from_snapshot(&world.snapshot(), true, grid_box(16));
        assert_eq!(
            restored.palette.values(),
            [0, 0xFF00_0002, 0xFF00_0003, 0xFF00_0001, 0xFF00_0004]
        );
    }

    #[test]
    fn a_restore_marks_every_slot_dirty() {
        let mut world = world(16);
        world.write_full(I3::new(1, 1, 1), RED);
        world.write_full(I3::new(40, 40, 40), RED);
        let restored = VoxelWorld::from_snapshot(&world.snapshot(), true, grid_box(16));
        assert_eq!(restored.dirty.count_ones(..), restored.slots.len());
        assert_ne!(restored.rev, world.rev);
    }

    /// The slot indices of a chunk box, `cx` outermost.
    fn slots_of(n: i32, cx: Range<i32>, cy: Range<i32>, cz: Range<i32>) -> Vec<usize> {
        let mut slots = Vec::new();
        for x in cx {
            for y in cy.clone() {
                for z in cz.clone() {
                    slots.push(slot_index(I3::new(x, y, z), n));
                }
            }
        }
        slots
    }

    fn dirty_slots(world: &mut VoxelWorld) -> Vec<usize> {
        world.take_dirty().ones().collect()
    }

    #[test]
    fn shrinking_the_box_dirties_the_touched_chunks_and_their_neighbours() {
        let mut world = world(16);
        let before = world.rev;
        let mut active = grid_box(16);
        active.max.x = 32;
        world.set_active_box(active);
        // The cells x 32..48 became unreadable; chunk column 1 holds their neighbours at x = 31.
        assert_eq!(dirty_slots(&mut world), slots_of(3, 1..3, 0..3, 0..3));
        assert_eq!(world.active_box, active);
        assert_ne!(world.rev, before);
    }

    #[test]
    fn growing_the_box_dirties_the_touched_chunks_and_their_neighbours() {
        let inner = IBox3 {
            min: I3::new(8, 8, 8),
            max: I3::new(40, 40, 40),
        };
        let mut world = VoxelWorld::new(16, true, inner);
        let mut grown = inner;
        grown.max.z = 41;
        let before = world.rev;
        world.set_active_box(grown);
        // The slab z = 40 and its neighbours at x, y 7..41 and z 39..41.
        assert_eq!(dirty_slots(&mut world), slots_of(3, 0..3, 0..3, 2..3));
        assert_ne!(world.rev, before);
    }

    #[test]
    fn an_unchanged_box_dirties_nothing_and_keeps_the_revision() {
        let mut world = world(16);
        let before = world.rev;
        world.set_active_box(grid_box(16));
        assert!(!world.is_dirty());
        assert_eq!(world.rev, before);
    }

    #[test]
    fn a_box_change_dirties_all_changed_sides_not_only_the_first() {
        let mut world = world(16);
        let mut active = grid_box(16);
        active.min.x = 17;
        active.max.z = 31;
        world.set_active_box(active);
        let dirty = dirty_slots(&mut world);
        // x slab 0..17 reaches chunk column 0 and 1; z slab 31..48 reaches layers 1 and 2.
        assert!(dirty.contains(&slot_index(I3::new(0, 0, 0), 3)));
        assert!(dirty.contains(&slot_index(I3::new(2, 2, 2), 3)));
    }

    /// Cells whose readability changed, grown by one voxel, as chunks: the literal reading.
    fn changed_chunks(world: &VoxelWorld, old: IBox3, new: IBox3) -> Vec<usize> {
        let extent = 3 * world.res as i32;
        let mut hit = FixedBitSet::with_capacity(world.slots.len());
        for x in 0..extent {
            for y in 0..extent {
                for z in 0..extent {
                    let cell = I3::new(x, y, z);
                    if old.contains(cell) == new.contains(cell) {
                        continue;
                    }
                    for d in 0..27 {
                        let near = cell + I3::new(d % 3 - 1, d / 3 % 3 - 1, d / 9 - 1);
                        if let Some((slot, _)) = world.locate(near) {
                            hit.insert(slot);
                        }
                    }
                }
            }
        }
        hit.ones().collect()
    }

    proptest! {
        #[test]
        fn a_box_change_dirties_exactly_the_chunks_near_the_changed_cells(
            old in arbitrary_box(33),
            new in arbitrary_box(33),
        ) {
            // R = 11: 33 cells per axis, the last chunk cut at 32.
            let mut world = VoxelWorld::new(11, true, old);
            world.set_active_box(new);
            let expected = changed_chunks(&world, old, new);
            prop_assert_eq!(dirty_slots(&mut world), expected);
        }
    }

    /// The literal restore of tools 7.10: a new world, every voxel written with the full writer in
    /// file order.
    fn replay(snap: &VoxelSnap, ao: bool, active: IBox3) -> VoxelWorld {
        let mut world = VoxelWorld::new(snap.res, ao, active);
        let n = world.n as i32;
        for cx in 0..n {
            for cy in 0..n {
                for cz in 0..n {
                    let c = I3::new(cx, cy, cz);
                    let ChunkSlot::Data(chunk) = &snap.slots[slot_index(c, n)] else {
                        continue;
                    };
                    for (local, value) in chunk.scan() {
                        let l = |shift: u32| i32::try_from(local >> shift & 15).unwrap();
                        let cell = I3::new(l(0), l(4), l(8));
                        world.write_full((c << CHUNK_SHIFT) + cell, value);
                    }
                }
            }
        }
        world
    }

    fn assert_same_content(a: &VoxelWorld, b: &VoxelWorld) {
        assert_eq!(a.palette.values(), b.palette.values(), "palette");
        for (index, (x, y)) in a.slots.iter().zip(&b.slots).enumerate() {
            match (x, y) {
                (ChunkSlot::Unallocated, ChunkSlot::Unallocated) => {}
                (ChunkSlot::Data(p), ChunkSlot::Data(q)) => {
                    assert_eq!(p.cells(), q.cells(), "cells of slot {index}");
                    assert_eq!(p.occ, q.occ, "occupancy of slot {index}");
                    assert_eq!(p.nonzero, q.nonzero, "count of slot {index}");
                }
                _ => panic!("slot {index}: {x:?} against {y:?}"),
            }
        }
    }

    /// A random half-open range `[lo, hi)` inside `0..=extent`.
    fn span(extent: i32) -> impl Strategy<Value = (i32, i32)> {
        (0..=extent).prop_flat_map(move |lo| (Just(lo), lo..=extent))
    }

    fn arbitrary_box(extent: i32) -> impl Strategy<Value = IBox3> {
        (span(extent), span(extent), span(extent)).prop_map(|((x0, x1), (y0, y1), (z0, z1))| {
            IBox3 {
                min: I3::new(x0, y0, z0),
                max: I3::new(x1, y1, z1),
            }
        })
    }

    proptest! {
        // The fast restore (shared chunks, boundary copies, parallel palette) against the literal
        // replay of every voxel through the full writer.
        #[test]
        fn a_restore_equals_the_replay_of_every_voxel_through_the_full_writer(
            writes in prop::collection::vec(
                ((0..33, 0..33, 0..33), prop::sample::select(vec![0, RED, GREEN, BLUE, 0x8000_0001, 0x0000_0005])),
                0..120,
            ),
            active in arbitrary_box(33),
        ) {
            // R = 11: 33 cells per axis in 3 chunks, the last one cut at 32.
            let mut original = world(11);
            for ((x, y, z), value) in writes {
                original.write_full(I3::new(x, y, z), value);
            }
            let snap = original.snapshot();
            let restored = VoxelWorld::from_snapshot(&snap, true, active);
            let replayed = replay(&snap, true, active);
            assert_same_content(&restored, &replayed);
            prop_assert!(restored.slots.iter().all(|s| !matches!(s, ChunkSlot::EmptyWritten)));
        }
    }

    #[test]
    fn restoring_a_world_with_no_cells_gives_an_empty_world() {
        let snap = world(16).snapshot();
        let restored = VoxelWorld::from_snapshot(&snap, false, grid_box(16));
        assert!(
            restored
                .slots
                .iter()
                .all(|s| matches!(s, ChunkSlot::Unallocated))
        );
        assert_eq!(restored.palette.values(), [0]);
        // Chunks that held cells before the restore must be meshed again, so an empty snapshot
        // still dirties the world.
        assert_eq!(restored.dirty.count_ones(..), restored.slots.len());
    }
}
