//! The menu bar: menus, mode tabs, undo and redo, snapping and the palette button.

use std::path::PathBuf;

use cuboid_actions::ActionId;
use cuboid_ui_kit::{Icon, Tokens, icon_button, key_chip, tab_button, tool_button};
use egui::{Align, Layout, MenuBar, Ui};

use crate::keymap::{Command, Keymap, UiCommand};
use crate::labels::command_label;
use crate::layout::{Layout as PanelLayout, PanelId, ViewLayout};
use crate::model::UiCx;
use crate::prefs::Prefs;
use crate::project_settings::{ProjectChange, ProjectView, Shading};
use crate::request::{Mode, ViewPreset};

/// Something the user chose in the bar.
pub(crate) enum MenuEvent {
    /// Run a command.
    Run(Command),
    /// Open a file from the recent list.
    OpenRecent(PathBuf),
    /// Switch workspace.
    Mode(Mode),
    /// Change a project setting (View > Shading).
    Project(ProjectChange),
    /// Open the project settings window.
    ProjectSettings,
}

struct Bar<'a> {
    keymap: &'a Keymap,
    mode: Mode,
    cx: &'a UiCx<'a>,
    project: &'a ProjectView,
    events: Vec<MenuEvent>,
}

impl Bar<'_> {
    fn enabled(&self, command: Command) -> bool {
        match command {
            Command::Action(id) => self.cx.can_run(id),
            Command::Ui(_) => true,
        }
    }

    fn shortcut(&self, command: Command) -> Option<String> {
        self.keymap
            .shortcut_of(command, self.mode)
            .map(|c| c.to_string())
    }

    /// A row with the standard label of the command.
    fn item(&mut self, ui: &mut Ui, command: Command) {
        self.labeled(ui, &command_label(command), command, true);
    }

    fn action(&mut self, ui: &mut Ui, id: ActionId) {
        self.item(ui, Command::Action(id));
    }

    fn labeled(&mut self, ui: &mut Ui, label: &str, command: Command, extra_enabled: bool) {
        let enabled = extra_enabled && self.enabled(command);
        let mut button = egui::Button::new(label);
        if let Some(sc) = self.shortcut(command) {
            button = button.shortcut_text(sc);
        }
        if ui.add_enabled(enabled, button).clicked() {
            self.events.push(MenuEvent::Run(command));
            ui.close();
        }
    }

    fn checked(&mut self, ui: &mut Ui, label: &str, on: bool, command: Command) {
        let mut button = egui::Button::selectable(on, label);
        if let Some(sc) = self.shortcut(command) {
            button = button.shortcut_text(sc);
        }
        if ui.add(button).clicked() {
            self.events.push(MenuEvent::Run(command));
            ui.close();
        }
    }
}

/// Draws the bar into `ui` (a top panel) and returns what was chosen.
pub(crate) fn show(
    ui: &mut Ui,
    keymap: &Keymap,
    mode: Mode,
    cx: &UiCx<'_>,
    prefs: &Prefs,
    project: &ProjectView,
    palette_open: bool,
) -> Vec<MenuEvent> {
    let mut bar = Bar {
        keymap,
        mode,
        cx,
        project,
        events: Vec::new(),
    };
    let tokens = Tokens::of(ui.ctx());
    let doc = cx.doc.as_ref();
    MenuBar::new().ui(ui, |ui| {
        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
        ui.menu_button("File", |ui| file_menu(ui, &mut bar));
        ui.add_enabled_ui(doc.is_some(), |ui| {
            ui.menu_button("Edit", |ui| edit_menu(ui, &mut bar));
            ui.menu_button("Create", |ui| create_menu(ui, &mut bar));
            ui.menu_button("Object", |ui| object_menu(ui, &mut bar));
        });
        ui.menu_button("View", |ui| {
            view_menu(ui, &mut bar, &prefs.layout, prefs.view_layout)
        });
        ui.menu_button("Help", |ui| help_menu(ui, &mut bar));

        if doc.is_some() {
            ui.add_space(12.0);
            let mut chosen = None;
            for m in Mode::ALL {
                if tab_button(ui, m.name(), m == mode).clicked() && m != mode {
                    chosen = Some(m);
                }
            }
            if let Some(m) = chosen {
                bar.events.push(MenuEvent::Mode(m));
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let hint = bar
                .keymap
                .shortcut_of(Command::Ui(UiCommand::Palette), mode);
            let label = if palette_open {
                "Searching commands"
            } else {
                "Search commands"
            };
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(chord) = hint {
                key_chip(ui, &chord.to_string());
            }
            let resp = ui.add(
                egui::Button::new(egui::RichText::new(label).color(tokens.text_weak))
                    .min_size(egui::vec2(152.0, 24.0)),
            );
            if resp.clicked() {
                bar.events
                    .push(MenuEvent::Run(Command::Ui(UiCommand::Palette)));
            }
            if doc.is_some() {
                ui.add_space(8.0);
                let snap = tool_button(ui, Icon::Magnet, prefs.snap.enabled, "Snapping");
                if snap.clicked() {
                    bar.events
                        .push(MenuEvent::Run(Command::Ui(UiCommand::ToggleSnap)));
                }
                let (undo_label, redo_label) = undo_labels(cx);
                ui.add_enabled_ui(redo_label.is_some() && cx.can_run(ActionId::Redo), |ui| {
                    let tip = redo_label
                        .as_deref()
                        .map_or("Redo".to_owned(), |l| format!("Redo {l}"));
                    if icon_button(ui, Icon::Redo, &tip).clicked() {
                        bar.events
                            .push(MenuEvent::Run(Command::Action(ActionId::Redo)));
                    }
                });
                ui.add_enabled_ui(undo_label.is_some() && cx.can_run(ActionId::Undo), |ui| {
                    let tip = undo_label
                        .as_deref()
                        .map_or("Undo".to_owned(), |l| format!("Undo {l}"));
                    if icon_button(ui, Icon::Undo, &tip).clicked() {
                        bar.events
                            .push(MenuEvent::Run(Command::Action(ActionId::Undo)));
                    }
                });
            }
        });
    });
    bar.events
}

/// Labels of the entries undo and redo would apply.
pub(crate) fn undo_labels(cx: &UiCx<'_>) -> (Option<String>, Option<String>) {
    let Some(doc) = cx.doc.as_ref() else {
        return (None, None);
    };
    let h = doc.history;
    let applied = h.cursor.min(h.labels.len());
    let undo = applied
        .checked_sub(1)
        .and_then(|i| h.labels.get(i))
        .map(|l| l.to_string());
    let redo = h.labels.get(applied).map(|l| l.to_string());
    (undo, redo)
}

fn file_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    bar.action(ui, ActionId::NewFile);
    bar.action(ui, ActionId::Open);
    ui.menu_button("Open recent", |ui| {
        if bar.cx.recent.is_empty() {
            ui.add_enabled(false, egui::Button::new("No recent files"));
        }
        for entry in bar.cx.recent.iter().take(10) {
            if ui
                .button(&entry.name)
                .on_hover_text(entry.path.display().to_string())
                .clicked()
            {
                bar.events.push(MenuEvent::OpenRecent(entry.path.clone()));
                ui.close();
            }
        }
        if !bar.cx.recent.is_empty() {
            ui.separator();
            bar.labeled(
                ui,
                "Clear list",
                Command::Action(ActionId::ClearRecentFiles),
                true,
            );
        }
    });
    bar.action(ui, ActionId::Merge);
    ui.separator();
    let has_doc = bar.cx.doc.is_some();
    bar.labeled(ui, "Save", Command::Action(ActionId::Save), has_doc);
    bar.labeled(ui, "Save as...", Command::Action(ActionId::SaveAs), has_doc);
    ui.separator();
    bar.action(ui, ActionId::Import);
    bar.labeled(ui, "Export...", Command::Action(ActionId::Export), has_doc);
    ui.separator();
    if ui
        .add_enabled(has_doc, egui::Button::new("Project settings..."))
        .clicked()
    {
        bar.events.push(MenuEvent::ProjectSettings);
        ui.close();
    }
    bar.item(ui, Command::Ui(UiCommand::Preferences));
    ui.separator();
    bar.action(ui, ActionId::Exit);
}

fn edit_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    let (undo, redo) = undo_labels(bar.cx);
    let label = undo.map_or("Undo".to_owned(), |l| format!("Undo {l}"));
    bar.labeled(
        ui,
        &label,
        Command::Action(ActionId::Undo),
        bar.cx.doc.as_ref().is_some_and(|d| d.history.cursor > 0),
    );
    let label = redo
        .clone()
        .map_or("Redo".to_owned(), |l| format!("Redo {l}"));
    bar.labeled(ui, &label, Command::Action(ActionId::Redo), redo.is_some());
    ui.separator();
    for id in [
        ActionId::Cut,
        ActionId::Copy,
        ActionId::Paste,
        ActionId::Clone,
        ActionId::Delete,
    ] {
        bar.action(ui, id);
    }
    ui.separator();
    for id in [
        ActionId::SelectAllElements,
        ActionId::SelectNone,
        ActionId::SelectInverse,
        ActionId::SelectName,
    ] {
        bar.action(ui, id);
    }
    ui.separator();
    bar.action(ui, ActionId::Rename);
    bar.item(ui, Command::Ui(UiCommand::GroupSelection));
    bar.item(ui, Command::Ui(UiCommand::Ungroup));
    ui.separator();
    bar.action(ui, ActionId::ModelProperties);
    bar.action(ui, ActionId::ModelOrigin);
    bar.action(ui, ActionId::ModelBlueprint);
}

fn create_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    bar.action(ui, ActionId::CreateElementCube);
    ui.menu_button("Planes", |ui| {
        for id in [
            ActionId::CreateElementPlaneX,
            ActionId::CreateElementPlaneY,
            ActionId::CreateElementPlaneZ,
        ] {
            bar.action(ui, id);
        }
    });
    ui.menu_button("Primitives", |ui| {
        for id in [
            ActionId::CreateMeshCube,
            ActionId::CreateMeshSphere,
            ActionId::CreateMeshCylinder,
            ActionId::CreateMeshCone,
            ActionId::CreateMeshPyramid,
            ActionId::CreateMeshSlope,
            ActionId::CreateMeshCornerIn,
            ActionId::CreateMeshCornerOut,
            ActionId::CreateMeshTorus,
        ] {
            bar.action(ui, id);
        }
    });
}

fn object_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    for id in [
        ActionId::ToggleVisibility,
        ActionId::ToggleLock,
        ActionId::FocusLock,
    ] {
        bar.action(ui, id);
    }
    ui.separator();
    ui.menu_button("Transform", |ui| {
        for id in [
            ActionId::SetPosition,
            ActionId::SetSize,
            ActionId::Move,
            ActionId::Scale,
            ActionId::Randomize,
            ActionId::Inflate,
            ActionId::ApplyRescale,
            ActionId::ResetRotation,
            ActionId::RotateX,
            ActionId::RotateY,
            ActionId::RotateZ,
            ActionId::CenterX,
            ActionId::CenterY,
            ActionId::CenterZ,
        ] {
            bar.action(ui, id);
        }
    });
    ui.menu_button("Element", |ui| {
        for id in [
            ActionId::ElementColor,
            ActionId::ElementColorRandom,
            ActionId::ShadeEnable,
            ActionId::ShadeDisable,
            ActionId::UvSnapEnable,
            ActionId::UvSnapDisable,
            ActionId::FacesEnableAll,
            ActionId::FacesDisableAll,
        ] {
            bar.action(ui, id);
        }
    });
    ui.menu_button("Voxels", |ui| {
        for id in [
            ActionId::VoxelSetup,
            ActionId::ConvertToElementsCubes,
            ActionId::ConvertToElements,
            ActionId::ConvertToElementsAtlas,
            ActionId::VoxelUpscale,
            ActionId::VoxelDownscale,
            ActionId::VoxelFlipX,
            ActionId::VoxelFlipY,
            ActionId::VoxelFlipZ,
            ActionId::VoxelRotateX,
            ActionId::VoxelRotateY,
            ActionId::VoxelRotateZ,
            ActionId::VoxelFillVolume,
            ActionId::VoxelHollowVolume,
            ActionId::ClearVoxels,
            ActionId::ResetVoxels,
        ] {
            bar.action(ui, id);
        }
    });
    ui.menu_button("Recover", |ui| {
        for id in [
            ActionId::ShowAll,
            ActionId::UnlockAll,
            ActionId::ResetBlueprint,
            ActionId::ResetCamera,
            ActionId::GetBackFarElements,
        ] {
            bar.action(ui, id);
        }
    });
    ui.separator();
    bar.action(ui, ActionId::Next);
    bar.action(ui, ActionId::Previous);
}

fn view_menu(ui: &mut Ui, bar: &mut Bar<'_>, layout: &PanelLayout, views: ViewLayout) {
    ui.menu_button("Camera", |ui| {
        for preset in [
            ViewPreset::Front,
            ViewPreset::Back,
            ViewPreset::Right,
            ViewPreset::Left,
            ViewPreset::Top,
            ViewPreset::Bottom,
            ViewPreset::ToggleProjection,
        ] {
            bar.item(ui, Command::Ui(UiCommand::View(preset)));
        }
    });
    bar.item(ui, Command::Ui(UiCommand::FrameSelected));
    bar.action(ui, ActionId::ZoomPlus);
    bar.action(ui, ActionId::ZoomMinus);
    bar.action(ui, ActionId::WireframeMode);
    bar.action(ui, ActionId::GridSettings);
    ui.menu_button("Shading", |ui| shading_menu(ui, bar));
    ui.separator();
    ui.menu_button("Views", |ui| {
        for l in ViewLayout::ALL {
            bar.checked(
                ui,
                l.name(),
                l == views,
                Command::Ui(UiCommand::SetViewLayout(l)),
            );
        }
    });
    ui.menu_button("Panels", |ui| {
        for panel in PanelId::ALL {
            let zone = layout.zone_of(panel).map(|z| layout.zone(z));
            let shown = zone.is_some_and(|z| z.open && z.current() == Some(panel));
            bar.checked(
                ui,
                panel.title(),
                shown,
                Command::Ui(UiCommand::TogglePanel(panel)),
            );
        }
        ui.separator();
        bar.item(ui, Command::Ui(UiCommand::ResetLayout));
    });
    ui.menu_button("Background images", |ui| {
        for id in [
            ActionId::LoadBgUp,
            ActionId::LoadBgWest,
            ActionId::LoadBgSouth,
            ActionId::LoadBgAll,
            ActionId::ToggleBgSize,
            ActionId::DeleteBgAll,
        ] {
            bar.action(ui, id);
        }
    });
    ui.separator();
    bar.item(ui, Command::Ui(UiCommand::ToggleTheme));
    bar.item(ui, Command::Ui(UiCommand::ShowStart));
}

/// The lighting of the views: one mode, plus the options that go with them.
fn shading_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    let project = bar.project;
    for mode in Shading::ALL {
        let on = project.shading == mode;
        if ui.add(egui::Button::selectable(on, mode.name())).clicked() {
            if !on {
                bar.events
                    .push(MenuEvent::Project(ProjectChange::SetShading(mode)));
            }
            ui.close();
        }
    }
    ui.separator();
    let smooth = project.smooth_light;
    let block = project.shading == Shading::MinecraftBlock;
    let button = egui::Button::selectable(smooth, "Smooth block light");
    if ui.add_enabled(block, button).clicked() {
        bar.events
            .push(MenuEvent::Project(ProjectChange::SetSmoothLight(!smooth)));
        ui.close();
    }
    let ao = project.voxel_ao;
    if ui
        .add(egui::Button::selectable(ao, "Voxel ambient occlusion"))
        .clicked()
    {
        bar.events
            .push(MenuEvent::Project(ProjectChange::SetVoxelAo(!ao)));
        ui.close();
    }
}

fn help_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    bar.item(ui, Command::Ui(UiCommand::Palette));
    bar.item(ui, Command::Ui(UiCommand::KeyboardShortcuts));
    for id in [ActionId::About, ActionId::Changelog, ActionId::ProjectSite] {
        bar.action(ui, id);
    }
}
