#![no_main]

use std::path::Path;

use cuboid_fmt_cubik::Limits;
use cuboid_fmt_cubik::cub::{self, Refusal, TextureFiles};
use libfuzzer_sys::fuzz_target;

/// No file exists: every texture is missing.
struct NoFiles;

impl TextureFiles for NoFiles {
    fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        Ok(None)
    }
}

// The first byte picks the input: bit 0 feeds the rest as the file itself, otherwise the rest is
// taken as the plain text and obfuscated first (the key stream is its own inverse), so that the
// fuzzer reaches the JSON dialect and the importer without having to guess the key. Bit 1 picks
// tight limits.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let file = if mode & 1 == 1 {
        rest.to_vec()
    } else {
        cub::deobfuscate(rest).into_bytes()
    };
    let limits = if mode & 2 == 0 {
        Limits::default()
    } else {
        Limits {
            max_text: 1 << 16,
            max_objects: 16,
            max_textures: 8,
            max_texels: 1 << 12,
            max_array: 16,
            voxel_budget: 1 << 20,
            ..Limits::default()
        }
    };
    let _ = cub::import_with(&file, Path::new("fuzz/model.cub"), &NoFiles, &limits);
});
