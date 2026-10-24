#![forbid(unsafe_code)]
// Exact float comparison and float-to-int casts are specified behaviour here (architecture 5).
#![allow(clippy::float_cmp, clippy::cast_possible_truncation)]
//! Layer L0. Numerics: rounding, snapping, Euler and quaternion conversion, colour packing,
//! G7/X9/NJ number text, invariant-culture parsing and `EngineRng`.
//!
//! Owns every parity-sensitive formula of tools 1.6, 3.4, 4.5, 7.3, native 4.3.2, 4.10 and
//! interop 3.1-3.2, plus the evaluation policy. Must not depend on anything but glam.
//!
//! Stored values are f32 and every formula follows the operation order of the specification,
//! without fused multiply-add or reassociation. Formulas that combine several float operations
//! are generic over the evaluation width and dispatch on [`EVAL`]; the Euler and quaternion
//! conversions are transcendental paths that compute in f64 and are compared with a tolerance.

mod eval;
mod rng;
mod rotation;
mod scalar;
mod uv;

pub mod numtext;
pub mod pack;

pub use eval::{EVAL, Eval};
pub use rng::EngineRng;
pub use rotation::{euler_to_quat, quat_to_euler};
pub use scalar::{engine_eq, round_he, round4, snap_angle, trunc_i32, wrap360};
pub use uv::{clamp_uv, fold_uv, uv_rotation};
