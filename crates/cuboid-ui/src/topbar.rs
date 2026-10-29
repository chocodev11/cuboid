//! The top bar: the menus, the mode tabs, the preset indicator and the command search. In a window
//! the system draws no title bar for, it is the title bar too: the document name, the window
//! buttons and empty space that moves the window (spec: title bar 2).

use std::path::PathBuf;

use cuboid_actions::ActionId;
use cuboid_ui_kit::metrics::{gutter, height, space};
use cuboid_ui_kit::{
    Button, Icon, IconButton, MenuItem, Tokens, Type, WindowButton, command_field, menu_bar,
    menu_button, submenu, tab_button, text,
};
use egui::{Align, Label, Layout, PointerButton, Response, Sense, Ui, vec2};

use crate::common::{ImageCache, logo};
use crate::keymap::{Command, Keymap, UiCommand};
use crate::labels::command_label;
use crate::layout::{Layout as PanelLayout, PanelId, ViewLayout, not_used_in};
use crate::model::{DocView, UiCx};
use crate::prefs::Prefs;
use crate::project_settings::{ProjectChange, ProjectView, Shading};
use crate::request::{Mode, ViewPreset, WindowRequest};

/// Width of the command search while the bar has the room.
const SEARCH_WIDTH: f32 = 200.0;
/// The command search shows as an icon below this width.
const SEARCH_MIN: f32 = 120.0;
/// Below this window width the command search shows no words (ui_v2_spec 11).
const COMPACT_WINDOW: f32 = 1024.0;
/// The document name shows when the free space of the bar is at least this wide.
const NAME_MIN: f32 = 96.0;
/// Edge of the logo in the bar (logo.md 5).
const LOGO: f32 = 20.0;
/// The logo of an inactive window is drawn at this opacity, next to the word in the weak colour.
const LOGO_INACTIVE: f32 = 0.6;
/// Below this window width the word goes and the logo stays; the document name went before it.
/// With a document open the word, the menus and the mode tabs reach about 728 points from the left
/// edge, and the search icon with the window buttons start 170 from the right: this width keeps
/// the gap `space::SM` between the two, and the minimum window of 900 points has room to spare.
const WORD_MIN: f32 = 906.0;

/// Something the user chose in the bar.
pub(crate) enum MenuEvent {
    /// Run a command.
    Run(Command),
    /// Ask the window for something (the drag area and the window buttons).
    Window(WindowRequest),
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
        let enabled = extra_enabled && command.runnable(self.cx, self.mode);
        let shortcut = self.shortcut(command);
        let mut item = MenuItem::new(label);
        if let Some(shortcut) = &shortcut {
            item = item.shortcut(shortcut);
        }
        if ui.add_enabled(enabled, item).clicked() {
            self.events.push(MenuEvent::Run(command));
            ui.close();
        }
    }

    fn checked(&mut self, ui: &mut Ui, label: &str, on: bool, command: Command) {
        let shortcut = self.shortcut(command);
        let mut item = MenuItem::new(label).checked(on);
        if let Some(shortcut) = &shortcut {
            item = item.shortcut(shortcut);
        }
        // Only a panel of another mode is turned down here; the tip text is built while it shows.
        let enabled = command.runnable(self.cx, self.mode);
        let response = ui.add_enabled(enabled, item);
        if !enabled {
            response.on_disabled_hover_ui(|ui| {
                ui.label(not_used_in(self.mode));
            });
        } else if response.clicked() {
            self.events.push(MenuEvent::Run(command));
            ui.close();
        }
    }

    /// One interaction over the whole bar. It is added before the widgets, so they come on top of
    /// it: a drag that starts on empty space moves the window, a double-click maximizes it or
    /// restores it.
    fn drag_area(&mut self, ui: &Ui) {
        let id = ui.scope_id().with("title_bar");
        let area = ui.interact(ui.max_rect(), id, Sense::click_and_drag());
        if area.double_clicked() {
            self.events
                .push(MenuEvent::Window(WindowRequest::ToggleMaximize));
        } else if area.drag_started_by(PointerButton::Primary) && pressed_on_empty_space(&area) {
            self.events.push(MenuEvent::Window(WindowRequest::Move));
        }
    }

    /// Close, Maximize or Restore and Minimize, from the right edge inwards. They sit flush with
    /// the edge and with each other; the gap to the widget on their left stays.
    fn window_buttons(&mut self, ui: &mut Ui) {
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;
        if ui
            .add(WindowButton::new(Icon::Close, "Close").close())
            .clicked()
        {
            self.events
                .push(MenuEvent::Run(Command::Action(ActionId::Exit)));
        }
        let (icon, name) = if self.cx.window.maximized {
            (Icon::Restore, "Restore")
        } else {
            (Icon::Maximize, "Maximize")
        };
        if ui.add(WindowButton::new(icon, name)).clicked() {
            self.events
                .push(MenuEvent::Window(WindowRequest::ToggleMaximize));
        }
        ui.spacing_mut().item_spacing.x = gap;
        if ui.add(WindowButton::new(Icon::Minus, "Minimize")).clicked() {
            self.events.push(MenuEvent::Window(WindowRequest::Minimize));
        }
    }
}

/// Whether the press that started a drag of the bar fell where no widget senses a click or a
/// drag. egui gives a drag to the interaction behind a button as soon as the pointer has moved, so
/// a press on a menu, a tab, a button or the search is told apart by where it began.
fn pressed_on_empty_space(area: &Response) -> bool {
    let Some(origin) = area.ctx.input(|input| input.pointer.press_origin()) else {
        return false;
    };
    area.ctx.viewport(|viewport| {
        viewport
            .prev_pass
            .widgets
            .get_layer(area.layer_id)
            .filter(|widget| widget.id != area.id)
            .filter(|widget| widget.sense.senses_click() || widget.sense.senses_drag())
            .all(|widget| !widget.interact_rect.contains(origin))
    })
}

/// The document name with the marker of unsaved changes, as the bar and the window title show it.
pub(crate) fn document_label(doc: &DocView<'_>) -> String {
    if doc.dirty {
        format!("* {}", doc.title)
    } else {
        doc.title.to_owned()
    }
}

/// Draws the bar into `ui` (a top panel) and returns what was chosen.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &mut Ui,
    keymap: &Keymap,
    mode: Mode,
    cx: &UiCx<'_>,
    prefs: &Prefs,
    project: &ProjectView,
    images: &mut ImageCache,
    window: f32,
) -> Vec<MenuEvent> {
    let mut bar = Bar {
        keymap,
        mode,
        cx,
        project,
        events: Vec::new(),
    };
    let doc = cx.doc.as_ref();
    let tokens = Tokens::of(ui.ctx());
    // Without the system frame (and outside full screen) the bar is the title bar.
    let own_frame = !cx.window.decorated && !cx.window.fullscreen;
    // An inactive window shows the logo faded, and the word and the document name in the weak
    // colour.
    let (brand_ink, name_ink, logo_opacity) = if cx.window.focused {
        (Type::Strong.color(&tokens), tokens.text, 1.0)
    } else {
        (tokens.text_weak, tokens.text_weak, LOGO_INACTIVE)
    };
    if own_frame {
        bar.drag_area(ui);
    }
    ui.spacing_mut().interact_size.y = height::ACTION;
    menu_bar(ui, |ui| {
        ui.add_space(gutter::EDGE);
        // The logo is no button: a drag on it moves the window like the empty bar.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            logo(ui, images, LOGO, logo_opacity);
            if window >= WORD_MIN {
                ui.label(text(Type::Strong, "cuboid").color(brand_ink));
            }
        });
        ui.add_space(space::SM);
        menu_button(ui, "File", |ui| file_menu(ui, &mut bar));
        if doc.is_some() {
            menu_button(ui, "Edit", |ui| edit_menu(ui, &mut bar));
        }
        menu_button(ui, "View", |ui| {
            view_menu(ui, &mut bar, &prefs.layout, prefs.view_layout);
        });
        if doc.is_some() {
            menu_button(ui, "Add", |ui| add_menu(ui, &mut bar));
            menu_button(ui, "Tools", |ui| tools_menu(ui, &mut bar));
        }
        menu_button(ui, "Help", |ui| help_menu(ui, &mut bar));

        if doc.is_some() {
            ui.add_space(gutter::GROUP);
            ui.spacing_mut().item_spacing.x = 0.0;
            for m in Mode::ALL {
                if tab_button(ui, m.name(), m == mode).clicked() && m != mode {
                    bar.events.push(MenuEvent::Mode(m));
                }
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            if own_frame {
                bar.window_buttons(ui);
            } else {
                ui.add_space(gutter::EDGE);
            }
            let preset = doc.is_some().then(|| project.label());
            let preset_full = preset.as_ref().map_or(0.0, |label| {
                Button::new(label)
                    .ghost()
                    .icon(Icon::Sliders)
                    .natural_width(ui)
            });
            // The search keeps its icon at least. Short of room the preset gives up its label,
            // and then its place: the File menu still opens the project settings.
            let after_search = ui.available_width() - height::CONTROL - space::SM;
            let preset_width = if after_search >= preset_full {
                preset_full
            } else if after_search >= height::CONTROL {
                height::CONTROL
            } else {
                0.0
            };
            let room = ui.available_width() - preset_width - space::SM;
            let hint = keymap
                .shortcut_of(Command::Ui(UiCommand::Palette), mode)
                .map(|chord| chord.to_string());
            if room >= SEARCH_MIN {
                // A narrow window keeps the magnifier and the chord only.
                let width = (window >= COMPACT_WINDOW).then(|| room.min(SEARCH_WIDTH));
                let open = command_field(ui, "Search commands", hint.as_deref(), width);
                if open.clicked() {
                    bar.events
                        .push(MenuEvent::Run(Command::Ui(UiCommand::Palette)));
                }
            } else if ui
                .add(IconButton::new(Icon::Search, "Search commands"))
                .clicked()
            {
                bar.events
                    .push(MenuEvent::Run(Command::Ui(UiCommand::Palette)));
            }
            if let Some(label) = preset.filter(|_| preset_width > 0.0) {
                let opened = if preset_width < preset_full {
                    ui.add(IconButton::new(Icon::Sliders, "Project settings"))
                } else {
                    let button = Button::new(&label).ghost().icon(Icon::Sliders);
                    ui.add(button).on_hover_text("Project settings")
                };
                if opened.clicked() {
                    bar.events.push(MenuEvent::ProjectSettings);
                }
            }
            // The document name goes in the middle of what is left; a narrow bar drops it.
            if own_frame && let Some(doc) = doc {
                let free = ui
                    .available_rect_before_wrap()
                    .shrink2(vec2(space::SM, 0.0));
                if free.width() >= NAME_MIN {
                    let name = text(Type::Body, document_label(doc)).color(name_ink);
                    ui.put(free, Label::new(name).truncate());
                }
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
    submenu(ui, "Open recent", |ui| {
        if bar.cx.recent.is_empty() {
            ui.add_enabled(false, MenuItem::new("No recent files"));
        }
        for entry in bar.cx.recent.iter().take(10) {
            let row = ui.add(MenuItem::new(&entry.name));
            if row
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
        .add_enabled(has_doc, MenuItem::new("Project settings..."))
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

fn add_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    bar.action(ui, ActionId::CreateElementCube);
    submenu(ui, "Planes", |ui| {
        for id in [
            ActionId::CreateElementPlaneX,
            ActionId::CreateElementPlaneY,
            ActionId::CreateElementPlaneZ,
        ] {
            bar.action(ui, id);
        }
    });
    submenu(ui, "Primitives", |ui| {
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

fn tools_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    for id in [
        ActionId::ToggleVisibility,
        ActionId::ToggleLock,
        ActionId::FocusLock,
    ] {
        bar.action(ui, id);
    }
    ui.separator();
    submenu(ui, "Transform", |ui| {
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
    submenu(ui, "Element", |ui| {
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
    submenu(ui, "Voxels", |ui| {
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
    submenu(ui, "Recover", |ui| {
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
    submenu(ui, "Camera", |ui| {
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
    submenu(ui, "Shading", |ui| shading_menu(ui, bar));
    ui.separator();
    submenu(ui, "Views", |ui| {
        for l in ViewLayout::ALL {
            bar.checked(
                ui,
                l.name(),
                l == views,
                Command::Ui(UiCommand::SetViewLayout(l)),
            );
        }
    });
    submenu(ui, "Panels", |ui| {
        for panel in PanelId::ALL {
            bar.checked(
                ui,
                panel.title(),
                layout.is_shown(panel, bar.mode),
                Command::Ui(UiCommand::TogglePanel(panel)),
            );
        }
        ui.separator();
        bar.item(ui, Command::Ui(UiCommand::ResetLayout));
    });
    submenu(ui, "Background images", |ui| {
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

/// The lighting of the views: one mode, plus the options that go with them. Also the shading
/// menu of the viewport headers.
pub(crate) fn shading_items(ui: &mut Ui, project: &ProjectView) -> Vec<ProjectChange> {
    let mut changes = Vec::new();
    for mode in Shading::ALL {
        let on = project.shading == mode;
        if ui.add(MenuItem::new(mode.name()).checked(on)).clicked() {
            if !on {
                changes.push(ProjectChange::SetShading(mode));
            }
            ui.close();
        }
    }
    ui.separator();
    let smooth = project.smooth_light;
    let block = project.shading == Shading::MinecraftBlock;
    let item = MenuItem::new("Smooth block light").checked(smooth);
    if ui.add_enabled(block, item).clicked() {
        changes.push(ProjectChange::SetSmoothLight(!smooth));
        ui.close();
    }
    let ao = project.voxel_ao;
    if ui
        .add(MenuItem::new("Voxel ambient occlusion").checked(ao))
        .clicked()
    {
        changes.push(ProjectChange::SetVoxelAo(!ao));
        ui.close();
    }
    changes
}

fn shading_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    let changes = shading_items(ui, bar.project);
    bar.events
        .extend(changes.into_iter().map(MenuEvent::Project));
}

fn help_menu(ui: &mut Ui, bar: &mut Bar<'_>) {
    bar.item(ui, Command::Ui(UiCommand::Palette));
    bar.item(ui, Command::Ui(UiCommand::KeyboardShortcuts));
    for id in [ActionId::About, ActionId::Changelog, ActionId::ProjectSite] {
        bar.action(ui, id);
    }
}
