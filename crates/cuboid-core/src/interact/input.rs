//! What one frame of input looks like to the interaction state machine.

use std::time::Instant;

use cuboid_scene::{Ray, Rect, ViewId};
use glam::{Mat4, Vec2, Vec3};

/// One mouse button during a frame.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ButtonState {
    /// The button is down now.
    pub down: bool,
    /// It went down this frame.
    pub pressed: bool,
    /// It went up this frame.
    pub released: bool,
}

impl ButtonState {
    /// A button that went down this frame.
    pub const PRESSED: Self = Self {
        down: true,
        pressed: true,
        released: false,
    };

    /// A button that stays down.
    pub const HELD: Self = Self {
        down: true,
        pressed: false,
        released: false,
    };

    /// A button that went up this frame.
    pub const RELEASED: Self = Self {
        down: false,
        pressed: false,
        released: true,
    };
}

/// The modifier keys. Shift adds to the selection, Ctrl toggles.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    /// Shift is held.
    pub shift: bool,
    /// Ctrl (Command on macOS) is held.
    pub ctrl: bool,
    /// Alt is held.
    pub alt: bool,
}

/// The view the pointer works in this frame. The UI and the scene crate know the cameras; the
/// core gets the results: the ray through the pointer to pick with, and the matrix to project
/// objects for a box selection.
#[derive(Copy, Clone, Debug)]
pub struct ViewInput {
    /// Which view; the gizmo picking depends on it.
    pub id: ViewId,
    /// The viewport in the same pixels as the pointer, y down.
    pub rect: Rect,
    /// World to clip space of the view's camera.
    pub view_proj: Mat4,
    /// The ray through the pointer; `None` when the pointer is outside the viewport.
    pub ray: Option<Ray>,
}

impl ViewInput {
    /// The pixel position of a world point, or `None` behind the camera or outside the depth
    /// range.
    pub fn project(&self, point: Vec3) -> Option<Vec2> {
        let clip = self.view_proj * point.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if ndc.z.abs() > 1.0 {
            return None;
        }
        Some(Vec2::new(
            self.rect.x + (ndc.x * 0.5 + 0.5) * self.rect.w,
            self.rect.y + (0.5 - ndc.y * 0.5) * self.rect.h,
        ))
    }
}

/// The input of one frame, built by the application from the shell's events.
///
/// spec: arch 10.5, arch 6 (cuboid-core)
#[derive(Clone, Debug)]
pub struct InputFrame {
    /// The frame time; the core reads no system clock.
    pub now: Instant,
    /// The pointer in canvas pixels, `None` when it is outside the window.
    pub pointer: Option<Vec2>,
    /// The left mouse button.
    pub left: ButtonState,
    /// The right mouse button.
    pub right: ButtonState,
    /// The modifier keys.
    pub mods: Modifiers,
    /// The pointer is over UI on top of the viewport; hover and presses are ignored.
    pub over_ui: bool,
    /// The view the pointer is in, if any.
    pub view: Option<ViewInput>,
}

impl InputFrame {
    /// A frame without input at time `now`.
    pub fn idle(now: Instant) -> Self {
        Self {
            now,
            pointer: None,
            left: ButtonState::default(),
            right: ButtonState::default(),
            mods: Modifiers::default(),
            over_ui: false,
            view: None,
        }
    }
}
