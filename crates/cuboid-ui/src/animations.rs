//! The Animations panel: the animations of the model and the controls that play one. Playing shows
//! the pose in the views and changes nothing in the document (spec: animation A2 6).

use cuboid_ui_kit::{
    DragFieldOpts, EmptyState, Icon, IconButton, ListRow, PropertyGrid, Section, SliderOpts, Type,
    drag_field, format_number, slider, text,
};
use egui::{Label, Ui};

use crate::common::Sink;
use crate::model::AnimationsView;
use crate::request::{AnimationRequest, UiRequest};

/// The slowest and the fastest playing.
const SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.1..=4.0;

fn ask(sink: &mut Sink, request: AnimationRequest) {
    sink.push(UiRequest::Animation(request));
}

/// The page of the panel.
pub(crate) fn show(ui: &mut Ui, view: Option<&AnimationsView>, sink: &mut Sink) {
    let Some(view) = view.filter(|v| !v.rows.is_empty()) else {
        EmptyState::new(Icon::Film, "No animations")
            .body("Open a Blockbench project that has animations to play them here.")
            .show(ui);
        return;
    };
    Section::new("Animations").show(ui, "animations_list", |ui| {
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
            if ui.add(item).clicked() && view.selected != Some(index) {
                ask(sink, AnimationRequest::Select(row.id));
            }
        }
    });
    let Some(current) = view.current() else {
        return;
    };
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
