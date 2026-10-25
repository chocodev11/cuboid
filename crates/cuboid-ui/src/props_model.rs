//! The Model tab: the preset of the document, its model box, voxel layer, origin and blueprint.
//! The colours are the Palette panel.

use cuboid_actions::ActionId;
use cuboid_ui_kit::metrics::space;
use cuboid_ui_kit::{Button, DragFieldOpts, Icon, PropertyGrid, Section, Type, text};
use egui::{Label, Ui};

use crate::common::{Sink, vector_row};
use crate::model::{DocView, ModelView};
use crate::project_settings::ProjectView;
use crate::request::Edit;

/// Largest distance of the origin or the blueprint from the centre, in units.
const EXTENT: f32 = 4096.0;

/// The page of the tab.
pub(crate) fn show(ui: &mut Ui, doc: &DocView<'_>, project: &ProjectView, sink: &mut Sink) {
    Section::new("Preset").show(ui, "model_preset", |ui| {
        PropertyGrid::new("model_preset").show(ui, |grid| {
            grid.row("Preset", |ui| {
                ui.add(Label::new(text(Type::Strong, project.label())).truncate());
            });
        });
        if !project.export_note.is_empty() {
            ui.add(Label::new(text(Type::Caption, project.export_note)).wrap());
        }
        ui.add_space(space::SM);
        if ui
            .add(Button::new("Project settings").icon(Icon::Settings))
            .clicked()
        {
            sink.project_settings = true;
        }
    });
    if let Some(model) = doc.model {
        placement(ui, model, sink);
        voxels(ui, model, sink);
    }
}

fn placement(ui: &mut Ui, model: &ModelView, sink: &mut Sink) {
    let [min, max] = model.model_box;
    let size = max - min;
    let unit = DragFieldOpts {
        speed: 0.05,
        decimals: 4,
        range: -EXTENT..=EXTENT,
        ..DragFieldOpts::default()
    };
    Section::new("Placement").show(ui, "model_placement", |ui| {
        PropertyGrid::new("model_placement").show(ui, |grid| {
            grid.row("Model box", |ui| {
                let text = text(
                    Type::Body,
                    format!("{} x {} x {} units", size.x, size.y, size.z),
                );
                ui.add(Label::new(text).truncate().selectable(false))
                    .on_hover_text(format!("From {min} to {max}"));
            });
            if let Some(edit) = vector_row(grid, "Origin", model.origin, &unit) {
                sink.edit(Edit::SetModelOrigin {
                    value: edit.value,
                    commit: edit.commit,
                });
            }
            if let Some(edit) = vector_row(grid, "Blueprint", model.blueprint, &unit) {
                sink.edit(Edit::SetBlueprint {
                    value: edit.value,
                    commit: edit.commit,
                });
            }
        });
    });
}

fn voxels(ui: &mut Ui, model: &ModelView, sink: &mut Sink) {
    Section::new("Voxels").show(ui, "model_voxels", |ui| {
        PropertyGrid::new("model_voxels").show(ui, |grid| {
            grid.row("Resolution", |ui| match model.voxel_resolution {
                Some(per_block) => {
                    ui.add(Label::new(text(
                        Type::Body,
                        format!("{per_block} per block"),
                    )));
                }
                None => {
                    ui.add(Label::new(text(Type::Label, "No voxel layer")));
                }
            });
        });
        if model.voxel_resolution.is_none() {
            ui.add_space(space::SM);
            if ui
                .add(Button::new("Set up voxels").icon(Icon::Voxel))
                .clicked()
            {
                sink.action(ActionId::VoxelSetup);
            }
        }
    });
}
