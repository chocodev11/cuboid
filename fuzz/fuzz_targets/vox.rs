#![no_main]

use std::path::Path;

use cuboid_fmt_voxel::vox::{Limits, import_with};
use libfuzzer_sys::fuzz_target;

// The first byte picks the limits: bit 0 switches to tight ones. Run with `-rss_limit_mb=2048`.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, file)) = data.split_first() else {
        return;
    };
    let limits = if mode & 1 == 0 {
        Limits::default()
    } else {
        Limits {
            max_file: 1 << 16,
            max_dim: 64,
            max_voxels: 1 << 12,
            max_nodes: 64,
            max_visits: 256,
            voxel_budget: 1 << 20,
        }
    };
    let _ = import_with(file, Path::new("fuzz/model.vox"), &limits);
});
