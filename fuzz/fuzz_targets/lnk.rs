#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

// The `.lnk` target resolver (interop 28): never a panic, whatever the bytes.
fuzz_target!(|data: &[u8]| {
    let _ = cuboid_io::lnk::target(Path::new("fuzz.lnk"), data);
});
