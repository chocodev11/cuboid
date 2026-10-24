//! The status bar: key hints, document messages, selection count, snapping and job progress.

use cuboid_ui_kit::{Icon, Tokens, icon_button};
use egui::{Align, Layout, ProgressBar, Ui};

use crate::common::Sink;
use crate::model::UiCx;
use crate::prefs::{NavPreset, Prefs};
use crate::request::{Mode, Tool, UiRequest};

/// The hint for the active tool and camera preset.
pub(crate) fn hint(mode: Mode, tool: Tool, nav: NavPreset) -> String {
    let tool_hint = match tool {
        Tool::Select => "Click selects, Shift+click adds, drag draws a selection box",
        Tool::Move => "Drag an arrow or plane to move; Shift+click adds to the selection",
        Tool::Rotate => "Drag a ring to rotate",
        Tool::Scale => "Drag a handle to resize",
        Tool::VoxelPencil => "Left click places a voxel, right click removes one",
        Tool::VoxelErase => "Click or drag to remove voxels",
        Tool::VoxelFill => "Click to fill the connected voxels of one colour",
        Tool::VoxelBox => "Drag out a box of voxels",
        Tool::VoxelLine => "Drag out a line of voxels",
        Tool::VoxelPicker => "Click a voxel to take its colour",
        Tool::PaintPencil => "Left click paints with the left colour, right click with the right",
        Tool::PaintEraser => "Click or drag to clear texels",
        Tool::PaintFill => "Click to fill a connected area of texels",
        Tool::PaintPicker => "Click a texel to take its colour",
    };
    let camera = match nav {
        NavPreset::Blender => "Middle drag orbits, Shift+middle pans, wheel zooms",
        NavPreset::Blockbench => "Right drag orbits, middle drag pans, wheel zooms",
    };
    if mode == Mode::Display {
        return format!("Edit the slot in the properties panel. {camera}");
    }
    format!("{tool_hint}. {camera}")
}

pub(crate) fn show(
    ui: &mut Ui,
    cx: &UiCx<'_>,
    mode: Mode,
    tool: Tool,
    prefs: &Prefs,
    sink: &mut Sink,
) {
    let tokens = Tokens::of(ui.ctx());
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let message = cx.doc.as_ref().map_or("", |d| d.status);
        let text = if message.is_empty() {
            hint(mode, tool, prefs.nav)
        } else {
            message.to_owned()
        };
        ui.add(
            egui::Label::new(egui::RichText::new(text).size(12.0).color(tokens.text_weak))
                .truncate(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(doc) = cx.doc.as_ref() {
                ui.label(
                    egui::RichText::new(format!("{} selected", doc.selection.len()))
                        .size(12.0)
                        .color(tokens.text_weak),
                );
                if let Some(step) = prefs.snap.step() {
                    ui.label(
                        egui::RichText::new(format!(
                            "Snap {}",
                            cuboid_ui_kit::format_number(step, 4)
                        ))
                        .size(12.0)
                        .color(tokens.text_weak),
                    );
                }
            }
            for job in cx.jobs {
                if job.cancellable && icon_button(ui, Icon::Close, "Cancel").clicked() {
                    sink.push(UiRequest::CancelJob(job.id));
                }
                let bar = match job.fraction {
                    Some(f) => ProgressBar::new(f.clamp(0.0, 1.0)),
                    None => ProgressBar::new(0.0).animate(true),
                };
                ui.add(bar.desired_width(120.0).desired_height(10.0));
                ui.label(egui::RichText::new(&job.label).size(12.0));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_hint_that_names_the_camera_buttons() {
        for mode in Mode::ALL {
            for tool in mode.tools() {
                let blender = hint(mode, *tool, NavPreset::Blender);
                let blockbench = hint(mode, *tool, NavPreset::Blockbench);
                assert!(blender.contains("Middle drag orbits"), "{tool:?}");
                assert!(blockbench.contains("Right drag orbits"), "{tool:?}");
            }
        }
        assert!(hint(Mode::Display, Tool::Select, NavPreset::Blender).contains("properties"));
    }
}
