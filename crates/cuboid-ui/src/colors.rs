//! The colour section: the two tool colours and the palette swatches.

use cuboid_actions::ActionId;
use cuboid_ui_kit::{Icon, Tokens, color_edit, color_swatch, icon_button, section};
use egui::{Color32, PointerButton, Ui, Vec2};

use crate::common::Sink;
use crate::model::PaletteView;
use crate::request::{ColorSlot, Edit};

fn to_color32(c: [f32; 4]) -> Color32 {
    let byte = |v: f32| -> u8 {
        // Clamped to 0..=255 first, so the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        b
    };
    Color32::from_rgba_unmultiplied(byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]))
}

fn same_color(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0 / 512.0)
}

fn from_color32(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    [r, g, b, a].map(|v| f32::from(v) / 255.0)
}

/// The tool colours and the swatch grid.
pub(crate) fn palette_section(ui: &mut Ui, palette: &PaletteView, sink: &mut Sink) {
    section(ui, "colors_section", "Colour", true, |ui| {
        ui.horizontal(|ui| {
            for (slot, name, color) in [
                (ColorSlot::Left, "Left", palette.left),
                (ColorSlot::Right, "Right", palette.right),
            ] {
                ui.vertical(|ui| {
                    ui.small(name);
                    let mut c32 = to_color32(color);
                    let r = color_edit(ui, ("tool_color", name), &mut c32, true);
                    if r.changed || r.committed {
                        sink.edit(Edit::SetToolColor {
                            slot,
                            color: from_color32(c32),
                        });
                    }
                });
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(palette.name.as_ref()).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Plus, "Add the left colour to the palette").clicked() {
                    sink.edit(Edit::AddPaletteColor(palette.left));
                }
                ui.menu_button("Palettes", |ui| {
                    for (label, id) in [
                        ("RGB", ActionId::DefaultPalette),
                        ("HSL", ActionId::HslPalette),
                        ("JMP", ActionId::JmpPalette),
                        ("Generic 16", ActionId::Generic16Palette),
                        ("Pico-8", ActionId::Pico8Palette),
                        ("EDG32", ActionId::Edg32Palette),
                        ("EDG16", ActionId::Edg16Palette),
                        ("Load from file", ActionId::LoadPalette),
                        ("Save to file", ActionId::SavePalette),
                    ] {
                        if ui.button(label).clicked() {
                            sink.action(id);
                            ui.close();
                        }
                    }
                });
            });
        });
        let tokens = Tokens::of(ui.ctx());
        let side = 20.0;
        let spacing = 4.0;
        ui.spacing_mut().item_spacing = Vec2::splat(spacing);
        ui.horizontal_wrapped(|ui| {
            for (index, color) in palette.colors.iter().enumerate() {
                let c32 = to_color32(*color);
                let selected =
                    same_color(*color, palette.left) || same_color(*color, palette.right);
                let resp = color_swatch(ui, c32, Vec2::splat(side), selected);
                if resp.clicked_by(PointerButton::Primary) {
                    sink.edit(Edit::PickPalette {
                        index,
                        slot: ColorSlot::Left,
                    });
                }
                if resp.clicked_by(PointerButton::Secondary) {
                    sink.edit(Edit::PickPalette {
                        index,
                        slot: ColorSlot::Right,
                    });
                }
                if resp.middle_clicked() {
                    sink.edit(Edit::RemovePaletteColor(index));
                }
                resp.on_hover_text(
                    "Click: left colour. Right click: right colour. Middle click: remove",
                );
            }
        });
        if palette.colors.is_empty() {
            ui.label(egui::RichText::new("The palette is empty").color(tokens.text_weak));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_conversion_round_trips_and_clamps() {
        for c in [[0.0, 0.5, 1.0, 1.0], [1.0, 0.0, 0.25, 0.5]] {
            let back = from_color32(to_color32(c));
            for (a, b) in c.iter().zip(back) {
                assert!((a - b).abs() < 0.01, "{c:?} {back:?}");
            }
        }
        assert_eq!(
            to_color32([2.0, -1.0, f32::NAN, 1.0]).to_array()[..2],
            [255, 0]
        );
    }
}
