//! The Animate mode of the core: bones are selected, the gizmo stands on the posed bone and a
//! drag keys the channel at the playhead (animate A3 2 and 5).

mod common;

use std::cell::Cell;
use std::time::{Duration, Instant};

use common::{assert_same, run, scene, select, state};
use cuboid_core::{
    BoneChannel, ButtonState, Command, Core, InputFrame, KeyInterp, KeyValue, Playhead, SelectMode,
    Target, ViewInput,
};
use cuboid_model::{Channel, ObjRef};
use cuboid_scene::{
    Axes, GizmoHit, GizmoKind, GizmoPart, GizmoState, Hit, HitTarget, Ray, Raycaster, Rect, ViewId,
};
use glam::{Mat4, Quat, Vec2, Vec3};

/// Answers picking from values the test sets.
#[derive(Default)]
struct Mock {
    model: Cell<Option<Hit>>,
    gizmo: Cell<Option<GizmoHit>>,
    plane: Cell<Vec3>,
}

impl Raycaster for Mock {
    fn pick_model(&self, _ray: Ray, _max_len: f32) -> Option<Hit> {
        self.model.get()
    }

    fn pick_gizmo(&self, _ray: Ray, _view: ViewId, _gizmo: &GizmoState) -> Option<GizmoHit> {
        self.gizmo.get()
    }

    fn drag_plane(&self, _ray: Ray, _through: Vec3, _max_len: f32) -> Option<Vec3> {
        Some(self.plane.get())
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
const Z: Axes = Axes {
    x: false,
    y: false,
    z: true,
};
struct Rig {
    core: Core,
    mock: Mock,
    t0: Instant,
    frames: u64,
}

impl Rig {
    /// The three cubes with a group "arm" over the first and an animation "walk" of two seconds.
    fn new() -> Self {
        let mut core = scene();
        select(&mut core, &[Target::Element(0)]);
        run(
            &mut core,
            Command::Group {
                targets: vec![],
                name: "arm".into(),
            },
        );
        run(
            &mut core,
            Command::AddAnimation {
                name: "walk".into(),
                length: 2.0,
            },
        );
        core.snap_mut().enabled = false;
        Rig {
            core,
            mock: Mock::default(),
            t0: Instant::now(),
            frames: 0,
        }
    }

    /// World (x, y) to canvas pixels of the test view: 40 pixels per unit, origin in the middle.
    fn px(x: f32, y: f32) -> Vec2 {
        Vec2::new(400.0 + 40.0 * x, 300.0 - 40.0 * y)
    }

    fn frame(&mut self, pointer: Vec2, left: ButtonState) {
        self.frames += 1;
        let mut input = InputFrame::idle(self.t0 + Duration::from_millis(16 * self.frames));
        input.pointer = Some(pointer);
        input.left = left;
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
        self.core.tick(&input, &self.mock);
    }

    fn click(&mut self, pointer: Vec2) {
        self.frame(pointer, ButtonState::PRESSED);
        self.frame(pointer, ButtonState::RELEASED);
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

    fn grab(&self, part: GizmoPart, axes: Axes) {
        self.mock.gizmo.set(Some(GizmoHit {
            part,
            axes,
            point: Vec3::ZERO,
            distance: 1.0,
        }));
    }

    /// Shows the first animation at `time`, as the Animate mode does.
    fn animate(&mut self, time: f32) {
        let animation = self.core.doc().animations.ids().first().copied();
        self.core.set_animating(Some(Playhead { animation, time }));
    }

    fn key(&mut self, channel: BoneChannel, time: f32, value: [f32; 3]) {
        run(
            &mut self.core,
            Command::SetKeyframe {
                animation: 0,
                group: Target::Group(0),
                channel,
                time,
                value: KeyValue::nums(value),
                interp: Some(KeyInterp::Linear),
            },
        );
    }

    /// A drag from the press to a pointer that moves `pixels` to the right.
    fn drag_by(&mut self, pixels: f32) {
        let start = Rig::px(0.0, 0.0);
        self.frame(start, ButtonState::PRESSED);
        self.frame(start + Vec2::new(pixels, 0.0), ButtonState::HELD);
        self.frame(start + Vec2::new(pixels, 0.0), ButtonState::RELEASED);
    }

    /// The keyframes of the channel of the first bone: time and value.
    fn keys(&self, channel: Channel) -> Vec<(f32, Vec3)> {
        let doc = self.core.doc();
        let group = doc.groups.ids()[0];
        doc.animations.items()[0]
            .track(group)
            .map_or_else(Vec::new, |track| {
                track
                    .channel(channel)
                    .iter()
                    .map(|k| {
                        let v = k.value.each_ref().map(|v| v.number().unwrap());
                        (k.time, Vec3::from_array(v))
                    })
                    .collect()
            })
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.abs_diff_eq(b, 1e-4)
}

/// Selects `targets`, which may be selected already.
fn pick(core: &mut Core, targets: &[Target]) {
    core.execute(Command::Select {
        targets: targets.to_vec(),
        op: cuboid_core::SelectOp::Replace,
    });
}

#[test]
fn a_move_drag_keys_the_position_at_the_playhead_and_leaves_the_rest_pose_alone() {
    let mut rig = Rig::new();
    pick(&mut rig.core, &[Target::Group(0)]);
    rig.animate(0.5);
    let before = state(&rig.core, true);
    let groups = rig.core.doc().groups.rev();
    let pivot = rig.core.doc().groups.items()[0].origin;
    let steps = rig.core.history().undo_len();
    assert_eq!(rig.core.gizmo_state().map(|g| g.origin), Some(pivot));

    rig.grab(GizmoPart::MoveArrow, X);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(2.0, 5.0, 1.0));
    rig.frame(Rig::px(1.0, 0.0), ButtonState::HELD);
    // The key is written while the pointer moves, and the gizmo follows the posed bone.
    assert_eq!(rig.keys(Channel::Position).len(), 1);
    assert!(close(
        rig.core.gizmo_state().unwrap().origin,
        pivot + Vec3::new(2.0, 0.0, 0.0)
    ));
    rig.mock.plane.set(Vec3::new(3.0, 0.0, 0.0));
    rig.frame(Rig::px(1.5, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(1.5, 0.0), ButtonState::RELEASED);

    let keys = rig.keys(Channel::Position);
    assert_eq!(keys.len(), 1, "one key, updated every frame");
    assert!((keys[0].0 - 0.5).abs() < 1e-6 && close(keys[0].1, Vec3::new(3.0, 0.0, 0.0)));
    assert_eq!(rig.core.history().undo_len(), steps + 1, "one undo step");
    assert_eq!(rig.core.history().undo_label(), Some("Key position"));
    assert_eq!(
        rig.core.doc().groups.rev(),
        groups,
        "the rest pose is untouched"
    );
    assert_eq!(rig.core.doc().groups.items()[0].origin, pivot);
    assert_eq!(rig.core.doc().elements.items()[0].base.p, Vec3::ZERO);

    assert!(rig.core.undo().is_done());
    assert!(rig.keys(Channel::Position).is_empty());
    assert_same(&state(&rig.core, true), &before, "undo of the keyed drag");
    assert_eq!(rig.core.gizmo_state().map(|g| g.origin), Some(pivot));
    assert!(rig.core.redo().is_done());
    assert_eq!(rig.keys(Channel::Position).len(), 1);
    assert!(
        close(
            rig.core.gizmo_state().unwrap().origin,
            pivot + Vec3::new(3.0, 0.0, 0.0)
        ),
        "redo brings the posed bone back"
    );
}

#[test]
fn a_move_drag_under_a_turned_parent_keys_the_motion_in_the_parent_frame() {
    // A bone "hand" under the arm; the arm turns a quarter about Z at the playhead, so a move of
    // 2 units along the world X is a move of 2 units along the arm's -Y.
    let mut rig = Rig::new();
    pick(&mut rig.core, &[Target::Element(1)]);
    run(
        &mut rig.core,
        Command::Group {
            targets: vec![],
            name: "hand".into(),
        },
    );
    run(
        &mut rig.core,
        Command::Reparent {
            targets: vec![Target::Group(1)],
            parent: Some(Target::Group(0)),
            index: None,
        },
    );
    rig.key(BoneChannel::Rotation, 0.0, [0.0, 0.0, 90.0]);
    rig.animate(0.0);
    pick(&mut rig.core, &[Target::Group(1)]);
    rig.grab(GizmoPart::MoveArrow, X);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(2.0, 0.0, 0.0));
    rig.frame(Rig::px(1.0, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(1.0, 0.0), ButtonState::RELEASED);
    let doc = rig.core.doc();
    let hand = doc.groups.ids()[1];
    let key = &doc.animations.items()[0].track(hand).unwrap().position[0];
    let value = Vec3::from_array(key.value.each_ref().map(|v| v.number().unwrap()));
    assert!(close(value, Vec3::new(0.0, -2.0, 0.0)), "{value:?}");
    // The hand rests 4 units from the arm's pivot, which the quarter turn put above it; the
    // drag moved it by what the pointer asked for in the world.
    let origin = rig.core.gizmo_state().unwrap().origin;
    assert!(close(origin, Vec3::new(3.0, 5.0, -1.0)), "{origin:?}");
}

#[test]
fn a_rotate_drag_keys_the_same_turn_the_rest_pose_gets_in_the_model_mode() {
    for (axes, name) in [(X, "x"), (Y, "y"), (Z, "z")] {
        // The same ring, the same pixels, on a bone that rests without a turn and has no parent.
        let turned = |animate: bool| {
            let mut rig = Rig::new();
            pick(&mut rig.core, &[Target::Group(0)]);
            rig.core.set_mode(SelectMode::Rotate);
            if animate {
                rig.animate(0.5);
            }
            rig.grab(GizmoPart::RotateRing, axes);
            rig.mock.plane.set(Vec3::ZERO);
            // A step of the restricted rotation of the rest pose, so both turn.
            rig.drag_by(45.0);
            rig
        };
        let (model, animated) = (turned(false), turned(true));
        let world = |rig: &Rig| rig.core.gizmo_state().expect("a gizmo").rotation;
        let (rest, posed) = (world(&model), world(&animated));
        assert!(
            rest.dot(posed).abs() > 0.999_99,
            "{name}: model {rest:?}, animate {posed:?}"
        );
        assert!(
            rest.angle_between(Quat::IDENTITY) > 0.1,
            "{name}: it turned"
        );
        assert_eq!(animated.keys(Channel::Rotation).len(), 1, "{name}");
        // The drag in Model changed the rest rotation, the one in Animate changed no group.
        assert_ne!(model.core.doc().groups.items()[0].rotation, Vec3::ZERO);
        assert_eq!(animated.core.doc().groups.items()[0].rotation, Vec3::ZERO);
    }
}

#[test]
fn a_scale_drag_multiplies_the_scale_the_channel_has_at_the_playhead() {
    let mut rig = Rig::new();
    rig.key(BoneChannel::Scale, 0.0, [1.0, 1.0, 1.0]);
    rig.key(BoneChannel::Scale, 1.0, [3.0, 1.0, 1.0]);
    rig.core.set_mode(SelectMode::Scale);
    pick(&mut rig.core, &[Target::Group(0)]);
    rig.animate(0.5);
    let gizmo = rig.core.gizmo_state().expect("a scale gizmo");
    assert_eq!(gizmo.kind, Some(GizmoKind::Scale));
    // The cube is 2 units each way and the channel doubles it along X at the playhead.
    assert!(
        close(gizmo.size, Vec3::new(4.0, 2.0, 2.0)),
        "{:?}",
        gizmo.size
    );
    rig.mock.gizmo.set(Some(GizmoHit {
        part: GizmoPart::ScaleArrow,
        axes: X,
        point: Vec3::new(5.0, 1.0, -1.0),
        distance: 1.0,
    }));
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(2.0, 0.0, 0.0));
    rig.frame(Rig::px(1.0, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(1.0, 0.0), ButtonState::RELEASED);
    let keys = rig.keys(Channel::Scale);
    // The channel was 2 at the playhead (halfway from 1 to 3); the box grew from 4 to 6 units,
    // which is a factor of 1.5.
    assert_eq!(keys.len(), 3);
    assert!((keys[1].0 - 0.5).abs() < 1e-6);
    assert!(
        close(keys[1].1, Vec3::new(3.0, 1.0, 1.0)),
        "{:?}",
        keys[1].1
    );
    assert_eq!(rig.core.history().undo_label(), Some("Key scale"));
}

#[test]
fn a_drag_between_keys_keys_the_interpolated_value() {
    let mut rig = Rig::new();
    rig.key(BoneChannel::Rotation, 0.0, [0.0; 3]);
    rig.key(BoneChannel::Rotation, 1.0, [90.0, 0.0, 0.0]);
    pick(&mut rig.core, &[Target::Group(0)]);
    rig.core.set_mode(SelectMode::Rotate);
    rig.animate(0.5);
    rig.grab(GizmoPart::RotateRing, X);
    rig.mock.plane.set(Vec3::ZERO);
    rig.drag_by(10.0);
    let keys = rig.keys(Channel::Rotation);
    assert_eq!(keys.len(), 3);
    assert!((keys[1].0 - 0.5).abs() < 1e-6);
    // Halfway between 0 and 90, then the drag of 10 pixels in the sign of a project.
    assert!((keys[1].1.x - 35.0).abs() < 1e-3, "{:?}", keys[1].1);
    assert!(close(keys[0].1, Vec3::ZERO) && close(keys[2].1, Vec3::new(90.0, 0.0, 0.0)));
}

#[test]
fn keys_land_on_the_frame_grid_and_a_locked_bone_is_not_keyed() {
    let mut rig = Rig::new();
    pick(&mut rig.core, &[Target::Group(0)]);
    // 24 frames per second: 0.51 s is frame 12 (0.5 s).
    rig.animate(0.51);
    rig.grab(GizmoPart::MoveArrow, X);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    rig.mock.plane.set(Vec3::new(1.0, 0.0, 0.0));
    rig.frame(Rig::px(1.0, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(1.0, 0.0), ButtonState::RELEASED);
    assert!((rig.keys(Channel::Position)[0].0 - 0.5).abs() < 1e-6);

    run(
        &mut rig.core,
        Command::SetLocked {
            targets: vec![Target::Group(0)],
            locked: true,
        },
    );
    let steps = rig.core.history().undo_len();
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    assert!(!rig.core.interaction().busy());
    rig.mock.plane.set(Vec3::new(4.0, 0.0, 0.0));
    rig.frame(Rig::px(2.0, 0.0), ButtonState::HELD);
    rig.frame(Rig::px(2.0, 0.0), ButtonState::RELEASED);
    assert_eq!(rig.core.history().undo_len(), steps);
    assert!(close(
        rig.keys(Channel::Position)[0].1,
        Vec3::new(1.0, 0.0, 0.0)
    ));
}

#[test]
fn without_an_animation_there_is_no_gizmo_and_a_click_says_why() {
    let mut rig = Rig::new();
    run(&mut rig.core, Command::RemoveAnimation { animation: 0 });
    pick(&mut rig.core, &[Target::Group(0)]);
    assert!(rig.core.gizmo_state().is_some(), "the rest pose shows one");
    rig.core.set_animating(Some(Playhead {
        animation: None,
        time: 0.0,
    }));
    assert!(rig.core.gizmo_state().is_none());
    assert_eq!(rig.core.status_text(), "Add an animation to key the pose");
    rig.grab(GizmoPart::MoveArrow, X);
    rig.mock.plane.set(Vec3::ZERO);
    rig.frame(Rig::px(0.0, 0.0), ButtonState::PRESSED);
    assert!(!rig.core.interaction().busy(), "nothing to key");
    rig.frame(Rig::px(0.0, 0.0), ButtonState::RELEASED);
    rig.mock.gizmo.set(None);
    rig.point_at(0);
    rig.click(Rig::px(1.0, 1.0));
    assert_eq!(rig.core.status_text(), "Add an animation to key the pose");
    rig.core.set_animating(None);
    assert!(rig.core.gizmo_state().is_some());
    assert_eq!(rig.core.animating(), None);
}

#[test]
fn a_click_on_an_object_selects_its_bone_and_one_outside_every_group_clears() {
    let mut rig = Rig::new();
    rig.animate(0.0);
    rig.point_at(0);
    rig.click(Rig::px(1.0, 1.0));
    assert_eq!(
        rig.core
            .selection()
            .list
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [ObjRef::Group(rig.core.doc().groups.ids()[0])]
    );
    rig.point_at(2);
    rig.click(Rig::px(9.0, 1.0));
    assert!(rig.core.selection().list.is_empty());
    assert_eq!(
        rig.core.status_text(),
        "Only groups are animated: put the object into a group"
    );
    // Outside Animate the same click selects the element.
    rig.core.set_animating(None);
    rig.point_at(2);
    rig.click(Rig::px(9.0, 1.0));
    assert_eq!(
        rig.core
            .selection()
            .list
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [ObjRef::Element(rig.core.doc().elements.ids()[2])]
    );
}

#[test]
fn a_box_selects_the_groups_of_the_objects_it_catches_where_the_pose_puts_them() {
    let mut rig = Rig::new();
    pick(&mut rig.core, &[Target::Element(1)]);
    run(
        &mut rig.core,
        Command::Group {
            targets: vec![],
            name: "leg".into(),
        },
    );
    // The arm sits 10 units to the right at the playhead; the leg stays.
    rig.key(BoneChannel::Position, 0.0, [10.0, 0.0, 0.0]);
    rig.animate(0.0);
    rig.mock.model.set(None);
    rig.frame(Rig::px(-1.0, -1.0), ButtonState::PRESSED);
    rig.frame(Rig::px(7.0, 3.0), ButtonState::HELD);
    rig.frame(Rig::px(7.0, 3.0), ButtonState::RELEASED);
    let groups = rig.core.doc().groups.ids().to_vec();
    assert_eq!(
        rig.core
            .selection()
            .list
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [ObjRef::Group(groups[1])],
        "the leg is inside the box, the arm moved out"
    );
    // The same box in the rest pose catches the arm's cube and the ungrouped neighbours.
    rig.core.set_animating(None);
    rig.frame(Rig::px(-1.0, -1.0), ButtonState::PRESSED);
    rig.frame(Rig::px(7.0, 3.0), ButtonState::HELD);
    rig.frame(Rig::px(7.0, 3.0), ButtonState::RELEASED);
    assert_eq!(rig.core.selection().list.len(), 2);
}
