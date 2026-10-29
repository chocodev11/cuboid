#![no_main]

use cuboid_library::kind::classify;
use cuboid_library::meta::{lore_meta, model_meta, texture_meta};
use libfuzzer_sys::fuzz_target;

/// Names that pick the branch of the readers: the extension decides what a file is.
const MODEL_NAMES: [&str; 5] = ["a.cubik", "a.cub", "a.bbmodel", "a.json", "a.vox"];
const CLASS_NAMES: [&str; 8] = [
    "a.png",
    "a.cubik",
    "a.cub",
    "a.bbmodel",
    "a.json",
    "a.mcmeta",
    "a.md",
    "a.obj",
];

// The first byte picks the reader and, for a texture, the second splits the rest into the PNG
// and its `.mcmeta`. Run with `-rss_limit_mb=2048`.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let pick = usize::from(mode >> 2);
    match mode & 3 {
        0 => {
            let _ = classify(CLASS_NAMES[pick % CLASS_NAMES.len()], rest);
        }
        1 => {
            let _ = model_meta(MODEL_NAMES[pick % MODEL_NAMES.len()], rest);
        }
        2 => {
            let Some((&split, rest)) = rest.split_first() else {
                return;
            };
            let (png, mcmeta) = rest.split_at(rest.len() * usize::from(split) / 255);
            let _ = texture_meta(png, Some(mcmeta));
            let _ = texture_meta(rest, None);
        }
        _ => {
            let _ = lore_meta(rest);
        }
    }
});
