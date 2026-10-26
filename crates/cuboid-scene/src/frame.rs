//! The placement of an element, a mesh or the blueprint in the world (spec: tools 1.3).

use cuboid_model::{MeshGeometry, ObjectBase};
use cuboid_num::euler_to_quat;
use glam::{Affine3A, Mat3, Mat4, Quat, Vec3};

use crate::geom::{Aabb, Obb};

/// Local to world: `p + R * (v * s)`. The unit box of an object is `x` in `0..1`, `y` in `0..1`
/// and `z` in `-1..0`, so the position is its corner with the smallest `x` and `y` and the
/// largest `z`, after the rotation.
///
/// The size keeps its sign. A negative component mirrors the box on that axis (an inverted cube,
/// `to < from` in Minecraft), which also reverses the winding of its faces, so back-face culling
/// then shows the inner side. Boxes and bounds use the min and max corners, not the sign.
///
/// spec: tools 1.3
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Frame {
    /// The position `p`.
    pub pos: Vec3,
    /// The rotation `R` built from the Euler angles.
    pub rot: Quat,
    /// The inverse of `rot`.
    pub inv_rot: Quat,
    /// The signed size `s`.
    pub size: Vec3,
}

impl Frame {
    /// The frame of an object, `None` when its position, size or angles are not finite.
    pub fn of(base: &ObjectBase) -> Option<Frame> {
        if !(base.p.is_finite() && base.s.is_finite() && base.e.is_finite()) {
            return None;
        }
        let rot = if base.e == Vec3::ZERO {
            Quat::IDENTITY
        } else {
            euler_to_quat(base.e)
        };
        Some(Frame {
            pos: base.p,
            rot,
            inv_rot: rot.conjugate(),
            size: base.s,
        })
    }

    /// The same placement with the absolute size, for objects that draw their own geometry (a
    /// mesh keeps its scale as stored, but is never mirrored).
    pub fn unsigned(self) -> Frame {
        Frame {
            size: self.size.abs(),
            ..self
        }
    }

    /// The placement after the motion `delta` of a pose carried the object from the rest pose: the
    /// position moves with it and the box turns and stretches. A motion that shears the box (a
    /// scale of a group about axes the box does not follow) leaves the nearest box: the one whose
    /// edges are the lengths of the carried edges, orthogonal, the first edge kept.
    pub fn posed(&self, delta: &Affine3A) -> Frame {
        let pos = delta.transform_point3(self.pos);
        let linear =
            Mat3::from(delta.matrix3) * Mat3::from_quat(self.rot) * Mat3::from_diagonal(self.size);
        let [c0, c1, c2] = [linear.x_axis, linear.y_axis, linear.z_axis];
        let (s0, x) = (c0.length(), c0.normalize_or_zero());
        let off = c1 - x * x.dot(c1);
        let (s1, y) = (off.length(), off.normalize_or_zero());
        if !(pos.is_finite() && s0 > f32::EPSILON && s1 > f32::EPSILON) {
            return Frame { pos, ..*self };
        }
        let z = x.cross(y);
        let rot = Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize();
        Frame {
            pos,
            rot,
            inv_rot: rot.conjugate(),
            size: Vec3::new(s0, s1, c2.dot(z)),
        }
    }

    /// A point of the unit box in world space.
    pub fn to_world(&self, unit: Vec3) -> Vec3 {
        self.pos + self.rot * (unit * self.size)
    }

    /// The matrix of `to_world`.
    pub fn model_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.size, self.rot, self.pos)
    }

    /// The oriented box of the whole unit box.
    pub fn obb(&self) -> Obb {
        Obb {
            center: self.to_world(Vec3::new(0.5, 0.5, -0.5)),
            rot: self.rot,
            half: self.size.abs() * 0.5,
        }
    }
}

/// The box of a mesh's geometry in its local frame; the unit box when the geometry is empty or
/// not usable (a primitive whose geometry is not generated yet).
pub fn geometry_bounds(geometry: &MeshGeometry) -> Aabb {
    let b = geometry
        .vertices
        .iter()
        .fold(Aabb::EMPTY, |b, &v| b.with_point(v));
    if b.is_valid() && geometry.triangles.len() >= 3 {
        b
    } else {
        Aabb::from_corners(Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use cuboid_model::{ObjectBase, Perms};

    use super::*;

    #[test]
    fn the_position_is_the_corner_with_the_largest_z_after_the_rotation() {
        let mut base = ObjectBase::new(
            "a",
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(2.0, 4.0, 6.0),
            Perms::ELEMENT,
        );
        let frame = Frame::of(&base).unwrap();
        assert_eq!(frame.to_world(Vec3::ZERO), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(
            frame.to_world(Vec3::new(1.0, 1.0, -1.0)),
            Vec3::new(3.0, 6.0, -3.0)
        );
        assert!((frame.obb().center - Vec3::new(2.0, 4.0, 0.0)).length() < 1e-6);
        // A quarter turn about Y moves the box's +X edge onto -Z.
        base.e = Vec3::new(0.0, 90.0, 0.0);
        let turned = Frame::of(&base).unwrap();
        assert!(
            (turned.to_world(Vec3::X) - (Vec3::new(1.0, 2.0, 3.0) + Vec3::new(0.0, 0.0, -2.0)))
                .length()
                < 1e-4
        );
    }

    #[test]
    fn a_negative_size_keeps_its_sign_and_the_box_its_corners() {
        let mut base = ObjectBase::new(
            "a",
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(-2.0, 4.0, -6.0),
            Perms::ELEMENT,
        );
        base.e = Vec3::ZERO;
        let frame = Frame::of(&base).unwrap();
        assert_eq!(frame.size, Vec3::new(-2.0, 4.0, -6.0));
        assert_eq!(frame.unsigned().size, Vec3::new(2.0, 4.0, 6.0));
        // x spans -1..1, y 2..6, z 3..9 (z is mirrored too).
        let obb = frame.obb();
        assert!((obb.center - Vec3::new(0.0, 4.0, 6.0)).length() < 1e-6);
        assert_eq!(obb.half, Vec3::new(1.0, 2.0, 3.0));
        let aabb = obb.aabb();
        assert!((aabb.min - Vec3::new(-1.0, 2.0, 3.0)).length() < 1e-6);
        assert!((aabb.max - Vec3::new(1.0, 6.0, 9.0)).length() < 1e-6);
        assert!(
            frame.model_matrix().determinant() > 0.0,
            "two mirrors cancel"
        );
        base.s.z = 6.0;
        assert!(Frame::of(&base).unwrap().model_matrix().determinant() < 0.0);
    }

    fn frame(p: Vec3, s: Vec3, e: Vec3) -> Frame {
        let mut base = ObjectBase::new("a", p, s, Perms::ELEMENT);
        base.e = e;
        Frame::of(&base).unwrap()
    }

    /// The corners of the unit box in world space.
    fn corners(f: &Frame) -> Vec<Vec3> {
        (0..8_u8)
            .map(|i| {
                f.to_world(Vec3::new(
                    f32::from(i & 1),
                    f32::from(i >> 1 & 1),
                    -f32::from(i >> 2 & 1),
                ))
            })
            .collect()
    }

    fn same_box(a: &Frame, b: &Frame) -> bool {
        corners(a)
            .iter()
            .zip(corners(b))
            .all(|(p, q)| p.abs_diff_eq(q, 1e-4))
    }

    #[test]
    fn a_pose_carries_the_box_with_it() {
        let rest = frame(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 3.0),
            Vec3::ZERO,
        );
        // A turn of 90 degrees about Z through the origin, then a move.
        let delta = Affine3A::from_translation(Vec3::new(0.0, 0.0, 5.0))
            * Affine3A::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let posed = rest.posed(&delta);
        let expected: Vec<Vec3> = corners(&rest)
            .into_iter()
            .map(|p| delta.transform_point3(p))
            .collect();
        for (got, want) in corners(&posed).iter().zip(&expected) {
            assert!(got.abs_diff_eq(*want, 1e-4), "{got:?} {want:?}");
        }
        assert!(posed.size.abs_diff_eq(Vec3::new(2.0, 1.0, 3.0), 1e-5));
        assert!(posed.pos.abs_diff_eq(Vec3::new(0.0, 1.0, 5.0), 1e-5));
        // The identity leaves the box where it is.
        assert!(same_box(&rest.posed(&Affine3A::IDENTITY), &rest));
    }

    #[test]
    fn a_scale_stretches_the_box_along_its_own_edges_and_a_mirror_keeps_the_box() {
        let rest = frame(Vec3::ZERO, Vec3::ONE, Vec3::new(0.0, 30.0, 0.0));
        let scaled = rest.posed(&Affine3A::from_scale(Vec3::splat(2.0)));
        assert!(scaled.size.abs_diff_eq(Vec3::splat(2.0), 1e-5));
        assert!(same_box(
            &scaled,
            &frame(Vec3::ZERO, Vec3::splat(2.0), Vec3::new(0.0, 30.0, 0.0))
        ));
        // A scale of -1 on X mirrors the box: the same corners, a flipped winding.
        let mirrored = rest.posed(&Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!(mirrored.model_matrix().determinant() < 0.0);
        let want: Vec<Vec3> = corners(&rest)
            .into_iter()
            .map(|p| Vec3::new(-p.x, p.y, p.z))
            .collect();
        for (got, want) in corners(&mirrored).iter().zip(&want) {
            assert!(got.abs_diff_eq(*want, 1e-4), "{got:?} {want:?}");
        }
    }

    #[test]
    fn a_shear_leaves_a_box_with_the_carried_lengths_and_no_non_finite_numbers() {
        let rest = frame(Vec3::ZERO, Vec3::ONE, Vec3::new(0.0, 0.0, 45.0));
        // A scale of X in the world axes shears a box that is turned 45 degrees about Z.
        let posed = rest.posed(&Affine3A::from_scale(Vec3::new(3.0, 1.0, 1.0)));
        assert!(posed.size.is_finite() && posed.rot.is_finite());
        assert!(posed.size.x > 1.0 && posed.size.z > 0.99);
        assert!((posed.rot.length() - 1.0).abs() < 1e-5);
        // A collapse to a point keeps the placement.
        let flat = rest.posed(&Affine3A::from_scale(Vec3::ZERO));
        assert!(flat.pos.is_finite() && flat.size == rest.size);
    }

    #[test]
    fn objects_with_non_finite_numbers_have_no_frame() {
        let mut base = ObjectBase::new("a", Vec3::ZERO, Vec3::ONE, Perms::ELEMENT);
        base.p.x = f32::NAN;
        assert!(Frame::of(&base).is_none());
    }
}
