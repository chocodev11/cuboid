#![no_main]

use cuboid_ui::Prefs;
use libfuzzer_sys::fuzz_target;

// The preferences file is read as text, so the input is made valid UTF-8 first.
fuzz_target!(|data: &[u8]| {
    let prefs = Prefs::from_json(&String::from_utf8_lossy(data));
    let _ = prefs.to_json();
});
