#![no_main]

use cuboid_bson::Limits;
use libfuzzer_sys::fuzz_target;

// Shared with the robustness tests of `cuboid-bson`.
#[path = "../../crates/cuboid-bson/tests/support/walk.rs"]
mod walk;

const TIGHT: Limits = Limits {
    max_doc: 4096,
    max_depth: 4,
    max_array: 16,
};

// The first byte picks the input: bit 0 puts a valid document header around the rest, so that
// the fuzzer reaches the element reader without first having to guess a size field.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let wrapped;
    let bytes = if mode & 1 == 0 {
        rest
    } else {
        wrapped = walk::wrap_body(rest);
        &wrapped
    };
    for limits in [Limits::DEFAULT, TIGHT] {
        walk::read_everything(bytes, &limits);
    }
});
