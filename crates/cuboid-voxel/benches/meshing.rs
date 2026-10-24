//! Meshing speed of a dense world of 4096 chunks (architecture 18): `cargo bench -p cuboid-voxel`.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use cuboid_voxel::mapping::grid_box;
use cuboid_voxel::{Chunk, ChunkSlot, VoxelFace, VoxelSnap, VoxelWorld, mesh_chunk};
use rayon::prelude::*;

/// R = 84: 252 cells per axis, 16 x 16 x 16 chunks.
const RES: u32 = 84;
const EDGE: u32 = 252;

/// A world whose 4096 chunks all hold cells; `filled` decides per cell.
fn dense_world(filled: impl Fn(u32, u32, u32) -> bool) -> VoxelWorld {
    let mut slots = Vec::new();
    for cx in 0..16 {
        for cy in 0..16 {
            for cz in 0..16 {
                let mut chunk = Chunk::default();
                for z in 0..16 {
                    for y in 0..16 {
                        for x in 0..16 {
                            let (gx, gy, gz) = (cx * 16 + x, cy * 16 + y, cz * 16 + z);
                            if gx < EDGE && gy < EDGE && gz < EDGE && filled(gx, gy, gz) {
                                let local = Chunk::local_index(x as usize, y as usize, z as usize);
                                chunk.set(local, 0xFF80_4020);
                            }
                        }
                    }
                }
                slots.push(ChunkSlot::Data(Arc::new(chunk)));
            }
        }
    }
    let snap = VoxelSnap {
        res: RES,
        slots: Arc::from(slots),
    };
    VoxelWorld::from_snapshot(&snap, true, grid_box(RES))
}

fn mesh_all(world: &VoxelWorld, parallel: bool) -> usize {
    let count = |out: &mut [Vec<VoxelFace>; 6], slot| {
        out.iter_mut().for_each(Vec::clear);
        mesh_chunk(world, slot, out);
        out.iter().map(Vec::len).sum::<usize>()
    };
    let slots = world.slots().len();
    if parallel {
        (0..slots)
            .into_par_iter()
            .map_init(Default::default, |out, slot| count(out, slot))
            .sum()
    } else {
        let mut out = Default::default();
        (0..slots).map(|slot| count(&mut out, slot)).sum()
    }
}

fn meshing(c: &mut Criterion) {
    let hash = |x: u32, y: u32, z: u32| {
        let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA6B);
        h ^= z.wrapping_mul(0xC2B2_AE35);
        h ^= h >> 15;
        h.wrapping_mul(0x2C1B_3C6D) >> 7
    };
    let worlds = [
        ("solid", dense_world(|_, _, _| true)),
        ("noise", dense_world(|x, y, z| hash(x, y, z) % 2 == 0)),
        ("checkerboard", dense_world(|x, y, z| (x + y + z) % 2 == 0)),
    ];
    let mut group = c.benchmark_group("mesh_4096_chunks");
    group.sample_size(10);
    group.throughput(Throughput::Elements(4096));
    for (name, world) in &worlds {
        group.bench_function(format!("{name}_1_thread"), |b| {
            b.iter(|| mesh_all(black_box(world), false));
        });
        group.bench_function(format!("{name}_parallel"), |b| {
            b.iter(|| mesh_all(black_box(world), true));
        });
    }
    group.finish();
}

criterion_group!(benches, meshing);
criterion_main!(benches);
