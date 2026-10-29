#![forbid(unsafe_code)]
//! Layer L0. Embedded own art, fonts, sounds, bundled presets, palettes, data tables and the
//! skin-pack loader.
//!
//! Owns the embedded bytes plus their inventories (picture names, sizes, borders; ui 13, 15).
//! Must not decode anything heavier than PNG headers.

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

pub mod brand;
pub mod fonts;
pub mod mc_versions;
pub mod phosphor;
mod phosphor_map;
