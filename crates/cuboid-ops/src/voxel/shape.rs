//! Cell geometry of the voxel tools: the cells a ray hit selects, lines, boxes, the cube brush and
//! the mirror (spec: tools 12.2, 12.3).

use cuboid_num::trunc_i32;
use cuboid_voxel::mapping::cell_size;
use cuboid_voxel::{I3, IBox3};
use glam::Vec3;

use super::{MAX_STROKE_CELLS, VoxelError};

/// Cell indices stay within this distance of the origin, so index arithmetic never overflows.
const FAR: i32 = 1 << 20;

/// The cells a ray hit selects and the face it hit.
///
/// spec: tools 12.2 (hit mapping)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FaceHit {
    /// The cell behind the face, the solid one.
    pub inward: I3,
    /// The cell in front of the face.
    pub outward: I3,
    /// The index axis the face is perpendicular to: 0 for X, 1 for Y, 2 for Z.
    pub axis: usize,
    /// The normal points toward +X, +Y or +Z of the world.
    pub positive: bool,
}

/// The cell holding the world point `p`. Unlike the truncated mapping of the specification it
/// rounds down, so a point below the grid maps to a cell outside it instead of to cell 0.
pub fn cell_at(p: Vec3, res: u32) -> I3 {
    let c = cell_size(res);
    let index = |v: f32| trunc_i32(v.floor()).clamp(-FAR, FAR);
    I3::new(
        index((p.x + 16.0) / c),
        index((p.y + 16.0) / c),
        index((16.0 - p.z) / c),
    )
}

/// The inward and outward cell of a hit: the cells a half voxel behind and in front of the hit
/// point along the normal. `None` for a non-finite point or a zero normal.
///
/// spec: tools 12.2
pub fn hit_cells(point: Vec3, normal: Vec3, res: u32) -> Option<FaceHit> {
    if !(point.is_finite() && normal.is_finite()) {
        return None;
    }
    let abs = normal.abs();
    if abs.max_element() <= 0.0 {
        return None;
    }
    let axis = if abs.x >= abs.y && abs.x >= abs.z {
        0
    } else if abs.y >= abs.z {
        1
    } else {
        2
    };
    let half = normal * (cell_size(res) * 0.5);
    Some(FaceHit {
        inward: cell_at(point - half, res),
        outward: cell_at(point + half, res),
        axis,
        positive: normal[axis] > 0.0,
    })
}

/// Appends the cells of the 3D Bresenham line from `a` to `b`, both ends included: `major + 1`
/// cells, `major` being the largest coordinate difference, each minor error starting at half of
/// it.
///
/// spec: tools 12.3 (Line)
pub fn line_cells(a: I3, b: I3, out: &mut Vec<I3>) {
    let delta = (b - a).abs();
    let step = (b - a).signum();
    let major = delta.max_element();
    let lead = if delta.x == major {
        0
    } else if delta.y == major {
        1
    } else {
        2
    };
    let mut at = a;
    let mut err = I3::splat(major / 2);
    out.push(at);
    for _ in 0..major {
        at[lead] += step[lead];
        for axis in (0..3).filter(|&axis| axis != lead) {
            err[axis] -= delta[axis];
            if err[axis] < 0 {
                at[axis] += step[axis];
                err[axis] += major;
            }
        }
        out.push(at);
    }
}

/// Appends the cells of the box between the corners `a` and `b`, both inclusive, that lie inside
/// `clip`. Refuses a box with more than [`MAX_STROKE_CELLS`] cells before it builds anything.
///
/// spec: tools 12.3 (Cuboid)
pub fn box_cells(a: I3, b: I3, clip: IBox3, out: &mut Vec<I3>) -> Result<(), VoxelError> {
    let min = a.min(b).max(clip.min);
    let max = (a.max(b) + I3::ONE).min(clip.max);
    if min.cmpge(max).any() {
        return Ok(());
    }
    let size = max - min;
    let volume = i64::from(size.x) * i64::from(size.y) * i64::from(size.z);
    if volume > i64::try_from(MAX_STROKE_CELLS).unwrap_or(i64::MAX) {
        return Err(VoxelError::TooLarge(MAX_STROKE_CELLS));
    }
    for z in min.z..max.z {
        for y in min.y..max.y {
            for x in min.x..max.x {
                out.push(I3::new(x, y, z));
            }
        }
    }
    Ok(())
}

/// The coordinates a cube brush of edge `size` covers around `c`: `c - (size - 1) / 2` and the
/// next `size - 1`, so an odd brush is centred and an even one reaches one cell further up.
pub fn brush_span(c: i32, size: i32) -> std::ops::Range<i32> {
    let lo = c - (size - 1) / 2;
    lo..lo + size
}

/// The cell and its mirror images across the planes through the middle of `active`, one image
/// per subset of the mirrored axes (up to eight; a cell on a plane repeats itself).
pub fn mirror_images(cell: I3, mirror: [bool; 3], active: IBox3) -> impl Iterator<Item = I3> {
    let pivot = active.min + active.max - I3::ONE;
    (0..8_u8)
        .filter(move |m| (0..3).all(|axis| (m >> axis) & 1 == 0 || mirror[axis]))
        .map(move |m| {
            let mut image = cell;
            for axis in (0..3).filter(|axis| (m >> axis) & 1 == 1) {
                image[axis] = pivot[axis] - cell[axis];
            }
            image
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(x: i32, y: i32, z: i32) -> I3 {
        I3::new(x, y, z)
    }

    fn line(a: I3, b: I3) -> Vec<I3> {
        let mut out = Vec::new();
        line_cells(a, b, &mut out);
        out
    }

    #[test]
    fn a_hit_selects_the_cells_on_both_sides_of_its_face() {
        // R = 16, cell size 1: the +X face of cell (20, 16, 16) is the plane x = 5.
        let hit = hit_cells(Vec3::new(5.0, 0.5, -0.5), Vec3::X, 16).expect("a hit");
        assert_eq!(hit.inward, cell(20, 16, 16));
        assert_eq!(hit.outward, cell(21, 16, 16));
        assert_eq!((hit.axis, hit.positive), (0, true));
        // Internal -Z is index +k, so the +Z face at z = -1 has its solid cell at k = 17.
        let hit = hit_cells(Vec3::new(0.5, 0.5, -1.0), Vec3::Z, 16).expect("a hit");
        assert_eq!(hit.inward, cell(16, 16, 17));
        assert_eq!(hit.outward, cell(16, 16, 16));
        assert_eq!((hit.axis, hit.positive), (2, true));
        assert_eq!(hit_cells(Vec3::ZERO, Vec3::ZERO, 16), None);
        assert_eq!(hit_cells(Vec3::NAN, Vec3::X, 16), None);
    }

    #[test]
    fn a_point_below_the_grid_maps_outside_it() {
        assert_eq!(cell_at(Vec3::new(-16.5, 0.0, 0.0), 16).x, -1);
        assert_eq!(cell_at(Vec3::new(1e30, 0.0, 0.0), 16).x, FAR);
    }

    #[test]
    fn a_line_has_major_plus_one_cells_and_both_ends() {
        let cells = line(cell(0, 0, 0), cell(10, 3, -5));
        assert_eq!(cells.len(), 11);
        assert_eq!((cells[0], cells[10]), (cell(0, 0, 0), cell(10, 3, -5)));
        assert!(
            cells
                .windows(2)
                .all(|w| (w[1] - w[0]).abs().max_element() == 1)
        );
        assert_eq!(line(cell(4, 4, 4), cell(4, 4, 4)), [cell(4, 4, 4)]);
        let back = line(cell(3, 0, 0), cell(0, 0, 0));
        assert_eq!(
            back,
            [cell(3, 0, 0), cell(2, 0, 0), cell(1, 0, 0), cell(0, 0, 0)]
        );
    }

    #[test]
    fn a_box_covers_both_corners_and_stays_inside_the_clip() {
        let clip = IBox3 {
            min: I3::ZERO,
            max: I3::splat(4),
        };
        let mut out = Vec::new();
        box_cells(cell(1, 1, 1), cell(2, 3, 1), clip, &mut out).expect("small");
        assert_eq!(out.len(), 6);
        out.clear();
        box_cells(cell(-5, 2, 2), cell(1, 9, 2), clip, &mut out).expect("small");
        assert_eq!(out.len(), 2 * 2);
        out.clear();
        box_cells(cell(10, 10, 10), cell(11, 11, 11), clip, &mut out).expect("outside");
        assert!(out.is_empty());
        let huge = IBox3 {
            min: I3::ZERO,
            max: I3::splat(1000),
        };
        assert!(matches!(
            box_cells(I3::ZERO, I3::splat(999), huge, &mut out),
            Err(VoxelError::TooLarge(_))
        ));
        assert!(out.is_empty(), "nothing is built for a refused box");
    }

    #[test]
    fn an_odd_brush_is_centred_and_an_even_one_reaches_up() {
        assert_eq!(brush_span(10, 1), 10..11);
        assert_eq!(brush_span(10, 3), 9..12);
        assert_eq!(brush_span(10, 2), 10..12);
        assert_eq!(brush_span(10, 4), 9..13);
    }

    #[test]
    fn mirror_images_cover_the_subsets_of_the_mirrored_axes() {
        let active = IBox3 {
            min: I3::ZERO,
            max: I3::splat(48),
        };
        let at = cell(3, 5, 10);
        let none: Vec<_> = mirror_images(at, [false; 3], active).collect();
        assert_eq!(none, [at]);
        let x: Vec<_> = mirror_images(at, [true, false, false], active).collect();
        assert_eq!(x, [at, cell(44, 5, 10)]);
        let all: Vec<_> = mirror_images(at, [true; 3], active).collect();
        assert_eq!(all.len(), 8);
        assert!(all.contains(&cell(44, 42, 37)));
        // A smaller active box mirrors about its own middle.
        let small = IBox3 {
            min: cell(10, 0, 0),
            max: cell(20, 48, 48),
        };
        let x: Vec<_> = mirror_images(cell(11, 5, 10), [true, false, false], small).collect();
        assert_eq!(x, [cell(11, 5, 10), cell(18, 5, 10)]);
    }
}
