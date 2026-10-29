#![no_main]

use std::path::Path;

use cuboid_fmt_minecraft::{Limits, Refusal, TextureFiles, import_with};
use libfuzzer_sys::fuzz_target;

/// The files of a model folder, all from the input: every `.json` that is asked for is the
/// parent, every `.png` the picture and every `.png.mcmeta` its animation. The parent chain, the
/// texture decoder and the animation reader are reached this way.
struct Files<'a> {
    parent: &'a [u8],
    picture: &'a [u8],
    animation: &'a [u8],
}

impl TextureFiles for Files<'_> {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        let name = path.to_string_lossy();
        let bytes = if name.ends_with(".png.mcmeta") {
            self.animation
        } else if name.ends_with(".png") {
            self.picture
        } else if name.ends_with(".json") {
            self.parent
        } else {
            return Ok(None);
        };
        Ok(Some(bytes.to_vec()))
    }
}

/// Cuts `data` at the share `at` / 255 of its length.
fn cut(data: &[u8], at: u8) -> (&[u8], &[u8]) {
    data.split_at(data.len() * usize::from(at) / 255)
}

// The first byte picks the limits (bit 0 switches to tight ones); the next three cut the rest
// into the model, the parent, the picture and the animation. Run with `-rss_limit_mb=2048`.
fuzz_target!(|data: &[u8]| {
    let [mode, first, second, third, rest @ ..] = data else {
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
    let (model, rest) = cut(rest, *first);
    let (parent, rest) = cut(rest, *second);
    let (picture, animation) = cut(rest, *third);
    let files = Files {
        parent,
        picture,
        animation,
    };
    let _ = import_with(
        model,
        Path::new("assets/fuzz/models/block/model.json"),
        &files,
        &limits,
    );
});
