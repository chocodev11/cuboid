//! The tool strip at the left edge: the tools of the workspace, the add buttons of the model
//! workspace, snapping, undo and redo.

use cuboid_actions::ActionId;
use cuboid_ui_kit::{Icon, Toolbar};
use egui::Ui;

use crate::keymap::{Command, Keymap, UiCommand};
use crate::model::UiCx;
use crate::prefs::Snap;
use crate::request::{Mode, Tool};
use crate::topbar::undo_labels;

/// What the user did in the strip.
pub(crate) enum ToolbarEvent {
    /// Chose a tool.
    Tool(Tool),
    /// Ran a command.
    Run(Command),
}

/// The icon of a tool.
pub(crate) fn tool_icon(tool: Tool) -> Icon {
    match tool {
        Tool::Select => Icon::Select,
        Tool::Move => Icon::Move,
        Tool::Rotate => Icon::Rotate,
        Tool::Scale => Icon::Scale,
        Tool::VoxelPencil | Tool::PaintPencil => Icon::Pencil,
        Tool::VoxelErase | Tool::PaintEraser => Icon::Eraser,
        Tool::VoxelFill | Tool::PaintFill => Icon::Fill,
        Tool::VoxelBox => Icon::Rect,
        Tool::VoxelLine => Icon::Line,
        Tool::VoxelPicker | Tool::PaintPicker => Icon::Picker,
    }
}

pub(crate) fn show(
    ui: &mut Ui,
    mode: Mode,
    tool: Tool,
    snap: &Snap,
    keymap: &Keymap,
    cx: &UiCx<'_>,
) -> Vec<ToolbarEvent> {
    let mut events = Vec::new();
    Toolbar::vertical().show(ui, |bar| {
        for &t in mode.tools() {
            let chord = keymap
                .shortcut_of(Command::Ui(UiCommand::SetTool(t)), mode)
                .map(|chord| chord.to_string());
            if bar
                .tool(tool_icon(t), t == tool, t.name(), chord.as_deref())
                .clicked()
            {
                events.push(ToolbarEvent::Tool(t));
            }
        }
        if mode == Mode::Model {
            bar.separator();
            for (icon, tip, command) in [
                (
                    Icon::Cube,
                    "Add cube",
                    Command::Action(ActionId::CreateElementCube),
                ),
                (
                    Icon::Plane,
                    "Add plane",
                    Command::Action(ActionId::CreateElementPlaneY),
                ),
                (
                    Icon::Group,
                    "Group selection",
                    Command::Ui(UiCommand::GroupSelection),
                ),
            ] {
                if bar.button(icon, tip).clicked() {
                    events.push(ToolbarEvent::Run(command));
                }
            }
        }
        if !mode.tools().is_empty() {
            bar.separator();
        }
        if bar.toggle(Icon::Magnet, snap.enabled, "Snapping").clicked() {
            events.push(ToolbarEvent::Run(Command::Ui(UiCommand::ToggleSnap)));
        }
        let (undo, redo) = undo_labels(cx);
        for (icon, label, name, id) in [
            (Icon::Undo, undo, "Undo", ActionId::Undo),
            (Icon::Redo, redo, "Redo", ActionId::Redo),
        ] {
            let enabled = label.is_some() && cx.can_run(id);
            let tip = label.map_or_else(|| name.to_owned(), |l| format!("{name} {l}"));
            let response = bar.ui().add_enabled_ui(enabled, |ui| {
                ui.add(cuboid_ui_kit::IconButton::new(icon, &tip).toolbar())
            });
            if response.inner.clicked() {
                events.push(ToolbarEvent::Run(Command::Action(id)));
            }
        }
    });
    events
}
