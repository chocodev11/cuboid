//! Gizmo placement, geometry, analytic hit volumes and drag projection (spec: tools 9.1-9.7; arch
//! 13.7).
//!
//! All sizes are multiples of the view factor `f`, a fixed share of the half-height of the view
//! at the gizmo, so a gizmo keeps its size on the screen at any zoom. Hit volumes mirror the
//! colliders of the original: a capsule per move arrow, a thin square per move plane, the ring
//! band, spheres at the scale handles.

use cuboid_model::{FaceName, ModelBox};
use cuboid_num::quat_to_euler;
use glam::{Quat, Vec3};

use crate::camera::Camera;
use crate::geom::{ray_plane, ray_segment_closest, ray_sphere, unit_or};
use crate::views::ViewId;
use cuboid_voxel::Ray;

/// The view factor `f` as a share of the half-height of the view at the gizmo.
pub const VIEW_FACTOR: f32 = 0.2;

/// Move arrow length in units of `f`.
const MOVE_SCALE: f32 = 0.6;
/// Rotation ring size in units of `f`: the ring's mid radius is `RING_SCALE * 1.9`.
const RING_SCALE: f32 = 0.7;
/// Scale handle size in units of `f`.
const SCALE_SCALE: f32 = 0.3;
/// Model-box handle scale (fixed, not view dependent).
const BOX_HANDLE_SCALE: f32 = 1.5;

/// Which transform gizmo is shown, following the select mode.
///
/// spec: tools 9.1
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum GizmoKind {
    /// Arrows and plane squares at the primary's origin.
    Move,
    /// Three rings and an axis triad at the primary's origin.
    Rotate,
    /// Six face handles around the primary's box.
    Scale,
}

/// Axis flags of a handle: x 1, y 2, z 4, and their unions.
///
/// spec: tools 8.1, tools 9.7 (Hit areas)
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Axes {
    /// The X axis.
    pub x: bool,
    /// The Y axis.
    pub y: bool,
    /// The Z axis.
    pub z: bool,
}

impl Axes {
    /// No axis.
    pub const NONE: Axes = Axes {
        x: false,
        y: false,
        z: false,
    };
    /// X.
    pub const X: Axes = Axes {
        x: true,
        y: false,
        z: false,
    };
    /// Y.
    pub const Y: Axes = Axes {
        x: false,
        y: true,
        z: false,
    };
    /// Z.
    pub const Z: Axes = Axes {
        x: false,
        y: false,
        z: true,
    };
    /// The XY plane.
    pub const XY: Axes = Axes {
        x: true,
        y: true,
        z: false,
    };
    /// The XZ plane.
    pub const XZ: Axes = Axes {
        x: true,
        y: false,
        z: true,
    };
    /// The YZ plane.
    pub const YZ: Axes = Axes {
        x: false,
        y: true,
        z: true,
    };

    /// The single axis with this index (0 X, 1 Y, 2 Z).
    pub fn single(axis: usize) -> Axes {
        Axes {
            x: axis == 0,
            y: axis == 1,
            z: axis == 2,
        }
    }

    /// The flags as bits: x 1, y 2, z 4.
    pub fn bits(self) -> u8 {
        u8::from(self.x) | u8::from(self.y) << 1 | u8::from(self.z) << 2
    }

    /// The flags as a 0/1 mask vector, `x -> (1,0,0)`.
    pub fn mask(self) -> Vec3 {
        Vec3::new(
            f32::from(u8::from(self.x)),
            f32::from(u8::from(self.y)),
            f32::from(u8::from(self.z)),
        )
    }

    /// Whether the axis with this index is set.
    pub fn has(self, axis: usize) -> bool {
        self.bits() >> axis & 1 == 1
    }
}

/// What the picking needs to know about the gizmos currently shown. An empty `kind` and no
/// handles mean no gizmo; with no primary all gizmos are hidden.
///
/// For [`GizmoKind::Move`] and [`GizmoKind::Rotate`] `origin` is the primary's origin `o`; for
/// [`GizmoKind::Scale`] it is the primary's position `p`, the corner the scale root hangs on.
///
/// spec: tools 9.1, tools 9.7, arch 13.7, arch 6 (cuboid-scene)
#[derive(Clone, Debug)]
pub struct GizmoState {
    /// The transform gizmo shown, if any.
    pub kind: Option<GizmoKind>,
    /// The primary's origin, where move and rotate gizmos sit; for the scale gizmo the primary's
    /// position.
    pub origin: Vec3,
    /// The primary's rotation.
    pub rotation: Quat,
    /// The primary's size.
    pub size: Vec3,
    /// The primary's scale gizmo extent multiplier.
    pub gizmo_size: Vec3,
    /// The primary's scale gizmo offset.
    pub gizmo_pivot: Vec3,
    /// The model box whose six handles are shown, if its handles are on.
    pub box_handles: Option<ModelBox>,
}

/// The part of a gizmo a ray hit.
///
/// spec: tools 9.7 (Hit areas)
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum GizmoPart {
    /// A move arrow (sphere collider).
    MoveArrow,
    /// A move plane square (flat box collider).
    MovePlane,
    /// A rotation ring (the ring mesh).
    RotateRing,
    /// A scale arrow (sphere collider).
    ScaleArrow,
    /// A model-box handle on the given face of the box; only these start a handle press.
    BoxHandle(FaceName),
}

/// A ray hit on a gizmo.
///
/// spec: tools 8.3 (step 1), tools 9.7, arch 6 (cuboid-scene)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GizmoHit {
    /// Which part.
    pub part: GizmoPart,
    /// The axes the part drags; they become the active axis.
    pub axes: Axes,
    /// Where the ray hit.
    pub point: Vec3,
    /// Distance from the ray origin.
    pub distance: f32,
}

/// The direction the move arrow of an axis points: X `+X`, Y `+Y`, Z `-Z` (Minecraft `+Z`).
pub fn arrow_dir(axis: usize) -> Vec3 {
    match axis {
        0 => Vec3::X,
        1 => Vec3::Y,
        _ => Vec3::NEG_Z,
    }
}

/// The view factor `f` of a gizmo at `origin`: it scales every gizmo size so the gizmo keeps its
/// size on the screen.
///
/// spec: tools 9.1
pub fn gizmo_scale(camera: &Camera, origin: Vec3) -> f32 {
    camera.half_height_at(origin) * VIEW_FACTOR
}

/// The axes of the three rotation rings for an object rotation: the Y ring turns about the world
/// Y axis, the X ring about X after the yaw, the Z ring about Z after yaw and pitch, which are the
/// axes of the Euler angles the rotation setter edits (`q = qY * qX * qZ`).
///
/// spec: tools 9.1, tools 3.4
pub fn ring_axes(rotation: Quat) -> [Vec3; 3] {
    let euler = quat_to_euler(rotation);
    if !euler.is_finite() {
        return [Vec3::X, Vec3::Y, Vec3::Z];
    }
    let yaw = Quat::from_rotation_y(euler.y.to_radians());
    let pitch = Quat::from_rotation_x(euler.x.to_radians());
    [yaw * Vec3::X, Vec3::Y, yaw * pitch * Vec3::Z]
}

/// Whether the axis is the view direction of an orthographic view, where its handles are hidden.
fn along_view(view: ViewId, axis: usize) -> bool {
    matches!(
        (view, axis),
        (ViewId::Top, 1) | (ViewId::Side, 0) | (ViewId::Front, 2)
    )
}

/// The outward direction of a model-box face in world axes and the axis index.
fn face_axis(face: FaceName) -> (usize, f32) {
    match face {
        FaceName::Down => (1, -1.0),
        FaceName::Up => (1, 1.0),
        FaceName::North => (2, 1.0),
        FaceName::South => (2, -1.0),
        FaceName::West => (0, -1.0),
        FaceName::East => (0, 1.0),
    }
}

/// Keeps the nearer of two hits.
fn nearer(best: &mut Option<GizmoHit>, hit: GizmoHit) {
    if best.as_ref().is_none_or(|b| hit.distance < b.distance) {
        *best = Some(hit);
    }
}

/// The nearest gizmo part under a ray with a unit direction, for the gizmos of `state` as seen
/// by the camera of `view`.
///
/// spec: tools 9.7 (Hit areas, visibility per view)
pub fn pick(state: &GizmoState, camera: &Camera, view: ViewId, ray: Ray) -> Option<GizmoHit> {
    let mut best = None;
    if let Some(b) = &state.box_handles {
        pick_box_handles(b, view, ray, &mut best);
    }
    if !(state.origin.is_finite() && state.size.is_finite()) {
        return best;
    }
    let f = gizmo_scale(camera, state.origin);
    match state.kind {
        Some(GizmoKind::Move) => pick_move(state.origin, f * MOVE_SCALE, view, ray, &mut best),
        Some(GizmoKind::Rotate) => pick_rotate(state, f * RING_SCALE, ray, &mut best),
        Some(GizmoKind::Scale) => pick_scale(state, f * SCALE_SCALE, view, ray, &mut best),
        None => {}
    }
    best
}

fn pick_move(origin: Vec3, s: f32, view: ViewId, ray: Ray, best: &mut Option<GizmoHit>) {
    for axis in 0..3 {
        if along_view(view, axis) {
            continue;
        }
        let dir = arrow_dir(axis);
        let (a, b) = (origin + dir * (0.2 * s), origin + dir * (1.15 * s));
        let (t, _, dist) = ray_segment_closest(ray.origin, ray.dir, a, b);
        if dist <= 0.25 * s && t > 0.0 {
            nearer(
                best,
                GizmoHit {
                    part: GizmoPart::MoveArrow,
                    axes: Axes::single(axis),
                    point: ray.origin + ray.dir * t,
                    distance: t,
                },
            );
        }
    }
    // Plane squares between two axes, drawn only when neither is the view direction... the one
    // square of an orthographic view is the one whose normal is the view direction.
    for (u, v, axes) in [(0, 1, Axes::XY), (0, 2, Axes::XZ), (1, 2, Axes::YZ)] {
        let normal_axis = 3 - u - v;
        let visible = match view {
            ViewId::Main => true,
            _ => along_view(view, normal_axis),
        };
        if !visible {
            continue;
        }
        let normal = arrow_dir(normal_axis);
        let Some(t) = ray_plane(ray.origin, ray.dir, origin, normal, f32::MAX) else {
            continue;
        };
        let p = ray.origin + ray.dir * t;
        let rel = p - origin;
        let (cu, cv) = (rel.dot(arrow_dir(u)), rel.dot(arrow_dir(v)));
        let range = 0.1 * s..=0.6 * s;
        if range.contains(&cu) && range.contains(&cv) {
            nearer(
                best,
                GizmoHit {
                    part: GizmoPart::MovePlane,
                    axes,
                    point: p,
                    distance: t,
                },
            );
        }
    }
}

fn pick_rotate(state: &GizmoState, k: f32, ray: Ray, best: &mut Option<GizmoHit>) {
    let (radius, tolerance) = (1.9 * k, 0.3 * k);
    let axes = ring_axes(state.rotation);
    for (axis, ring_axis) in axes.into_iter().enumerate() {
        let Some(t) = ray_plane(ray.origin, ray.dir, state.origin, ring_axis, f32::MAX) else {
            continue;
        };
        let p = ray.origin + ray.dir * t;
        if ((p - state.origin).length() - radius).abs() <= tolerance {
            nearer(
                best,
                GizmoHit {
                    part: GizmoPart::RotateRing,
                    axes: Axes::single(axis),
                    point: p,
                    distance: t,
                },
            );
        }
    }
}

fn pick_scale(state: &GizmoState, h: f32, view: ViewId, ray: Ray, best: &mut Option<GizmoHit>) {
    let extent = state.size * state.gizmo_size;
    let root = state.origin + state.rotation * (state.gizmo_pivot * state.size);
    for face in FaceName::ALL {
        let (axis, sign) = face_axis(face);
        if along_view(view, axis) {
            continue;
        }
        // Face centre of the unit box x 0..1, y 0..1, z -1..0.
        let mut centre = Vec3::new(0.5, 0.5, -0.5);
        centre[axis] += 0.5 * sign;
        // A negative size mirrors the handle with the box; it still points out of its face.
        let mut dir = Vec3::ZERO;
        dir[axis] = if extent[axis] < 0.0 { -sign } else { sign };
        let sphere = root + state.rotation * (centre * extent + dir * (0.33 * h));
        if let Some(t) = ray_sphere(ray.origin, ray.dir, sphere, 0.5 * h, f32::MAX) {
            nearer(
                best,
                GizmoHit {
                    part: GizmoPart::ScaleArrow,
                    axes: Axes::single(axis),
                    point: ray.origin + ray.dir * t,
                    distance: t,
                },
            );
        }
    }
}

fn pick_box_handles(b: &ModelBox, view: ViewId, ray: Ray, best: &mut Option<GizmoHit>) {
    let centre = (b.min + b.max) * 0.5;
    for face in FaceName::ALL {
        let (axis, sign) = face_axis(face);
        if along_view(view, axis) {
            continue;
        }
        let mut sphere = centre;
        sphere[axis] = if sign > 0.0 { b.max[axis] } else { b.min[axis] };
        sphere[axis] += sign * 0.33 * BOX_HANDLE_SCALE;
        if let Some(t) = ray_sphere(
            ray.origin,
            ray.dir,
            sphere,
            0.5 * BOX_HANDLE_SCALE,
            f32::MAX,
        ) {
            nearer(
                best,
                GizmoHit {
                    part: GizmoPart::BoxHandle(face),
                    axes: Axes::single(axis),
                    point: ray.origin + ray.dir * t,
                    distance: t,
                },
            );
        }
    }
}

/// Snap steps for drags.
///
/// spec: roadmap_lean 6 (1/16 grid, whole units, 22.5 degree rotation)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Snap {
    /// Step of distances in world units, if snapping.
    pub grid: Option<f32>,
    /// Step of angles in degrees, if snapping.
    pub angle_deg: Option<f32>,
}

impl Snap {
    /// No snapping.
    pub const OFF: Snap = Snap {
        grid: None,
        angle_deg: None,
    };
    /// 1/16 of a unit and 22.5 degrees.
    pub const SIXTEENTH: Snap = Snap {
        grid: Some(0.0625),
        angle_deg: Some(22.5),
    };
    /// Whole units and 22.5 degrees.
    pub const UNIT: Snap = Snap {
        grid: Some(1.0),
        angle_deg: Some(22.5),
    };

    /// `v` rounded to the grid step.
    pub fn distance(&self, v: f32) -> f32 {
        self.grid.map_or(v, |step| snap_value(v, step))
    }

    /// `radians` rounded to the angle step.
    pub fn angle(&self, radians: f32) -> f32 {
        self.angle_deg
            .map_or(radians, |step| snap_value(radians, step.to_radians()))
    }

    /// Every component of `v` rounded to the grid step: the absolute snap of a target position.
    pub fn point(&self, v: Vec3) -> Vec3 {
        Vec3::new(self.distance(v.x), self.distance(v.y), self.distance(v.z))
    }
}

/// `v` rounded to a multiple of `step`; a step that is not positive and finite changes nothing.
pub fn snap_value(v: f32, step: f32) -> f32 {
    if step.is_finite() && step > 0.0 && v.is_finite() {
        (v / step).round() * step
    } else {
        v
    }
}

/// What a drag is projected onto.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Constraint {
    /// A line through `origin` along the unit vector `dir`.
    Axis {
        /// A point of the line.
        origin: Vec3,
        /// Unit direction.
        dir: Vec3,
    },
    /// A plane through `origin` with the unit `normal`.
    Plane {
        /// A point of the plane.
        origin: Vec3,
        /// Unit normal.
        normal: Vec3,
    },
    /// A circle around `center` in the plane with the unit normal `axis`.
    Arc {
        /// The centre.
        center: Vec3,
        /// Unit rotation axis.
        axis: Vec3,
    },
}

/// What a drag produced so far, relative to where it began.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DragDelta {
    /// A translation, along the axis or in the plane.
    Translation(Vec3),
    /// A rotation about the axis in radians, in the sense of `Quat::from_axis_angle(axis, angle)`.
    /// It accumulates across the half turn, so a full circle reports 2 pi.
    Angle(f32),
}

/// A drag in progress: the constraint and where the pointer started on it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Drag {
    constraint: Constraint,
    start: f32,
    start_point: Vec3,
    basis: (Vec3, Vec3),
    last: f32,
    total: f32,
}

/// The parameter along the line `(origin, dir)` of the point closest to the ray; `None` when
/// the ray is parallel to the line.
fn axis_param(ray: Ray, origin: Vec3, dir: Vec3) -> Option<f32> {
    let w = ray.origin - origin;
    let b = ray.dir.dot(dir);
    let denom = 1.0 - b * b;
    if denom < 1e-6 {
        return None;
    }
    let s = (dir.dot(w) - b * ray.dir.dot(w)) / denom;
    s.is_finite().then_some(s)
}

/// The signed angle of a ray's hit on the arc plane in the basis `(u, v)` about the centre.
fn arc_angle(ray: Ray, center: Vec3, axis: Vec3, basis: (Vec3, Vec3)) -> Option<f32> {
    let t = ray_plane(ray.origin, ray.dir, center, axis, f32::MAX)?;
    let rel = ray.origin + ray.dir * t - center;
    let angle = rel.dot(basis.1).atan2(rel.dot(basis.0));
    angle.is_finite().then_some(angle)
}

impl Drag {
    /// Starts a drag with the ray of the pressed pointer. `None` when the ray is parallel to the
    /// constraint (the pointer sees an axis end-on or a plane edge-on) or not finite.
    pub fn begin(constraint: Constraint, ray: Ray) -> Option<Drag> {
        if !(ray.origin.is_finite() && ray.dir.is_finite()) {
            return None;
        }
        let mut drag = Drag {
            constraint,
            start: 0.0,
            start_point: Vec3::ZERO,
            basis: (Vec3::X, Vec3::Y),
            last: 0.0,
            total: 0.0,
        };
        match constraint {
            Constraint::Axis { origin, dir } => {
                drag.start = axis_param(ray, origin, dir)?;
            }
            Constraint::Plane { origin, normal } => {
                let t = ray_plane(ray.origin, ray.dir, origin, normal, f32::MAX)?;
                drag.start_point = ray.origin + ray.dir * t;
            }
            Constraint::Arc { center, axis } => {
                let u = unit_or(axis.any_orthonormal_vector(), Vec3::X);
                drag.basis = (u, axis.cross(u));
                drag.start = arc_angle(ray, center, axis, drag.basis)?;
                drag.last = drag.start;
            }
        }
        Some(drag)
    }

    /// The drag result for the pointer ray now, with the snap applied to the relative amount.
    /// `None` when the ray cannot reach the constraint this frame; the drag keeps its state.
    pub fn update(&mut self, ray: Ray, snap: Snap) -> Option<DragDelta> {
        if !(ray.origin.is_finite() && ray.dir.is_finite()) {
            return None;
        }
        match self.constraint {
            Constraint::Axis { origin, dir } => {
                let s = axis_param(ray, origin, dir)?;
                Some(DragDelta::Translation(dir * snap.distance(s - self.start)))
            }
            Constraint::Plane { origin, normal } => {
                let t = ray_plane(ray.origin, ray.dir, origin, normal, f32::MAX)?;
                let delta = ray.origin + ray.dir * t - self.start_point;
                // Components along the normal stay out of the grid.
                let snapped = Vec3::from_array(std::array::from_fn(|a| {
                    if normal[a].abs() < 0.5 {
                        snap.distance(delta[a])
                    } else {
                        delta[a]
                    }
                }));
                Some(DragDelta::Translation(snapped))
            }
            Constraint::Arc { center, axis } => {
                let angle = arc_angle(ray, center, axis, self.basis)?;
                let step = (angle - self.last + std::f32::consts::PI)
                    .rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                self.last = angle;
                self.total += step;
                Some(DragDelta::Angle(snap.angle(self.total)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn state(kind: GizmoKind) -> GizmoState {
        GizmoState {
            kind: Some(kind),
            origin: Vec3::new(8.0, 8.0, -8.0),
            rotation: Quat::IDENTITY,
            size: Vec3::splat(4.0),
            gizmo_size: Vec3::ONE,
            gizmo_pivot: Vec3::ZERO,
            box_handles: None,
        }
    }

    fn main_camera() -> Camera {
        let mut cam = Camera::default_for(ViewId::Main);
        crate::navigation::aim_at(&mut cam, Vec3::new(8.0, 8.0, -8.0));
        cam
    }

    /// A ray from the camera through a world point.
    fn through(cam: &Camera, p: Vec3) -> Ray {
        Ray {
            origin: cam.pos,
            dir: (p - cam.pos).normalize(),
        }
    }

    #[test]
    fn axes_flags_follow_the_bit_values() {
        assert_eq!((Axes::X.bits(), Axes::Y.bits(), Axes::Z.bits()), (1, 2, 4));
        assert_eq!(
            (Axes::XY.bits(), Axes::XZ.bits(), Axes::YZ.bits()),
            (3, 5, 6)
        );
        assert_eq!(Axes::XZ.mask(), Vec3::new(1.0, 0.0, 1.0));
        assert!(Axes::YZ.has(2) && !Axes::YZ.has(0));
    }

    #[test]
    fn the_gizmo_keeps_its_size_on_the_screen() {
        let cam = main_camera();
        let origin = Vec3::new(8.0, 8.0, -8.0);
        let near = gizmo_scale(&cam, origin);
        let mut far_cam = cam;
        crate::navigation::zoom(&mut far_cam, origin, -5.0);
        let far = gizmo_scale(&far_cam, origin);
        let ratio = (far_cam.pos - origin).length() / (cam.pos - origin).length();
        assert!((far / near - ratio).abs() < 1e-3);
        // The projected length of the factor is the same share of the view at both distances.
        let rect = crate::Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        };
        let px = |c: &Camera, f: f32| {
            let a = c.project_to_rect(origin, rect).unwrap().0;
            let b = c.project_to_rect(origin + c.right() * f, rect).unwrap().0;
            (a - b).length()
        };
        assert!((px(&cam, near) - px(&far_cam, far)).abs() < 0.5);
        // An orthographic camera scales with its size.
        let mut ortho = Camera::default_for(ViewId::Front);
        let a = gizmo_scale(&ortho, origin);
        ortho.ortho_size *= 2.0;
        assert!((gizmo_scale(&ortho, origin) / a - 2.0).abs() < 1e-4);
    }

    #[test]
    fn move_arrows_are_hit_along_their_shaft_and_report_their_axis() {
        let cam = main_camera();
        let s = state(GizmoKind::Move);
        let f = gizmo_scale(&cam, s.origin) * MOVE_SCALE;
        for (axis, expected) in [(0, Axes::X), (1, Axes::Y), (2, Axes::Z)] {
            let tip = s.origin + arrow_dir(axis) * (0.9 * f);
            let hit = pick(&s, &cam, ViewId::Main, through(&cam, tip)).unwrap();
            assert_eq!(
                (hit.part, hit.axes),
                (GizmoPart::MoveArrow, expected),
                "axis {axis}"
            );
        }
        let away = s.origin + Vec3::new(1.0, 1.0, 1.0).normalize() * (5.0 * f);
        assert!(pick(&s, &cam, ViewId::Main, through(&cam, away)).is_none());
    }

    #[test]
    fn move_planes_are_hit_between_their_axes() {
        let cam = main_camera();
        let s = state(GizmoKind::Move);
        let f = gizmo_scale(&cam, s.origin) * MOVE_SCALE;
        let p = s.origin + Vec3::new(0.35 * f, 0.0, -0.35 * f);
        let hit = pick(&s, &cam, ViewId::Main, through(&cam, p)).unwrap();
        assert_eq!((hit.part, hit.axes), (GizmoPart::MovePlane, Axes::XZ));
    }

    #[test]
    fn orthographic_views_hide_the_handles_along_the_view_direction() {
        let cam = Camera::default_for(ViewId::Top);
        let s = state(GizmoKind::Move);
        let f = gizmo_scale(&cam, s.origin) * MOVE_SCALE;
        // Looking down Y, the Y arrow is end-on and not pickable; the XZ square is.
        let ray = Ray {
            origin: s.origin + Vec3::new(0.35 * f, 50.0, -0.35 * f),
            dir: Vec3::NEG_Y,
        };
        let hit = pick(&s, &cam, ViewId::Top, ray).unwrap();
        assert_eq!(hit.part, GizmoPart::MovePlane);
        let x_arrow = Ray {
            origin: s.origin + Vec3::new(0.9 * f, 50.0, 0.0),
            dir: Vec3::NEG_Y,
        };
        let hit = pick(&s, &cam, ViewId::Top, x_arrow).unwrap();
        assert_eq!((hit.part, hit.axes), (GizmoPart::MoveArrow, Axes::X));
        let mut scale = state(GizmoKind::Scale);
        scale.origin = Vec3::new(0.0, 0.0, 0.0);
        let top_of_box = scale.origin + Vec3::new(2.0, 4.3, -2.0);
        let ray = Ray {
            origin: top_of_box + Vec3::Y * 50.0,
            dir: Vec3::NEG_Y,
        };
        let hit = pick(&scale, &cam, ViewId::Top, ray);
        assert!(
            hit.is_none_or(|h| h.axes != Axes::Y),
            "the Y handles are hidden in the top view"
        );
    }

    #[test]
    fn rotation_rings_are_hit_on_their_band() {
        let cam = main_camera();
        let s = state(GizmoKind::Rotate);
        let k = gizmo_scale(&cam, s.origin) * RING_SCALE;
        // A point on the Y ring (the XZ plane around the origin), off the other two planes.
        let on_ring = s.origin + Vec3::new(1.0, 0.0, 1.0).normalize() * (1.9 * k);
        let hit = pick(&s, &cam, ViewId::Main, through(&cam, on_ring)).unwrap();
        assert_eq!((hit.part, hit.axes), (GizmoPart::RotateRing, Axes::Y));
        // Well inside the ring nothing is hit.
        let inside = s.origin + Vec3::new(0.3 * k, 0.0, 0.3 * k);
        assert!(pick(&s, &cam, ViewId::Main, through(&cam, inside)).is_none());
    }

    #[test]
    fn scale_handles_of_an_inverted_box_still_sit_outside_its_faces() {
        let cam = main_camera();
        let mut s = state(GizmoKind::Scale);
        s.origin = Vec3::ZERO;
        // x runs from -2 to 0.
        s.size = Vec3::new(-2.0, 2.0, 2.0);
        let h = gizmo_scale(&cam, s.origin) * SCALE_SCALE;
        let from_west = Ray {
            origin: Vec3::new(-50.0, 1.0, -1.0),
            dir: Vec3::X,
        };
        let hit = pick(&s, &cam, ViewId::Main, from_west).unwrap();
        assert_eq!((hit.part, hit.axes), (GizmoPart::ScaleArrow, Axes::X));
        assert!((hit.point.x - (-2.0 - 0.33 * h)).abs() <= 0.5 * h + 1e-4);
    }

    #[test]
    fn scale_handles_sit_outside_the_faces_of_the_box() {
        let cam = main_camera();
        let mut s = state(GizmoKind::Scale);
        s.origin = Vec3::ZERO;
        s.size = Vec3::new(2.0, 2.0, 2.0);
        let h = gizmo_scale(&cam, s.origin) * SCALE_SCALE;
        // Rays along the axes meet the handle on the outside of the box first.
        let from_x = Ray {
            origin: Vec3::new(50.0, 1.0, -1.0),
            dir: Vec3::NEG_X,
        };
        let hit = pick(&s, &cam, ViewId::Main, from_x).unwrap();
        assert_eq!((hit.part, hit.axes), (GizmoPart::ScaleArrow, Axes::X));
        assert!((hit.point.x - (2.0 + 0.33 * h)).abs() <= 0.5 * h + 1e-4);
        let from_z = Ray {
            origin: Vec3::new(1.0, 1.0, 50.0),
            dir: Vec3::NEG_Z,
        };
        let hit = pick(&s, &cam, ViewId::Main, from_z).unwrap();
        assert_eq!((hit.part, hit.axes), (GizmoPart::ScaleArrow, Axes::Z));
        // The handle at the south face is reached from the other side.
        let from_back = Ray {
            origin: Vec3::new(1.0, 1.0, -50.0),
            dir: Vec3::Z,
        };
        let hit = pick(&s, &cam, ViewId::Main, from_back).unwrap();
        assert_eq!((hit.part, hit.axes), (GizmoPart::ScaleArrow, Axes::Z));
        assert!(hit.point.z < -2.0);
        // A ray far from the box hits none.
        let miss = Ray {
            origin: Vec3::new(40.0, 40.0, -0.3),
            dir: Vec3::NEG_Y,
        };
        assert!(pick(&s, &cam, ViewId::Main, miss).is_none());
    }

    #[test]
    fn box_handles_report_their_face_and_only_they_start_a_press() {
        let cam = main_camera();
        let mut s = state(GizmoKind::Move);
        s.kind = None;
        s.box_handles = Some(cuboid_voxel::WBox3::EDITING_VOLUME);
        let handle = Vec3::new(32.0 + 0.495, 8.0, -8.0);
        let hit = pick(&s, &cam, ViewId::Main, through(&cam, handle)).unwrap();
        assert_eq!(hit.part, GizmoPart::BoxHandle(FaceName::East));
        assert_eq!(hit.axes, Axes::X);
        let handle = Vec3::new(8.0, 8.0, -32.0 - 0.495);
        let hit = pick(&s, &cam, ViewId::Main, through(&cam, handle)).unwrap();
        assert_eq!(hit.part, GizmoPart::BoxHandle(FaceName::South));
    }

    #[test]
    fn no_gizmo_without_a_kind_or_with_a_broken_state() {
        let cam = main_camera();
        let mut s = state(GizmoKind::Move);
        s.kind = None;
        assert!(pick(&s, &cam, ViewId::Main, through(&cam, s.origin)).is_none());
        s.kind = Some(GizmoKind::Rotate);
        s.origin = Vec3::NAN;
        let ray = Ray {
            origin: cam.pos,
            dir: cam.forward(),
        };
        assert!(pick(&s, &cam, ViewId::Main, ray).is_none());
    }

    #[test]
    fn ring_axes_follow_the_euler_chain() {
        let q = cuboid_num::euler_to_quat(Vec3::new(30.0, 40.0, 50.0));
        let [x, y, z] = ring_axes(q);
        assert!((y - Vec3::Y).length() < 1e-5);
        let yaw = Quat::from_rotation_y(40.0_f32.to_radians());
        let pitch = Quat::from_rotation_x(30.0_f32.to_radians());
        assert!((x - yaw * Vec3::X).length() < 1e-4);
        assert!((z - yaw * pitch * Vec3::Z).length() < 1e-4);
        assert_eq!(
            ring_axes(Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0)),
            [Vec3::X, Vec3::Y, Vec3::Z]
        );
    }

    #[test]
    fn an_axis_drag_follows_the_pointer_along_the_axis() {
        let origin = Vec3::new(1.0, 2.0, 3.0);
        let c = Constraint::Axis {
            origin,
            dir: Vec3::X,
        };
        let ray_at = |x: f32| Ray {
            origin: Vec3::new(x, 20.0, 3.0),
            dir: Vec3::NEG_Y,
        };
        let mut drag = Drag::begin(c, ray_at(2.0)).unwrap();
        let DragDelta::Translation(d) = drag.update(ray_at(5.25), Snap::OFF).unwrap() else {
            panic!("translation expected");
        };
        assert!((d - Vec3::new(3.25, 0.0, 0.0)).length() < 1e-5);
        let DragDelta::Translation(d) = drag.update(ray_at(5.3), Snap::SIXTEENTH).unwrap() else {
            panic!("translation expected");
        };
        assert!((d.x - 3.3125).abs() < 1e-5, "{d:?}");
        let DragDelta::Translation(d) = drag.update(ray_at(5.3), Snap::UNIT).unwrap() else {
            panic!("translation expected");
        };
        assert!((d.x - 3.0).abs() < 1e-5);
    }

    #[test]
    fn an_axis_seen_end_on_cannot_start_a_drag() {
        let c = Constraint::Axis {
            origin: Vec3::ZERO,
            dir: Vec3::Y,
        };
        assert!(
            Drag::begin(
                c,
                Ray {
                    origin: Vec3::new(0.0, 9.0, 0.0),
                    dir: Vec3::NEG_Y
                }
            )
            .is_none()
        );
        assert!(
            Drag::begin(
                c,
                Ray {
                    origin: Vec3::NAN,
                    dir: Vec3::NEG_Y
                }
            )
            .is_none()
        );
    }

    #[test]
    fn a_plane_drag_moves_in_the_plane_and_snaps_its_components() {
        let c = Constraint::Plane {
            origin: Vec3::new(0.0, 1.0, 0.0),
            normal: Vec3::Y,
        };
        let ray_at = |x: f32, z: f32| Ray {
            origin: Vec3::new(x, 30.0, z),
            dir: Vec3::NEG_Y,
        };
        let mut drag = Drag::begin(c, ray_at(1.0, 1.0)).unwrap();
        let DragDelta::Translation(d) = drag.update(ray_at(3.4, -0.9), Snap::UNIT).unwrap() else {
            panic!("translation expected");
        };
        assert_eq!(d, Vec3::new(2.0, 0.0, -2.0));
        let DragDelta::Translation(d) = drag.update(ray_at(3.4, -0.9), Snap::OFF).unwrap() else {
            panic!("translation expected");
        };
        assert!((d - Vec3::new(2.4, 0.0, -1.9)).length() < 1e-4);
        // A ray parallel to the plane cannot reach it; the drag survives.
        let flat = Ray {
            origin: Vec3::new(0.0, 1.0, 0.0),
            dir: Vec3::X,
        };
        assert!(drag.update(flat, Snap::OFF).is_none());
        assert!(drag.update(ray_at(1.0, 1.0), Snap::OFF).is_some());
    }

    #[test]
    fn an_arc_drag_reports_the_angle_about_the_axis_and_keeps_turning_past_a_half_turn() {
        let c = Constraint::Arc {
            center: Vec3::ZERO,
            axis: Vec3::Y,
        };
        let at = |deg: f32| {
            let a = deg.to_radians();
            // A ray straight down onto the point of the circle, in the sense of from_axis_angle.
            let q = Quat::from_axis_angle(Vec3::Y, a);
            let p = q * Vec3::X * 3.0;
            Ray {
                origin: p + Vec3::Y * 40.0,
                dir: Vec3::NEG_Y,
            }
        };
        let mut drag = Drag::begin(c, at(0.0)).unwrap();
        for deg in [20.0_f32, 60.0, 100.0, 170.0, 230.0, 300.0, 360.0] {
            let DragDelta::Angle(a) = drag.update(at(deg), Snap::OFF).unwrap() else {
                panic!("angle expected");
            };
            // Steps are below a half turn, so the angle accumulates past 180 degrees.
            assert!(
                (a.to_degrees() - deg).abs() < 1e-2,
                "{deg}: {}",
                a.to_degrees()
            );
        }
        let DragDelta::Angle(a) = drag.update(at(14.0 + 360.0), Snap::SIXTEENTH).unwrap() else {
            panic!("angle expected");
        };
        // 14 degrees wraps through 360 again; the snap picks the 22.5 degree multiple.
        assert!(((a.to_degrees() / 22.5).round() * 22.5 - a.to_degrees()).abs() < 1e-3);
    }

    #[test]
    fn snapping_ignores_steps_that_make_no_sense() {
        assert!((snap_value(0.3, 0.0625) - 0.3125).abs() < 1e-6);
        assert!((snap_value(0.3, 0.0) - 0.3).abs() < 1e-9);
        assert!(snap_value(f32::NAN, 1.0).is_nan());
        assert!((snap_value(7.2, f32::INFINITY) - 7.2).abs() < 1e-9);
        assert_eq!(
            Snap::OFF.point(Vec3::new(0.3, 1.7, -2.2)),
            Vec3::new(0.3, 1.7, -2.2)
        );
        assert_eq!(
            Snap::UNIT.point(Vec3::new(0.3, 1.7, -2.2)),
            Vec3::new(0.0, 2.0, -2.0)
        );
    }

    proptest! {
        #[test]
        fn snapped_axis_drags_land_on_the_grid(
            start in -20.0f32..20.0, end in -20.0f32..20.0, step_n in 1u32..64,
        ) {
            let step = 1.0 / f32::from(u16::try_from(step_n).unwrap());
            let snap = Snap { grid: Some(step), angle_deg: None };
            let c = Constraint::Axis { origin: Vec3::ZERO, dir: Vec3::X };
            let ray_at = |x: f32| Ray { origin: Vec3::new(x, 20.0, 0.0), dir: Vec3::NEG_Y };
            let mut drag = Drag::begin(c, ray_at(start)).unwrap();
            let Some(DragDelta::Translation(d)) = drag.update(ray_at(end), snap) else {
                return Err(TestCaseError::fail("no delta"));
            };
            let cells = d.x / step;
            prop_assert!((cells - cells.round()).abs() < 1e-3);
            prop_assert!((d.x - (end - start)).abs() <= step * 0.5 + 1e-4);
        }

        #[test]
        fn drags_never_produce_non_finite_numbers(
            ox in -1.0e4f32..1.0e4, oy in -1.0e4f32..1.0e4, oz in -1.0e4f32..1.0e4,
            dx in -1.0f32..1.0, dy in -1.0f32..1.0, dz in -1.0f32..1.0,
        ) {
            let dir = unit_or(Vec3::new(dx, dy, dz), Vec3::X);
            let ray = Ray { origin: Vec3::new(ox, oy, oz), dir };
            for c in [
                Constraint::Axis { origin: Vec3::ZERO, dir: Vec3::Z },
                Constraint::Plane { origin: Vec3::ZERO, normal: Vec3::Y },
                Constraint::Arc { center: Vec3::ZERO, axis: Vec3::X },
            ] {
                if let Some(mut drag) = Drag::begin(c, ray) {
                    for _ in 0..3 {
                        match drag.update(ray, Snap::SIXTEENTH) {
                            Some(DragDelta::Translation(t)) => prop_assert!(t.is_finite()),
                            Some(DragDelta::Angle(a)) => prop_assert!(a.is_finite()),
                            None => {}
                        }
                    }
                }
            }
        }
    }
}
