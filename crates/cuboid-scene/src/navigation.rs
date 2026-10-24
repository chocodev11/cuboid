//! Orbit, pan, zoom, frame and numpad views as pure functions of input deltas, plus the two
//! input presets (spec: app 5.5-5.6, replaced by roadmap_lean 6).
//!
//! Every function moves the camera around a pivot. Orbit and zoom keep the pivot where it is on
//! the screen, pan moves camera and pivot together, and all of them accept any input: a delta
//! that is not finite changes nothing, and the distance to the pivot stays in
//! [`DISTANCE_RANGE`].

use cuboid_num::euler_to_quat;
use glam::{Mat3, Quat, Vec2, Vec3};

use crate::camera::{Camera, ORTHO_SIZE_RANGE};
use crate::geom::unit_or;

/// Degrees of orbit per pixel of pointer travel.
pub const ORBIT_DEG_PER_PIXEL: f32 = 0.4;

/// Zoom strength of one wheel notch: the distance changes by `exp(-RATE)` per notch (about 13 %).
pub const ZOOM_RATE: f32 = 0.14;

/// Zoom per pixel of a dolly drag, in notches.
pub const DOLLY_NOTCHES_PER_PIXEL: f32 = 0.03;

/// Closest and farthest distance of a perspective camera from its pivot.
pub const DISTANCE_RANGE: (f32, f32) = (0.1, 100_000.0);

/// The orbit never reaches the poles, so the camera cannot flip over them.
const PITCH_LIMIT: f32 = 89.9 * std::f32::consts::PI / 180.0;

/// Margin around a framed object.
const FRAME_MARGIN: f32 = 1.15;

/// The mouse buttons held during a navigation drag.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Buttons {
    /// Primary button.
    pub left: bool,
    /// Wheel button.
    pub middle: bool,
    /// Secondary button.
    pub right: bool,
}

/// The modifier keys held during a navigation drag.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
    /// Alt.
    pub alt: bool,
}

/// What a drag does to the camera.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NavAction {
    /// Turn around the pivot.
    Orbit,
    /// Slide the camera and the pivot in the view plane.
    Pan,
    /// Zoom with the vertical pointer travel.
    Dolly,
}

/// The two input presets of the viewport.
///
/// spec: roadmap_lean 6
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum NavPreset {
    /// Middle drag orbits, Shift + middle drag pans, Ctrl + middle drag dollies.
    #[default]
    Blender,
    /// Right drag orbits, middle drag pans, Shift + right drag pans.
    Blockbench,
}

impl NavPreset {
    /// The action of a drag with these buttons and modifiers, `None` when the drag belongs to a
    /// tool. Alt + left drag orbits in both presets (Shift added: pan), for mice without a
    /// middle button.
    pub fn action(self, buttons: Buttons, mods: Modifiers) -> Option<NavAction> {
        if buttons.left && !buttons.middle && !buttons.right && mods.alt {
            return Some(if mods.shift {
                NavAction::Pan
            } else {
                NavAction::Orbit
            });
        }
        match self {
            NavPreset::Blender if buttons.middle => Some(match (mods.shift, mods.ctrl) {
                (true, _) => NavAction::Pan,
                (false, true) => NavAction::Dolly,
                (false, false) => NavAction::Orbit,
            }),
            NavPreset::Blockbench if buttons.right && !buttons.middle => Some(if mods.shift {
                NavAction::Pan
            } else {
                NavAction::Orbit
            }),
            NavPreset::Blockbench if buttons.middle => Some(NavAction::Pan),
            _ => None,
        }
    }
}

/// One frame of navigation input.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct NavInput {
    /// Buttons held.
    pub buttons: Buttons,
    /// Modifiers held.
    pub mods: Modifiers,
    /// Pointer travel since the last frame in pixels, `y` down.
    pub drag: Vec2,
    /// Wheel notches, positive toward the screen (zoom in).
    pub wheel: f32,
}

/// Applies one frame of input under a preset: the wheel zooms, a drag runs the action the
/// preset assigns to the held buttons. Returns whether the camera or the pivot changed.
pub fn navigate(
    preset: NavPreset,
    camera: &mut Camera,
    pivot: &mut Vec3,
    input: &NavInput,
    viewport_h: f32,
) -> bool {
    let before = (*camera, *pivot);
    if input.wheel.is_finite() && input.wheel.abs() > 0.0 {
        zoom(camera, *pivot, input.wheel);
    }
    if input.drag.is_finite() && input.drag != Vec2::ZERO {
        match preset.action(input.buttons, input.mods) {
            Some(NavAction::Orbit) => orbit(camera, *pivot, input.drag.x, input.drag.y),
            Some(NavAction::Pan) => pan(camera, pivot, input.drag.x, input.drag.y, viewport_h),
            Some(NavAction::Dolly) => {
                zoom(camera, *pivot, -input.drag.y * DOLLY_NOTCHES_PER_PIXEL);
            }
            None => {}
        }
    }
    before != (*camera, *pivot)
}

/// The vector from the pivot to the camera, replaced by a step back along the view direction
/// when the camera sits on the pivot.
fn offset_from(camera: &Camera, pivot: Vec3) -> Vec3 {
    let offset = camera.pos - pivot;
    if offset.length_squared() > 1e-12 {
        offset
    } else {
        -camera.forward()
    }
}

/// Orbits around the pivot by a pointer delta in pixels (`y` down): the scene follows the pointer.
/// Yaw turns about world `+Y`, pitch about the camera's right axis; the pitch stops short of the
/// poles. Distance and pivot position on the screen do not change.
pub fn orbit(camera: &mut Camera, pivot: Vec3, dx: f32, dy: f32) {
    orbit_degrees(
        camera,
        pivot,
        dx * ORBIT_DEG_PER_PIXEL,
        dy * ORBIT_DEG_PER_PIXEL,
    );
}

/// As [`orbit`] with angles in degrees; positive yaw moves the camera toward screen left,
/// positive pitch moves it up. The numpad steps use it.
pub fn orbit_degrees(camera: &mut Camera, pivot: Vec3, yaw_deg: f32, pitch_deg: f32) {
    if !(yaw_deg.is_finite() && pitch_deg.is_finite() && pivot.is_finite()) {
        return;
    }
    let yaw = Quat::from_rotation_y(yaw_deg.to_radians());
    let looking_down = (-camera.forward().y).clamp(-1.0, 1.0).asin();
    let target = (looking_down + pitch_deg.to_radians()).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    let right = yaw * camera.right();
    let pitch = Quat::from_axis_angle(unit_or(right, Vec3::X), target - looking_down);
    let turn = pitch * yaw;
    let offset = offset_from(camera, pivot);
    camera.pos = pivot + turn * offset;
    camera.rot = (turn * camera.rot).normalize();
}

/// Zooms toward the pivot by `z` notches, positive to zoom in. The distance (or the
/// orthographic size) scales by `exp(-ZOOM_RATE * z)`, so zooming in and out by the same amount
/// returns to the start, and the pivot stays where it is on the screen.
pub fn zoom(camera: &mut Camera, pivot: Vec3, z: f32) {
    if !(z.is_finite() && pivot.is_finite()) {
        return;
    }
    let factor = (-ZOOM_RATE * z.clamp(-50.0, 50.0)).exp();
    if camera.ortho {
        camera.ortho_size =
            (camera.ortho_size * factor).clamp(ORTHO_SIZE_RANGE.0, ORTHO_SIZE_RANGE.1);
        return;
    }
    let offset = offset_from(camera, pivot);
    let dist = offset.length();
    let scaled = (dist * factor).clamp(DISTANCE_RANGE.0, DISTANCE_RANGE.1);
    camera.pos = pivot + offset * (scaled / dist);
    camera.far = camera.far.max(scaled * 2.0 + 64.0);
}

/// Slides camera and pivot in the view plane so that the scene follows the pointer one to one at
/// the depth of the pivot. `viewport_h` is the height of the view in pixels.
pub fn pan(camera: &mut Camera, pivot: &mut Vec3, dx: f32, dy: f32, viewport_h: f32) {
    if !(dx.is_finite() && dy.is_finite() && viewport_h.is_finite()) {
        return;
    }
    let per_pixel = camera.world_per_pixel(*pivot, viewport_h);
    let delta = (camera.up() * dy - camera.right() * dx) * per_pixel;
    camera.pos += delta;
    *pivot += delta;
}

/// Turns the camera to look at the pivot without moving it, keeping the horizon level.
pub fn aim_at(camera: &mut Camera, pivot: Vec3) {
    let dir = pivot - camera.pos;
    if !dir.is_finite() || dir.length_squared() < 1e-12 {
        return;
    }
    camera.rot = look_rotation(dir.normalize(), Vec3::Y, camera.right());
}

/// The rotation that looks along `forward` with `up_hint` up; `fallback_right` replaces the
/// right axis when `forward` is parallel to the hint.
fn look_rotation(forward: Vec3, up_hint: Vec3, fallback_right: Vec3) -> Quat {
    let mut right = up_hint.cross(forward);
    if right.length_squared() < 1e-10 {
        right = fallback_right - forward * fallback_right.dot(forward);
    }
    let right = unit_or(right, Vec3::X);
    let up = forward.cross(right);
    Quat::from_mat3(&Mat3::from_cols(right, up, forward)).normalize()
}

/// The Focus key: moves the pivot to the hit point and aims the camera at it.
///
/// spec: app 5.5 (f)
pub fn focus(camera: &mut Camera, pivot: &mut Vec3, hit: Vec3) {
    if !hit.is_finite() {
        return;
    }
    *pivot = hit;
    aim_at(camera, hit);
}

/// Moves the pivot to the centre of a bounding sphere and backs the camera off along its view
/// direction until the sphere fills the view of the given width over height. An orthographic
/// camera gets the matching size. Does nothing for a centre or radius that is not finite.
pub fn frame_sphere(camera: &mut Camera, pivot: &mut Vec3, center: Vec3, radius: f32, aspect: f32) {
    if !(center.is_finite() && radius.is_finite()) {
        return;
    }
    let radius = radius.max(0.25);
    let aspect = if aspect.is_finite() && aspect > 1e-4 {
        aspect
    } else {
        1.0
    };
    let dist = if camera.ortho {
        camera.ortho_size =
            (radius * FRAME_MARGIN / aspect.min(1.0)).clamp(ORTHO_SIZE_RANGE.0, ORTHO_SIZE_RANGE.1);
        radius * 2.0 + 2.0
    } else {
        let tan_v = (camera.fov_deg.to_radians() * 0.5).tan();
        let tan_min = tan_v * aspect.min(1.0);
        (radius * FRAME_MARGIN * (1.0 + tan_min * tan_min).sqrt() / tan_min)
            .clamp(DISTANCE_RANGE.0, DISTANCE_RANGE.1)
    };
    *pivot = center;
    camera.pos = center - camera.forward() * dist;
    camera.far = camera.far.max(dist + radius * 2.0 + 1.0);
}

/// Frames a world box (see [`frame_sphere`]). The empty box is ignored.
pub fn frame_bounds(camera: &mut Camera, pivot: &mut Vec3, min: Vec3, max: Vec3, aspect: f32) {
    let b = crate::geom::Aabb::from_corners(min, max);
    if b.is_valid() {
        frame_sphere(camera, pivot, b.center(), b.half().length(), aspect);
    }
}

/// The numpad views. The camera sits on the named side of the pivot; the Front view is the
/// default view of the Front camera, looking along `+Z`.
///
/// spec: app 5.1 (anchors)
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum StandardView {
    /// Looks along `+Z` (numpad 1).
    Front,
    /// Looks along `-Z` (Ctrl + numpad 1).
    Back,
    /// Camera on the `-X` side, looks along `+X` (Ctrl + numpad 3).
    Left,
    /// Camera on the `+X` side, looks along `-X` (numpad 3).
    Right,
    /// Looks down along `-Y` (numpad 7).
    Top,
    /// Looks up along `+Y` (Ctrl + numpad 7).
    Bottom,
}

impl StandardView {
    /// The Euler angles of the camera in degrees.
    fn euler(self) -> Vec3 {
        match self {
            StandardView::Front => Vec3::ZERO,
            StandardView::Back => Vec3::new(0.0, 180.0, 0.0),
            StandardView::Left => Vec3::new(0.0, 90.0, 0.0),
            StandardView::Right => Vec3::new(0.0, 270.0, 0.0),
            StandardView::Top => Vec3::new(90.0, 0.0, 0.0),
            StandardView::Bottom => Vec3::new(270.0, 0.0, 0.0),
        }
    }
}

/// Turns the camera to a standard view, keeping its distance from the pivot and its projection.
pub fn snap_view(camera: &mut Camera, pivot: Vec3, view: StandardView) {
    if !pivot.is_finite() {
        return;
    }
    let dist = (camera.pos - pivot)
        .length()
        .clamp(DISTANCE_RANGE.0, DISTANCE_RANGE.1);
    camera.rot = euler_to_quat(view.euler());
    camera.pos = pivot - camera.forward() * dist;
}

/// Switches between perspective and orthographic so that the pivot keeps its apparent size.
pub fn set_ortho(camera: &mut Camera, pivot: Vec3, ortho: bool) {
    if camera.ortho == ortho || !pivot.is_finite() {
        return;
    }
    let tan_half = (camera.fov_deg.to_radians() * 0.5).tan();
    if ortho {
        let dist = (camera.pos - pivot).length();
        camera.ortho_size = (dist * tan_half).clamp(ORTHO_SIZE_RANGE.0, ORTHO_SIZE_RANGE.1);
    } else {
        let dist = (camera.ortho_size / tan_half).clamp(DISTANCE_RANGE.0, DISTANCE_RANGE.1);
        let dir = unit_or(camera.pos - pivot, -camera.forward());
        camera.pos = pivot + dir * dist;
        camera.far = camera.far.max(dist * 2.0 + 64.0);
    }
    camera.ortho = ortho;
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::views::{Rect, ViewId};

    fn main_camera() -> (Camera, Vec3) {
        let mut cam = Camera::default_for(ViewId::Main);
        let pivot = Vec3::new(8.0, 8.0, -8.0);
        aim_at(&mut cam, pivot);
        (cam, pivot)
    }

    fn pivot_ndc(cam: &Camera, pivot: Vec3) -> Vec2 {
        cam.project(pivot, 1.5).map(|(n, _)| n).unwrap_or(Vec2::NAN)
    }

    #[test]
    fn aiming_puts_the_pivot_at_the_centre_of_the_view() {
        let (cam, pivot) = main_camera();
        assert!(pivot_ndc(&cam, pivot).length() < 1e-4);
        assert!((cam.rot.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn orbit_keeps_the_distance_and_the_pivot_on_the_screen() {
        let (mut cam, pivot) = main_camera();
        let dist = (cam.pos - pivot).length();
        for (dx, dy) in [(37.0, -12.0), (-200.0, 90.0), (1.0e5, 1.0e5), (0.0, -1.0e5)] {
            orbit(&mut cam, pivot, dx, dy);
            assert!(((cam.pos - pivot).length() - dist).abs() < dist * 1e-4);
            assert!(pivot_ndc(&cam, pivot).length() < 1e-3);
        }
    }

    #[test]
    fn orbit_never_flips_over_the_poles() {
        let (mut cam, pivot) = main_camera();
        for _ in 0..200 {
            orbit(&mut cam, pivot, 0.0, 50.0);
        }
        assert!(cam.forward().y < -0.99 && cam.up().y > 0.0);
        for _ in 0..400 {
            orbit(&mut cam, pivot, 3.0, -50.0);
        }
        assert!(cam.forward().y > 0.99 && cam.up().y > 0.0);
    }

    #[test]
    fn the_scene_follows_the_pointer_during_an_orbit() {
        let (mut cam, pivot) = main_camera();
        // A point between the pivot and the camera moves with the pointer: right for a drag right.
        let near = pivot - cam.forward() * 3.0;
        orbit(&mut cam, pivot, 20.0, 0.0);
        assert!(cam.project(near, 1.5).unwrap().0.x > 0.0);
        let (mut cam, pivot) = main_camera();
        let near = pivot - cam.forward() * 3.0;
        orbit(&mut cam, pivot, 0.0, 20.0);
        assert!(cam.project(near, 1.5).unwrap().0.y < 0.0);
    }

    #[test]
    fn zoom_in_and_out_returns_to_the_start() {
        let (mut cam, pivot) = main_camera();
        let start = cam;
        let mut sum = 0.0;
        for z in [1.0, 0.1, -3.0, 2.5, 0.5, -1.1] {
            zoom(&mut cam, pivot, z);
            sum += z;
        }
        zoom(&mut cam, pivot, -sum);
        assert!((cam.pos - start.pos).length() < 1e-3);
    }

    #[test]
    fn zoom_never_leaves_the_distance_range() {
        let (mut cam, pivot) = main_camera();
        for _ in 0..100 {
            zoom(&mut cam, pivot, 50.0);
        }
        assert!((cam.pos - pivot).length() >= DISTANCE_RANGE.0 * 0.999);
        for _ in 0..100 {
            zoom(&mut cam, pivot, -50.0);
        }
        assert!((cam.pos - pivot).length() <= DISTANCE_RANGE.1 * 1.001);
        let mut ortho = Camera::default_for(ViewId::Top);
        for _ in 0..100 {
            zoom(&mut ortho, pivot, 50.0);
        }
        assert!(ortho.ortho_size >= ORTHO_SIZE_RANGE.0);
    }

    #[test]
    fn zoom_keeps_the_pivot_on_the_screen_even_when_the_camera_looks_past_it() {
        let mut cam = Camera::default_for(ViewId::Main);
        let pivot = Vec3::new(8.0, 8.0, -8.0);
        let before = pivot_ndc(&cam, pivot);
        zoom(&mut cam, pivot, 4.0);
        assert!((pivot_ndc(&cam, pivot) - before).length() < 1e-3);
    }

    #[test]
    fn degenerate_input_changes_nothing() {
        let (mut cam, mut pivot) = main_camera();
        let start = (cam, pivot);
        orbit(&mut cam, pivot, f32::NAN, 1.0);
        orbit(&mut cam, pivot, f32::INFINITY, 1.0);
        zoom(&mut cam, pivot, f32::NAN);
        pan(&mut cam, &mut pivot, f32::NAN, 0.0, 600.0);
        pan(&mut cam, &mut pivot, 1.0, 1.0, f32::NAN);
        focus(&mut cam, &mut pivot, Vec3::NAN);
        frame_sphere(&mut cam, &mut pivot, Vec3::NAN, 1.0, 1.0);
        frame_sphere(&mut cam, &mut pivot, Vec3::ZERO, f32::NAN, 1.0);
        assert_eq!((cam, pivot), start);
    }

    #[test]
    fn a_camera_on_its_pivot_stays_finite() {
        let mut cam = Camera::default_for(ViewId::Main);
        let pivot = cam.pos;
        orbit(&mut cam, pivot, 10.0, 10.0);
        zoom(&mut cam, pivot, 1.0);
        aim_at(&mut cam, pivot);
        assert!(cam.pos.is_finite() && cam.rot.is_finite());
        assert!(cam.view_proj(1.0).is_finite());
    }

    #[test]
    fn pan_moves_the_pivot_with_the_pointer_at_the_pivot_depth() {
        let (mut cam, mut pivot) = main_camera();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 600.0,
            h: 400.0,
        };
        let (before, _) = cam.project_to_rect(pivot, rect).unwrap();
        let start = pivot;
        pan(&mut cam, &mut pivot, 30.0, -20.0, rect.h);
        // The old pivot position now appears 30 right and 20 up of where it was.
        let (after, _) = cam.project_to_rect(start, rect).unwrap();
        assert!((after - before - Vec2::new(30.0, -20.0)).length() < 0.5);
        assert!((pivot - start).length() > 0.0);
    }

    #[test]
    fn framing_fits_the_box_in_view() {
        let (mut cam, mut pivot) = main_camera();
        let (min, max) = (Vec3::new(0.0, 0.0, -16.0), Vec3::new(16.0, 8.0, 0.0));
        frame_bounds(&mut cam, &mut pivot, min, max, 1.5);
        assert!((pivot - Vec3::new(8.0, 4.0, -8.0)).length() < 1e-4);
        for corner in [
            min,
            max,
            Vec3::new(0.0, 8.0, 0.0),
            Vec3::new(16.0, 0.0, -16.0),
        ] {
            let (ndc, depth) = cam.project(corner, 1.5).unwrap();
            assert!(
                depth > cam.near && ndc.abs().max_element() <= 1.0,
                "{ndc:?}"
            );
        }
        let mut top = Camera::default_for(ViewId::Top);
        frame_bounds(&mut top, &mut pivot, min, max, 1.5);
        assert!(top.ortho_size >= 8.0 * 1.15 - 1e-3);
    }

    #[test]
    fn standard_views_look_along_their_axes() {
        let (mut cam, pivot) = main_camera();
        let dist = (cam.pos - pivot).length();
        let cases = [
            (StandardView::Front, Vec3::Z),
            (StandardView::Back, Vec3::NEG_Z),
            (StandardView::Left, Vec3::X),
            (StandardView::Right, Vec3::NEG_X),
            (StandardView::Top, Vec3::NEG_Y),
            (StandardView::Bottom, Vec3::Y),
        ];
        for (view, dir) in cases {
            snap_view(&mut cam, pivot, view);
            assert!((cam.forward() - dir).length() < 1e-4, "{view:?}");
            assert!(((cam.pos - pivot).length() - dist).abs() < 1e-3);
            assert!(pivot_ndc(&cam, pivot).length() < 1e-4);
        }
    }

    #[test]
    fn switching_the_projection_keeps_the_apparent_size_of_the_pivot_plane() {
        let (mut cam, pivot) = main_camera();
        let p = pivot + cam.right() * 3.0;
        let before = cam.project(p, 1.0).unwrap().0;
        set_ortho(&mut cam, pivot, true);
        let ortho = cam.project(p, 1.0).unwrap().0;
        assert!((before - ortho).length() < 1e-3);
        set_ortho(&mut cam, pivot, false);
        let back = cam.project(p, 1.0).unwrap().0;
        assert!((before - back).length() < 1e-3);
    }

    #[test]
    fn the_presets_assign_the_documented_buttons() {
        let mid = Buttons {
            middle: true,
            ..Buttons::default()
        };
        let right = Buttons {
            right: true,
            ..Buttons::default()
        };
        let left = Buttons {
            left: true,
            ..Buttons::default()
        };
        let none = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..none
        };
        let ctrl = Modifiers { ctrl: true, ..none };
        let alt = Modifiers { alt: true, ..none };
        assert_eq!(NavPreset::Blender.action(mid, none), Some(NavAction::Orbit));
        assert_eq!(NavPreset::Blender.action(mid, shift), Some(NavAction::Pan));
        assert_eq!(NavPreset::Blender.action(mid, ctrl), Some(NavAction::Dolly));
        assert_eq!(NavPreset::Blender.action(right, none), None);
        assert_eq!(
            NavPreset::Blockbench.action(right, none),
            Some(NavAction::Orbit)
        );
        assert_eq!(
            NavPreset::Blockbench.action(right, shift),
            Some(NavAction::Pan)
        );
        assert_eq!(
            NavPreset::Blockbench.action(mid, none),
            Some(NavAction::Pan)
        );
        assert_eq!(NavPreset::Blockbench.action(left, none), None);
        assert_eq!(NavPreset::Blender.action(left, alt), Some(NavAction::Orbit));
        assert_eq!(
            NavPreset::Blockbench.action(
                left,
                Modifiers {
                    shift: true,
                    alt: true,
                    ..none
                }
            ),
            Some(NavAction::Pan)
        );
    }

    #[test]
    fn navigate_reports_a_change_only_when_something_moved() {
        let (mut cam, mut pivot) = main_camera();
        let mut input = NavInput::default();
        assert!(!navigate(
            NavPreset::Blender,
            &mut cam,
            &mut pivot,
            &input,
            600.0
        ));
        input.drag = Vec2::new(10.0, 4.0);
        assert!(!navigate(
            NavPreset::Blender,
            &mut cam,
            &mut pivot,
            &input,
            600.0
        ));
        input.buttons.middle = true;
        assert!(navigate(
            NavPreset::Blender,
            &mut cam,
            &mut pivot,
            &input,
            600.0
        ));
        input.drag = Vec2::ZERO;
        input.wheel = 1.0;
        assert!(navigate(
            NavPreset::Blockbench,
            &mut cam,
            &mut pivot,
            &input,
            600.0
        ));
    }

    proptest! {
        #[test]
        fn random_navigation_keeps_the_camera_usable(
            ops in proptest::collection::vec((0u8..6, -1.0e6f32..1.0e6, -1.0e6f32..1.0e6), 1..40)
        ) {
            let (mut cam, mut pivot) = main_camera();
            for (op, a, b) in ops {
                match op {
                    0 => orbit(&mut cam, pivot, a, b),
                    1 => zoom(&mut cam, pivot, a * 1e-3),
                    2 => pan(&mut cam, &mut pivot, a * 1e-3, b * 1e-3, 500.0),
                    3 => aim_at(&mut cam, pivot),
                    4 => snap_view(&mut cam, pivot, StandardView::Top),
                    _ => set_ortho(&mut cam, pivot, a > 0.0),
                }
                prop_assert!(cam.pos.is_finite() && cam.rot.is_finite() && pivot.is_finite());
                prop_assert!((cam.rot.length() - 1.0).abs() < 1e-3);
                prop_assert!(cam.view_proj(1.3).is_finite());
            }
        }

        #[test]
        fn orbit_preserves_the_distance(dx in -5000.0f32..5000.0, dy in -5000.0f32..5000.0) {
            let (mut cam, pivot) = main_camera();
            let dist = (cam.pos - pivot).length();
            orbit(&mut cam, pivot, dx, dy);
            prop_assert!(((cam.pos - pivot).length() - dist).abs() < dist * 1e-3);
        }
    }
}
