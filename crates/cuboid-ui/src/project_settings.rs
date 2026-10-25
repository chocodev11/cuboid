//! The preset of the open document as the UI shows it: the project settings window, the data
//! behind the shading menu and the label of the status bar. The app owns the settings and
//! applies the [`ProjectChange`]s that come back.

use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    Modal, ModalButton, PropertyGrid, Section, Type, dropdown, segmented, text, toggle,
};
use egui::{Context, Label, Ui};

use crate::common::Sink;
use crate::dialogs::fixed_body;
use crate::request::UiRequest;

/// Height of the body: the rows of the Minecraft preset and the notes under them.
const BODY: f32 = height::ROW * 8.0;

/// The family of preset a document uses (roadmap_lean 6.1).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum PresetKind {
    /// Cubik's rules, lighting and units.
    #[default]
    Cubik,
    /// Minecraft Java rules for a game version.
    Minecraft,
}

impl PresetKind {
    /// Both kinds in window order.
    pub const ALL: [PresetKind; 2] = [PresetKind::Cubik, PresetKind::Minecraft];

    /// Label of the kind.
    pub fn name(self) -> &'static str {
        match self {
            PresetKind::Cubik => "Cubik",
            PresetKind::Minecraft => "Minecraft",
        }
    }
}

/// How faces are lit in the views; the View > Shading menu.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum Shading {
    /// Cubik's look.
    #[default]
    Cubik,
    /// A Minecraft block placed in the world.
    MinecraftBlock,
    /// A Minecraft item or entity.
    MinecraftItem,
    /// No lighting.
    Flat,
}

impl Shading {
    /// All modes in menu order.
    pub const ALL: [Shading; 4] = [
        Shading::Cubik,
        Shading::MinecraftBlock,
        Shading::MinecraftItem,
        Shading::Flat,
    ];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            Shading::Cubik => "Cubik",
            Shading::MinecraftBlock => "Minecraft block",
            Shading::MinecraftItem => "Minecraft item",
            Shading::Flat => "Flat",
        }
    }
}

/// The settings of the open document, described by the app each frame with [`crate::Ui::set_project`].
#[derive(Clone, Debug)]
pub struct ProjectView {
    /// The preset kind.
    pub preset: PresetKind,
    /// The chosen game version of the Minecraft preset, such as `1.21.11`; empty for Cubik.
    pub version: String,
    /// The game versions to choose from, newest first.
    pub versions: &'static [String],
    /// Rotations are restricted to one axis in 22.5 degree steps.
    pub restrict_rotations: bool,
    /// Sizes may cross zero (inverted cubes).
    pub allow_negative_size: bool,
    /// How the views are lit.
    pub shading: Shading,
    /// Minecraft block shading lights by the normal continuously.
    pub smooth_light: bool,
    /// Voxel corners are darkened by ambient occlusion.
    pub voxel_ao: bool,
    /// What the validator checks the model for, such as `Java block 1.21.11`.
    pub validation: String,
    /// One line on what the preset does when the model is saved or exported.
    pub export_note: &'static str,
}

impl Default for ProjectView {
    fn default() -> Self {
        Self {
            preset: PresetKind::Cubik,
            version: String::new(),
            versions: &[],
            restrict_rotations: true,
            allow_negative_size: false,
            shading: Shading::Cubik,
            smooth_light: false,
            voxel_ao: true,
            validation: String::new(),
            export_note: "",
        }
    }
}

impl ProjectView {
    /// The name for the status bar: `Cubik` or `Minecraft 1.21.11`.
    pub fn label(&self) -> String {
        match self.preset {
            PresetKind::Cubik => PresetKind::Cubik.name().to_owned(),
            PresetKind::Minecraft if self.version.is_empty() => {
                PresetKind::Minecraft.name().to_owned()
            }
            PresetKind::Minecraft => format!("Minecraft {}", self.version),
        }
    }
}

/// A change of the project settings that the user asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectChange {
    /// Use this preset and take its defaults (also resets the overrides).
    SetPreset(PresetKind),
    /// Use this game version, given as text such as `1.21.11`.
    SetVersion(String),
    /// Override the restriction of rotations for this document.
    SetRestrictRotations(bool),
    /// Override whether sizes may cross zero for this document.
    SetAllowNegativeSize(bool),
    /// Light the views this way.
    SetShading(Shading),
    /// Light Minecraft blocks by the normal continuously.
    SetSmoothLight(bool),
    /// Darken voxel corners by ambient occlusion.
    SetVoxelAo(bool),
}

/// State of the project settings window.
#[derive(Default)]
pub(crate) struct ProjectWindow {
    open: bool,
}

impl ProjectWindow {
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Draws the dialog while it is open. Enter and Esc close it; the changes are already sent.
    pub fn show(&mut self, ctx: &Context, project: &ProjectView, sink: &mut Sink) {
        if !self.open {
            return;
        }
        let buttons = [
            ModalButton::new("Use the preset defaults"),
            ModalButton::primary("Done"),
        ];
        let outcome = Modal::new("project_settings", "Project settings")
            .width(520.0)
            .buttons(&buttons)
            .show(ctx, |ui| {
                fixed_body(ui, "project_settings_body", BODY, |ui| {
                    body(ui, project, sink);
                });
            });
        if outcome.button == Some(0) {
            change(sink, ProjectChange::SetPreset(project.preset));
        } else if outcome.button == Some(1) || outcome.confirmed || outcome.cancelled {
            self.open = false;
        }
    }
}

fn change(sink: &mut Sink, change: ProjectChange) {
    sink.push(UiRequest::Project(change));
}

fn body(ui: &mut Ui, project: &ProjectView, sink: &mut Sink) {
    PropertyGrid::new("project_preset").show(ui, |grid| {
        grid.row("Preset", |ui| {
            let mut kind = PresetKind::ALL
                .iter()
                .position(|k| *k == project.preset)
                .unwrap_or(0);
            let names = PresetKind::ALL.map(PresetKind::name);
            if segmented(ui, &mut kind, &names) {
                change(sink, ProjectChange::SetPreset(PresetKind::ALL[kind.min(1)]));
            }
        });
        if project.preset == PresetKind::Minecraft {
            grid.row("Java version", |ui| version_choice(ui, project, sink));
        }
    });
    ui.add_space(space::MD);
    Section::new("This document").show(ui, "project_document", |ui| {
        PropertyGrid::new("project_document").show(ui, |grid| {
            grid.row("Rotations", |ui| {
                let mut restrict = project.restrict_rotations;
                let label = "Restrict rotations to one axis and 22.5 degree steps";
                if toggle(ui, &mut restrict, label).changed() {
                    change(sink, ProjectChange::SetRestrictRotations(restrict));
                }
            });
            grid.row("Sizes", |ui| {
                let mut negative = project.allow_negative_size;
                if toggle(ui, &mut negative, "Allow negative sizes (inverted cubes)").changed() {
                    change(sink, ProjectChange::SetAllowNegativeSize(negative));
                }
            });
        });
    });
    if !project.validation.is_empty() {
        let checked = format!("Checked as: {}", project.validation);
        ui.add(Label::new(text(Type::Caption, checked)).wrap());
    }
    if !project.export_note.is_empty() {
        ui.add(Label::new(text(Type::Caption, project.export_note)).wrap());
    }
}

/// The drop-down of game versions. A version the table lacks stays choosable at the top.
fn version_choice(ui: &mut Ui, project: &ProjectView, sink: &mut Sink) {
    let mut options: Vec<&str> = project.versions.iter().map(String::as_str).collect();
    if !project.version.is_empty() && !options.contains(&project.version.as_str()) {
        options.insert(0, project.version.as_str());
    }
    let mut at = options
        .iter()
        .position(|v| *v == project.version)
        .unwrap_or(0);
    if dropdown(ui, "project_version", &mut at, &options).changed()
        && let Some(version) = options.get(at)
    {
        change(sink, ProjectChange::SetVersion((*version).to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_names_the_preset_and_the_version() {
        let mut view = ProjectView::default();
        assert_eq!(view.label(), "Cubik");
        view.preset = PresetKind::Minecraft;
        assert_eq!(view.label(), "Minecraft");
        view.version = "1.21.11".to_owned();
        assert_eq!(view.label(), "Minecraft 1.21.11");
    }
}
