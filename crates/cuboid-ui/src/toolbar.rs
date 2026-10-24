//! The tool row below the menu bar: the tools of the workspace, their options and the panel
//! toggles.

use cuboid_actions::ActionId;
use cuboid_ui_kit::{DragFieldOpts, Icon, Tokens, drag_field, icon_button, tool_button};
use egui::{Align, Layout, Ui};

use crate::keymap::{Command, Keymap, UiCommand};
use crate::layout::{Layout as PanelLayout, Zone};
use crate::request::{Mode, Tool, ToolOptions};

/// What the user did in the row.
pub(crate) enum ToolbarEvent {
    /// Chose a tool.
    Tool(Tool),
    /// Ran a command.
    Run(Command),
    /// Changed the tool options.
    Options(ToolOptions),
    /// Showed or hid a zone.
    ToggleZone(Zone),
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
    options: ToolOptions,
    keymap: &Keymap,
    layout: &PanelLayout,
) -> Vec<ToolbarEvent> {
    let tokens = Tokens::of(ui.ctx());
    let mut events = Vec::new();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for &t in mode.tools() {
            let tip = match keymap.shortcut_of(Command::Ui(UiCommand::SetTool(t)), mode) {
                Some(chord) => format!("{} ({chord})", t.name()),
                None => t.name().to_owned(),
            };
            if tool_button(ui, tool_icon(t), t == tool, &tip).clicked() {
                events.push(ToolbarEvent::Tool(t));
            }
        }
        if !mode.tools().is_empty() {
            ui.separator();
        }
        match mode {
            Mode::Model => {
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
                    if icon_button(ui, icon, tip).clicked() {
                        events.push(ToolbarEvent::Run(command));
                    }
                }
            }
            Mode::Voxel | Mode::Paint => {
                let mut opts = options;
                let mut brush = f32::from(u16::try_from(opts.brush).unwrap_or(1));
                ui.label(egui::RichText::new("Brush").color(tokens.text_weak));
                let field = DragFieldOpts {
                    name: "Brush size",
                    speed: 0.1,
                    decimals: 0,
                    range: 1.0..=64.0,
                    snap: Some(1.0),
                    width: Some(56.0),
                    ..DragFieldOpts::default()
                };
                let r = drag_field(ui, "toolbar_brush", &mut brush, &field);
                if r.committed {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let size = brush.round().max(1.0) as u32;
                    opts.brush = size;
                }
                if mode == Mode::Voxel {
                    ui.separator();
                    ui.label(egui::RichText::new("Mirror").color(tokens.text_weak));
                    for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                        let on = opts.mirror[i];
                        if ui
                            .selectable_label(on, axis)
                            .on_hover_text("Mirror across this axis")
                            .clicked()
                        {
                            opts.mirror[i] = !on;
                        }
                    }
                }
                if opts != options {
                    events.push(ToolbarEvent::Options(opts));
                }
            }
            Mode::Display => {
                ui.label(
                    egui::RichText::new("Pick a slot in the properties panel")
                        .color(tokens.text_weak),
                );
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (icon, zone, tip) in [
                (Icon::PanelBottom, Zone::Bottom, "Bottom panels"),
                (Icon::PanelRight, Zone::Right, "Right panels"),
                (Icon::PanelLeft, Zone::Left, "Left panels"),
            ] {
                let shown = layout.zone(zone).open;
                if tool_button(ui, icon, shown, tip).clicked() {
                    events.push(ToolbarEvent::ToggleZone(zone));
                }
            }
        });
    });
    events
}
