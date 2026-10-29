#![no_main]

use cuboid_settings::{Environment, Settings};
use libfuzzer_sys::fuzz_target;

// `settings.json` and the legacy `cubikdata.json` go through the same reader. The environment
// is fixed, since only the first-run values depend on it.
fuzz_target!(|data: &[u8]| {
    let env = Environment {
        desktop_dir: "/home/fuzz/Desktop".to_owned(),
        minecraft_dir: "/home/fuzz/.minecraft".to_owned(),
    };
    let _ = Settings::parse(data, &env);
});
