//! Decode speed on a payload shaped like the voxel data of scene S2 (architecture 18: BSON
//! decode at least 300 MB/s). Timing needs an optimised build:
//! `cargo test -p cuboid-bson --release --test throughput -- --ignored --nocapture`.

mod support;

use std::time::Instant;

use support::payload::{decode_voxels, voxel_payload};

const VOXELS: usize = 4_000_000;
const REQUIRED_MIB_PER_SECOND: f64 = 300.0;

#[test]
#[ignore = "timing: run with --release"]
fn decoding_the_voxels_of_scene_s2_reaches_300_mib_per_second() {
    let bytes = voxel_payload(VOXELS);
    let mib = bytes.len() as f64 / (1 << 20) as f64;
    let mut best = f64::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        let (count, _) = decode_voxels(&bytes);
        best = best.min(start.elapsed().as_secs_f64());
        assert_eq!(count, VOXELS);
    }
    let speed = mib / best;
    println!(
        "decode: {mib:.1} MiB in {:.0} ms = {speed:.0} MiB/s ({:.0} MB/s), {:.1} ns per voxel",
        best * 1000.0,
        speed * 1.048_576,
        best * 1e9 / VOXELS as f64
    );
    assert!(
        speed >= REQUIRED_MIB_PER_SECOND,
        "{speed:.0} MiB/s is below {REQUIRED_MIB_PER_SECOND}"
    );
}
