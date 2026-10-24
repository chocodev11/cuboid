//! The interaction state machine driven by synthetic input and a scripted ray caster.

mod common;

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use common::{assert_same, run, scene, select, state};
use cuboid_core::{
    Button, ButtonState, Command, Core, CustomTool, InputFrame, Modifiers, SelectMode, StrokeStep,
    Target, TickOutput, ToolCx, ViewInput,
};
use cuboid_model::ObjRef;
use cuboid_scene::{
    Axes, GizmoHit, GizmoKind, GizmoPart, GizmoState, Hit, HitTarget, Ray, Raycaster, Rect, ViewId,
};
use glam::{Mat4, Vec2, Vec3};

/// Answers picking from values the test sets.
#[derive(Default)]
struct Mock {
    model: Cell<Option<Hit>>,
    gizmo: Cell<Option<GizmoHit>>,
    plane: Cell<Vec3>,
    asked: RefCell<Vec<GizmoKind>>,
}

impl Raycaster for Mock {
    fn pick_model(&self, _ray: Ray, _max_len: f32) -> Option<Hit> {
        self.model.get()
    }

    fn pick_gizmo(&self, _ray: Ray, _view: ViewId, gizmo: &GizmoState) -> Option<GizmoHit> {
        self.asked.borrow_mut().extend(gizmo.kind);
        self.gizmo.get()
    }

    fn drag_plane(&self, _ray: Ray, _through: Vec3, _max_len: f32) -> Option<Vec3> {
        Some(self.plane.get())
    }
}

struct Rig {
    core: Core,
    mock: Mock,
    t0: Instant,
    frames: u64,
}

const NO_MODS: Modifiers = Modifiers {
    shift: false,
    ctrl: false,
    alt: false,
};

impl Rig {
    fn new() -> Self {
        Rig {
            core: scene(),
            mock: Mock::default(),
            t0: Instant::now(),
            frames: 0,
        }
    }

    /// World (x, y) to canvas pixels of the test view: 40 pixels per unit, origin in the middle.
    fn px(x: f32, y: f32) -> Vec2 {
        Vec2::new(400.0 + 40.0 * x, 300.0 - 40.0 * y)
    }

    fn input(&mut self, pointer: Vec2, left: ButtonState, mods: Modifiers) -> InputFrame {
        self.frames += 1;
        let mut input = InputFrame::idle(self.t0 + Duration::from_millis(16 * self.frames));
        input.pointer = Some(pointer);
        input.left = left;
        input.mods = mods;
        input.view = Some(ViewInput {
            id: ViewId::Main,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            view_proj: Mat4::from_scale(Vec3::new(0.1, 1.0 / 7.5, 0.01)),
            ray: Some(Ray {
                origin: Vec3::ZERO,
                dir: Vec3::NEG_Z,
            }),
        });
        input
    }

    fn frame(&mut self, pointer: Vec2, left: ButtonState) -> TickOutput {
        self.frame_with(pointer, left, NO_MODS)
    }

    fn frame_with(&mut self, pointer: Vec2, left: ButtonState, mods: Modifiers) -> TickOutput {
        let input = self.input(pointer, left, mods);
        self.core.tick(&input, &self.mock)
    }

    /// A click: press, then release at the same place.
    fn click(&mut self, pointer: Vec2, mods: Modifiers) {
        self.frame_with(pointer, ButtonState::PRESSED, mods);
        self.frame_with(pointer, ButtonState::RELEASED, mods);
    }

    fn point_at(&self, element: usize) {
        let id = self.core.doc().elements.ids()[element];
        self.mock.model.set(Some(Hit {
            target: HitTarget::Element(id),
            point: Vec3::ZERO,
            normal: Vec3::Y,
            face: None,
            uv: None,
        }));
    }

    fn grab(&self, part: GizmoPart, axes: Axes, point: Vec3) {
        self.mock.gizmo.set(Some(GizmoHit {
            part,
            axes,
            point,
            distance: 1.0,
        }));
    }

    fn selected(&self) -> Vec<Target> {
        let mut v: Vec<_> = self
            .core
            .selection()
            .list
            .iter()
            .filter_map(|&o| self.core.target_of(o))
            .collect();
        v.sort_by_key(|t| format!("{t:?}"));
        v
    }

    fn element(&self, at: usize) -> cuboid_model::Element {
        self.core.doc().elements.items()[at].as_ref().clone()
    }
}

const X: Axes = Axes {
    x: true,
    y: false,
    z: false,
};
const Y: Axes = Axes {
    x: false,
    y: true,
    z: false,
};

#[test]
fn hovering_reports_the_object_under_the_pointer() {
    let mut rig = Rig::new();
    rig.point_at(1);
    let out = rig.frame(Rig::px(5.0, 1.0), ButtonState::default());
    let id = rig.core.doc().elements.ids()[1];
    assert_eq!(out.hovered, Some(ObjRef::Element(id)));
    assert_eq!(rig.core.interaction().hovered, Some(ObjRef::Element(id)));
    rig.mock.model.set(None);
    assert_eq!(
        rig.frame(Rig::px(0.0, 0.0), ButtonState::default()).hovered,
        None
    );
}

#[test]
fn a_click_selects_the_hovered_object_exclusively() {
    let mut rig = Rig::new();
    rig.point_at(0);
    rig.click(Rig::px(1.0, 1.0), NO_MODS);
    assert_eq!(rig.selected(), [Target::Element(0)]);
    rig.point_at(2);
    rig.click(Rig::px(9.0, 1.0), NO_MODS);
    assert_eq!(rig.selected(), [Target::Element(2)]);
    assert_eq!(
        rig.core.history().undo_len(),
        9,
        "selecting is no undo step"
    );
}

#[test]
fn shift_adds_and_ctrl_toggles() {
    let mut rig = Rig::new();
    let shift = Modifiers {
        shift: true,
        ..NO_MODS
    };
    let ctrl = Modifiers {
        ctrl: true,
        ..NO_MODS
    };
    rig.point_at(0);
    rig.click(Rig::px(1.0, 1.0), NO_MODS);
    rig.point_at(1);
    rig.click(Rig::px(5.0, 1.0), shift);
    assert_eq!(rig.selected(), [Target::Element(0), Target::Element(1)]);
    rig.point_at(0);
    rig.click(Rig::px(1.0, 1.0), ctrl);
    assert_eq!(rig.selected(), [Target::Element(1)]);
    rig.point_at(2);
    rig.click(Rig::px(9.0, 1.0), ctrl);
    assert_eq!(rig.selected(), [Target::Element(1), Target::Element(2)]);
}

#[test]
fn a_click_on_empty_space_clears_the_selection() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0), Target::Element(1)]);
    rig.mock.model.set(None);
    rig.click(Rig::px(-8.0, -6.0), NO_MODS);
    assert!(rig.selected().is_empty());
    select(&mut rig.core, &[Target::Element(0)]);
    rig.click(
        Rig::px(-8.0, -6.0),
        Modifiers {
            shift: true,
            ..NO_MODS
        },
    );
    assert_eq!(
        rig.selected(),
        [Target::Element(0)],
        "a modifier click keeps the selection"
    );
}

#[test]
fn a_box_selects_the_objects_whose_centres_fall_inside() {
    let mut rig = Rig::new();
    rig.mock.model.set(None);
    // The centres of the three cubes are at x = 1, 5 and 9, y = 1.
    let start = Rig::px(0.0, 3.0);
    rig.frame(start, ButtonState::PRESSED);
    let out = rig.frame(Rig::px(6.5, -1.0), ButtonState::HELD);
    let band = out.select_box.expect("a rubber band");
    assert!((band.x - start.x).abs() < 1e-3 && band.w > 200.0);
    assert_eq!(rig.selected(), [Target::Element(0), Target::Element(1)]);
    assert!(rig.core.interaction().busy());
    // Growing the box takes the third one in, shrinking it lets the first go.
    rig.frame(Rig::px(10.0, -1.0), ButtonState::HELD);
    assert_eq!(rig.selected().len(), 3);
    let out = rig.frame(Rig::px(10.0, -1.0), ButtonState::RELEASED);
    assert!(out.select_box.is_none());
    assert!(!rig.core.interaction().busy());
    assert_eq!(rig.selected().len(), 3);
    assert_eq!(rig.core.history().undo_len(), 9);
}

#[test]
fn a_box_with_shift_adds_to_the_selection_and_with_ctrl_toggles() {
    let mut rig = Rig::new();
    rig.mock.model.set(None);
    select(&mut rig.core, &[Target::Element(2)]);
    let shift = Modifiers {
        shift: true,
        ..NO_MODS
    };
    rig.frame_with(Rig::px(0.0, 3.0), ButtonState::PRESSED, shift);
    rig.frame_with(Rig::px(6.5, -1.0), ButtonState::HELD, shift);
    rig.frame_with(Rig::px(6.5, -1.0), ButtonState::RELEASED, shift);
    assert_eq!(rig.selected().len(), 3);

    let ctrl = Modifiers {
        ctrl: true,
        ..NO_MODS
    };
    rig.frame_with(Rig::px(0.0, 3.0), ButtonState::PRESSED, ctrl);
    rig.frame_with(Rig::px(2.0, -1.0), ButtonState::HELD, ctrl);
    rig.frame_with(Rig::px(2.0, -1.0), ButtonState::RELEASED, ctrl);
    assert_eq!(rig.selected(), [Target::Element(1), Target::Element(2)]);
}

#[test]
fn a_box_ignores_hidden_objects() {
    let mut rig = Rig::new();
    rig.mock.model.set(None);
    run(
        &mut rig.core,
        Command::SetHidden {
            targets: vec![Target::Element(0)],
            hidden: true,
        },
    );
    rig.frame(Rig::px(-2.0, 3.0), ButtonState::PRESSED);
    rig.frame(Rig::px(6.5, -1.0), ButtonState::HELD);
    assert_eq!(rig.selected(), [Target::Element(1)]);
}

#[test]
fn the_gizmo_follows_the_mode_and_the_primary() {
    let mut rig = Rig::new();
    let out = rig.frame(Rig::px(0.0, 0.0), ButtonState::default());
    assert!(out.gizmo.is_none(), "no primary, no gizmo");
    select(&mut rig.core, &[Target::Element(1)]);
    let out = rig.frame(Rig::px(0.0, 0.0), ButtonState::default());
    let gizmo = out.gizmo.expect("a gizmo");
    assert_eq!(gizmo.kind, Some(GizmoKind::Move));
    assert_eq!(gizmo.origin, Vec3::new(4.0, 0.0, 0.0));
    assert_eq!(gizmo.size, Vec3::splat(2.0));
    rig.core.set_mode(SelectMode::Rotate);
    let out = rig.frame(Rig::px(0.0, 0.0), ButtonState::default());
    assert_eq!(out.gizmo.and_then(|g| g.kind), Some(GizmoKind::Rotate));
    assert!(rig.mock.asked.borrow().contains(&GizmoKind::Rotate));
    select(&mut rig.core, &[Target::ModelOrigin]);
    let out = rig.frame(Rig::px(0.0, 0.0), ButtonState::default());
    assert!(out.gizmo.is_none(), "the model origin only moves");
}

#[test]
fn hovering_a_handle_sets_the_active_axis() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    rig.grab(GizmoPart::MoveArrow, X, Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::default());
    assert_eq!(rig.core.interaction().active_axis, X);
    rig.mock.gizmo.set(None);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::default());
    assert_eq!(rig.core.interaction().active_axis, Axes::default());
}

#[test]
fn a_move_drag_follows_the_plane_on_the_grabbed_axis_with_snapping() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(1)]);
    let before = state(&rig.core, true);
    rig.grab(GizmoPart::MoveArrow, X, Vec3::new(5.0, 1.0, 0.0));
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(5.0, 1.0), ButtonState::PRESSED);
    assert!(rig.core.interaction().busy());
    assert_eq!(rig.core.history().undo_len(), 9, "nothing moved yet");

    rig.mock.plane.set(Vec3::new(3.2, 5.0, 1.0));
    let out = rig.frame(Rig::px(5.5, 1.0), ButtonState::HELD);
    assert_eq!(
        rig.element(1).base.p,
        Vec3::new(7.0, 0.0, 0.0),
        "x snapped to 0.5, y and z masked"
    );
    assert_eq!(rig.element(1).base.o, Vec3::new(7.0, 0.0, 0.0));
    assert!(out.effects.refresh.contains(cuboid_core::Refresh::GIZMOS));
    assert_eq!(rig.core.history().undo_len(), 10);
    assert_eq!(rig.core.history().undo_label(), Some("Move selection"));

    rig.mock.plane.set(Vec3::new(3.3, 0.0, 0.0));
    rig.frame(Rig::px(5.6, 1.0), ButtonState::HELD);
    assert_eq!(rig.element(1).base.p.x, 7.5);
    rig.frame(Rig::px(5.6, 1.0), ButtonState::RELEASED);
    assert!(!rig.core.interaction().busy());
    assert_eq!(
        rig.core.history().undo_len(),
        10,
        "a whole drag is one step"
    );
    assert!(
        rig.core
            .selection()
            .contains(rig.core.selection().primary.unwrap())
    );

    assert!(rig.core.undo().is_done());
    assert_same(&state(&rig.core, true), &before, "undo of the drag");
    assert!(rig.core.redo().is_done());
    assert_eq!(rig.element(1).base.p.x, 7.5);
}

#[test]
fn dragging_the_primary_moves_the_rest_of_the_selection_with_it() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(1), Target::Element(2)]);
    rig.grab(GizmoPart::MoveArrow, X, Vec3::ZERO);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(2.0, 0.0, 0.0));
    rig.frame(Rig::px(1.0, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(1.0, 0.0), ButtonState::RELEASED);
    assert_eq!(rig.element(1).base.p.x, 6.0);
    assert_eq!(rig.element(2).base.p.x, 10.0);
    assert_eq!(rig.element(0).base.p.x, 0.0);
}

#[test]
fn a_drag_without_movement_leaves_no_undo_step() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    rig.grab(GizmoPart::MoveArrow, X, Vec3::ZERO);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::RELEASED);
    assert_eq!(rig.core.history().undo_len(), 9);
}

#[test]
fn a_locked_object_refuses_the_drag_and_the_press_selects_nothing() {
    let mut rig = Rig::new();
    run(
        &mut rig.core,
        Command::SetLocked {
            targets: vec![Target::Element(0)],
            locked: true,
        },
    );
    select(&mut rig.core, &[Target::Element(0)]);
    let steps = rig.core.history().undo_len();
    rig.grab(GizmoPart::MoveArrow, X, Vec3::ZERO);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    assert!(!rig.core.interaction().busy());
    rig.mock.plane.set(Vec3::new(4.0, 0.0, 0.0));
    rig.frame(Rig::px(2.0, 0.0), ButtonState::HELD);
    assert_eq!(rig.element(0).base.p.x, 0.0);
    assert_eq!(rig.core.history().undo_len(), steps);
    assert_eq!(rig.selected(), [Target::Element(0)]);
}

#[test]
fn a_rotate_drag_turns_one_degree_per_pixel_and_snaps() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    rig.core.set_mode(SelectMode::Rotate);
    rig.grab(GizmoPart::RotateRing, Y, Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    // 45 pixels to the right: y gets minus 45 degrees, which is 315.
    rig.frame(Vec2::new(400.0 + 45.0, 300.0), ButtonState::HELD);
    assert_eq!(rig.element(0).base.e, Vec3::new(0.0, 315.0, 0.0));
    rig.frame(Vec2::new(400.0 + 45.0, 300.0), ButtonState::RELEASED);
    assert_eq!(rig.core.history().undo_len(), 10);
    assert_eq!(rig.core.history().undo_label(), Some("Rotate selection"));
    assert!(rig.core.undo().is_done());
    assert_eq!(rig.element(0).base.e, Vec3::ZERO);
}

#[test]
fn a_rotate_drag_stays_on_the_grid_of_the_setter() {
    let mut rig = Rig::new();
    rig.core.snap_mut().enabled = false;
    select(&mut rig.core, &[Target::Element(0)]);
    rig.core.set_mode(SelectMode::Rotate);
    rig.grab(GizmoPart::RotateRing, Y, Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.frame(Vec2::new(400.0 + 30.0, 300.0), ButtonState::HELD);
    // -30 is 330, and the restricted grid of 22.5 degrees rounds it to 337.5.
    assert_eq!(rig.element(0).base.e.y, 337.5);
}

#[test]
fn a_scale_drag_grows_the_size_toward_the_dragged_side() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    rig.core.set_mode(SelectMode::Scale);
    // The handle sits on the +x face of the box [0, 2].
    rig.grab(GizmoPart::ScaleArrow, X, Vec3::new(2.0, 1.0, -1.0));
    rig.mock.plane.set(Vec3::new(2.0, 1.0, 0.0));
    rig.frame(Rig::px(2.0, 1.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(3.0, 1.0, 0.0));
    rig.frame(Rig::px(3.0, 1.0), ButtonState::HELD);
    rig.frame(Rig::px(3.0, 1.0), ButtonState::RELEASED);
    assert_eq!(rig.element(0).base.s, Vec3::new(3.0, 2.0, 2.0));
    assert_eq!(rig.core.history().undo_label(), Some("Scale selection"));
    assert!(rig.core.undo().is_done());
    assert_eq!(rig.element(0).base.s, Vec3::splat(2.0));
}

#[test]
fn dragging_the_handle_of_the_far_side_inward_shrinks_it() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    rig.core.set_mode(SelectMode::Scale);
    // The handle sits on the -x face; moving it toward +x makes the box smaller.
    rig.grab(GizmoPart::ScaleArrow, X, Vec3::new(0.0, 1.0, -1.0));
    rig.mock.plane.set(Vec3::new(0.0, 1.0, 0.0));
    rig.frame(Rig::px(0.0, 1.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(1.0, 1.0, 0.0));
    rig.frame(Rig::px(1.0, 1.0), ButtonState::HELD);
    assert_eq!(rig.element(0).base.s.x, 1.0);
}

#[test]
fn the_mouse_over_ui_does_nothing() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    rig.grab(GizmoPart::MoveArrow, X, Vec3::ZERO);
    let mut input = rig.input(Rig::px(0.0, 0.0), ButtonState::PRESSED, NO_MODS);
    input.over_ui = true;
    let out = rig.core.tick(&input, &rig.mock);
    assert!(!rig.core.interaction().busy());
    assert!(out.hovered.is_none() && out.cursor.sprite == cuboid_core::CursorSprite::Hidden);
}

#[test]
fn undoing_in_the_middle_of_a_drag_ends_the_drag() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    rig.grab(GizmoPart::MoveArrow, X, Vec3::ZERO);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(2.0, 0.0, 0.0));
    rig.frame(Rig::px(1.0, 0.0), ButtonState::HELD);
    assert!(rig.core.undo().is_done());
    assert!(!rig.core.interaction().busy());
    rig.frame(Rig::px(2.0, 0.0), ButtonState::HELD);
    assert_eq!(
        rig.element(0).base.p.x,
        0.0,
        "the held button no longer drags"
    );
}

#[test]
fn a_model_origin_can_be_dragged() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::ModelOrigin]);
    rig.grab(GizmoPart::MoveArrow, X, Vec3::ZERO);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(2.0, 0.0, 0.0));
    rig.frame(Rig::px(1.0, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(1.0, 0.0), ButtonState::RELEASED);
    assert_eq!(rig.core.doc().props.general.origin.x, 2.0);
    assert!(rig.core.undo().is_done());
    assert_eq!(rig.core.doc().props.general.origin.x, 0.0);
}

/// Records the calls of a stroke.
struct Probe(std::rc::Rc<RefCell<Vec<String>>>);

impl CustomTool for Probe {
    fn begin(&mut self, _cx: &mut ToolCx<'_>, button: Button, _hit: &Hit) -> StrokeStep {
        self.0.borrow_mut().push(format!("begin {button:?}"));
        StrokeStep::Continue
    }

    fn drag(&mut self, _cx: &mut ToolCx<'_>, hit: Option<&Hit>) -> StrokeStep {
        self.0.borrow_mut().push(format!("drag {}", hit.is_some()));
        StrokeStep::Continue
    }

    fn end(&mut self, _cx: &mut ToolCx<'_>) {
        self.0.borrow_mut().push("end".into());
    }
}

#[test]
fn a_tool_takes_the_pointer_and_clears_the_selection() {
    let mut rig = Rig::new();
    select(&mut rig.core, &[Target::Element(0)]);
    let log = std::rc::Rc::new(RefCell::new(Vec::new()));
    rig.core.set_tool(Some(Box::new(Probe(log.clone()))));
    assert_eq!(rig.core.mode(), SelectMode::Custom);
    assert!(rig.selected().is_empty());

    rig.point_at(1);
    let out = rig.frame(Rig::px(5.0, 1.0), ButtonState::PRESSED);
    assert!(out.gizmo.is_none());
    rig.mock.model.set(None);
    rig.frame(Rig::px(5.0, 1.0), ButtonState::HELD);
    rig.frame(Rig::px(5.0, 1.0), ButtonState::RELEASED);
    assert_eq!(*log.borrow(), ["begin Left", "drag false", "end"]);
    assert!(
        rig.selected().is_empty(),
        "a click with a tool does not select"
    );

    rig.core.set_tool(None);
    assert_eq!(rig.core.mode(), SelectMode::Move);
}

/// A tool that ends on the first drag frame and hands over by selecting an object.
struct Handover(ObjRef);

impl CustomTool for Handover {
    fn begin(&mut self, _cx: &mut ToolCx<'_>, _button: Button, _hit: &Hit) -> StrokeStep {
        StrokeStep::SelectLater(self.0)
    }

    fn drag(&mut self, _cx: &mut ToolCx<'_>, _hit: Option<&Hit>) -> StrokeStep {
        StrokeStep::End
    }

    fn end(&mut self, _cx: &mut ToolCx<'_>) {}
}

#[test]
fn a_tool_can_hand_over_by_selecting_an_object() {
    let mut rig = Rig::new();
    let id = rig.core.doc().elements.ids()[2];
    rig.core
        .set_tool(Some(Box::new(Handover(ObjRef::Element(id)))));
    rig.point_at(0);
    rig.frame(Rig::px(1.0, 1.0), ButtonState::PRESSED);
    assert_eq!(rig.selected(), [Target::Element(2)]);
    assert_eq!(rig.core.mode(), SelectMode::Move, "the tool is off");
}
