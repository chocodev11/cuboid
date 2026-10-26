//! The pointer path of the viewport, end to end without a GPU: synthetic egui input, the UI
//! frame, its requests applied by the app, and the resulting camera.

use std::time::Instant;

use cuboid_actions::ActionId;
use cuboid_core::Command;
use cuboid_settings::{Environment, Settings};
use cuboid_ui::{FontSet, NavPreset, Prefs, UiRequest};
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, vec2};

use super::*;
use crate::io_thread::IoThread;
use crate::platform::Dirs;

/// What a drag or a wheel turn did to the main camera.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Moved {
    Nothing,
    Orbit,
    Pan,
    Zoom,
}

struct Rig {
    state: State,
    front: Frontend,
    ctx: egui::Context,
    time: f64,
    pointer: Pos2,
    mods: Modifiers,
    /// Dropped after the state: its IO thread writes the last files while it shuts down, and
    /// would make the folder again.
    _dir: tempfile::TempDir,
}

const NONE: Modifiers = Modifiers::NONE;
const SHIFT: Modifiers = Modifiers::SHIFT;
const CTRL: Modifiers = Modifiers::COMMAND;
const ALT: Modifiers = Modifiers::ALT;

impl Rig {
    fn new(nav: NavPreset) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::first_run(&Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        });
        let state = State::new(
            Dirs::under(dir.path()),
            settings,
            IoThread::spawn().unwrap(),
            1252,
            Instant::now(),
        );
        let prefs = Prefs {
            nav,
            nav_chosen: true,
            ..Prefs::default()
        };
        let front = Frontend::new(prefs, FontSet::default(), dir.path().join("ui.json"));
        let mut rig = Rig {
            _dir: dir,
            state,
            front,
            ctx: egui::Context::default(),
            time: 0.0,
            pointer: Pos2::ZERO,
            mods: NONE,
        };
        for _ in 0..3 {
            rig.frame(Vec::new(), NONE);
        }
        rig.centre();
        rig
    }

    /// Puts the pointer in the middle of the main view.
    fn centre(&mut self) {
        let view = self.front.placements[0];
        self.pointer = Pos2::new(
            view.rect[0] + view.rect[2] * 0.5,
            view.rect[1] + view.rect[3] * 0.5,
        );
        self.frame(vec![Event::PointerMoved(self.pointer)], NONE);
    }

    /// One frame with the given events, as the shell runs it.
    fn frame(&mut self, events: Vec<Event>, modifiers: Modifiers) {
        self.frame_at(Instant::now(), events, modifiers);
    }

    /// One frame at the clock `now`.
    fn frame_at(&mut self, now: Instant, mut events: Vec<Event>, modifiers: Modifiers) {
        if modifiers != self.mods {
            self.mods = modifiers;
            events.insert(0, Event::ModifiersChanged(modifiers));
        }
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        self.time += 1.0 / 60.0;
        self.ctx.begin_pass(raw);
        self.front.update(&mut self.state, &self.ctx, now);
        self.ctx.end_pass().drop_without_applying_deltas();
    }

    fn button(&mut self, button: PointerButton, pressed: bool, modifiers: Modifiers) {
        let event = Event::PointerButton {
            pos: self.pointer,
            button,
            pressed,
            modifiers,
        };
        self.frame(vec![event], modifiers);
    }

    /// Press, move by `by` in a few steps, release.
    fn drag(&mut self, button: PointerButton, modifiers: Modifiers, by: egui::Vec2) {
        self.button(button, true, modifiers);
        for _ in 0..4 {
            self.pointer += by / 4.0;
            self.frame(vec![Event::PointerMoved(self.pointer)], modifiers);
        }
        self.button(button, false, modifiers);
        self.frame(Vec::new(), NONE);
    }

    fn wheel(&mut self, unit: MouseWheelUnit, delta: f32) {
        let event = Event::MouseWheel {
            unit,
            delta: vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: NONE,
        };
        self.frame(vec![event], NONE);
    }

    fn camera(&self) -> (Vec3, Vec3, f32) {
        let main = &self.front.views.views[3].camera;
        (main.pos, self.front.views.pivot, main.ortho_size)
    }

    /// What `act` did to the main camera.
    fn moved(&mut self, act: impl FnOnce(&mut Rig)) -> Moved {
        self.centre();
        let (pos, pivot, _) = self.camera();
        let dist = (pos - pivot).length();
        act(self);
        let (pos2, pivot2, _) = self.camera();
        let dist2 = (pos2 - pivot2).length();
        if (pivot2 - pivot).length() > 1e-4 {
            Moved::Pan
        } else if (dist2 - dist).abs() > 1e-4 {
            Moved::Zoom
        } else if (pos2 - pos).length() > 1e-4 {
            Moved::Orbit
        } else {
            Moved::Nothing
        }
    }
}

const DRAG: egui::Vec2 = vec2(60.0, 24.0);

#[test]
fn blockbench_style_navigation() {
    let mut rig = Rig::new(NavPreset::Blockbench);
    let case = |rig: &mut Rig, button, mods, want| {
        let got = rig.moved(|r| r.drag(button, mods, DRAG));
        assert_eq!(got, want, "{button:?} {mods:?}");
    };
    case(&mut rig, PointerButton::Primary, NONE, Moved::Orbit);
    case(&mut rig, PointerButton::Secondary, NONE, Moved::Pan);
    case(&mut rig, PointerButton::Middle, NONE, Moved::Pan);
    case(&mut rig, PointerButton::Middle, CTRL, Moved::Zoom);
    case(&mut rig, PointerButton::Primary, ALT, Moved::Orbit);
    case(&mut rig, PointerButton::Primary, SHIFT, Moved::Nothing);
    case(&mut rig, PointerButton::Primary, CTRL, Moved::Nothing);
    wheel_cases(&mut rig);
}

#[test]
fn blender_style_navigation() {
    let mut rig = Rig::new(NavPreset::Blender);
    let case = |rig: &mut Rig, button, mods, want| {
        let got = rig.moved(|r| r.drag(button, mods, DRAG));
        assert_eq!(got, want, "{button:?} {mods:?}");
    };
    case(&mut rig, PointerButton::Middle, NONE, Moved::Orbit);
    case(&mut rig, PointerButton::Middle, SHIFT, Moved::Pan);
    case(&mut rig, PointerButton::Middle, CTRL, Moved::Zoom);
    case(&mut rig, PointerButton::Primary, ALT, Moved::Orbit);
    case(&mut rig, PointerButton::Primary, NONE, Moved::Nothing);
    case(&mut rig, PointerButton::Secondary, NONE, Moved::Nothing);
    wheel_cases(&mut rig);
}

#[test]
fn cubik_style_navigation() {
    let mut rig = Rig::new(NavPreset::Cubik);
    let case = |rig: &mut Rig, button, mods, want| {
        let got = rig.moved(|r| r.drag(button, mods, DRAG));
        assert_eq!(got, want, "{button:?} {mods:?}");
    };
    case(&mut rig, PointerButton::Middle, NONE, Moved::Pan);
    case(&mut rig, PointerButton::Middle, ALT, Moved::Orbit);
    case(&mut rig, PointerButton::Middle, CTRL, Moved::Orbit);
    case(&mut rig, PointerButton::Primary, NONE, Moved::Nothing);
    case(&mut rig, PointerButton::Secondary, NONE, Moved::Nothing);
    wheel_cases(&mut rig);
}

/// Yaw about +Y and the angle above the horizon of the camera, in degrees.
fn angles(rig: &Rig) -> (f32, f32) {
    let (pos, pivot, _) = rig.camera();
    let from_pivot = pos - pivot;
    let yaw = from_pivot.x.atan2(from_pivot.z).to_degrees();
    let pitch = (from_pivot.y / from_pivot.length()).asin().to_degrees();
    (yaw, pitch)
}

#[test]
fn the_cubik_orbit_turns_048_degrees_across_and_032_down_per_pixel() {
    // The UI scales the pointer travel so that the scene's rate gives these angles.
    assert!(
        (cuboid_scene::ORBIT_DEG_PER_PIXEL * cuboid_ui::CUBIK_ORBIT_SCALE[0] - 0.48).abs() < 1e-6
    );
    assert!(
        (cuboid_scene::ORBIT_DEG_PER_PIXEL * cuboid_ui::CUBIK_ORBIT_SCALE[1] - 0.32).abs() < 1e-6
    );
    let mut rig = Rig::new(NavPreset::Cubik);
    rig.centre();
    let (yaw, pitch) = angles(&rig);
    rig.drag(PointerButton::Middle, ALT, vec2(50.0, 0.0));
    let (turned, same) = angles(&rig);
    let mut across = (turned - yaw).abs();
    if across > 180.0 {
        across = 360.0 - across;
    }
    assert!((across - 24.0).abs() < 0.5, "{across}");
    assert!((same - pitch).abs() < 0.5);
    let (yaw, pitch) = angles(&rig);
    rig.drag(PointerButton::Middle, ALT, vec2(0.0, 20.0));
    let (kept, tilted) = angles(&rig);
    assert!((kept - yaw).abs() < 0.5);
    assert!(
        ((tilted - pitch).abs() - 6.4).abs() < 0.5,
        "{}",
        tilted - pitch
    );
}

// --- Playing an animation (animation A2 6) -----------------------------------------------------

use std::time::Duration;

use cuboid_core::{AnimationLoop, BoneChannel, Core, KeyInterp, KeyValue, Target};
use cuboid_scene::{ExtractBudget, RenderExtract};
use cuboid_ui::AnimationRequest;

impl Rig {
    /// A cube in a group "bone" and an animation "slide" of one second that moves the group up by
    /// 8 units, once.
    fn slide() -> Rig {
        let mut rig = Rig::new(NavPreset::Blockbench);
        let core = &mut rig.state.editor.core;
        core.execute(Command::AddElement(cuboid_core::ElementKind::Cube));
        core.execute(Command::Group {
            targets: vec![Target::Element(0)],
            name: "bone".into(),
        });
        core.execute(Command::AddAnimation {
            name: "slide".into(),
            length: 1.0,
        });
        for (time, y) in [(0.0, 0.0), (1.0, 8.0)] {
            core.execute(Command::SetKeyframe {
                animation: 0,
                group: Target::Group(0),
                channel: BoneChannel::Position,
                time,
                value: KeyValue::nums([0.0, y, 0.0]),
                interp: Some(KeyInterp::Linear),
            });
        }
        rig.frame(Vec::new(), NONE);
        rig.extract();
        rig
    }

    /// What the extractor writes now, as the renderer would take it.
    fn extract(&mut self) -> RenderExtract {
        let core = &mut self.state.editor.core;
        let prefs = core.env_mut().render.clone();
        let out =
            self.front
                .extractor
                .extract(core.doc_for_extract(), &prefs, &ExtractBudget::UNLIMITED);
        core.clear_journal();
        out
    }

    /// Whether the player wants a frame at once; the other things that keep the window drawing
    /// (the pointer, uploads) are not its business.
    fn asks_for_frame(&mut self) -> bool {
        self.front.more = false;
        self.front.step_player(&mut self.state);
        self.front.more
    }

    fn ask(&mut self, request: AnimationRequest) {
        self.front
            .apply(&mut self.state, UiRequest::Animation(request));
    }

    /// The y of the cube as the last record has it.
    fn height(&self) -> f32 {
        let record = self.front.extractor.cache().instance(0).expect("a record");
        f32::from_bits(record.0[1])
    }

    fn core(&self) -> &Core {
        &self.state.editor.core
    }

    /// Plays frames of 1/4 second from `start` on.
    fn play_for(&mut self, start: Instant, quarters: u32) -> Instant {
        let mut now = start;
        for _ in 0..quarters {
            now += Duration::from_millis(250);
            self.frame_at(now, Vec::new(), NONE);
        }
        now
    }
}

#[test]
fn playing_moves_the_record_of_the_cube_and_stop_puts_it_back() {
    let mut rig = Rig::slide();
    assert!(!rig.asks_for_frame(), "an idle player asks for no frame");
    let rest = rig.height();
    let (revision, steps, dirty) = {
        let core = rig.core();
        (
            core.doc().revision,
            core.history().undo_len(),
            core.is_dirty(),
        )
    };
    let start = Instant::now();
    rig.ask(AnimationRequest::Play);
    rig.frame_at(start, Vec::new(), NONE);
    assert!(
        rig.asks_for_frame(),
        "a playing animation asks for the next frame"
    );
    // The pose at the start is the rest pose, so no record needs to change.
    let out = rig.extract();
    assert!(out.instances.len() <= 1);
    assert!(
        (rig.height() - rest).abs() < 1e-5,
        "the playhead is at the start"
    );

    // A quarter second later the cube is two units up, and only its record is written.
    rig.frame_at(start + Duration::from_millis(250), Vec::new(), NONE);
    let out = rig.extract();
    assert_eq!(out.instances.len(), 1);
    assert!(
        (rig.height() - (rest + 2.0)).abs() < 1e-4,
        "{}",
        rig.height()
    );
    let now = rig.play_for(start + Duration::from_millis(250), 2);
    rig.extract();
    assert!((rig.height() - (rest + 6.0)).abs() < 1e-4);
    let header = rig.front.animations.clone();
    assert!(header.playing && header.previewing);
    assert_eq!(header.preview_name(), Some("slide"));

    // The end: the player stops at the last pose and asks for no more frames.
    let now = rig.play_for(now, 2);
    rig.extract();
    assert!((rig.height() - (rest + 8.0)).abs() < 1e-4);
    assert!(!rig.front.animations.playing);
    assert!(rig.front.animations.previewing, "the last pose stays");
    assert!(!rig.asks_for_frame(), "a stopped player asks for nothing");
    let now = rig.play_for(now, 1);
    assert!(rig.extract().instances.is_empty());

    // Stop returns to the rest pose and the previewing name goes.
    rig.ask(AnimationRequest::Stop);
    rig.frame_at(now + Duration::from_millis(250), Vec::new(), NONE);
    let out = rig.extract();
    assert_eq!(out.instances.len(), 1);
    assert!((rig.height() - rest).abs() < 1e-5);
    assert_eq!(rig.front.animations.preview_name(), None);
    assert!(!rig.asks_for_frame());

    // The document never changed: no revision, no undo step, no unsaved change.
    let core = rig.core();
    assert_eq!(core.doc().revision, revision);
    assert_eq!(core.history().undo_len(), steps);
    assert_eq!(core.is_dirty(), dirty);
}

#[test]
fn pause_keeps_the_pose_and_scrubbing_shows_the_pose_at_the_time() {
    let mut rig = Rig::slide();
    let rest = rig.height();
    let start = Instant::now();
    rig.ask(AnimationRequest::Play);
    rig.frame_at(start, Vec::new(), NONE);
    let now = rig.play_for(start, 2);
    rig.extract();
    let halfway = rig.height();
    assert!((halfway - (rest + 4.0)).abs() < 1e-4, "{halfway}");
    rig.ask(AnimationRequest::Pause);
    let now = rig.play_for(now, 3);
    assert!(!rig.asks_for_frame(), "a paused player asks for no frame");
    assert!(rig.extract().instances.len() <= 1);
    let kept = rig.height();
    let after = rig.play_for(now, 2);
    assert!(
        rig.extract().instances.is_empty(),
        "nothing moves while paused"
    );
    assert!((rig.height() - kept).abs() < 1e-6);

    // Scrubbing shows the pose at the time without playing.
    rig.ask(AnimationRequest::Seek(0.75));
    rig.frame_at(after, Vec::new(), NONE);
    assert!(!rig.asks_for_frame());
    rig.extract();
    assert!((rig.height() - (rest + 6.0)).abs() < 1e-4);
    assert!((rig.front.animations.time - 0.75).abs() < 1e-6);
    // A time past the end or a bad one is brought into range.
    rig.ask(AnimationRequest::Seek(9.0));
    rig.frame_at(after, Vec::new(), NONE);
    assert!((rig.front.animations.time - 1.0).abs() < 1e-6);
    rig.ask(AnimationRequest::Seek(f32::NAN));
    rig.frame_at(after, Vec::new(), NONE);
    assert!(rig.front.animations.time.abs() < 1e-6);
}

#[test]
fn speed_and_loop_decide_how_the_playhead_goes() {
    let mut rig = Rig::slide();
    let start = Instant::now();
    rig.ask(AnimationRequest::SetSpeed(2.0));
    rig.ask(AnimationRequest::SetLoop(true));
    rig.ask(AnimationRequest::Play);
    rig.frame_at(start, Vec::new(), NONE);
    rig.play_for(start, 3);
    // Three quarter seconds at twice the speed is one and a half seconds: one lap and a half.
    let view = rig.front.animations.clone();
    assert!(view.playing, "a loop does not end");
    assert!((view.time - 0.5).abs() < 1e-4, "{}", view.time);
    assert!(rig.asks_for_frame());
    // The speed stays in range.
    let speed = |rig: &mut Rig| rig.front.player.view(rig.state.editor.core.doc()).speed;
    rig.ask(AnimationRequest::SetSpeed(100.0));
    assert!((speed(&mut rig) - 4.0).abs() < 1e-6);
    rig.ask(AnimationRequest::SetSpeed(f32::NAN));
    assert!((speed(&mut rig) - 4.0).abs() < 1e-6);
    rig.ask(AnimationRequest::SetSpeed(0.0));
    assert!((speed(&mut rig) - 0.1).abs() < 1e-6);
}

#[test]
fn a_long_stall_does_not_make_the_animation_jump() {
    let mut rig = Rig::slide();
    let start = Instant::now();
    rig.ask(AnimationRequest::Play);
    rig.frame_at(start, Vec::new(), NONE);
    // A minute passes between two frames: the playhead moves by a quarter second.
    rig.frame_at(start + Duration::from_secs(60), Vec::new(), NONE);
    assert!((rig.front.animations.time - 0.25).abs() < 1e-6);
}

#[test]
fn the_panel_gets_the_list_and_the_state_of_the_player() {
    let mut rig = Rig::slide();
    let view = rig.front.animations.clone();
    assert_eq!(view.rows.len(), 1);
    assert_eq!(&*view.rows[0].name, "slide");
    assert!((view.rows[0].length - 1.0).abs() < 1e-6);
    assert_eq!(
        view.selected, None,
        "nothing is chosen before the first request"
    );
    let id = view.rows[0].id;
    rig.ask(AnimationRequest::Select(id));
    rig.frame(Vec::new(), NONE);
    let view = rig.front.animations.clone();
    assert_eq!(view.selected, Some(0));
    assert!(
        view.previewing && view.time.abs() < 1e-6,
        "choosing an animation shows its pose at the playhead"
    );
    assert!(!view.playing);
    // An id that is not in the list is ignored.
    rig.ask(AnimationRequest::Select(id + 1_000_000));
    rig.frame(Vec::new(), NONE);
    assert_eq!(rig.front.animations.selected, Some(0));
}

#[test]
fn removing_the_animation_that_plays_returns_to_the_rest_pose() {
    let mut rig = Rig::slide();
    let rest = rig.height();
    let start = Instant::now();
    rig.ask(AnimationRequest::Play);
    rig.play_for(start, 2);
    rig.extract();
    assert!(rig.height() > rest + 1.0);
    rig.state
        .editor
        .core
        .execute(Command::RemoveAnimation { animation: 0 });
    rig.frame_at(start + Duration::from_secs(2), Vec::new(), NONE);
    rig.extract();
    assert!((rig.height() - rest).abs() < 1e-5);
    assert!(!rig.front.animations.playing && !rig.front.animations.previewing);
    assert!(rig.front.animations.rows.is_empty());
    assert!(!rig.asks_for_frame());
}

#[test]
fn a_loop_animation_starts_with_the_loop_toggle_on_and_a_new_document_resets_the_player() {
    let mut rig = Rig::slide();
    rig.state.editor.core.execute(Command::SetAnimation {
        animation: 0,
        name: None,
        length: None,
        loop_mode: Some(AnimationLoop::Loop),
        snapping: None,
    });
    rig.frame(Vec::new(), NONE);
    let id = rig.front.animations.rows[0].id;
    rig.ask(AnimationRequest::Select(id));
    rig.frame(Vec::new(), NONE);
    assert!(rig.front.animations.looping);
    rig.ask(AnimationRequest::Play);
    rig.frame(Vec::new(), NONE);
    assert!(rig.front.animations.previewing);

    // A new document has no animations and no pose.
    rig.front.apply(
        &mut rig.state,
        UiRequest::NewDocument(cuboid_ui::Template::Empty),
    );
    for _ in 0..4 {
        rig.frame(Vec::new(), NONE);
    }
    let view = rig.front.animations.clone();
    assert!(view.rows.is_empty() && !view.previewing && !view.playing);
    assert!(!rig.asks_for_frame());
}

// --- The Animate workspace (animate A3 2, 5, 8) ------------------------------------------------

use cuboid_core::SelectOp;
use cuboid_model::{Channel, ObjRef};
use cuboid_ui::{AnimateEdit, ChannelKind, KeyId, NodeId};

impl Rig {
    /// The outliner id of the group "bone".
    fn bone(&self) -> NodeId {
        let group = self.core().doc().groups.ids()[0];
        crate::uimodel::node_id(ObjRef::Group(group)).expect("a node")
    }

    fn edit(&mut self, edit: AnimateEdit) {
        self.front.apply(&mut self.state, UiRequest::Animate(edit));
    }

    fn select_bone(&mut self) {
        self.state.editor.core.execute(Command::Select {
            targets: vec![Target::Group(0)],
            op: SelectOp::Replace,
        });
    }

    /// The keyframes of a channel of the bone: time and value.
    fn keys(&self, channel: Channel) -> Vec<(f32, Vec3)> {
        let doc = self.core().doc();
        doc.animations.items()[0]
            .track(doc.groups.ids()[0])
            .map_or_else(Vec::new, |track| {
                track
                    .channel(channel)
                    .iter()
                    .map(|k| {
                        let v = k.value.each_ref().map(|v| v.number().unwrap_or(f32::NAN));
                        (k.time, Vec3::from_array(v))
                    })
                    .collect()
            })
    }

    fn undo_steps(&self) -> usize {
        self.core().history().undo_len()
    }
}

#[test]
fn the_timeline_of_a_new_document_never_takes_the_revision_of_the_old_one() {
    let mut rig = Rig::slide();
    rig.front.ui.set_mode(Mode::Animate);
    rig.select_bone();
    rig.frame(Vec::new(), NONE);
    let old = rig.front.timeline.view().rev;
    rig.front.apply(
        &mut rig.state,
        UiRequest::NewDocument(cuboid_ui::Template::Empty),
    );
    rig.frame(Vec::new(), NONE);
    // The timeline keeps its rows while the revision stays; rows of the old document would
    // name bones the new one does not have.
    assert!(rig.front.timeline.view().rev > old);
    assert!(rig.front.timeline.view().bones.is_empty());
}

#[test]
fn entering_animate_shows_the_pose_at_the_playhead_and_leaving_shows_the_rest_pose() {
    let mut rig = Rig::slide();
    let rest = rig.height();
    let steps = rig.undo_steps();
    assert_eq!(rig.front.animations.selected, None);
    assert!(rig.core().animating().is_none());
    rig.front.ui.set_mode(Mode::Animate);
    rig.frame(Vec::new(), NONE);
    assert_eq!(
        rig.front.animations.selected,
        Some(0),
        "the first animation is chosen when none is"
    );
    assert!(rig.front.animations.previewing);
    assert!(rig.core().animating().is_some());
    rig.ask(AnimationRequest::Seek(0.5));
    rig.frame(Vec::new(), NONE);
    rig.extract();
    assert!(
        (rig.height() - (rest + 4.0)).abs() < 1e-4,
        "{}",
        rig.height()
    );

    // Leaving puts the rest pose back; the animation and the playhead stay.
    rig.front.ui.set_mode(Mode::Model);
    rig.frame(Vec::new(), NONE);
    rig.extract();
    assert!((rig.height() - rest).abs() < 1e-5);
    assert!(!rig.front.animations.previewing && !rig.front.animations.playing);
    assert_eq!(rig.front.animations.selected, Some(0));
    assert!((rig.front.animations.time - 0.5).abs() < 1e-6);
    assert!(rig.core().animating().is_none());

    // Back in Animate the pose at the playhead is shown again.
    rig.front.ui.set_mode(Mode::Animate);
    rig.frame(Vec::new(), NONE);
    rig.extract();
    assert!((rig.height() - (rest + 4.0)).abs() < 1e-4);
    // The player is not the document: nothing of this is an edit.
    assert_eq!(rig.undo_steps(), steps, "the set-up only");
}

#[test]
fn choosing_an_animation_shows_its_pose_at_the_playhead_clamped_to_its_length() {
    let mut rig = Rig::slide();
    rig.state.editor.core.execute(Command::AddAnimation {
        name: "short".into(),
        length: 0.25,
    });
    rig.frame(Vec::new(), NONE);
    let (slide, short) = (
        rig.front.animations.rows[0].id,
        rig.front.animations.rows[1].id,
    );
    rig.ask(AnimationRequest::Select(slide));
    rig.ask(AnimationRequest::Seek(0.75));
    rig.frame(Vec::new(), NONE);
    assert!((rig.front.animations.time - 0.75).abs() < 1e-6);
    rig.ask(AnimationRequest::Select(short));
    rig.frame(Vec::new(), NONE);
    let view = rig.front.animations.clone();
    assert!(view.previewing && view.selected == Some(1));
    assert!((view.time - 0.25).abs() < 1e-6, "{}", view.time);
    // Back on the long one the playhead stays where the short one left it.
    rig.ask(AnimationRequest::Select(slide));
    rig.frame(Vec::new(), NONE);
    assert!((rig.front.animations.time - 0.25).abs() < 1e-6);
}

#[test]
fn pause_leaves_the_playhead_on_the_nearest_frame() {
    let mut rig = Rig::slide();
    let start = Instant::now();
    rig.ask(AnimationRequest::Play);
    rig.frame_at(start, Vec::new(), NONE);
    for step in 1..=3 {
        rig.frame_at(start + Duration::from_millis(170 * step), Vec::new(), NONE);
    }
    assert!((rig.front.animations.time - 0.51).abs() < 1e-4);
    rig.ask(AnimationRequest::Pause);
    rig.frame_at(start + Duration::from_millis(600), Vec::new(), NONE);
    // 0.51 s is between frames 12 and 13 of 24 per second: it rests on frame 12.
    assert!((rig.front.animations.time - 0.5).abs() < 1e-6);
    assert!(!rig.front.animations.playing);
}

#[test]
fn the_pose_fields_key_the_channel_at_the_frame_of_the_playhead_in_one_undo_step() {
    let mut rig = Rig::slide();
    rig.front.ui.set_mode(Mode::Animate);
    rig.select_bone();
    rig.frame(Vec::new(), NONE);
    rig.ask(AnimationRequest::Seek(0.51));
    rig.frame(Vec::new(), NONE);
    let steps = rig.undo_steps();
    let bone = rig.bone();
    let key = |x: f32, commit: bool| AnimateEdit::Key {
        bone,
        channel: ChannelKind::Rotation,
        value: Vec3::new(x, 0.0, 0.0),
        commit,
    };
    for (x, commit) in [(10.0, false), (20.0, false), (30.0, true)] {
        let edit = key(x, commit);
        rig.edit(edit);
    }
    let keys = rig.keys(Channel::Rotation);
    assert_eq!(keys.len(), 1);
    assert!(
        (keys[0].0 - 0.5).abs() < 1e-6,
        "on the frame: {}",
        keys[0].0
    );
    assert!((keys[0].1.x - 30.0).abs() < 1e-5);
    assert_eq!(rig.undo_steps(), steps + 1);
    assert_eq!(rig.core().history().undo_label(), Some("Set keyframe"));
    rig.state.editor.core.undo();
    assert!(rig.keys(Channel::Rotation).is_empty());
    // The "+" keys the channel with the value it has: the position moved 4 units at 0.5 s.
    let bone = rig.bone();
    rig.edit(AnimateEdit::KeyHere {
        bone,
        channel: ChannelKind::Position,
    });
    let keys = rig.keys(Channel::Position);
    assert_eq!(keys.len(), 3);
    assert!((keys[1].0 - 0.5).abs() < 1e-6 && (keys[1].1.y - 4.0).abs() < 1e-4);
}

#[test]
fn a_key_for_the_selected_bones_sets_every_channel_in_one_undo_step() {
    let mut rig = Rig::slide();
    rig.front.ui.set_mode(Mode::Animate);
    rig.select_bone();
    rig.frame(Vec::new(), NONE);
    rig.ask(AnimationRequest::Seek(0.25));
    rig.frame(Vec::new(), NONE);
    let steps = rig.undo_steps();
    rig.edit(AnimateEdit::KeySelected);
    assert_eq!(rig.undo_steps(), steps + 1);
    for channel in Channel::ALL {
        assert!(
            rig.keys(channel)
                .iter()
                .any(|(t, _)| (t - 0.25).abs() < 1e-6),
            "{channel:?}"
        );
    }
    // The rotation and the scale are at rest; the position is where the animation had it.
    assert!((rig.keys(Channel::Scale)[0].1 - Vec3::ONE).length() < 1e-6);
    assert!(
        rig.keys(Channel::Position)
            .iter()
            .any(|(t, v)| (t - 0.25).abs() < 1e-6 && (v.y - 2.0).abs() < 1e-4)
    );
    rig.state.editor.core.undo();
    assert!(rig.keys(Channel::Rotation).is_empty());
    assert_eq!(rig.keys(Channel::Position).len(), 2);
}

#[test]
fn keyframes_are_copied_pasted_moved_and_deleted_by_their_names() {
    let mut rig = Rig::slide();
    rig.front.ui.set_mode(Mode::Animate);
    rig.frame(Vec::new(), NONE);
    let bone = rig.bone();
    let key = |time: f32| KeyId {
        bone,
        channel: ChannelKind::Position,
        time,
    };
    rig.edit(AnimateEdit::Copy {
        keys: vec![key(0.0), key(1.0)],
    });
    rig.ask(AnimationRequest::Seek(0.5));
    rig.frame(Vec::new(), NONE);
    // The playhead is at 0.5: the copied pair lands at 0.5 and 1.5 and the animation grows.
    rig.edit(AnimateEdit::Paste { bone: None });
    let times: Vec<f32> = rig.keys(Channel::Position).iter().map(|k| k.0).collect();
    assert_eq!(times.len(), 4, "{times:?}");
    assert!((times[1] - 0.5).abs() < 1e-6 && (times[3] - 1.5).abs() < 1e-6);
    assert!((rig.core().doc().animations.items()[0].length - 1.5).abs() < 1e-6);
    rig.state.editor.core.undo();
    assert_eq!(rig.keys(Channel::Position).len(), 2);
    // A move by the name of a keyframe, then the same keyframe deleted at its new time.
    rig.edit(AnimateEdit::Move {
        keys: vec![key(1.0)],
        delta: -0.25,
        commit: true,
    });
    assert!((rig.keys(Channel::Position)[1].0 - 0.75).abs() < 1e-6);
    rig.edit(AnimateEdit::Interp {
        keys: vec![key(0.75)],
        kind: cuboid_ui::KeyKind::Step,
    });
    assert_eq!(
        rig.core().doc().animations.items()[0].tracks[0].position[1].interp,
        cuboid_model::Interp::Step
    );
    rig.edit(AnimateEdit::Delete {
        keys: vec![key(0.75), key(7.0)],
    });
    assert_eq!(
        rig.keys(Channel::Position).len(),
        1,
        "a missing one is skipped"
    );
    // A value written as text stays an expression.
    rig.edit(AnimateEdit::SetValue {
        key: key(0.0),
        value: ["1".into(), "math.sin(q.anim_time)".into(), "0".into()],
    });
    let first = &rig.core().doc().animations.items()[0].tracks[0].position[0];
    assert!(first.value[1].is_expression() && !first.value[0].is_expression());
}

#[test]
fn animations_are_created_duplicated_renamed_changed_and_deleted_from_the_panel() {
    let mut rig = Rig::slide();
    rig.front.ui.set_mode(Mode::Animate);
    rig.frame(Vec::new(), NONE);
    let slide = rig.front.animations.rows[0].id;

    // A new one is named after the others, chosen and shown at 0.
    rig.edit(AnimateEdit::NewAnimation);
    rig.edit(AnimateEdit::NewAnimation);
    rig.frame(Vec::new(), NONE);
    let names: Vec<String> = rig
        .core()
        .doc()
        .animations
        .items()
        .iter()
        .map(|a| a.name.to_string())
        .collect();
    assert_eq!(names, ["slide", "animation", "animation2"]);
    assert_eq!(rig.front.animations.selected, Some(2));
    assert!(rig.front.animations.time.abs() < 1e-6);
    let fresh = &rig.core().doc().animations.items()[2];
    assert!((fresh.length - 1.0).abs() < 1e-6 && fresh.snapping == 24);

    rig.edit(AnimateEdit::DuplicateAnimation { id: slide });
    rig.frame(Vec::new(), NONE);
    let names: Vec<String> = rig
        .core()
        .doc()
        .animations
        .items()
        .iter()
        .map(|a| a.name.to_string())
        .collect();
    assert_eq!(names, ["slide", "slide_copy", "animation", "animation2"]);
    assert_eq!(rig.front.animations.selected, Some(1), "the copy is chosen");

    let copy = rig.front.animations.rows[1].id;
    rig.edit(AnimateEdit::RenameAnimation {
        id: copy,
        name: "slide2".into(),
    });
    rig.edit(AnimateEdit::SetLoop {
        id: copy,
        kind: cuboid_ui::LoopKind::Hold,
    });
    for (length, commit) in [(2.0, false), (3.0, true)] {
        rig.edit(AnimateEdit::SetLength {
            id: copy,
            length,
            commit,
        });
    }
    for (fps, commit) in [(30, false), (60, true)] {
        rig.edit(AnimateEdit::SetFrameRate {
            id: copy,
            fps,
            commit,
        });
    }
    let steps = rig.undo_steps();
    let a = &rig.core().doc().animations.items()[1];
    assert_eq!((&*a.name, a.length, a.snapping), ("slide2", 3.0, 60));
    assert_eq!(a.loop_mode, cuboid_model::LoopMode::Hold);
    // A drag of a field is one undo step: the rate took one and the length one, however many
    // previews each had; both are back after two.
    rig.state.editor.core.undo();
    rig.state.editor.core.undo();
    assert_eq!(rig.undo_steps(), steps - 2);
    let a = &rig.core().doc().animations.items()[1];
    assert_eq!((a.snapping, a.length), (24, 1.0));

    // Deleting the chosen animation chooses another one: Animate always has one.
    rig.edit(AnimateEdit::DeleteAnimation { id: copy });
    rig.frame(Vec::new(), NONE);
    assert_eq!(rig.core().doc().animations.len(), 3);
    assert!(rig.front.animations.selected.is_some());
    rig.ask(AnimationRequest::Select(slide));
    rig.frame(Vec::new(), NONE);
    for id in [slide, rig.front.animations.rows[0].id] {
        rig.edit(AnimateEdit::DeleteAnimation { id });
    }
}

#[test]
fn framing_and_picking_in_animate_use_the_pose_of_the_playhead() {
    let mut rig = Rig::slide();
    rig.front.ui.set_mode(Mode::Animate);
    rig.select_bone();
    rig.ask(AnimationRequest::Select(rig.front.animations.rows[0].id));
    rig.ask(AnimationRequest::Seek(0.5));
    rig.frame(Vec::new(), NONE);
    let cube = rig.core().doc().elements.items()[0].base.clone();
    let rest_y = cube.p.y + cube.s.y * 0.5;
    // F frames the selected bone where the pose has it: 4 units above its rest place.
    rig.front.apply(
        &mut rig.state,
        UiRequest::Viewport(cuboid_ui::ViewportEvent::Frame {
            slot: cuboid_ui::ViewSlot::MAIN,
        }),
    );
    let pivot = rig.front.views.pivot;
    assert!(
        (pivot.y - (rest_y + 4.0)).abs() < 0.6,
        "{pivot:?} rest {rest_y}"
    );
    // The ray at the posed place hits the cube, the ray at its rest place does not.
    let ray_at = |y: f32| cuboid_scene::Ray {
        origin: Vec3::new(cube.p.x + cube.s.x * 0.5, y, 50.0),
        dir: Vec3::NEG_Z,
    };
    let hits_the_cube = |rig: &Rig, y: f32| {
        let hit = rig.front.index.pick_model(ray_at(y), 1000.0);
        hit.is_some_and(|h| matches!(h.target, cuboid_scene::HitTarget::Element(_)))
    };
    assert!(hits_the_cube(&rig, rest_y + 4.0));
    // Near the foot of the rest place the pose has left nothing: it moved 4 units up.
    let foot = cube.p.y + 1.0;
    assert!(!hits_the_cube(&rig, foot));
    // Outside Animate the picking is at rest again.
    rig.front.ui.set_mode(Mode::Model);
    rig.frame(Vec::new(), NONE);
    assert!(hits_the_cube(&rig, foot));
}

#[test]
fn a_preview_outside_animate_leaves_the_picking_at_rest() {
    let mut rig = Rig::slide();
    let cube = rig.core().doc().elements.items()[0].base.clone();
    rig.ask(AnimationRequest::Select(rig.front.animations.rows[0].id));
    rig.ask(AnimationRequest::Seek(0.5));
    rig.frame(Vec::new(), NONE);
    rig.extract();
    assert!(rig.front.animations.previewing);
    let foot = cuboid_scene::Ray {
        origin: Vec3::new(cube.p.x + cube.s.x * 0.5, cube.p.y + 1.0, 50.0),
        dir: Vec3::NEG_Z,
    };
    let hit = rig.front.index.pick_model(foot, 1000.0);
    assert!(hit.is_some_and(|h| matches!(h.target, cuboid_scene::HitTarget::Element(_))));
}

#[test]
fn a_drag_of_the_gizmo_keys_the_posed_bone_pauses_the_player_and_is_one_undo_step() {
    let mut rig = Rig::new(NavPreset::Blockbench);
    rig.state
        .editor
        .core
        .execute(Command::AddElement(cuboid_core::ElementKind::Cube));
    rig.state.editor.core.execute(Command::Group {
        targets: vec![Target::Element(0)],
        name: "bone".into(),
    });
    rig.state.editor.core.execute(Command::AddAnimation {
        name: "slide".into(),
        length: 1.0,
    });
    for (time, y) in [(0.0, 0.0), (1.0, 8.0)] {
        rig.state.editor.core.execute(Command::SetKeyframe {
            animation: 0,
            group: Target::Group(0),
            channel: BoneChannel::Position,
            time,
            value: KeyValue::nums([0.0, y, 0.0]),
            interp: Some(KeyInterp::Linear),
        });
    }
    rig.front.ui.set_mode(Mode::Animate);
    rig.select_bone();
    // Play to a time between two frames; the animation moves the bone up as it goes.
    let start = Instant::now();
    rig.ask(AnimationRequest::Play);
    for step in 0..=3 {
        rig.frame_at(start + Duration::from_millis(170 * step), Vec::new(), NONE);
    }
    assert!(rig.front.animations.playing);
    let rest_p = rig.core().doc().elements.items()[0].base.p;
    let before = rig.undo_steps();
    let gizmo = rig.front.tick.gizmo.clone().expect("the gizmo of the bone");
    let kind = gizmo.kind.expect("a move gizmo");
    assert_eq!(kind, cuboid_scene::GizmoKind::Move);
    // The bone stands 4 units up at 0.5 s and the gizmo with it.
    assert!(
        gizmo.origin.y > rest_p.y + 3.0,
        "posed: {:?} rest {rest_p:?}",
        gizmo.origin
    );

    // Hover the X arrow, press, drag along it, let go.
    let view = &rig.front.views.views[3];
    let aspect = view.rect.aspect();
    let scale = cuboid_scene::gizmo_scale(&view.camera, gizmo.origin) * 0.6;
    let arrow = gizmo.origin + Vec3::X * (0.7 * scale);
    let (ndc, _) = view.camera.project(arrow, aspect).expect("on the screen");
    let placement = rig.front.placements[0];
    let at = |ndc: Vec2| {
        Pos2::new(
            placement.rect[0] + (ndc.x + 1.0) * 0.5 * view.rect.w,
            placement.rect[1] + (1.0 - ndc.y) * 0.5 * view.rect.h,
        )
    };
    // Every frame of the gesture has the clock of the last one: the playhead is where it was.
    let now = start + Duration::from_millis(510);
    let button = |pressed: bool, pos: Pos2| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: NONE,
    };
    rig.pointer = at(ndc);
    rig.frame_at(now, vec![Event::PointerMoved(rig.pointer)], NONE);
    rig.frame_at(now, Vec::new(), NONE);
    assert_ne!(
        rig.core().interaction().active_axis,
        cuboid_scene::Axes::NONE,
        "the pointer is on the arrow"
    );
    rig.frame_at(now, vec![button(true, rig.pointer)], NONE);
    assert!(
        !rig.front.animations.playing,
        "pressing in the views pauses the player"
    );
    for _ in 0..4 {
        rig.pointer += vec2(12.0, 0.0);
        rig.frame_at(now, vec![Event::PointerMoved(rig.pointer)], NONE);
    }
    rig.frame_at(now, vec![button(false, rig.pointer)], NONE);
    rig.frame_at(now, Vec::new(), NONE);

    assert_eq!(rig.undo_steps(), before + 1, "one step for the whole drag");
    assert_eq!(rig.core().history().undo_label(), Some("Key position"));
    let keys = rig.keys(Channel::Position);
    assert_eq!(keys.len(), 3, "a keyframe at the frame of the playhead");
    assert!((keys[1].0 - 0.5).abs() < 1e-6, "{keys:?}");
    assert!(keys[1].1.x.abs() > 0.01, "moved along X: {keys:?}");
    assert!(
        (keys[1].1.y - 4.0).abs() < 1e-3,
        "keyed the value between keys: {keys:?}"
    );
    // The rest pose is untouched; the playhead rests on the frame that was keyed.
    assert_eq!(rig.core().doc().elements.items()[0].base.p, rest_p);
    assert!((rig.front.animations.time - 0.5).abs() < 1e-6);
    rig.state.editor.core.undo();
    assert_eq!(rig.keys(Channel::Position).len(), 2);
}

/// The wheel and a touchpad two-finger scroll zoom in both presets.
fn wheel_cases(rig: &mut Rig) {
    let notch = rig.moved(|r| r.wheel(MouseWheelUnit::Line, 1.0));
    assert_eq!(notch, Moved::Zoom);
    let touchpad = rig.moved(|r| r.wheel(MouseWheelUnit::Point, 20.0));
    assert_eq!(touchpad, Moved::Zoom);
}

#[test]
fn a_left_click_on_an_object_selects_it_in_both_presets() {
    for nav in NavPreset::ALL {
        let mut rig = Rig::new(nav);
        rig.front.apply(
            &mut rig.state,
            UiRequest::Action(ActionId::CreateElementCube),
        );
        rig.state.editor.core.execute(Command::SelectNone);
        rig.frame(Vec::new(), NONE);
        let cube = rig.state.editor.doc().elements.items()[0].base.clone();
        let centre = cube.p + Vec3::new(0.5, 0.5, -0.5) * cube.s;
        let view = &rig.front.views.views[3];
        let (ndc, _) = view.camera.project(centre, view.rect.aspect()).unwrap();
        let placement = rig.front.placements[0];
        rig.pointer = Pos2::new(
            placement.rect[0] + (ndc.x + 1.0) * 0.5 * view.rect.w,
            placement.rect[1] + (1.0 - ndc.y) * 0.5 * view.rect.h,
        );
        rig.frame(vec![Event::PointerMoved(rig.pointer)], NONE);
        rig.frame(Vec::new(), NONE);
        rig.button(PointerButton::Primary, true, NONE);
        rig.button(PointerButton::Primary, false, NONE);
        assert_eq!(rig.state.editor.core.selection().list.len(), 1, "{nav:?}");
    }
}

/// A rig whose window has no system title bar, so the top bar is the title bar.
fn frameless() -> Rig {
    let mut rig = Rig::new(NavPreset::Blockbench);
    rig.front.window.decorated = false;
    rig.frame(Vec::new(), NONE);
    rig
}

impl Rig {
    /// A click with the primary button at `at`.
    fn click_at(&mut self, at: Pos2) {
        self.pointer = at;
        self.frame(vec![Event::PointerMoved(at)], NONE);
        self.button(PointerButton::Primary, true, NONE);
        self.button(PointerButton::Primary, false, NONE);
    }
}

/// The centre of a window button, counted from the right edge of the 1280 point window.
fn window_button(from_right: f32) -> Pos2 {
    Pos2::new(1280.0 - from_right, 16.0)
}

#[test]
fn the_window_buttons_become_window_commands() {
    let mut rig = frameless();
    rig.click_at(window_button(115.0));
    assert_eq!(rig.front.take_window_commands(), [WindowCommand::Minimize]);
    rig.click_at(window_button(69.0));
    assert_eq!(
        rig.front.take_window_commands(),
        [WindowCommand::ToggleMaximize]
    );
    assert!(!rig.state.quit);
}

#[test]
fn a_drag_on_empty_space_of_the_bar_becomes_a_move_command() {
    let mut rig = frameless();
    let start = Pos2::new(720.0, 16.0);
    rig.pointer = start;
    rig.frame(vec![Event::PointerMoved(start)], NONE);
    rig.drag(PointerButton::Primary, NONE, vec2(40.0, 0.0));
    assert_eq!(rig.front.take_window_commands(), [WindowCommand::Move]);
}

#[test]
fn close_quits_a_clean_document_and_asks_before_it_drops_unsaved_changes() {
    let mut clean = frameless();
    clean.click_at(window_button(23.0));
    assert!(clean.state.quit, "nothing is unsaved");

    let mut dirty = frameless();
    let cube = UiRequest::Action(ActionId::CreateElementCube);
    dirty.front.apply(&mut dirty.state, cube);
    dirty.frame(Vec::new(), NONE);
    assert!(dirty.state.editor.is_dirty());
    dirty.click_at(window_button(23.0));
    dirty.frame(Vec::new(), NONE);
    assert!(!dirty.state.quit, "the question comes first");
}

#[test]
fn the_title_bar_choice_becomes_a_decoration_command_when_it_changes() {
    let mut rig = Rig::new(NavPreset::Blockbench);
    let prefs = |system_title_bar| {
        Box::new(Prefs {
            system_title_bar,
            ..Prefs::default()
        })
    };
    // The same choice again asks for nothing.
    rig.front
        .apply(&mut rig.state, UiRequest::PrefsChanged(prefs(false)));
    assert!(rig.front.take_window_commands().is_empty());
    rig.front
        .apply(&mut rig.state, UiRequest::PrefsChanged(prefs(true)));
    assert_eq!(
        rig.front.take_window_commands(),
        [WindowCommand::SetDecorations(true)]
    );
    rig.front
        .apply(&mut rig.state, UiRequest::PrefsChanged(prefs(true)));
    assert!(rig.front.take_window_commands().is_empty());
    rig.front
        .apply(&mut rig.state, UiRequest::PrefsChanged(prefs(false)));
    assert_eq!(
        rig.front.take_window_commands(),
        [WindowCommand::SetDecorations(false)]
    );
}

// --- Playing animated textures (textures T1 4) --------------------------------------------------

use cuboid_ui::{FramesEdit, FramesRequest};

/// The place of the `texture` entry in the list of a new model, after `particle`.
const STRIP: usize = 1;

impl Rig {
    /// A cube on the default texture, which is a strip of three frames of 16 x 16 texels that
    /// last two ticks each.
    fn strip() -> Rig {
        let mut rig = Rig::new(NavPreset::Blockbench);
        let core = &mut rig.state.editor.core;
        core.execute(Command::AddElement(cuboid_core::ElementKind::Cube));
        for _ in 0..2 {
            core.execute(Command::SetTextureAnimation {
                label: "texture".into(),
                animation: Some(cuboid_core::TextureFrames {
                    frame_height: 16,
                    frametime: 2,
                    interpolate: false,
                    frames: Vec::new(),
                }),
            });
            core.execute(Command::AddTextureFrame {
                label: "texture".into(),
                tile: 0,
            });
        }
        rig.frame(Vec::new(), NONE);
        rig.extract();
        rig
    }

    fn frames(&mut self, request: FramesRequest) {
        self.front
            .apply(&mut self.state, UiRequest::Frames(request));
    }

    /// The step of the first texture that the panels show.
    fn shown_step(&self) -> usize {
        let view = self.front.model.textures[STRIP].animation.as_ref();
        view.expect("an animated texture").shown
    }
}

#[test]
fn playing_the_textures_moves_the_tick_at_the_game_rate_and_pausing_stops_it() {
    let mut rig = Rig::strip();
    assert_eq!(
        rig.front.texture_wake(),
        None,
        "a texture at rest asks for no frame"
    );
    assert_eq!((rig.front.textures.tick(), rig.shown_step()), (0, 0));
    let (revision, steps, dirty) = {
        let core = rig.core();
        (
            core.doc().revision,
            core.history().undo_len(),
            core.is_dirty(),
        )
    };

    let start = Instant::now();
    rig.frames(FramesRequest::Play(true));
    rig.frame_at(start, Vec::new(), NONE);
    let wait = rig
        .front
        .texture_wake()
        .expect("playing asks for the next tick");
    assert!(wait <= Duration::from_millis(50), "{wait:?}");
    // 100 ms are two ticks: the second step, which the core and the extractor are told about.
    rig.frame_at(start + Duration::from_millis(100), Vec::new(), NONE);
    assert_eq!(rig.front.textures.tick(), 2);
    assert_eq!(rig.shown_step(), 1);
    assert_eq!(rig.core().texture_tick(), 2);
    let out = rig.extract();
    assert_eq!(out.atlas.len(), 1, "one rectangle, the frame, is sent");
    assert!(out.instances.is_empty());
    assert!(rig.front.texture_wake().is_some());

    rig.frames(FramesRequest::Play(false));
    assert_eq!(rig.front.texture_wake(), None);
    rig.frame_at(start + Duration::from_secs(5), Vec::new(), NONE);
    assert_eq!(rig.front.textures.tick(), 2);
    assert!(rig.extract().atlas.is_empty());

    // Playing never touched the document.
    let core = rig.core();
    assert_eq!(
        (
            core.doc().revision,
            core.history().undo_len(),
            core.is_dirty()
        ),
        (revision, steps, dirty)
    );
}

#[test]
fn showing_a_step_goes_to_its_first_tick_and_stops() {
    let mut rig = Rig::strip();
    rig.frames(FramesRequest::Play(true));
    rig.frames(FramesRequest::Show {
        texture: STRIP,
        step: 2,
    });
    rig.frame(Vec::new(), NONE);
    assert_eq!(
        rig.front.textures.tick(),
        4,
        "two steps of two ticks come before"
    );
    assert_eq!(rig.shown_step(), 2);
    assert!(!rig.front.textures.playing());
    assert_eq!(rig.core().texture_tick(), 4);
    // A step the texture does not have is ignored.
    rig.frames(FramesRequest::Show {
        texture: 9,
        step: 0,
    });
    assert_eq!(rig.front.textures.tick(), 4);
}

#[test]
fn a_model_without_an_animated_texture_does_not_play() {
    let mut rig = Rig::new(NavPreset::Blockbench);
    rig.frames(FramesRequest::Play(true));
    rig.frame(Vec::new(), NONE);
    assert!(!rig.front.textures.playing());
    assert_eq!(rig.front.texture_wake(), None);
}

#[test]
fn the_frames_requests_become_undoable_commands_on_the_frame_shown() {
    let mut rig = Rig::strip();
    let steps = rig.core().history().undo_len();
    let edit = FramesEdit {
        frame_height: 16,
        frametime: 3,
        interpolate: true,
        steps: vec![(2, None), (0, Some(8))],
    };
    rig.frames(FramesRequest::Set {
        texture: STRIP,
        animation: Some(edit),
    });
    assert_eq!(rig.core().history().undo_len(), steps + 1);
    let animation = rig.core().doc().textures.entries[STRIP].animation.clone();
    let animation = animation.expect("animated");
    assert_eq!((animation.frametime, animation.interpolate), (3, true));
    assert_eq!(animation.frames.as_ref().map(Vec::len), Some(2));

    // The frame added is a copy of the one shown: the second step of the order shows the tile 0.
    rig.frames(FramesRequest::Show {
        texture: STRIP,
        step: 1,
    });
    rig.frames(FramesRequest::AddFrame { texture: STRIP });
    let doc = rig.core().doc();
    assert_eq!(doc.textures.entries[STRIP].texture.image.h, 64);
    let animation = doc.textures.entries[STRIP].animation.as_ref().unwrap();
    assert_eq!(animation.frames.as_ref().map(Vec::len), Some(3));
    assert_eq!(rig.core().history().undo_len(), steps + 2);
    rig.frames(FramesRequest::Set {
        texture: STRIP,
        animation: None,
    });
    assert!(rig.core().doc().textures.entries[STRIP].animation.is_none());
}
