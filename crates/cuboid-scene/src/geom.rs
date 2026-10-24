//! Boxes, frusta and ray tests shared by the BVH, the picking and the gizmo hit volumes.
//!
//! Every function takes finite input to finite output; a NaN or infinite input gives a miss.

use cuboid_num::trunc_i32;
use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

/// The index of a value in `0..len`, clamped; NaN gives 0.
pub fn clamped_index(v: f32, len: usize) -> usize {
    let top = i32::try_from(len.saturating_sub(1)).unwrap_or(i32::MAX);
    usize::try_from(trunc_i32(v).clamp(0, top)).unwrap_or(0)
}

/// `index` as a `u32`; indices past the range saturate (a scene never holds 4 billion items).
pub fn to_u32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// A unit vector along `v`, or `fallback` when `v` is zero or not finite.
pub fn unit_or(v: Vec3, fallback: Vec3) -> Vec3 {
    let len2 = v.length_squared();
    if len2.is_finite() && len2 > 1e-24 {
        v / len2.sqrt()
    } else {
        fallback
    }
}

/// The reciprocal of a ray direction with the zero components replaced by a huge finite value, so
/// the slab test never multiplies zero by infinity.
pub fn inv_dir(dir: Vec3) -> Vec3 {
    dir.to_array()
        .map(|d| {
            if d.abs() < 1e-20 {
                1e20_f32.copysign(d)
            } else {
                1.0 / d
            }
        })
        .into()
}

/// Reciprocal magnitude from which a direction component counts as parallel to its slab.
const PARALLEL: f32 = 1e19;

/// An axis-aligned box. The empty box has `min > max`, which no ray and no frustum hits.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Aabb {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl Aabb {
    /// The box that contains nothing and is the identity of [`Aabb::union`].
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::INFINITY,
        max: Vec3::NEG_INFINITY,
    };

    /// A box over two corners in any order.
    pub fn from_corners(a: Vec3, b: Vec3) -> Aabb {
        Aabb {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// Whether the box holds at least one finite point.
    pub fn is_valid(&self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.min.cmple(self.max).all()
    }

    /// The box itself, or the empty box when it holds a NaN or an infinity.
    pub fn valid_or_empty(self) -> Aabb {
        if self.is_valid() { self } else { Aabb::EMPTY }
    }

    /// The smallest box that holds both.
    pub fn union(self, other: Aabb) -> Aabb {
        Aabb {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// The box grown to hold `p`.
    pub fn with_point(self, p: Vec3) -> Aabb {
        Aabb {
            min: self.min.min(p),
            max: self.max.max(p),
        }
    }

    /// The centre; meaningless for the empty box.
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Half of the extent on every axis.
    pub fn half(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    /// The surface area, 0 for the empty box.
    pub fn area(&self) -> f32 {
        if !self.is_valid() {
            return 0.0;
        }
        let e = self.max - self.min;
        2.0 * (e.x * e.y + e.y * e.z + e.z * e.x)
    }

    /// Whether the two boxes overlap or touch.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.min.cmple(other.max).all() && other.min.cmple(self.max).all()
    }

    /// The ray parameter where the ray enters the box, 0 when it starts inside, if that is within
    /// `max_t`. `inv` is [`inv_dir`] of the ray direction.
    pub fn ray_entry(&self, origin: Vec3, inv: Vec3, max_t: f32) -> Option<f32> {
        let (mut t_near, mut t_far) = (0.0_f32, max_t);
        for a in 0..3 {
            if inv[a].abs() >= PARALLEL {
                // A ray parallel to the slab is inside it or never enters; the faces count.
                if origin[a].is_nan() || origin[a] < self.min[a] || origin[a] > self.max[a] {
                    return None;
                }
                continue;
            }
            let (t1, t2) = (
                (self.min[a] - origin[a]) * inv[a],
                (self.max[a] - origin[a]) * inv[a],
            );
            t_near = t_near.max(t1.min(t2));
            t_far = t_far.min(t1.max(t2));
        }
        (t_near <= t_far && self.is_valid()).then_some(t_near)
    }
}

/// An oriented box: centre, orientation and half extents.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Obb {
    /// Centre.
    pub center: Vec3,
    /// Orientation.
    pub rot: Quat,
    /// Half extents along the local axes.
    pub half: Vec3,
}

impl Obb {
    /// The world box that holds the oriented box.
    pub fn aabb(&self) -> Aabb {
        let m = Mat3::from_quat(self.rot);
        let r = Vec3::new(
            m.row(0).abs().dot(self.half),
            m.row(1).abs().dot(self.half),
            m.row(2).abs().dot(self.half),
        );
        let a = Aabb {
            min: self.center - r,
            max: self.center + r,
        };
        if a.is_valid() { a } else { Aabb::EMPTY }
    }

    /// The eight corners.
    pub fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|i| {
            let sign = Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            );
            self.center + self.rot * (sign * self.half)
        })
    }
}

/// Six planes with inward normals: a point is inside when `dot(n, p) + w >= 0` for every plane.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Frustum {
    /// The planes as `(n.x, n.y, n.z, w)` with unit normals, in the order left, right, bottom,
    /// top, near, far.
    pub planes: [Vec4; 6],
}

impl Frustum {
    /// The frustum of the part of the view inside the NDC rectangle `ndc_min..ndc_max`, for a
    /// matrix that maps depth to `0..1` (`x` and `y` in `-1..1`).
    pub fn from_view_proj(m: Mat4, ndc_min: Vec2, ndc_max: Vec2) -> Frustum {
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        let raw = [
            r0 - r3 * ndc_min.x,
            r3 * ndc_max.x - r0,
            r1 - r3 * ndc_min.y,
            r3 * ndc_max.y - r1,
            r2,
            r3 - r2,
        ];
        Frustum {
            planes: raw.map(|p| {
                let n = p.truncate().length();
                if n.is_finite() && n > 1e-12 {
                    p / n
                } else {
                    // A plane that admits everything.
                    Vec4::new(0.0, 0.0, 0.0, 1.0)
                }
            }),
        }
    }

    /// Whether the point is inside every plane.
    pub fn contains_point(&self, p: Vec3) -> bool {
        self.planes
            .iter()
            .all(|pl| pl.truncate().dot(p) + pl.w >= 0.0)
    }

    /// Whether the box may intersect the frustum: false only when it lies entirely outside one
    /// plane, so a box near a frustum edge can be reported although it misses.
    pub fn intersects_aabb(&self, b: &Aabb) -> bool {
        if !b.is_valid() {
            return false;
        }
        let (c, h) = (b.center(), b.half());
        self.planes.iter().all(|pl| {
            let n = pl.truncate();
            n.dot(c) + pl.w + n.abs().dot(h) >= 0.0
        })
    }

    /// Whether a box given by its corners may intersect the frustum (see [`Frustum::intersects_aabb`]).
    pub fn intersects_corners(&self, corners: &[Vec3; 8]) -> bool {
        self.planes.iter().all(|pl| {
            let n = pl.truncate();
            corners.iter().any(|&c| n.dot(c) + pl.w >= 0.0)
        })
    }

    /// Whether every corner is inside.
    pub fn contains_corners(&self, corners: &[Vec3; 8]) -> bool {
        corners.iter().all(|&c| self.contains_point(c))
    }
}

/// The ray parameter where the ray meets the plane through `point` with `normal`, if it is in
/// `0..=max_t` and the ray is not parallel to the plane.
pub fn ray_plane(origin: Vec3, dir: Vec3, point: Vec3, normal: Vec3, max_t: f32) -> Option<f32> {
    let denom = dir.dot(normal);
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = (point - origin).dot(normal) / denom;
    (t.is_finite() && (0.0..=max_t).contains(&t)).then_some(t)
}

/// The ray parameter of the first intersection with a sphere, if it is in `0..=max_t`.
pub fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32, max_t: f32) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - dir.length_squared() * c;
    if disc.is_nan() || disc < 0.0 {
        return None;
    }
    let a = dir.length_squared();
    let root = disc.sqrt();
    let t = [(-b - root) / a, (-b + root) / a]
        .into_iter()
        .find(|t| (0.0..=max_t).contains(t))?;
    t.is_finite().then_some(t)
}

/// The parameters of the closest points of a ray and a segment, and the distance between them:
/// `(ray t, segment s in 0..=1, distance)`. Parallel lines pick the segment start.
pub fn ray_segment_closest(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3) -> (f32, f32, f32) {
    let d2 = b - a;
    let r = origin - a;
    let (aa, ee) = (dir.length_squared(), d2.length_squared());
    let (bb, c, f) = (dir.dot(d2), dir.dot(r), d2.dot(r));
    if aa < 1e-12 {
        return (0.0, 0.0, r.length());
    }
    let denom = aa * ee - bb * bb;
    let mut t = if denom > 1e-12 * aa * ee {
        ((bb * f - c * ee) / denom).max(0.0)
    } else {
        0.0
    };
    let s = if ee > 1e-12 {
        ((bb * t + f) / ee).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // With the segment point fixed, the ray point follows.
    t = ((bb * s - c) / aa).max(0.0);
    let dist = (origin + dir * t - (a + d2 * s)).length();
    (t, s, dist)
}

/// A triangle hit: ray parameter and barycentric weights of the second and third vertex.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TriHit {
    /// Ray parameter.
    pub t: f32,
    /// Weight of the second vertex.
    pub u: f32,
    /// Weight of the third vertex.
    pub v: f32,
}

/// Ray against the front face of a triangle. The front face is the one whose normal
/// `(v1 - v0) x (v2 - v0)` points against the ray. Misses give `None`.
pub fn ray_triangle_front(
    origin: Vec3,
    dir: Vec3,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    max_t: f32,
) -> Option<TriHit> {
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    // `det = -dir . (e1 x e2)`, positive when the ray runs against the normal.
    if det <= 1e-12 {
        return None;
    }
    let s = origin - v0;
    let u = s.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) / det;
    (t.is_finite() && (0.0..=max_t).contains(&t)).then_some(TriHit { t, u, v })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_front_face_of_a_triangle_is_the_one_its_normal_points_out_of() {
        let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
        // The normal (b - a) x (c - a) is +Z: a ray travelling -Z hits it, one travelling +Z does not.
        let from_front = ray_triangle_front(Vec3::new(0.2, 0.2, 1.0), Vec3::NEG_Z, a, b, c, 10.0);
        assert!(from_front.is_some_and(|h| (h.t - 1.0).abs() < 1e-6));
        let from_back = ray_triangle_front(Vec3::new(0.2, 0.2, -1.0), Vec3::Z, a, b, c, 10.0);
        assert!(from_back.is_none());
    }

    #[test]
    fn a_ray_starting_inside_a_box_enters_at_zero() {
        let b = Aabb::from_corners(Vec3::splat(-1.0), Vec3::splat(1.0));
        let inv = inv_dir(Vec3::X);
        assert_eq!(b.ray_entry(Vec3::ZERO, inv, 5.0), Some(0.0));
        assert!(Aabb::EMPTY.ray_entry(Vec3::ZERO, inv, 5.0).is_none());
        // A ray along a face of the box still enters it.
        assert!(b.ray_entry(Vec3::new(-5.0, 1.0, 0.0), inv, 10.0).is_some());
    }

    #[test]
    fn a_plane_through_the_origin_never_admits_nan() {
        let f = Frustum::from_view_proj(Mat4::ZERO, Vec2::splat(-1.0), Vec2::ONE);
        assert!(f.planes.iter().all(|p| p.is_finite()));
    }
}
