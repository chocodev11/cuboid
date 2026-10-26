//! The status bar: viewport hints while a view is hovered, job progress in the middle, counts
//! and the version at the right.

use cuboid_ui_kit::metrics::{gutter, space};
use cuboid_ui_kit::{Icon, IconButton, Type, format_number, progress, text};
use egui::{Align, Label, Layout, Rect, Sense, Ui, UiBuilder, vec2};

use crate::common::Sink;
use crate::model::UiCx;
use crate::prefs::{NavPreset, Prefs};
use crate::request::{Mode, Tool, UiRequest};

/// Width of a job's progress bar.
const PROGRESS_WIDTH: f32 = 120.0;
/// Most of the bar the job progress may take.
const JOBS_SHARE: f32 = 0.4;

/// The hint for the active tool and camera preset.
pub(crate) fn hint(mode: Mode, tool: Tool, nav: NavPreset) -> String {
    let tool_hint = match tool {
        Tool::Select => "Click selects, Shift+click adds, a selection box picks several",
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
        NavPreset::Blockbench => "Left drag orbits, right drag pans, wheel zooms",
        NavPreset::Cubik => "Middle drag or Space pans, with Alt or Ctrl it orbits, wheel zooms",
    };
    if mode == Mode::Display {
        return format!("Edit the slot in the properties panel. {camera}");
    }
    if mode == Mode::Animate && tool != Tool::Select {
        return format!("Drag a handle to key the posed bone at the playhead. {camera}");
    }
    format!("{tool_hint}. {camera}")
}

/// What the bar says about the window itself.
pub(crate) struct BarState {
    /// The pointer is over a viewport.
    pub view_hovered: bool,
}

/// Draws the bar into a bottom panel.
pub(crate) fn show(
    ui: &mut Ui,
    cx: &UiCx<'_>,
    mode: Mode,
    tool: Tool,
    prefs: &Prefs,
    state: &BarState,
    sink: &mut Sink,
) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let inner = rect.shrink2(vec2(gutter::EDGE, 0.0));
    let child = |ui: &mut Ui, area: Rect, layout: Layout| {
        ui.new_child(UiBuilder::new().max_rect(area).layout(layout))
    };

    // Counts and the version, right to left.
    let mut right = child(ui, inner, Layout::right_to_left(Align::Center));
    right.spacing_mut().item_spacing.x = space::MD;
    right.add(Label::new(text(Type::Caption, version())).selectable(false));
    if let Some(doc) = cx.doc.as_ref() {
        right.add(
            Label::new(text(
                Type::Caption,
                format!("{} selected", doc.selection.len()),
            ))
            .selectable(false),
        );
        right.add(
            Label::new(text(Type::Caption, format!("{} objects", doc.nodes.len())))
                .selectable(false),
        );
        if let Some(step) = prefs.snap.step() {
            right.add(
                Label::new(text(
                    Type::Caption,
                    format!("Snap {}", format_number(step, 4)),
                ))
                .selectable(false),
            );
        }
    }
    let taken = right.min_rect().width();

    // Job progress, centred.
    if !cx.jobs.is_empty() {
        let width = (inner.width() * JOBS_SHARE).max(PROGRESS_WIDTH);
        let area = Rect::from_center_size(inner.center(), vec2(width, inner.height()));
        let mut middle = child(ui, area, Layout::right_to_left(Align::Center));
        middle.spacing_mut().item_spacing.x = space::SM;
        for job in cx.jobs {
            if job.cancellable && middle.add(IconButton::new(Icon::Close, "Cancel")).clicked() {
                sink.push(UiRequest::CancelJob(job.id));
            }
            progress(&mut middle, job.fraction, PROGRESS_WIDTH);
            middle.add(
                Label::new(text(Type::Caption, &job.label))
                    .truncate()
                    .selectable(false),
            );
        }
    }

    // The message or the hint, left to right, up to the counts.
    let message = cx.doc.as_ref().map_or("Ready", |d| d.status);
    let shown = if !message.is_empty() {
        message.to_owned()
    } else if cx.doc.is_some() && state.view_hovered {
        hint(mode, tool, prefs.nav)
    } else {
        String::new()
    };
    if !shown.is_empty() {
        let jobs_left = if cx.jobs.is_empty() {
            inner.right() - taken
        } else {
            inner.center().x - (inner.width() * JOBS_SHARE).max(PROGRESS_WIDTH) / 2.0
        };
        let area = Rect::from_min_max(inner.min, egui::pos2(jobs_left - space::MD, inner.max.y));
        let mut left = child(ui, area, Layout::left_to_right(Align::Center));
        left.add(
            Label::new(text(Type::Caption, shown))
                .truncate()
                .selectable(false),
        );
    }
}

/// The product name and version.
pub(crate) fn version() -> String {
    format!("cuboid {}", env!("CARGO_PKG_VERSION"))
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
                let cubik = hint(mode, *tool, NavPreset::Cubik);
                assert!(blender.contains("Middle drag orbits"), "{tool:?}");
                assert!(blockbench.contains("Left drag orbits"), "{tool:?}");
                assert!(cubik.contains("Space pans"), "{tool:?}");
            }
        }
        assert!(hint(Mode::Display, Tool::Select, NavPreset::Blender).contains("properties"));
    }
}
