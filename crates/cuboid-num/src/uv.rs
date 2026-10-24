//! UV rectangle numerics (tools 4.5, native 4.4.3).

use glam::Vec2;

use crate::eval::{Real, eval};

/// Normalises a face UV rotation to `(round(r / 90) * 90 + 360) % 360`. The integer arithmetic
/// wraps like the original's, and values below -405 can stay negative.
///
/// spec: tools 4.5, native 4.4.3, interop 2.3
pub fn uv_rotation(r: i32) -> i32 {
    eval!(uv_rotation_in(r))
}

fn uv_rotation_in<R: Real>(r: i32) -> i32 {
    let quarter_turns = (R::from_i32(r) / R::from_f32(90.0)).round_he().to_i32();
    quarter_turns.wrapping_mul(90).wrapping_add(360) % 360
}

/// Clamps UV corners to the range `[-1, 2]` that the setters of a face UV allow. The legacy
/// `.cub` and Minecraft JSON importers store their corners without this clamp.
///
/// spec: tools 4.5, native 4.4.3
pub fn clamp_uv(corner: Vec2) -> Vec2 {
    corner.clamp(Vec2::splat(-1.0), Vec2::splat(2.0))
}

/// Folds raw (clamped) UV corners back into `[0, 1]` per axis, the value every consumer reads.
/// Returns the folded `(tl, br)`.
///
/// spec: tools 4.5, native 4.4.3, interop 2.3
pub fn fold_uv(tl: Vec2, br: Vec2) -> (Vec2, Vec2) {
    let (x0, x1) = fold_axis(tl.x, br.x);
    let (y0, y1) = fold_axis(tl.y, br.y);
    (Vec2::new(x0, y0), Vec2::new(x1, y1))
}

/// `a` and `b` are the raw first and second corner components of one axis.
fn fold_axis(a: f32, b: f32) -> (f32, f32) {
    let first = if a < 0.0 && b > 0.0 {
        0.0
    } else if a < 0.0 {
        a + 1.0
    } else if a > 1.0 || (a == 1.0 && b > 1.0) {
        a - 1.0
    } else {
        a
    };
    let second = if a < 1.0 && b > 1.0 {
        1.0
    } else if b > 1.0 {
        b - 1.0
    } else if b < 0.0 || (b == 0.0 && a < 0.0) {
        b + 1.0
    } else {
        b
    };
    (first, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_normalised_to_quarter_turns() {
        // `(round(r / 90) * 90 + 360) % 360` with half-to-even rounding.
        let cases = [
            (0, 0),
            (90, 90),
            (180, 180),
            (270, 270),
            (360, 0),
            (450, 90),
            (-90, 270),
            (-180, 180),
            // Ties go to the even quarter turn.
            (45, 0),
            (135, 180),
            (225, 180),
            (315, 0),
            (44, 0),
            (46, 90),
            // tools 4.5: values below -405 can stay negative.
            (-405, 0),
            (-406, -90),
            (-450, -90),
        ];
        for (r, expected) in cases {
            assert_eq!(uv_rotation_in::<f32>(r), expected, "uv_rotation({r})");
            assert_eq!(uv_rotation_in::<f64>(r), expected, "uv_rotation({r}) wide");
            assert_eq!(uv_rotation(r), expected);
        }
    }

    #[test]
    fn rotation_never_panics_on_extreme_input() {
        for r in [
            i32::MIN,
            i32::MIN + 1,
            -1_000_000_007,
            1_000_000_007,
            i32::MAX - 1,
            i32::MAX,
        ] {
            assert!((-360..360).contains(&uv_rotation_in::<f32>(r)));
            assert!((-360..360).contains(&uv_rotation_in::<f64>(r)));
        }
    }

    #[test]
    fn corners_are_clamped_to_minus_one_and_two() {
        assert_eq!(clamp_uv(Vec2::new(-3.0, 5.0)), Vec2::new(-1.0, 2.0));
        assert_eq!(clamp_uv(Vec2::new(-1.0, 2.0)), Vec2::new(-1.0, 2.0));
        assert_eq!(clamp_uv(Vec2::new(0.25, 1.5)), Vec2::new(0.25, 1.5));
    }

    /// One axis: raw `(tl, br)` to folded `(tl, br)`.
    fn fold(a: f32, b: f32) -> (f32, f32) {
        let (tl, br) = fold_uv(Vec2::splat(a), Vec2::splat(b));
        assert_eq!((tl.x, br.x), (tl.y, br.y), "both axes fold alike");
        (tl.x, br.x)
    }

    #[test]
    fn fold_follows_the_read_back_rules() {
        // tools 17 item 7: rectangles in [-1, 0], in [1, 2] and straddling 0 and 1.
        let cases = [
            // (raw first, raw second) -> (first, second)
            ((0.0, 1.0), (0.0, 1.0)),
            ((0.25, 0.75), (0.25, 0.75)),
            ((-0.25, -0.1), (0.75, 0.9)),
            ((-0.5, 0.0), (0.5, 1.0)),
            ((-1.0, -0.5), (0.0, 0.5)),
            ((-0.25, 0.5), (0.0, 0.5)),
            ((1.25, 1.75), (0.25, 0.75)),
            ((1.0, 1.5), (0.0, 0.5)),
            ((1.5, 2.0), (0.5, 1.0)),
            ((0.5, 1.5), (0.5, 1.0)),
            ((1.0, 1.0), (1.0, 1.0)),
            ((0.0, 0.0), (0.0, 0.0)),
            // Mirrored rectangles keep their corner order.
            ((0.75, 0.25), (0.75, 0.25)),
        ];
        for ((a, b), expected) in cases {
            assert_eq!(fold(a, b), expected, "fold({a}, {b})");
        }
    }

    #[test]
    fn fold_axes_are_independent() {
        let (tl, br) = fold_uv(Vec2::new(-0.25, 1.25), Vec2::new(-0.1, 1.75));
        assert_eq!((tl, br), (Vec2::new(0.75, 0.25), Vec2::new(0.9, 0.75)));
    }

    /// native 4.4.3: "for corners within [-1, 2] the written values are always within [0, 1]
    /// and a load-then-save cycle is stable after the first save".
    #[test]
    fn folded_values_stay_in_unit_range_and_are_stable() {
        let steps: Vec<f32> = (-8..=16).map(|i| i as f32 * 0.125).collect();
        for &a in &steps {
            for &b in &steps {
                let (first, second) = fold(a, b);
                assert!((0.0..=1.0).contains(&first), "fold({a}, {b}) first {first}");
                assert!(
                    (0.0..=1.0).contains(&second),
                    "fold({a}, {b}) second {second}"
                );
                assert_eq!(fold(first, second), (first, second), "fold({a}, {b}) twice");
            }
        }
    }
}
