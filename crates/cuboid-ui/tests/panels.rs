//! The panels of the editor in headless frames: what each one shows and the requests its controls
//! send. The input is synthetic input to the harness.

mod common;
#[path = "panels/uv.rs"]
mod uv;

use std::sync::Arc;

use common::Fixture;
use cuboid_actions::ActionId;
use cuboid_ui::{
    ComingFrom, DisplayPreset, Edit, Face, FaceChange, FontSet, FramesEdit, FramesRequest,
    FramesView, GroupView, Mode, NodeId, NodeKind, PanelId, Prefs, PresetKind, ProjectView,
    SelectOp, StepView, Ui, UiRequest,
};
use egui::{Event, Key, Modifiers, PointerButton, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

static NO_VERSIONS: [String; 0] = [];

struct Rig {
    ui: Ui,
    fx: Fixture,
    requests: Vec<UiRequest>,
    view: Option<[f32; 4]>,
}

fn frame(ui: &mut egui::Ui, rig: &mut Rig) {
    let cx = rig.fx.cx();
    let out = rig.ui.show(ui, &cx);
    // The document follows a move of faces the way the app applies it: by the step asked for.
    for request in &out.requests {
        if let UiRequest::Edit(Edit::MoveFaces { faces, delta, .. }) = request
            && let Some(all) = rig.fx.props.faces.as_mut()
        {
            for face in faces {
                let uv = &mut all[face.index()].uv;
                uv[0] += delta.x;
                uv[2] += delta.x;
                uv[1] += delta.y;
                uv[3] += delta.y;
            }
        }
    }
    rig.requests.extend(out.requests);
    rig.view = out.viewports.first().map(|v| v.rect);
}

fn prefs(from: ComingFrom) -> Prefs {
    let mut prefs = Prefs::default();
    prefs.apply_coming_from(from);
    prefs.accent_chosen = true;
    prefs
}

fn minecraft() -> ProjectView {
    ProjectView {
        preset: PresetKind::Minecraft,
        version: "1.21.11".to_owned(),
        versions: &NO_VERSIONS,
        ..ProjectView::default()
    }
}

fn harness_with(rig: Rig, size: egui::Vec2) -> Harness<'static, Rig> {
    let mut harness = Harness::builder()
        .with_size(size)
        .allow_missing_glyphs()
        .build_ui_state(frame, rig);
    harness.run_steps(4);
    harness
}

fn rig(prefs: Prefs, fx: Fixture) -> Rig {
    let mut ui = Ui::new(prefs, FontSet::default());
    ui.set_project(minecraft());
    Rig {
        ui,
        fx,
        requests: Vec::new(),
        view: None,
    }
}

fn harness() -> Harness<'static, Rig> {
    harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new()),
        vec2(1600.0, 900.0),
    )
}

/// A window of the Blockbench arrangement that starts in `mode`.
fn harness_in(mode: Mode, fx: Fixture) -> Harness<'static, Rig> {
    let mut rig = rig(prefs(ComingFrom::Blockbench), fx);
    rig.ui.set_mode(mode);
    harness_with(rig, vec2(1600.0, 900.0))
}

/// The requests since the last call, without the hover reports and the saved preferences.
fn take(harness: &mut Harness<'static, Rig>) -> Vec<UiRequest> {
    harness.run_steps(3);
    let mut requests = std::mem::take(&mut harness.state_mut().requests);
    requests.retain(|r| {
        !matches!(
            r,
            UiRequest::PrefsChanged(_)
                | UiRequest::Viewport(cuboid_ui::ViewportEvent::Hover { .. })
        )
    });
    requests
}

fn tab(harness: &mut Harness<'static, Rig>, tip: &str) {
    harness.get_by_role_and_label(Role::Tab, tip).click();
    harness.run_steps(3);
    take(harness);
}

fn edits(requests: &[UiRequest]) -> Vec<&Edit> {
    requests
        .iter()
        .filter_map(|r| match r {
            UiRequest::Edit(edit) => Some(edit),
            _ => None,
        })
        .collect()
}

#[test]
fn the_icon_strip_faces_the_viewport_in_both_arrangements() {
    let standard = harness();
    let strip = standard.get_by_role_and_label(Role::Tab, "Element").rect();
    let field = standard
        .get_by_role_and_label(Role::TextInput, "Object name")
        .rect();
    assert!(strip.right() <= field.left(), "{strip:?} {field:?}");
    let view = standard.state().view.expect("a view is drawn");
    assert!(
        strip.left() >= view[0] + view[2],
        "the strip is right of the views"
    );

    let cubik = harness_with(
        rig(prefs(ComingFrom::Cubik), Fixture::new()),
        vec2(1600.0, 900.0),
    );
    let strip = cubik.get_by_role_and_label(Role::Tab, "Element").rect();
    let field = cubik
        .get_by_role_and_label(Role::TextInput, "Object name")
        .rect();
    assert!(strip.left() >= field.right(), "{strip:?} {field:?}");
    let view = cubik.state().view.expect("a view is drawn");
    assert!(
        (view[0] - strip.right()).abs() <= 4.0,
        "the strip touches the views, not the tool strip: {strip:?} {view:?}"
    );
}

#[test]
fn the_element_tab_edits_the_name_and_the_transform() {
    let mut harness = harness();
    harness.get_by_label("Position Y").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Position Y")
        .type_text("4");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    let requests = take(&mut harness);
    assert!(
        edits(&requests).iter().any(|e| matches!(
            e,
            Edit::Transform { value, commit: true, .. } if (value.y - 4.0).abs() < 1e-4
        )),
        "{requests:?}"
    );
    // A new name goes out when the field loses focus.
    let name = harness.get_by_role_and_label(Role::TextInput, "Object name");
    name.focus();
    harness.run_steps(2);
    harness
        .get_by_role_and_label(Role::TextInput, "Object name")
        .type_text("Pane");
    harness.run_steps(2);
    harness.key_press(Key::Tab);
    let requests = take(&mut harness);
    assert!(
        edits(&requests)
            .iter()
            .any(|e| matches!(e, Edit::Rename { id: NodeId(3), name } if name.contains("Pane"))),
        "{requests:?}"
    );
}

#[test]
fn a_very_long_name_is_cut_in_the_field_and_leaving_the_field_does_not_rename() {
    let mut fx = Fixture::new();
    fx.props.name = Arc::from("x".repeat(100_000));
    let mut harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    let field = harness.get_by_role_and_label(Role::TextInput, "Object name");
    assert_eq!(field.value().map(|v| v.chars().count()), Some(257));
    field.focus();
    harness.run_steps(2);
    harness.key_press(Key::Tab);
    let requests = take(&mut harness);
    assert!(
        edits(&requests)
            .iter()
            .all(|e| !matches!(e, Edit::Rename { .. })),
        "{requests:?}"
    );
}

#[test]
fn a_very_long_name_is_cut_in_the_outliner_rename_and_committing_it_unchanged_does_not_rename() {
    let mut fx = Fixture::new();
    // Glass, the primary node that F2 renames.
    fx.nodes[2].name = Arc::from("x".repeat(100_000));
    let mut harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    harness.key_press(Key::F2);
    harness.run_steps(3);
    let field = harness.get_by_role_and_label(Role::TextInput, "Rename");
    assert_eq!(field.value().map(|v| v.chars().count()), Some(257));
    harness.key_press(Key::Enter);
    let requests = take(&mut harness);
    assert!(
        edits(&requests)
            .iter()
            .all(|e| !matches!(e, Edit::Rename { .. })),
        "{requests:?}"
    );

    // A change is a rename, and what goes out is the edited text, not the whole label.
    harness.key_press(Key::F2);
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Rename")
        .type_text("Pane");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    let requests = take(&mut harness);
    assert!(
        edits(&requests).iter().any(|e| matches!(
            e,
            Edit::Rename { id: NodeId(3), name }
                if name.contains("Pane") && name.chars().count() < 300
        )),
        "{requests:?}"
    );
}

#[test]
fn the_faces_tab_sends_the_face_edits_and_keeps_minecraft_fields_to_the_minecraft_preset() {
    let mut harness = harness();
    tab(&mut harness, "Faces and UV");
    harness.get_by_label("Tint index");
    harness.get_by_role_and_label(Role::Button, "North").click();
    harness.run_steps(3);
    take(&mut harness);
    // The UV editor, left of the properties, has a rotation control of its own.
    let editor = harness.get_by_label("UV view").rect();
    harness
        .get_all_by_role_and_label(Role::RadioButton, "180\u{b0}")
        .find(|control| control.rect().left() > editor.right())
        .expect("the rotation control of the Faces tab")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Auto UV").click();
    harness.run_steps(3);
    harness.get_by_label("Flip V").click();
    let requests = take(&mut harness);
    assert_eq!(
        edits(&requests),
        [
            &Edit::Face {
                face: Face::North,
                change: FaceChange::Rotation(180)
            },
            &Edit::Face {
                face: Face::North,
                change: FaceChange::AutoUv
            },
            &Edit::Face {
                face: Face::North,
                change: FaceChange::FlipV
            },
        ]
    );

    let mut cubik = harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new()),
        vec2(1600.0, 900.0),
    );
    cubik.state_mut().ui.set_project(ProjectView::default());
    cubik.run_steps(2);
    tab(&mut cubik, "Faces and UV");
    assert!(cubik.query_by_label("Tint index").is_none());
    cubik.get_by_label("Auto UV");
}

#[test]
fn the_texture_tab_shows_the_selected_texture_and_sends_its_actions() {
    let mut harness = harness();
    tab(&mut harness, "Texture");
    harness.get_by_label("16 x 16 px");
    harness.get_by_label("C:/models/lantern/lantern.png");
    harness
        .get_by_role_and_label(Role::Button, "Replace")
        .click();
    harness
        .get_by_role_and_label(Role::Button, "Reload")
        .click();
    harness
        .get_by_role_and_label(Role::Button, "Export")
        .click();
    let requests = take(&mut harness);
    assert_eq!(
        requests,
        [
            UiRequest::Action(ActionId::LoadTexture),
            UiRequest::Edit(Edit::ReloadTexture(0)),
            UiRequest::Action(ActionId::SaveTexture),
        ]
    );
    // The palette is not part of this tab.
    assert!(harness.query_by_label("Palettes").is_none());
}

#[test]
fn a_texture_without_a_file_cannot_be_reloaded() {
    let mut fx = Fixture::new();
    fx.textures[0].source = None;
    let mut harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    tab(&mut harness, "Texture");
    harness.get_by_label("Not loaded from a file");
    harness
        .get_by_role_and_label(Role::Button, "Reload")
        .click();
    assert!(take(&mut harness).is_empty());
}

#[test]
fn the_model_tab_shows_the_preset_and_the_placement_and_opens_the_project_settings() {
    let mut harness = harness();
    tab(&mut harness, "Model properties");
    harness.get_by_role_and_label(Role::Label, "Minecraft 1.21.11");
    harness.get_by_label("48 x 48 x 48 units");
    harness.get_by_label("16 per block");
    assert!(harness.query_by_label("Palettes").is_none());
    let before = harness.get_all_by_label("Project settings").count();
    harness
        .get_by_role_and_label(Role::Button, "Project settings")
        .click();
    harness.run_steps(4);
    assert!(harness.get_all_by_label("Project settings").count() > before);
    // The dialog is modal: Esc closes it before the panel is used again.
    harness.key_press(Key::Escape);
    harness.run_steps(3);
    assert_eq!(harness.get_all_by_label("Project settings").count(), before);
    take(&mut harness);
    // The origin goes out as an edit of its own.
    harness.get_by_label("Origin X").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Origin X")
        .type_text("2");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    let requests = take(&mut harness);
    assert!(
        edits(&requests).iter().any(|e| matches!(
            e,
            Edit::SetModelOrigin { value, commit: true } if (value.x - 2.0).abs() < 1e-4
        )),
        "{requests:?}"
    );
}

#[test]
fn a_model_without_voxels_offers_to_set_them_up() {
    let mut fx = Fixture::new();
    fx.model.voxel_resolution = None;
    let mut harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    tab(&mut harness, "Model properties");
    harness.get_by_label("No voxel layer");
    harness
        .get_by_role_and_label(Role::Button, "Set up voxels")
        .click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Action(ActionId::VoxelSetup)]
    );
}

#[test]
fn the_display_tab_lists_the_contexts_and_edits_the_chosen_one() {
    let mut harness = harness_in(Mode::Display, Fixture::new());
    tab(&mut harness, "Display properties");
    for name in [
        "Third person, right hand",
        "First person, left hand",
        "Head",
        "GUI",
        "Ground",
        "Item frame",
    ] {
        harness.get_by_role_and_label(Role::ListItem, name);
    }
    // A click on a row chooses the slot; the switch exports it or not.
    harness
        .get_by_role_and_label(Role::ListItem, "Ground")
        .click();
    harness
        .get_by_label("Third person, left hand exported")
        .click();
    let requests = take(&mut harness);
    assert_eq!(
        edits(&requests),
        [
            &Edit::SelectDisplaySlot(6),
            &Edit::SetDisplayEnabled {
                slot: 1,
                enabled: true
            }
        ]
    );
    // The values of the chosen slot (the fixture chose the GUI, number 5).
    harness.get_by_label("Rotation Y").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Rotation Y")
        .type_text("90");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    let requests = take(&mut harness);
    assert!(
        edits(&requests).iter().any(|e| matches!(
            e,
            Edit::SetDisplaySlot { slot: 5, rotation, commit: true, .. }
                if (rotation.y - 90.0).abs() < 1e-4
                    && (rotation.x - 30.0).abs() < 1e-4
        )),
        "{requests:?}"
    );
}

#[test]
fn a_built_in_parent_can_be_loaded_into_the_chosen_slot() {
    let mut harness = harness_in(Mode::Display, Fixture::new());
    tab(&mut harness, "Display properties");
    harness.get_by_label("Load preset").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "item/handheld")
        .click();
    assert_eq!(
        edits(&take(&mut harness)),
        [&Edit::LoadDisplayPreset {
            slot: 5,
            preset: DisplayPreset::ItemHandheld
        }]
    );
}

#[test]
fn a_cubik_document_sees_the_slots_with_a_note_and_a_switched_off_slot_is_locked() {
    let mut cubik = rig(prefs(ComingFrom::Blockbench), Fixture::new());
    cubik.ui.set_project(ProjectView::default());
    cubik.fx.display.active = 1;
    cubik.ui.set_mode(Mode::Display);
    let mut harness = harness_with(cubik, vec2(1600.0, 900.0));
    tab(&mut harness, "Display properties");
    harness.get_by_label_contains("They matter for the Minecraft export");
    // The slot is off: its fields ignore a click.
    harness.get_by_label("Rotation X").click();
    harness.run_steps(3);
    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, "Rotation X")
            .is_none()
    );
}

#[test]
fn the_validator_lists_the_issues_with_their_objects_and_fixes() {
    let mut harness = harness();
    tab(&mut harness, "Validator");
    for chip in ["Errors 3", "Warnings 1", "Info 1"] {
        harness.get_by_role_and_label(Role::Button, chip);
    }
    harness.get_by_label("Two faces name a texture the list does not have.");
    harness.get_by_label("Frame and 1 more");
    harness
        .get_all_by_role_and_label(Role::Button, "Fix")
        .next()
        .expect("a fix")
        .click();
    let requests = take(&mut harness);
    assert!(matches!(
        requests.as_slice(),
        [UiRequest::Validator(cuboid_ui::ValidatorRequest::Fix {
            rev: 3,
            index: 0
        })]
    ));
    // A click on an issue selects its objects.
    harness
        .get_by_label("Hook reaches 4 units outside the block.")
        .click();
    assert_eq!(
        edits(&take(&mut harness)),
        [&Edit::Select {
            ids: vec![NodeId(8)],
            op: SelectOp::Replace
        }]
    );
}

#[test]
fn the_palette_is_a_panel_that_voxel_and_paint_bring_forward() {
    let mut harness = harness();
    let layout = |harness: &Harness<'static, Rig>, panel| {
        let ui = &harness.state().ui;
        ui.prefs().layout.is_shown(panel, ui.mode())
    };
    assert!(!layout(&harness, PanelId::Palette));
    assert!(harness.query_by_label("Left button").is_none());
    harness.get_by_role_and_label(Role::Tab, "Voxel").click();
    harness.run_steps(3);
    assert!(layout(&harness, PanelId::Palette));
    harness.get_by_label("Left button");
    harness.get_by_label("Right button");
    // The colours are picked from the swatches.
    take(&mut harness);
    harness.get_by_role_and_label(Role::Tab, "Paint").click();
    harness.run_steps(3);
    assert!(layout(&harness, PanelId::Palette));
    assert!(layout(&harness, PanelId::Uv));
    harness.get_by_role_and_label(Role::Tab, "Model").click();
    harness.run_steps(3);
}

#[test]
fn the_palette_panel_picks_removes_and_loads_colours() {
    let mut rig = rig(prefs(ComingFrom::Blockbench), Fixture::new());
    rig.ui.set_mode(Mode::Voxel);
    let mut harness = harness_with(rig, vec2(1600.0, 900.0));
    take(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, "Add the left colour to the palette")
        .click();
    let requests = take(&mut harness);
    assert!(matches!(
        edits(&requests).as_slice(),
        [Edit::AddPaletteColor(_)]
    ));
    harness.get_by_label("Palettes").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "Pico-8")
        .click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Action(ActionId::Pico8Palette)]
    );
}

#[test]
fn the_textures_panel_lists_and_sends_the_texture_actions() {
    let mut harness = harness();
    harness
        .get_by_role_and_label(Role::ListItem, "particle")
        .click();
    harness
        .get_by_role_and_label(Role::Button, "Add texture")
        .click();
    for tip in [
        "Import a texture from a file",
        "Replace the selected texture from a file",
        "Remove the selected texture",
    ] {
        harness.get_by_role_and_label(Role::Button, tip).click();
    }
    let requests = take(&mut harness);
    assert_eq!(
        requests,
        [
            UiRequest::Edit(Edit::SelectTexture(1)),
            UiRequest::Action(ActionId::AddTexture),
            UiRequest::Action(ActionId::LoadTexture),
            UiRequest::Action(ActionId::LoadTexture),
            UiRequest::Action(ActionId::DeleteTexture),
        ]
    );
    harness
        .get_by_role_and_label(Role::ListItem, "lantern")
        .click_secondary();
    harness.run_steps(3);
    for item in ["Replace from file", "Remove", "Rename"] {
        harness.get_by_role_and_label(Role::MenuItem, item);
    }
    harness
        .get_by_role_and_label(Role::MenuItem, "Remove")
        .click();
    assert!(take(&mut harness).contains(&UiRequest::Action(ActionId::DeleteTexture)));
}

/// The lantern with its first texture as a strip of four frames, the second one shown.
fn strip_fixture(playing: bool) -> Fixture {
    let mut fx = Fixture::new();
    fx.textures[0].animation = Some(FramesView {
        frame_height: 16,
        tiles: 4,
        frametime: 2,
        interpolate: false,
        steps: (0..4)
            .map(|tile| StepView { tile, time: None })
            .collect::<Vec<_>>()
            .into(),
        explicit: false,
        shown: 1,
    });
    fx.texture_playing = playing;
    fx
}

fn frames_harness(playing: bool) -> Harness<'static, Rig> {
    // Tall enough for the list of steps and the buttons under it to be on the screen.
    harness_with(
        rig(prefs(ComingFrom::Blockbench), strip_fixture(playing)),
        vec2(1600.0, 1800.0),
    )
}

fn set(animation: FramesEdit) -> UiRequest {
    UiRequest::Frames(FramesRequest::Set {
        texture: 0,
        animation: Some(animation),
    })
}

fn steps(list: &[(u32, Option<u32>)]) -> FramesEdit {
    FramesEdit {
        frame_height: 16,
        frametime: 2,
        interpolate: false,
        steps: list.to_vec(),
    }
}

#[test]
fn an_animated_texture_has_its_frame_count_and_the_panel_a_play_button() {
    let mut harness = frames_harness(false);
    harness
        .get_by_role_and_label(Role::Button, "Play the animated textures")
        .click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Frames(FramesRequest::Play(true))]
    );

    let mut harness = frames_harness(true);
    harness
        .get_by_role_and_label(Role::Button, "Pause the animated textures")
        .click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Frames(FramesRequest::Play(false))]
    );

    // Without an animated texture there is nothing to play.
    let still = harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new()),
        vec2(1600.0, 900.0),
    );
    assert!(
        still
            .query_by_role_and_label(Role::Button, "Play the animated textures")
            .is_none()
    );
}

#[test]
fn the_frames_section_lists_the_steps_and_shows_one_on_a_click() {
    let mut harness = frames_harness(false);
    tab(&mut harness, "Texture");
    harness.get_by_label("16 x 16 px, 4 frames");
    for step in 1..=4 {
        harness.get_by_role_and_label(Role::ListItem, &format!("Step {step}"));
    }
    harness
        .get_by_role_and_label(Role::ListItem, "Step 3")
        .click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Frames(FramesRequest::Show {
            texture: 0,
            step: 2
        })]
    );
}

#[test]
fn the_buttons_of_the_frames_section_change_the_order_and_add_frames() {
    let mut harness = frames_harness(false);
    tab(&mut harness, "Texture");
    let show = |step| UiRequest::Frames(FramesRequest::Show { texture: 0, step });
    // The shown step is the second one: it moves up, down and out, and a frame is added.
    harness
        .get_by_role_and_label(Role::Button, "Move the step up")
        .click();
    harness
        .get_by_role_and_label(Role::Button, "Move the step down")
        .click();
    harness
        .get_by_role_and_label(Role::Button, "Remove the step")
        .click();
    harness
        .get_by_role_and_label(
            Role::Button,
            "Add a copy of the frame shown at the end of the strip",
        )
        .click();
    assert_eq!(
        take(&mut harness),
        [
            set(steps(&[(1, None), (0, None), (2, None), (3, None)])),
            show(0),
            set(steps(&[(0, None), (2, None), (1, None), (3, None)])),
            show(2),
            set(steps(&[(0, None), (2, None), (3, None)])),
            show(1),
            UiRequest::Frames(FramesRequest::AddFrame { texture: 0 }),
            show(4),
        ]
    );
}

#[test]
fn the_frames_section_sets_the_time_the_blend_and_a_still_texture_again() {
    let mut harness = frames_harness(false);
    tab(&mut harness, "Texture");
    harness
        .get_by_role_and_label(Role::CheckBox, "Blend frames")
        .click();
    harness
        .get_by_role_and_label(Role::Button, "Make still")
        .click();
    let requests = take(&mut harness);
    assert_eq!(
        requests,
        [
            set(FramesEdit {
                interpolate: true,
                ..steps(&[])
            }),
            UiRequest::Frames(FramesRequest::Set {
                texture: 0,
                animation: None
            }),
        ]
    );

    // A typed time of the shown step is one step of its own; the others keep theirs.
    harness.get_by_label("Time of the step").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Time of the step")
        .type_text("7");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    assert_eq!(
        take(&mut harness),
        [
            set(steps(&[(0, None), (1, Some(7)), (2, None), (3, None)])),
            UiRequest::Frames(FramesRequest::Show {
                texture: 0,
                step: 1
            }),
        ]
    );
}

#[test]
fn a_still_texture_can_become_a_strip_when_its_height_allows() {
    let mut harness = harness();
    tab(&mut harness, "Texture");
    harness
        .get_by_role_and_label(Role::Button, "Animate")
        .click();
    assert_eq!(
        take(&mut harness),
        [set(FramesEdit {
            frametime: 1,
            ..steps(&[])
        })]
    );

    let mut fx = Fixture::new();
    fx.textures[0].animatable = None;
    let mut harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    tab(&mut harness, "Texture");
    harness.get_by_label("The picture must be a whole number of squares high.");
    harness
        .get_by_role_and_label(Role::Button, "Animate")
        .click();
    assert!(take(&mut harness).is_empty());
}

#[test]
fn an_empty_texture_list_offers_the_two_ways_to_get_one() {
    let mut harness = harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new().blank()),
        vec2(1600.0, 900.0),
    );
    harness.get_by_label("No textures yet");
    harness.get_by_label("Import").click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Action(ActionId::LoadTexture)]
    );
    harness.get_by_label("Add texture").click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Action(ActionId::AddTexture)]
    );
}

#[test]
fn the_history_marks_the_current_step_and_an_empty_one_says_so() {
    let mut harness = harness();
    harness.get_by_role_and_label(Role::Tab, "History").click();
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::ListItem, "Original");
    harness
        .get_by_role_and_label(Role::ListItem, "Rename")
        .click();
    assert_eq!(take(&mut harness), [UiRequest::HistoryJump(4)]);
    // The step that is current does not jump.
    harness
        .get_by_role_and_label(Role::ListItem, "Move")
        .click();
    assert_eq!(take(&mut harness), []);

    let mut empty = harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new().blank()),
        vec2(1600.0, 900.0),
    );
    empty.get_by_role_and_label(Role::Tab, "History").click();
    empty.run_steps(3);
    empty.get_by_label("No history yet");
}

#[test]
fn the_outliner_has_a_context_menu_a_rename_and_an_empty_state() {
    let mut harness = harness();
    harness
        .get_by_role_and_label(Role::TreeItem, "Cap")
        .click_secondary();
    harness.run_steps(3);
    for item in [
        "Rename",
        "Duplicate",
        "Group selection",
        "Hide",
        "Lock",
        "Delete",
    ] {
        harness.get_by_role_and_label(Role::MenuItem, item);
    }
    take(&mut harness);
    harness
        .get_by_role_and_label(Role::MenuItem, "Duplicate")
        .click();
    assert!(take(&mut harness).contains(&UiRequest::Action(ActionId::Clone)));

    // F2 renames the primary selection; Enter sends the name.
    harness.key_press(Key::F2);
    harness.run_steps(3);
    let field = harness.get_by_role_and_label(Role::TextInput, "Rename");
    field.type_text("Pane");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    let requests = take(&mut harness);
    assert!(
        edits(&requests)
            .iter()
            .any(|e| matches!(e, Edit::Rename { id: NodeId(3), name } if name.contains("Pane"))),
        "{requests:?}"
    );

    let mut empty = harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new().blank()),
        vec2(1600.0, 900.0),
    );
    empty.get_by_label("No objects yet");
    // The tool strip has a button of that name too: the one in the area is further right.
    let add = empty
        .get_all_by_role_and_label(Role::Button, "Add cube")
        .max_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the button of the empty state");
    add.click();
    assert_eq!(
        take(&mut empty),
        [UiRequest::Action(ActionId::CreateElementCube)]
    );
}

#[test]
fn a_group_shows_its_origin_rotation_and_flags_in_the_element_tab() {
    let mut fx = Fixture::new();
    fx.selection = vec![NodeId(5)];
    fx.props.kind = NodeKind::Group;
    fx.props.name = "Chain".into();
    fx.props.selected = 1;
    fx.props.faces = None;
    fx.props.group = Some(GroupView {
        visible: true,
        locked: false,
        export: true,
        children: 2,
    });
    let mut harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    for label in ["Origin", "Rotation", "Visible", "Locked", "Export"] {
        harness.get_by_label(label);
    }
    for label in ["Position", "Size", "Pivot"] {
        assert!(harness.query_by_label(label).is_none(), "{label}");
    }
    harness.get_by_label("2 inside");
    take(&mut harness);
    harness
        .get_by_role_and_label(Role::CheckBox, "Included in exports with groups")
        .click();
    assert!(edits(&take(&mut harness)).iter().any(|e| matches!(
        e,
        Edit::SetExport {
            id: NodeId(5),
            export: false
        }
    )),);
    harness
        .get_by_role_and_label(Role::Button, "Select children")
        .click();
    assert!(
        edits(&take(&mut harness))
            .iter()
            .any(|e| matches!(e, Edit::SelectChildren { id: NodeId(5) })),
    );
}

#[test]
fn the_context_menu_of_a_group_can_ungroup_and_select_what_it_holds() {
    let mut harness = harness();
    harness
        .get_by_role_and_label(Role::TreeItem, "Chain")
        .click_secondary();
    harness.run_steps(3);
    for item in ["Group selection", "Ungroup", "Select children"] {
        harness.get_by_role_and_label(Role::MenuItem, item);
    }
    take(&mut harness);
    harness
        .get_by_role_and_label(Role::MenuItem, "Select children")
        .click();
    assert!(
        edits(&take(&mut harness))
            .iter()
            .any(|e| matches!(e, Edit::SelectChildren { id: NodeId(5) }))
    );

    // An element has neither of the two group rows.
    harness
        .get_by_role_and_label(Role::TreeItem, "Cap")
        .click_secondary();
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::MenuItem, "Group selection");
    assert!(
        harness
            .query_by_role_and_label(Role::MenuItem, "Select children")
            .is_none()
    );
}

#[test]
fn dragging_a_row_onto_a_group_moves_it_inside() {
    let mut harness = harness();
    let from = harness
        .get_by_role_and_label(Role::TreeItem, "Cap")
        .rect()
        .center();
    let onto = harness
        .get_by_role_and_label(Role::TreeItem, "Chain")
        .rect()
        .center();
    take(&mut harness);
    harness.event(Event::PointerMoved(from));
    harness.run_steps(2);
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
    for step in 1..=4 {
        harness.event(Event::PointerMoved(
            from + (onto - from) * (step as f32 / 4.0),
        ));
        harness.run_steps(2);
    }
    harness.event(Event::PointerButton {
        pos: onto,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    let requests = take(&mut harness);
    assert!(
        edits(&requests).iter().any(|e| matches!(
            e,
            Edit::Reparent {
                parent: Some(NodeId(5)),
                before: None,
                ..
            }
        )),
        "{requests:?}"
    );
}

#[test]
fn the_library_panel_imports_through_a_menu_and_says_when_it_is_empty() {
    let mut harness = harness();
    harness.get_by_role_and_label(Role::Tab, "Library").click();
    harness.run_steps(3);
    take(&mut harness);
    harness.get_by_label("Import").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "Import folder...")
        .click();
    assert!(
        take(&mut harness).contains(&UiRequest::Library(cuboid_ui::LibraryRequest::ImportFolder))
    );
}

#[test]
fn the_uv_editor_has_a_toolbar_and_a_face_picker() {
    let mut harness = harness();
    for tip in [
        "Apply auto UV",
        "Rotate the UV a quarter turn",
        "Mirror the UV left to right",
        "Flip the UV top to bottom",
    ] {
        harness.get_by_role_and_label(Role::Button, tip);
    }
    harness.get_by_label("Apply auto UV").click();
    assert_eq!(
        edits(&take(&mut harness)),
        [&Edit::Faces {
            faces: vec![Face::Up],
            change: FaceChange::AutoUv
        }]
    );
    harness.get_by_role_and_label(Role::Button, "Up").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::MenuItem, "West")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Apply auto UV").click();
    assert_eq!(
        edits(&take(&mut harness)),
        [&Edit::Faces {
            faces: vec![Face::West],
            change: FaceChange::AutoUv
        }]
    );
}

#[test]
fn a_narrow_window_keeps_the_search_to_its_icon_and_chord() {
    let wide = harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new()),
        vec2(1400.0, 800.0),
    );
    let narrow = harness_with(
        rig(prefs(ComingFrom::Blockbench), Fixture::new()),
        vec2(900.0, 700.0),
    );
    let width = |harness: &Harness<'static, Rig>| {
        harness
            .get_by_role_and_label(Role::Button, "Search commands")
            .rect()
            .width()
    };
    assert!(width(&wide) > 150.0, "{}", width(&wide));
    assert!(width(&narrow) < 90.0, "{}", width(&narrow));
}

#[test]
fn a_narrow_header_moves_the_tool_options_into_a_menu() {
    let mut quad = prefs(ComingFrom::Blockbench);
    quad.view_layout = cuboid_ui::ViewLayout::Quad;
    let mut app = rig(quad, Fixture::new());
    app.ui.set_mode(Mode::Voxel);
    let mut harness = harness_with(app, vec2(1024.0, 768.0));
    take(&mut harness);
    harness.get_by_label("Tool options").click();
    harness.run_steps(3);
    harness.get_by_label("Brush");
    harness.get_by_label("Mirror");
    harness.get_by_role_and_label(Role::Button, "Y").click();
    harness.run_steps(3);
    let options = take(&mut harness).into_iter().find_map(|r| match r {
        UiRequest::SetToolOptions(options) => Some(options),
        _ => None,
    });
    assert_eq!(options.map(|o| o.mirror), Some([false, true, false]));

    // A wide header keeps them in the bar, with no menu.
    let mut app = rig(prefs(ComingFrom::Blockbench), Fixture::new());
    app.ui.set_mode(Mode::Voxel);
    let wide = harness_with(app, vec2(1600.0, 900.0));
    assert!(wide.query_by_label("Tool options").is_none());
    wide.get_by_label("Brush size");
}

#[test]
fn a_large_outliner_draws_the_visible_rows_only() {
    let mut fx = Fixture::new();
    fx.nodes = (0..10_000_u64)
        .map(|i| {
            let mut node = fx.nodes[1].clone();
            node.id = NodeId(i + 100);
            node.parent = None;
            node.name = std::sync::Arc::from(format!("Element {i}"));
            node
        })
        .collect();
    let harness = harness_with(rig(prefs(ComingFrom::Blockbench), fx), vec2(1600.0, 900.0));
    let rows = harness.get_all_by_role(Role::TreeItem).count();
    assert!(rows > 5 && rows < 60, "{rows}");
}

fn animations_view(playing: bool, previewing: bool) -> cuboid_ui::AnimationsView {
    use std::sync::Arc;

    use cuboid_ui::{AnimationRow, AnimationsView, LoopKind};
    AnimationsView {
        rows: Arc::from(vec![
            AnimationRow {
                id: 11,
                name: Arc::from("walk"),
                length: 1.5,
                loop_kind: LoopKind::Loop,
                fps: 24,
            },
            AnimationRow {
                id: 12,
                name: Arc::from("idle"),
                length: 4.0,
                loop_kind: LoopKind::Once,
                fps: 30,
            },
        ]),
        selected: Some(0),
        playing,
        previewing,
        time: 0.5,
        speed: 1.0,
        looping: false,
    }
}

#[test]
fn the_animations_panel_lists_the_animations_and_drives_the_player() {
    use cuboid_ui::AnimationRequest;

    let mut fx = Fixture::new();
    fx.animations = Some(animations_view(false, false));
    let mut harness = harness_in(Mode::Animate, fx);
    assert!(PanelId::ALL.contains(&PanelId::Animations));
    harness.get_by_label("walk");
    harness.get_by_label("idle");
    take(&mut harness);

    harness
        .get_by_role_and_label(Role::ListItem, "idle")
        .click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animation(AnimationRequest::Select(12))]
    );
    // The animation that is chosen already is not chosen again.
    harness
        .get_by_role_and_label(Role::ListItem, "walk")
        .click();
    assert_eq!(take(&mut harness), []);

    harness.get_by_label("Play").click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animation(AnimationRequest::Play)]
    );
    harness.get_by_label("Stop and show the rest pose").click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animation(AnimationRequest::Stop)]
    );
    harness.get_by_label("Start again at the end").click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animation(AnimationRequest::SetLoop(true))]
    );
    // The panel only reads: a frame without a click sends nothing.
    harness.run_steps(5);
    assert_eq!(take(&mut harness), []);
}

#[test]
fn a_playing_animation_offers_pause_and_a_preview_is_named_in_the_view_header() {
    use cuboid_ui::AnimationRequest;

    let mut fx = Fixture::new();
    fx.animations = Some(animations_view(true, true));
    let mut harness = harness_in(Mode::Animate, fx);
    harness.get_by_label("Animating walk");
    assert!(harness.query_by_label("Play").is_none());
    take(&mut harness);
    harness.get_by_label("Pause").click();
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animation(AnimationRequest::Pause)]
    );

    // The other modes name the preview, and without a preview the header has no such label.
    let mut fx = Fixture::new();
    fx.animations = Some(animations_view(true, true));
    harness_in(Mode::Model, fx).get_by_label("Previewing walk");
    let mut fx = Fixture::new();
    fx.animations = Some(animations_view(false, false));
    let harness = harness_in(Mode::Animate, fx);
    assert!(harness.query_by_label("Animating walk").is_none());
}

#[test]
fn the_animations_panel_of_a_model_without_animations_says_so() {
    let harness = harness_in(Mode::Animate, Fixture::new());
    harness.get_by_label("No animations");
}
