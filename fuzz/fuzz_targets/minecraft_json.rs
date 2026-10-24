#![no_main]

use std::path::Path;

use cuboid_fmt_minecraft::{Limits, Refusal, TextureFiles, import_with};
use libfuzzer_sys::fuzz_target;

/// No file exists: every parent and texture is missing.
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
            max_texels: 1 << 12,
            ..Limits::default()
        }
    };
    let _ = import_with(text, Path::new("fuzz/model.json"), &NoFiles, &limits);
});
