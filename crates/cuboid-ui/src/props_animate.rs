//! The Animate tab of the properties: the pose of the selected bone at the playhead and the
//! selected keyframes (spec: animate A3 4).

use cuboid_ui_kit::{
    Button, DragFieldOpts, EmptyState, Icon, PropertyGrid, Section, Type, drag_field, dropdown,
    format_number, text, text_field,
};
use egui::{ScrollArea, Ui};

use crate::common::{Sink, vector_row};
use crate::model::{AnimationsView, ChannelKind, KeyKind, PoseView, TimelineView};
use crate::prefs::Prefs;
use crate::request::{AnimateEdit, KeyId, UiRequest};
use crate::timeline::{TimelineState, find_key, shared_kind};

/// Largest position the fields accept.
const EXTENT: f32 = 4096.0;
/// Largest rotation the fields accept, in degrees.
const TURNS: f32 = 3600.0;
/// Smallest scale factor the fields accept: the model refuses a bone of no size.
const LEAST_SCALE: f32 = 0.001;
/// Largest scale factor the fields accept.
const MOST_SCALE: f32 = 64.0;

/// The text buffers of the keyframe fields.
#[derive(Default)]
pub(crate) struct AnimateTab {
    text: [String; 3],
    focused: [bool; 3],
}

fn push(sink: &mut Sink, edit: AnimateEdit) {
    sink.push(UiRequest::Animate(edit));
}

/// The page of the tab.
pub(crate) fn show(
    ui: &mut Ui,
    animations: Option<&AnimationsView>,
    view: Option<&TimelineView>,
    timeline: &mut TimelineState,
    tab: &mut AnimateTab,
    prefs: &Prefs,
    sink: &mut Sink,
) {
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let Some(current) = animations.and_then(|a| a.current()) else {
                let empty = EmptyState::new(Icon::Film, "No animation")
                    .body("Add an animation to key the pose.")
                    .action(Button::new("New animation").primary())
                    .show(ui);
                if empty.is_some() {
                    push(sink, AnimateEdit::NewAnimation);
                }
                return;
            };
            let Some(view) = view else {
                return;
            };
            let time = animations.map_or(0.0, |a| a.time);
            pose_section(ui, view.pose.as_ref(), time, prefs, sink);
            keyframe_section(ui, view, current.fps, timeline, tab, sink);
        });
}

fn pose_section(ui: &mut Ui, pose: Option<&PoseView>, time: f32, prefs: &Prefs, sink: &mut Sink) {
    Section::new("Pose").show(ui, "animate_pose", |ui| {
        let Some(pose) = pose else {
            ui.label(text(Type::Caption, "Select a bone to pose it."));
            return;
        };
        ui.label(text(Type::Strong, &*pose.name));
        ui.label(text(
            Type::Caption,
            format!("at {} s", format_number(time, 3)),
        ));
        PropertyGrid::new("animate_pose").show(ui, |grid| {
            for channel in ChannelKind::ALL {
                let opts = match channel {
                    ChannelKind::Rotation => DragFieldOpts {
                        speed: 0.5,
                        decimals: 2,
                        range: -TURNS..=TURNS,
                        unit: "\u{b0}",
                        snap: prefs.snap.enabled.then_some(prefs.snap.rotation),
                        ..DragFieldOpts::default()
                    },
                    ChannelKind::Position => DragFieldOpts {
                        speed: 0.05,
                        decimals: 4,
                        range: -EXTENT..=EXTENT,
                        snap: prefs.snap.step(),
                        ..DragFieldOpts::default()
                    },
                    ChannelKind::Scale => DragFieldOpts {
                        speed: 0.01,
                        decimals: 3,
                        range: LEAST_SCALE..=MOST_SCALE,
                        ..DragFieldOpts::default()
                    },
                };
                let value = pose.values[channel.index()];
                if let Some(edit) = vector_row(grid, channel.name(), value, &opts) {
                    push(
                        sink,
                        AnimateEdit::Key {
                            bone: pose.bone,
                            channel,
                            value: edit.value,
                            commit: edit.commit,
                        },
                    );
                }
            }
        });
        let keyed: Vec<&str> = ChannelKind::ALL
            .into_iter()
            .filter(|c| pose.keyed[c.index()])
            .map(ChannelKind::name)
            .collect();
        if !keyed.is_empty() {
            ui.label(text(
                Type::Caption,
                format!("Keyframe here: {}", keyed.join(", ")),
            ));
        }
    });
}

fn keyframe_section(
    ui: &mut Ui,
    view: &TimelineView,
    fps: u32,
    timeline: &mut TimelineState,
    tab: &mut AnimateTab,
    sink: &mut Sink,
) {
    Section::new("Keyframe").show(ui, "animate_keyframe", |ui| {
        let keys: Vec<KeyId> = timeline.selected().to_vec();
        let Some(&first) = keys.first() else {
            ui.label(text(Type::Caption, "Select a keyframe in the timeline."));
            return;
        };
        if let [key] = keys.as_slice()
            && let Some(mark) = find_key(view, key)
        {
            PropertyGrid::new("animate_key").show(ui, |grid| {
                grid.row("Time", |ui| {
                    let mut time = key.time;
                    let opts = DragFieldOpts {
                        name: "Keyframe time",
                        speed: 0.01,
                        range: 0.0..=3600.0,
                        unit: "s",
                        decimals: 3,
                        snap: Some(1.0 / fps.max(1) as f32),
                        ..DragFieldOpts::default()
                    };
                    let field = drag_field(ui, "animate_key_time", &mut time, &opts);
                    // The field shows its own value while it is dragged; the keyframe moves once,
                    // when the edit ends, so it does not take the place of keyframes on its way.
                    let delta = time - key.time;
                    if field.committed && delta.abs() > f32::EPSILON {
                        push(
                            sink,
                            AnimateEdit::Move {
                                keys: keys.clone(),
                                delta,
                                commit: true,
                            },
                        );
                        // The moved keyframe is the same one: the selection follows it.
                        timeline.follow_move(delta, view.rev);
                    }
                });
                for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                    grid.row(name, |ui| {
                        if !tab.focused[axis] {
                            tab.text[axis] = mark.value[axis].text();
                        }
                        let hint = format!("Keyframe {name}");
                        let field =
                            text_field(ui, ("animate_key_value", axis), &mut tab.text[axis], &hint);
                        tab.focused[axis] = field.has_focus();
                        if field.lost_focus() && !tab.text[axis].trim().is_empty() {
                            let mut value = [0, 1, 2].map(|i| mark.value[i].text());
                            value[axis].clone_from(&tab.text[axis]);
                            if value[axis] != mark.value[axis].text() {
                                push(sink, AnimateEdit::SetValue { key: *key, value });
                            }
                        }
                    });
                }
            });
            if mark.jump {
                ui.label(text(
                    Type::Caption,
                    "This keyframe jumps; a new value removes the jump",
                ));
            }
        } else {
            ui.label(text(
                Type::Caption,
                format!("{} keyframes selected", keys.len()),
            ));
        }
        let kinds = KeyKind::ALL.map(KeyKind::name);
        let mut chosen = shared_kind(view, &keys)
            .or_else(|| find_key(view, &first).map(|m| m.kind))
            .and_then(|kind| KeyKind::ALL.iter().position(|k| *k == kind))
            .unwrap_or(0);
        PropertyGrid::new("animate_interp").show(ui, |grid| {
            grid.row("Interpolation", |ui| {
                if dropdown(ui, "animate_interp_kind", &mut chosen, &kinds).changed() {
                    push(
                        sink,
                        AnimateEdit::Interp {
                            keys: keys.clone(),
                            kind: KeyKind::ALL[chosen],
                        },
                    );
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{Key, Role, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use glam::Vec3;

    use super::*;
    use crate::model::{AnimationRow, BoneRow, KeyMark, KeyNumber, LoopKind, NodeId};

    struct Rig {
        animations: AnimationsView,
        view: TimelineView,
        state: TimelineState,
        tab: AnimateTab,
        prefs: Prefs,
        sink: Sink,
    }

    fn key(time: f32, kind: KeyKind, value: [KeyNumber; 3], jump: bool) -> KeyMark {
        KeyMark {
            time,
            kind,
            value,
            jump,
        }
    }

    fn num(x: f32) -> [KeyNumber; 3] {
        [KeyNumber::Num(x), KeyNumber::Num(0.0), KeyNumber::Num(0.0)]
    }

    /// A bone "root" that is posed at the playhead, with a linear keyframe and a keyframe that
    /// steps, jumps and has an expression on Y.
    fn rig() -> Rig {
        let expression = [
            KeyNumber::Num(3.0),
            KeyNumber::Expr(Arc::from("math.sin(q.anim_time)")),
            KeyNumber::Num(0.0),
        ];
        let none: Arc<[KeyMark]> = Arc::from(Vec::new());
        let rotation: Arc<[KeyMark]> = Arc::from(vec![
            key(0.5, KeyKind::Linear, num(12.0), false),
            key(1.0, KeyKind::Step, expression, true),
        ]);
        Rig {
            animations: AnimationsView {
                rows: Arc::from(vec![AnimationRow {
                    id: 7,
                    name: Arc::from("walk"),
                    length: 2.0,
                    loop_kind: LoopKind::Once,
                    fps: 24,
                }]),
                selected: Some(0),
                time: 0.5,
                speed: 1.0,
                ..AnimationsView::default()
            },
            view: TimelineView {
                rev: 1,
                bones: Arc::from(vec![BoneRow {
                    id: NodeId(1),
                    name: Arc::from("root"),
                    depth: 0,
                    selected: true,
                    channels: [rotation, none.clone(), none],
                }]),
                has_groups: true,
                pose: Some(PoseView {
                    bone: NodeId(1),
                    name: Arc::from("root"),
                    values: [Vec3::new(12.0, 0.0, 0.0), Vec3::ZERO, Vec3::ONE],
                    keyed: [true, false, false],
                }),
            },
            state: TimelineState::default(),
            tab: AnimateTab::default(),
            prefs: Prefs::default(),
            sink: Sink::default(),
        }
    }

    fn frame(ui: &mut Ui, rig: &mut Rig) {
        show(
            ui,
            Some(&rig.animations),
            Some(&rig.view),
            &mut rig.state,
            &mut rig.tab,
            &rig.prefs,
            &mut rig.sink,
        );
    }

    /// The tab and then the timeline, in the order the window draws them.
    fn frame_and_timeline(ui: &mut Ui, rig: &mut Rig) {
        frame(ui, rig);
        rig.state
            .show(ui, Some(&rig.animations), Some(&rig.view), &mut rig.sink);
    }

    fn harness_of(rig: Rig, draw: fn(&mut Ui, &mut Rig)) -> Harness<'static, Rig> {
        let mut harness = Harness::builder()
            .with_size(vec2(360.0, 700.0))
            .allow_missing_glyphs()
            .build_ui_state(draw, rig);
        harness.run_steps(4);
        harness
    }

    fn harness(rig: Rig) -> Harness<'static, Rig> {
        harness_of(rig, frame)
    }

    fn requests(harness: &mut Harness<'static, Rig>) -> Vec<UiRequest> {
        harness.run_steps(3);
        std::mem::take(&mut harness.state_mut().sink.requests)
    }

    fn id(time: f32) -> KeyId {
        KeyId {
            bone: NodeId(1),
            channel: ChannelKind::Rotation,
            time,
        }
    }

    #[test]
    fn the_pose_fields_key_the_channel_of_the_bone_at_the_playhead() {
        let mut harness = harness(rig());
        harness.get_by_label("Keyframe here: Rotation");
        harness.get_by_label("Rotation X").click();
        harness.run_steps(3);
        harness
            .get_by_role_and_label(Role::TextInput, "Rotation X")
            .type_text("30");
        harness.run_steps(2);
        harness.key_press(Key::Enter);
        let sent = requests(&mut harness);
        assert!(
            sent.iter().any(|r| matches!(
                r,
                UiRequest::Animate(AnimateEdit::Key {
                    bone: NodeId(1),
                    channel: ChannelKind::Rotation,
                    value,
                    commit: true,
                }) if (value.x - 30.0).abs() < 1e-4
            )),
            "{sent:?}"
        );
    }

    #[test]
    fn without_a_bone_the_pose_section_says_to_select_one() {
        let mut rig = rig();
        rig.view.pose = None;
        let harness = harness(rig);
        harness.get_by_label("Select a bone to pose it.");
        harness.get_by_label("Select a keyframe in the timeline.");
    }

    #[test]
    fn one_selected_keyframe_has_its_time_values_and_interpolation_to_edit() {
        let mut rig = rig();
        rig.state.select_keys(vec![id(0.5)]);
        let mut harness = harness(rig);
        harness.get_by_label("Keyframe time");
        // A value is a number or an expression, taken when the field loses the keyboard.
        harness
            .get_by_role_and_label(Role::TextInput, "Keyframe X")
            .focus();
        harness.run_steps(2);
        harness
            .get_by_role_and_label(Role::TextInput, "Keyframe X")
            .type_text("5");
        harness.run_steps(2);
        harness.key_press(Key::Tab);
        let sent = requests(&mut harness);
        assert!(
            sent.iter().any(|r| matches!(
                r,
                UiRequest::Animate(AnimateEdit::SetValue { key, value })
                    if (key.time - 0.5).abs() < 1e-6 && value[0].contains('5') && value[1] == "0"
            )),
            "{sent:?}"
        );
        // The interpolation of the keyframe is a choice of the list.
        harness.get_by_label("Linear").click();
        harness.run_steps(3);
        harness.get_by_label("Catmull-Rom").click();
        let sent = requests(&mut harness);
        assert!(
            sent.iter().any(|r| matches!(
                r,
                UiRequest::Animate(AnimateEdit::Interp { kind: KeyKind::CatmullRom, keys })
                    if keys.len() == 1
            )),
            "{sent:?}"
        );
    }

    #[test]
    fn an_expression_is_shown_as_written_and_a_jump_is_named() {
        let mut rig = rig();
        rig.state.select_keys(vec![id(1.0)]);
        let harness = harness(rig);
        harness.get_by_role_and_label(Role::TextInput, "Keyframe Y");
        let shown = harness
            .get_by_role_and_label(Role::TextInput, "Keyframe Y")
            .value();
        assert_eq!(shown.as_deref(), Some("math.sin(q.anim_time)"));
        harness.get_by_label("This keyframe jumps; a new value removes the jump");
    }

    #[test]
    fn several_keyframes_have_an_interpolation_to_change_and_nothing_else() {
        let mut rig = rig();
        rig.state.select_keys(vec![id(0.5), id(1.0)]);
        let mut harness = harness(rig);
        harness.get_by_label("2 keyframes selected");
        assert!(harness.query_by_label("Keyframe time").is_none());
        // The kinds differ, so the list shows the first one.
        harness.get_by_label("Linear").click();
        harness.run_steps(3);
        harness.get_by_label("Step").click();
        let sent = requests(&mut harness);
        assert!(
            sent.iter().any(|r| matches!(
                r,
                UiRequest::Animate(AnimateEdit::Interp { kind: KeyKind::Step, keys })
                    if keys.len() == 2
            )),
            "{sent:?}"
        );
    }

    #[test]
    fn the_time_of_a_keyframe_moves_it_once_and_the_selection_follows() {
        let mut rig = rig();
        rig.state.select_keys(vec![id(0.5)]);
        let mut harness = harness_of(rig, frame_and_timeline);
        harness.get_by_label("Keyframe time").click();
        harness.run_steps(3);
        harness
            .get_by_role_and_label(Role::TextInput, "Keyframe time")
            .type_text("0.75");
        harness.run_steps(2);
        harness.key_press(Key::Enter);
        // The frame that ends the edit sends one move; the timeline drawn after the tab keeps
        // the keyframe selected at its new time.
        let mut sent = Vec::new();
        for _ in 0..5 {
            harness.step();
            sent.append(&mut harness.state_mut().sink.requests);
            if !sent.is_empty() {
                break;
            }
        }
        let moves: Vec<f32> = sent
            .iter()
            .filter_map(|r| match r {
                UiRequest::Animate(AnimateEdit::Move {
                    delta,
                    commit: true,
                    ..
                }) => Some(*delta),
                _ => None,
            })
            .collect();
        assert!(
            matches!(moves.as_slice(), [delta] if (delta - 0.25).abs() < 1e-3),
            "{sent:?}"
        );
        // The app moves the keyframe, and the next view has it there.
        let rig = harness.state_mut();
        let mut bone = rig.view.bones[0].clone();
        bone.channels[0] = Arc::from(vec![
            key(0.75, KeyKind::Linear, num(12.0), false),
            key(1.0, KeyKind::Step, num(3.0), true),
        ]);
        rig.view.bones = Arc::from(vec![bone]);
        rig.view.rev += 1;
        harness.run_steps(3);
        let times: Vec<f32> = harness
            .state()
            .state
            .selected()
            .iter()
            .map(|k| k.time)
            .collect();
        assert!(
            matches!(times.as_slice(), [t] if (t - 0.75).abs() < 1e-3),
            "{times:?}"
        );
    }

    #[test]
    fn a_model_without_an_animation_offers_a_new_one() {
        let mut rig = rig();
        rig.animations.rows = Arc::from(Vec::new());
        rig.animations.selected = None;
        let mut harness = harness(rig);
        harness.get_by_label("No animation");
        harness.get_by_label("New animation").click();
        assert_eq!(
            requests(&mut harness),
            [UiRequest::Animate(AnimateEdit::NewAnimation)]
        );
    }
}
