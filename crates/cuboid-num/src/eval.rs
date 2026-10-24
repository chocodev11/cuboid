//! Evaluation policy of the parity formulas (architecture 7, risk R1).

use std::ops::{Add, Div, Mul, Rem, Sub};

/// Width in which a parity formula evaluates its intermediate results.
///
/// The specifications state single-precision arithmetic. The original's runtime may evaluate
/// single-precision expressions with double intermediates and round only when a value is stored
/// (**UNVERIFIED**). The two readings differ only at rounding ties, for example when a snapped
/// angle sits on an exact half step. Every formula that combines several float operations is
/// written once, generic over the width, and dispatched on [`EVAL`].
///
/// Single operations and the transcendental paths (`euler_to_quat`, `quat_to_euler`, which
/// always compute in f64 and are specified with a tolerance) do not depend on the policy.
///
/// spec: architecture 7
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eval {
    /// Every operation rounds to f32, as the specifications state.
    Strict32,
    /// Intermediate results stay in f64 and round to f32 where a value is stored.
    Wide64,
}

/// The evaluation policy of the whole workspace; switching it touches only this constant.
pub const EVAL: Eval = Eval::Strict32;

/// Arithmetic of one evaluation width. Literals of the specifications are f32 values widened.
pub(crate) trait Real:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Rem<Output = Self>
{
    fn from_f32(x: f32) -> Self;
    fn from_i32(x: i32) -> Self;
    /// The store back into an f32 value.
    fn to_f32(self) -> f32;
    /// Saturating, NaN gives 0, like `trunc_i32`.
    fn to_i32(self) -> i32;
    fn round_he(self) -> Self;
    fn trunc(self) -> Self;
}

impl Real for f32 {
    #[inline]
    fn from_f32(x: f32) -> Self {
        x
    }
    #[inline]
    fn from_i32(x: i32) -> Self {
        x as f32
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self
    }
    #[inline]
    fn to_i32(self) -> i32 {
        self as i32
    }
    #[inline]
    fn round_he(self) -> Self {
        self.round_ties_even()
    }
    #[inline]
    fn trunc(self) -> Self {
        f32::trunc(self)
    }
}

impl Real for f64 {
    #[inline]
    fn from_f32(x: f32) -> Self {
        f64::from(x)
    }
    #[inline]
    fn from_i32(x: i32) -> Self {
        f64::from(x)
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    fn to_i32(self) -> i32 {
        self as i32
    }
    #[inline]
    fn round_he(self) -> Self {
        self.round_ties_even()
    }
    #[inline]
    fn trunc(self) -> Self {
        f64::trunc(self)
    }
}

/// Calls the generic formula `$f::<R>(args)` with the width chosen by [`EVAL`].
macro_rules! eval {
    ($f:ident($($arg:expr),* $(,)?)) => {
        match $crate::eval::EVAL {
            $crate::eval::Eval::Strict32 => $f::<f32>($($arg),*),
            $crate::eval::Eval::Wide64 => $f::<f64>($($arg),*),
        }
    };
}
pub(crate) use eval;
