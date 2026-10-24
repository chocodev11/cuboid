//! The mapping between the element fields of a Minecraft Java model and the stored fields of an
//! element (spec: interop 2.1, 2.2, 2.3, 7.5, 7.6, 8.5; tools 1.3).
//!
//! The Minecraft model JSON, the Blockbench `.bbmodel` project and the later Java resource-pack
//! importers all describe a box by `from`, `to`, a rotation pivot (`origin`) and a rotation, with
//! face rectangles in texture units. This module turns those into the position `p`, size `s`,
//! pivot `o` and Euler rotation `e` of an element and back, so that every format maps in one way.
//! It parses and writes no text.
//!
//! - **Model space** `M` is the space of the JSON: `E(x, y, z) = M(x, y, -z)`.
//! - A rotation of the JSON is given as angles about X, Y and Z that are applied in the order
//!   X, then Y, then Z: `R = Rz(c) * Ry(b) * Rx(a)`. A single-axis rotation of the classic JSON
//!   is the special case with one non-zero angle.
//! - The stored position `p` is the rotated corner of the box (tools 1.3), so a rotated element
//!   is placed by rotating the box about its pivot.

use glam::{Mat3, Vec2, Vec3};

use crate::objects::{FaceFlags, FaceName, MIN_SIZE};
use cuboid_num::{euler_to_quat, quat_to_euler, round_he};

/// Face names of the JSON, in slot order.
pub const FACE_KEYS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// UV units of the Minecraft JSON: a face rectangle spans 0 to 16.
pub const JSON_UV_UNITS: Vec2 = Vec2::splat(16.0);

/// The JSON name of a face.
pub fn face_key(name: FaceName) -> &'static str {
    FACE_KEYS[name as usize]
}

/// The face a JSON name stands for.
pub fn face_from_key(key: &str) -> Option<FaceName> {
    FaceName::ALL
        .into_iter()
        .zip(FACE_KEYS)
        .find_map(|(face, name)| (name == key).then_some(face))
}

/// The cull direction a JSON `cullface` names. The alias `bottom` is the down side; any other
/// text means none.
pub fn cull_from_key(key: &str) -> FaceFlags {
    if key == "bottom" {
        return FaceFlags::DOWN;
    }
    face_from_key(key).map_or(FaceFlags::empty(), |face| {
        FaceFlags::from_bits_truncate(1 << (face as u8))
    })
}

/// The JSON name of a cull direction; `None` unless it is exactly one side.
pub fn cull_key(flags: FaceFlags) -> Option<&'static str> {
    FaceName::ALL
        .into_iter()
        .find(|&face| flags.bits() == 1 << (face as u8))
        .map(face_key)
}

fn flip_z(m: Mat3) -> Mat3 {
    Mat3::from_cols(
        Vec3::new(m.x_axis.x, m.x_axis.y, -m.x_axis.z),
        Vec3::new(m.y_axis.x, m.y_axis.y, -m.y_axis.z),
        Vec3::new(-m.z_axis.x, -m.z_axis.y, m.z_axis.z),
    )
}

/// Rounds to four decimals, so that the noise of the matrix products does not reach the fields.
fn clean(v: f32) -> f32 {
    (v * 10_000.0).round() / 10_000.0 + 0.0
}

fn clean3(v: Vec3) -> Vec3 {
    Vec3::new(clean(v.x), clean(v.y), clean(v.z))
}

/// The rotation matrix of angles about X, Y and Z in degrees, applied in that order.
pub fn rotation_m(deg: Vec3) -> Mat3 {
    if deg == Vec3::ZERO {
        return Mat3::IDENTITY;
    }
    Mat3::from_rotation_z(deg.z.to_radians())
        * Mat3::from_rotation_y(deg.y.to_radians())
        * Mat3::from_rotation_x(deg.x.to_radians())
}

/// The angles in degrees about X, Y and Z that [`rotation_m`] turns into `m`. At a quarter turn
/// about Y the split between X and Z is not unique; Z is then 0.
pub fn angles_m(m: Mat3) -> Vec3 {
    let (r00, r10, r20) = (m.x_axis.x, m.x_axis.y, m.x_axis.z);
    let (r11, r21) = (m.y_axis.y, m.y_axis.z);
    let (r12, r22) = (m.z_axis.y, m.z_axis.z);
    let y = (-r20).clamp(-1.0, 1.0).asin();
    let (x, z) = if r20.abs() < 0.999_999 {
        (r21.atan2(r22), r10.atan2(r00))
    } else {
        ((-r12).atan2(r11), 0.0)
    };
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

/// A rigid motion of model space: `p -> rot * p + pos`. It is the combined transform of the
/// groups that enclose an element in a format with a hierarchy.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rigid {
    /// The rotation.
    pub rot: Mat3,
    /// The translation after the rotation.
    pub pos: Vec3,
}

impl Rigid {
    /// The motion that leaves every point where it is.
    pub const IDENTITY: Rigid = Rigid {
        rot: Mat3::IDENTITY,
        pos: Vec3::ZERO,
    };

    /// The rotation by the angles `deg` (see [`rotation_m`]) about the point `pivot`.
    pub fn about(pivot: Vec3, deg: Vec3) -> Rigid {
        let rot = rotation_m(deg);
        Rigid {
            rot,
            pos: pivot - rot * pivot,
        }
    }

    /// The motion that does `inner` first and `self` after it.
    pub fn then(self, inner: Rigid) -> Rigid {
        Rigid {
            rot: self.rot * inner.rot,
            pos: self.rot * inner.pos + self.pos,
        }
    }

    /// Moves a point.
    pub fn apply(&self, p: Vec3) -> Vec3 {
        self.rot * p + self.pos
    }
}

/// The box of one element as the JSON gives it, in model space.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct McBox {
    /// The `from` corner.
    pub from: Vec3,
    /// The `to` corner. It may lie below `from`: the box is then inverted on that axis.
    pub to: Vec3,
    /// The pivot of the element's own rotation.
    pub pivot: Vec3,
    /// The element's own rotation, angles about X, Y and Z in degrees.
    pub rotation: Vec3,
}

/// The stored fields of an element.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Placed {
    /// The stored position: the rotated `from` corner (the min-x, min-y, max-z corner for a box
    /// that is not inverted).
    pub p: Vec3,
    /// The signed size `to - from`. A zero extent is raised to [`MIN_SIZE`]; a negative one is an
    /// inverted axis (the box is drawn inside out there) and stays negative.
    pub s: Vec3,
    /// The pivot.
    pub o: Vec3,
    /// The Euler rotation in degrees, as `quat_to_euler` returns it (components from 0 to 360).
    pub e: Vec3,
}

/// Stores a box that is moved by `outer` after its own rotation. `outer` is the identity for a
/// plain Minecraft element and the transform of the enclosing groups for a hierarchy that is
/// baked into the element.
///
/// The size is `to - from` with a zero extent raised to [`MIN_SIZE`]; a negative extent is kept,
/// because the loaders store sizes as the file has them (native 6.3). The position is the
/// rotated `from` corner `outer(rotation about pivot (from))`, the pivot is the moved pivot.
///
/// spec: interop 8.5, tools 1.3, tools 3.4
pub fn place(b: &McBox, outer: Rigid) -> Placed {
    let s = (b.to - b.from).map(|c| if c == 0.0 { MIN_SIZE } else { c });
    let total = outer.then(Rigid::about(b.pivot, b.rotation));
    let corner = total.apply(b.from);
    let pivot = outer.apply(b.pivot);
    let e = if total.rot == Mat3::IDENTITY {
        Vec3::ZERO
    } else {
        let q = glam::Quat::from_mat3(&flip_z(total.rot));
        quat_to_euler(q).map(clean)
    };
    Placed {
        p: clean3(Vec3::new(corner.x, corner.y, -corner.z)),
        s,
        o: clean3(Vec3::new(pivot.x, pivot.y, -pivot.z)),
        e,
    }
}

/// The inverse of [`place`] with the identity as `outer`: the box, pivot and rotation of the
/// JSON for the stored fields. `from` is the unrotated corner (interop 2.2.2); with `invert` the
/// two corners are swapped. The result is cleaned to four decimals.
///
/// The rotation is the general form with three angles; [`single_axis`] gives the classic form.
///
/// spec: interop 2.2.2, interop 7.5
pub fn unplace(p: Vec3, s: Vec3, o: Vec3, e: Vec3, invert: bool) -> McBox {
    let rot = flip_z(Mat3::from_quat(euler_to_quat(e)));
    let pivot = Vec3::new(o.x, o.y, -o.z);
    let corner = Vec3::new(p.x, p.y, -p.z);
    let from = rot.transpose() * (corner - pivot) + pivot;
    let to = from + s;
    let (from, to) = if invert { (to, from) } else { (from, to) };
    McBox {
        from: clean3(from),
        to: clean3(to),
        pivot: clean3(pivot),
        rotation: if rot == Mat3::IDENTITY {
            Vec3::ZERO
        } else {
            angles_m(rot).map(clean)
        },
    }
}

/// The classic JSON form of a rotation: the axis letter and the angle, when at most one angle
/// is not zero. The identity gives the axis `y` and 0.
pub fn single_axis(rotation: Vec3) -> Option<(char, f32)> {
    match (rotation.x != 0.0, rotation.y != 0.0, rotation.z != 0.0) {
        (false, false, false) => Some(('y', 0.0)),
        (true, false, false) => Some(('x', rotation.x)),
        (false, true, false) => Some(('y', rotation.y)),
        (false, false, true) => Some(('z', rotation.z)),
        _ => None,
    }
}

/// Whether the stored rotation `e` can be written as the classic JSON rotation of Minecraft: one
/// axis, an angle of 0, 22.5, 45, 315 or 337.5 degrees.
///
/// spec: interop 2.2.2 (invalid for Minecraft)
pub fn rotation_fits_json(e: Vec3) -> bool {
    let parts = e.to_array();
    if parts.iter().filter(|c| **c != 0.0).count() > 1 {
        return false;
    }
    parts.iter().all(|&a| {
        let between = a > 45.0 && a < 315.0;
        let on_grid = round_he(a / 22.5) * 22.5 == a;
        !between && on_grid
    })
}

/// The face rectangle of the JSON `uv: [u0, v0, u1, v1]` in texture units, with `units` the
/// size of the texture space (16 x 16 for Minecraft), as the corners `(tl, br)` of a face: Y up,
/// not clamped. Mirrored rectangles keep their order.
///
/// spec: interop 8.5 (faces, uv), interop 2.3
pub fn uv_from_units(uv: [f32; 4], units: Vec2) -> (Vec2, Vec2) {
    (
        Vec2::new(uv[0] / units.x, 1.0 - uv[3] / units.y),
        Vec2::new(uv[2] / units.x, 1.0 - uv[1] / units.y),
    )
}

/// The inverse of [`uv_from_units`]: the corners of a face as `[u0, v0, u1, v1]`.
///
/// spec: interop 2.3 (16-unit forms)
pub fn uv_to_units(tl: Vec2, br: Vec2, units: Vec2) -> [f32; 4] {
    [
        tl.x * units.x,
        (1.0 - br.y) * units.y,
        br.x * units.x,
        (1.0 - tl.y) * units.y,
    ]
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.abs_diff_eq(b, 2e-3)
    }

    #[test]
    fn an_unrotated_box_is_the_flipped_corner() {
        let b = McBox {
            from: v(2.0, 3.0, 4.0),
            to: v(6.0, 5.0, 10.0),
            pivot: v(8.0, 8.0, 8.0),
            rotation: Vec3::ZERO,
        };
        let placed = place(&b, Rigid::IDENTITY);
        assert_eq!(placed.p, v(2.0, 3.0, -4.0));
        assert_eq!(placed.s, v(4.0, 2.0, 6.0));
        assert_eq!(placed.o, v(8.0, 8.0, -8.0));
        assert_eq!(placed.e, Vec3::ZERO);
    }

    #[test]
    fn a_single_axis_rotation_negates_x_and_y_and_keeps_z() {
        let at = |rotation| {
            place(
                &McBox {
                    from: Vec3::ZERO,
                    to: Vec3::ONE,
                    pivot: Vec3::ZERO,
                    rotation,
                },
                Rigid::IDENTITY,
            )
            .e
        };
        assert_eq!(at(v(22.5, 0.0, 0.0)), v(337.5, 0.0, 0.0));
        assert_eq!(at(v(0.0, -45.0, 0.0)), v(0.0, 45.0, 0.0));
        assert_eq!(at(v(0.0, 0.0, 22.5)), v(0.0, 0.0, 22.5));
    }

    #[test]
    fn the_corner_is_rotated_about_the_pivot() {
        // A quarter turn about +Y in the JSON maps the offset (-8, 0, 0) of the corner to
        // (0, 0, 8), so the corner of a box at the origin with pivot (8, 0, 0) lands on z = 8.
        let placed = place(
            &McBox {
                from: Vec3::ZERO,
                to: Vec3::ONE,
                pivot: v(8.0, 0.0, 0.0),
                rotation: v(0.0, 90.0, 0.0),
            },
            Rigid::IDENTITY,
        );
        assert!(near(placed.p, v(8.0, 0.0, -8.0)), "{:?}", placed.p);
        assert!(near(placed.e, v(0.0, 270.0, 0.0)), "{:?}", placed.e);
    }

    #[test]
    fn an_inverted_box_keeps_its_negative_size() {
        let b = McBox {
            from: v(4.0, 0.0, 6.0),
            to: v(0.0, 2.0, 1.0),
            pivot: Vec3::ZERO,
            rotation: Vec3::ZERO,
        };
        let placed = place(&b, Rigid::IDENTITY);
        assert_eq!(placed.s, v(-4.0, 2.0, -5.0));
        assert_eq!(placed.p, v(4.0, 0.0, -6.0));
        let back = unplace(placed.p, placed.s, placed.o, placed.e, false);
        assert_eq!((back.from, back.to), (b.from, b.to));
    }

    #[test]
    fn a_flat_box_gets_the_minimum_thickness() {
        let placed = place(
            &McBox {
                from: Vec3::ZERO,
                to: v(16.0, 16.0, 0.0),
                pivot: Vec3::ZERO,
                rotation: Vec3::ZERO,
            },
            Rigid::IDENTITY,
        );
        assert_eq!(placed.s, v(16.0, 16.0, MIN_SIZE));
    }

    #[test]
    fn an_outer_motion_moves_the_corner_and_the_pivot() {
        let outer = Rigid::about(v(8.0, 8.0, 8.0), v(0.0, 0.0, 90.0));
        let placed = place(
            &McBox {
                from: v(8.0, 8.0, 8.0),
                to: v(9.0, 9.0, 9.0),
                pivot: v(8.0, 8.0, 8.0),
                rotation: Vec3::ZERO,
            },
            outer,
        );
        // The corner lies on the pivot of the outer motion, so it stays; the box turns.
        assert!(near(placed.p, v(8.0, 8.0, -8.0)));
        assert!(near(placed.e, v(0.0, 0.0, 90.0)));
        assert!(near(placed.o, v(8.0, 8.0, -8.0)));
    }

    #[test]
    fn json_validity_follows_the_rules_of_interop_2_2_2() {
        for ok in [
            v(0.0, 0.0, 0.0),
            v(22.5, 0.0, 0.0),
            v(0.0, 45.0, 0.0),
            v(0.0, 0.0, 315.0),
            v(337.5, 0.0, 0.0),
        ] {
            assert!(rotation_fits_json(ok), "{ok:?}");
        }
        for bad in [
            v(10.0, 0.0, 0.0),
            v(0.0, 67.5, 0.0),
            v(22.5, 22.5, 0.0),
            v(0.0, 180.0, 0.0),
        ] {
            assert!(!rotation_fits_json(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_json_name_of_a_cull_direction_round_trips() {
        for face in FaceName::ALL {
            let flags = FaceFlags::from_bits_truncate(1 << (face as u8));
            assert_eq!(cull_from_key(face_key(face)), flags);
            assert_eq!(cull_key(flags), Some(face_key(face)));
        }
        assert_eq!(cull_from_key("bottom"), FaceFlags::DOWN);
        assert_eq!(cull_from_key("sideways"), FaceFlags::empty());
        assert_eq!(cull_key(FaceFlags::ALL), None);
    }

    #[test]
    fn uv_units_follow_the_rules_of_interop_2_3() {
        let (tl, br) = uv_from_units([2.0, 4.0, 10.0, 8.0], JSON_UV_UNITS);
        assert_eq!((tl, br), (Vec2::new(0.125, 0.5), Vec2::new(0.625, 0.75)));
        assert_eq!(uv_to_units(tl, br, JSON_UV_UNITS), [2.0, 4.0, 10.0, 8.0]);
        // A texture space of 64 x 32 units.
        let units = Vec2::new(64.0, 32.0);
        let (tl, br) = uv_from_units([0.0, 0.0, 16.0, 16.0], units);
        assert_eq!((tl, br), (Vec2::new(0.0, 0.5), Vec2::new(0.25, 1.0)));
    }

    #[test]
    fn angles_of_a_rotation_come_back() {
        for deg in [
            v(30.0, 0.0, 0.0),
            v(0.0, -60.0, 0.0),
            v(0.0, 0.0, 12.5),
            v(20.0, 40.0, -70.0),
            v(-10.0, 90.0, 0.0),
        ] {
            let back = angles_m(rotation_m(deg));
            assert!(near(back, deg), "{deg:?} -> {back:?}");
        }
    }

    proptest! {
        #[test]
        fn place_and_unplace_agree(
            from in prop::array::uniform3(-16.0f32..32.0),
            size in prop::array::uniform3(0.5f32..16.0),
            pivot in prop::array::uniform3(-16.0f32..32.0),
            rot in prop::array::uniform3(-170.0f32..170.0),
        ) {
            let from = Vec3::from_array(from);
            let b = McBox {
                from,
                to: from + Vec3::from_array(size),
                pivot: Vec3::from_array(pivot),
                rotation: Vec3::from_array(rot),
            };
            let placed = place(&b, Rigid::IDENTITY);
            let back = unplace(placed.p, placed.s, placed.o, placed.e, false);
            prop_assert!(near(back.from, b.from), "{:?} {:?}", back.from, b.from);
            prop_assert!(near(back.to, b.to));
            prop_assert!(near(back.pivot, b.pivot));
            // The rotation is the same one, perhaps written with other angles.
            let a = rotation_m(back.rotation);
            let c = rotation_m(b.rotation);
            prop_assert!(a.abs_diff_eq(c, 2e-3));
        }
    }
}
