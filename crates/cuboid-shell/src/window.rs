//! Window rules: UI scale, client size limits and position restore.
//!
//! All sizes are physical pixels. The scale `S` is computed once at start-up from the display
//! dpi and fixes the canvas: one egui point is `S` physical pixels, one canvas pixel.

use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};

/// A stored window coordinate of this magnitude or more is not restored.
const POSITION_LIMIT: u32 = 30_000;

/// Scale factors are small: the narrowing to f32 loses nothing that matters.
#[expect(
    clippy::cast_possible_truncation,
    reason = "winit reports f64, egui and the contract use f32"
)]
pub(crate) fn to_f32(x: f64) -> f32 {
    x as f32
}

/// The UI scale `S = floor(max(96, dpi) / 96 * 2) / 2`, clamped to `[1, 1.99]`, with the dpi taken
/// as the display scale factor times 96. A user override replaces the derived value; a NaN or
/// infinite override is ignored.
///
/// spec: ui 1.2, app 4.3, arch 14.1
pub fn ui_scale(scale_factor: f64, override_scale: Option<f32>) -> f32 {
    let dpi = scale_factor * 96.0;
    let derived = to_f32((dpi.max(96.0) / 96.0 * 2.0).floor() / 2.0);
    override_scale
        .filter(|scale| scale.is_finite())
        .unwrap_or(derived)
        .clamp(1.0, 1.99)
}

/// The egui zoom factor that makes `pixels_per_point = zoom * scale_factor` equal to `ui_scale`.
pub(crate) fn zoom_factor(ui_scale: f32, scale_factor: f64) -> f32 {
    to_f32(f64::from(ui_scale) / scale_factor)
}

/// The minimum client size: `min_size` at scale 1 times the UI scale.
///
/// spec: app 4.2, ui 1.3
pub fn min_client_size(min_size: (u32, u32), ui_scale: f32) -> PhysicalSize<u32> {
    LogicalSize::new(f64::from(min_size.0), f64::from(min_size.1)).to_physical(f64::from(ui_scale))
}

/// The client size of the first launch: `size`, raised to the minimum where the scale makes the
/// minimum larger.
///
/// spec: app 1.1, app 4.2, ui 1.3
pub fn initial_client_size(
    size: (u32, u32),
    min_size: (u32, u32),
    ui_scale: f32,
) -> PhysicalSize<u32> {
    let min = min_client_size(min_size, ui_scale);
    PhysicalSize::new(size.0.max(min.width), size.1.max(min.height))
}

/// The stored window position, or `None` when it must not be restored: a coordinate of 0 means
/// the position was never stored, and a magnitude of 30 000 or more is off any screen.
///
/// spec: app 1.2 (step 5), app 4.2
pub fn restorable_position((x, y): (i32, i32)) -> Option<PhysicalPosition<i32>> {
    let usable = |v: i32| v != 0 && v.unsigned_abs() < POSITION_LIMIT;
    (usable(x) && usable(y)).then_some(PhysicalPosition::new(x, y))
}

/// The window position the application reads: the outer position, or `None` while the window is
/// minimised (a minimised Windows window sits at -32000) and where the platform has no outer
/// position (Wayland).
///
/// spec: app 4.2, app 10.1
pub(crate) fn reported_position(
    outer: Option<PhysicalPosition<i32>>,
    minimised: bool,
) -> Option<(i32, i32)> {
    outer
        .filter(|_| !minimised)
        .map(|position| (position.x, position.y))
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "the scales of the specification are exact values"
)]
mod tests {
    use super::*;

    #[test]
    fn scale_follows_the_dpi_steps_of_ui_1_2() {
        // 96 dpi is scale factor 1, 120 dpi 1.25, 144 dpi 1.5, 168 dpi 1.75, 192 dpi 2.
        for (factor, expected) in [
            (1.0, 1.0),
            (1.25, 1.0),
            (1.5, 1.5),
            (1.75, 1.5),
            (2.0, 1.99),
        ] {
            assert_eq!(ui_scale(factor, None), expected, "scale factor {factor}");
        }
    }

    #[test]
    fn scale_is_clamped_below_96_dpi_and_above_2() {
        assert_eq!(ui_scale(0.75, None), 1.0);
        assert_eq!(ui_scale(3.0, None), 1.99);
    }

    #[test]
    fn an_override_replaces_the_derived_scale_within_the_clamp() {
        assert_eq!(ui_scale(2.0, Some(1.5)), 1.5);
        assert_eq!(ui_scale(1.0, Some(4.0)), 1.99);
        assert_eq!(ui_scale(1.0, Some(0.5)), 1.0);
        assert_eq!(ui_scale(1.5, Some(f32::NAN)), 1.5);
        assert_eq!(ui_scale(1.0, Some(f32::INFINITY)), 1.0);
    }

    #[test]
    fn zoom_makes_a_point_as_large_as_the_scale() {
        let zoom = zoom_factor(1.5, 1.25);
        assert!((f64::from(zoom) * 1.25 - 1.5).abs() < 1e-6);
        assert_eq!(zoom_factor(1.0, 1.0), 1.0);
    }

    #[test]
    fn the_first_window_is_1000_by_700_at_scale_1() {
        let opts = crate::ShellOptions::default();
        let size = initial_client_size(opts.size, opts.min_size, 1.0);
        assert_eq!((size.width, size.height), (1000, 700));
    }

    #[test]
    fn the_minimum_is_900_by_700_times_the_scale() {
        for (scale, expected) in [(1.0, (900, 700)), (1.5, (1350, 1050)), (1.99, (1791, 1393))] {
            let min = min_client_size((900, 700), scale);
            assert_eq!((min.width, min.height), expected, "scale {scale}");
        }
    }

    #[test]
    fn the_first_window_is_raised_to_the_scaled_minimum() {
        let size = initial_client_size((1000, 700), (900, 700), 1.5);
        assert_eq!((size.width, size.height), (1350, 1050));
        // A larger stored size stays.
        let size = initial_client_size((1600, 900), (900, 700), 1.0);
        assert_eq!((size.width, size.height), (1600, 900));
    }

    #[test]
    fn a_stored_position_is_restored_unless_zero_or_far_away() {
        assert_eq!(
            restorable_position((120, -40)),
            Some(PhysicalPosition::new(120, -40))
        );
        assert_eq!(
            restorable_position((29_999, 29_999)),
            Some(PhysicalPosition::new(29_999, 29_999))
        );
        assert_eq!(restorable_position((0, 120)), None);
        assert_eq!(restorable_position((120, 0)), None);
        assert_eq!(restorable_position((30_000, 120)), None);
        assert_eq!(restorable_position((120, -30_000)), None);
        assert_eq!(restorable_position((i32::MIN, 1)), None);
    }

    #[test]
    fn the_reported_position_is_the_outer_position_with_a_fallback_of_none() {
        let at = Some(PhysicalPosition::new(120, -40));
        assert_eq!(reported_position(at, false), Some((120, -40)));
        // Minimised, or a platform without an outer position.
        assert_eq!(reported_position(at, true), None);
        assert_eq!(reported_position(None, false), None);
    }
}
