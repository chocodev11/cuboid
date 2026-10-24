//! Rounding, wrapping, snapping and equality rules of the engine (tools 1.6 and 3.4).

use glam::Vec3;

use crate::eval::{Real, eval};

/// Squared distance below which the engine's vector equality holds: the f32 just under the one
/// nearest to `1e-10`.
const ENGINE_EQ_LIMIT: f32 = 9.999_999_4e-11;

/// Rounds half to even, the rounding of the original's math library. Every "round" of the
/// specifications is this function.
///
/// spec: tools 1.6, interop 3.2
#[inline]
pub fn round_he(x: f32) -> f32 {
    x.round_ties_even()
}

/// Truncates toward zero, the integer cast of the specifications. Out-of-range values saturate
/// and NaN gives 0 (the original's cast is unspecified there).
///
/// spec: tools 1.6, interop 3.2
#[inline]
pub fn trunc_i32(x: f32) -> i32 {
    x as i32
}

/// Wraps a rotation component with `(c + 360) % 360`. The remainder keeps the sign of the
/// dividend, so inputs below -360 stay negative.
///
/// spec: tools 3.4
pub fn wrap360(c: f32) -> f32 {
    eval!(wrap360_in(c))
}

fn wrap360_in<R: Real>(c: f32) -> f32 {
    let turn = R::from_f32(360.0);
    ((R::from_f32(c) + turn) % turn).to_f32()
}

/// Rounds a rotation component, half to even, to the 22.5 degree grid of restricted rotations
/// or to the 5 degree grid. The result can be 360.
///
/// spec: tools 3.4
pub fn snap_angle(c: f32, restricted: bool) -> f32 {
    eval!(snap_angle_in(c, restricted))
}

fn snap_angle_in<R: Real>(c: f32, restricted: bool) -> f32 {
    let step = R::from_f32(if restricted { 22.5 } else { 5.0 });
    ((R::from_f32(c) / step).round_he() * step).to_f32()
}

/// Cleans a value to four decimals: `round(v * 10000) * 0.0001`.
///
/// spec: tools 1.6, interop 2.2.2
pub fn round4(v: f32) -> f32 {
    eval!(round4_in(v))
}

fn round4_in<R: Real>(v: f32) -> f32 {
    ((R::from_f32(v) * R::from_f32(10000.0)).round_he() * R::from_f32(0.0001)).to_f32()
}

/// The engine's vector equality: the squared distance is below 9.99999944e-11. Two vectors
/// 1e-5 apart are not equal.
///
/// spec: interop 2.2.2
pub fn engine_eq(a: Vec3, b: Vec3) -> bool {
    eval!(engine_eq_in(a, b))
}

fn engine_eq_in<R: Real>(a: Vec3, b: Vec3) -> bool {
    let square = |p: f32, q: f32| {
        let d = R::from_f32(p) - R::from_f32(q);
        d * d
    };
    square(a.x, b.x) + square(a.y, b.y) + square(a.z, b.z) < R::from_f32(ENGINE_EQ_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{EVAL, Eval};

    /// Runs `check` once per evaluation width, so that every vector holds under both policies.
    fn both(check: impl Fn(&dyn Fn(f32, bool) -> f32)) {
        check(&|c, restricted| snap_angle_in::<f32>(c, restricted));
        check(&|c, restricted| snap_angle_in::<f64>(c, restricted));
    }

    #[test]
    fn round_he_rounds_ties_to_even() {
        let cases = [
            (0.5, 0.0),
            (1.5, 2.0),
            (2.5, 2.0),
            (3.5, 4.0),
            (-0.5, -0.0),
            (-1.5, -2.0),
            (-2.5, -2.0),
            (22.5 / 5.0, 4.0),
            (0.49999997, 0.0),
            (8_388_609.0, 8_388_609.0),
        ];
        for (x, expected) in cases {
            assert_eq!(round_he(x), expected, "round_he({x})");
        }
    }

    #[test]
    fn trunc_i32_goes_toward_zero_and_saturates() {
        let cases = [
            (1.9, 1),
            (-1.9, -1),
            (-0.9, 0),
            (255.0 * 0.999, 254),
            (3.0e9, i32::MAX),
            (-3.0e9, i32::MIN),
            (f32::INFINITY, i32::MAX),
            (f32::NAN, 0),
        ];
        for (x, expected) in cases {
            assert_eq!(trunc_i32(x), expected, "trunc_i32({x})");
        }
    }

    #[test]
    fn wrap360_keeps_the_sign_of_the_dividend() {
        // tools 3.4: "(c + 360) mod 360 ... inputs below -360 stay negative".
        let cases = [
            (0.0, 0.0),
            (5.0, 5.0),
            (360.0, 0.0),
            (365.0, 5.0),
            (725.0, 5.0),
            (-1.0, 359.0),
            (-360.0, 0.0),
            (-361.0, -1.0),
            (-400.0, -40.0),
            (-22.5, 337.5),
        ];
        for (c, expected) in cases {
            assert_eq!(wrap360(c), expected, "wrap360({c})");
        }
    }

    #[test]
    fn snap_angle_on_the_five_degree_grid() {
        // tools 3.4 and 17 item 24: "22.5 loading as 20, nudges 0 -> 20 -> 40 and 5 -> 30,
        // 358 -> 360" and "every value in [357.5, 360) becomes 360".
        let cases = [
            (0.0, 0.0),
            (22.5, 20.0),
            (42.5, 40.0),
            (27.5, 30.0),
            (67.5, 70.0),
            (2.5, 0.0),
            (7.5, 10.0),
            (12.5, 10.0),
            (357.4, 355.0),
            (357.5, 360.0),
            (358.0, 360.0),
            (359.99, 360.0),
            (360.0, 360.0),
        ];
        both(|snap| {
            for (c, expected) in cases {
                assert_eq!(snap(c, false), expected, "snap_angle({c}, false)");
            }
        });
    }

    #[test]
    fn snap_angle_on_the_restricted_grid() {
        // "on the 22.5 grid every value in [348.75, 360) becomes 360".
        let cases = [
            (0.0, 0.0),
            (11.25, 0.0),
            (11.3, 22.5),
            (22.5, 22.5),
            (33.75, 45.0),
            (56.25, 45.0),
            (67.5, 67.5),
            (292.5, 292.5),
            (315.0, 315.0),
            (337.5, 337.5),
            (348.7, 337.5),
            (348.75, 360.0),
            (359.0, 360.0),
        ];
        both(|snap| {
            for (c, expected) in cases {
                assert_eq!(snap(c, true), expected, "snap_angle({c}, true)");
            }
        });
    }

    #[test]
    fn round4_cleans_to_four_decimals() {
        // The 7-digit text hides the single-precision residue of the multiplication.
        let cases = [
            (1.23456, "1.2346"),
            (-1.23456, "-1.2346"),
            (5.656_800_3, "5.6568"),
            (12.5, "12.5"),
        ];
        for (v, text) in cases {
            assert_eq!(crate::numtext::g7(round4(v)), text, "round4({v})");
        }
        assert_eq!(round4(0.00004), 0.0);
        assert_eq!(round4(-0.00004), 0.0);
        // 0.5 steps round half to even: 0.00005 -> 0, 0.00015 -> 0.0002.
        assert_eq!(round4(0.00005), 0.0);
        assert_eq!(crate::numtext::g7(round4(0.00015)), "0.0002");
    }

    #[test]
    fn engine_eq_threshold() {
        // interop 2.2.2: "zero when its squared length is below 1e-10"; Unity's epsilon is 1e-5.
        let zero = Vec3::ZERO;
        assert!(engine_eq(zero, zero));
        assert!(engine_eq(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(1.0, 2.0, 3.0)
        ));
        assert!(engine_eq(zero, Vec3::new(0.99e-5, 0.0, 0.0)));
        assert!(!engine_eq(zero, Vec3::new(1.0e-5, 0.0, 0.0)));
        assert!(engine_eq(zero, Vec3::splat(0.5e-5)));
        assert!(!engine_eq(zero, Vec3::splat(0.6e-5)));
        assert!(!engine_eq(zero, Vec3::NAN));
    }

    /// Where the policies differ: a product that rounds to a tie only in single precision.
    #[test]
    fn evaluation_width_matters_for_compound_expressions() {
        // 360 + 1e-8 is 360 in f32 but not in f64.
        assert_eq!(wrap360_in::<f32>(1e-8), 0.0);
        assert!((wrap360_in::<f64>(1e-8) - 1e-8).abs() < 1e-13);
        // 0.00175 * 10000 rounds to the tie 17.5 in f32 (-> 18) but is 17.49999937 in f64 (-> 17).
        assert_eq!(round4_in::<f32>(0.0017499999), 0.0018);
        assert_eq!(round4_in::<f64>(0.0017499999), 0.0016999999);
    }

    /// The snap formula (one division, a rounding, one product) is the same under both policies
    /// for every angle next to a tie: the risk R1 of snapping at exact half steps sits in the
    /// wrap before it, not in the snap itself.
    #[test]
    fn snapping_agrees_between_policies_next_to_ties() {
        for restricted in [false, true] {
            let step = if restricted { 22.5f32 } else { 5.0 };
            for k in 0..800u32 {
                let tie = step * (k as f32 + 0.5);
                for offset in 0..4u32 {
                    for c in [
                        f32::from_bits(tie.to_bits() + offset),
                        f32::from_bits(tie.to_bits() - offset),
                    ] {
                        assert_eq!(
                            snap_angle_in::<f32>(c, restricted),
                            snap_angle_in::<f64>(c, restricted),
                            "{c} restricted {restricted}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn public_functions_follow_the_workspace_policy() {
        for c in [-361.0, -1e-8, 1e-8, 0.0017499999, 22.5, 357.5] {
            let (wrap, round, snap) = match EVAL {
                Eval::Strict32 => (
                    wrap360_in::<f32>(c),
                    round4_in::<f32>(c),
                    snap_angle_in::<f32>(c, false),
                ),
                Eval::Wide64 => (
                    wrap360_in::<f64>(c),
                    round4_in::<f64>(c),
                    snap_angle_in::<f64>(c, false),
                ),
            };
            assert_eq!(wrap360(c).to_bits(), wrap.to_bits());
            assert_eq!(round4(c).to_bits(), round.to_bits());
            assert_eq!(snap_angle(c, false).to_bits(), snap.to_bits());
        }
    }
}
