//! The top bar as the title bar of a window the system draws no frame for, in headless frames:
//! the window buttons and what they send, the drag area, and what the bar shows in a window with
//! the system title bar, in full screen and while inactive. The input is synthetic input to the
//! harness.

mod common;

use common::Fixture;
use cuboid_ui::{ComingFrom, FontSet, Prefs, Ui, UiRequest, WindowRequest, WindowView};
use cuboid_ui_kit::Tokens;
use egui::{Color32, Event, Modifiers, PointerButton, Pos2, Role, Shape, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// The name the lantern document shows in the bar: it has unsaved changes.
const NAME: &str = "* lantern.cubik";

struct Rig {
    ui: Ui,
    fx: Fixture,
    requests: Vec<UiRequest>,
    window: Vec<WindowRequest>,
}

fn frame(ui: &mut egui::Ui, rig: &mut Rig) {
    let cx = rig.fx.cx();
    let full = ui.ctx().content_rect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(full));
    child.set_clip_rect(full);
    let out = rig.ui.show(&mut child, &cx);
    rig.requests.extend(out.requests);
    rig.window.extend(out.window);
}

fn harness(fx: Fixture, width: f32) -> Harness<'static, Rig> {
    let mut prefs = Prefs::default();
    prefs.apply_coming_from(ComingFrom::Blockbench);
    prefs.accent_chosen = true;
    let rig = Rig {
        ui: Ui::new(prefs, FontSet::default()),
        fx,
        requests: Vec::new(),
        window: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(width, 800.0))
        // Clicks in a row stay within the double-click delay of egui.
        .with_step_dt(0.02)
        .allow_missing_glyphs()
        .build_ui_state(frame, rig);
    harness.run_steps(4);
    harness
}

fn window_requests(harness: &mut Harness<'static, Rig>) -> Vec<WindowRequest> {
    std::mem::take(&mut harness.state_mut().window)
}

fn requests(harness: &mut Harness<'static, Rig>) -> Vec<UiRequest> {
    std::mem::take(&mut harness.state_mut().requests)
}

fn button(harness: &mut Harness<'static, Rig>, at: Pos2, pressed: bool) {
    harness.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

/// Presses at `from`, moves the pointer away in steps and lets go.
fn drag_from(harness: &mut Harness<'static, Rig>, from: Pos2) {
    harness.event(Event::PointerMoved(from));
    harness.run_steps(2);
    button(harness, from, true);
    harness.run_steps(2);
    for step in 1..=3 {
        harness.event(Event::PointerMoved(from + vec2(12.0, 0.0) * step as f32));
        harness.run_steps(2);
    }
    button(harness, from + vec2(36.0, 0.0), false);
    harness.run_steps(2);
}

/// A point on the bar that no widget covers: the middle of the document name.
fn empty_spot(harness: &Harness<'static, Rig>) -> Pos2 {
    harness.get_by_label(NAME).rect().center()
}

/// The colour a line of text was painted in during the last frame.
fn ink_of(harness: &Harness<'static, Rig>, label: &str) -> Option<Color32> {
    fn find(shape: &Shape, label: &str) -> Option<Color32> {
        match shape {
            Shape::Vec(inner) => inner.iter().find_map(|shape| find(shape, label)),
            Shape::Text(text) if text.galley.job.text == label => {
                let baked = text.galley.job.sections[0].format.color;
                Some(if baked == Color32::PLACEHOLDER {
                    text.fallback_color
                } else {
                    baked
                })
            }
            _ => None,
        }
    }
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| find(&clipped.shape, label))
}

#[test]
fn the_window_buttons_are_46_by_32_and_flush_with_the_right_edge() {
    let harness = harness(Fixture::new().frameless(), 1400.0);
    let close = harness.get_by_role_and_label(Role::Button, "Close").rect();
    let maximize = harness
        .get_by_role_and_label(Role::Button, "Maximize")
        .rect();
    let minimize = harness
        .get_by_role_and_label(Role::Button, "Minimize")
        .rect();
    for rect in [close, maximize, minimize] {
        assert_eq!([rect.width(), rect.height()], [46.0, 32.0]);
        assert_eq!(rect.top(), 0.0);
    }
    assert_eq!(close.right(), 1400.0);
    assert_eq!(maximize.right(), close.left());
    assert_eq!(minimize.right(), maximize.left());
}

#[test]
fn minimize_and_maximize_send_their_requests() {
    let mut harness = harness(Fixture::new().frameless(), 1400.0);
    harness
        .get_by_role_and_label(Role::Button, "Minimize")
        .click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::Button, "Maximize")
        .click();
    harness.run_steps(3);
    assert_eq!(
        window_requests(&mut harness),
        [WindowRequest::Minimize, WindowRequest::ToggleMaximize]
    );
    assert!(requests(&mut harness).is_empty());
}

#[test]
fn a_maximized_window_shows_restore_in_place_of_maximize() {
    let mut fx = Fixture::new().frameless();
    fx.window.maximized = true;
    let mut harness = harness(fx, 1400.0);
    assert!(harness.query_by_label("Maximize").is_none());
    harness
        .get_by_role_and_label(Role::Button, "Restore")
        .click();
    harness.run_steps(3);
    assert_eq!(
        window_requests(&mut harness),
        [WindowRequest::ToggleMaximize]
    );
}

#[test]
fn close_runs_the_exit_action_and_asks_about_unsaved_changes() {
    // The start screen has nothing unsaved: Exit becomes Quit at once.
    let mut clean = harness(Fixture::new().frameless().start(), 1400.0);
    clean.get_by_role_and_label(Role::Button, "Close").click();
    clean.run_steps(3);
    assert!(requests(&mut clean).contains(&UiRequest::Quit));

    // The lantern is dirty: the question comes first, and Quit follows the answer.
    let mut dirty = harness(Fixture::new().frameless(), 1400.0);
    dirty.get_by_role_and_label(Role::Button, "Close").click();
    dirty.run_steps(4);
    assert!(!requests(&mut dirty).contains(&UiRequest::Quit));
    dirty.get_by_label("Unsaved changes");
    dirty
        .get_by_role_and_label(Role::Button, "Discard changes")
        .click();
    dirty.run_steps(3);
    assert!(requests(&mut dirty).contains(&UiRequest::Quit));
}

#[test]
fn a_drag_on_empty_space_moves_the_window_once() {
    let mut harness = harness(Fixture::new().frameless(), 1400.0);
    let spot = empty_spot(&harness);
    drag_from(&mut harness, spot);
    assert_eq!(window_requests(&mut harness), [WindowRequest::Move]);
    assert!(requests(&mut harness).is_empty());
}

#[test]
fn a_double_click_on_empty_space_maximizes_or_restores() {
    let mut harness = harness(Fixture::new().frameless(), 1400.0);
    let spot = empty_spot(&harness);
    harness.event(Event::PointerMoved(spot));
    harness.run_steps(2);
    for pressed in [true, false, true, false] {
        button(&mut harness, spot, pressed);
    }
    harness.run_steps(3);
    assert_eq!(
        window_requests(&mut harness),
        [WindowRequest::ToggleMaximize]
    );
}

#[test]
fn a_press_on_a_menu_a_tab_a_button_or_the_search_moves_nothing() {
    let mut harness = harness(Fixture::new().frameless(), 1400.0);
    for label in ["File", "Voxel", "Search commands", "Minimize"] {
        let at = harness.get_by_label(label).rect().center();
        drag_from(&mut harness, at);
        let moved = window_requests(&mut harness);
        assert!(moved.is_empty(), "a drag from {label} asked for {moved:?}");
        // Whatever the drag opened or switched is put away again.
        harness.key_press(egui::Key::Escape);
        harness.run_steps(3);
        requests(&mut harness);
    }
    // The empty space still works afterwards.
    let spot = empty_spot(&harness);
    drag_from(&mut harness, spot);
    assert_eq!(window_requests(&mut harness), [WindowRequest::Move]);
}

#[test]
fn a_window_with_the_system_title_bar_has_no_buttons_and_no_drag_area() {
    let mut harness = harness(Fixture::new(), 1400.0);
    for name in ["Close", "Maximize", "Restore", "Minimize"] {
        assert!(harness.query_by_label(name).is_none(), "{name}");
    }
    // The system draws the name, and the menus stay.
    assert!(harness.query_by_label(NAME).is_none());
    harness.get_by_label("File");
    drag_from(&mut harness, pos2(700.0, 16.0));
    assert!(window_requests(&mut harness).is_empty());
}

#[test]
fn full_screen_hides_the_buttons_and_the_drag_area_but_keeps_the_menus() {
    let mut fx = Fixture::new().frameless();
    fx.window = WindowView {
        fullscreen: true,
        decorated: false,
        ..WindowView::default()
    };
    let mut harness = harness(fx, 1400.0);
    for name in ["Close", "Maximize", "Minimize"] {
        assert!(harness.query_by_label(name).is_none(), "{name}");
    }
    harness.get_by_label("File");
    harness.get_by_label("Voxel");
    drag_from(&mut harness, pos2(700.0, 16.0));
    assert!(window_requests(&mut harness).is_empty());
}

#[test]
fn an_inactive_window_shows_the_brand_and_the_name_in_the_weak_colour() {
    let active = harness(Fixture::new().frameless(), 1400.0);
    let tokens = Tokens::of(&active.ctx);
    assert_eq!(ink_of(&active, "cuboid"), Some(tokens.text_strong));
    assert_eq!(ink_of(&active, NAME), Some(tokens.text));

    let mut fx = Fixture::new().frameless();
    fx.window.focused = false;
    let inactive = harness(fx, 1400.0);
    assert_eq!(ink_of(&inactive, "cuboid"), Some(tokens.text_weak));
    assert_eq!(ink_of(&inactive, NAME), Some(tokens.text_weak));
}

#[test]
fn the_name_is_centred_in_the_free_space_and_a_narrow_bar_drops_it() {
    let wide = harness(Fixture::new().frameless(), 1920.0);
    let name = wide.get_by_label(NAME).rect();
    let tab = wide.get_by_role_and_label(Role::Tab, "Animate").rect();
    let preset = wide.get_by_role_and_label(Role::Button, "Cubik").rect();
    assert!(name.left() > tab.right() && name.right() < preset.left());
    let free = (tab.right() + preset.left()) / 2.0;
    assert!((name.center().x - free).abs() < 12.0, "{name:?} in {free}");

    let narrow = harness(Fixture::new().frameless(), 1100.0);
    assert!(narrow.query_by_label(NAME).is_none());
}

#[test]
fn a_tight_bar_shrinks_the_preset_to_its_icon_and_then_drops_it() {
    // (window width, the preset as a button with its label, as an icon, or gone)
    let steps = [(1400.0, "label"), (940.0, "icon"), (900.0, "none")];
    for (width, preset) in steps {
        let harness = harness(Fixture::new().frameless(), width);
        let button = |role, name| harness.query_by_role_and_label(role, name);
        let label = button(Role::Button, "Cubik");
        let icon = button(Role::Button, "Project settings");
        assert_eq!(
            [label.is_some(), icon.is_some()],
            [preset == "label", preset == "icon"],
            "at {width}"
        );
        // Whatever is shown stands apart from its neighbours and inside the window.
        let rect = |name| harness.get_by_role_and_label(Role::Button, name).rect();
        let mut order = vec![harness.get_by_role_and_label(Role::Tab, "Animate").rect()];
        order.extend(label.or(icon).map(|node| node.rect()));
        order.extend(["Search commands", "Minimize", "Maximize", "Close"].map(rect));
        for pair in order.windows(2) {
            assert!(
                pair[0].right() <= pair[1].left() + 0.5,
                "at {width}: {pair:?}"
            );
        }
        assert_eq!(order.last().map(|close| close.right()), Some(width));
    }
}
