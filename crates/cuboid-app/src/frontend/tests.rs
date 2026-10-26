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
    _dir: tempfile::TempDir,
    state: State,
    front: Frontend,
    ctx: egui::Context,
    time: f64,
    pointer: Pos2,
    mods: Modifiers,
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

use cuboid_core::{AnimationLoop, BoneChannel, Core, KeyInterp, Target};
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
                value: [0.0, y, 0.0],
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
        self.front.step_player(&self.state);
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
    assert!(!view.previewing, "choosing an animation shows nothing yet");
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
