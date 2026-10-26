//! Window rules: UI scale, client size limits and position restore.
//!
//! All sizes are physical pixels. The scale `S` is computed once at start-up from the display
//! dpi and fixes the canvas: one egui point is `S` physical pixels, one canvas pixel.

use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::window::ResizeDirection;

use crate::CursorKind;

/// A stored window coordinate of this magnitude or more is not restored.
const POSITION_LIMIT: u32 = 30_000;

/// Depth in points of the zone along each window edge where a press starts a resize.
///
/// spec: title bar 2
pub(crate) const EDGE_BAND: f64 = 5.0;

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

/// The resize a press at `pos` would start in a window of `size`, all in physical pixels. The
/// edge zones are `band` deep; a corner zone reaches twice as far along the edges. A band of 0
/// (a maximized or full-screen window, or one with the system frame) finds no edge.
///
/// spec: title bar 2, title bar 3
pub(crate) fn resize_edge(
    pos: PhysicalPosition<f64>,
    size: PhysicalSize<u32>,
    band: f64,
) -> Option<ResizeDirection> {
    let (width, height) = (f64::from(size.width), f64::from(size.height));
    let inside = (0.0..width).contains(&pos.x) && (0.0..height).contains(&pos.y);
    if !inside {
        return None;
    }
    let reach = 2.0 * band;
    let (west, east) = (pos.x < band, pos.x >= width - band);
    let (north, south) = (pos.y < band, pos.y >= height - band);
    let (left, right) = (pos.x < reach, pos.x >= width - reach);
    let (top, bottom) = (pos.y < reach, pos.y >= height - reach);
    let direction = if north && left || west && top {
        ResizeDirection::NorthWest
    } else if north && right || east && top {
        ResizeDirection::NorthEast
    } else if south && left || west && bottom {
        ResizeDirection::SouthWest
    } else if south && right || east && bottom {
        ResizeDirection::SouthEast
    } else if north {
        ResizeDirection::North
    } else if south {
        ResizeDirection::South
    } else if west {
        ResizeDirection::West
    } else if east {
        ResizeDirection::East
    } else {
        return None;
    };
    Some(direction)
}

/// The cursor that shows a resize direction.
pub(crate) fn resize_cursor(direction: ResizeDirection) -> CursorKind {
    match direction {
        ResizeDirection::East | ResizeDirection::West => CursorKind::ResizeHorizontal,
        ResizeDirection::North | ResizeDirection::South => CursorKind::ResizeVertical,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorKind::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorKind::ResizeNeSw,
    }
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

    #[test]
    fn the_edges_corners_and_inside_of_a_window_start_the_right_resize() {
        use ResizeDirection::{
            East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West,
        };
        let size = PhysicalSize::new(1000, 700);
        // A band of 5 points: edge zones 5 deep, corner zones 10 long.
        let table = [
            ((0.0, 0.0), Some(NorthWest)),
            ((999.5, 0.0), Some(NorthEast)),
            ((0.0, 699.5), Some(SouthWest)),
            ((999.5, 699.5), Some(SouthEast)),
            ((500.0, 0.0), Some(North)),
            ((500.0, 4.9), Some(North)),
            ((500.0, 5.0), None),
            ((500.0, 699.0), Some(South)),
            ((0.0, 350.0), Some(West)),
            ((4.9, 350.0), Some(West)),
            ((5.0, 350.0), None),
            ((999.0, 350.0), Some(East)),
            // The corner zone is the edge zone along 10 points of the other edge.
            ((9.9, 2.0), Some(NorthWest)),
            ((2.0, 9.9), Some(NorthWest)),
            ((10.0, 2.0), Some(North)),
            ((2.0, 10.0), Some(West)),
            ((990.0, 2.0), Some(NorthEast)),
            ((997.0, 9.0), Some(NorthEast)),
            ((3.0, 690.0), Some(SouthWest)),
            ((990.0, 698.0), Some(SouthEast)),
            // Inside the window, even close to a corner, and outside it.
            ((7.0, 7.0), None),
            ((500.0, 350.0), None),
            ((-1.0, 10.0), None),
            ((10.0, 700.0), None),
            ((1000.0, 350.0), None),
        ];
        for ((x, y), expected) in table {
            let found = resize_edge(PhysicalPosition::new(x, y), size, EDGE_BAND);
            assert_eq!(found, expected, "at ({x}, {y})");
        }
    }

    #[test]
    fn the_band_scales_and_a_band_of_zero_finds_no_edge() {
        let size = PhysicalSize::new(1000, 700);
        let at = |x, y, band| resize_edge(PhysicalPosition::new(x, y), size, band);
        // At scale 1.5 the zone is 7.5 pixels deep.
        assert_eq!(at(7.0, 350.0, 7.5), Some(ResizeDirection::West));
        assert_eq!(at(7.0, 350.0, EDGE_BAND), None);
        // A maximized window, a full-screen one or one with the system frame passes 0.
        for (x, y) in [(0.0, 0.0), (500.0, 0.0), (999.0, 699.0), (0.0, 350.0)] {
            assert_eq!(at(x, y, 0.0), None, "at ({x}, {y})");
        }
    }

    #[test]
    fn a_resize_shows_the_cursor_of_its_direction() {
        use ResizeDirection::{
            East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West,
        };
        let table = [
            (West, CursorKind::ResizeHorizontal),
            (East, CursorKind::ResizeHorizontal),
            (North, CursorKind::ResizeVertical),
            (South, CursorKind::ResizeVertical),
            (NorthWest, CursorKind::ResizeNwSe),
            (SouthEast, CursorKind::ResizeNwSe),
            (NorthEast, CursorKind::ResizeNeSw),
            (SouthWest, CursorKind::ResizeNeSw),
        ];
        for (direction, cursor) in table {
            assert_eq!(resize_cursor(direction), cursor, "{direction:?}");
        }
    }
}
