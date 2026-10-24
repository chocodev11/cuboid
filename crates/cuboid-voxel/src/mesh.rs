//! Canonical chunk mesher: 8-byte face records, ambient-occlusion codes and CPU quad expansion
//! (spec: tools 7.6; arch 13.5).
//!
//! The mesher reads the occupancy bits of the chunk and its 26 neighbours into an 18 x 18 x 18
//! window with the active-box rule applied, finds the exposed faces of whole rows with bit
//! operations and looks the ambient-occlusion neighbours up in the same window. It never
//! allocates chunks and never writes to the world.

use std::sync::Once;

use glam::Vec3;

use crate::chunk::{CHUNK_EDGE, Chunk};
use crate::contract::{ChunkSlot, Dir, ExportVertex, I3, VoxelFace};
use crate::world::VoxelWorld;

/// Cells per chunk edge as a signed coordinate step.
const EDGE: i32 = 16;

/// Cells per axis of the occupancy window: the chunk and one cell of its neighbours around it.
const WINDOW: usize = CHUNK_EDGE + 2;
const WINDOW_I32: i32 = 18;

/// Occupancy rows of the window: bit `x + 1` of `rows[z + 1][y + 1]` is set when the cell
/// `(x, y, z)`, counted from the minimum corner of the chunk, is non-zero and inside the active
/// box. The cells of the neighbouring chunks sit at index 0 and 17.
type Window = [[u32; WINDOW]; WINDOW];

/// Chunks per axis that the 6-bit chunk fields of a face record can address (resolutions up to
/// 341).
const MAX_CHUNKS_PER_AXIS: u32 = 64;

/// Brightness of an ambient-occlusion code: `1 - 0.1 * code`. Codes 0 to 2 occur; 3 completes the
/// two bits.
const AO_FACTOR: [f32; 4] = [1.0, 0.9, 0.8, 0.7];

/// One vertex of a face quad (tools 7.6): the offset from the minimum corner of the voxel in
/// `(x, y, z)` steps, where a z step is toward internal -Z (the index `k` grows that way), and the
/// index offsets `(di, dj, dk)` of the ambient-occlusion neighbours `A` and `B` (the two sides)
/// and `D` (the corner), relative to the voxel.
#[derive(Copy, Clone)]
struct Corner([u8; 3], [i8; 3], [i8; 3], [i8; 3]);

/// The four vertices of each face in pool order ([`Dir::ALL`]), vertex order `v0..v3` of the
/// table of tools 7.6. The quad is `(v0, v1, v2)` and `(v0, v2, v3)`.
const CORNERS: [[Corner; 4]; 6] = [
    // -Y
    [
        Corner([0, 0, 1], [0, -1, 1], [-1, -1, 0], [-1, -1, 1]),
        Corner([1, 0, 1], [1, -1, 0], [0, -1, 1], [1, -1, 1]),
        Corner([1, 0, 0], [0, -1, -1], [1, -1, 0], [1, -1, -1]),
        Corner([0, 0, 0], [-1, -1, 0], [0, -1, -1], [-1, -1, -1]),
    ],
    // +Y
    [
        Corner([0, 1, 0], [-1, 1, 0], [0, 1, -1], [-1, 1, -1]),
        Corner([1, 1, 0], [0, 1, -1], [1, 1, 0], [1, 1, -1]),
        Corner([1, 1, 1], [1, 1, 0], [0, 1, 1], [1, 1, 1]),
        Corner([0, 1, 1], [0, 1, 1], [-1, 1, 0], [-1, 1, 1]),
    ],
    // +Z (internal)
    [
        Corner([1, 0, 0], [1, 0, -1], [0, -1, -1], [1, -1, -1]),
        Corner([1, 1, 0], [0, 1, -1], [1, 0, -1], [1, 1, -1]),
        Corner([0, 1, 0], [-1, 0, -1], [0, 1, -1], [-1, 1, -1]),
        Corner([0, 0, 0], [0, -1, -1], [-1, 0, -1], [-1, -1, -1]),
    ],
    // -Z (internal)
    [
        Corner([0, 0, 1], [0, -1, 1], [-1, 0, 1], [-1, -1, 1]),
        Corner([0, 1, 1], [-1, 0, 1], [0, 1, 1], [-1, 1, 1]),
        Corner([1, 1, 1], [0, 1, 1], [1, 0, 1], [1, 1, 1]),
        Corner([1, 0, 1], [1, 0, 1], [0, -1, 1], [1, -1, 1]),
    ],
    // -X
    [
        Corner([0, 0, 0], [-1, -1, 0], [-1, 0, -1], [-1, -1, -1]),
        Corner([0, 1, 0], [-1, 0, -1], [-1, 1, 0], [-1, 1, -1]),
        Corner([0, 1, 1], [-1, 1, 0], [-1, 0, 1], [-1, 1, 1]),
        Corner([0, 0, 1], [-1, 0, 1], [-1, -1, 0], [-1, -1, 1]),
    ],
    // +X
    [
        Corner([1, 0, 1], [1, 0, 1], [1, -1, 0], [1, -1, 1]),
        Corner([1, 1, 1], [1, 1, 0], [1, 0, 1], [1, 1, 1]),
        Corner([1, 1, 0], [1, 0, -1], [1, 1, 0], [1, 1, -1]),
        Corner([1, 0, 0], [1, -1, 0], [1, 0, -1], [1, -1, -1]),
    ],
];

/// The flat normal of each face in pool order, in internal space.
const NORMALS: [Vec3; 6] = [
    Vec3::new(0.0, -1.0, 0.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::new(-1.0, 0.0, 0.0),
    Vec3::new(1.0, 0.0, 0.0),
];

impl VoxelFace {
    /// Packs a face record. `local` and `chunk` are the voxel inside its chunk (below 16) and the
    /// chunk in the chunk grid (below 64), `rgb` is `0xRRGGBB` and `ao` the codes of the vertices
    /// `v0..v3` (below 4).
    ///
    /// Word 0: bits 0-3 local x, 4-7 local y, 8-11 local z, 12-17 chunk x, 18-23 chunk y, 24-29
    /// chunk z. Word 1: bits 0-23 colour, 24-25 AO of `v0`, 26-27 `v1`, 28-29 `v2`, 30-31 `v3`.
    ///
    /// spec: arch 13.5
    pub fn new(local: [u32; 3], chunk: [u32; 3], rgb: u32, ao: [u32; 4]) -> Self {
        let [x, y, z] = local;
        let [cx, cy, cz] = chunk;
        let [a0, a1, a2, a3] = ao;
        Self([
            x | y << 4 | z << 8 | cx << 12 | cy << 18 | cz << 24,
            rgb & 0xFF_FFFF | a0 << 24 | a1 << 26 | a2 << 28 | a3 << 30,
        ])
    }

    /// The voxel `(x, y, z)` inside its chunk.
    pub fn local(self) -> [u32; 3] {
        let w = self.0[0];
        [w & 15, w >> 4 & 15, w >> 8 & 15]
    }

    /// The chunk `(cx, cy, cz)` in the chunk grid.
    pub fn chunk(self) -> [u32; 3] {
        let w = self.0[0];
        [w >> 12 & 63, w >> 18 & 63, w >> 24 & 63]
    }

    /// The voxel colour `0xRRGGBB`.
    pub fn rgb(self) -> u32 {
        self.0[1] & 0xFF_FFFF
    }

    /// The ambient-occlusion codes of the vertices `v0..v3`: the brightness is `1 - 0.1 * code`.
    pub fn ao(self) -> [u32; 4] {
        let w = self.0[1];
        [w >> 24 & 3, w >> 26 & 3, w >> 28 & 3, w >> 30]
    }
}

/// The occupancy bits of row `(y, z)` of `chunk`, bit `x` for the cell `x`; 0 without a chunk.
fn row_bits(chunk: Option<&Chunk>, y: usize, z: usize) -> u32 {
    let row = z * CHUNK_EDGE + y;
    let word = chunk.and_then(|c| c.occ.get(row / 4)).copied();
    word.map_or(0, |w| {
        u32::from(u16::try_from(w >> (row % 4 * 16) & 0xFFFF).unwrap_or_default())
    })
}

/// The neighbour chunk (0 = before, 1 = this, 2 = after) and the local coordinate of window
/// position `p`.
fn split(p: usize) -> (usize, usize) {
    if p == 0 {
        (0, CHUNK_EDGE - 1)
    } else if p == WINDOW - 1 {
        (2, 0)
    } else {
        (1, p - 1)
    }
}

/// The window around chunk `c`: occupancy bits of the chunk and its 26 neighbours with the
/// active-box rule applied (a cell outside the box is empty).
fn window(world: &VoxelWorld, c: I3) -> Window {
    let n = world.chunks_per_axis() as i32;
    let offsets = (-1..=1)
        .flat_map(|dz| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| I3::new(dx, dy, dz))));
    let mut near: [Option<&Chunk>; 27] = [None; 27];
    for (slot, offset) in near.iter_mut().zip(offsets) {
        let q = c + offset;
        if q.cmplt(I3::ZERO).any() || q.cmpge(I3::splat(n)).any() {
            continue;
        }
        if let Some(ChunkSlot::Data(chunk)) =
            world.slots().get(((q.x * n + q.y) * n + q.z) as usize)
        {
            *slot = Some(chunk);
        }
    }

    let mut rows: Window = [[0; WINDOW]; WINDOW];
    for (pz, plane) in rows.iter_mut().enumerate() {
        let (dz, lz) = split(pz);
        for (py, row) in plane.iter_mut().enumerate() {
            let (dy, ly) = split(py);
            let at = |dx: usize| {
                let chunk = near.get(dz * 9 + dy * 3 + dx).copied().flatten();
                row_bits(chunk, ly, lz)
            };
            *row = at(1) << 1 | at(0) >> 15 | (at(2) & 1) << 17;
        }
    }

    let origin = c * EDGE - I3::ONE;
    let active = world.active_box;
    if active.min.cmple(origin).all() && active.max.cmpge(origin + I3::splat(WINDOW_I32)).all() {
        return rows;
    }
    // Bits of the window columns that lie inside the box.
    let first = (active.min.x - origin.x).clamp(0, WINDOW_I32);
    let end = (active.max.x - origin.x).clamp(0, WINDOW_I32);
    let columns = if end > first {
        (1u32 << end) - (1 << first)
    } else {
        0
    };
    for (gz, plane) in (origin.z..).zip(rows.iter_mut()) {
        for (gy, row) in (origin.y..).zip(plane.iter_mut()) {
            let inside = (active.min.y..active.max.y).contains(&gy)
                && (active.min.z..active.max.z).contains(&gz);
            *row = if inside { *row & columns } else { 0 };
        }
    }
    rows
}

/// The ambient-occlusion code of a vertex from its neighbours: 2 when both sides are occupied,
/// else the number of occupied neighbours (tools 7.6: factor `0.8`, else `1 - 0.1 * count`).
fn ao_code(a: bool, b: bool, d: bool) -> u32 {
    if a && b {
        2
    } else {
        u32::from(a) + u32::from(b) + u32::from(d)
    }
}

/// Meshes the chunk in slot `slot` into one list of face records per direction (`out` is indexed
/// like [`Dir::ALL`]). A face is emitted for every non-zero voxel whose neighbour in that
/// direction is empty, a neighbour outside the active box counting as empty. The lists are
/// appended to; the caller clears them.
///
/// A world with more than 64 chunks per axis (resolution 342 and up) does not fit the chunk fields
/// of the record: it logs an error once and gives no faces.
///
/// A stored voxel outside the active box is not read (tools 7.2), so it gets no faces and does not
/// occlude. The faces of a chunk come in the order `z`, `y`, `x` of the voxels within each list;
/// the specification does not fix an order.
///
/// spec: tools 7.6, arch 13.5
pub fn mesh_chunk(world: &VoxelWorld, slot: usize, out: &mut [Vec<VoxelFace>; 6]) {
    let Some(ChunkSlot::Data(chunk)) = world.slots().get(slot) else {
        return;
    };
    if world.chunks_per_axis() > MAX_CHUNKS_PER_AXIS {
        static LOGGED: Once = Once::new();
        LOGGED.call_once(|| {
            log::error!(
                "voxel resolution {} needs more than {MAX_CHUNKS_PER_AXIS} chunks per axis; the mesher draws nothing",
                world.res
            );
        });
        return;
    }
    let n = world.chunks_per_axis() as usize;
    let coords = [slot / (n * n), slot / n % n, slot % n].map(|c| u32::try_from(c).unwrap_or(0));
    let [cx, cy, cz] = coords;
    let rows = window(world, I3::new(cx as i32, cy as i32, cz as i32));
    let cells = chunk.cells();

    for (z, planes) in (0u32..).zip(rows.windows(3)) {
        let [before, plane, after] = planes else {
            continue;
        };
        let neighbours = before.iter().skip(1).zip(after.iter().skip(1));
        for ((y, trio), (k_before, k_after)) in (0u32..).zip(plane.windows(3)).zip(neighbours) {
            let [south, here, north] = trio else {
                continue;
            };
            let mid = *here >> 1 & 0xFFFF;
            // Exposed voxels of the row, one bit per x, in pool order.
            let masks = [
                mid & !(south >> 1),
                mid & !(north >> 1),
                mid & !(k_before >> 1),
                mid & !(k_after >> 1),
                mid & !here,
                mid & !(here >> 2),
            ];
            if masks.iter().all(|&m| m == 0) {
                continue;
            }
            // Whether the cell at the index offset from voxel `(x, y, z)` of this row is occupied.
            let occupied = |x: u32, [di, dj, dk]: [i8; 3]| {
                let at = |p: u32, d: i8| (p as usize + 1).checked_add_signed(isize::from(d));
                let (Some(px), Some(py), Some(pz)) = (at(x, di), at(y, dj), at(z, dk)) else {
                    return false;
                };
                rows.get(pz)
                    .and_then(|p| p.get(py))
                    .is_some_and(|r| r >> px & 1 == 1)
            };
            for ((mask, faces), corners) in masks.into_iter().zip(out.iter_mut()).zip(&CORNERS) {
                let mut bits = mask;
                while bits != 0 {
                    let x = bits.trailing_zeros();
                    bits &= bits - 1;
                    let ao = if world.ao {
                        corners.map(|Corner(_, a, b, d)| {
                            ao_code(occupied(x, a), occupied(x, b), occupied(x, d))
                        })
                    } else {
                        [0; 4]
                    };
                    let local = (z * 256 + y * 16 + x) as usize;
                    let rgb = cells.get(local).copied().unwrap_or(0);
                    faces.push(VoxelFace::new([x, y, z], coords, rgb, ao));
                }
            }
        }
    }
}

/// Expands a face record into the four vertices of its quad, in the vertex order of the mesher
/// table. `cell` is the voxel edge in world units (`16 / res`).
///
/// The positions are the chunk-local vertices of tools 7.6 (`s = cell`) moved to the chunk origin
/// `(16 i' c - 16, 16 j' c - 16, 16 - 16 k' c)`. The colour is the voxel colour times the
/// ambient-occlusion factor of the vertex in all four channels, alpha included, as the
/// specification states.
///
/// spec: tools 7.6, arch 13.5
pub fn expand_face(face: VoxelFace, dir: Dir, cell: f32) -> [ExportVertex; 4] {
    let [x, y, z] = face.local();
    let [cx, cy, cz] = face.chunk();
    let origin = Vec3::new(
        (16 * cx) as f32 * cell - 16.0,
        (16 * cy) as f32 * cell - 16.0,
        16.0 - (16 * cz) as f32 * cell,
    );
    let rgb = face.rgb();
    let [r, g, b] = [rgb >> 16, rgb >> 8, rgb].map(|c| (c & 0xFF) as f32 / 255.0);
    let (Some(&normal), Some(&corners)) = (NORMALS.get(dir as usize), CORNERS.get(dir as usize))
    else {
        return [ExportVertex::default(); 4];
    };
    let mut vertices = [ExportVertex::default(); 4];
    for ((vertex, Corner([ox, oy, oz], ..)), code) in
        vertices.iter_mut().zip(corners).zip(face.ao())
    {
        let local = Vec3::new(
            (x + u32::from(ox)) as f32 * cell,
            (y + u32::from(oy)) as f32 * cell,
            -((z + u32::from(oz)) as f32) * cell,
        );
        let shade = AO_FACTOR.get(code as usize).copied().unwrap_or(1.0);
        *vertex = ExportVertex {
            pos: origin + local,
            normal,
            color: [r * shade, g * shade, b * shade, shade],
        };
    }
    vertices
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use proptest::prelude::*;
    use rayon::prelude::*;

    use super::*;
    use crate::contract::{IBox3, WriteKind};
    use crate::mapping::{cell_size, grid_box};

    const RED: u32 = 0xFFFF_0000;
    const GREEN: u32 = 0xFF00_FF00;
    const BLUE: u32 = 0xFF00_00FF;

    /// A vertex as the table of tools 7.6 lists it: the position as offsets from `(x, y, -z)`
    /// (so `-z-1` is `-1`) and the occupancy neighbours `A`, `B` and `D` as index offsets.
    type SpecVertex = ([i32; 3], [i32; 3], [i32; 3], [i32; 3]);

    /// The table of tools 7.6 typed from the specification, rows in pool order.
    const SPEC: [[SpecVertex; 4]; 6] = [
        // -Y
        [
            ([0, 0, -1], [0, -1, 1], [-1, -1, 0], [-1, -1, 1]),
            ([1, 0, -1], [1, -1, 0], [0, -1, 1], [1, -1, 1]),
            ([1, 0, 0], [0, -1, -1], [1, -1, 0], [1, -1, -1]),
            ([0, 0, 0], [-1, -1, 0], [0, -1, -1], [-1, -1, -1]),
        ],
        // +Y
        [
            ([0, 1, 0], [-1, 1, 0], [0, 1, -1], [-1, 1, -1]),
            ([1, 1, 0], [0, 1, -1], [1, 1, 0], [1, 1, -1]),
            ([1, 1, -1], [1, 1, 0], [0, 1, 1], [1, 1, 1]),
            ([0, 1, -1], [0, 1, 1], [-1, 1, 0], [-1, 1, 1]),
        ],
        // +Z
        [
            ([1, 0, 0], [1, 0, -1], [0, -1, -1], [1, -1, -1]),
            ([1, 1, 0], [0, 1, -1], [1, 0, -1], [1, 1, -1]),
            ([0, 1, 0], [-1, 0, -1], [0, 1, -1], [-1, 1, -1]),
            ([0, 0, 0], [0, -1, -1], [-1, 0, -1], [-1, -1, -1]),
        ],
        // -Z
        [
            ([0, 0, -1], [0, -1, 1], [-1, 0, 1], [-1, -1, 1]),
            ([0, 1, -1], [-1, 0, 1], [0, 1, 1], [-1, 1, 1]),
            ([1, 1, -1], [0, 1, 1], [1, 0, 1], [1, 1, 1]),
            ([1, 0, -1], [1, 0, 1], [0, -1, 1], [1, -1, 1]),
        ],
        // -X
        [
            ([0, 0, 0], [-1, -1, 0], [-1, 0, -1], [-1, -1, -1]),
            ([0, 1, 0], [-1, 0, -1], [-1, 1, 0], [-1, 1, -1]),
            ([0, 1, -1], [-1, 1, 0], [-1, 0, 1], [-1, 1, 1]),
            ([0, 0, -1], [-1, 0, 1], [-1, -1, 0], [-1, -1, 1]),
        ],
        // +X
        [
            ([1, 0, -1], [1, 0, 1], [1, -1, 0], [1, -1, 1]),
            ([1, 1, -1], [1, 1, 0], [1, 0, 1], [1, 1, 1]),
            ([1, 1, 0], [1, 0, -1], [1, 1, 0], [1, 1, -1]),
            ([1, 0, 0], [1, -1, 0], [1, 0, -1], [1, -1, -1]),
        ],
    ];

    /// The neighbour index offset of each face: `-Y` is `j - 1`, `+Y` `j + 1`, `+Z` (internal)
    /// `k - 1`, `-Z` `k + 1`, `-X` `i - 1`, `+X` `i + 1`.
    const STEPS: [[i32; 3]; 6] = [
        [0, -1, 0],
        [0, 1, 0],
        [0, 0, -1],
        [0, 0, 1],
        [-1, 0, 0],
        [1, 0, 0],
    ];

    type Faces = [Vec<VoxelFace>; 6];

    fn world(res: u32) -> VoxelWorld {
        VoxelWorld::new(res, true, grid_box(res))
    }

    fn cell(x: i32, y: i32, z: i32) -> I3 {
        I3::new(x, y, z)
    }

    fn slot_of(world: &VoxelWorld, i: I3) -> usize {
        world.locate(i).unwrap().0
    }

    fn mesh(world: &VoxelWorld, slot: usize) -> Faces {
        let mut out = Faces::default();
        mesh_chunk(world, slot, &mut out);
        out
    }

    /// Every face of the world as `(slot, pool, record)`, sorted.
    fn mesh_all(world: &VoxelWorld) -> Vec<(usize, usize, VoxelFace)> {
        let mut all = Vec::new();
        for slot in 0..world.slots().len() {
            for (dir, faces) in mesh(world, slot).into_iter().enumerate() {
                all.extend(faces.into_iter().map(|face| (slot, dir, face)));
            }
        }
        all.sort_by_key(|&(slot, dir, face)| (slot, dir, face.0));
        all
    }

    /// The literal mesher of tools 7.6: every readable non-zero cell, six reads through the box
    /// rule, ambient occlusion from the typed table.
    fn reference(world: &VoxelWorld) -> Vec<(usize, usize, VoxelFace)> {
        let extent = 3 * world.res as i32;
        let mut all = Vec::new();
        for x in 0..extent {
            for y in 0..extent {
                for z in 0..extent {
                    let i = cell(x, y, z);
                    let value = world.read(i);
                    if value == 0 {
                        continue;
                    }
                    let (slot, _) = world.locate(i).unwrap();
                    let local = [x, y, z].map(|v| u32::try_from(v % 16).unwrap());
                    let chunk = [x, y, z].map(|v| u32::try_from(v / 16).unwrap());
                    for (dir, step) in STEPS.iter().enumerate() {
                        if world.read(i + I3::from_array(*step)) != 0 {
                            continue;
                        }
                        let seen = |off: [i32; 3]| world.read(i + I3::from_array(off)) != 0;
                        let ao = SPEC[dir].map(|(_, a, b, d)| {
                            if !world.ao {
                                0
                            } else if seen(a) && seen(b) {
                                2
                            } else {
                                u32::from(seen(a)) + u32::from(seen(b)) + u32::from(seen(d))
                            }
                        });
                        let face = VoxelFace::new(local, chunk, value & 0xFF_FFFF, ao);
                        all.push((slot, dir, face));
                    }
                }
            }
        }
        all.sort_by_key(|&(slot, dir, face)| (slot, dir, face.0));
        all
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn assert_vec(actual: Vec3, expected: Vec3, what: &str) {
        assert!(
            close(actual.x, expected.x)
                && close(actual.y, expected.y)
                && close(actual.z, expected.z),
            "{what}: {actual} against {expected}"
        );
    }

    #[test]
    fn a_single_voxel_gives_one_face_in_each_pool_with_the_packed_fields() {
        let mut world = world(16);
        world.write_full(cell(3, 4, 5), RED);
        let out = mesh(&world, 0);
        for (dir, faces) in out.iter().enumerate() {
            assert_eq!(faces.len(), 1, "pool {dir}");
        }
        // Local (3, 4, 5) in chunk (0, 0, 0); colour without alpha; no occlusion.
        assert_eq!(out[0][0].0, [0x0000_0543, 0x00FF_0000]);
        assert!(out.iter().all(|faces| faces[0] == out[0][0]));
    }

    #[test]
    fn the_chunk_and_the_ao_codes_sit_at_their_bit_positions() {
        let face = VoxelFace::new([3, 1, 8], [2, 1, 3], 0x12_3456, [1, 2, 0, 3]);
        assert_eq!(face.0[0], 0x0304_2813);
        assert_eq!(face.0[1], 0xC800_0000 | 0x0100_0000 | 0x12_3456);
        assert_eq!(face.local(), [3, 1, 8]);
        assert_eq!(face.chunk(), [2, 1, 3]);
        assert_eq!(face.rgb(), 0x12_3456);
        assert_eq!(face.ao(), [1, 2, 0, 3]);

        // The largest chunk grid (R = 254, 48 chunks per axis) fits the 6-bit fields.
        let far = VoxelFace::new([15; 3], [47; 3], 0xFF_FFFF, [3; 4]);
        assert_eq!(
            (far.local(), far.chunk(), far.ao()),
            ([15; 3], [47; 3], [3; 4])
        );
    }

    #[test]
    fn a_voxel_in_a_later_chunk_carries_its_chunk_coordinates() {
        let mut world = world(16);
        let i = cell(35, 17, 40);
        world.write_full(i, GREEN);
        let out = mesh(&world, slot_of(&world, i));
        assert_eq!(out[3][0].0, [0x0204_2813, 0x0000_FF00]);
        assert_eq!(out[3][0].chunk(), [2, 1, 2]);
        assert_eq!(out[3][0].local(), [3, 1, 8]);
    }

    #[test]
    fn the_chunk_fields_cover_64_chunks_per_axis_and_larger_worlds_give_no_faces() {
        // R = 341 has 64 chunks per axis: the last chunk is 63.
        let mut fits = world(341);
        assert_eq!(fits.chunks_per_axis(), 64);
        let last = cell(1022, 1022, 1022);
        fits.write_full(last, RED);
        let out = mesh(&fits, slot_of(&fits, last));
        assert_eq!(out.iter().map(Vec::len).sum::<usize>(), 6);
        assert_eq!(out[0][0].chunk(), [63, 63, 63]);
        assert_eq!(out[0][0].local(), [14, 14, 14]);

        // R = 342 has 65: chunk 64 would run into the next field.
        let mut too_big = world(342);
        assert_eq!(too_big.chunks_per_axis(), 65);
        let last = cell(1025, 1025, 1025);
        too_big.write_full(last, RED);
        let out = mesh(&too_big, slot_of(&too_big, last));
        assert!(out.iter().all(Vec::is_empty));
    }

    #[test]
    fn the_corner_table_equals_the_specification_table() {
        for (dir, spec) in SPEC.iter().enumerate() {
            for (v, (pos, a, b, d)) in spec.iter().enumerate() {
                let Corner(table_pos, table_a, table_b, table_d) = CORNERS[dir][v];
                // The table counts z steps toward -Z, the specification writes -z - 1 and -z.
                let expected_pos = [pos[0], pos[1], -pos[2]];
                assert_eq!(
                    table_pos.map(i32::from),
                    expected_pos,
                    "{dir} v{v} position"
                );
                assert_eq!(table_a.map(i32::from), *a, "{dir} v{v} A");
                assert_eq!(table_b.map(i32::from), *b, "{dir} v{v} B");
                assert_eq!(table_d.map(i32::from), *d, "{dir} v{v} D");
            }
        }
    }

    #[test]
    fn the_ao_neighbours_of_every_vertex_lie_toward_that_vertex() {
        // Independent of the typed tables: A and B are the neighbours behind the face shifted
        // along one side, D behind the corner, and the shifts point to the vertex.
        for (dir, step) in STEPS.iter().enumerate() {
            let axis = step.iter().position(|&s| s != 0).unwrap();
            for Corner(pos, a, b, d) in CORNERS[dir] {
                let (a, b, d) = (a.map(i32::from), b.map(i32::from), d.map(i32::from));
                for k in 0..3 {
                    if k == axis {
                        assert_eq!((a[k], b[k], d[k]), (step[k], step[k], step[k]));
                    } else {
                        let toward = 2 * i32::from(pos[k]) - 1;
                        assert_eq!(d[k], toward, "corner {pos:?} of face {dir}, axis {k}");
                        assert_eq!(a[k] + b[k], toward, "A and B split the corner");
                    }
                }
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn the_quads_face_along_their_normal_by_the_cross_product_rule() {
        // Tools 6.5: `(v1 - v0) x (v2 - v0)` points along the outward normal. In the left-handed
        // internal space these triangles look clockwise from outside.
        let face = VoxelFace::new([3, 4, 5], [0; 3], 0xFF_FFFF, [0; 4]);
        for (dir, normal) in Dir::ALL.into_iter().zip(NORMALS) {
            let v = expand_face(face, dir, 1.0);
            let first = (v[1].pos - v[0].pos).cross(v[2].pos - v[0].pos);
            let second = (v[2].pos - v[0].pos).cross(v[3].pos - v[0].pos);
            assert_vec(first, normal, "triangle (0, 1, 2)");
            assert_vec(second, normal, "triangle (0, 2, 3)");
            assert!(v.iter().all(|vertex| vertex.normal == normal));
        }
    }

    #[test]
    fn expanded_quads_equal_the_specification_positions_for_all_six_faces() {
        // Chunk (0, 0, 0) with c = 1 starts at (-16, -16, 16).
        let origin = Vec3::new(-16.0, -16.0, 16.0);
        let face = VoxelFace::new([3, 4, 5], [0; 3], 0xFF_FFFF, [0; 4]);
        for (dir, spec) in Dir::ALL.into_iter().zip(SPEC) {
            let v = expand_face(face, dir, 1.0);
            for (n, (pos, ..)) in spec.iter().enumerate() {
                let expected =
                    origin + Vec3::new(3.0, 4.0, -5.0) + Vec3::from_array(pos.map(|p| p as f32));
                assert_vec(v[n].pos, expected, &format!("{dir:?} v{n}"));
            }
        }
        // Spelled out for the first face: -Y at z = -5 spans -6 to -5 above the chunk origin.
        let v = expand_face(face, Dir::NegY, 1.0);
        assert_eq!(v[0].pos, Vec3::new(-13.0, -12.0, 10.0));
        assert_eq!(v[2].pos, Vec3::new(-12.0, -12.0, 11.0));
    }

    #[test]
    fn expansion_scales_with_the_cell_and_moves_to_the_chunk_origin() {
        // R = 32: c = 0.5. Chunk (2, 1, 3) starts at (16 * 2 * 0.5 - 16, 16 * 1 * 0.5 - 16,
        // 16 - 16 * 3 * 0.5) = (0, -8, -8).
        let face = VoxelFace::new([1, 2, 3], [2, 1, 3], 0, [0; 4]);
        // +X: plane x + 1, v0 at -z - 1.
        let v = expand_face(face, Dir::PosX, 0.5);
        assert_vec(v[0].pos, Vec3::new(1.0, -7.0, -10.0), "v0");
        assert_vec(v[2].pos, Vec3::new(1.0, -6.5, -9.5), "v2");

        // The vertex -Y v3 is the minimum x, minimum y and maximum z corner of the cell, which
        // is what mapping::index_to_world returns.
        let res = 32;
        let i = cell(2 * 16 + 1, 16 + 2, 3 * 16 + 3);
        let v = expand_face(face, Dir::NegY, cell_size(res));
        assert_vec(
            v[3].pos,
            crate::mapping::index_to_world(i, res),
            "corner of the cell",
        );
    }

    #[test]
    fn expanded_colours_multiply_all_four_channels_by_the_vertex_factor() {
        let face = VoxelFace::new([0; 3], [0; 3], 0xFF_8000, [0, 1, 2, 3]);
        let v = expand_face(face, Dir::PosY, 1.0);
        let green = 128.0 / 255.0;
        let expected = [
            [1.0, green, 0.0, 1.0],
            [0.9, 0.9 * green, 0.0, 0.9],
            [0.8, 0.8 * green, 0.0, 0.8],
            [0.7, 0.7 * green, 0.0, 0.7],
        ];
        for (vertex, want) in v.iter().zip(expected) {
            assert!(
                vertex.color.iter().zip(want).all(|(a, b)| close(*a, b)),
                "{vertex:?}"
            );
        }
    }

    #[test]
    fn every_ao_combination_of_every_vertex_gives_the_specified_code() {
        // Voxels in the middle of a chunk and on its corners, so the neighbours sit in the same
        // chunk and in up to seven others.
        for base in [
            cell(8, 8, 8),
            cell(15, 15, 15),
            cell(16, 16, 16),
            cell(31, 16, 15),
        ] {
            let local = [base.x, base.y, base.z].map(|c| u32::try_from(c % 16).unwrap());
            for (dir, spec) in SPEC.iter().enumerate() {
                for (v, (_, a, b, d)) in spec.iter().enumerate() {
                    for subset in 0..8_u32 {
                        let mut world = world(16);
                        world.write_full(base, RED);
                        for (bit, off) in [a, b, d].into_iter().enumerate() {
                            if subset >> bit & 1 == 1 {
                                world.write_full(base + I3::from_array(*off), GREEN);
                            }
                        }
                        let out = mesh(&world, slot_of(&world, base));
                        let face = out[dir].iter().find(|f| f.local() == local).unwrap();
                        let (a, b, d) = (subset & 1 == 1, subset & 2 != 0, subset & 4 != 0);
                        let expected = if a && b {
                            2
                        } else {
                            u32::from(a) + u32::from(b) + u32::from(d)
                        };
                        assert_eq!(
                            face.ao()[v],
                            expected,
                            "{base} face {dir} v{v} subset {subset:03b}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_ao_factors_are_one_nine_and_eight_tenths() {
        assert_eq!(AO_FACTOR[..3], [1.0, 0.9, 0.8]);
        for (code, factor) in AO_FACTOR.into_iter().enumerate() {
            assert!(close(factor, 1.0 - 0.1 * code as f32));
        }
    }

    #[test]
    fn a_wall_beside_a_voxel_darkens_the_vertices_that_touch_it() {
        // A voxel at (8, 8, 8) and a block over its -X neighbour column at (7, 9, 8): the +Y face
        // of the voxel has vertices v0 (x, -z) and v3 (x, -z - 1) on the wall side.
        let mut world = world(16);
        world.write_full(cell(8, 8, 8), RED);
        world.write_full(cell(7, 9, 8), GREEN);
        let out = mesh(&world, 0);
        let top = out[1].iter().find(|f| f.local() == [8, 8, 8]).unwrap();
        assert_eq!(top.ao(), [1, 0, 0, 1]);
    }

    #[test]
    fn ambient_occlusion_off_gives_code_zero_everywhere() {
        let mut world = world(16);
        world.ao = false;
        for i in [cell(8, 8, 8), cell(8, 9, 8), cell(9, 9, 8), cell(7, 9, 9)] {
            world.write_full(i, RED);
        }
        let all = mesh_all(&world);
        assert!(!all.is_empty());
        assert!(all.iter().all(|(_, _, face)| face.ao() == [0; 4]));
    }

    #[test]
    fn faces_between_voxels_of_neighbouring_chunks_are_culled_on_both_sides() {
        let mut world = world(16);
        world.write_full(cell(15, 3, 3), RED);
        world.write_full(cell(16, 3, 3), GREEN);
        let left = mesh(&world, slot_of(&world, cell(15, 3, 3)));
        let right = mesh(&world, slot_of(&world, cell(16, 3, 3)));
        // Pool order -Y +Y +Z -Z -X +X: the touching face is missing on each side.
        assert_eq!(left.each_ref().map(Vec::len), [1, 1, 1, 1, 1, 0]);
        assert_eq!(right.each_ref().map(Vec::len), [1, 1, 1, 1, 0, 1]);
    }

    #[test]
    fn a_cell_outside_the_active_box_is_empty_for_culling_occlusion_and_meshing() {
        let mut world = world(16);
        world.write_full(cell(10, 10, 10), RED);
        world.write_full(cell(11, 10, 10), RED);
        world.write_full(cell(11, 11, 10), GREEN);
        let before = mesh(&world, 0);
        // +X of (10, 10, 10) is covered by (11, 10, 10); its +Y face feels (11, 11, 10).
        assert!(before[5].iter().all(|f| f.local() != [10, 10, 10]));
        let top = before[1]
            .iter()
            .find(|f| f.local() == [10, 10, 10])
            .unwrap();
        assert_ne!(top.ao(), [0; 4]);

        // Cut the box at x = 11: the cells at x >= 11 are no longer read.
        let mut active = grid_box(16);
        active.max.x = 11;
        world.set_active_box(active);
        let after = mesh(&world, 0);
        let side = after[5].iter().find(|f| f.local() == [10, 10, 10]).unwrap();
        assert_eq!(side.ao(), [0; 4]);
        let top = after[1].iter().find(|f| f.local() == [10, 10, 10]).unwrap();
        assert_eq!(top.ao(), [0; 4]);
        // The stored voxels outside the box give no faces: one voxel, six faces.
        assert_eq!(after.iter().map(Vec::len).sum::<usize>(), 6);
    }

    #[test]
    fn the_box_edge_between_chunks_hides_the_cells_beyond_it() {
        // The box ends at x = 16, the edge of chunk 0: the voxels of chunk 1 are unreadable and
        // give no faces, and chunk 0 shows its +X face.
        let mut world = world(16);
        world.write_full(cell(15, 5, 5), RED);
        world.write_full(cell(16, 5, 5), RED);
        world.write_unchecked(cell(17, 5, 5), RED);
        let mut active = grid_box(16);
        active.max.x = 16;
        world.set_active_box(active);
        let left = mesh(&world, slot_of(&world, cell(15, 5, 5)));
        let right = mesh(&world, slot_of(&world, cell(16, 5, 5)));
        assert_eq!(left.each_ref().map(Vec::len), [1; 6]);
        assert!(right.iter().all(Vec::is_empty));
    }

    #[test]
    fn meshing_appends_and_ignores_empty_slots() {
        let mut world = world(16);
        world.write_full(cell(1, 1, 1), RED);
        world.write_full(cell(1, 1, 1), 0);
        let mut out = Faces::default();
        for slot in [0, 1, 26, 27, usize::MAX] {
            mesh_chunk(&world, slot, &mut out);
        }
        assert!(
            out.iter().all(Vec::is_empty),
            "erased chunk, unallocated chunks and slots past the table"
        );

        world.write_full(cell(1, 1, 1), RED);
        mesh_chunk(&world, 0, &mut out);
        mesh_chunk(&world, 0, &mut out);
        assert_eq!(out.each_ref().map(Vec::len), [2; 6]);
    }

    #[test]
    fn a_solid_chunk_has_only_its_surface() {
        let mut world = world(16);
        let cells = (0..16)
            .flat_map(|x| (0..16).flat_map(move |y| (0..16).map(move |z| (cell(x, y, z), RED))));
        world.write_many(&mut cells.collect::<Vec<_>>().into_iter(), WriteKind::Full);
        let out = mesh(&world, 0);
        assert_eq!(out.each_ref().map(Vec::len), [256; 6]);
        // A flat face is not occluded, also in the corner of the chunk.
        for pool in &out {
            assert!(pool.iter().all(|f| f.ao() == [0; 4]));
        }
    }

    #[test]
    fn meshing_reads_only_the_chunk_and_its_neighbours_and_changes_nothing() {
        let mut world = world(16);
        for i in [cell(15, 15, 15), cell(16, 16, 16), cell(17, 15, 16)] {
            world.write_full(i, RED);
        }
        // A voxel two chunks away, outside the 27 chunks around chunk (0, 0, 0).
        world.write_full(cell(40, 40, 40), GREEN);
        world.write_full(cell(33, 2, 2), BLUE);
        let dirty = world.is_dirty();
        let (rev, kinds) = (world.rev, world.slots().len());
        let near = mesh(&world, 0);
        assert_eq!(mesh(&world, 0), near, "meshing is deterministic");
        assert_eq!(
            (world.is_dirty(), world.rev, world.slots().len()),
            (dirty, rev, kinds)
        );

        let mut other = world.clone();
        other.write_full(cell(40, 40, 40), 0);
        other.write_full(cell(33, 2, 2), 0);
        assert_eq!(
            mesh(&other, 0),
            near,
            "chunks beyond the neighbours do not matter"
        );
        // The neighbour at (16, 16, 16) does: it hides the corner of (15, 15, 15) diagonally.
        other.write_full(cell(16, 16, 16), 0);
        assert_ne!(mesh(&other, 0), near);
    }

    #[test]
    fn faces_of_a_pool_come_in_z_then_y_then_x_order_of_their_voxels() {
        let mut world = world(16);
        let voxels = [
            cell(5, 1, 0),
            cell(2, 3, 0),
            cell(9, 3, 0),
            cell(0, 0, 4),
            cell(7, 0, 0),
        ];
        for i in voxels {
            world.write_full(i, RED);
        }
        let out = mesh(&world, 0);
        let order: Vec<[u32; 3]> = out[0].iter().map(|f| f.local()).collect();
        assert_eq!(
            order,
            [[7, 0, 0], [5, 1, 0], [2, 3, 0], [9, 3, 0], [0, 0, 4]]
        );
    }

    /// Cells of the cluster (index and value), its offset, the box spans and the AO flag.
    type Cluster = (Vec<(u8, u32)>, i32, [(i32, i32); 3], bool);

    fn cluster() -> impl Strategy<Value = Cluster> {
        let values = prop::sample::select(vec![RED, GREEN, BLUE, 0x8012_3456, 0x0000_0001]);
        (
            prop::collection::vec((0..216_u8, values), 0..260),
            prop::sample::select(vec![13, 27, 2]),
            // Box spans per axis, wide enough to keep most of the cluster.
            [(0..16, 20..34), (0..16, 20..34), (0..16, 20..34)],
            any::<bool>(),
        )
    }

    proptest! {
        // R = 11: 33 cells per axis in three chunks, the last one cut at 32. A cluster of
        // 6 x 6 x 6 cells at an offset that straddles a chunk border or the grid edge, a random box
        // and AO flag; the mesher against the literal reading of the specification.
        #[test]
        fn the_mesher_agrees_with_the_literal_specification_on_random_worlds(
            (cells, base, spans, ao) in cluster(),
        ) {
            let mut world = VoxelWorld::new(11, ao, grid_box(11));
            for (n, value) in cells {
                let n = i32::from(n);
                world.write_full(cell(base + n % 6, base + n / 6 % 6, base + n / 36), value);
            }
            world.set_active_box(IBox3 {
                min: I3::new(spans[0].0, spans[1].0, spans[2].0),
                max: I3::new(spans[0].1, spans[1].1, spans[2].1),
            });
            prop_assert_eq!(mesh_all(&world), reference(&world));
        }
    }

    /// A world of 16 x 16 x 16 chunks (R = 84, 252 cells per axis) filled by `fill`.
    fn dense_world(fill: impl Fn(u32, u32, u32) -> u32) -> VoxelWorld {
        let res = 84;
        let mut slots = Vec::new();
        for cx in 0..16 {
            for cy in 0..16 {
                for cz in 0..16 {
                    let mut chunk = Chunk::default();
                    for z in 0..16 {
                        for y in 0..16 {
                            for x in 0..16 {
                                let value = fill(cx * 16 + x, cy * 16 + y, cz * 16 + z);
                                let local = Chunk::local_index(x as usize, y as usize, z as usize);
                                chunk.set(local, value);
                            }
                        }
                    }
                    slots.push(if chunk.nonzero() == 0 {
                        ChunkSlot::Unallocated
                    } else {
                        ChunkSlot::Data(Arc::new(chunk))
                    });
                }
            }
        }
        let snap = crate::VoxelSnap {
            res,
            slots: Arc::from(slots),
        };
        VoxelWorld::from_snapshot(&snap, true, grid_box(res))
    }

    /// Meshes all chunks of `world` one after the other and in parallel; returns the number of
    /// faces and the best time of five runs of each, in seconds.
    fn time_meshing(world: &VoxelWorld) -> (usize, f64, f64) {
        let mut faces = 0;
        let mut sequential = f64::MAX;
        let mut out = Faces::default();
        for _ in 0..5 {
            let start = Instant::now();
            faces = 0;
            for slot in 0..world.slots().len() {
                out.iter_mut().for_each(Vec::clear);
                mesh_chunk(world, slot, &mut out);
                faces += out.iter().map(Vec::len).sum::<usize>();
            }
            sequential = sequential.min(start.elapsed().as_secs_f64());
        }
        let mut parallel = f64::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let total: usize = (0..world.slots().len())
                .into_par_iter()
                .map_init(Faces::default, |out, slot| {
                    out.iter_mut().for_each(Vec::clear);
                    mesh_chunk(world, slot, out);
                    out.iter().map(Vec::len).sum::<usize>()
                })
                .sum();
            parallel = parallel.min(start.elapsed().as_secs_f64());
            assert_eq!(total, faces);
        }
        (faces, sequential, parallel)
    }

    #[test]
    #[ignore = "timing: run with --release"]
    fn meshing_a_dense_world_of_4096_chunks_is_recorded() {
        let edge = 252;
        let inside = |x: u32, y: u32, z: u32| x < edge && y < edge && z < edge;
        let hash = |x: u32, y: u32, z: u32| {
            let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA6B);
            h ^= z.wrapping_mul(0xC2B2_AE35);
            h ^= h >> 15;
            h.wrapping_mul(0x2C1B_3C6D) >> 7
        };
        let cases = [
            (
                "solid",
                dense_world(|x, y, z| if inside(x, y, z) { RED } else { 0 }),
                6 * 252 * 252,
            ),
            (
                "checkerboard",
                dense_world(|x, y, z| {
                    if inside(x, y, z) && (x + y + z) % 2 == 0 {
                        GREEN
                    } else {
                        0
                    }
                }),
                6 * 252 * 252 * 252 / 2,
            ),
            (
                "noise",
                dense_world(|x, y, z| {
                    if inside(x, y, z) && hash(x, y, z) % 2 == 0 {
                        BLUE
                    } else {
                        0
                    }
                }),
                0,
            ),
        ];
        for (name, world, expected) in cases {
            let data = world
                .slots()
                .iter()
                .filter(|s| matches!(s, ChunkSlot::Data(_)));
            assert_eq!(data.count(), 4096, "{name}");
            let (faces, sequential, parallel) = time_meshing(&world);
            if expected != 0 {
                assert_eq!(faces, expected, "{name}");
            }
            println!(
                "{name}: {faces} faces; 1 thread {:.0} ms ({:.0} us per chunk, {:.1} ns per cell); \
                 {} threads {:.0} ms",
                sequential * 1e3,
                sequential * 1e6 / 4096.0,
                sequential * 1e9 / 252.0_f64.powi(3),
                rayon::current_num_threads(),
                parallel * 1e3,
            );
            if name == "solid" {
                assert!(
                    parallel <= 1.5,
                    "{parallel:.2} s is above the budget of architecture 18"
                );
            }
        }
    }
}
