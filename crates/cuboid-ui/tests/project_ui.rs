//! The shading menu, the project settings window and the preset label of the status bar in
//! headless frames, with synthetic input only.

use std::sync::Arc;

use cuboid_ui::{
    ColorMode, DocView, FontSet, HistoryView, PaletteView, Prefs, PresetKind, ProjectChange,
    ProjectView, Shading, Ui, UiCx, UiRequest,
};
use egui::{Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

static VERSIONS: [String; 0] = [];

struct App {
    ui: Ui,
    palette: PaletteView,
    history: HistoryView,
    requests: Vec<UiRequest>,
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let App {
        ui: cuboid,
        palette,
        history,
        requests,
    } = app;
    let mut cx = UiCx::empty(&[]);
    cx.doc = Some(DocView {
        title: "tower.cubik",
        path: None,
        dirty: false,
        nodes: &[],
        nodes_rev: 1,
        selection: &[],
        primary: None,
        props: None,
        uv_outlines: &[],
        textures: &[],
        active_texture: None,
        texture_playing: false,
        palette,
        history,
        display: None,
        model: None,
        animations: None,
        timeline: None,
        status: "",
    });
    requests.extend(cuboid.show(ui, &cx).requests);
}

fn harness(project: ProjectView) -> Harness<'static, App> {
    let prefs = Prefs {
        color_mode: ColorMode::Dark,
        nav_chosen: true,
        accent_chosen: true,
        ..Prefs::default()
    };
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.set_project(project);
    let app = App {
        ui,
        palette: PaletteView {
            name: Arc::from("RGB"),
            colors: Arc::from(vec![[1.0, 0.0, 0.0, 1.0]]),
            left: [1.0, 0.0, 0.0, 1.0],
            right: [0.0, 1.0, 0.0, 1.0],
        },
        history: HistoryView::default(),
        requests: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, app);
    harness.run_steps(3);
    harness
}

fn minecraft_block() -> ProjectView {
    ProjectView {
        preset: PresetKind::Minecraft,
        version: "1.21.11".to_owned(),
        versions: &VERSIONS,
        shading: Shading::MinecraftBlock,
        ..ProjectView::default()
    }
}

fn changes(harness: &mut Harness<'static, App>) -> Vec<ProjectChange> {
    std::mem::take(&mut harness.state_mut().requests)
        .into_iter()
        .filter_map(|request| match request {
            UiRequest::Project(change) => Some(change),
            _ => None,
        })
        .collect()
}

fn open_shading_menu(harness: &mut Harness<'static, App>) {
    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "Shading")
        .click();
    harness.run_steps(3);
}

#[test]
fn the_viewport_header_has_the_same_shading_menu() {
    let mut harness = harness(minecraft_block());
    harness
        .get_by_role_and_label(Role::Button, "Shading")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Flat").click();
    harness.run_steps(3);
    assert_eq!(
        changes(&mut harness),
        [ProjectChange::SetShading(Shading::Flat)]
    );
}

#[test]
fn the_shading_menu_asks_for_the_chosen_mode() {
    let mut harness = harness(minecraft_block());
    open_shading_menu(&mut harness);
    harness.get_by_label("Flat").click();
    harness.run_steps(3);
    assert_eq!(
        changes(&mut harness),
        [ProjectChange::SetShading(Shading::Flat)]
    );
}

#[test]
fn the_shading_menu_has_the_smooth_light_and_ambient_occlusion_options() {
    let mut harness = harness(minecraft_block());
    open_shading_menu(&mut harness);
    harness.get_by_label("Smooth block light").click();
    harness.run_steps(3);
    assert_eq!(changes(&mut harness), [ProjectChange::SetSmoothLight(true)]);

    open_shading_menu(&mut harness);
    harness.get_by_label("Voxel ambient occlusion").click();
    harness.run_steps(3);
    // The view starts with ambient occlusion on, so the click asks for off.
    assert_eq!(changes(&mut harness), [ProjectChange::SetVoxelAo(false)]);
}

#[test]
fn smooth_light_only_applies_to_the_block_shading() {
    let project = ProjectView {
        shading: Shading::Flat,
        ..minecraft_block()
    };
    let mut harness = harness(project);
    open_shading_menu(&mut harness);
    harness.get_by_label("Smooth block light").click();
    harness.run_steps(3);
    assert!(changes(&mut harness).is_empty());
}

#[test]
fn the_top_bar_names_the_preset_and_opens_the_settings() {
    let mut harness = harness(minecraft_block());
    harness.get_by_label("Minecraft 1.21.11").click();
    harness.run_steps(3);
    harness.get_by_label("Project settings");
}

#[test]
fn the_settings_window_changes_the_preset_and_the_overrides() {
    let mut harness = harness(ProjectView::default());
    assert!(harness.query_by_label("Project settings").is_none());
    harness.get_by_label("File").click();
    harness.run_steps(3);
    harness.get_by_label("Project settings...").click();
    harness.run_steps(3);
    harness.get_by_label("Preset");

    harness.get_by_label("Minecraft").click();
    harness.run_steps(3);
    assert_eq!(
        changes(&mut harness),
        [ProjectChange::SetPreset(PresetKind::Minecraft)]
    );

    harness
        .get_by_label("Allow negative sizes (inverted cubes)")
        .click();
    harness.run_steps(3);
    assert_eq!(
        changes(&mut harness),
        [ProjectChange::SetAllowNegativeSize(true)]
    );

    harness
        .get_by_label("Restrict rotations to one axis and 22.5 degree steps")
        .click();
    harness.run_steps(3);
    assert_eq!(
        changes(&mut harness),
        [ProjectChange::SetRestrictRotations(false)]
    );

    harness.get_by_label("Use the preset defaults").click();
    harness.run_steps(3);
    assert_eq!(
        changes(&mut harness),
        [ProjectChange::SetPreset(PresetKind::Cubik)]
    );
}
