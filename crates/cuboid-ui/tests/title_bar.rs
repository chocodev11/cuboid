//! The top bar as the title bar of a window the system draws no frame for, in headless frames:
//! the window buttons and what they send, the drag area, and what the bar shows in a window with
//! the system title bar, in full screen and while inactive. The input is synthetic input to the
//! harness.

mod common;

use common::Fixture;
use cuboid_ui::{ComingFrom, FontSet, Prefs, Ui, UiRequest, WindowRequest, WindowView};
use cuboid_ui_kit::Tokens;
use egui::{
    Color32, Event, Modifiers, PointerButton, Pos2, Role, Shape, TextureId, TextureOptions, pos2,
    vec2,
};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// The name the lantern document shows in the bar: it has unsaved changes.
const NAME: &str = "* lantern.cubik";

struct Rig {
    ui: Ui,
    fx: Fixture,
    requests: Vec<UiRequest>,
    window: Vec<WindowRequest>,
    /// The textures the frames sent to the renderer, once for each frame that sent them. The
    /// harness empties the output's own list.
    uploads: Vec<TextureId>,
}

fn frame(ui: &mut egui::Ui, rig: &mut Rig) {
    let cx = rig.fx.cx();
    let full = ui.ctx().content_rect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(full));
    child.set_clip_rect(full);
    let out = rig.ui.show(&mut child, &cx);
    rig.requests.extend(out.requests);
    rig.window.extend(out.window);
    let mut delta = ui.ctx().tex_manager().write().take_delta();
    rig.uploads.extend(delta.set.keys().copied());
    delta.clear();
}

/// A harness of `width` points at `scale` pixels per point that has not run a frame yet.
fn unrun(fx: Fixture, width: f32, scale: f32) -> Harness<'static, Rig> {
    let mut prefs = Prefs::default();
    prefs.apply_coming_from(ComingFrom::Blockbench);
    prefs.accent_chosen = true;
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.set_logo(common::logo());
    let rig = Rig {
        ui,
        fx,
        requests: Vec::new(),
        window: Vec::new(),
        uploads: Vec::new(),
    };
    Harness::builder()
        .with_size(vec2(width, 800.0))
        .with_pixels_per_point(scale)
        // Clicks in a row stay within the double-click delay of egui.
        .with_step_dt(0.02)
        .allow_missing_glyphs()
        .build_ui_state(frame, rig)
}

fn harness(fx: Fixture, width: f32) -> Harness<'static, Rig> {
    let mut harness = unrun(fx, width, 1.0);
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
    // (window width, the preset as a button with its label, as an icon, or gone). The word of
    // the brand goes below 906 and gives its room to the icon.
    let steps = [
        (1400.0, "label"),
        (940.0, "icon"),
        (900.0, "icon"),
        (860.0, "none"),
    ];
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

/// The logo textures egui holds: the id, the name and the size in pixels.
fn logo_textures(ctx: &egui::Context) -> Vec<(TextureId, String, [usize; 2])> {
    let manager = ctx.tex_manager();
    let manager = manager.read();
    manager
        .allocated()
        .filter(|(_, meta)| meta.name.starts_with("cuboid-ui/logo:"))
        .map(|(id, meta)| (*id, meta.name.clone(), meta.size))
        .collect()
}

/// The alpha of the tint of the picture of texture `id` in the last frame.
fn picture_alpha(harness: &Harness<'static, Rig>, id: TextureId) -> Option<u8> {
    fn find(shape: &Shape, id: TextureId) -> Option<u8> {
        match shape {
            Shape::Vec(inner) => inner.iter().find_map(|shape| find(shape, id)),
            Shape::Rect(rect) if rect.fill_texture_id() == id => Some(rect.fill.a()),
            _ => None,
        }
    }
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| find(&clipped.shape, id))
}

#[test]
fn the_brand_is_the_logo_at_20_points_and_the_word_4_points_after_it() {
    let harness = harness(Fixture::new().frameless(), 1400.0);
    let logo = harness.get_by_label("cuboid logo").rect();
    let word = harness.get_by_label("cuboid").rect();
    assert_eq!([logo.width(), logo.height()], [20.0, 20.0]);
    assert_eq!(logo.left(), 8.0, "the edge gutter");
    assert_eq!(logo.center().y, 16.0, "centred in the 32 point bar");
    assert_eq!(word.left() - logo.right(), 4.0, "space XS between them");
    // The menus follow with their own gap.
    let file = harness.get_by_label("File").rect();
    assert!(file.left() > word.right() + 8.0, "{word:?} then {file:?}");
}

#[test]
fn the_logo_is_the_raster_for_its_size_and_is_uploaded_with_linear_filtering() {
    // (pixels per point, the raster for 20 points: the smallest with at least that many pixels)
    for (scale, raster) in [(1.0, 20), (1.25, 32), (1.5, 32), (2.0, 40)] {
        let mut harness = unrun(Fixture::new().frameless(), 1400.0, scale);
        harness.run_steps(3);
        let textures = logo_textures(&harness.ctx);
        let [(id, name, size)] = textures.as_slice() else {
            panic!("one logo texture at {scale}, found {textures:?}");
        };
        assert_eq!(name, &format!("cuboid-ui/logo:{raster}"), "at {scale}");
        assert_eq!(*size, [raster, raster]);
        let options = harness
            .ctx
            .tex_manager()
            .read()
            .meta(*id)
            .map(|m| m.options);
        assert_eq!(options, Some(TextureOptions::LINEAR), "at {scale}");
    }
}

#[test]
fn the_logo_is_sent_to_the_renderer_once_and_keeps_its_texture() {
    let mut harness = unrun(Fixture::new().frameless(), 1400.0, 1.0);
    let textures = logo_textures(&harness.ctx);
    let [(id, _, _)] = textures.as_slice() else {
        panic!("one logo texture after the first frames, found {textures:?}");
    };
    let sent = |harness: &Harness<'static, Rig>| {
        let uploads = &harness.state().uploads;
        uploads.iter().filter(|upload| *upload == id).count()
    };
    assert_eq!(sent(&harness), 1, "the first frames send it once");
    harness.run_steps(8);
    assert_eq!(sent(&harness), 1, "later frames do not send it again");
    assert_eq!(logo_textures(&harness.ctx)[0].0, *id);
}

#[test]
fn an_inactive_window_draws_the_logo_at_60_percent() {
    let active = harness(Fixture::new().frameless(), 1400.0);
    let (id, _, _) = logo_textures(&active.ctx)[0].clone();
    assert_eq!(picture_alpha(&active, id), Some(255));

    let mut fx = Fixture::new().frameless();
    fx.window.focused = false;
    let inactive = harness(fx, 1400.0);
    let (id, _, _) = logo_textures(&inactive.ctx)[0].clone();
    // 60 % of 255, up to the rounding of the gamma multiplication.
    let alpha = picture_alpha(&inactive, id).unwrap();
    assert!((152..=154).contains(&alpha), "{alpha}");
}

#[test]
fn the_logo_is_no_button_a_drag_on_it_moves_the_window_and_a_double_click_maximizes() {
    let mut harness = harness(Fixture::new().frameless(), 1400.0);
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "cuboid logo")
            .is_none()
    );
    let spot = harness.get_by_label("cuboid logo").rect().center();
    drag_from(&mut harness, spot);
    assert_eq!(window_requests(&mut harness), [WindowRequest::Move]);
    assert!(requests(&mut harness).is_empty());

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
fn a_narrow_window_drops_the_word_after_the_name_and_keeps_the_logo() {
    // (window width, the name, the word): the name goes first, the word follows.
    let steps = [
        (1400.0, true, true),
        (1100.0, false, true),
        (920.0, false, true),
        (900.0, false, false),
        (800.0, false, false),
    ];
    for (width, name, word) in steps {
        let harness = harness(Fixture::new().frameless(), width);
        assert_eq!(
            harness.query_by_label(NAME).is_some(),
            name,
            "name at {width}"
        );
        assert_eq!(
            harness.query_by_label("cuboid").is_some(),
            word,
            "word at {width}"
        );
        let logo = harness.get_by_label("cuboid logo").rect();
        assert_eq!(logo.left(), 8.0, "the logo stays at {width}");
        // The bar keeps the logo apart from the menus either way.
        let file = harness.get_by_label("File").rect();
        assert!(logo.right() < file.left(), "at {width}: {logo:?} {file:?}");
    }
}

#[test]
fn the_word_never_meets_the_search_while_it_shows() {
    for width in [906.0, 920.0, 1000.0, 1100.0, 1400.0] {
        let harness = harness(Fixture::new().frameless(), width);
        let tab = harness.get_by_role_and_label(Role::Tab, "Animate").rect();
        let search = harness
            .get_by_role_and_label(Role::Button, "Search commands")
            .rect();
        assert!(
            tab.right() + 4.0 <= search.left(),
            "at {width}: {tab:?} then {search:?}"
        );
    }
}
