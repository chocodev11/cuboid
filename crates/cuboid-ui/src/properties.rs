//! The properties panel: object, transform, element, faces and UV, textures, colours and the
//! display slots, arranged by workspace.

use cuboid_ui_kit::{DragFieldOpts, Tokens, color_edit, drag_field, section};
use egui::{Color32, ComboBox, TextEdit, Ui};
use glam::Vec3;

use crate::colors;
use crate::common::{ImageCache, Sink, vec3_row};
use crate::model::{DocView, Face, FaceView, NodeKind, PropsView};
use crate::prefs::Prefs;
use crate::request::{Edit, ElementFlag, FaceChange, Mode, TransformField};
use crate::textures;

/// State of the panel between frames.
pub(crate) struct PropsState {
    /// The face shown in the faces section and the UV editor.
    pub face: Face,
    name_buffer: String,
}

impl Default for PropsState {
    fn default() -> Self {
        Self {
            face: Face::Up,
            name_buffer: String::new(),
        }
    }
}

fn tint32(c: [f32; 4]) -> Color32 {
    let byte = |v: f32| -> u8 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        b
    };
    Color32::from_rgba_unmultiplied(byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]))
}

impl PropsState {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        mode: Mode,
        doc: &DocView<'_>,
        prefs: &Prefs,
        images: &mut ImageCache,
        sink: &mut Sink,
    ) {
        let tokens = Tokens::of(ui.ctx());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                match mode {
                    Mode::Model => {
                        self.object(ui, doc, sink);
                        if let (Some(props), Some(_)) = (doc.props, doc.primary) {
                            self.transform(ui, props, prefs, sink);
                            if props.kind == NodeKind::Element {
                                element(ui, props, sink);
                                self.faces(ui, doc, props, sink);
                            }
                        } else {
                            empty_hint(ui, &tokens, "Select an object to edit it.");
                        }
                    }
                    Mode::Voxel => {
                        colors::palette_section(ui, doc.palette, sink);
                    }
                    Mode::Paint => {
                        textures::section_body(ui, doc, images, sink);
                        colors::palette_section(ui, doc.palette, sink);
                        if let Some(props) = doc.props.filter(|p| p.kind == NodeKind::Element) {
                            self.faces(ui, doc, props, sink);
                        }
                    }
                    Mode::Display => display(ui, doc, prefs, sink),
                }
            });
    }

    fn object(&mut self, ui: &mut Ui, doc: &DocView<'_>, sink: &mut Sink) {
        let (Some(props), Some(primary)) = (doc.props, doc.primary) else {
            return;
        };
        let id = ui.make_persistent_id("props_name");
        let focused = ui.memory(|m| m.has_focus(id));
        if !focused {
            self.name_buffer.clear();
            self.name_buffer.push_str(&props.name);
        }
        ui.horizontal(|ui| {
            let resp = ui
                .add(
                    TextEdit::singleline(&mut self.name_buffer)
                        .id(id)
                        .desired_width(ui.available_width()),
                )
                .accessible_name("Object name");
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
        if props.selected > 1 {
            ui.label(format!(
                "{} objects selected; edits apply to the primary one.",
                props.selected
            ));
        }
    }

    fn transform(&mut self, ui: &mut Ui, props: &PropsView, prefs: &Prefs, sink: &mut Sink) {
        if props.kind == NodeKind::Voxels {
            return;
        }
        section(ui, "transform_section", "Transform", true, |ui| {
            let step = prefs.snap.step();
            let rows: [(&str, &str, TransformField, Vec3, DragFieldOpts<'_>); 4] = [
                (
                    "Position",
                    "props_pos",
                    TransformField::Position,
                    props.position,
                    DragFieldOpts {
                        speed: 0.05,
                        decimals: 4,
                        snap: step,
                        ..DragFieldOpts::default()
                    },
                ),
                (
                    "Size",
                    "props_size",
                    TransformField::Size,
                    props.size,
                    DragFieldOpts {
                        speed: 0.05,
                        decimals: 4,
                        range: 0.0..=4096.0,
                        snap: step,
                        ..DragFieldOpts::default()
                    },
                ),
                (
                    "Rotation",
                    "props_rot",
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
                    "Pivot",
                    "props_pivot",
                    TransformField::Pivot,
                    props.pivot,
                    DragFieldOpts {
                        speed: 0.05,
                        decimals: 4,
                        snap: step,
                        ..DragFieldOpts::default()
                    },
                ),
            ];
            for (label, id, field, value, opts) in rows {
                if props.kind == NodeKind::Group && !matches!(field, TransformField::Position) {
                    continue;
                }
                ui.label(label);
                let edit = vec3_row(ui, id, label, value, &opts);
                if edit.changed || edit.committed {
                    sink.edit(Edit::Transform {
                        field,
                        value: edit.value,
                        commit: edit.committed,
                    });
                }
            }
        });
    }

    fn faces(&mut self, ui: &mut Ui, doc: &DocView<'_>, props: &PropsView, sink: &mut Sink) {
        let Some(faces) = &props.faces else { return };
        section(ui, "faces_section", "Faces and UV", true, |ui| {
            ui.horizontal_wrapped(|ui| {
                for face in Face::ALL {
                    ui.selectable_value(&mut self.face, face, face.name());
                }
            });
            let face = self.face;
            let state = &faces[face.index()];
            face_editor(ui, doc, face, state, sink);
        });
    }
}

fn element(ui: &mut Ui, props: &PropsView, sink: &mut Sink) {
    section(ui, "element_section", "Element", true, |ui| {
        ui.horizontal(|ui| {
            ui.label("Colour");
            let mut c = tint32(props.color);
            let r = color_edit(ui, "props_color", &mut c, true);
            if r.changed || r.committed {
                let [rr, g, b, a] = c.to_srgba_unmultiplied().map(|v| f32::from(v) / 255.0);
                sink.edit(Edit::SetColor {
                    color: [rr, g, b, a],
                    commit: r.committed,
                });
            }
        });
        let mut shade = props.shade;
        if ui.checkbox(&mut shade, "Directional shading").changed() {
            sink.edit(Edit::SetFlag {
                flag: ElementFlag::Shade,
                on: shade,
            });
        }
        let mut uv_snap = props.uv_snap;
        if ui
            .checkbox(&mut uv_snap, "UV follows the element")
            .changed()
        {
            sink.edit(Edit::SetFlag {
                flag: ElementFlag::UvSnap,
                on: uv_snap,
            });
        }
    });
}

fn face_editor(ui: &mut Ui, doc: &DocView<'_>, face: Face, state: &FaceView, sink: &mut Sink) {
    let mut enabled = state.enabled;
    if ui.checkbox(&mut enabled, "Visible").changed() {
        sink.edit(Edit::Face {
            face,
            change: FaceChange::Enabled(enabled),
        });
    }
    ui.horizontal(|ui| {
        ui.label("Texture");
        let combo = ComboBox::from_id_salt(("face_texture", face.index()))
            .selected_text(state.texture.as_ref())
            .show_ui(ui, |ui| {
                for texture in doc.textures {
                    let selected = texture.label == state.texture;
                    if ui
                        .selectable_label(selected, texture.label.as_ref())
                        .clicked()
                        && !selected
                    {
                        sink.edit(Edit::Face {
                            face,
                            change: FaceChange::Texture(texture.label.to_string()),
                        });
                    }
                }
            });
        combo.response.accessible_name("Face texture");
    });
    let tokens = Tokens::of(ui.ctx());
    let mut uv = state.uv;
    let (mut changed, mut committed) = (false, false);
    let names = ["U1", "V1", "U2", "V2"];
    let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(56.0);
    for pair in 0..2 {
        ui.horizontal(|ui| {
            for k in 0..2 {
                let i = pair * 2 + k;
                let opts = DragFieldOpts {
                    name: names[i],
                    speed: 0.25,
                    decimals: 2,
                    range: -16384.0..=16384.0,
                    snap: Some(1.0),
                    label: Some((names[i], tokens.text_weak)),
                    width: Some(width),
                    ..DragFieldOpts::default()
                };
                let r = drag_field(ui, ("face_uv", face.index(), i), &mut uv[i], &opts);
                changed |= r.changed;
                committed |= r.committed;
            }
        });
    }
    if changed || committed {
        sink.edit(Edit::Face {
            face,
            change: FaceChange::Uv {
                uv,
                commit: committed,
            },
        });
    }
    ui.horizontal(|ui| {
        ui.label("Tint");
        let mut tint = f32::from(i16::try_from(state.tint).unwrap_or(-1));
        let opts = DragFieldOpts {
            name: "Tint index",
            speed: 0.1,
            decimals: 0,
            range: -1.0..=15.0,
            snap: Some(1.0),
            width: Some(56.0),
            ..DragFieldOpts::default()
        };
        let r = drag_field(ui, ("face_tint", face.index()), &mut tint, &opts);
        if r.committed {
            #[allow(clippy::cast_possible_truncation)]
            let value = tint.round() as i32;
            sink.edit(Edit::Face {
                face,
                change: FaceChange::Tint(value),
            });
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (label, change) in [
            ("Auto UV", FaceChange::AutoUv),
            ("Rotate", FaceChange::RotateUv),
            ("Flip U", FaceChange::FlipU),
            ("Flip V", FaceChange::FlipV),
        ] {
            if ui.button(label).clicked() {
                sink.edit(Edit::Face { face, change });
            }
        }
    });
}

fn display(ui: &mut Ui, doc: &DocView<'_>, prefs: &Prefs, sink: &mut Sink) {
    let tokens = Tokens::of(ui.ctx());
    let Some(view) = doc.display.filter(|d| !d.slots.is_empty()) else {
        empty_hint(ui, &tokens, "This model has no display settings.");
        return;
    };
    let active = view.active.min(view.slots.len() - 1);
    let slot = &view.slots[active];
    section(ui, "display_section", "Display slot", true, |ui| {
        ComboBox::from_id_salt("display_slot")
            .selected_text(slot.name.as_ref())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for (i, s) in view.slots.iter().enumerate() {
                    if ui.selectable_label(i == active, s.name.as_ref()).clicked() && i != active {
                        sink.edit(Edit::SelectDisplaySlot(i));
                    }
                }
            })
            .response
            .accessible_name("Display slot");
        let rows = [
            (
                "Rotation",
                "display_rot",
                slot.rotation,
                0.5,
                360.0,
                "\u{b0}",
                prefs.snap.enabled.then_some(prefs.snap.rotation),
            ),
            (
                "Translation",
                "display_pos",
                slot.translation,
                0.1,
                80.0,
                "",
                None,
            ),
            ("Scale", "display_scale", slot.scale, 0.01, 4.0, "", None),
        ];
        let mut values = [slot.rotation, slot.translation, slot.scale];
        let (mut changed, mut committed) = (false, false);
        for (i, (label, id, value, speed, limit, unit, snap)) in rows.into_iter().enumerate() {
            ui.label(label);
            let opts = DragFieldOpts {
                speed,
                decimals: 3,
                range: -limit..=limit,
                unit,
                snap,
                ..DragFieldOpts::default()
            };
            let r = vec3_row(ui, id, label, value, &opts);
            values[i] = r.value;
            changed |= r.changed;
            committed |= r.committed;
        }
        if changed || committed {
            sink.edit(Edit::SetDisplaySlot {
                slot: active,
                rotation: values[0],
                translation: values[1],
                scale: values[2],
                commit: committed,
            });
        }
    });
}

fn empty_hint(ui: &mut Ui, tokens: &Tokens, text: &str) {
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).color(tokens.text_weak));
    });
}
