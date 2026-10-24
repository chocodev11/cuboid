//! A 16x16x16 chunk of voxel cells with a non-zero count and a 4096-bit occupancy set (spec: tools
//! 7.1, 7.10).

use std::fmt;

/// Cells per chunk edge.
pub const CHUNK_EDGE: usize = 16;

/// Cells per chunk.
pub const CHUNK_CELLS: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE;

/// 4096 cells at index `x + 16 * y + 256 * z`, signed ARGB values with 0 = empty. `nonzero` and
/// `occ` always agree with the cells; the mesher, hollow, fill and the searches run on `occ`.
///
/// The cells sit inline (16.5 KiB with the counters), so a chunk is one allocation behind its
/// `Arc` and a read is one pointer away from the slot table.
///
/// spec: tools 7.1, arch 6 (cuboid-voxel), arch 8.4
#[derive(Clone)]
pub struct Chunk {
    pub(crate) cells: [u32; CHUNK_CELLS],
    pub(crate) nonzero: u16,
    pub(crate) occ: [u64; CHUNK_CELLS / 64],
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            cells: [0; CHUNK_CELLS],
            nonzero: 0,
            occ: [0; CHUNK_CELLS / 64],
        }
    }
}

impl fmt::Debug for Chunk {
    // The cells would fill a test report.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chunk")
            .field("nonzero", &self.nonzero)
            .finish_non_exhaustive()
    }
}

impl Chunk {
    /// The local index of the cell `(x, y, z)`, `x + 16 * y + 256 * z`. Each coordinate is below
    /// 16.
    pub const fn local_index(x: usize, y: usize, z: usize) -> usize {
        x + CHUNK_EDGE * y + CHUNK_EDGE * CHUNK_EDGE * z
    }

    /// The cell at local index `x + 16 * y + 256 * z`; 0 past the end.
    pub fn cell(&self, local: usize) -> u32 {
        self.cells.get(local).copied().unwrap_or(0)
    }

    /// All 4096 cells at index `x + 16 * y + 256 * z`, for linear walks (mesher).
    pub fn cells(&self) -> &[u32] {
        self.cells.as_slice()
    }

    /// The non-zero cells as `(local index, value)` in the order the file lists voxels: `x`
    /// outermost, then `y`, then `z` (not memory order). The save and the palette rebuild of a
    /// restore walk this order.
    ///
    /// spec: native 4.7, tools 7.10
    pub fn scan(&self) -> impl Iterator<Item = (usize, u32)> + '_ {
        (0..CHUNK_EDGE)
            .flat_map(|x| {
                (0..CHUNK_EDGE)
                    .flat_map(move |y| (0..CHUNK_EDGE).map(move |z| Self::local_index(x, y, z)))
            })
            .filter_map(|local| match self.cell(local) {
                0 => None,
                value => Some((local, value)),
            })
    }

    /// Sets the cell at local index `local` and keeps the non-zero count and the occupancy bits
    /// in step. Loaders use it to build the chunks of a [`VoxelSnap`](crate::VoxelSnap); the
    /// palette is not touched. An index past the end is ignored.
    pub fn set(&mut self, local: usize, value: u32) {
        let (Some(cell), Some(word)) = (self.cells.get_mut(local), self.occ.get_mut(local / 64))
        else {
            return;
        };
        let was_set = *cell != 0;
        *cell = value;
        if was_set == (value != 0) {
            return;
        }
        let bit = 1 << (local % 64);
        if was_set {
            self.nonzero -= 1;
            *word &= !bit;
        } else {
            self.nonzero += 1;
            *word |= bit;
        }
    }

    /// Whether the cell at local index `local` is non-zero, from the occupancy bits.
    pub fn occupied(&self, local: usize) -> bool {
        self.occ
            .get(local / 64)
            .is_some_and(|word| word >> (local % 64) & 1 == 1)
    }

    /// Number of non-zero cells.
    pub fn nonzero(&self) -> u16 {
        self.nonzero
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_chunk_reads_zero_everywhere_including_past_the_end() {
        let chunk = Chunk::default();
        assert_eq!(chunk.cell(0), 0);
        assert_eq!(chunk.cell(CHUNK_CELLS - 1), 0);
        assert_eq!(chunk.cell(CHUNK_CELLS), 0);
        assert!(!chunk.occupied(CHUNK_CELLS));
        assert_eq!(chunk.nonzero(), 0);
    }

    #[test]
    fn local_index_is_x_plus_16y_plus_256z() {
        assert_eq!(Chunk::local_index(0, 0, 0), 0);
        assert_eq!(Chunk::local_index(1, 0, 0), 1);
        assert_eq!(Chunk::local_index(0, 1, 0), 16);
        assert_eq!(Chunk::local_index(0, 0, 1), 256);
        assert_eq!(Chunk::local_index(15, 15, 15), CHUNK_CELLS - 1);
    }

    #[test]
    fn set_keeps_the_count_and_the_occupancy_bits_in_step() {
        let mut chunk = Chunk::default();
        chunk.set(0, 0xFF00_0001);
        chunk.set(63, 0xFF00_0002);
        chunk.set(64, 0xFF00_0003);
        chunk.set(CHUNK_CELLS - 1, 0xFF00_0004);
        assert_eq!(chunk.nonzero(), 4);
        assert_eq!(chunk.occ[0], 1 | 1 << 63);
        assert_eq!(chunk.occ[1], 1);
        assert_eq!(chunk.occ[63], 1 << 63);

        // Overwriting with another non-zero value changes nothing but the cell.
        chunk.set(0, 0xFF00_0009);
        assert_eq!((chunk.cell(0), chunk.nonzero()), (0xFF00_0009, 4));

        // Erasing clears the bit; erasing an empty cell changes nothing.
        chunk.set(63, 0);
        chunk.set(63, 0);
        assert_eq!(chunk.nonzero(), 3);
        assert!(!chunk.occupied(63) && chunk.occupied(64));
        assert_eq!(chunk.occ[0], 1);

        // Past the end is ignored.
        chunk.set(CHUNK_CELLS, 5);
        assert_eq!(chunk.nonzero(), 3);
    }

    #[test]
    fn scan_lists_voxels_x_outermost_then_y_then_z() {
        let mut chunk = Chunk::default();
        // Memory order is a, c, b; the file order is b, c, a.
        let a = Chunk::local_index(1, 0, 0);
        let c = Chunk::local_index(0, 1, 0);
        let b = Chunk::local_index(0, 0, 1);
        chunk.set(a, 1);
        chunk.set(b, 2);
        chunk.set(c, 3);
        assert_eq!(chunk.scan().collect::<Vec<_>>(), [(b, 2), (c, 3), (a, 1)]);
        assert_eq!(Chunk::default().scan().count(), 0);
    }
}
