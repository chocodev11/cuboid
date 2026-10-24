//! A voxel payload shaped like native 4.7, for the timing test and the benchmark.

use cuboid_bson::{DocWriter, Limits, RawDoc};

/// A chunk holds 16 x 16 x 16 cells.
const CELLS: usize = 4096;

/// A root document with one world of full chunks (the last one partial) of `voxels` records
/// of 33 bytes in total, in the scan order of native 4.7.
pub fn voxel_payload(voxels: usize) -> Vec<u8> {
    let mut w = DocWriter::with_capacity(voxels * 40 + 1024);
    let root = w.begin_root();
    w.i32(b"v\0", 1);
    let vx = w.begin_doc(b"vx\0");
    let worlds = w.begin_array(b"w\0");
    let world = w.begin_doc(0usize);
    w.i32(b"r\0", 128);
    let chunks = w.begin_array(b"c\0");
    let mut colour = 0xFF12_3456_u32;
    for (index, first) in (0..voxels).step_by(CELLS).enumerate() {
        let chunk = w.begin_doc(index);
        w.i32(b"cx\0", (index / 576) as i32);
        w.i32(b"cy\0", (index / 24 % 24) as i32);
        w.i32(b"cz\0", (index % 24) as i32);
        let list = w.begin_array(b"v\0");
        for cell in 0..CELLS.min(voxels - first) {
            colour = colour.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) | 0xFF00_0000;
            let voxel = w.begin_doc(cell);
            w.i32(b"x\0", (cell & 15) as i32);
            w.i32(b"y\0", (cell >> 4 & 15) as i32);
            w.i32(b"z\0", (cell >> 8) as i32);
            w.i32(b"c\0", colour.cast_signed());
            w.end(voxel);
        }
        w.end(list);
        w.end(chunk);
    }
    w.end(chunks);
    w.end(world);
    w.end(worlds);
    w.end(vx);
    w.end(root);
    w.finish().expect("below 2 GiB")
}

/// What a record decoder does with the payload: finds the members by key, reads every voxel
/// with the typed getters and checks nothing else. Returns the number of voxels and a checksum.
pub fn decode_voxels(bytes: &[u8]) -> (usize, i64) {
    let root = RawDoc::parse(bytes, &Limits::DEFAULT).unwrap();
    let vx = root.last(b"vx").unwrap().unwrap().doc().unwrap().unwrap();
    let worlds = vx.last(b"w").unwrap().unwrap().array().unwrap().unwrap();
    let (mut count, mut sum) = (0, 0_i64);
    for world in worlds.iter() {
        let world = world.unwrap().doc().unwrap().unwrap();
        let chunks = world.last(b"c").unwrap().unwrap().array().unwrap().unwrap();
        for chunk in chunks.iter() {
            let chunk = chunk.unwrap().doc().unwrap().unwrap();
            for member in chunk.iter() {
                let member = member.unwrap();
                if member.key != b"v" {
                    sum += i64::from(member.i32().unwrap());
                    continue;
                }
                for voxel in member.array().unwrap().unwrap().iter() {
                    let voxel = voxel.unwrap().doc().unwrap().unwrap();
                    let mut cell = [0_i32; 4];
                    for field in voxel.iter() {
                        let field = field.unwrap();
                        let slot = match field.key {
                            b"x" => 0,
                            b"y" => 1,
                            b"z" => 2,
                            b"c" => 3,
                            _ => continue,
                        };
                        cell[slot] = field.i32().unwrap();
                    }
                    sum += cell.iter().map(|&n| i64::from(n)).sum::<i64>();
                    count += 1;
                }
            }
        }
    }
    (count, sum)
}
