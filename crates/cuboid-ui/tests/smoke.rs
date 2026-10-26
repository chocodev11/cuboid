//! Headless smoke tests of the whole window and of each panel, through the public API only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_actions::ActionId;
use cuboid_ui::{
    ColorMode, ComingFrom, DisplaySlotView, DisplayView, DocView, Edit, Face, FaceChange, FaceView,
    FontSet, HistoryView, ImageData, Level, Mode, NavPreset, NodeId, NodeKind, NodeView,
    PaletteView, Prefs, PropsView, RecentEntry, RecoveryChoice, RecoveryEntry, RecoveryInfo,
    SelectOp, Template, TextureView, Tool, Ui, UiCx, UiRequest, ViewSlot, ViewportEvent,
    ViewportFrame, ViewportPlacement,
};
use egui::{Event, Key, Modifiers, PointerButton, Role, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::Vec3;

fn node(id: u64, parent: Option<u64>, name: &str, kind: NodeKind) -> NodeView {
    NodeView {
        id: NodeId(id),
        parent: parent.map(NodeId),
        name: Arc::from(name),
        kind,
        visible: true,
        locked: false,
        hidden_above: false,
    }
}

/// Owned data behind a `UiCx`.
struct Fixture {
    has_doc: bool,
    dirty: bool,
    nodes: Vec<NodeView>,
    selection: Vec<NodeId>,
    props: Option<PropsView>,
    textures: Vec<TextureView>,
    palette: PaletteView,
    history: HistoryView,
    display: DisplayView,
    recent: Vec<RecentEntry>,
    views: [Option<ViewportFrame>; 4],
    deny_save: bool,
    tool_hot: bool,
}

fn face(texture: &str, uv: [f32; 4]) -> FaceView {
    FaceView {
        enabled: true,
        texture: Arc::from(texture),
        uv,
        tint: -1,
        rotation: 0,
        cull: None,
    }
}

impl Fixture {
    fn new() -> Fixture {
        let image = ImageData {
            size: [4, 4],
            rgba: Arc::from(vec![200_u8; 4 * 4 * 4]),
            rev: 1,
        };
        Fixture {
            has_doc: true,
            dirty: false,
            nodes: vec![
                node(1, None, "Body", NodeKind::Group),
                node(2, Some(1), "Head", NodeKind::Element),
                node(3, Some(1), "Arm", NodeKind::Element),
                node(4, None, "Base", NodeKind::Mesh),
            ],
            selection: vec![NodeId(2)],
            props: Some(PropsView {
                kind: NodeKind::Element,
                name: Arc::from("Head"),
                selected: 1,
                position: Vec3::new(1.0, 2.0, 3.0),
                size: Vec3::new(8.0, 8.0, 8.0),
                rotation: Vec3::ZERO,
                pivot: Vec3::new(4.0, 4.0, 4.0),
                color: [1.0, 0.5, 0.25, 1.0],
                shade: true,
                uv_snap: false,
                faces: Some(std::array::from_fn(|i| {
                    face("texture", [0.0, 0.0, 2.0 + i as f32, 2.0])
                })),
                group: None,
            }),
            textures: vec![
                TextureView {
                    label: Arc::from("texture"),
                    size: [4, 4],
                    image: Some(image),
                    source: None,
                },
                TextureView {
                    label: Arc::from("particle"),
                    size: [16, 16],
                    image: None,
                    source: None,
                },
            ],
            palette: PaletteView {
                name: Arc::from("RGB"),
                colors: Arc::from(vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]]),
                left: [1.0, 0.0, 0.0, 1.0],
                right: [0.0, 1.0, 0.0, 1.0],
            },
            history: HistoryView {
                labels: Arc::from(vec![
                    Arc::from("Create cube"),
                    Arc::from("Move"),
                    Arc::from("Rename"),
                ]),
                cursor: 2,
            },
            display: DisplayView {
                slots: Arc::from(vec![
                    DisplaySlotView {
                        name: Arc::from("gui"),
                        enabled: true,
                        preset: Arc::from(""),
                        rotation: Vec3::ZERO,
                        translation: Vec3::ZERO,
                        scale: Vec3::ONE,
                    },
                    DisplaySlotView {
                        name: Arc::from("head"),
                        enabled: true,
                        preset: Arc::from(""),
                        rotation: Vec3::new(0.0, 90.0, 0.0),
                        translation: Vec3::ZERO,
                        scale: Vec3::ONE,
                    },
                ]),
                active: 0,
            },
            recent: vec![RecentEntry {
                path: PathBuf::from("C:/models/tower.cubik"),
                name: "tower.cubik".into(),
                detail: "C:/models".into(),
                thumbnail: None,
                modified: None,
            }],
            views: [None; 4],
            deny_save: false,
            tool_hot: false,
        }
    }

    fn cx(&self) -> UiCx<'_> {
        let mut cx = UiCx::empty(&self.recent);
        cx.views = self.views;
        cx.tool_hot = self.tool_hot;
        if self.deny_save {
            cx.enabled = Some(&deny_save);
        }
        if self.has_doc {
            cx.doc = Some(DocView {
                title: "tower.cubik",
                path: Some(Path::new("C:/models/tower.cubik")),
                dirty: self.dirty,
                nodes: &self.nodes,
                nodes_rev: 1,
                selection: &self.selection,
                primary: self.selection.first().copied(),
                props: self.props.as_ref(),
                textures: &self.textures,
                active_texture: Some(0),
                palette: &self.palette,
                history: &self.history,
                display: Some(&self.display),
                model: None,
                status: "",
            });
        }
        cx
    }
}

fn deny_save(id: ActionId) -> bool {
    id != ActionId::Save
}

struct App {
    ui: Ui,
    fx: Fixture,
    requests: Vec<UiRequest>,
    placements: Vec<ViewportPlacement>,
    title: Option<String>,
    keyboard_captured: bool,
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let App {
        ui: cuboid,
        fx,
        requests,
        placements,
        title,
        keyboard_captured,
    } = app;
    let cx = fx.cx();
    let out = cuboid.show(ui, &cx);
    requests.extend(out.requests);
    *placements = out.viewports;
    if out.title.is_some() {
        *title = out.title;
    }
    *keyboard_captured = out.keyboard_captured;
}

fn prefs_without_system_theme() -> Prefs {
    Prefs {
        color_mode: ColorMode::Dark,
        nav_chosen: true,
        accent_chosen: true,
        ..Prefs::default()
    }
}

fn app_with(fx: Fixture, prefs: Prefs) -> App {
    App {
        ui: Ui::new(prefs, FontSet::default()),
        fx,
        requests: Vec::new(),
        placements: Vec::new(),
        title: None,
        keyboard_captured: false,
    }
}

fn harness_for(app: App) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness
}

fn harness() -> Harness<'static, App> {
    harness_for(app_with(Fixture::new(), prefs_without_system_theme()))
}

fn harness_nav(nav: NavPreset) -> Harness<'static, App> {
    let prefs = Prefs {
        nav,
        ..prefs_without_system_theme()
    };
    harness_for(app_with(Fixture::new(), prefs))
}

fn take(harness: &mut Harness<'static, App>) -> Vec<UiRequest> {
    let mut requests = std::mem::take(&mut harness.state_mut().requests);
    requests.retain(|r| !matches!(r, UiRequest::Viewport(ViewportEvent::Hover { .. })));
    requests
}

fn chord(harness: &mut Harness<'static, App>, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run_steps(3);
}

#[test]
fn start_screen_shows_templates_and_recent_files() {
    let mut fx = Fixture::new();
    fx.has_doc = false;
    let mut harness = harness_for(app_with(fx, prefs_without_system_theme()));
    harness.get_by_role_and_label(Role::Button, "tower");
    harness
        .get_by_role_and_label(Role::ListItem, "Block")
        .click();
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::Button, "tower").click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [
            UiRequest::NewDocument(Template::Block),
            UiRequest::Open(PathBuf::from("C:/models/tower.cubik")),
        ]
    );
    assert_eq!(harness.state().title.as_deref(), Some("Cuboid"));
}

#[test]
fn the_start_screen_offers_the_open_dialog() {
    let mut fx = Fixture::new();
    fx.has_doc = false;
    let mut harness = harness_for(app_with(fx, prefs_without_system_theme()));
    harness.get_by_label("Open file").click();
    harness.run_steps(3);
    assert_eq!(take(&mut harness), [UiRequest::OpenDialog]);
}

#[test]
fn the_document_window_shows_every_main_panel() {
    let harness = harness();
    for label in ["File", "Edit", "View", "Add", "Tools", "Help"] {
        harness.get_by_label(label);
    }
    for mode in ["Model", "Voxel", "Paint", "Display"] {
        harness.get_by_role_and_label(Role::Tab, mode);
    }
    for tab in ["UV editor", "Library", "Textures", "History"] {
        harness.get_by_role_and_label(Role::Tab, tab);
    }
    for tab in [
        "Element",
        "Faces and UV",
        "Texture",
        "Model properties",
        "Display properties",
        "Validator",
    ] {
        harness.get_by_role_and_label(Role::Tab, tab);
    }
    harness.get_by_role_and_label(Role::Button, "Outliner");
    harness.get_by_label("Head");
    harness.get_by_label("Body");
    harness.get_by_label("Position X");
    harness.get_by_label("Size Z");
    assert_eq!(harness.state().placements.len(), 1);
    let placement = harness.state().placements[0];
    assert_eq!(placement.slot, ViewSlot::MAIN);
    assert!(placement.size_px[0] > 100 && placement.size_px[1] > 100);
    assert_eq!(
        harness.state().title.as_deref(),
        Some("tower.cubik - Cuboid")
    );
}

#[test]
fn selecting_a_row_requests_a_selection_change() {
    let mut harness = harness();
    harness.get_by_role_and_label(Role::TreeItem, "Arm").click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Edit(Edit::Select {
            ids: vec![NodeId(3)],
            op: SelectOp::Replace
        })]
    );
    harness
        .get_by_role_and_label(Role::TreeItem, "Base")
        .click_modifiers(Modifiers::COMMAND);
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Edit(Edit::Select {
            ids: vec![NodeId(4)],
            op: SelectOp::Toggle
        })]
    );
}

#[test]
fn the_outliner_toggles_visibility_and_lock() {
    let mut harness = harness();
    let hides = harness
        .get_all_by_role_and_label(Role::Button, "Hide")
        .count();
    assert!(hides >= 4, "{hides}");
    harness
        .get_all_by_role_and_label(Role::Button, "Hide")
        .next()
        .expect("a row")
        .click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    assert!(matches!(
        requests.as_slice(),
        [UiRequest::Edit(Edit::SetVisible { visible: false, .. })]
    ));
}

#[test]
fn collapsing_a_group_hides_its_children() {
    let mut harness = harness();
    harness.get_by_label("Head");
    // The group's arrow is the first child of its row; click through the row's left edge.
    let row = harness.get_by_role_and_label(Role::TreeItem, "Body").rect();
    harness.event(Event::PointerMoved(pos2(row.left() + 14.0, row.center().y)));
    harness.event(Event::PointerButton {
        pos: pos2(row.left() + 14.0, row.center().y),
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.event(Event::PointerButton {
        pos: pos2(row.left() + 14.0, row.center().y),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(3);
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Head")
            .is_none()
    );
    assert!(
        take(&mut harness)
            .iter()
            .all(|r| !matches!(r, UiRequest::Edit(Edit::Select { .. })))
    );
}

#[test]
fn mode_tabs_switch_the_workspace_and_its_tools() {
    let mut harness = harness();
    harness.get_by_role_and_label(Role::Tab, "Paint").click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    assert!(
        requests.contains(&UiRequest::SetMode(Mode::Paint)),
        "{requests:?}"
    );
    assert!(
        requests.contains(&UiRequest::SetTool(Tool::PaintPencil)),
        "{requests:?}"
    );
    assert_eq!(harness.state().ui.mode(), Mode::Paint);
    harness.get_by_label_contains("Pencil");
    harness.get_by_label_contains("Eraser");
}

#[test]
fn shortcuts_run_registry_actions_and_tools() {
    let mut harness = harness();
    chord(&mut harness, Modifiers::COMMAND, Key::S);
    chord(&mut harness, Modifiers::COMMAND, Key::Z);
    chord(&mut harness, Modifiers::NONE, Key::G);
    let requests = take(&mut harness);
    assert_eq!(
        requests,
        [
            UiRequest::Save,
            UiRequest::Action(ActionId::Undo),
            UiRequest::SetTool(Tool::Move)
        ]
    );
    assert_eq!(harness.state().ui.tool(), Tool::Move);
}

#[test]
fn disabled_actions_do_not_run() {
    let mut app = app_with(Fixture::new(), prefs_without_system_theme());
    app.fx.deny_save = true;
    let mut harness = harness_for(app);
    chord(&mut harness, Modifiers::COMMAND, Key::S);
    chord(&mut harness, Modifiers::COMMAND, Key::Z);
    assert_eq!(take(&mut harness), [UiRequest::Action(ActionId::Undo)]);
}

#[test]
fn the_command_palette_runs_any_action() {
    let mut harness = harness();
    chord(&mut harness, Modifiers::COMMAND, Key::P);
    assert!(harness.state().keyboard_captured);
    harness
        .get_by_label("Type a command")
        .type_text("select inv");
    harness.run_steps(3);
    harness.get_by_label("Select inverse");
    chord(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Action(ActionId::SelectInverse)]
    );
    assert!(harness.query_by_label("Type a command").is_none());
}

#[test]
fn the_palette_can_be_closed_with_escape() {
    let mut harness = harness();
    chord(&mut harness, Modifiers::NONE, Key::F3);
    harness.get_by_label("Type a command");
    chord(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.query_by_label("Type a command").is_none());
    assert!(take(&mut harness).is_empty());
}

#[test]
fn typing_a_number_into_a_transform_field_commits_it() {
    let mut harness = harness();
    harness.get_by_label("Position X").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Position X")
        .type_text("2.5");
    harness.run_steps(2);
    chord(&mut harness, Modifiers::NONE, Key::Enter);
    let requests = take(&mut harness);
    let committed = requests.iter().any(|r| {
        matches!(r, UiRequest::Edit(Edit::Transform { value, commit: true, .. })
            if (value.x - 2.5).abs() < 1e-4 && (value.y - 2.0).abs() < 1e-4)
    });
    assert!(committed, "{requests:?}");
}

#[test]
fn faces_and_uv_edits_reach_the_app() {
    let mut harness = harness();
    harness
        .get_by_role_and_label(Role::Tab, "Faces and UV")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Auto UV").scroll_to_me();
    harness.run_steps(10);
    harness.get_by_label("Auto UV").click();
    harness.run_steps(3);
    harness.get_by_label("Flip U").click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [
            UiRequest::Edit(Edit::Face {
                face: Face::Up,
                change: FaceChange::AutoUv
            }),
            UiRequest::Edit(Edit::Face {
                face: Face::Up,
                change: FaceChange::FlipU
            }),
        ]
    );
}

#[test]
fn the_left_area_stacks_uv_and_textures_with_library_and_history_as_tabs() {
    let mut harness = harness();
    for tab in ["UV editor", "Library", "Textures", "History"] {
        harness.get_by_role_and_label(Role::Tab, tab);
    }
    harness.get_by_label("particle");
    harness.get_by_role_and_label(Role::Tab, "History").click();
    harness.run_steps(3);
    harness.get_by_label("Original").click();
    harness.run_steps(3);
    assert!(take(&mut harness).contains(&UiRequest::HistoryJump(0)));
    harness
        .get_by_role_and_label(Role::Tab, "UV editor")
        .click();
    harness.run_steps(3);
}

#[test]
fn a_panel_key_brings_its_tab_forward() {
    let mut harness = harness();
    assert!(harness.query_by_label("Original").is_none());
    chord(&mut harness, Modifiers::NONE, Key::F10);
    harness.get_by_label("Original");
}

#[test]
fn the_outliner_moves_to_the_left_area_and_back_through_the_palette() {
    let mut harness = harness();
    let at = |harness: &Harness<'static, App>| {
        harness
            .state()
            .ui
            .prefs()
            .layout
            .zone_of(cuboid_ui::PanelId::Outliner)
    };
    assert_eq!(at(&harness), Some(cuboid_ui::Zone::Right));
    chord(&mut harness, Modifiers::COMMAND, Key::P);
    harness
        .get_by_label("Type a command")
        .type_text("side panels left");
    harness.run_steps(3);
    chord(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(at(&harness), Some(cuboid_ui::Zone::Left));
    chord(&mut harness, Modifiers::COMMAND, Key::P);
    harness
        .get_by_label("Type a command")
        .type_text("side panels right");
    harness.run_steps(3);
    chord(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(at(&harness), Some(cuboid_ui::Zone::Right));
}

#[test]
fn display_mode_edits_the_slot() {
    let mut harness = harness();
    harness.get_by_role_and_label(Role::Tab, "Display").click();
    harness.run_steps(4);
    harness.get_by_label("Rotation Y");
    harness.get_by_label("Scale X");
}

#[test]
fn a_dirty_document_asks_before_quitting() {
    let mut app = app_with(Fixture::new(), prefs_without_system_theme());
    app.fx.dirty = true;
    let mut harness = harness_for(app);
    assert_eq!(
        harness.state().title.as_deref(),
        Some("* tower.cubik - Cuboid")
    );
    chord(&mut harness, Modifiers::COMMAND, Key::Q);
    assert!(take(&mut harness).is_empty());
    harness.get_by_label("Discard changes").click();
    harness.run_steps(3);
    assert_eq!(take(&mut harness), [UiRequest::Quit]);

    chord(&mut harness, Modifiers::COMMAND, Key::Q);
    harness.get_by_label("Save").click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [UiRequest::SaveThen(Box::new(UiRequest::Quit))]
    );

    chord(&mut harness, Modifiers::COMMAND, Key::Q);
    harness.get_by_label("Cancel").click();
    harness.run_steps(3);
    assert!(take(&mut harness).is_empty());
    assert!(!harness.state_mut().ui.close_requested());
}

#[test]
fn a_clean_document_may_close_at_once() {
    let mut harness = harness();
    assert!(harness.state_mut().ui.close_requested());
    chord(&mut harness, Modifiers::COMMAND, Key::Q);
    assert_eq!(take(&mut harness), [UiRequest::Quit]);
}

#[test]
fn the_recovery_dialog_reports_the_choice() {
    let mut harness = harness();
    harness.state_mut().ui.open_recovery(RecoveryInfo {
        message: "Cuboid did not close properly.".into(),
        entries: vec![RecoveryEntry {
            path: PathBuf::from("recovery/a.cubik"),
            name: "tower".into(),
            saved: "Today 10:00".into(),
            note: String::new(),
        }],
    });
    harness.run_steps(3);
    harness.get_by_label("Recover your work");
    harness.get_by_label("Restore").click();
    harness.run_steps(3);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Recovery(RecoveryChoice::Restore(PathBuf::from(
            "recovery/a.cubik"
        )))]
    );
    assert!(harness.query_by_label("Recover your work").is_none());
}

#[test]
fn preferences_change_the_theme_and_report_it() {
    let mut harness = harness();
    chord(&mut harness, Modifiers::COMMAND, Key::Comma);
    harness.get_by_label("Preferences");
    harness.get_by_label("Light").click();
    harness.run_steps(3);
    let requests = take(&mut harness);
    let changed = requests.iter().find_map(|r| match r {
        UiRequest::PrefsChanged(p) => Some(p.color_mode),
        _ => None,
    });
    assert_eq!(changed, Some(ColorMode::Light));
    assert_eq!(harness.state().ui.prefs().color_mode, ColorMode::Light);
}

#[test]
fn the_keymap_preset_switch_changes_the_shortcuts() {
    let mut app = app_with(Fixture::new(), prefs_without_system_theme());
    app.ui = Ui::new(
        Prefs {
            keymap: cuboid_ui::KeymapPreset::Blockbench,
            ..prefs_without_system_theme()
        },
        FontSet::default(),
    );
    let mut harness = harness_for(app);
    chord(&mut harness, Modifiers::NONE, Key::G);
    assert!(take(&mut harness).is_empty());
    chord(&mut harness, Modifiers::NONE, Key::V);
    assert_eq!(take(&mut harness), [UiRequest::SetTool(Tool::Move)]);
}

#[test]
fn middle_drag_orbits_and_the_wheel_zooms_in_the_viewport() {
    let mut harness = harness_nav(NavPreset::Blender);
    let placement = harness.state().placements[0];
    let centre = pos2(
        placement.rect[0] + placement.rect[2] / 2.0,
        placement.rect[1] + placement.rect[3] / 2.0,
    );
    harness.event(Event::PointerMoved(centre));
    harness.run_steps(2);
    take(&mut harness);
    harness.event(Event::PointerButton {
        pos: centre,
        button: PointerButton::Middle,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    harness.event(Event::PointerMoved(centre + vec2(30.0, 10.0)));
    harness.run_steps(2);
    harness.event(Event::PointerButton {
        pos: centre + vec2(30.0, 10.0),
        button: PointerButton::Middle,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    let requests = take(&mut harness);
    let orbits = requests
        .iter()
        .filter(|r| matches!(r, UiRequest::Viewport(ViewportEvent::Orbit { .. })))
        .count();
    let zooms = requests
        .iter()
        .filter(|r| matches!(r, UiRequest::Viewport(ViewportEvent::Zoom { amount, .. }) if *amount > 0.0))
        .count();
    assert!(orbits >= 1, "{requests:?}");
    assert_eq!(zooms, 1, "{requests:?}");
    assert!(
        !requests
            .iter()
            .any(|r| matches!(r, UiRequest::Viewport(ViewportEvent::Press { .. }))),
        "{requests:?}"
    );
}

#[test]
fn left_drag_in_the_viewport_goes_to_the_tools() {
    let mut harness = harness_nav(NavPreset::Blender);
    let placement = harness.state().placements[0];
    let start = pos2(placement.rect[0] + 100.0, placement.rect[1] + 100.0);
    harness.event(Event::PointerMoved(start));
    harness.run_steps(2);
    take(&mut harness);
    harness.event(Event::PointerButton {
        pos: start,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    harness.event(Event::PointerMoved(start + vec2(20.0, 0.0)));
    harness.run_steps(2);
    harness.event(Event::PointerButton {
        pos: start + vec2(20.0, 0.0),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    let requests = take(&mut harness);
    let kinds: Vec<&str> = requests
        .iter()
        .filter_map(|r| match r {
            UiRequest::Viewport(ViewportEvent::Press { .. }) => Some("press"),
            UiRequest::Viewport(ViewportEvent::Drag { .. }) => Some("drag"),
            UiRequest::Viewport(ViewportEvent::Release { .. }) => Some("release"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds.first(), Some(&"press"), "{requests:?}");
    assert_eq!(kinds.last(), Some(&"release"), "{requests:?}");
    assert!(kinds.contains(&"drag"), "{requests:?}");
}

#[test]
fn quad_layout_places_four_views() {
    let mut prefs = prefs_without_system_theme();
    prefs.view_layout = cuboid_ui::ViewLayout::Quad;
    let harness = harness_for(app_with(Fixture::new(), prefs));
    let slots: Vec<u8> = harness
        .state()
        .placements
        .iter()
        .map(|p| p.slot.0)
        .collect();
    assert_eq!(slots.len(), 4);
    let mut sorted = slots.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 4);
}

#[test]
fn toasts_show_up() {
    let mut harness = harness();
    harness
        .state_mut()
        .ui
        .toast(Level::Error, "Could not save tower.cubik");
    harness.run_steps(3);
    harness.get_by_label_contains("Could not save");
}

#[test]
fn views_with_a_render_texture_draw_it() {
    let mut app = app_with(Fixture::new(), prefs_without_system_theme());
    app.fx.views[3] = Some(ViewportFrame {
        texture: Some(egui::TextureId::Managed(0)),
        size_px: [800, 600],
    });
    let harness = harness_for(app);
    assert_eq!(harness.state().placements.len(), 1);
}

#[test]
fn bad_data_does_not_panic() {
    let mut fx = Fixture::new();
    fx.nodes = vec![
        node(1, Some(2), "", NodeKind::Group),
        node(2, Some(1), "Loop", NodeKind::Group),
        node(3, Some(3), "Self", NodeKind::Element),
        node(3, None, "Twice", NodeKind::Element),
    ];
    fx.selection = vec![NodeId(99), NodeId(1)];
    if let Some(props) = fx.props.as_mut() {
        props.position = Vec3::new(f32::NAN, f32::INFINITY, -1.0e30);
        props.size = Vec3::splat(f32::NAN);
        if let Some(faces) = props.faces.as_mut() {
            faces[0].uv = [f32::NAN, f32::INFINITY, -1.0e30, 1.0e30];
            faces[1].texture = Arc::from("missing");
        }
    }
    fx.textures[0].size = [0, 0];
    fx.textures[1].size = [u32::MAX, u32::MAX];
    fx.history.cursor = 999;
    fx.palette.colors = Arc::from(vec![[f32::NAN; 4]; 3]);
    let mut harness = harness_for(app_with(fx, prefs_without_system_theme()));
    chord(&mut harness, Modifiers::NONE, Key::F8);
    chord(&mut harness, Modifiers::NONE, Key::F10);
    for mode in ["Voxel", "Paint", "Display", "Model"] {
        harness.get_by_role_and_label(Role::Tab, mode).click();
        harness.run_steps(3);
    }
}

#[test]
fn a_large_document_renders_only_the_visible_rows() {
    let mut fx = Fixture::new();
    fx.nodes = (0..20_000_u64)
        .map(|i| node(i + 1, None, &format!("Element {i}"), NodeKind::Element))
        .collect();
    fx.selection = vec![NodeId(1)];
    let harness = harness_for(app_with(fx, prefs_without_system_theme()));
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Element 0")
            .is_some()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Element 19999")
            .is_none()
    );
    let rows = harness.get_all_by_role(Role::TreeItem).count();
    assert!(rows < 100, "{rows}");
}

#[test]
fn ui_scale_is_applied_to_the_context() {
    let mut prefs = prefs_without_system_theme();
    prefs.ui_scale = 1.5;
    let harness = harness_for(app_with(Fixture::new(), prefs));
    assert!((harness.ctx.zoom_factor() - 1.5).abs() < 1e-4);
}

/// Presses `button` in the middle of the main view, moves by `by` and releases; the requests.
fn drag_in_view(
    harness: &mut Harness<'static, App>,
    button: PointerButton,
    modifiers: Modifiers,
    by: egui::Vec2,
) -> Vec<UiRequest> {
    let placement = harness.state().placements[0];
    let start = pos2(
        placement.rect[0] + placement.rect[2] / 2.0,
        placement.rect[1] + placement.rect[3] / 2.0,
    );
    harness.event(Event::PointerMoved(start));
    harness.run_steps(2);
    take(harness);
    harness.event(Event::ModifiersChanged(modifiers));
    for (pos, pressed) in [(start, true), (start + by, false)] {
        harness.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        });
        harness.run_steps(2);
        if pressed {
            harness.event(Event::PointerMoved(start + by / 2.0));
            harness.run_steps(2);
            harness.event(Event::PointerMoved(start + by));
            harness.run_steps(2);
        }
    }
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(2);
    take(harness)
}

fn count(requests: &[UiRequest], wanted: fn(&ViewportEvent) -> bool) -> usize {
    requests
        .iter()
        .filter(|r| matches!(r, UiRequest::Viewport(event) if wanted(event)))
        .count()
}

#[test]
fn blockbench_style_left_drag_orbits_and_right_drag_pans() {
    let mut harness = harness_nav(NavPreset::Blockbench);
    let by = vec2(40.0, 12.0);
    let left = drag_in_view(&mut harness, PointerButton::Primary, Modifiers::NONE, by);
    assert!(
        count(&left, |e| matches!(e, ViewportEvent::Orbit { .. })) >= 1,
        "{left:?}"
    );
    assert_eq!(
        count(&left, |e| matches!(e, ViewportEvent::Press { .. })),
        0,
        "{left:?}"
    );
    let right = drag_in_view(&mut harness, PointerButton::Secondary, Modifiers::NONE, by);
    assert!(
        count(&right, |e| matches!(e, ViewportEvent::Pan { .. })) >= 1,
        "{right:?}"
    );
    let middle = drag_in_view(&mut harness, PointerButton::Middle, Modifiers::NONE, by);
    assert!(
        count(&middle, |e| matches!(e, ViewportEvent::Pan { .. })) >= 1,
        "{middle:?}"
    );
}

#[test]
fn blockbench_style_left_click_selects_and_shift_drag_boxes() {
    let mut harness = harness_nav(NavPreset::Blockbench);
    let click = drag_in_view(
        &mut harness,
        PointerButton::Primary,
        Modifiers::NONE,
        vec2(0.0, 0.0),
    );
    assert_eq!(
        count(&click, |e| matches!(e, ViewportEvent::Press { .. })),
        1,
        "{click:?}"
    );
    assert_eq!(
        count(&click, |e| matches!(e, ViewportEvent::Release { .. })),
        1,
        "{click:?}"
    );
    let boxed = drag_in_view(
        &mut harness,
        PointerButton::Primary,
        Modifiers::SHIFT,
        vec2(40.0, 12.0),
    );
    assert!(
        count(&boxed, |e| matches!(e, ViewportEvent::Drag { .. })) >= 1,
        "{boxed:?}"
    );
    assert_eq!(
        count(&boxed, |e| matches!(e, ViewportEvent::Orbit { .. })),
        0,
        "{boxed:?}"
    );
}

#[test]
fn a_hot_tool_keeps_the_left_drag_in_the_blockbench_style() {
    let mut harness = harness_nav(NavPreset::Blockbench);
    harness.state_mut().fx.tool_hot = true;
    let requests = drag_in_view(
        &mut harness,
        PointerButton::Primary,
        Modifiers::NONE,
        vec2(40.0, 12.0),
    );
    assert!(
        count(&requests, |e| matches!(e, ViewportEvent::Drag { .. })) >= 1,
        "{requests:?}"
    );
    assert_eq!(
        count(&requests, |e| matches!(e, ViewportEvent::Orbit { .. })),
        0,
        "{requests:?}"
    );
}

#[test]
fn the_first_run_asks_which_navigation_to_use_and_remembers_it() {
    let prefs = Prefs {
        color_mode: ColorMode::Dark,
        ..Prefs::default()
    };
    assert!(!prefs.nav_chosen);
    let mut harness = harness_for(app_with(Fixture::new(), prefs));
    for from in ComingFrom::ALL {
        harness.get_by_role_and_label(Role::RadioButton, from.name());
    }
    harness
        .get_by_role_and_label(Role::RadioButton, ComingFrom::Blender.name())
        .click();
    harness.run_steps(3);
    let stored = harness.state().ui.prefs().clone();
    assert_eq!(stored.nav, NavPreset::Blender);
    assert_eq!(stored.keymap, cuboid_ui::KeymapPreset::Blender);
    assert!(stored.nav_chosen);
    assert!(take(&mut harness).iter().any(|r| matches!(
        r,
        UiRequest::PrefsChanged(p) if p.nav == NavPreset::Blender && p.nav_chosen
    )));
    // The accent is the other half of the question; Continue ends it.
    harness.get_by_role_and_label(Role::Button, "Blue").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::Button, "Continue")
        .click();
    harness.run_steps(3);
    // Answered: the question is gone.
    assert!(
        harness
            .query_by_role_and_label(Role::RadioButton, ComingFrom::Blockbench.name())
            .is_none()
    );
}

#[test]
fn f_and_period_frame_the_selection() {
    let mut harness = harness();
    for key in [Key::F, Key::Period] {
        chord(&mut harness, Modifiers::NONE, key);
        let framed = take(&mut harness)
            .iter()
            .filter(|r| matches!(r, UiRequest::Viewport(ViewportEvent::Frame { .. })))
            .count();
        assert_eq!(framed, 1, "{key:?}");
    }
}

fn is_pan(event: &ViewportEvent) -> bool {
    matches!(event, ViewportEvent::Pan { .. })
}

fn is_orbit(event: &ViewportEvent) -> bool {
    matches!(event, ViewportEvent::Orbit { .. })
}

#[test]
fn cubik_style_middle_drag_pans_and_alt_or_ctrl_with_it_orbits() {
    let mut harness = harness_nav(NavPreset::Cubik);
    let by = vec2(40.0, 12.0);
    let plain = drag_in_view(&mut harness, PointerButton::Middle, Modifiers::NONE, by);
    assert!(count(&plain, is_pan) >= 1, "{plain:?}");
    assert_eq!(count(&plain, is_orbit), 0, "{plain:?}");
    for modifiers in [Modifiers::ALT, Modifiers::COMMAND] {
        let turned = drag_in_view(&mut harness, PointerButton::Middle, modifiers, by);
        assert!(count(&turned, is_orbit) >= 1, "{modifiers:?} {turned:?}");
        assert_eq!(count(&turned, is_pan), 0, "{modifiers:?} {turned:?}");
    }
    // The left button belongs to the tools, as in Cubik.
    let left = drag_in_view(&mut harness, PointerButton::Primary, Modifiers::NONE, by);
    assert_eq!(count(&left, is_orbit) + count(&left, is_pan), 0, "{left:?}");
    assert!(
        left.iter()
            .any(|r| matches!(r, UiRequest::Viewport(ViewportEvent::Press { .. }))),
        "{left:?}"
    );
}

#[test]
fn cubik_style_orbit_turns_further_across_than_down() {
    let mut harness = harness_nav(NavPreset::Cubik);
    let sideways = drag_in_view(
        &mut harness,
        PointerButton::Middle,
        Modifiers::ALT,
        vec2(40.0, 0.0),
    );
    let down = drag_in_view(
        &mut harness,
        PointerButton::Middle,
        Modifiers::ALT,
        vec2(0.0, 40.0),
    );
    let total = |requests: &[UiRequest]| {
        requests
            .iter()
            .filter_map(|r| match r {
                UiRequest::Viewport(ViewportEvent::Orbit { delta, .. }) => Some(*delta),
                _ => None,
            })
            .fold(glam::Vec2::ZERO, |sum, d| sum + d)
    };
    let (across, vertical) = (total(&sideways).x, total(&down).y);
    assert!(across > 0.0 && vertical > 0.0, "{across} {vertical}");
    // 0.48 against 0.32 degrees per pixel.
    assert!(
        (across / vertical - 1.5).abs() < 0.05,
        "{across} {vertical}"
    );
}

#[test]
fn cubik_style_pans_while_space_is_held_without_a_button() {
    let mut harness = harness_nav(NavPreset::Cubik);
    let placement = harness.state().placements[0];
    let centre = pos2(
        placement.rect[0] + placement.rect[2] / 2.0,
        placement.rect[1] + placement.rect[3] / 2.0,
    );
    harness.event(Event::PointerMoved(centre));
    harness.run_steps(2);
    take(&mut harness);
    harness.key_down(Key::Space);
    harness.run_steps(2);
    harness.event(Event::PointerMoved(centre + vec2(20.0, 8.0)));
    harness.run_steps(2);
    harness.event(Event::PointerMoved(centre + vec2(40.0, 16.0)));
    harness.run_steps(2);
    let panned = take(&mut harness);
    assert!(count(&panned, is_pan) >= 1, "{panned:?}");
    // With Alt held as well, the same movement orbits.
    harness.event(Event::ModifiersChanged(Modifiers::ALT));
    harness.event(Event::PointerMoved(centre + vec2(60.0, 24.0)));
    harness.run_steps(2);
    let turned = take(&mut harness);
    assert!(count(&turned, is_orbit) >= 1, "{turned:?}");
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.key_up(Key::Space);
    harness.run_steps(2);
    take(&mut harness);
    // Without Space the pointer only hovers.
    harness.event(Event::PointerMoved(centre + vec2(10.0, 5.0)));
    harness.run_steps(2);
    let idle = take(&mut harness);
    assert_eq!(count(&idle, is_pan) + count(&idle, is_orbit), 0, "{idle:?}");
}

#[test]
fn an_idle_window_asks_for_no_further_frames() {
    for has_doc in [true, false] {
        let mut fx = Fixture::new();
        fx.has_doc = has_doc;
        let mut harness = harness_for(app_with(fx, prefs_without_system_theme()));
        harness.try_run().unwrap_or_else(|err| {
            panic!("has_doc {has_doc}: the window keeps repainting: {err:?}")
        });
    }
}

/// The width of the right area, read from the header of the outliner.
fn right_area_width(window: f32) -> f32 {
    let mut harness = Harness::builder()
        .with_size(vec2(window, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(
            frame,
            app_with(Fixture::new(), prefs_without_system_theme()),
        );
    harness.run_steps(4);
    harness
        .get_by_role_and_label(Role::Button, "Outliner")
        .rect()
        .width()
}

#[test]
fn the_right_area_narrows_to_the_strip_plus_260_below_1280() {
    let wide = right_area_width(1920.0);
    let narrow = right_area_width(1100.0);
    assert!(wide > 330.0 && wide < 340.0, "{wide}");
    assert!(narrow <= 300.0, "{narrow}");
    assert!(narrow >= 260.0, "{narrow}");
}
