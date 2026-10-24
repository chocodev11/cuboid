#![no_main]

use libfuzzer_sys::fuzz_target;

// The permissive dialect of the Minecraft JSON and `.cub` importers (native 9.4, interop 8.6):
// never a panic, whatever the bytes.
fuzz_target!(|data: &[u8]| {
    let text = cuboid_io::text::decode_bom(data);
    let _ = cuboid_io::text::jsonlite::parse(&text);
});
