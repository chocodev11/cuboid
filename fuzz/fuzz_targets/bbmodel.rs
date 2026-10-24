#![no_main]

use std::path::Path;

use cuboid_fmt_bbmodel::{Limits, TextureFiles, import_with};
use cuboid_io::texture_files::Refusal;
use libfuzzer_sys::fuzz_target;

/// No file exists: every texture is missing.
struct NoFiles;

impl TextureFiles for NoFiles {
    fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        Ok(None)
    }
}

// The first byte picks the limits: bit 0 switches to tight ones. Run with `-rss_limit_mb=2048`.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, text)) = data.split_first() else {
        return;
    };
    let limits = if mode & 1 == 0 {
        Limits::default()
    } else {
        Limits {
            max_text: 1 << 16,
            max_values: 1 << 10,
            max_objects: 16,
            max_textures: 8,
            max_texels: 1 << 12,
            max_mesh_items: 1 << 10,
            max_depth: 8,
            max_label_bytes: 1 << 10,
        }
    };
    let _ = import_with(text, Path::new("fuzz/model.bbmodel"), &NoFiles, &limits);
});
