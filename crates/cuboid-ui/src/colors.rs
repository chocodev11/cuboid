//! The Palette panel: the two tool colours and the swatches of the palette.

use cuboid_actions::ActionId;
use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    EmptyState, Icon, IconButton, MenuItem, PropertyGrid, Section, color_edit, color_rgba,
    color_swatch, menu_trigger, rgba_color,
};
use egui::{PointerButton, ScrollArea, Ui, vec2};

use crate::common::Sink;
use crate::model::PaletteView;
use crate::request::{ColorSlot, Edit};

/// The palettes the menu offers, with the action that loads them.
const PALETTES: [(&str, ActionId); 9] = [
    ("RGB", ActionId::DefaultPalette),
    ("HSL", ActionId::HslPalette),
    ("JMP", ActionId::JmpPalette),
    ("Generic 16", ActionId::Generic16Palette),
    ("Pico-8", ActionId::Pico8Palette),
    ("EDG32", ActionId::Edg32Palette),
    ("EDG16", ActionId::Edg16Palette),
    ("Load from file", ActionId::LoadPalette),
    ("Save to file", ActionId::SavePalette),
];

fn same_color(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0 / 512.0)
}

/// How many swatches of `side` with `gap` between them fit in `width`; at least one.
fn per_row(width: f32, side: f32, gap: f32) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fit = ((width + gap) / (side + gap)).floor().max(1.0) as usize;
    fit
}

/// The panel body.
pub(crate) fn show(ui: &mut Ui, palette: &PaletteView, sink: &mut Sink) {
    Section::new("Tool colours").show(ui, "palette_tools", |ui| {
        PropertyGrid::new("palette_tools").show(ui, |grid| {
            for (slot, label, color) in [
                (ColorSlot::Left, "Left button", palette.left),
                (ColorSlot::Right, "Right button", palette.right),
            ] {
                grid.row(label, |ui| {
                    let mut picked = rgba_color(color);
                    let edit = color_edit(ui, ("tool_color", label), &mut picked, true);
                    if edit.changed || edit.committed {
                        sink.edit(Edit::SetToolColor {
                            slot,
                            color: color_rgba(picked),
                        });
                    }
                });
            }
        });
    });
    Section::new(&palette.name).show(ui, "palette_swatches", |ui| {
        ui.horizontal(|ui| {
            menu_trigger(ui, None, "Palettes", false, |ui| {
                for (label, id) in PALETTES {
                    if ui.add(MenuItem::new(label)).clicked() {
                        sink.action(id);
                        ui.close();
                    }
                }
            });
            if ui
                .add(IconButton::new(
                    Icon::Plus,
                    "Add the left colour to the palette",
                ))
                .clicked()
            {
                sink.edit(Edit::AddPaletteColor(palette.left));
            }
        });
        ui.add_space(space::XS);
        swatches(ui, palette, sink);
    });
}

/// The swatch grid, drawn row by row so that a palette of thousands costs the visible rows.
fn swatches(ui: &mut Ui, palette: &PaletteView, sink: &mut Sink) {
    if palette.colors.is_empty() {
        EmptyState::new(Icon::Palette, "The palette is empty")
            .body("Add the left colour with the plus button.")
            .show(ui);
        return;
    }
    let side = height::CONTROL;
    let gap = space::XS;
    let columns = per_row(ui.available_width(), side, gap);
    let rows = palette.colors.len().div_ceil(columns);
    ui.spacing_mut().item_spacing = vec2(gap, gap);
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, side, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    let from = row * columns;
                    let to = (from + columns).min(palette.colors.len());
                    for (offset, color) in palette.colors[from..to].iter().enumerate() {
                        let selected =
                            same_color(*color, palette.left) || same_color(*color, palette.right);
                        let resp = color_swatch(ui, rgba_color(*color), vec2(side, side), selected);
                        let index = from + offset;
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
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fits_whole_swatches_and_never_zero() {
        assert_eq!(per_row(224.0, 24.0, 4.0), 8);
        assert_eq!(per_row(10.0, 24.0, 4.0), 1);
        assert_eq!(per_row(0.0, 24.0, 4.0), 1);
    }

    #[test]
    fn two_colours_are_the_same_within_a_byte() {
        assert!(same_color([0.5; 4], [0.5 + 1.0 / 1024.0; 4]));
        assert!(!same_color([0.5; 4], [0.6; 4]));
    }
}
