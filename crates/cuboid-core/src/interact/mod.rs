//! Interaction state machine: hover, click and box selection, gizmo drags and tool strokes
//! (spec: tools 8.3-9.3; arch 10.5).
//!
//! The UI builds an [`InputFrame`] per frame with the ray through the pointer; the gizmo
//! geometry and the picking sit behind `cuboid_scene::Raycaster`. A drag turns into document
//! edits through `cuboid-ops` and is one undo step however many frames it lasts.

mod frame;
mod gizmo;
mod input;
pub(crate) mod select;

use cuboid_model::ObjRef;
use cuboid_scene::{Axes, GizmoState, Rect};

use crate::commands::Effects;
pub(crate) use frame::update;
pub(crate) use gizmo::{Drag, gizmo_state};
pub use input::{ButtonState, InputFrame, Modifiers, ViewInput};

/// What the pointer does with the selection.
///
/// spec: tools 8.3, tools 16.1
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum SelectMode {
    /// Move gizmo and drags.
    #[default]
    Move,
    /// Rotate gizmo and drags.
    Rotate,
    /// Scale gizmo and drags.
    Scale,
    /// A paint, colour-picker or voxel tool drives the views; set by `Core::set_tool`.
    Custom,
}

/// Snapping of the gizmo drags.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SnapSettings {
    /// Snap at all.
    pub enabled: bool,
    /// Move and scale snap to multiples of this many units.
    pub grid: f32,
    /// Rotation snaps to multiples of this many degrees (before the rotation setter applies
    /// its own 5 or 22.5 degree grid).
    pub angle: f32,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            grid: 0.5,
            angle: 22.5,
        }
    }
}

/// What the pointer is doing between frames.
#[derive(Debug, Default)]
pub(crate) enum State {
    /// Nothing pressed.
    #[default]
    Idle,
    /// The left button went down away from any gizmo; a release without movement selects.
    Armed(frame::Armed),
    /// A box selection is open.
    Boxing(frame::BoxSelect),
    /// A gizmo drag.
    Dragging(Box<Drag>),
    /// A tool stroke holds the pointer.
    Stroke,
}

/// The state of the interaction between frames: hover, the grabbed axes, the pointer state and
/// the snapping. Hover and the drag are not part of the selection (arch 8.5).
///
/// spec: tools 8.1, tools 8.3, arch 10.5
#[derive(Debug, Default)]
pub struct Interaction {
    /// Which gizmo the pointer drives.
    pub mode: SelectMode,
    /// How drags snap.
    pub snap: SnapSettings,
    /// The object under the pointer.
    pub hovered: Option<ObjRef>,
    /// The axes of the gizmo handle under the pointer or being dragged.
    pub active_axis: Axes,
    pub(crate) state: State,
    pub(crate) next_gesture: u64,
}

impl Interaction {
    /// A gizmo drag, a box selection or a tool stroke is in progress.
    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            State::Dragging(_) | State::Boxing(_) | State::Stroke
        )
    }

    /// The rubber band of a box selection, in pixels.
    pub fn select_box(&self) -> Option<Rect> {
        match &self.state {
            State::Boxing(b) => Some(b.rect()),
            _ => None,
        }
    }

    /// Drops a drag, a box or a click in progress without touching the document.
    pub(crate) fn cancel(&mut self) {
        self.state = State::Idle;
        self.active_axis = Axes::default();
    }
}

/// The sprites of the tool cursor.
///
/// spec: tools 8.11
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum CursorSprite {
    /// Fully transparent.
    #[default]
    Hidden,
    /// Move arrows.
    Move,
    /// A ring: rotate and custom tools.
    Ring,
    /// Scale corners.
    Scale,
}

/// The symbol that follows the pointer.
///
/// spec: tools 8.11
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ToolCursor {
    /// Which sprite.
    pub sprite: CursorSprite,
    /// RGBA tint in 0..=1.
    pub tint: [f32; 4],
}

/// What one tick produced.
///
/// spec: arch 6 (cuboid-core), arch 10.5
#[derive(Clone, Debug, Default)]
pub struct TickOutput {
    /// UI requests of the commands and drags that ran.
    pub effects: Effects,
    /// The tool cursor for this frame.
    pub cursor: ToolCursor,
    /// The object under the pointer.
    pub hovered: Option<ObjRef>,
    /// The rubber band of a box selection, in pixels.
    pub select_box: Option<Rect>,
    /// The transform gizmo to draw and pick, if any.
    pub gizmo: Option<GizmoState>,
}
