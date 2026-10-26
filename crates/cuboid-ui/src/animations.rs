//! The Animations panel: the animations of the model, the controls that play one and the fields of
//! the chosen one. Playing shows the pose in the views and changes nothing in the document; the
//! list and the fields do change it (spec: animation A2 6, animate A3 8).

use cuboid_ui_kit::{
    Button, DragFieldOpts, EmptyState, Icon, IconButton, ListRow, PropertyGrid, Section,
    SliderOpts, Type, drag_field, dropdown, format_number, slider, text, text_field,
};
use egui::{Label, Ui};

use crate::common::Sink;
use crate::model::{AnimationRow, AnimationsView, LoopKind};
use crate::request::{AnimateEdit, AnimationRequest, UiRequest};

/// The slowest and the fastest playing.
const SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.1..=4.0;
/// The lowest and the highest frame rate, as the model limits it.
const RATE_RANGE: std::ops::RangeInclusive<f32> = 10.0..=500.0;
/// The longest animation, in seconds.
const LONGEST: f32 = 3600.0;

/// The choices of what an animation does after its end, in dropdown order.
const LOOPS: [(LoopKind, &str); 3] = [
    (LoopKind::Once, "Plays once"),
    (LoopKind::Loop, "Loops"),
    (LoopKind::Hold, "Holds the end"),
];

/// State of the panel between frames.
#[derive(Default)]
pub(crate) struct AnimationsPanel {
    name: String,
    name_focused: bool,
    /// The name field takes the keyboard at its next frame: Rename was asked for.
    rename: bool,
}

impl AnimationsPanel {
    /// Asks for the name field of the chosen animation to be edited.
    pub fn begin_rename(&mut self) {
        self.rename = true;
    }
}

fn ask(sink: &mut Sink, request: AnimationRequest) {
    sink.push(UiRequest::Animation(request));
}

fn edit(sink: &mut Sink, edit: AnimateEdit) {
    sink.push(UiRequest::Animate(edit));
}

/// The frame rate a field value stands for, within the range of the model.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rate_of(value: f32) -> u32 {
    value.round().clamp(*RATE_RANGE.start(), *RATE_RANGE.end()) as u32
}

/// The page of the panel.
pub(crate) fn show(
    ui: &mut Ui,
    view: Option<&AnimationsView>,
    panel: &mut AnimationsPanel,
    sink: &mut Sink,
) {
    let Some(view) = view.filter(|v| !v.rows.is_empty()) else {
        let empty = EmptyState::new(Icon::Film, "No animations")
            .body("Add an animation to key a pose, or open a project that has some.")
            .action(Button::new("New animation").primary())
            .show(ui);
        if empty.is_some() {
            edit(sink, AnimateEdit::NewAnimation);
        }
        return;
    };
    let current = view.current();
    Section::new("Animations").show(ui, "animations_list", |ui| {
        ui.horizontal(|ui| {
            if ui
                .add(IconButton::new(Icon::Plus, "New animation"))
                .clicked()
            {
                edit(sink, AnimateEdit::NewAnimation);
            }
            let id = current.map(|c| c.id);
            ui.add_enabled_ui(id.is_some(), |ui| {
                let id = id.unwrap_or_default();
                if ui
                    .add(IconButton::new(Icon::Copy, "Duplicate the animation"))
                    .clicked()
                {
                    edit(sink, AnimateEdit::DuplicateAnimation { id });
                }
                if ui
                    .add(IconButton::new(Icon::Trash, "Delete the animation"))
                    .clicked()
                {
                    edit(sink, AnimateEdit::DeleteAnimation { id });
                }
            });
        });
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, row) in view.rows.iter().enumerate() {
            let caption = format!(
                "{} s, {}",
                format_number(row.length, 2),
                row.loop_kind.name()
            );
            let item = ListRow::new(&row.name)
                .icon(Icon::Film)
                .caption(&caption)
                .selected(view.selected == Some(index));
            let response = ui.add(item);
            if response.clicked() && view.selected != Some(index) {
                ask(sink, AnimationRequest::Select(row.id));
            }
            if response.double_clicked() {
                panel.begin_rename();
            }
        }
    });
    let Some(current) = current else {
        // A rename asked for with no animation chosen does not wait for the next one.
        panel.rename = false;
        return;
    };
    Section::new("Animation").show(ui, "animations_fields", |ui| {
        fields(ui, current, panel, sink)
    });
    Section::new("Playback").show(ui, "animations_playback", |ui| {
        ui.horizontal(|ui| {
            let (icon, tip, request) = if view.playing {
                (Icon::Pause, "Pause", AnimationRequest::Pause)
            } else {
                (Icon::Play, "Play", AnimationRequest::Play)
            };
            if ui.add(IconButton::new(icon, tip)).clicked() {
                ask(sink, request);
            }
            let stop = IconButton::new(Icon::Stop, "Stop and show the rest pose");
            if ui.add(stop).clicked() {
                ask(sink, AnimationRequest::Stop);
            }
            let repeat =
                IconButton::new(Icon::Repeat, "Start again at the end").selected(view.looping);
            if ui.add(repeat).clicked() {
                ask(sink, AnimationRequest::SetLoop(!view.looping));
            }
        });
        let mut time = view.time.clamp(0.0, current.length);
        let opts = SliderOpts {
            name: "Playhead",
            decimals: 2,
            unit: "s",
            ..SliderOpts::default()
        };
        // A zero-length animation has a playhead that cannot move.
        let range = 0.0..=current.length.max(f32::EPSILON);
        if slider(ui, "animation_playhead", &mut time, range, &opts).changed() {
            ask(sink, AnimationRequest::Seek(time));
        }
        let length = format_number(current.length, 2);
        ui.add(Label::new(text(Type::Caption, format!("of {length} s"))).selectable(false));
        PropertyGrid::new("animation_speed").show(ui, |grid| {
            let mut speed = view.speed.clamp(*SPEED_RANGE.start(), *SPEED_RANGE.end());
            let field = DragFieldOpts {
                name: "Playing speed",
                speed: 0.01,
                range: SPEED_RANGE,
                unit: "x",
                decimals: 2,
                ..DragFieldOpts::default()
            };
            let changed = grid.row("Speed", |ui| {
                drag_field(ui, "animation_speed_field", &mut speed, &field).changed
            });
            if changed {
                ask(sink, AnimationRequest::SetSpeed(speed));
            }
        });
    });
}

/// Name, length, what happens after the end and the frame rate of the chosen animation.
fn fields(ui: &mut Ui, current: &AnimationRow, panel: &mut AnimationsPanel, sink: &mut Sink) {
    let id = current.id;
    PropertyGrid::new("animation_fields").show(ui, |grid| {
        grid.row("Name", |ui| {
            if !panel.name_focused {
                panel.name.clear();
                panel.name.push_str(&current.name);
            }
            let field = text_field(ui, "animation_name", &mut panel.name, "Animation name");
            if std::mem::take(&mut panel.rename) {
                field.request_focus();
            }
            panel.name_focused = field.has_focus();
            if field.lost_focus() && panel.name != *current.name && !panel.name.trim().is_empty() {
                edit(
                    sink,
                    AnimateEdit::RenameAnimation {
                        id,
                        name: panel.name.clone(),
                    },
                );
            }
        });
        grid.row("Length", |ui| {
            let mut length = current.length;
            let opts = DragFieldOpts {
                name: "Animation length",
                speed: 0.01,
                range: 0.0..=LONGEST,
                unit: "s",
                decimals: 3,
                ..DragFieldOpts::default()
            };
            let field = drag_field(ui, "animation_length", &mut length, &opts);
            if field.changed || field.committed {
                edit(
                    sink,
                    AnimateEdit::SetLength {
                        id,
                        length,
                        commit: field.committed,
                    },
                );
            }
        });
        grid.row("After the end", |ui| {
            let labels = LOOPS.map(|(_, label)| label);
            let mut chosen = LOOPS
                .iter()
                .position(|(kind, _)| *kind == current.loop_kind)
                .unwrap_or(0);
            if dropdown(ui, "animation_loop", &mut chosen, &labels).changed() {
                edit(
                    sink,
                    AnimateEdit::SetLoop {
                        id,
                        kind: LOOPS[chosen].0,
                    },
                );
            }
        });
        grid.row("Frame rate", |ui| {
            let mut rate = current.fps as f32;
            let opts = DragFieldOpts {
                name: "Frame rate",
                speed: 0.25,
                range: RATE_RANGE,
                unit: "fps",
                decimals: 0,
                snap: Some(1.0),
                ..DragFieldOpts::default()
            };
            let field = drag_field(ui, "animation_rate", &mut rate, &opts);
            if field.changed || field.committed {
                edit(
                    sink,
                    AnimateEdit::SetFrameRate {
                        id,
                        fps: rate_of(rate),
                        commit: field.committed,
                    },
                );
            }
        });
    });
}
