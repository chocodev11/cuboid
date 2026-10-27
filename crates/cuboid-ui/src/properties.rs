//! The contents of the properties tabs: the element, its faces and UV, and the dispatch to the
//! other tabs. The shell around them is `props_area`.

use cuboid_ui_kit::metrics::space;
use cuboid_ui_kit::{
    Button, DragFieldOpts, EmptyState, Icon, PropertyGrid, PropertyRows, Section, Tokens, Type,
    checkbox, chip, color_edit, color_rgba, drag_field, dropdown, rgba_color, segmented, text,
    text_field,
};
use egui::{ScrollArea, Ui};

use crate::common::{ImageCache, Sink, vector_row};
use crate::model::{DocView, Face, FaceView, GroupView, NodeId, NodeKind, PropsView};
use crate::prefs::Prefs;
use crate::project_settings::{PresetKind, ProjectView};
use crate::props_area::PropsTab;
use crate::request::{Edit, ElementFlag, FaceChange, TransformField};
use crate::{props_display, props_model, props_texture};

/// Largest size and position the fields accept.
const EXTENT: f32 = 4096.0;
/// Largest UV coordinate the fields accept, in texels.
const UV_EXTENT: f32 = 16384.0;
/// Quarter turns of a face's UV, as the labels of its segmented control.
const TURNS: [&str; 4] = ["0\u{b0}", "90\u{b0}", "180\u{b0}", "270\u{b0}"];
/// The cull choices: none, then a face each.
const CULL: [&str; 7] = ["None", "Up", "Down", "North", "South", "West", "East"];
/// The faces behind the cull choices after "None".
const CULL_FACES: [Face; 6] = [
    Face::Up,
    Face::Down,
    Face::North,
    Face::South,
    Face::West,
    Face::East,
];

/// State of the panel between frames.
pub(crate) struct PropsState {
    /// The face shown in the faces tab and the UV editor.
    pub face: Face,
    name_buffer: String,
    name_focused: bool,
}

impl Default for PropsState {
    fn default() -> Self {
        Self {
            face: Face::Up,
            name_buffer: String::new(),
            name_focused: false,
        }
    }
}

impl PropsState {
    /// The contents of one tab of the properties area. The validator tab is drawn by the shell.
    #[allow(clippy::too_many_arguments)]
    pub fn show_tab(
        &mut self,
        ui: &mut Ui,
        tab: PropsTab,
        doc: &DocView<'_>,
        project: &ProjectView,
        prefs: &Prefs,
        images: &mut ImageCache,
        sink: &mut Sink,
    ) {
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match tab {
                PropsTab::Element => self.element_tab(ui, doc, project, prefs, sink),
                PropsTab::Faces => self.faces_tab(ui, doc, project, sink),
                PropsTab::Texture => props_texture::show(ui, doc, images, sink),
                PropsTab::Model => props_model::show(ui, doc, project, sink),
                PropsTab::Display => props_display::show(ui, doc, project, prefs, sink),
                PropsTab::Animate | PropsTab::Validator => {}
            });
    }

    fn element_tab(
        &mut self,
        ui: &mut Ui,
        doc: &DocView<'_>,
        project: &ProjectView,
        prefs: &Prefs,
        sink: &mut Sink,
    ) {
        let (Some(props), Some(primary)) = (doc.props, doc.primary) else {
            EmptyState::new(Icon::Cube, "Nothing selected")
                .body("Select an object to edit it.")
                .show(ui);
            return;
        };
        Section::new("Object").show(ui, "props_object", |ui| {
            PropertyGrid::new("props_object").show(ui, |grid| {
                grid.row("Name", |ui| {
                    if !self.name_focused {
                        self.name_buffer.clear();
                        self.name_buffer.push_str(&props.name);
                    }
                    let resp = text_field(ui, "props_name", &mut self.name_buffer, "Object name");
                    self.name_focused = resp.has_focus();
                    if resp.lost_focus()
                        && self.name_buffer != *props.name
                        && !self.name_buffer.trim().is_empty()
                    {
                        sink.edit(Edit::Rename {
                            id: primary,
                            name: self.name_buffer.clone(),
                        });
                    }
                });
            });
            if props.selected > 1 {
                ui.label(text(
                    Type::Caption,
                    format!(
                        "{} objects selected; edits apply to the primary one.",
                        props.selected
                    ),
                ));
                ui.add_space(space::XS);
            }
        });
        if props.kind != NodeKind::Voxels {
            transform_section(ui, props, project, prefs, sink);
        }
        if props.kind == NodeKind::Element {
            element_section(ui, props, sink);
        }
        if let Some(group) = props.group {
            group_section(ui, group, primary, sink);
        }
    }

    fn faces_tab(
        &mut self,
        ui: &mut Ui,
        doc: &DocView<'_>,
        project: &ProjectView,
        sink: &mut Sink,
    ) {
        let Some(faces) = doc
            .props
            .filter(|p| p.kind == NodeKind::Element)
            .and_then(|p| p.faces.as_ref())
        else {
            EmptyState::new(Icon::FaceUv, "No element selected")
                .body("Select an element to edit its faces.")
                .show(ui);
            return;
        };
        Section::new("Face").show(ui, "faces_picker", |ui| {
            ui.horizontal_wrapped(|ui| {
                for face in Face::ALL {
                    if chip(ui, face.name(), self.face == face).clicked() {
                        self.face = face;
                    }
                }
            });
        });
        let face = self.face;
        face_editor(ui, doc, project, face, &faces[face.index()], sink);
    }
}

fn transform_section(
    ui: &mut Ui,
    props: &PropsView,
    project: &ProjectView,
    prefs: &Prefs,
    sink: &mut Sink,
) {
    let step = prefs.snap.step();
    let smallest = if project.allow_negative_size {
        -EXTENT
    } else {
        0.001
    };
    // A group has an origin, which only moves the pivot, and a rotation, which turns what it
    // holds. The blueprint and the model origin (also listed as groups) have a position only.
    let is_group = props.group.is_some();
    let pivot_label = if is_group { "Origin" } else { "Pivot" };
    Section::new("Transform").show(ui, "props_transform", |ui| {
        PropertyGrid::new("props_transform").show(ui, |grid| {
            let rows = [
                (
                    "Position",
                    TransformField::Position,
                    props.position,
                    DragFieldOpts {
                        speed: 0.05,
                        decimals: 4,
                        range: -EXTENT..=EXTENT,
                        snap: step,
                        ..DragFieldOpts::default()
                    },
                ),
                (
                    "Size",
                    TransformField::Size,
                    props.size,
                    DragFieldOpts {
                        speed: 0.05,
                        decimals: 4,
                        range: smallest..=EXTENT,
                        snap: step,
                        ..DragFieldOpts::default()
                    },
                ),
                (
                    "Rotation",
                    TransformField::Rotation,
                    props.rotation,
                    DragFieldOpts {
                        speed: 0.5,
                        decimals: 2,
                        unit: "\u{b0}",
                        snap: prefs.snap.enabled.then_some(prefs.snap.rotation),
                        ..DragFieldOpts::default()
                    },
                ),
                (
                    pivot_label,
                    TransformField::Pivot,
                    props.pivot,
                    DragFieldOpts {
                        speed: 0.05,
                        decimals: 4,
                        range: -EXTENT..=EXTENT,
                        snap: step,
                        ..DragFieldOpts::default()
                    },
                ),
            ];
            for (label, field, value, opts) in rows {
                let shown = if is_group {
                    matches!(field, TransformField::Pivot | TransformField::Rotation)
                } else if props.kind == NodeKind::Group {
                    field == TransformField::Position
                } else {
                    true
                };
                if !shown {
                    continue;
                }
                if let Some(edit) = vector_row(grid, label, value, &opts) {
                    sink.edit(Edit::Transform {
                        field,
                        value: edit.value,
                        commit: edit.commit,
                    });
                }
            }
        });
    });
}

fn element_section(ui: &mut Ui, props: &PropsView, sink: &mut Sink) {
    Section::new("Element").show(ui, "props_element", |ui| {
        PropertyGrid::new("props_element").show(ui, |grid| {
            grid.row("Colour", |ui| {
                let mut picked = rgba_color(props.color);
                let edit = color_edit(ui, "props_color", &mut picked, true);
                if edit.changed || edit.committed {
                    sink.edit(Edit::SetColor {
                        color: color_rgba(picked),
                        commit: edit.committed,
                    });
                }
            });
            flag_row(
                grid,
                "Shading",
                "Directional",
                props.shade,
                ElementFlag::Shade,
                sink,
            );
            flag_row(
                grid,
                "UV",
                "Follow the element",
                props.uv_snap,
                ElementFlag::UvSnap,
                sink,
            );
        });
    });
}

fn group_section(ui: &mut Ui, group: GroupView, id: NodeId, sink: &mut Sink) {
    Section::new("Group").show(ui, "props_group", |ui| {
        PropertyGrid::new("props_group").show(ui, |grid| {
            grid.row("Visible", |ui| {
                let mut on = group.visible;
                if checkbox(ui, &mut on, "Shown, with everything inside").changed() {
                    sink.edit(Edit::SetVisible { id, visible: on });
                }
            });
            grid.row("Locked", |ui| {
                let mut on = group.locked;
                if checkbox(ui, &mut on, "Protected, with everything inside").changed() {
                    sink.edit(Edit::SetLocked { id, locked: on });
                }
            });
            grid.row("Export", |ui| {
                let mut on = group.export;
                if checkbox(ui, &mut on, "Included in exports with groups").changed() {
                    sink.edit(Edit::SetExport { id, export: on });
                }
            });
        });
        ui.label(text(Type::Caption, format!("{} inside", group.children)));
        if group.children > 0 && ui.add(Button::new("Select children")).clicked() {
            sink.edit(Edit::SelectChildren { id });
        }
    });
}

fn flag_row(
    grid: &mut PropertyRows<'_>,
    label: &str,
    text: &str,
    on: bool,
    flag: ElementFlag,
    sink: &mut Sink,
) {
    grid.row(label, |ui| {
        let mut value = on;
        if checkbox(ui, &mut value, text).changed() {
            sink.edit(Edit::SetFlag { flag, on: value });
        }
    });
}

/// The choice of the quarter turn of a face's UV; the change when another one is picked.
pub(crate) fn rotation_picker(ui: &mut Ui, degrees: i32) -> Option<FaceChange> {
    let mut turn = usize::try_from(degrees.rem_euclid(360) / 90).unwrap_or(0);
    segmented(ui, &mut turn, &TURNS)
        .then(|| FaceChange::Rotation(i32::try_from(turn * 90).unwrap_or(0)))
}

fn face_editor(
    ui: &mut Ui,
    doc: &DocView<'_>,
    project: &ProjectView,
    face: Face,
    state: &FaceView,
    sink: &mut Sink,
) {
    let tokens = Tokens::of(ui.ctx());
    let mut changes: Vec<FaceChange> = Vec::new();
    let mut uv = state.uv;
    let (mut uv_changed, mut uv_committed) = (false, false);
    Section::new(face.name()).show(ui, "face_settings", |ui| {
        PropertyGrid::new("face_settings").show(ui, |grid| {
            grid.row("Visible", |ui| {
                let mut enabled = state.enabled;
                if checkbox(ui, &mut enabled, "").changed() {
                    changes.push(FaceChange::Enabled(enabled));
                }
            });
            grid.row("Texture", |ui| {
                let mut labels: Vec<&str> = doc.textures.iter().map(|t| &*t.label).collect();
                // A face may name a texture the list no longer has: it stays choosable.
                let mut current = labels
                    .iter()
                    .position(|label| *label == &*state.texture)
                    .unwrap_or_else(|| {
                        labels.push(&state.texture);
                        labels.len() - 1
                    });
                if dropdown(ui, ("face_texture", face.index()), &mut current, &labels).changed() {
                    changes.push(FaceChange::Texture(labels[current].to_owned()));
                }
            });
            let corners = [
                ("UV from", [("U", "U1"), ("V", "V1")]),
                ("UV to", [("U", "U2"), ("V", "V2")]),
            ];
            for (row, (label, fields)) in corners.into_iter().enumerate() {
                grid.row(label, |ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    let width = (ui.available_width() - space::XS) / 2.0;
                    for (k, (caption, name)) in fields.into_iter().enumerate() {
                        let i = row * 2 + k;
                        let opts = DragFieldOpts {
                            name,
                            speed: 0.25,
                            decimals: 2,
                            range: -UV_EXTENT..=UV_EXTENT,
                            snap: Some(1.0),
                            label: Some((caption, tokens.text_weak)),
                            width: Some(width),
                            ..DragFieldOpts::default()
                        };
                        let r = drag_field(ui, ("face_uv", face.index(), i), &mut uv[i], &opts);
                        uv_changed |= r.changed;
                        uv_committed |= r.committed;
                    }
                });
            }
            grid.row("Rotation", |ui| {
                changes.extend(rotation_picker(ui, state.rotation));
            });
            if project.preset != PresetKind::Minecraft {
                return;
            }
            grid.row("Tint", |ui| {
                let mut tint = f32::from(i16::try_from(state.tint).unwrap_or(-1));
                let opts = DragFieldOpts {
                    name: "Tint index",
                    speed: 0.1,
                    decimals: 0,
                    range: -1.0..=15.0,
                    snap: Some(1.0),
                    width: None,
                    ..DragFieldOpts::default()
                };
                if drag_field(ui, ("face_tint", face.index()), &mut tint, &opts).committed {
                    #[allow(clippy::cast_possible_truncation)]
                    let value = tint.round() as i32;
                    changes.push(FaceChange::Tint(value));
                }
            });
            grid.row("Cull face", |ui| {
                let mut current = state
                    .cull
                    .and_then(|cull| CULL_FACES.iter().position(|f| *f == cull))
                    .map_or(0, |at| at + 1);
                if dropdown(ui, ("face_cull", face.index()), &mut current, &CULL).changed() {
                    let cull = current.checked_sub(1).map(|at| CULL_FACES[at]);
                    changes.push(FaceChange::Cull(cull));
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            for (label, action) in [
                ("Auto UV", FaceChange::AutoUv),
                ("Rotate", FaceChange::RotateUv),
                ("Flip U", FaceChange::FlipU),
                ("Flip V", FaceChange::FlipV),
            ] {
                if ui.add(Button::new(label)).clicked() {
                    changes.push(action);
                }
            }
        });
    });
    if uv_changed || uv_committed {
        changes.push(FaceChange::Uv {
            uv,
            commit: uv_committed,
        });
    }
    for change in changes {
        sink.edit(Edit::Face { face, change });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cull_choices_name_every_face_once() {
        assert_eq!(CULL.len(), CULL_FACES.len() + 1);
        for (name, face) in CULL[1..].iter().zip(CULL_FACES) {
            assert_eq!(*name, face.name());
        }
    }

    #[test]
    fn a_turn_of_the_uv_picks_its_segment() {
        let segment = |degrees: i32| usize::try_from(degrees.rem_euclid(360) / 90).unwrap_or(0);
        assert_eq!([segment(0), segment(90), segment(270)], [0, 1, 3]);
        assert_eq!(segment(-90), 3);
    }
}
