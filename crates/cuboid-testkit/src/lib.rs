#![forbid(unsafe_code)]
//! Dev-only crate (layer L6, used through `dev-dependencies` only). Golden harness, parity diff
//! tiers, canonical dumps, scenario DSL and oracle gating (architecture 19).
//!
//! The kit depends on no other workspace crate. Crates of every layer can therefore use it in
//! their unit tests: a dependency back on them would compile the crate under test twice, and its
//! types would differ from the copy the kit sees.
//!
//! | Tier | Helper |
//! |---|---|
//! | P0 bit-exact | [`diff::bytes`], [`diff::f32_bits`], [`diff::text`] |
//! | P1 geometric | [`diff::numbers`], [`diff::set`], [`diff::sequence`] |
//! | P2 raster | [`raster::within`] |
//! | P3 perceptual | [`raster::ssim`] |
//! | P4 behavioural | [`diff::sequence`] over the traces of [`scenario`], [`diff::dumps`] |
//!
//! The exceptions to P0 compare [`diff::numbers`] with [`diff::Tolerance::ANGLE`] (Euler angles)
//! or [`diff::Tolerance::RELATIVE`] (positions after a pivot rotation, quaternion components).
//!
//! [`golden::Golden`] binds the tiers to files of `tests/golden` and blesses them with
//! `CUBOID_BLESS=1` (`cargo xtask bless`). Tests that need an oracle of architecture 19.1 gate
//! themselves with [`oracle::require`] and print a skip notice when it is absent.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

/// Makes `Debug` print the `Display` text, so `unwrap()` and tests returning `Result` show a
/// readable multi-line explanation instead of an escaped string.
macro_rules! debug_is_display {
    ($($ty:ty),+ $(,)?) => {$(
        impl ::std::fmt::Debug for $ty {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(self, f)
            }
        }
    )+};
}

pub mod diff;
pub mod dump;
pub mod golden;
pub mod oracle;
pub mod raster;
pub mod scenario;

use std::io::Write;
use std::path::Path;

/// The workspace root, derived from the manifest folder `<root>/crates/cuboid-testkit`.
pub fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the kit lives two folders below the workspace root")
}

/// A line on the real stderr: unlike `eprintln!` it is not captured for passing tests, so the
/// notices of skipped oracle tests and blessed goldens stay visible.
pub(crate) fn notice(message: &str) {
    // A closed stderr leaves nobody to tell.
    let _ = writeln!(std::io::stderr(), "{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_holds_the_workspace_manifest_and_the_corpus() {
        let root = workspace_root();
        assert!(root.join("Cargo.toml").is_file());
        assert!(golden::root().is_dir());
    }
}
