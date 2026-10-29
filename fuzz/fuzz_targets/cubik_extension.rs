#![no_main]

use cuboid_fmt_cubik::{Limits, container, load_bytes};
use libfuzzer_sys::fuzz_target;

/// The payload of a model with nothing in it: the empty BSON document.
const EMPTY_PAYLOAD: [u8; 5] = [5, 0, 0, 0, 0];

// The `cuboid.json` entry of a `.cubik` file. The `cubik` target reaches it only through a ZIP
// with a correct checksum, which a fuzzer almost never builds, so the input is the entry and the
// container around it is made here.
fuzz_target!(|data: &[u8]| {
    let Ok(file) = container::write(&EMPTY_PAYLOAD, Some(data)) else {
        return;
    };
    let _ = load_bytes(&file, &Limits::default());
});
