//! Voxel ray traversal over a collider snapshot: hit point and normal of the first exposed face
//! (spec: tools 7.6 port notes, 12.2).
//!
//! The walk runs in index space, where a cell is a unit cube, so every step is one integer
//! increment. The ray parameter is the same in both spaces because the direction is scaled by the
//! same factor as the origin, so the distances it reports are world distances.

use cuboid_num::trunc_i32;
use glam::Vec3;

use crate::contract::{I3, Ray, VoxelHit};
use crate::mapping::{cell_size, index_to_world};
use crate::world::VoxelWorld;

/// Walks the ray through the world and returns the point and outward normal of the first face
/// of a non-empty voxel it enters within `max_len`, which equals a single-sided mesh-collider
/// hit. Cells outside the active box are empty, so a box that is empty on an axis gives no hit, and
/// so does a zero direction.
///
/// A ray that starts inside a voxel, or exactly on one of its face planes, leaves it without a hit
/// (its faces are back faces) and reports the next voxel it enters. A ray that lies exactly in a
/// cell plane walks the cells on the positive side of the plane. `ray.dir` must be a unit vector.
///
/// spec: arch 13.7 (Voxels), tools 12.2
pub fn dda(world: &VoxelWorld, ray: Ray, max_len: f32) -> Option<VoxelHit> {
    let active = world.active_box;
    if active.min.cmpge(active.max).any() || ray.dir == Vec3::ZERO {
        return None;
    }
    let res = world.res;
    let c = cell_size(res);
    let q = Vec3::new(
        (ray.origin.x + 16.0) / c,
        (ray.origin.y + 16.0) / c,
        (16.0 - ray.origin.z) / c,
    );
    let d = Vec3::new(ray.dir.x / c, ray.dir.y / c, -ray.dir.z / c);

    // Clip the ray to the active box: everything outside it is empty. `entry` is the axis of the
    // box face the ray enters through when it starts outside.
    let mut t_in = 0.0_f32;
    let mut t_out = max_len;
    let mut entry = None;
    for axis in 0..3 {
        let (lo, hi) = (active.min[axis] as f32, active.max[axis] as f32);
        if d[axis] == 0.0 {
            if q[axis] < lo || q[axis] >= hi {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo - q[axis]) / d[axis], (hi - q[axis]) / d[axis]);
        let (near, far) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
        if near > t_in {
            t_in = near;
            entry = Some(axis);
        }
        t_out = t_out.min(far);
    }
    if t_in > t_out {
        return None;
    }

    let hit = |cell: I3, axis: usize, t: f32| {
        // The ray enters through the near side of the cell on this axis.
        let mut corner = cell;
        corner[axis] += i32::from(d[axis] < 0.0);
        let mut point = ray.origin + ray.dir * t;
        point[axis] = index_to_world(corner, res)[axis];
        let mut normal = Vec3::ZERO;
        normal[axis] = -ray.dir[axis].signum();
        VoxelHit {
            point,
            normal,
            distance: t,
        }
    };

    let start = q + d * t_in;
    let mut cell = I3::new(
        trunc_i32(start.x.floor()),
        trunc_i32(start.y.floor()),
        trunc_i32(start.z.floor()),
    )
    .clamp(active.min, active.max - I3::ONE);
    let mut solid = world.cell_at(cell) != 0;
    if solid && let Some(axis) = entry {
        return Some(hit(cell, axis, t_in));
    }

    // Parameter of the next cell boundary on each axis, computed from the origin each time so
    // that rounding does not accumulate over the steps.
    let sign = |v: f32| i32::from(v > 0.0) - i32::from(v < 0.0);
    let step = I3::new(sign(d.x), sign(d.y), sign(d.z));
    let boundary = |cell: I3, axis: usize| {
        ((cell[axis] + i32::from(d[axis] > 0.0)) as f32 - q[axis]) / d[axis]
    };
    let mut t_next = Vec3::INFINITY;
    for axis in 0..3 {
        if d[axis] != 0.0 {
            t_next[axis] = boundary(cell, axis);
        }
    }

    loop {
        let axis = if t_next.x <= t_next.y && t_next.x <= t_next.z {
            0
        } else if t_next.y <= t_next.z {
            1
        } else {
            2
        };
        let t = t_next[axis];
        if t.is_nan() || t > t_out {
            return None;
        }
        cell[axis] += step[axis];
        t_next[axis] = boundary(cell, axis);
        if !active.contains(cell) {
            return None;
        }
        let was_solid = solid;
        solid = world.cell_at(cell) != 0;
        if solid && !was_solid {
            return Some(hit(cell, axis, t));
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::contract::IBox3;
    use crate::mapping::grid_box;

    const RED: u32 = 0xFFFF_0000;

    /// R = 16, so a cell is one unit: cell `(16, 16, 16)` is the unit cube with its corner at the
    /// origin, `x` and `y` in `[0, 1]` and `z` in `[-1, 0]`.
    fn world() -> VoxelWorld {
        let mut world = VoxelWorld::new(16, true, grid_box(16));
        world.write_full(I3::new(16, 16, 16), RED);
        world
    }

    fn ray(origin: [f32; 3], dir: [f32; 3]) -> Ray {
        Ray {
            origin: Vec3::from_array(origin),
            dir: Vec3::from_array(dir).normalize(),
        }
    }

    fn assert_hit(hit: Option<VoxelHit>, point: [f32; 3], normal: [f32; 3], distance: f32) {
        let hit = hit.expect("a hit");
        let near = |a: Vec3, b: [f32; 3]| (a - Vec3::from_array(b)).abs().max_element() < 1e-4;
        assert!(
            near(hit.point, point),
            "point {} against {point:?}",
            hit.point
        );
        assert!(
            near(hit.normal, normal),
            "normal {} against {normal:?}",
            hit.normal
        );
        assert!(
            (hit.distance - distance).abs() < 1e-4,
            "distance {}",
            hit.distance
        );
    }

    #[test]
    fn a_ray_along_each_axis_hits_the_face_it_enters() {
        let world = world();
        let long = 100.0;
        // From -Y, +Y, +Z, -Z, -X and +X toward the cube centre (0.5, 0.5, -0.5).
        let cases = [
            (
                [0.5, -4.0, -0.5],
                [0.0, 1.0, 0.0],
                [0.5, 0.0, -0.5],
                [0.0, -1.0, 0.0],
                4.0,
            ),
            (
                [0.5, 5.0, -0.5],
                [0.0, -1.0, 0.0],
                [0.5, 1.0, -0.5],
                [0.0, 1.0, 0.0],
                4.0,
            ),
            (
                [0.5, 0.5, 5.0],
                [0.0, 0.0, -1.0],
                [0.5, 0.5, 0.0],
                [0.0, 0.0, 1.0],
                5.0,
            ),
            (
                [0.5, 0.5, -6.0],
                [0.0, 0.0, 1.0],
                [0.5, 0.5, -1.0],
                [0.0, 0.0, -1.0],
                5.0,
            ),
            (
                [-3.0, 0.5, -0.5],
                [1.0, 0.0, 0.0],
                [0.0, 0.5, -0.5],
                [-1.0, 0.0, 0.0],
                3.0,
            ),
            (
                [4.0, 0.5, -0.5],
                [-1.0, 0.0, 0.0],
                [1.0, 0.5, -0.5],
                [1.0, 0.0, 0.0],
                3.0,
            ),
        ];
        for (origin, dir, point, normal, distance) in cases {
            assert_hit(dda(&world, ray(origin, dir), long), point, normal, distance);
        }
    }

    #[test]
    fn an_oblique_ray_reports_the_entered_face_and_the_point_on_it() {
        let world = world();
        // Slope -0.5 in z per unit of x: from z = 0.5 the ray is at z = -0.5 when it reaches the
        // -X face at x = 0; from z = -0.25 it is already below the cube there (z = -1.25).
        let dir = Vec3::new(1.0, 0.0, -0.5).normalize();
        let hit = dda(
            &world,
            Ray {
                origin: Vec3::new(-2.0, 0.5, 0.5),
                dir,
            },
            50.0,
        );
        assert_hit(hit, [0.0, 0.5, -0.5], [-1.0, 0.0, 0.0], 2.0 / dir.x);
        let hit = dda(
            &world,
            Ray {
                origin: Vec3::new(-2.0, 0.5, -0.25),
                dir,
            },
            50.0,
        );
        assert!(hit.is_none());
    }

    #[test]
    fn a_ray_that_misses_or_stops_short_gives_nothing() {
        let world = world();
        assert!(dda(&world, ray([3.0, 0.5, -0.5], [0.0, 1.0, 0.0]), 50.0).is_none());
        // The cube is 4 units away.
        let toward = ray([0.5, -4.0, -0.5], [0.0, 1.0, 0.0]);
        assert!(dda(&world, toward, 3.9).is_none());
        assert!(dda(&world, toward, 4.0).is_some());
        // Pointing away, and parallel to a face outside the cube.
        assert!(dda(&world, ray([0.5, -4.0, -0.5], [0.0, -1.0, 0.0]), 50.0).is_none());
        assert!(dda(&world, ray([-5.0, 2.5, -0.5], [1.0, 0.0, 0.0]), 50.0).is_none());
    }

    #[test]
    fn a_ray_starting_inside_a_voxel_leaves_it_and_hits_the_next_one() {
        let mut world = world();
        let inside = ray([0.5, 0.5, -0.5], [0.0, 1.0, 0.0]);
        assert!(
            dda(&world, inside, 50.0).is_none(),
            "back faces are not hit"
        );
        // A voxel three cells further on: entered from the empty cell between.
        world.write_full(I3::new(16, 19, 16), RED);
        assert_hit(
            dda(&world, inside, 50.0),
            [0.5, 3.0, -0.5],
            [0.0, -1.0, 0.0],
            2.5,
        );
    }

    #[test]
    fn voxels_that_touch_have_no_face_between_them() {
        let mut world = world();
        world.write_full(I3::new(16, 17, 16), RED);
        // From below the lower voxel is hit once at its bottom; from inside the lower one, the
        // shared face is not a hit and the ray leaves through the top of the upper voxel.
        assert_hit(
            dda(&world, ray([0.5, -2.0, -0.5], [0.0, 1.0, 0.0]), 50.0),
            [0.5, 0.0, -0.5],
            [0.0, -1.0, 0.0],
            2.0,
        );
        assert!(dda(&world, ray([0.5, 0.5, -0.5], [0.0, 1.0, 0.0]), 50.0).is_none());
        // From above, only the upper voxel's top face counts.
        assert_hit(
            dda(&world, ray([0.5, 6.0, -0.5], [0.0, -1.0, 0.0]), 50.0),
            [0.5, 2.0, -0.5],
            [0.0, 1.0, 0.0],
            4.0,
        );
    }

    #[test]
    fn a_ray_may_start_outside_the_grid_or_cross_chunks() {
        let mut world = VoxelWorld::new(16, true, grid_box(16));
        // Cell (47, 0, 47) is the corner of the grid: x in [31, 32], y in [-16, -15], z in
        // [-32, -31].
        world.write_full(I3::new(47, 0, 47), RED);
        assert_hit(
            dda(&world, ray([100.0, -15.5, -31.5], [-1.0, 0.0, 0.0]), 500.0),
            [32.0, -15.5, -31.5],
            [1.0, 0.0, 0.0],
            68.0,
        );
        // A slanted ray from inside the grid, through several chunks, onto the top of the voxel.
        let dir = Vec3::new(1.0, 0.0, -1.0).normalize();
        let origin = Vec3::new(-15.0, -15.5, 15.5);
        let hit = dda(&world, Ray { origin, dir }, 500.0);
        assert_hit(
            hit,
            [31.5, -15.5, -31.0],
            [0.0, 0.0, 1.0],
            46.5 * 2.0_f32.sqrt(),
        );
    }

    #[test]
    fn cells_outside_the_active_box_are_empty() {
        let mut world = world();
        world.write_full(I3::new(16, 20, 16), RED);
        let down = ray([0.5, 8.0, -0.5], [0.0, -1.0, 0.0]);
        let up = ray([0.5, -4.0, -0.5], [0.0, 1.0, 0.0]);
        assert_hit(
            dda(&world, down, 50.0),
            [0.5, 5.0, -0.5],
            [0.0, 1.0, 0.0],
            3.0,
        );

        // The box stops below the second voxel (its cell is j = 20): the first is hit instead.
        let mut active = grid_box(16);
        active.max.y = 20;
        world.set_active_box(active);
        assert_hit(
            dda(&world, down, 50.0),
            [0.5, 1.0, -0.5],
            [0.0, 1.0, 0.0],
            7.0,
        );

        // A box that starts above the first voxel hides it, and the ray from below meets the
        // second one.
        let mut active = grid_box(16);
        active.min.y = 17;
        world.set_active_box(active);
        assert_hit(
            dda(&world, up, 50.0),
            [0.5, 4.0, -0.5],
            [0.0, -1.0, 0.0],
            8.0,
        );
        world.write_full(I3::new(16, 20, 16), 0);
        assert!(dda(&world, up, 50.0).is_none());
    }

    #[test]
    fn a_box_that_is_empty_on_one_axis_gives_no_hit() {
        for axis in 0..3 {
            let mut world = world();
            // The voxel is cell 16 on every axis; the box ends right below its cell index + 1.
            let mut active = grid_box(16);
            active.min[axis] = 17;
            active.max[axis] = 17;
            world.set_active_box(active);
            let from = [[-3.0, 0.5, -0.5], [0.5, -3.0, -0.5], [0.5, 0.5, 3.0]][axis];
            let toward = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]][axis];
            assert!(
                dda(&world, ray(from, toward), 100.0).is_none(),
                "axis {axis}"
            );
            // Oblique rays that cross the empty plane in front of the voxel.
            let mut dir = Vec3::from_array(toward) * 4.0;
            dir[(axis + 1) % 3] += 0.25;
            let oblique = Ray {
                origin: Vec3::from_array(from),
                dir: dir.normalize(),
            };
            assert!(
                dda(&world, oblique, 100.0).is_none(),
                "oblique, axis {axis}"
            );
            // The voxel is only hidden: with the box restored the axis ray hits it.
            world.set_active_box(grid_box(16));
            assert!(
                dda(&world, ray(from, toward), 100.0).is_some(),
                "axis {axis}"
            );
        }
    }

    #[test]
    fn a_zero_direction_gives_no_hit() {
        let world = world();
        let still = Ray {
            origin: Vec3::new(0.5, 0.5, -0.5),
            dir: Vec3::ZERO,
        };
        assert!(dda(&world, still, 100.0).is_none());
        assert!(dda(&world, still, f32::INFINITY).is_none());
        assert!(dda(&world, still, f32::NAN).is_none());
    }

    #[test]
    fn rays_on_a_cell_plane_or_face_plane_follow_the_documented_rules() {
        let world = world();
        // Along +X in the plane y = 0, the lower face of the voxel: the positive side is cell
        // j = 16, so the voxel is entered. In the plane y = 1 (its upper face) the positive side
        // is the empty cell j = 17: no hit.
        assert_hit(
            dda(&world, ray([-3.0, 0.0, -0.5], [1.0, 0.0, 0.0]), 50.0),
            [0.0, 0.0, -0.5],
            [-1.0, 0.0, 0.0],
            3.0,
        );
        assert!(dda(&world, ray([-3.0, 1.0, -0.5], [1.0, 0.0, 0.0]), 50.0).is_none());
        // A ray that starts exactly on the front face plane starts inside the voxel and misses
        // that face.
        assert!(dda(&world, ray([0.0, 0.5, -0.5], [1.0, 0.0, 0.0]), 50.0).is_none());
    }

    #[test]
    fn the_hit_lies_on_the_face_at_other_resolutions() {
        // R = 4: c = 4. Cell (4, 5, 6): x in [0, 4], y in [4, 8], z in [-12, -8].
        let mut world = VoxelWorld::new(4, false, grid_box(4));
        world.write_full(I3::new(4, 5, 6), RED);
        let hit = dda(&world, ray([2.0, 6.0, 12.0], [0.0, 0.0, -1.0]), 100.0);
        assert_hit(hit, [2.0, 6.0, -8.0], [0.0, 0.0, 1.0], 20.0);
        let hit = dda(&world, ray([2.0, 6.0, -30.0], [0.0, 0.0, 1.0]), 100.0);
        assert_hit(hit, [2.0, 6.0, -12.0], [0.0, 0.0, -1.0], 18.0);
    }

    /// The single-sided mesh collider of the specification: the nearest front-facing exposed face
    /// of any readable voxel.
    fn brute_force(world: &VoxelWorld, ray: Ray, max_len: f32) -> Option<(f32, Vec3)> {
        let c = cell_size(world.res);
        let extent = 3 * world.res as i32;
        let mut best: Option<(f32, Vec3)> = None;
        for i in 0..extent {
            for j in 0..extent {
                for k in 0..extent {
                    let cell = I3::new(i, j, k);
                    if world.read(cell) == 0 {
                        continue;
                    }
                    let min = index_to_world(cell, world.res) - Vec3::new(0.0, 0.0, c);
                    for (axis, sign) in [(0, -1), (0, 1), (1, -1), (1, 1), (2, -1), (2, 1)] {
                        // The neighbour index across this face: z flips.
                        let mut next = cell;
                        next[axis] += if axis == 2 { -sign } else { sign };
                        if world.read(next) != 0 {
                            continue;
                        }
                        let mut normal = Vec3::ZERO;
                        normal[axis] = sign as f32;
                        if ray.dir.dot(normal) >= 0.0 {
                            continue;
                        }
                        let plane = if sign < 0 { min[axis] } else { min[axis] + c };
                        let t = (plane - ray.origin[axis]) / ray.dir[axis];
                        if !(0.0..=max_len).contains(&t) {
                            continue;
                        }
                        let p = ray.origin + ray.dir * t;
                        let on_face = (0..3)
                            .filter(|&a| a != axis)
                            .all(|a| p[a] >= min[a] && p[a] <= min[a] + c);
                        if on_face && best.is_none_or(|(b, _)| t < b) {
                            best = Some((t, normal));
                        }
                    }
                }
            }
        }
        best
    }

    proptest! {
        // R = 4 (c = 4, 12 cells per axis): random cells, a random box and random rays from
        // inside and outside the grid, against the brute-force search over all faces.
        #[test]
        fn the_walk_equals_the_nearest_front_face_of_all_voxels(
            cells in prop::collection::vec((0..12, 0..12, 0..12), 0..160),
            lo in (0_i32..6, 0_i32..6, 0_i32..6),
            span in (0_i32..13, 0_i32..13, 0_i32..13),
            origin in (-40.0_f32..40.0, -40.0_f32..40.0, -40.0_f32..40.0),
            dir in (-1.0_f32..1.0, -1.0_f32..1.0, -1.0_f32..1.0),
            max_len in 1.0_f32..120.0,
        ) {
            let dir = Vec3::new(dir.0, dir.1, dir.2);
            prop_assume!(dir.length() > 0.05);
            // The box may be empty on an axis (span 0).
            let hi = ((lo.0 + span.0).min(12), (lo.1 + span.1).min(12), (lo.2 + span.2).min(12));
            let mut world = VoxelWorld::new(4, true, grid_box(4));
            for (x, y, z) in cells {
                world.write_full(I3::new(x, y, z), RED);
            }
            world.set_active_box(IBox3 {
                min: I3::new(lo.0, lo.1, lo.2),
                max: I3::new(hi.0, hi.1, hi.2),
            });
            let ray = Ray { origin: Vec3::new(origin.0, origin.1, origin.2), dir: dir.normalize() };
            let expected = brute_force(&world, ray, max_len);
            let actual = dda(&world, ray, max_len);
            match (expected, actual) {
                (None, None) => {}
                (Some((t, normal)), Some(hit)) => {
                    prop_assert!((hit.distance - t).abs() < 1e-3, "{} against {}", hit.distance, t);
                    prop_assert_eq!(hit.normal, normal);
                    let on_ray = ray.origin + ray.dir * t;
                    prop_assert!((hit.point - on_ray).abs().max_element() < 1e-3);
                }
                (expected, actual) => prop_assert!(false, "{expected:?} against {actual:?}"),
            }
        }
    }
}
