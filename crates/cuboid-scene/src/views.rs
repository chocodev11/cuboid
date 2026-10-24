//! The views: one by default, optional two or four split views, hover and active latching, side
//! swap (spec: app 5.2-5.4, with the layouts of roadmap_lean 6).

use cuboid_model::CameraState;
use glam::{Vec2, Vec3};

use crate::camera::Camera;

/// A rectangle in canvas pixels (y down), the shape of a view's quadrant. The UI kit has a
/// rectangle type of its own; the UI crate converts between the two.
///
/// spec: app 5.2, ui 5.1
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Whether the point is inside; the right and bottom edges are outside.
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }

    /// Width over height, 1 for an empty rectangle.
    pub fn aspect(&self) -> f32 {
        if self.w > 0.0 && self.h > 0.0 {
            self.w / self.h
        } else {
            1.0
        }
    }

    /// Whether the rectangle has pixels.
    pub fn is_empty(&self) -> bool {
        !(self.w > 0.0 && self.h > 0.0)
    }
}

/// The four views in quadrant order. The order is also the order of the cameras in the view
/// properties of a file.
///
/// spec: app 5.1, arch 6 (cuboid-scene)
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ViewId {
    /// Top-left, looking along -Y; planar axes X and Z.
    Top,
    /// Top-right, looking along +X; planar axes Y and Z.
    Side,
    /// Bottom-left, looking along +Z; planar axes X and Y.
    Front,
    /// Bottom-right perspective view with free axes.
    Main,
}

impl ViewId {
    /// All views in quadrant order.
    pub const ALL: [ViewId; 4] = [ViewId::Top, ViewId::Side, ViewId::Front, ViewId::Main];

    /// The position in [`ViewId::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// How many views are shown and where.
///
/// spec: roadmap_lean 6
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ViewLayout {
    /// The Main view fills the canvas.
    #[default]
    Single,
    /// The Main view on the left, the second view on the right.
    Split2,
    /// The four quadrants of app 5.2.
    Split4,
}

/// One view: its camera, which of its two anchors it sits on, and its quadrant.
///
/// spec: app 5.1, app 5.3
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// Which view.
    pub id: ViewId,
    /// Current camera.
    pub camera: Camera,
    /// Index of the current camera anchor (Up or Down, West or East, South or North); the
    /// Main view has one.
    pub anchor: u8,
    /// The viewport in canvas pixels; empty while the view is not shown.
    pub rect: Rect,
}

/// The pose of the second anchor of the three side views: position and Euler angles.
fn second_anchor(view: ViewId) -> Option<(Vec3, Vec3)> {
    match view {
        ViewId::Top => Some((Vec3::new(8.0, -54.0, -8.0), Vec3::new(270.0, 0.0, 0.0))),
        ViewId::Side => Some((Vec3::new(46.0, 8.0, -8.0), Vec3::new(0.0, 270.0, 0.0))),
        ViewId::Front => Some((Vec3::new(8.0, 8.0, 46.0), Vec3::new(0.0, 180.0, 0.0))),
        ViewId::Main => None,
    }
}

/// The state shared by the views.
///
/// spec: app 5.2-5.4, arch 6 (cuboid-scene)
#[derive(Clone, Debug)]
pub struct ViewSet {
    /// The views in [`ViewId::ALL`] order.
    pub views: [View; 4],
    /// The maximized view; the other views are neither drawn nor interactive.
    pub maximized: Option<ViewId>,
    /// The orbit pivot ("focus point"), `(8, 8, -8)` by default.
    pub pivot: Vec3,
    /// The view under the pointer, if no UI is on top.
    pub hovered: Option<ViewId>,
    /// The hovered view of this frame, copied to `active` unless a button is held.
    pub pending: Option<ViewId>,
    /// The view whose camera zooms, pans and picks.
    pub active: ViewId,
    /// The cinema frame is on (first-person display modes).
    pub cinema: bool,
    /// How many views are shown.
    pub layout: ViewLayout,
    /// The view next to Main in [`ViewLayout::Split2`].
    pub secondary: ViewId,
}

impl Default for ViewSet {
    /// Four views at their default cameras, one shown (Main), nothing maximized, `Main` active.
    fn default() -> Self {
        Self {
            views: ViewId::ALL.map(|id| View {
                id,
                camera: Camera::default_for(id),
                anchor: 0,
                rect: Rect::default(),
            }),
            maximized: None,
            pivot: Self::DEFAULT_PIVOT,
            hovered: None,
            pending: None,
            active: ViewId::Main,
            cinema: false,
            layout: ViewLayout::Single,
            secondary: ViewId::Top,
        }
    }
}

impl ViewSet {
    /// The default orbit pivot.
    pub const DEFAULT_PIVOT: Vec3 = Vec3::new(8.0, 8.0, -8.0);

    /// The view with this id.
    pub fn view(&self, id: ViewId) -> &View {
        &self.views[id.index()]
    }

    /// The view with this id, mutable.
    pub fn view_mut(&mut self, id: ViewId) -> &mut View {
        &mut self.views[id.index()]
    }

    /// The camera of the active view.
    pub fn active_camera(&self) -> &Camera {
        &self.view(self.active).camera
    }

    /// The views that are shown, in drawing order.
    pub fn visible(&self) -> Vec<ViewId> {
        if let Some(id) = self.maximized {
            return vec![id];
        }
        match self.layout {
            ViewLayout::Single => vec![ViewId::Main],
            ViewLayout::Split2 if self.secondary == ViewId::Main => vec![ViewId::Main],
            ViewLayout::Split2 => vec![ViewId::Main, self.secondary],
            ViewLayout::Split4 => ViewId::ALL.to_vec(),
        }
    }

    /// Places the shown views in `canvas` and empties the rectangles of the others. Four views
    /// take equal quadrants, two views equal halves.
    ///
    /// spec: app 5.2
    pub fn arrange(&mut self, canvas: Rect) {
        let shown = self.visible();
        for view in &mut self.views {
            view.rect = Rect::default();
        }
        let (hw, hh) = (canvas.w * 0.5, canvas.h * 0.5);
        for (n, id) in shown.iter().enumerate() {
            let rect = match (shown.len(), id) {
                (1, _) => canvas,
                (2, _) => Rect {
                    x: canvas.x + hw * n as f32,
                    y: canvas.y,
                    w: hw,
                    h: canvas.h,
                },
                (_, ViewId::Top) => Rect {
                    x: canvas.x,
                    y: canvas.y,
                    w: hw,
                    h: hh,
                },
                (_, ViewId::Side) => Rect {
                    x: canvas.x + hw,
                    y: canvas.y,
                    w: hw,
                    h: hh,
                },
                (_, ViewId::Front) => Rect {
                    x: canvas.x,
                    y: canvas.y + hh,
                    w: hw,
                    h: hh,
                },
                (_, ViewId::Main) => Rect {
                    x: canvas.x + hw,
                    y: canvas.y + hh,
                    w: hw,
                    h: hh,
                },
            };
            self.view_mut(*id).rect = rect;
        }
    }

    /// Changes the layout and re-places the views. Maximizing is cleared; the active view
    /// moves to Main when it is no longer shown.
    pub fn set_layout(&mut self, layout: ViewLayout, canvas: Rect) {
        self.layout = layout;
        self.maximized = None;
        self.arrange(canvas);
        if !self.visible().contains(&self.active) {
            self.active = ViewId::Main;
        }
    }

    /// Toggles the maximized state of a view and makes it pending (spec 5.2).
    pub fn toggle_maximize(&mut self, id: ViewId, canvas: Rect) {
        self.maximized = if self.maximized == Some(id) {
            None
        } else {
            Some(id)
        };
        self.pending = Some(id);
        self.arrange(canvas);
    }

    /// Updates the hovered, pending and active views for this frame. A view is hovered while the
    /// pointer is in its rectangle and no UI is on top of it; the pending view becomes active
    /// unless a button is held, so a drag keeps its view.
    ///
    /// spec: app 5.4
    pub fn update_hover(&mut self, pointer: Option<Vec2>, over_ui: bool, button_held: bool) {
        self.hovered = match pointer {
            Some(p) if !over_ui => self
                .visible()
                .into_iter()
                .find(|&id| self.view(id).rect.contains(p)),
            _ => None,
        };
        self.pending = self.hovered;
        if !button_held && let Some(id) = self.pending {
            self.active = id;
        }
    }

    /// Cycles `view` to its next camera anchor (right click on the label): keeps the camera
    /// coordinate on the axes where both anchors agree, takes the anchor's elsewhere, and
    /// adopts the anchor's rotation.
    ///
    /// spec: app 5.3
    pub fn swap_side(&mut self, view: ViewId) {
        let first = Camera::default_for(view);
        let v = self.view_mut(view);
        let Some((pos, euler)) = second_anchor(view) else {
            // Main has one anchor; the click only resets the orientation.
            v.camera.rot = Camera::default_for(ViewId::Main).rot;
            return;
        };
        let (from, to) = if v.anchor == 0 {
            (first.pos, pos)
        } else {
            (pos, first.pos)
        };
        let to_rot = if v.anchor == 0 {
            cuboid_num::euler_to_quat(euler)
        } else {
            first.rot
        };
        for axis in 0..3 {
            // Where the anchors agree the camera keeps its coordinate, so panning survives.
            if (from[axis] - to[axis]).abs() > 0.0 {
                v.camera.pos[axis] = to[axis];
            }
        }
        v.camera.rot = to_rot;
        v.anchor ^= 1;
    }

    /// Restores the four cameras and the pivot to their defaults. The layout, the maximized
    /// view, the anchor indices and the cinema frame stay.
    ///
    /// spec: app 5.7
    pub fn reset_views(&mut self) {
        for view in &mut self.views {
            view.camera = Camera::default_for(view.id);
        }
        self.pivot = Self::DEFAULT_PIVOT;
    }

    /// The four cameras as the file stores them.
    pub fn camera_states(&self) -> [Option<CameraState>; 4] {
        std::array::from_fn(|i| Some(self.views[i].camera.to_state()))
    }

    /// Adopts the cameras of a file; a view without a stored camera keeps its own.
    pub fn apply_camera_states(&mut self, states: &[Option<CameraState>; 4]) {
        for (view, state) in self.views.iter_mut().zip(states) {
            if let Some(state) = state {
                view.camera = Camera::from_state(view.id, state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: Rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 800.0,
        h: 600.0,
    };

    #[test]
    fn views_are_numbered_in_quadrant_order() {
        let order: Vec<usize> = ViewId::ALL.iter().map(|v| v.index()).collect();
        assert_eq!(order, [0, 1, 2, 3]);
        assert_eq!(ViewId::Main.index(), 3);
    }

    #[test]
    fn one_view_fills_the_canvas_by_default() {
        let mut set = ViewSet::default();
        assert_eq!((set.layout, set.active), (ViewLayout::Single, ViewId::Main));
        set.arrange(CANVAS);
        assert_eq!(set.visible(), [ViewId::Main]);
        assert_eq!(set.view(ViewId::Main).rect, CANVAS);
        assert!(set.view(ViewId::Top).rect.is_empty());
    }

    #[test]
    fn two_views_split_the_canvas_in_halves() {
        let mut set = ViewSet {
            secondary: ViewId::Front,
            ..ViewSet::default()
        };
        set.set_layout(ViewLayout::Split2, CANVAS);
        assert_eq!(set.visible(), [ViewId::Main, ViewId::Front]);
        let (a, b) = (set.view(ViewId::Main).rect, set.view(ViewId::Front).rect);
        assert_eq!((a.x, b.x, a.w, b.w), (10.0, 410.0, 400.0, 400.0));
        assert_eq!((a.h, b.h), (600.0, 600.0));
    }

    #[test]
    fn four_views_take_the_quadrants_of_app_5_2() {
        let mut set = ViewSet::default();
        set.set_layout(ViewLayout::Split4, CANVAS);
        let at = |id| {
            let r = set.view(id).rect;
            (r.x, r.y, r.w, r.h)
        };
        assert_eq!(at(ViewId::Top), (10.0, 20.0, 400.0, 300.0));
        assert_eq!(at(ViewId::Side), (410.0, 20.0, 400.0, 300.0));
        assert_eq!(at(ViewId::Front), (10.0, 320.0, 400.0, 300.0));
        assert_eq!(at(ViewId::Main), (410.0, 320.0, 400.0, 300.0));
    }

    #[test]
    fn maximizing_shows_one_view_and_clicking_again_returns() {
        let mut set = ViewSet::default();
        set.set_layout(ViewLayout::Split4, CANVAS);
        set.toggle_maximize(ViewId::Front, CANVAS);
        assert_eq!(set.visible(), [ViewId::Front]);
        assert_eq!(set.view(ViewId::Front).rect, CANVAS);
        assert_eq!(set.pending, Some(ViewId::Front));
        set.toggle_maximize(ViewId::Front, CANVAS);
        assert_eq!(set.visible().len(), 4);
    }

    #[test]
    fn a_drag_keeps_its_view_until_the_buttons_are_released() {
        let mut set = ViewSet::default();
        set.set_layout(ViewLayout::Split4, CANVAS);
        set.update_hover(Some(Vec2::new(100.0, 100.0)), false, false);
        assert_eq!(set.active, ViewId::Top);
        set.update_hover(Some(Vec2::new(600.0, 500.0)), false, true);
        assert_eq!((set.hovered, set.active), (Some(ViewId::Main), ViewId::Top));
        set.update_hover(Some(Vec2::new(600.0, 500.0)), true, false);
        assert_eq!((set.hovered, set.active), (None, ViewId::Top));
        set.update_hover(Some(Vec2::new(600.0, 500.0)), false, false);
        assert_eq!(set.active, ViewId::Main);
    }

    #[test]
    fn the_active_view_returns_to_main_when_it_is_hidden() {
        let mut set = ViewSet::default();
        set.set_layout(ViewLayout::Split4, CANVAS);
        set.active = ViewId::Top;
        set.set_layout(ViewLayout::Single, CANVAS);
        assert_eq!(set.active, ViewId::Main);
    }

    #[test]
    fn side_swap_keeps_the_panned_coordinates_and_cycles_back() {
        let mut set = ViewSet::default();
        set.view_mut(ViewId::Top).camera.pos = Vec3::new(20.0, 72.0, 5.0);
        set.swap_side(ViewId::Top);
        let cam = set.view(ViewId::Top).camera;
        assert_eq!(cam.pos, Vec3::new(20.0, -54.0, 5.0));
        assert!((cam.forward() - Vec3::Y).length() < 1e-4);
        set.swap_side(ViewId::Top);
        assert_eq!(set.view(ViewId::Top).camera.pos, Vec3::new(20.0, 72.0, 5.0));
        assert!((set.view(ViewId::Top).camera.forward() - Vec3::NEG_Y).length() < 1e-4);
    }

    #[test]
    fn side_swap_of_the_front_view_moves_to_the_north_anchor() {
        let mut set = ViewSet::default();
        set.swap_side(ViewId::Front);
        let cam = set.view(ViewId::Front).camera;
        assert_eq!(cam.pos, Vec3::new(8.0, 8.0, 46.0));
        assert!((cam.forward() - Vec3::NEG_Z).length() < 1e-4);
    }

    #[test]
    fn reset_restores_the_cameras_and_keeps_the_layout() {
        let mut set = ViewSet::default();
        set.set_layout(ViewLayout::Split4, CANVAS);
        set.view_mut(ViewId::Main).camera.pos = Vec3::splat(100.0);
        set.pivot = Vec3::ONE;
        set.reset_views();
        assert_eq!(
            set.view(ViewId::Main).camera,
            Camera::default_for(ViewId::Main)
        );
        assert_eq!(
            (set.pivot, set.layout),
            (ViewSet::DEFAULT_PIVOT, ViewLayout::Split4)
        );
    }

    #[test]
    fn stored_cameras_apply_and_come_back() {
        let mut set = ViewSet::default();
        let mut states = set.camera_states();
        let mut moved = Camera::default_for(ViewId::Side).to_state();
        moved.pos.y = 99.0;
        states[1] = Some(moved);
        states[0] = None;
        set.apply_camera_states(&states);
        assert!((set.view(ViewId::Side).camera.pos.y - 99.0).abs() < 1e-6);
        assert_eq!(
            set.view(ViewId::Top).camera,
            Camera::default_for(ViewId::Top)
        );
    }
}
