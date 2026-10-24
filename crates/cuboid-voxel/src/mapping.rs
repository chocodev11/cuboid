//! Cell index to world corner and back, rounded and truncated variants, active-box conversion
//! (spec: tools 7.2).
//!
//! The arithmetic is single precision in the operation order of the specification: the cell size
//! `c = 16 / R` is one f32, and every coordinate is one subtraction or addition and one product
//! or quotient with `c`. The evaluation policy of `cuboid-num` does not reach these formulas
//! (its generic arithmetic is private to that crate); this is the width the specification
//! states (**UNVERIFIED** against the original's runtime, architecture risk R1).

use cuboid_num::{round_he, trunc_i32};
use glam::Vec3;

use crate::contract::{I3, IBox3, WBox3};

impl IBox3 {
    /// Whether `i` lies in the half-open box.
    pub fn contains(self, i: I3) -> bool {
        i.cmpge(self.min).all() && i.cmplt(self.max).all()
    }
}

/// The whole grid, `[0, 3R)` on every axis: the active box of a world whose model box is the
/// editing volume.
///
/// spec: tools 7.1, tools 7.2
pub fn grid_box(res: u32) -> IBox3 {
    IBox3 {
        min: I3::ZERO,
        max: I3::splat(3 * res as i32),
    }
}

/// The edge of a cell in world units, `16 / R`.
///
/// spec: tools 7.1
pub fn cell_size(res: u32) -> f32 {
    16.0 / res as f32
}

/// The world corner of cell `i` with the smallest x, the smallest y and the largest z (the
/// element convention): `((i - R) * c, (j - R) * c, (R - k) * c)`. The cell spans
/// `[x, x + c] x [y, y + c] x [z - c, z]`.
///
/// spec: tools 7.2
pub fn index_to_world(i: I3, res: u32) -> Vec3 {
    let c = cell_size(res);
    let r = res as i32;
    Vec3::new(
        (i.x - r) as f32 * c,
        (i.y - r) as f32 * c,
        (r - i.z) as f32 * c,
    )
}

/// The cell holding the world point `p`, rounded half to even: box corners, blueprint boxes,
/// copy and paste. `i = round((x + 16) / c)`, `j = round((y + 16) / c)`,
/// `k = round((16 - z) / c)`.
///
/// spec: tools 7.2
pub fn world_to_index_rounded(p: Vec3, res: u32) -> I3 {
    to_index(p, res, |v| trunc_i32(round_he(v)))
}

/// The cell holding the world point `p`, truncated toward zero: ray hits and the voxel tools.
/// A point just outside the grid on the negative side maps to 0, not -1.
///
/// spec: tools 7.2
pub fn world_to_index_truncated(p: Vec3, res: u32) -> I3 {
    to_index(p, res, trunc_i32)
}

fn to_index(p: Vec3, res: u32, to_int: fn(f32) -> i32) -> I3 {
    let c = cell_size(res);
    I3::new(
        to_int((p.x + 16.0) / c),
        to_int((p.y + 16.0) / c),
        to_int((16.0 - p.z) / c),
    )
}

/// The active box of a world: the model box converted with the rounded mapping. Z flips, so the
/// minimum and maximum are taken per axis after the conversion. The box is half-open.
///
/// spec: tools 7.2
pub fn active_box(model_box: WBox3, res: u32) -> IBox3 {
    let a = world_to_index_rounded(model_box.min, res);
    let b = world_to_index_rounded(model_box.max, res);
    IBox3 {
        min: a.min(b),
        max: a.max(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(i: i32, j: i32, k: i32) -> I3 {
        I3::new(i, j, k)
    }

    #[test]
    fn a_cell_corner_is_min_x_min_y_max_z() {
        let cases = [
            (16, cell(0, 0, 0), Vec3::new(-16.0, -16.0, 16.0)),
            (16, cell(16, 16, 16), Vec3::ZERO),
            (16, cell(47, 47, 47), Vec3::new(31.0, 31.0, -31.0)),
            (16, cell(20, 3, 40), Vec3::new(4.0, -13.0, -24.0)),
            (8, cell(3, 5, 7), Vec3::new(-10.0, -6.0, 2.0)),
            (2, cell(1, 2, 3), Vec3::new(-8.0, 0.0, -8.0)),
            (128, cell(130, 0, 383), Vec3::new(0.25, -16.0, -31.875)),
        ];
        for (res, index, corner) in cases {
            assert_eq!(
                index_to_world(index, res),
                corner,
                "R = {res}, cell {index}"
            );
        }
    }

    #[test]
    fn the_rounded_mapping_rounds_half_to_even() {
        let cases = [
            // Ties: 16.5 -> 16, 17.5 -> 18, 18.5 -> 18, 19.5 -> 20.
            (16, Vec3::new(0.5, 0.5, -0.5), cell(16, 16, 16)),
            (16, Vec3::new(1.5, 2.5, -3.5), cell(18, 18, 20)),
            (16, Vec3::new(0.6, 0.4, -0.6), cell(17, 16, 17)),
            // Just outside the grid: -0.5 -> -0, 48.5 -> 48, -1.5 -> -2.
            (16, Vec3::new(-16.5, 32.5, 16.5), cell(0, 48, 0)),
            (16, Vec3::new(-17.5, 0.0, 0.0), cell(-2, 16, 16)),
            // R = 8, c = 2: 8.5 -> 8, 9.5 -> 10.
            (8, Vec3::new(1.0, 1.0, -1.0), cell(8, 8, 8)),
            (8, Vec3::new(3.0, 3.0, -3.0), cell(10, 10, 10)),
            // R = 2, c = 8: 2.5 -> 2, 3.5 -> 4.
            (2, Vec3::new(4.0, 12.0, -4.0), cell(2, 4, 2)),
        ];
        for (res, point, index) in cases {
            assert_eq!(
                world_to_index_rounded(point, res),
                index,
                "R = {res}, point {point}"
            );
        }
    }

    #[test]
    fn the_truncated_mapping_goes_toward_zero() {
        let cases = [
            (16, Vec3::new(0.5, 0.5, -0.5), cell(16, 16, 16)),
            (16, Vec3::new(0.6, 0.4, -0.6), cell(16, 16, 16)),
            (16, Vec3::new(1.5, 2.5, -3.5), cell(17, 18, 19)),
            // -0.5 truncates to 0 (a floor would give -1); -1.5 to -1.
            (16, Vec3::new(-16.5, 32.5, 16.5), cell(0, 48, 0)),
            (16, Vec3::new(-17.5, 0.0, 0.0), cell(-1, 16, 16)),
            (8, Vec3::new(3.0, 3.0, -3.0), cell(9, 9, 9)),
        ];
        for (res, point, index) in cases {
            assert_eq!(
                world_to_index_truncated(point, res),
                index,
                "R = {res}, point {point}"
            );
        }
    }

    #[test]
    fn corners_round_back_and_cell_centres_truncate_back_to_the_cell() {
        for res in 2..=128_u32 {
            let c = cell_size(res);
            let n = 3 * res as i32;
            for i in 0..n {
                let index = I3::splat(i);
                let corner = index_to_world(index, res);
                assert_eq!(world_to_index_rounded(corner, res), index, "R = {res}");
                let centre = corner + Vec3::new(c / 2.0, c / 2.0, -c / 2.0);
                assert_eq!(world_to_index_truncated(centre, res), index, "R = {res}");
            }
        }
    }

    #[test]
    fn the_active_box_of_the_editing_volume_is_the_whole_grid() {
        for res in 2..=128_u32 {
            assert_eq!(
                active_box(WBox3::EDITING_VOLUME, res),
                grid_box(res),
                "R = {res}"
            );
        }
    }

    #[test]
    fn the_active_box_takes_min_and_max_per_axis_because_z_flips() {
        let model_box = WBox3 {
            min: Vec3::new(-16.0, -16.0, -32.0),
            max: Vec3::ZERO,
        };
        // Z: the model box maximum 0 is index 16, the minimum -32 is index 48.
        assert_eq!(
            active_box(model_box, 16),
            IBox3 {
                min: cell(0, 0, 16),
                max: cell(16, 16, 48),
            }
        );
        // R = 2, c = 8: x from 4 to 12 is 2.5 -> 2 and 3.5 -> 4.
        let model_box = WBox3 {
            min: Vec3::new(4.0, -16.0, -32.0),
            max: Vec3::new(12.0, 32.0, 16.0),
        };
        assert_eq!(
            active_box(model_box, 2),
            IBox3 {
                min: cell(2, 0, 0),
                max: cell(4, 6, 6),
            }
        );
    }

    #[test]
    fn a_box_is_half_open() {
        let b = IBox3 {
            min: cell(2, 0, 4),
            max: cell(4, 1, 6),
        };
        assert!(b.contains(cell(2, 0, 4)));
        assert!(b.contains(cell(3, 0, 5)));
        assert!(!b.contains(cell(4, 0, 4)), "max x is outside");
        assert!(!b.contains(cell(3, 1, 5)), "max y is outside");
        assert!(!b.contains(cell(3, 0, 6)), "max z is outside");
        assert!(!b.contains(cell(1, 0, 4)), "below min x");
        assert!(!b.contains(cell(2, -1, 4)), "below min y");
    }
}
