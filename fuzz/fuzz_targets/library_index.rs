#![no_main]

use cuboid_library::parse_index;
use libfuzzer_sys::fuzz_target;

// The `library.json` index of a library; what it holds is then summarised the way the start
// screen and the agent brief do.
fuzz_target!(|data: &[u8]| {
    let Ok(library) = parse_index(data) else {
        return;
    };
    let _ = library.view(8);
    let _ = library.tags();
    let _ = library.brief_json();
    let _ = library.brief_markdown();
});
