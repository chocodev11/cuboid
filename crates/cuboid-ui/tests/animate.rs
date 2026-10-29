//! The Animate workspace in headless frames of the whole window: the mode tab, the timeline below
//! the views, the keys of the presets and the panels (spec: animate A3 2, 3, 8, 9).

mod common;

use std::sync::Arc;

use common::Fixture;
use cuboid_ui::{
    AnimateEdit, AnimationRequest, ComingFrom, Edit, FontSet, KeyKind, Mode, NodeId, PanelId,
    Prefs, Tool, Ui, UiRequest, ViewportEvent,
};
use egui::{Event, Key, Modifiers, PointerButton, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct Rig {
    ui: Ui,
    fx: Fixture,
    requests: Vec<UiRequest>,
}

fn frame(ui: &mut egui::Ui, rig: &mut Rig) {
    let cx = rig.fx.cx();
    let out = rig.ui.show(ui, &cx);
    rig.requests.extend(out.requests);
}

fn prefs(from: ComingFrom) -> Prefs {
    let mut prefs = Prefs::default();
    prefs.apply_coming_from(from);
    prefs.accent_chosen = true;
    prefs
}

/// A window in the Animate workspace over the animated fixture.
fn harness(from: ComingFrom) -> Harness<'static, Rig> {
    let mut ui = Ui::new(prefs(from), FontSet::default());
    ui.set_mode(Mode::Animate);
    let rig = Rig {
        ui,
        fx: Fixture::new().animated(),
        requests: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1600.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .allow_missing_glyphs()
        .build_ui_state(frame, rig);
    harness.run_steps(4);
    harness
}

/// The requests since the last call, without the hover reports and the saved preferences.
fn take(harness: &mut Harness<'static, Rig>) -> Vec<UiRequest> {
    harness.run_steps(3);
    let mut requests = std::mem::take(&mut harness.state_mut().requests);
    requests.retain(|r| {
        !matches!(
            r,
            UiRequest::PrefsChanged(_) | UiRequest::Viewport(ViewportEvent::Hover { .. })
        )
    });
    requests
}

fn chord(harness: &mut Harness<'static, Rig>, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run_steps(3);
}

/// The row of a bone in the timeline: the outliner lists the same names above it.
fn timeline_row(harness: &Harness<'static, Rig>, name: &str) -> egui::Rect {
    harness
        .get_all_by_role_and_label(Role::TreeItem, name)
        .map(|node| node.rect())
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("a row")
}

fn seeks(requests: &[UiRequest]) -> Vec<f32> {
    requests
        .iter()
        .filter_map(|r| match r {
            UiRequest::Animation(AnimationRequest::Seek(t)) => Some(*t),
            _ => None,
        })
        .collect()
}

#[test]
fn the_animate_tab_opens_the_workspace_with_the_timeline_and_the_panels_for_it() {
    let mut ui = Ui::new(prefs(ComingFrom::Blockbench), FontSet::default());
    ui.set_mode(Mode::Model);
    let rig = Rig {
        ui,
        fx: Fixture::new().animated(),
        requests: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1600.0, 900.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, rig);
    harness.run_steps(4);
    assert!(
        harness
            .query_by_label("Fit the animation to the width")
            .is_none()
    );
    harness.get_by_role_and_label(Role::Tab, "Animate").click();
    harness.run_steps(4);
    let requests = take(&mut harness);
    assert!(
        requests.contains(&UiRequest::SetMode(Mode::Animate)),
        "{requests:?}"
    );
    assert_eq!(harness.state().ui.mode(), Mode::Animate);
    harness.get_by_label("Fit the animation to the width");
    // The Animate tab of the properties and the Animations panel are in front.
    harness.get_by_label("Pose");
    harness.get_by_label("Animation");
    assert_eq!(
        harness
            .get_all_by_role_and_label(Role::TreeItem, "Lantern")
            .count(),
        2,
        "the outliner and the timeline list the bone"
    );
    // Leaving takes the timeline away again.
    harness.get_by_role_and_label(Role::Tab, "Model").click();
    harness.run_steps(4);
    assert!(
        harness
            .query_by_label("Fit the animation to the width")
            .is_none()
    );
}

#[test]
fn the_tools_of_model_work_in_animate_with_their_keys() {
    let mut harness = harness(ComingFrom::Blender);
    assert_eq!(harness.state().ui.tool(), Tool::Select);
    chord(&mut harness, Modifiers::NONE, Key::G);
    let requests = take(&mut harness);
    assert!(
        requests.contains(&UiRequest::SetTool(Tool::Move)),
        "{requests:?}"
    );
    assert!(
        !requests.iter().any(|r| matches!(r, UiRequest::SetMode(_))),
        "the workspace stays: {requests:?}"
    );
    assert_eq!(
        (harness.state().ui.mode(), harness.state().ui.tool()),
        (Mode::Animate, Tool::Move)
    );
    // A click on the tool strip stays in the workspace too.
    harness.get_by_label("Rotate").click();
    harness.run_steps(3);
    assert_eq!(harness.state().ui.mode(), Mode::Animate);
    assert_eq!(harness.state().ui.tool(), Tool::Rotate);
    // Tab goes round all five workspaces and ends at Model after Animate.
    chord(&mut harness, Modifiers::NONE, Key::Tab);
    assert_eq!(harness.state().ui.mode(), Mode::Model);
    chord(&mut harness, Modifiers::COMMAND | Modifiers::ALT, Key::Num5);
    assert_eq!(harness.state().ui.mode(), Mode::Animate);
}

#[test]
fn space_plays_and_pauses_unless_the_camera_pans_with_it() {
    for from in [ComingFrom::Blender, ComingFrom::Blockbench] {
        let mut harness = harness(from);
        take(&mut harness);
        chord(&mut harness, Modifiers::NONE, Key::Space);
        assert_eq!(
            take(&mut harness),
            [UiRequest::Animation(AnimationRequest::Play)],
            "{from:?}"
        );
        harness
            .state_mut()
            .fx
            .animations
            .as_mut()
            .expect("animations")
            .playing = true;
        harness.run_steps(2);
        chord(&mut harness, Modifiers::NONE, Key::Space);
        assert_eq!(
            take(&mut harness),
            [UiRequest::Animation(AnimationRequest::Pause)],
            "{from:?}"
        );
    }
    // The Cubik camera pans while Space is held, and its play key is P.
    let mut harness = harness(ComingFrom::Cubik);
    take(&mut harness);
    chord(&mut harness, Modifiers::NONE, Key::Space);
    assert_eq!(take(&mut harness), []);
    chord(&mut harness, Modifiers::NONE, Key::P);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animation(AnimationRequest::Play)]
    );
    // Blender with the Cubik camera keeps Space for the camera as well.
    let mut mixed = prefs(ComingFrom::Blender);
    mixed.nav = cuboid_ui::NavPreset::Cubik;
    let mut ui = Ui::new(mixed, FontSet::default());
    ui.set_mode(Mode::Animate);
    let rig = Rig {
        ui,
        fx: Fixture::new().animated(),
        requests: Vec::new(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(1600.0, 900.0))
        .allow_missing_glyphs()
        .build_ui_state(frame, rig);
    harness.run_steps(4);
    take(&mut harness);
    chord(&mut harness, Modifiers::NONE, Key::Space);
    assert_eq!(take(&mut harness), []);
}

#[test]
fn the_arrow_keys_step_the_playhead_by_frames_and_to_the_ends() {
    for from in [
        ComingFrom::Blender,
        ComingFrom::Blockbench,
        ComingFrom::Cubik,
    ] {
        let mut harness = harness(from);
        take(&mut harness);
        // The playhead is at 0.5 s: frame 12 of 24 per second.
        chord(&mut harness, Modifiers::NONE, Key::ArrowRight);
        chord(&mut harness, Modifiers::NONE, Key::ArrowLeft);
        chord(&mut harness, Modifiers::SHIFT, Key::ArrowLeft);
        chord(&mut harness, Modifiers::SHIFT, Key::ArrowRight);
        let times = seeks(&take(&mut harness));
        assert_eq!(times.len(), 4, "{from:?}");
        assert!((times[0] - 13.0 / 24.0).abs() < 1e-5, "{times:?}");
        assert!((times[1] - 11.0 / 24.0).abs() < 1e-5, "{times:?}");
        assert!(times[2].abs() < 1e-6, "{times:?}");
        assert!((times[3] - 1.5).abs() < 1e-6, "{times:?}");
    }
}

#[test]
fn the_arrow_keys_step_the_playhead_everywhere_because_animate_has_no_uv_editor() {
    let mut harness = harness(ComingFrom::Blockbench);
    take(&mut harness);
    assert!(harness.query_by_label("UV view").is_none());
    // Where the UV editor stands in the other modes (the left area) ...
    harness.event(Event::PointerMoved(vec2(120.0, 200.0).to_pos2()));
    harness.run_steps(2);
    chord(&mut harness, Modifiers::NONE, Key::ArrowRight);
    let requests = take(&mut harness);
    assert_eq!(seeks(&requests).len(), 1, "{requests:?}");
    assert!(
        !requests
            .iter()
            .any(|r| matches!(r, UiRequest::Edit(Edit::MoveFaces { .. }))),
        "{requests:?}"
    );

    // ... and over the views they step the playhead too.
    harness.event(Event::PointerMoved(vec2(800.0, 400.0).to_pos2()));
    harness.run_steps(2);
    chord(&mut harness, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(seeks(&take(&mut harness)).len(), 1);
}

#[test]
fn i_inserts_keyframes_in_the_blender_preset_only() {
    let mut harness = harness(ComingFrom::Blender);
    take(&mut harness);
    chord(&mut harness, Modifiers::NONE, Key::I);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animate(AnimateEdit::KeySelected)]
    );
    let mut harness = self::harness(ComingFrom::Blockbench);
    take(&mut harness);
    chord(&mut harness, Modifiers::NONE, Key::I);
    assert_eq!(take(&mut harness), []);
}

/// Drags over the rows of the timeline from empty space at the left to the right edge, which
/// selects the keyframes of the rows that are in view.
fn select_the_visible_keyframes(harness: &mut Harness<'static, Rig>) {
    let lantern = timeline_row(harness, "Lantern");
    // The bone row and the three channel rows below it, 28 points each.
    let from = egui::pos2(lantern.left() + 200.0 + 2.0, lantern.top() + 2.0);
    let to = egui::pos2(1590.0, lantern.top() + 4.0 * 28.0 - 2.0);
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
            from + (to - from) * (step as f32 / 4.0),
        ));
        harness.run_steps(2);
    }
    harness.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(4);
}

#[test]
fn delete_copy_and_paste_mean_the_selected_keyframes_in_animate() {
    let mut harness = harness(ComingFrom::Blender);
    take(&mut harness);
    // With no keyframe selected the keys do what they do in Model.
    chord(&mut harness, Modifiers::NONE, Key::Delete);
    let plain = take(&mut harness);
    assert!(
        plain.iter().all(|r| !matches!(r, UiRequest::Animate(_))),
        "{plain:?}"
    );
    select_the_visible_keyframes(&mut harness);
    take(&mut harness);

    chord(&mut harness, Modifiers::COMMAND, Key::C);
    let copied = take(&mut harness);
    let [UiRequest::Animate(AnimateEdit::Copy { keys })] = copied.as_slice() else {
        panic!("a copy: {copied:?}");
    };
    assert!(keys.len() >= 6, "{keys:?}");
    chord(&mut harness, Modifiers::COMMAND, Key::V);
    assert_eq!(
        take(&mut harness),
        [UiRequest::Animate(AnimateEdit::Paste {
            bone: Some(NodeId(5))
        })]
    );
    chord(&mut harness, Modifiers::NONE, Key::Delete);
    let deleted = take(&mut harness);
    let [UiRequest::Animate(AnimateEdit::Delete { keys: gone })] = deleted.as_slice() else {
        panic!("a delete: {deleted:?}");
    };
    assert_eq!(gone.len(), keys.len());
    // The selection went with the keyframes: the next Delete is the plain one again.
    chord(&mut harness, Modifiers::NONE, Key::Delete);
    assert!(
        take(&mut harness)
            .iter()
            .all(|r| !matches!(r, UiRequest::Animate(_)))
    );
}

#[test]
fn the_selected_keyframes_show_in_the_animate_tab_and_the_bone_in_the_pose_section() {
    let mut harness = harness(ComingFrom::Blockbench);
    harness.get_by_label("Select a keyframe in the timeline.");
    harness.get_by_label("Rotation X");
    select_the_visible_keyframes(&mut harness);
    harness.get_by_label_contains("keyframes selected");
    // The interpolation of all of them in one step.
    harness
        .get_by_role_and_label(Role::ComboBox, "Linear")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Step").click();
    let sent = take(&mut harness);
    assert!(
        sent.iter().any(|r| matches!(
            r,
            UiRequest::Animate(AnimateEdit::Interp { kind: KeyKind::Step, keys }) if keys.len() >= 6
        )),
        "{sent:?}"
    );
}

#[test]
fn the_animations_panel_makes_duplicates_renames_and_deletes_animations() {
    let mut harness = harness(ComingFrom::Blockbench);
    take(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, "New animation")
        .click();
    harness.get_by_label("Duplicate the animation").click();
    harness.get_by_label("Delete the animation").click();
    assert_eq!(
        take(&mut harness),
        [
            UiRequest::Animate(AnimateEdit::NewAnimation),
            UiRequest::Animate(AnimateEdit::DuplicateAnimation { id: 11 }),
            UiRequest::Animate(AnimateEdit::DeleteAnimation { id: 11 }),
        ]
    );
    // Rename takes the name field; Enter or leaving it sends the new name.
    chord(&mut harness, Modifiers::NONE, Key::F2);
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Animation name")
        .type_text("run");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    let sent = take(&mut harness);
    assert!(
        sent.iter().any(|r| matches!(
            r,
            UiRequest::Animate(AnimateEdit::RenameAnimation { id: 11, name }) if name.contains("run")
        )),
        "{sent:?}"
    );
    // The fields: length, what happens after the end and the frame rate.
    harness.get_by_label("Animation length").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, "Animation length")
        .type_text("2.5");
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    harness
        .get_by_role_and_label(Role::ComboBox, "Loops")
        .click();
    harness.run_steps(3);
    harness.get_by_label("Holds the end").click();
    harness.run_steps(3);
    let sent = take(&mut harness);
    assert!(
        sent.iter().any(|r| matches!(
            r,
            UiRequest::Animate(AnimateEdit::SetLength { id: 11, length, commit: true })
                if (length - 2.5).abs() < 1e-4
        )),
        "{sent:?}"
    );
    assert!(
        sent.iter().any(|r| matches!(
            r,
            UiRequest::Animate(AnimateEdit::SetLoop {
                id: 11,
                kind: cuboid_ui::LoopKind::Hold
            })
        )),
        "{sent:?}"
    );
    harness.get_by_role_and_label(Role::SpinButton, "Frame rate");
}

#[test]
fn a_very_long_animation_name_is_cut_in_the_field_and_leaving_the_field_does_not_rename() {
    let mut harness = harness(ComingFrom::Blockbench);
    let view = harness
        .state_mut()
        .fx
        .animations
        .as_mut()
        .expect("animations");
    let mut rows = view.rows.to_vec();
    rows[0].name = Arc::from("x".repeat(100_000));
    view.rows = Arc::from(rows);
    harness.run_steps(3);
    take(&mut harness);
    let field = harness.get_by_role_and_label(Role::TextInput, "Animation name");
    assert_eq!(field.value().map(|v| v.chars().count()), Some(257));
    field.focus();
    harness.run_steps(2);
    harness.key_press(Key::Tab);
    let sent = take(&mut harness);
    assert!(
        sent.iter()
            .all(|r| !matches!(r, UiRequest::Animate(AnimateEdit::RenameAnimation { .. }))),
        "{sent:?}"
    );
}

#[test]
fn an_empty_animations_panel_offers_a_new_animation() {
    let mut harness = harness(ComingFrom::Blockbench);
    harness.state_mut().fx.animations = None;
    harness.state_mut().fx.timeline = None;
    harness.run_steps(3);
    take(&mut harness);
    harness.get_by_label("No animations");
    harness
        .get_all_by_role_and_label(Role::Button, "New animation")
        .next()
        .expect("a button")
        .click();
    let sent = take(&mut harness);
    assert!(
        sent.contains(&UiRequest::Animate(AnimateEdit::NewAnimation)),
        "{sent:?}"
    );
    assert!(PanelId::ALL.contains(&PanelId::Animations));
}
