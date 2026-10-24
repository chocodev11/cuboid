//! Euler angle and quaternion conversion with the engine's conventions (native 4.3.2).
//!
//! Both directions are transcendental paths: they compute in f64, round once to f32, and the
//! specification compares them with a tolerance of 1e-4 degrees, not bit for bit. The
//! evaluation policy does not apply.

use glam::{Quat, Vec3};

/// `-0.0001` rad in degrees: components below this get 360 added.
const FLIP_LOW: f32 = -0.005_729_578;
/// Components above this get 360 subtracted.
const FLIP_HIGH: f32 = 360.0 + FLIP_LOW;
/// `|-sin(x)|` from which the decomposition treats `x` as +-90 degrees. A quaternion rounded
/// to f32 has an angular error of about 1e-7, which makes `y` and `z` individually meaningless
/// that close to the lock; only their sum or difference is determined.
const GIMBAL_LOCK: f64 = 1.0 - 1e-8;

/// Builds the rotation of `Euler(x, y, z)` in degrees: rotate about Z, then X, then Y in a
/// left-handed, Y-up frame, as the quaternion product `qY * qX * qZ`.
///
/// spec: native 4.3.2, interop 2.1
pub fn euler_to_quat(deg: Vec3) -> Quat {
    let (sx, cx) = (f64::from(deg.x).to_radians() * 0.5).sin_cos();
    let (sy, cy) = (f64::from(deg.y).to_radians() * 0.5).sin_cos();
    let (sz, cz) = (f64::from(deg.z).to_radians() * 0.5).sin_cos();
    Quat::from_xyzw(
        (cy * sx * cz + sy * cx * sz) as f32,
        (sy * cx * cz - cy * sx * sz) as f32,
        (cy * cx * sz - sy * sx * cz) as f32,
        (cy * cx * cz + sy * sx * sz) as f32,
    )
}

/// Decomposes a rotation into the Euler angles in degrees that `euler_to_quat` inverts, made
/// positive per component: a component below -0.0057295779 gets 360 added and one above
/// 359.99427 gets 360 subtracted. At `x = +-90` the decomposition is not unique; it returns
/// `z = 0` and puts the combined angle into `y`. A zero quaternion gives NaN.
///
/// spec: native 4.3.2
pub fn quat_to_euler(q: Quat) -> Vec3 {
    let (x, y, z, w) = (
        f64::from(q.x),
        f64::from(q.y),
        f64::from(q.z),
        f64::from(q.w),
    );
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);

    // Entries of the rotation matrix R = Ry * Rx * Rz that the decomposition reads.
    let m00 = 1.0 - 2.0 * (y * y + z * z);
    let m01 = 2.0 * (x * y - w * z);
    let m02 = 2.0 * (x * z + w * y);
    let m10 = 2.0 * (x * y + w * z);
    let m11 = 1.0 - 2.0 * (x * x + z * z);
    let m12 = 2.0 * (y * z - w * x);
    let m22 = 1.0 - 2.0 * (x * x + y * y);

    let half_turn = std::f64::consts::FRAC_PI_2;
    let (ex, ey, ez) = if m12 <= -GIMBAL_LOCK {
        (half_turn, m01.atan2(m00), 0.0)
    } else if m12 >= GIMBAL_LOCK {
        (-half_turn, (-m01).atan2(m00), 0.0)
    } else {
        ((-m12).asin(), m02.atan2(m22), m10.atan2(m11))
    };
    Vec3::new(
        make_positive(ex.to_degrees() as f32),
        make_positive(ey.to_degrees() as f32),
        make_positive(ez.to_degrees() as f32),
    )
}

fn make_positive(deg: f32) -> f32 {
    let wrapped = if deg < FLIP_LOW {
        deg + 360.0
    } else if deg > FLIP_HIGH {
        deg - 360.0
    } else {
        deg
    };
    // Adding zero turns -0.0 into 0.0, which the stored files should not contain.
    wrapped + 0.0
}

#[cfg(test)]
mod tests {
    use glam::Mat3;
    use proptest::prelude::*;

    use super::*;

    /// Distance between two angles on the circle, in degrees.
    fn gap(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    }

    #[track_caller]
    fn assert_euler(actual: Vec3, expected: Vec3) {
        // native 4.3.2: tests compare with a tolerance of 1e-4 degrees.
        for (a, e) in [
            (actual.x, expected.x),
            (actual.y, expected.y),
            (actual.z, expected.z),
        ] {
            assert!(gap(a, e) < 1e-4, "{actual:?} differs from {expected:?}");
        }
    }

    fn same_rotation(a: Quat, b: Quat) -> bool {
        a.dot(b).abs() > 1.0 - 1e-6
    }

    #[test]
    fn single_axis_rotations() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let cases = [
            (Vec3::new(90.0, 0.0, 0.0), Quat::from_xyzw(h, 0.0, 0.0, h)),
            (Vec3::new(0.0, 90.0, 0.0), Quat::from_xyzw(0.0, h, 0.0, h)),
            (Vec3::new(0.0, 0.0, 90.0), Quat::from_xyzw(0.0, 0.0, h, h)),
            (Vec3::new(270.0, 0.0, 0.0), Quat::from_xyzw(h, 0.0, 0.0, -h)),
            (Vec3::ZERO, Quat::IDENTITY),
        ];
        for (euler, expected) in cases {
            let q = euler_to_quat(euler);
            assert!(q.abs_diff_eq(expected, 1e-7), "{euler:?}: {q:?}");
        }
    }

    /// app 0: "Euler(x,y,z) rotation that applies Z first, then X, then Y; rotating +Z about +X by
    /// +90 degrees yields -Y", and interop 2.1: `R = Ry(b) * Rx(a) * Rz(c)`.
    #[test]
    fn rotates_z_then_x_then_y() {
        let q = euler_to_quat(Vec3::new(90.0, 0.0, 0.0));
        assert!((q * Vec3::Z).abs_diff_eq(-Vec3::Y, 1e-6));

        let triples = [
            Vec3::new(30.0, 60.0, 10.0),
            Vec3::new(45.0, 45.0, 0.0),
            Vec3::new(10.0, 200.0, 300.0),
            Vec3::new(0.0, 0.0, 90.0),
            Vec3::new(90.0, 0.0, 90.0),
        ];
        let v = Vec3::new(1.0, 2.0, 3.0);
        for e in triples {
            let r = Mat3::from_rotation_y(e.y.to_radians())
                * Mat3::from_rotation_x(e.x.to_radians())
                * Mat3::from_rotation_z(e.z.to_radians());
            let got = euler_to_quat(e) * v;
            assert!(
                got.abs_diff_eq(r * v, 1e-5),
                "{e:?}: {got:?} vs {:?}",
                r * v
            );
        }
        // Z first: a quarter turn about Z maps +X to +Y, which the X turn then sends to +Z.
        let q = euler_to_quat(Vec3::new(90.0, 0.0, 90.0));
        assert!((q * Vec3::X).abs_diff_eq(Vec3::Z, 1e-6));
    }

    #[test]
    fn camera_defaults_round_trip() {
        // app 5.1: default poses of the four views, plus the second anchors.
        let poses = [
            Vec3::new(90.0, 0.0, 0.0),
            Vec3::new(270.0, 0.0, 0.0),
            Vec3::new(0.0, 90.0, 0.0),
            Vec3::new(0.0, 270.0, 0.0),
            Vec3::new(45.0, 45.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
        ];
        for e in poses {
            assert_euler(quat_to_euler(euler_to_quat(e)), e);
        }
    }

    #[test]
    fn round_trips_away_from_the_poles() {
        // The f32 quaternion limits the accuracy near x = +-90, so the grid stops at 85 degrees.
        let xs = [
            0.0, 15.0, 30.0, 45.0, 60.0, 75.0, 85.0, 275.0, 285.0, 300.0, 315.0, 330.0, 345.0,
        ];
        let others = [
            0.0, 5.0, 22.5, 67.5, 90.0, 135.0, 180.0, 225.0, 292.5, 315.0, 355.0,
        ];
        for x in xs {
            for y in others {
                for z in others {
                    let e = Vec3::new(x, y, z);
                    assert_euler(quat_to_euler(euler_to_quat(e)), e);
                }
            }
        }
    }

    /// At x = +-90 the triple is not unique: z becomes 0 and y holds the combined angle.
    #[test]
    fn gimbal_lock_keeps_the_rotation() {
        let cases = [
            (Vec3::new(90.0, 30.0, 0.0), Vec3::new(90.0, 30.0, 0.0)),
            (Vec3::new(90.0, 30.0, 20.0), Vec3::new(90.0, 10.0, 0.0)),
            (Vec3::new(90.0, 0.0, 20.0), Vec3::new(90.0, 340.0, 0.0)),
            (Vec3::new(270.0, 30.0, 20.0), Vec3::new(270.0, 50.0, 0.0)),
            (Vec3::new(270.0, 0.0, 20.0), Vec3::new(270.0, 20.0, 0.0)),
            (Vec3::new(90.0, 0.0, 0.0), Vec3::new(90.0, 0.0, 0.0)),
            (Vec3::new(270.0, 0.0, 0.0), Vec3::new(270.0, 0.0, 0.0)),
        ];
        for (input, expected) in cases {
            let q = euler_to_quat(input);
            let e = quat_to_euler(q);
            assert_euler(e, expected);
            assert!(same_rotation(euler_to_quat(e), q), "{input:?} -> {e:?}");
        }
    }

    #[test]
    fn components_are_made_positive_below_the_flip_threshold() {
        // -0.0001 rad is -0.0057295779 degrees: smaller values wrap, larger ones stay.
        assert_eq!(make_positive(-0.0058), 359.9942);
        assert_eq!(make_positive(-0.0057), -0.0057);
        assert_eq!(make_positive(-90.0), 270.0);
        assert_eq!(make_positive(180.0), 180.0);
        // Above 359.99427, 360 is subtracted.
        assert!((make_positive(359.9999) + 0.0001).abs() < 1e-4);
        assert_eq!(make_positive(359.99), 359.99);

        let tiny = quat_to_euler(euler_to_quat(Vec3::new(0.0, -0.001, 0.0)));
        assert!((tiny.y + 0.001).abs() < 1e-5, "{tiny:?}");
        let wrapped = quat_to_euler(euler_to_quat(Vec3::new(0.0, -0.01, 0.0)));
        assert!((wrapped.y - 359.99).abs() < 1e-4, "{wrapped:?}");
    }

    #[test]
    fn never_returns_negative_zero() {
        for e in [
            Vec3::ZERO,
            Vec3::new(270.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 180.0),
        ] {
            let out = quat_to_euler(euler_to_quat(e));
            for c in [out.x, out.y, out.z] {
                assert!(c.is_sign_positive(), "{e:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn decomposition_does_not_need_a_unit_quaternion() {
        let q = euler_to_quat(Vec3::new(30.0, 60.0, 10.0));
        assert_euler(quat_to_euler(q * 3.0), Vec3::new(30.0, 60.0, 10.0));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(5000))]

        /// Any triple with |x| <= 85 survives the f32 quaternion within 1e-4 degrees, and the
        /// decomposition agrees with an independent one (glam's YXZ order).
        #[test]
        fn random_triples_round_trip(x in -85.0f32..85.0, y in -180.0f32..180.0, z in -180.0f32..180.0) {
            let e = Vec3::new(x, y, z);
            let q = euler_to_quat(e);
            let got = quat_to_euler(q);
            for (a, b) in [(got.x, x), (got.y, y), (got.z, z)] {
                prop_assert!(gap(a, b) < 1e-4, "{e:?} -> {got:?}");
            }
            let (gy, gx, gz) = q.to_euler(glam::EulerRot::YXZ);
            for (a, b) in [(got.x, gx), (got.y, gy), (got.z, gz)] {
                prop_assert!(gap(a, b.to_degrees()) < 2e-3, "{e:?}: {got:?} vs glam");
            }
        }
    }
}
