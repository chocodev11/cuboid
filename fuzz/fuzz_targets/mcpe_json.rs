#![no_main]

use libfuzzer_sys::fuzz_target;

// The lenient JSON of the `.mcpe` importer (interop 10.1): never a panic, whatever the bytes.
fuzz_target!(|data: &[u8]| {
    let text = cuboid_io::text::decode_bom(data);
    let _ = cuboid_io::text::tolerant::parse(&text);
});
