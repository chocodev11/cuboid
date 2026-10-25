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
    fn frame(&mut self, mut events: Vec<Event>, modifiers: Modifiers) {
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
        self.front
            .update(&mut self.state, &self.ctx, Instant::now());
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
