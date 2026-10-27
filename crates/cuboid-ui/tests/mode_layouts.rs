//! The layout per mode in headless frames of the whole window: the panels and the Properties tabs
//! each mode shows, the selection that shows the transform, the one New animation button and the
//! tab rows that fit (`docs/design/mode_layouts.md`). The input is synthetic input to the harness.

mod common;

use common::Fixture;
use cuboid_ui::{ComingFrom, FontSet, Mode, NodeId, PanelId, Prefs, Ui, ViewportPlacement, Zone};
use egui::accesskit::Toggled;
use egui::{Key, Modifiers, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

struct Rig {
    ui: Ui,
    fx: Fixture,
    view: Option<ViewportPlacement>,
}

fn frame(ui: &mut egui::Ui, rig: &mut Rig) {
    let cx = rig.fx.cx();
    let full = ui.ctx().content_rect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(full));
    child.set_clip_rect(full);
    let out = rig.ui.show(&mut child, &cx);
    rig.view = out.viewports.first().copied();
}

fn prefs(from: ComingFrom) -> Prefs {
    let mut prefs = Prefs::default();
    prefs.apply_coming_from(from);
    prefs.accent_chosen = true;
    prefs
}

/// Missing glyphs are not allowed: egui draws the warning of an id clash with a glyph the fonts
/// lack, so two panels that share an id fail the test that draws them.
fn harness_with(prefs: Prefs, mode: Mode, fx: Fixture) -> Harness<'static, Rig> {
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.set_mode(mode);
    let rig = Rig { ui, fx, view: None };
    let mut harness = Harness::builder()
        .with_size(vec2(1600.0, 900.0))
        .build_ui_state(frame, rig);
    harness.run_steps(4);
    harness
}

/// A window of the Blockbench arrangement that starts in `mode`.
fn harness_in(mode: Mode, fx: Fixture) -> Harness<'static, Rig> {
    harness_with(prefs(ComingFrom::Blockbench), mode, fx)
}

fn count(harness: &Harness<'static, Rig>, role: Role, label: &str) -> usize {
    harness.query_all_by_role_and_label(role, label).count()
}

/// The left, top, right and bottom edge of the view the window drew.
fn view(harness: &Harness<'static, Rig>) -> [f32; 4] {
    let [x, y, w, h] = harness.state().view.expect("a view is drawn").rect;
    [x, y, x + w, y + h]
}

fn selected(harness: &Harness<'static, Rig>, tip: &str) -> bool {
    let tab = harness.get_by_role_and_label(Role::Tab, tip);
    tab.accesskit_node().toggled() == Some(Toggled::True)
}

fn select(harness: &mut Harness<'static, Rig>, ids: &[u64]) {
    harness.state_mut().fx.selection = ids.iter().copied().map(NodeId).collect();
    harness.run_steps(3);
}

fn click_tab(harness: &mut Harness<'static, Rig>, tip: &str) {
    harness.get_by_role_and_label(Role::Tab, tip).click();
    harness.run_steps(3);
}

const PROPS_TABS: [&str; 7] = [
    "Element",
    "Faces and UV",
    "Texture",
    "Model properties",
    "Display properties",
    "Animate properties",
    "Validator",
];

/// The tabs of the Properties strip on screen, in the order they are drawn.
fn strip(harness: &Harness<'static, Rig>) -> Vec<String> {
    harness
        .get_all_by_role(Role::Tab)
        .filter_map(|tab| tab.accesskit_node().label())
        .filter(|label| PROPS_TABS.contains(&label.as_str()))
        .collect()
}

#[test]
fn animate_shows_the_animations_panel_the_properties_and_the_outliner_but_no_uv_editor() {
    let harness = harness_in(Mode::Animate, Fixture::new().animated());
    let [view_left, view_top, view_right, view_bottom] = view(&harness);
    // The Animations panel stands alone in the left area, at the top of it.
    let animations = harness
        .get_all_by_role_and_label(Role::Button, "Animations")
        .map(|node| node.rect())
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the Animations panel");
    assert!(animations.right() <= view_left, "{animations:?}");
    let pose = harness.get_by_role_and_label(Role::Tab, "Animate properties");
    let outliner = harness
        .get_by_role_and_label(Role::Button, "Outliner")
        .rect();
    // The Properties are above the Outliner in the right area, with the strip facing the views.
    assert!(pose.rect().left() >= view_right, "{:?}", pose.rect());
    assert!(outliner.left() >= view_right, "{outliner:?}");
    assert!(pose.rect().top() < outliner.top());
    let properties = harness
        .get_by_role_and_label(Role::Button, "Animate")
        .rect();
    assert!((animations.top() - properties.top()).abs() < 1.0);
    // The timeline is below the views.
    let timeline = harness
        .get_by_label("Fit the animation to the width")
        .rect();
    assert!(timeline.top() >= view_bottom - 1.0 && view_top < view_bottom);
    for gone in ["UV editor", "Library", "Textures", "Palette", "History"] {
        assert_eq!(count(&harness, Role::Tab, gone), 0, "{gone}");
    }
    assert_eq!(count(&harness, Role::Button, "Textures"), 0);
    assert_eq!(harness.query_all_by_label("UV view").count(), 0);
}

#[test]
fn display_shows_the_properties_alone_and_the_views_take_the_left_area() {
    let display = harness_in(Mode::Display, Fixture::new());
    display.get_by_role_and_label(Role::Tab, "Display properties");
    for gone in ["UV editor", "Library", "Textures", "Palette", "History"] {
        assert_eq!(count(&display, Role::Tab, gone), 0, "{gone}");
    }
    for gone in ["Outliner", "Animations"] {
        assert_eq!(count(&display, Role::Button, gone), 0, "{gone}");
    }
    assert_eq!(display.query_all_by_label("UV view").count(), 0);
    // The left area has nothing to show: the views start at the tool strip, and the areas stay
    // where the user left them for the next mode.
    let model = harness_in(Mode::Model, Fixture::new());
    let [display_left, ..] = view(&display);
    let [model_left, ..] = view(&model);
    assert!(display_left < 40.0, "{display_left}");
    assert!(model_left > 200.0, "{model_left}");
    let left = display.state().ui.prefs().layout.zone(Zone::Left);
    assert!(left.open && left.size >= 180.0);
}

#[test]
fn model_shows_no_animations_panel_and_no_animate_tab() {
    let harness = harness_in(Mode::Model, Fixture::new().animated());
    assert_eq!(count(&harness, Role::Tab, "Animations"), 0);
    assert_eq!(count(&harness, Role::Button, "Animations"), 0);
    assert_eq!(count(&harness, Role::Tab, "Animate properties"), 0);
    assert_eq!(count(&harness, Role::Tab, "Palette"), 0);
    for shown in ["UV editor", "Library", "Textures", "History"] {
        harness.get_by_role_and_label(Role::Tab, shown);
    }
}

#[test]
fn the_strip_of_each_mode_lists_exactly_the_tabs_of_its_table() {
    let table: [(Mode, &[&str]); 5] = [
        (
            Mode::Model,
            &[
                "Element",
                "Faces and UV",
                "Texture",
                "Model properties",
                "Validator",
            ],
        ),
        (Mode::Voxel, &["Element", "Model properties", "Validator"]),
        (Mode::Paint, &["Texture", "Validator"]),
        (Mode::Display, &["Display properties", "Validator"]),
        (Mode::Animate, &["Animate properties", "Validator"]),
    ];
    for (mode, tabs) in table {
        let harness = harness_in(mode, Fixture::new().animated());
        assert_eq!(strip(&harness), tabs, "{mode:?}");
    }
}

#[test]
fn the_strip_and_the_tab_on_entering_a_mode_follow_the_mode_tabs() {
    let mut harness = harness_in(Mode::Model, Fixture::new());
    for (mode, entering) in [
        (Mode::Paint, "Texture"),
        (Mode::Display, "Display properties"),
        (Mode::Animate, "Animate properties"),
        (Mode::Voxel, "Model properties"),
        (Mode::Model, "Element"),
    ] {
        harness
            .get_by_role_and_label(Role::Tab, mode.name())
            .click();
        harness.run_steps(4);
        assert_eq!(harness.state().ui.mode(), mode);
        assert!(selected(&harness, entering), "{mode:?}");
        assert!(strip(&harness).contains(&entering.to_owned()));
    }
}

#[test]
fn the_panels_menu_turns_down_the_panels_of_other_modes() {
    let mut harness = harness_in(Mode::Model, Fixture::new());
    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness.get_by_label_contains("Panels").click();
    harness.run_steps(3);
    let serving = |mode: Mode| PanelId::ALL.map(|panel| (panel, panel.serves(mode)));
    for (panel, serves) in serving(Mode::Model) {
        let item = harness.get_by_role_and_label(Role::MenuItem, panel.title());
        assert_eq!(item.accesskit_node().is_disabled(), !serves, "{panel:?}");
        // A panel of the mode shows its state; one of another mode is never ticked.
        if !serves {
            assert_ne!(item.accesskit_node().toggled(), Some(Toggled::True));
        }
    }
    harness
        .get_by_role_and_label(Role::MenuItem, "Animations")
        .hover();
    harness.run_steps(8);
    harness.get_by_label("Not used in Model");
}

#[test]
fn the_command_palette_lists_a_panel_of_another_mode_disabled_with_the_tip() {
    let mut harness = harness_in(Mode::Model, Fixture::new());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
    harness.run_steps(3);
    harness
        .get_by_label("Type a command")
        .type_text("Toggle Animations panel");
    harness.run_steps(3);
    let row = harness.get_by_role_and_label(Role::ListItem, "Toggle Animations panel");
    assert!(row.accesskit_node().is_disabled());
    row.hover();
    harness.run_steps(8);
    harness.get_by_label("Not used in Model");
    // Enter on the disabled row runs nothing and leaves the palette open.
    let before = harness.state().ui.prefs().layout.clone();
    harness.key_press(Key::Enter);
    harness.run_steps(3);
    assert_eq!(harness.state().ui.prefs().layout, before);
    harness.get_by_label("Type a command");
    // In the mode that has the panel the row is enabled.
    let mut animate = harness_in(Mode::Animate, Fixture::new());
    animate.key_press_modifiers(Modifiers::COMMAND, Key::P);
    animate.run_steps(3);
    animate
        .get_by_label("Type a command")
        .type_text("Toggle Animations panel");
    animate.run_steps(3);
    let row = animate.get_by_role_and_label(Role::ListItem, "Toggle Animations panel");
    assert!(!row.accesskit_node().is_disabled());
}

/// Opens the command palette of a window and types `query`.
fn search_commands(harness: &mut Harness<'static, Rig>, query: &str) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
    harness.run_steps(3);
    harness.get_by_label("Type a command").type_text(query);
    harness.run_steps(3);
}

#[test]
fn the_show_actions_of_a_panel_that_another_mode_uses_are_disabled_with_the_tip() {
    // Model has no palette; Display has neither the outliner nor the textures.
    let turned_down = [
        (Mode::Model, "Show colour tools"),
        (Mode::Model, "Show colour palette"),
        (Mode::Display, "Show outliner"),
        (Mode::Display, "Show textures"),
    ];
    for (mode, label) in turned_down {
        let mut harness = harness_in(mode, Fixture::new());
        search_commands(&mut harness, label);
        let row = harness.get_by_role_and_label(Role::ListItem, label);
        assert!(row.accesskit_node().is_disabled(), "{mode:?} {label}");
        row.hover();
        harness.run_steps(8);
        harness.get_by_label(&format!("Not used in {}", mode.name()));
        // Enter on the disabled row runs nothing and leaves the palette open.
        let before = harness.state().ui.prefs().layout.clone();
        harness.key_press(Key::Enter);
        harness.run_steps(3);
        assert_eq!(
            harness.state().ui.prefs().layout,
            before,
            "{mode:?} {label}"
        );
        harness.get_by_label("Type a command");
    }
    // The modes that use the panel list the rows enabled.
    let offered = [
        (Mode::Voxel, "Show colour palette"),
        (Mode::Paint, "Show colour tools"),
        (Mode::Paint, "Show textures"),
        (Mode::Animate, "Show outliner"),
    ];
    for (mode, label) in offered {
        let mut harness = harness_in(mode, Fixture::new());
        search_commands(&mut harness, label);
        let row = harness.get_by_role_and_label(Role::ListItem, label);
        assert!(!row.accesskit_node().is_disabled(), "{mode:?} {label}");
    }
}

#[test]
fn the_toggle_keys_of_panels_that_another_mode_uses_change_nothing() {
    let layout = |harness: &Harness<'static, Rig>| harness.state().ui.prefs().layout.clone();
    let mut harness = harness_in(Mode::Display, Fixture::new());
    let before = layout(&harness);
    for key in [Key::F8, Key::F9, Key::F10] {
        harness.key_press(key);
        harness.run_steps(3);
        assert_eq!(layout(&harness), before, "{key:?}");
    }
    // The same keys work in a mode that has the panels.
    let mut harness = harness_in(Mode::Model, Fixture::new());
    let before = layout(&harness);
    harness.key_press(Key::F10);
    harness.run_steps(3);
    assert_ne!(layout(&harness), before);
}

#[test]
fn a_new_primary_selection_shows_the_element_tab_where_the_tab_was_texture_or_model() {
    let mut harness = harness_in(Mode::Model, Fixture::new());
    select(&mut harness, &[]);
    click_tab(&mut harness, "Texture");
    select(&mut harness, &[6]);
    assert!(selected(&harness, "Element"));
    click_tab(&mut harness, "Model properties");
    select(&mut harness, &[3]);
    assert!(selected(&harness, "Element"));
    // Faces and UV and the Validator stay: an issue clicked in the list selects its object and
    // keeps the list.
    for tip in ["Faces and UV", "Validator"] {
        click_tab(&mut harness, tip);
        select(&mut harness, &[6]);
        assert!(selected(&harness, tip), "{tip}");
        select(&mut harness, &[3]);
        assert!(selected(&harness, tip), "{tip}");
    }
    // Clearing the selection changes nothing; choosing the object again is a new primary.
    click_tab(&mut harness, "Texture");
    select(&mut harness, &[]);
    assert!(selected(&harness, "Texture"));
    select(&mut harness, &[3]);
    assert!(selected(&harness, "Element"));
    // The same primary in the next frames, even with more objects around it, is not a new one.
    click_tab(&mut harness, "Texture");
    select(&mut harness, &[3, 6]);
    assert!(selected(&harness, "Texture"));
}

#[test]
fn voxel_follows_the_selection_and_paint_keeps_the_texture() {
    let mut voxel = harness_in(Mode::Voxel, Fixture::new());
    select(&mut voxel, &[]);
    click_tab(&mut voxel, "Model properties");
    select(&mut voxel, &[6]);
    assert!(selected(&voxel, "Element"));
    let mut paint = harness_in(Mode::Paint, Fixture::new());
    select(&mut paint, &[]);
    select(&mut paint, &[6]);
    assert!(selected(&paint, "Texture"));
}

#[test]
fn the_validator_is_in_every_mode_and_show_validator_opens_it() {
    for mode in Mode::ALL {
        let mut harness = harness_in(mode, Fixture::new());
        assert!(!harness.state().ui.validator_visible(), "{mode:?}");
        harness.state_mut().ui.show_validator();
        harness.run_steps(3);
        assert!(selected(&harness, "Validator"), "{mode:?}");
        assert!(harness.state().ui.validator_visible(), "{mode:?}");
    }
}

#[test]
fn animate_without_an_animation_has_exactly_one_new_animation_button() {
    let button = |harness: &Harness<'static, Rig>| count(harness, Role::Button, "New animation");
    // No animation data at all.
    let harness = harness_in(Mode::Animate, Fixture::new());
    assert_eq!(button(&harness), 1);
    harness.get_by_label("No animations");
    // A model with the views but no animation: the line of the Animate tab and the line of the
    // timeline say where to add one.
    let mut fx = Fixture::new().animated();
    let animations = fx.animations.as_mut().expect("animations");
    animations.rows = std::sync::Arc::from(Vec::new());
    animations.selected = None;
    let harness = harness_in(Mode::Animate, fx);
    assert_eq!(button(&harness), 1);
    let line = "No animation. Add one in the Animations panel.";
    assert_eq!(harness.query_all_by_label(line).count(), 2);
    // Animations but none chosen: the Animations panel lists them (its plus button is the one
    // New animation button) and the lines point to it.
    let mut fx = Fixture::new().animated();
    fx.animations.as_mut().expect("animations").selected = None;
    let harness = harness_in(Mode::Animate, fx);
    assert_eq!(button(&harness), 1);
    let line = "Choose an animation in the Animations panel.";
    assert_eq!(harness.query_all_by_label(line).count(), 2);
}

#[test]
fn a_click_on_a_tab_stores_the_clicked_panel_whatever_the_mode_leaves_out() {
    let mut harness = harness_in(Mode::Model, Fixture::new());
    click_tab(&mut harness, "Library");
    click_tab(&mut harness, "History");
    // History is the third panel of its group, behind the palette that Model leaves out.
    let left = harness.state().ui.prefs().layout.zone(Zone::Left).clone();
    assert_eq!(left.groups[0].active, 1);
    assert_eq!(left.groups[1].active, 2);
    assert_eq!(left.groups[1].current(Mode::Model), Some(PanelId::History));
}

#[test]
fn a_mode_that_shows_a_panel_anyway_leaves_the_tab_in_front_of_its_group() {
    let mut harness = harness_in(Mode::Model, Fixture::new().animated());
    click_tab(&mut harness, "History");
    // Animate shows Animations in that group whatever tab is stored, so its rule changes nothing.
    for mode in [Mode::Animate, Mode::Model] {
        harness
            .get_by_role_and_label(Role::Tab, mode.name())
            .click();
        harness.run_steps(4);
    }
    assert!(selected(&harness, "History"));
    let left = harness.state().ui.prefs().layout.zone(Zone::Left).clone();
    assert_eq!(left.groups[1].active, 2);
}

#[test]
fn drawing_in_every_mode_leaves_the_stored_layout_unchanged() {
    for mode in Mode::ALL {
        let mut prefs = prefs(ComingFrom::Blockbench);
        prefs.mode = mode;
        let before = prefs.to_json();
        let ui = Ui::new(prefs, FontSet::default());
        let rig = Rig {
            ui,
            fx: Fixture::new().animated(),
            view: None,
        };
        let mut harness = Harness::builder()
            .with_size(vec2(1600.0, 900.0))
            .build_ui_state(frame, rig);
        harness.run_steps(6);
        assert_eq!(harness.state().ui.mode(), mode);
        assert_eq!(harness.state().ui.prefs().to_json(), before, "{mode:?}");
    }
}

#[test]
fn the_panels_of_every_mode_share_no_id_in_any_arrangement() {
    for from in [
        ComingFrom::Blockbench,
        ComingFrom::Blender,
        ComingFrom::Cubik,
    ] {
        for mode in Mode::ALL {
            let fx = if mode == Mode::Animate {
                Fixture::new().animated()
            } else {
                Fixture::new()
            };
            let mut harness = harness_with(prefs(from), mode, fx);
            harness.run_steps(2);
        }
    }
}

#[test]
fn going_through_the_modes_moves_no_panel_and_changes_no_size() {
    let mut harness = harness_in(Mode::Model, Fixture::new().animated());
    let before = harness.state().ui.prefs().layout.clone();
    for mode in [
        Mode::Animate,
        Mode::Display,
        Mode::Paint,
        Mode::Voxel,
        Mode::Model,
    ] {
        harness
            .get_by_role_and_label(Role::Tab, mode.name())
            .click();
        harness.run_steps(4);
    }
    // Only the Paint rule moves a tab, the palette in the lower left group; every other tab in
    // front, like the sizes and the splits, is where it was.
    let mut after = harness.state().ui.prefs().layout.clone();
    assert_eq!(after.zones[0].groups[1].active, 1);
    after.zones[0].groups[1].active = before.zones[0].groups[1].active;
    assert_eq!(after, before);
}

/// The move button at the right end of the header that stands lowest in the left area.
fn lowest_move_button(harness: &Harness<'static, Rig>) -> egui::Rect {
    harness
        .get_all_by_role_and_label(Role::Button, "Move to the right area")
        .map(|node| node.rect())
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("a move button")
}

/// The four panels of the lower left group of the Cubik arrangement that Paint uses.
const PAINT_TABS: [&str; 4] = ["UV editor", "Textures", "Palette", "History"];

/// A Paint window of the Cubik arrangement whose left area is `width` wide, with `shown` in front
/// in the lower group of the left area.
fn paint_row(width: usize, shown: &str) -> Harness<'static, Rig> {
    let mut prefs = prefs(ComingFrom::Cubik);
    prefs.layout.zones[0].size = width as f32;
    let mut harness = harness_with(prefs, Mode::Paint, Fixture::new());
    click_tab(&mut harness, shown);
    harness
}

fn tab_rect(harness: &Harness<'static, Rig>, title: &str) -> egui::Rect {
    harness.get_by_role_and_label(Role::Tab, title).rect()
}

#[test]
fn a_tab_row_from_200_to_340_points_wide_never_reaches_the_move_button() {
    for width in (200..=340).step_by(20) {
        for shown in PAINT_TABS {
            let harness = paint_row(width, shown);
            let button = lowest_move_button(&harness);
            for title in PAINT_TABS {
                let tab = tab_rect(&harness, title);
                assert!(
                    tab.right() <= button.left() + 0.5,
                    "{width} {shown} {title}: {tab:?} reaches {button:?}"
                );
            }
        }
    }
}

#[test]
fn the_tabs_give_up_their_titles_one_step_at_a_time_and_a_cut_title_keeps_its_name() {
    // Wide enough for every title: all of them are words.
    let wide = paint_row(400, "UV editor");
    let words: Vec<f32> = PAINT_TABS
        .iter()
        .map(|t| tab_rect(&wide, t).width())
        .collect();
    assert!(words.iter().all(|w| *w > 50.0), "{words:?}");
    // Narrower: the others are icons of one width and the shown tab keeps its title.
    let mid = paint_row(300, "UV editor");
    let others: Vec<f32> = PAINT_TABS[1..]
        .iter()
        .map(|t| tab_rect(&mid, t).width())
        .collect();
    assert!(
        others.iter().all(|w| (w - others[0]).abs() < 0.5),
        "{others:?}"
    );
    assert!(others[0] < words[1], "{others:?} {words:?}");
    assert!((tab_rect(&mid, "UV editor").width() - words[0]).abs() < 0.5);
    // At the smallest width the shown title is cut to what is left, and its whole name stays:
    // the tab is found by it.
    let narrow = paint_row(200, "UV editor");
    let cut = tab_rect(&narrow, "UV editor");
    assert!(cut.width() < words[0] - 4.0, "{cut:?} {words:?}");
    // The icons after it and the cut tab use the room up to the move button, no more.
    let last = tab_rect(&narrow, "History");
    let button = lowest_move_button(&narrow);
    assert!(
        (last.right() - button.left()).abs() < 0.5,
        "{last:?} {button:?}"
    );
}
