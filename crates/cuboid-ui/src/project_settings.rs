//! The preset of the open document as the UI shows it: the project settings window, the data
//! behind the shading menu and the label of the status bar. The app owns the settings and
//! applies the [`ProjectChange`]s that come back.

use cuboid_ui_kit::segmented;
use egui::{ComboBox, Context, Ui, Window};

use crate::common::Sink;
use crate::request::UiRequest;

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

    pub fn show(&mut self, ctx: &Context, project: &ProjectView, sink: &mut Sink) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        Window::new("Project settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| body(ui, project, sink));
        self.open = open;
    }
}

fn change(sink: &mut Sink, change: ProjectChange) {
    sink.push(UiRequest::Project(change));
}

fn body(ui: &mut Ui, project: &ProjectView, sink: &mut Sink) {
    ui.label("Preset");
    let mut kind = PresetKind::ALL
        .iter()
        .position(|k| *k == project.preset)
        .unwrap_or(0);
    let names = PresetKind::ALL.map(PresetKind::name);
    if segmented(ui, &mut kind, &names) {
        change(sink, ProjectChange::SetPreset(PresetKind::ALL[kind.min(1)]));
    }
    if project.preset == PresetKind::Minecraft {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Java version");
            ComboBox::from_id_salt("project_version")
                .selected_text(project.version.as_str())
                .show_ui(ui, |ui| {
                    for version in project.versions {
                        let on = *version == project.version;
                        if ui.selectable_label(on, version).clicked() && !on {
                            change(sink, ProjectChange::SetVersion(version.clone()));
                        }
                    }
                });
        });
    }
    ui.add_space(8.0);
    ui.separator();
    ui.label("This document");
    let mut restrict = project.restrict_rotations;
    if ui
        .checkbox(
            &mut restrict,
            "Restrict rotations to one axis and 22.5 degree steps",
        )
        .changed()
    {
        change(sink, ProjectChange::SetRestrictRotations(restrict));
    }
    let mut negative = project.allow_negative_size;
    if ui
        .checkbox(&mut negative, "Allow negative sizes (inverted cubes)")
        .changed()
    {
        change(sink, ProjectChange::SetAllowNegativeSize(negative));
    }
    ui.add_space(8.0);
    if ui.button("Use the preset defaults").clicked() {
        change(sink, ProjectChange::SetPreset(project.preset));
    }
    if !project.validation.is_empty() {
        ui.add_space(8.0);
        ui.weak(format!("Checked as: {}", project.validation));
    }
    if !project.export_note.is_empty() {
        ui.weak(project.export_note);
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
