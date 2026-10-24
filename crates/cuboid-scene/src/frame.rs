//! The placement of an element, a mesh or the blueprint in the world (spec: tools 1.3).

use cuboid_model::{MeshGeometry, ObjectBase};
use cuboid_num::euler_to_quat;
use glam::{Mat4, Quat, Vec3};

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

    #[test]
    fn objects_with_non_finite_numbers_have_no_frame() {
        let mut base = ObjectBase::new("a", Vec3::ZERO, Vec3::ONE, Perms::ELEMENT);
        base.p.x = f32::NAN;
        assert!(Frame::of(&base).is_none());
    }
}
