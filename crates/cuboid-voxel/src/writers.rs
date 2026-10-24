//! Full, bounded and unchecked writers and the read primitives (spec: tools 7.5).

use std::sync::Arc;

use crate::chunk::Chunk;
use crate::contract::{ChunkSlot, I3, Rev, WriteKind};
use crate::mapping::grid_box;
use crate::world::VoxelWorld;

impl VoxelWorld {
    /// The value at `i`; 0 outside the active box and in chunks that were never written. Never
    /// allocates.
    ///
    /// spec: tools 7.5, tools 7.2
    pub fn read(&self, i: I3) -> u32 {
        if !self.active_box.contains(i) {
            return 0;
        }
        self.cell_at(i)
    }

    /// Like [`VoxelWorld::read`], but a cell outside the active box counts as solid (1).
    ///
    /// spec: tools 7.5
    pub fn read_solid_outside(&self, i: I3) -> u32 {
        if !self.active_box.contains(i) {
            return 1;
        }
        self.cell_at(i)
    }

    /// The full write: box check, palette update, dirtying of every chunk within one voxel.
    /// A write outside the active box is ignored silently.
    ///
    /// spec: tools 7.5
    pub fn write_full(&mut self, i: I3, v: u32) {
        self.write_many(&mut std::iter::once((i, v)), WriteKind::Full);
    }

    /// The bounded raw write: box check and palette update. The original skips the neighbour
    /// dirtying of the full write; the port dirties the same chunks.
    ///
    /// spec: tools 7.5
    pub fn write_bounded(&mut self, i: I3, v: u32) {
        self.write_many(&mut std::iter::once((i, v)), WriteKind::Bounded);
    }

    /// The unchecked raw write: grid bounds only, ignoring the active box, and palette update.
    /// The port dirties the same chunks as the full write.
    ///
    /// spec: tools 7.5
    pub fn write_unchecked(&mut self, i: I3, v: u32) {
        self.write_many(&mut std::iter::once((i, v)), WriteKind::Unchecked);
    }

    /// Writes a batch with the given primitive. A chunk shared with a snapshot is copied by its
    /// first write only, so a brush stroke copies each touched chunk once. The revision changes
    /// once, if any cell passed its box check.
    ///
    /// spec: tools 7.5, arch 8.4
    pub fn write_many(&mut self, cells: &mut dyn Iterator<Item = (I3, u32)>, kind: WriteKind) {
        let mut accepted = false;
        for (i, v) in cells {
            accepted |= self.write_cell(i, v, kind);
        }
        if accepted {
            self.rev = Rev::next();
        }
    }

    /// One write of the matrix of tools 7.5. Returns whether the cell passed its box check.
    ///
    /// | Primitive | Box check | Palette update | Dirtying |
    /// |---|---|---|---|
    /// | full | active box | non-zero alpha | chunks within one voxel |
    /// | bounded | active box | non-zero alpha | chunks within one voxel |
    /// | unchecked | grid `[0, 3R)` | non-zero alpha | chunks within one voxel |
    ///
    /// The original updates the palette of the raw writers for non-zero values only and
    /// dirties neighbours for the full write only, and only across faces; the port note of
    /// tools 7.5 dirties every neighbour for every write (architecture 17.2).
    fn write_cell(&mut self, i: I3, v: u32, kind: WriteKind) -> bool {
        let bounds = match kind {
            WriteKind::Full | WriteKind::Bounded => self.active_box,
            WriteKind::Unchecked => grid_box(self.res),
        };
        if !bounds.contains(i) {
            return false;
        }
        let Some((slot, local)) = self.locate(i) else {
            return false;
        };
        let Some(entry) = self.slots.get_mut(slot) else {
            return false;
        };
        self.palette.insert(v);
        match entry {
            ChunkSlot::Data(chunk) => {
                if chunk.cell(local) != v {
                    let chunk = Arc::make_mut(chunk);
                    chunk.set(local, v);
                    if chunk.nonzero() == 0 {
                        *entry = ChunkSlot::EmptyWritten;
                    }
                }
            }
            // An erase of a cell that holds nothing leaves the chunk as it is: never listed in a
            // file if it never held a voxel, still listed if it did.
            ChunkSlot::Unallocated | ChunkSlot::EmptyWritten if v == 0 => {}
            ChunkSlot::Unallocated | ChunkSlot::EmptyWritten => {
                let mut chunk = Chunk::default();
                chunk.set(local, v);
                *entry = ChunkSlot::Data(Arc::new(chunk));
            }
        }
        self.mark_dirty(i);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use proptest::prelude::*;

    use super::*;
    use crate::contract::IBox3;

    const RED: u32 = 0xFFFF_0000;
    const GREEN: u32 = 0xFF00_FF00;
    const BLUE: u32 = 0xFF00_00FF;
    const KINDS: [WriteKind; 3] = [WriteKind::Full, WriteKind::Bounded, WriteKind::Unchecked];

    fn world(res: u32) -> VoxelWorld {
        VoxelWorld::new(res, true, grid_box(res))
    }

    fn cell(x: i32, y: i32, z: i32) -> I3 {
        I3::new(x, y, z)
    }

    fn write(world: &mut VoxelWorld, kind: WriteKind, i: I3, v: u32) {
        match kind {
            WriteKind::Full => world.write_full(i, v),
            WriteKind::Bounded => world.write_bounded(i, v),
            WriteKind::Unchecked => world.write_unchecked(i, v),
        }
    }

    /// `U`nallocated, `E`mptyWritten or `D`ata per slot.
    fn kinds(world: &VoxelWorld) -> String {
        let letter = |slot: &ChunkSlot| match slot {
            ChunkSlot::Unallocated => 'U',
            ChunkSlot::EmptyWritten => 'E',
            ChunkSlot::Data(_) => 'D',
        };
        world.slots.iter().map(letter).collect()
    }

    fn dirty_slots(world: &VoxelWorld) -> Vec<usize> {
        world.dirty.ones().collect()
    }

    fn chunk_ptr(world: &VoxelWorld, slot: usize) -> *const Chunk {
        match &world.slots[slot] {
            ChunkSlot::Data(chunk) => Arc::as_ptr(chunk),
            other => panic!("expected a data slot, found {other:?}"),
        }
    }

    #[test]
    fn reads_never_allocate_a_chunk() {
        let mut world = world(16);
        world.write_full(cell(5, 5, 5), RED);
        world.write_full(cell(5, 5, 5), 0);
        world.write_full(cell(40, 40, 40), GREEN);
        world.take_dirty();
        let before = (kinds(&world), world.rev);

        for x in -2..50 {
            for y in -2..50 {
                for z in -2..50 {
                    let i = cell(x, y, z);
                    world.read(i);
                    world.read_solid_outside(i);
                }
            }
        }
        assert_eq!((kinds(&world), world.rev), before);
        assert_eq!(kinds(&world).matches('E').count(), 1);
        assert_eq!(kinds(&world).matches('D').count(), 1);
        assert!(!world.is_dirty());

        let fresh = self::world(16);
        assert_eq!(fresh.read(cell(1, 2, 3)), 0);
        assert!(
            fresh
                .slots
                .iter()
                .all(|s| matches!(s, ChunkSlot::Unallocated))
        );
    }

    #[test]
    fn a_read_gives_zero_outside_the_active_box_and_solid_outside_gives_one() {
        let mut world = world(16);
        world.active_box = IBox3 {
            min: cell(8, 8, 8),
            max: cell(24, 24, 24),
        };
        world.write_unchecked(cell(10, 10, 10), RED);
        world.write_unchecked(cell(30, 10, 10), GREEN);

        assert_eq!(world.read(cell(10, 10, 10)), RED);
        assert_eq!(world.read_solid_outside(cell(10, 10, 10)), RED);
        // Inside the box but empty: both give 0.
        assert_eq!(world.read(cell(11, 10, 10)), 0);
        assert_eq!(world.read_solid_outside(cell(11, 10, 10)), 0);
        // Stored, but outside the box: 0 and 1. The box is half-open.
        assert_eq!(world.read(cell(30, 10, 10)), 0);
        assert_eq!(world.read_solid_outside(cell(30, 10, 10)), 1);
        assert_eq!(world.read_solid_outside(cell(24, 10, 10)), 1);
        assert_eq!(world.read_solid_outside(cell(23, 10, 10)), 0);
        // Outside the grid altogether.
        assert_eq!(world.read(cell(-1, 0, 0)), 0);
        assert_eq!(world.read_solid_outside(cell(-1, 0, 0)), 1);
    }

    #[test]
    fn full_and_bounded_writes_outside_the_active_box_are_ignored_silently() {
        for kind in [WriteKind::Full, WriteKind::Bounded] {
            let mut world = world(16);
            world.active_box = IBox3 {
                min: cell(8, 8, 8),
                max: cell(24, 24, 24),
            };
            let rev = world.rev;
            // Inside the grid and outside the box on each side, and outside the grid.
            for i in [
                cell(7, 10, 10),
                cell(24, 10, 10),
                cell(10, 7, 10),
                cell(10, 24, 10),
                cell(10, 10, 7),
                cell(10, 10, 24),
                cell(-1, 10, 10),
                cell(48, 10, 10),
            ] {
                write(&mut world, kind, i, RED);
            }
            assert_eq!(kinds(&world), "U".repeat(27), "{kind:?}");
            assert_eq!(world.palette.values(), [0], "{kind:?}");
            assert!(!world.is_dirty(), "{kind:?}");
            assert_eq!(world.rev, rev, "{kind:?}");

            write(&mut world, kind, cell(8, 8, 8), RED);
            write(&mut world, kind, cell(23, 23, 23), GREEN);
            assert_eq!(world.read(cell(8, 8, 8)), RED, "{kind:?}");
            assert_eq!(world.read(cell(23, 23, 23)), GREEN, "{kind:?}");
            assert_ne!(world.rev, rev, "{kind:?}");
        }
    }

    #[test]
    fn the_unchecked_write_ignores_the_box_but_not_the_grid() {
        let mut world = world(16);
        world.active_box = IBox3 {
            min: cell(8, 8, 8),
            max: cell(24, 24, 24),
        };
        world.write_unchecked(cell(1, 1, 1), RED);
        world.write_unchecked(cell(47, 47, 47), GREEN);
        assert_eq!(world.cell_at(cell(1, 1, 1)), RED);
        assert_eq!(world.cell_at(cell(47, 47, 47)), GREEN);
        assert_eq!(world.read(cell(1, 1, 1)), 0, "the box still hides it");
        assert_eq!(world.palette.values(), [0, RED, GREEN]);

        let rev = world.rev;
        for i in [
            cell(-1, 1, 1),
            cell(1, -1, 1),
            cell(1, 1, -1),
            cell(48, 1, 1),
            cell(1, 48, 1),
            cell(1, 1, 48),
        ] {
            world.write_unchecked(i, BLUE);
        }
        assert_eq!(world.palette.values(), [0, RED, GREEN]);
        assert_eq!(world.rev, rev);
        assert_eq!(kinds(&world).matches('D').count(), 2);
    }

    #[test]
    fn a_write_beyond_the_grid_inside_the_chunk_grid_is_ignored() {
        // R = 2: 6 cells per axis in one chunk of 16. The unchecked write stops at the grid.
        let mut world = world(2);
        world.write_unchecked(cell(5, 5, 5), RED);
        world.write_unchecked(cell(6, 5, 5), GREEN);
        assert_eq!(world.cell_at(cell(5, 5, 5)), RED);
        assert_eq!(world.cell_at(cell(6, 5, 5)), 0);
        assert_eq!(world.palette.values(), [0, RED]);
    }

    #[test]
    fn every_writer_updates_the_palette_for_values_with_alpha_only() {
        for kind in KINDS {
            let mut world = world(16);
            write(&mut world, kind, cell(1, 1, 1), RED);
            write(&mut world, kind, cell(2, 1, 1), GREEN);
            write(&mut world, kind, cell(3, 1, 1), RED);
            write(&mut world, kind, cell(1, 1, 1), 0);
            write(&mut world, kind, cell(4, 1, 1), 0);
            assert_eq!(world.palette.values(), [0, RED, GREEN], "{kind:?}");
        }
    }

    #[test]
    fn a_value_without_alpha_is_stored_but_does_not_enter_the_palette() {
        // Packed colours never look like this; hand-made files can. Tools 7.4 says "non-zero
        // alpha", the matrix of 7.5 says "non-zero" (ledger: unverified-spec).
        let mut world = world(16);
        world.write_full(cell(1, 1, 1), 0x0012_3456);
        assert_eq!(world.read(cell(1, 1, 1)), 0x0012_3456);
        assert_eq!(world.palette.values(), [0]);
    }

    #[test]
    fn a_write_replaces_and_erases_a_cell() {
        for kind in KINDS {
            let mut world = world(16);
            let i = cell(17, 2, 33);
            write(&mut world, kind, i, RED);
            assert_eq!(world.read(i), RED, "{kind:?}");
            write(&mut world, kind, i, GREEN);
            assert_eq!(world.read(i), GREEN, "{kind:?}");
            write(&mut world, kind, i, 0);
            assert_eq!(world.read(i), 0, "{kind:?}");
            // The palette keeps what was used (tools 7.4).
            assert_eq!(world.palette.values(), [0, RED, GREEN], "{kind:?}");
        }
    }

    #[test]
    fn a_write_dirties_the_chunks_within_one_voxel_for_every_writer() {
        // n = 3; slot (cx * 3 + cy) * 3 + cz.
        let cases: [(I3, Vec<usize>); 8] = [
            (cell(8, 8, 8), vec![0]),
            (cell(1, 14, 7), vec![0]),
            // Local x = 0 and local x = 15 touch the chunk on that side.
            (cell(16, 8, 8), vec![0, 9]),
            (cell(15, 8, 8), vec![0, 9]),
            (cell(8, 16, 8), vec![0, 3]),
            (cell(8, 8, 16), vec![0, 1]),
            // An edge touches four chunks, a corner eight.
            (cell(16, 16, 8), vec![0, 3, 9, 12]),
            (cell(16, 16, 16), vec![0, 1, 3, 4, 9, 10, 12, 13]),
        ];
        for kind in KINDS {
            for (i, expected) in &cases {
                let mut world = world(16);
                write(&mut world, kind, *i, RED);
                assert_eq!(dirty_slots(&world), *expected, "{kind:?} at {i}");
                assert!(world.is_dirty());
            }
        }
    }

    #[test]
    fn dirtying_stops_at_the_edge_of_the_chunk_grid() {
        for kind in KINDS {
            let mut world = world(16);
            write(&mut world, kind, cell(0, 0, 0), RED);
            assert_eq!(dirty_slots(&world), [0], "{kind:?}");
            let mut world = self::world(16);
            write(&mut world, kind, cell(47, 47, 47), RED);
            assert_eq!(dirty_slots(&world), [26], "{kind:?}");
        }
    }

    #[test]
    fn an_erase_and_an_unchanged_value_dirty_like_any_write() {
        let mut world = world(16);
        world.write_full(cell(16, 8, 8), RED);
        world.take_dirty();
        world.write_full(cell(16, 8, 8), RED);
        assert_eq!(dirty_slots(&world), [0, 9]);
        world.take_dirty();
        world.write_full(cell(16, 8, 8), 0);
        assert_eq!(dirty_slots(&world), [0, 9]);
        // Erasing where nothing was ever written is a write all the same.
        world.take_dirty();
        world.write_full(cell(40, 40, 40), 0);
        assert!(world.is_dirty());
    }

    #[test]
    fn the_first_non_zero_write_allocates_and_a_zero_write_does_not() {
        let mut world = world(16);
        world.write_full(cell(5, 5, 5), 0);
        assert_eq!(kinds(&world), "U".repeat(27));
        world.write_full(cell(5, 5, 5), RED);
        assert_eq!(&kinds(&world)[..2], "DU");
        world.write_unchecked(cell(40, 40, 40), 0);
        assert_eq!(kinds(&world).matches('D').count(), 1);
    }

    #[test]
    fn an_erased_chunk_is_still_written() {
        for kind in KINDS {
            let mut world = world(16);
            write(&mut world, kind, cell(3, 3, 3), RED);
            write(&mut world, kind, cell(4, 3, 3), GREEN);
            write(&mut world, kind, cell(3, 3, 3), 0);
            assert_eq!(&kinds(&world)[..1], "D", "one voxel is left");
            write(&mut world, kind, cell(4, 3, 3), 0);
            assert_eq!(&kinds(&world)[..1], "E", "{kind:?}");
            assert!(matches!(world.snapshot().slots[0], ChunkSlot::EmptyWritten));
        }
    }

    #[test]
    fn an_erased_chunk_stays_flagged_until_a_voxel_is_written_again() {
        let mut world = world(16);
        world.write_full(cell(3, 3, 3), RED);
        world.write_full(cell(3, 3, 3), 0);
        // A write that clears nothing leaves the flag alone.
        world.write_full(cell(9, 9, 9), 0);
        assert_eq!(&kinds(&world)[..1], "E");
        world.write_full(cell(9, 9, 9), GREEN);
        assert_eq!(&kinds(&world)[..1], "D");
        assert_eq!(world.read(cell(9, 9, 9)), GREEN);
        assert_eq!(world.read(cell(3, 3, 3)), 0);
    }

    #[test]
    fn a_write_copies_a_shared_chunk_once_and_leaves_the_snapshot_untouched() {
        let mut world = world(16);
        world.write_full(cell(1, 1, 1), RED);
        let snap = world.snapshot();
        let shared = chunk_ptr(&world, 0);

        world.write_full(cell(2, 1, 1), GREEN);
        let copy = chunk_ptr(&world, 0);
        assert_ne!(shared, copy, "the first write copies");
        world.write_full(cell(3, 1, 1), GREEN);
        world.write_full(cell(1, 1, 1), 0);
        assert_eq!(chunk_ptr(&world, 0), copy, "the later writes do not");

        let ChunkSlot::Data(old) = &snap.slots[0] else {
            panic!("the snapshot lost its chunk");
        };
        assert_eq!(Arc::as_ptr(old), shared);
        assert_eq!(old.nonzero(), 1);
        assert_eq!(old.cell(Chunk::local_index(1, 1, 1)), RED);
    }

    #[test]
    fn writing_the_value_a_cell_already_holds_does_not_copy_a_shared_chunk() {
        let mut world = world(16);
        world.write_full(cell(1, 1, 1), RED);
        let _snap = world.snapshot();
        let shared = chunk_ptr(&world, 0);
        world.write_full(cell(1, 1, 1), RED);
        assert_eq!(chunk_ptr(&world, 0), shared);
        // The same holds for an erase of an empty cell in an allocated chunk.
        world.write_full(cell(2, 2, 2), 0);
        assert_eq!(chunk_ptr(&world, 0), shared);
    }

    #[test]
    fn a_batch_equals_the_same_writes_one_by_one() {
        let cells: Vec<(I3, u32)> = (0..400)
            .map(|n| {
                let n = n * 7;
                let value = [0, RED, GREEN, BLUE][(n % 4) as usize];
                // Some cells fall outside the box and some outside the grid.
                (cell(n % 53 - 2, n % 41 - 1, n % 47 - 3), value)
            })
            .collect();
        for kind in KINDS {
            let mut batch = world(16);
            batch.active_box = IBox3 {
                min: cell(4, 4, 4),
                max: cell(44, 40, 44),
            };
            let mut single = batch.clone();
            batch.write_many(&mut cells.iter().copied(), kind);
            for &(i, v) in &cells {
                write(&mut single, kind, i, v);
            }
            assert_eq!(kinds(&batch), kinds(&single), "{kind:?}");
            assert_eq!(batch.palette.values(), single.palette.values(), "{kind:?}");
            assert_eq!(batch.dirty, single.dirty, "{kind:?}");
            for x in -2..50 {
                for y in -2..50 {
                    assert_eq!(
                        batch.cell_at(cell(x, y, 20)),
                        single.cell_at(cell(x, y, 20))
                    );
                }
            }
        }
    }

    #[test]
    fn a_batch_changes_the_revision_once_and_only_when_a_cell_was_accepted() {
        let mut world = world(16);
        let start = world.rev;
        world.write_many(&mut std::iter::empty(), WriteKind::Full);
        world.write_many(&mut [(cell(-5, 0, 0), RED)].into_iter(), WriteKind::Full);
        assert_eq!(world.rev, start);
        world.write_many(
            &mut [(cell(1, 1, 1), RED), (cell(2, 1, 1), RED)].into_iter(),
            WriteKind::Full,
        );
        assert_ne!(world.rev, start);
    }

    // The writers against a plain map: stored cells, palette order and the persistence flag of
    // every chunk, over random writes with all three primitives, inside and outside the box.
    struct Model {
        active: IBox3,
        grid: IBox3,
        cells: HashMap<I3, u32>,
        flagged: HashSet<usize>,
        palette: Vec<u32>,
        n: i32,
    }

    impl Model {
        fn slot(&self, i: I3) -> usize {
            (((i.x >> 4) * self.n + (i.y >> 4)) * self.n + (i.z >> 4)) as usize
        }

        fn write(&mut self, kind: WriteKind, i: I3, v: u32) {
            let bounds = if kind == WriteKind::Unchecked {
                self.grid
            } else {
                self.active
            };
            if !bounds.contains(i) {
                return;
            }
            if v >> 24 != 0 && !self.palette.contains(&v) {
                self.palette.push(v);
            }
            if v == 0 {
                self.cells.remove(&i);
                return;
            }
            self.cells.insert(i, v);
            self.flagged.insert(self.slot(i));
        }
    }

    fn arbitrary_write(extent: i32) -> impl Strategy<Value = (WriteKind, I3, u32)> {
        (
            prop::sample::select(KINDS.to_vec()),
            (-2..extent + 2, -2..extent + 2, -2..extent + 2),
            prop::sample::select(vec![0, 0, RED, GREEN, BLUE, 0x8000_0001, 0x0000_0005]),
        )
            .prop_map(|(kind, (x, y, z), v)| (kind, cell(x, y, z), v))
    }

    /// A resolution, writes around its grid, the margin of the active box inside the grid and
    /// the step of a final erase of the stored cells (0 = none, 1 = all), which leaves chunks
    /// that are flagged but empty.
    fn scenario() -> impl Strategy<Value = (u32, Vec<(WriteKind, I3, u32)>, i32, usize)> {
        prop::sample::select(vec![2_u32, 6, 11, 16]).prop_flat_map(|res| {
            let writes = prop::collection::vec(arbitrary_write(3 * res as i32), 0..300);
            (Just(res), writes, 0..8_i32, 0..4_usize)
        })
    }

    proptest! {
        #[test]
        fn the_writers_agree_with_a_plain_map_model(scenario in scenario()) {
            let (res, writes, margin, erase_step) = scenario;
            let mut world = world(res);
            let extent = 3 * res as i32;
            let margin = margin.min(extent / 2);
            world.active_box = IBox3 {
                min: I3::splat(margin),
                max: I3::splat(extent - margin),
            };
            let mut model = Model {
                active: world.active_box,
                grid: grid_box(res),
                cells: HashMap::new(),
                flagged: HashSet::new(),
                palette: vec![0],
                n: world.n as i32,
            };
            for (kind, i, v) in writes {
                write(&mut world, kind, i, v);
                model.write(kind, i, v);
            }
            let mut stored_cells: Vec<I3> = model.cells.keys().copied().collect();
            stored_cells.sort_by_key(|i| i.to_array());
            if erase_step > 0 {
                for &i in stored_cells.iter().step_by(erase_step) {
                    write(&mut world, WriteKind::Unchecked, i, 0);
                    model.write(WriteKind::Unchecked, i, 0);
                }
            }

            prop_assert_eq!(world.palette.values(), model.palette.as_slice());
            let mut stored = 0;
            for (index, slot) in world.slots.iter().enumerate() {
                let has_cells = model.cells.keys().any(|&i| model.slot(i) == index);
                match slot {
                    ChunkSlot::Data(chunk) => {
                        prop_assert!(has_cells, "slot {index} holds data the model lacks");
                        prop_assert!(chunk.nonzero() > 0);
                        let counted = chunk.cells().iter().filter(|&&c| c != 0).count();
                        prop_assert_eq!(counted, usize::from(chunk.nonzero()));
                        let set_bits: u32 = chunk.occ.iter().map(|w| w.count_ones()).sum();
                        prop_assert_eq!(set_bits as usize, counted);
                        for local in 0..4096 {
                            prop_assert_eq!(chunk.occupied(local), chunk.cell(local) != 0);
                        }
                        stored += counted;
                    }
                    ChunkSlot::EmptyWritten => {
                        prop_assert!(!has_cells && model.flagged.contains(&index));
                    }
                    ChunkSlot::Unallocated => {
                        prop_assert!(!has_cells && !model.flagged.contains(&index));
                    }
                }
            }
            prop_assert_eq!(stored, model.cells.len());
            for (&i, &v) in &model.cells {
                prop_assert_eq!(world.cell_at(i), v);
                prop_assert_eq!(world.read(i), if model.active.contains(i) { v } else { 0 });
            }
        }
    }
}
