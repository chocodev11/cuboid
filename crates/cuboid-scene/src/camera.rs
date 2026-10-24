//! Camera state, projections and per-view defaults (spec: app 5.1, 5.7-5.8).
//!
//! The camera frame follows the file: a rotation `q` turns local `+Z` into the view direction,
//! `+X` into screen right and `+Y` into screen up, the same numbers as the original's engine. The
//! matrices use the left-handed convention that goes with it, with depth in `0..1`.

use cuboid_model::CameraState;
use cuboid_num::euler_to_quat;
use glam::camera::lh::proj::directx;
use glam::{Mat4, Quat, Vec2, Vec3};

use crate::geom::{Frustum, unit_or};
use crate::views::{Rect, ViewId};

/// Smallest and largest half-height of an orthographic view in world units.
pub const ORTHO_SIZE_RANGE: (f32, f32) = (0.05, 4096.0);

/// Smallest and largest vertical field of view in degrees.
pub const FOV_RANGE: (f32, f32) = (1.0, 170.0);

/// A view camera: pose, projection and clip planes.
///
/// spec: app 5.1, arch 6 (cuboid-scene)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Camera {
    /// World position.
    pub pos: Vec3,
    /// World rotation.
    pub rot: Quat,
    /// Orthographic projection (the three side views, and the Main view by preference).
    pub ortho: bool,
    /// Half of the vertical view extent in world units, 0.05 to 4096.
    pub ortho_size: f32,
    /// Vertical field of view in degrees (64 for the Main view).
    pub fov_deg: f32,
    /// Near clip plane (0.3).
    pub near: f32,
    /// Far clip plane (128; 256 for the Main view).
    pub far: f32,
}

impl Camera {
    /// The default camera of a view at its first anchor: the pose, projection and clip planes
    /// of the table of app 5.1.
    pub fn default_for(view: ViewId) -> Camera {
        let (pos, euler) = match view {
            ViewId::Top => (Vec3::new(8.0, 72.0, -8.0), Vec3::new(90.0, 0.0, 0.0)),
            ViewId::Side => (Vec3::new(-54.0, 8.0, -8.0), Vec3::new(0.0, 90.0, 0.0)),
            ViewId::Front => (Vec3::new(8.0, 8.0, -54.0), Vec3::ZERO),
            ViewId::Main => (Vec3::new(-4.0, 24.0, -20.0), Vec3::new(45.0, 45.0, 0.0)),
        };
        let main = view == ViewId::Main;
        Camera {
            pos,
            rot: euler_to_quat(euler),
            ortho: !main,
            ortho_size: 12.0,
            fov_deg: 64.0,
            near: 0.3,
            far: if main { 256.0 } else { 128.0 },
        }
    }

    /// The camera for a stored state: the stored pose and projection with the clip planes of
    /// `view`. A stored value that is not finite falls back to the default of the view.
    pub fn from_state(view: ViewId, state: &CameraState) -> Camera {
        let base = Camera::default_for(view);
        let mut camera = Camera {
            pos: state.pos,
            rot: state.rot,
            ortho: state.ortho,
            ortho_size: state.ortho_size,
            fov_deg: state.fov_deg,
            ..base
        };
        camera.sanitize(&base);
        camera
    }

    /// The state to store in the view properties.
    pub fn to_state(&self) -> CameraState {
        CameraState {
            pos: self.pos,
            rot: self.rot,
            ortho: self.ortho,
            ortho_size: self.ortho_size,
            fov_deg: self.fov_deg,
        }
    }

    /// Replaces every field that is not usable by the one of `fallback` and normalises the
    /// rotation and the ranges, so the matrices and rays of this camera are always finite.
    pub fn sanitize(&mut self, fallback: &Camera) {
        if !self.pos.is_finite() {
            self.pos = fallback.pos;
        }
        let norm2 = self.rot.length_squared();
        self.rot = if !self.rot.is_finite() || norm2 < 1e-12 {
            fallback.rot
        } else if (norm2 - 1.0).abs() > 1e-6 {
            self.rot.normalize()
        } else {
            self.rot
        };
        self.ortho_size = if self.ortho_size.is_finite() {
            self.ortho_size
                .clamp(ORTHO_SIZE_RANGE.0, ORTHO_SIZE_RANGE.1)
        } else {
            fallback.ortho_size
        };
        self.fov_deg = if self.fov_deg.is_finite() {
            self.fov_deg.clamp(FOV_RANGE.0, FOV_RANGE.1)
        } else {
            fallback.fov_deg
        };
        if !(self.near.is_finite() && self.near > 0.0) {
            self.near = fallback.near;
        }
        if !(self.far.is_finite() && self.far > self.near) {
            self.far = fallback.far.max(self.near * 2.0);
        }
    }

    /// The view direction.
    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::Z
    }

    /// Screen right.
    pub fn right(&self) -> Vec3 {
        self.rot * Vec3::X
    }

    /// Screen up.
    pub fn up(&self) -> Vec3 {
        self.rot * Vec3::Y
    }

    /// World to view space (`x` right, `y` up, `z` along the view direction).
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::from_quat(self.rot.conjugate()) * Mat4::from_translation(-self.pos)
    }

    /// View to clip space for a viewport of the given width over height. Depth maps to `0..1`.
    pub fn proj_matrix(&self, aspect: f32) -> Mat4 {
        let aspect = if aspect.is_finite() && aspect > 1e-4 {
            aspect
        } else {
            1.0
        };
        if self.ortho {
            let h = self.ortho_size;
            let w = h * aspect;
            directx::orthographic(-w, w, -h, h, self.near, self.far)
        } else {
            directx::perspective(self.fov_deg.to_radians(), aspect, self.near, self.far)
        }
    }

    /// World to clip space.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj_matrix(aspect) * self.view_matrix()
    }

    /// The ray through a point of the viewport in NDC (`x` right and `y` up, both in `-1..1`).
    /// Perspective rays start at the camera, orthographic rays on the near plane's footprint
    /// through the camera plane.
    pub fn ray_ndc(&self, ndc: Vec2, aspect: f32) -> Option<crate::Ray> {
        if !ndc.is_finite() {
            return None;
        }
        let aspect = if aspect.is_finite() && aspect > 1e-4 {
            aspect
        } else {
            1.0
        };
        let (right, up, fwd) = (self.right(), self.up(), self.forward());
        if self.ortho {
            let h = self.ortho_size;
            Some(crate::Ray {
                origin: self.pos + right * (ndc.x * h * aspect) + up * (ndc.y * h),
                dir: fwd,
            })
        } else {
            let t = (self.fov_deg.to_radians() * 0.5).tan();
            let dir = fwd + right * (ndc.x * t * aspect) + up * (ndc.y * t);
            Some(crate::Ray {
                origin: self.pos,
                dir: unit_or(dir, fwd),
            })
        }
    }

    /// The ray through a pointer position in canvas pixels (`y` down) for a view drawn in
    /// `rect`. `None` for an empty rectangle or a pointer that is not finite.
    pub fn pixel_ray(&self, pointer: Vec2, rect: Rect) -> Option<crate::Ray> {
        if !(rect.w > 0.0 && rect.h > 0.0) {
            return None;
        }
        let ndc = Vec2::new(
            (pointer.x - rect.x) / rect.w * 2.0 - 1.0,
            1.0 - (pointer.y - rect.y) / rect.h * 2.0,
        );
        self.ray_ndc(ndc, rect.w / rect.h)
    }

    /// The NDC position and the view depth of a world point, `None` behind the camera or when
    /// the point is not finite.
    pub fn project(&self, p: Vec3, aspect: f32) -> Option<(Vec2, f32)> {
        let v = self.view_matrix().transform_point3(p);
        if !v.is_finite() || (!self.ortho && v.z <= 1e-6) {
            return None;
        }
        let clip = self.proj_matrix(aspect) * v.extend(1.0);
        let w = if self.ortho { 1.0 } else { clip.w };
        let ndc = Vec2::new(clip.x / w, clip.y / w);
        ndc.is_finite().then_some((ndc, v.z))
    }

    /// The pointer position in canvas pixels of a world point, with its view depth.
    pub fn project_to_rect(&self, p: Vec3, rect: Rect) -> Option<(Vec2, f32)> {
        if !(rect.w > 0.0 && rect.h > 0.0) {
            return None;
        }
        let (ndc, depth) = self.project(p, rect.w / rect.h)?;
        Some((
            Vec2::new(
                rect.x + (ndc.x * 0.5 + 0.5) * rect.w,
                rect.y + (0.5 - ndc.y * 0.5) * rect.h,
            ),
            depth,
        ))
    }

    /// Half of the vertical extent of the view, in world units, at the depth of `p`. A gizmo or
    /// a marker of constant screen size scales with it.
    pub fn half_height_at(&self, p: Vec3) -> f32 {
        let depth = (p - self.pos).dot(self.forward());
        let h = if self.ortho {
            self.ortho_size
        } else {
            depth.max(self.near) * (self.fov_deg.to_radians() * 0.5).tan()
        };
        if h.is_finite() { h.max(1e-6) } else { 1.0 }
    }

    /// World units per pixel of a viewport `viewport_h` pixels high, at the depth of `p`.
    pub fn world_per_pixel(&self, p: Vec3, viewport_h: f32) -> f32 {
        2.0 * self.half_height_at(p) / viewport_h.max(1.0)
    }

    /// The frustum of the part of the view inside an NDC rectangle, for box selection.
    pub fn frustum(&self, aspect: f32, ndc_min: Vec2, ndc_max: Vec2) -> Frustum {
        let lo = ndc_min.min(ndc_max).clamp(Vec2::splat(-1.0), Vec2::ONE);
        let hi = ndc_min.max(ndc_max).clamp(Vec2::splat(-1.0), Vec2::ONE);
        Frustum::from_view_proj(self.view_proj(aspect), lo, hi)
    }

    /// The frustum of a pointer rectangle, for box selection: two corners in canvas pixels
    /// (`y` down) of a view drawn in `rect`, in any order and clipped to the view. `None` for an
    /// empty view or corners that are not finite.
    pub fn frustum_px(&self, rect: Rect, a: Vec2, b: Vec2) -> Option<Frustum> {
        if !(rect.w > 0.0 && rect.h > 0.0 && a.is_finite() && b.is_finite()) {
            return None;
        }
        let ndc = |p: Vec2| {
            Vec2::new(
                (p.x - rect.x) / rect.w * 2.0 - 1.0,
                1.0 - (p.y - rect.y) / rect.h * 2.0,
            )
        };
        Some(self.frustum(rect.aspect(), ndc(a), ndc(b)))
    }

    /// The view depth that sorts objects back to front: along the view direction for an
    /// orthographic camera, the distance to the camera for a perspective one.
    pub fn sort_depth(&self, p: Vec3) -> f32 {
        if self.ortho {
            (p - self.pos).dot(self.forward())
        } else {
            (p - self.pos).length()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cameras_follow_the_table_of_app_5_1() {
        let top = Camera::default_for(ViewId::Top);
        assert!(top.ortho && (top.forward() - Vec3::NEG_Y).length() < 1e-5);
        assert!((top.up() - Vec3::Z).length() < 1e-5);
        let side = Camera::default_for(ViewId::Side);
        assert!((side.forward() - Vec3::X).length() < 1e-5);
        assert!((side.right() + Vec3::Z).length() < 1e-5);
        let front = Camera::default_for(ViewId::Front);
        assert!((front.forward() - Vec3::Z).length() < 1e-5);
        assert!((front.right() - Vec3::X).length() < 1e-5);
        let main = Camera::default_for(ViewId::Main);
        assert!(!main.ortho && (main.far - 256.0).abs() < 1e-6);
        assert!((main.fov_deg - 64.0).abs() < 1e-6);
    }

    #[test]
    fn the_pixel_ray_at_the_centre_runs_along_the_view_direction() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            w: 400.0,
            h: 300.0,
        };
        for view in ViewId::ALL {
            let cam = Camera::default_for(view);
            let ray = cam.pixel_ray(Vec2::new(210.0, 170.0), rect).unwrap();
            assert!((ray.dir - cam.forward()).length() < 1e-5);
        }
    }

    #[test]
    fn a_projected_point_comes_back_through_the_pixel_ray() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 640.0,
            h: 480.0,
        };
        let cam = Camera::default_for(ViewId::Main);
        let p = Vec3::new(10.0, 3.0, -6.0);
        let (px, depth) = cam.project_to_rect(p, rect).unwrap();
        let ray = cam.pixel_ray(px, rect).unwrap();
        let on_ray = ray.origin + ray.dir * (p - ray.origin).dot(ray.dir);
        assert!(depth > 0.0 && (on_ray - p).length() < 1e-3);
    }

    #[test]
    fn stored_cameras_that_are_not_finite_fall_back_to_the_default() {
        let state = CameraState {
            pos: Vec3::NAN,
            rot: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            ortho: false,
            ortho_size: f32::INFINITY,
            fov_deg: f32::NAN,
        };
        let cam = Camera::from_state(ViewId::Main, &state);
        let base = Camera::default_for(ViewId::Main);
        assert_eq!(cam.pos, base.pos);
        assert!(cam.rot.is_finite() && cam.ortho_size.is_finite() && cam.fov_deg.is_finite());
        assert!(cam.view_proj(1.5).is_finite());
    }

    #[test]
    fn a_pixel_rectangle_selects_what_projects_into_it() {
        let rect = Rect {
            x: 100.0,
            y: 50.0,
            w: 800.0,
            h: 600.0,
        };
        let cam = Camera::default_for(ViewId::Main);
        let p = Vec3::new(10.0, 5.0, -9.0);
        let (px, _) = cam.project_to_rect(p, rect).unwrap();
        let around = cam
            .frustum_px(rect, px + Vec2::splat(20.0), px - Vec2::splat(20.0))
            .unwrap();
        assert!(around.contains_point(p));
        let elsewhere = cam
            .frustum_px(
                rect,
                px + Vec2::new(60.0, 20.0),
                px + Vec2::new(100.0, 60.0),
            )
            .unwrap();
        assert!(!elsewhere.contains_point(p));
        assert!(cam.frustum_px(Rect::default(), px, px).is_none());
        assert!(cam.frustum_px(rect, Vec2::NAN, px).is_none());
    }

    #[test]
    fn the_state_round_trips() {
        let cam = Camera::default_for(ViewId::Side);
        let back = Camera::from_state(ViewId::Side, &cam.to_state());
        assert_eq!(cam, back);
    }
}
