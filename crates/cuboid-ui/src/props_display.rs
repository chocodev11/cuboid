//! The Display tab: the eight Minecraft display contexts, each switched on or off, and the
//! rotation, translation and scale of the chosen one.

use cuboid_ui_kit::metrics::space;
use cuboid_ui_kit::{
    DragFieldOpts, EmptyState, Icon, ListRow, MenuItem, PropertyGrid, Section, Type, menu_trigger,
    text, toggle,
};
use egui::{Label, Ui};
use glam::Vec3;

use crate::common::{Sink, vector_row};
use crate::model::{DisplaySlotView, DocView};
use crate::prefs::Prefs;
use crate::project_settings::{PresetKind, ProjectView};
use crate::request::{DisplayPreset, Edit};

/// What the game's name of a context is called in the list; a name the table lacks stays as it is.
fn label(key: &str) -> &str {
    match key {
        "thirdperson_righthand" => "Third person, right hand",
        "thirdperson_lefthand" => "Third person, left hand",
        "firstperson_righthand" => "First person, right hand",
        "firstperson_lefthand" => "First person, left hand",
        "head" => "Head",
        "gui" => "GUI",
        "ground" => "Ground",
        "fixed" => "Item frame",
        other => other,
    }
}

/// The page of the tab.
pub(crate) fn show(
    ui: &mut Ui,
    doc: &DocView<'_>,
    project: &ProjectView,
    prefs: &Prefs,
    sink: &mut Sink,
) {
    let Some(view) = doc.display.filter(|d| !d.slots.is_empty()) else {
        EmptyState::new(Icon::Display, "No display settings")
            .body("This model has no display contexts.")
            .show(ui);
        return;
    };
    if project.preset == PresetKind::Cubik {
        ui.add(
            Label::new(text(
                Type::Caption,
                "Cubik stores these settings. They matter for the Minecraft export.",
            ))
            .wrap(),
        );
        ui.add_space(space::SM);
    }
    let active = view.active.min(view.slots.len() - 1);
    Section::new("Contexts").show(ui, "display_contexts", |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, slot) in view.slots.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let name = label(&slot.name);
                let mut on = slot.enabled;
                let switch = toggle(ui, &mut on, "").accessible_name(format!("{name} exported"));
                if switch.changed() {
                    sink.edit(Edit::SetDisplayEnabled {
                        slot: index,
                        enabled: on,
                    });
                }
                let mut row = ListRow::new(name).selected(index == active);
                if !slot.preset.is_empty() && &*slot.preset != "Default" {
                    row = row.caption(&slot.preset);
                }
                let row = ui.add(row);
                if row.clicked() && index != active {
                    sink.edit(Edit::SelectDisplaySlot(index));
                }
            });
        }
    });
    let slot = &view.slots[active];
    Section::new(label(&slot.name)).show(ui, "display_values", |ui| {
        values(ui, slot, active, prefs, sink);
        ui.add_space(space::SM);
        menu_trigger(ui, Some(Icon::Import), "Load preset", false, |ui| {
            for preset in DisplayPreset::ALL {
                if ui.add(MenuItem::new(preset.id())).clicked() {
                    sink.edit(Edit::LoadDisplayPreset {
                        slot: active,
                        preset,
                    });
                    ui.close();
                }
            }
        });
    });
}

/// The three vectors of the chosen context; they are locked while it is switched off.
fn values(ui: &mut Ui, slot: &DisplaySlotView, active: usize, prefs: &Prefs, sink: &mut Sink) {
    ui.add_enabled_ui(slot.enabled, |ui| {
        let mut next = [slot.rotation, slot.translation, slot.scale];
        let mut edit = None;
        PropertyGrid::new("display_values").show(ui, |grid| {
            let rows = [
                (
                    "Rotation",
                    DragFieldOpts {
                        speed: 0.5,
                        decimals: 3,
                        range: -360.0..=360.0,
                        unit: "\u{b0}",
                        snap: prefs.snap.enabled.then_some(prefs.snap.rotation),
                        ..DragFieldOpts::default()
                    },
                ),
                (
                    "Translation",
                    DragFieldOpts {
                        speed: 0.1,
                        decimals: 3,
                        range: -80.0..=80.0,
                        ..DragFieldOpts::default()
                    },
                ),
                (
                    "Scale",
                    DragFieldOpts {
                        speed: 0.01,
                        decimals: 3,
                        range: -4.0..=4.0,
                        ..DragFieldOpts::default()
                    },
                ),
            ];
            for (i, (label, opts)) in rows.into_iter().enumerate() {
                if let Some(changed) = vector_row(grid, label, next[i], &opts) {
                    next[i] = changed.value;
                    edit = Some(edit.unwrap_or(false) | changed.commit);
                }
            }
        });
        if let Some(commit) = edit {
            let [rotation, translation, scale]: [Vec3; 3] = next;
            sink.edit(Edit::SetDisplaySlot {
                slot: active,
                rotation,
                translation,
                scale,
                commit,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_context_of_the_game_has_a_name_and_an_unknown_one_keeps_its_own() {
        for key in [
            "thirdperson_righthand",
            "thirdperson_lefthand",
            "firstperson_righthand",
            "firstperson_lefthand",
            "head",
            "gui",
            "ground",
            "fixed",
        ] {
            assert_ne!(label(key), key, "{key}");
        }
        assert_eq!(label("hologram"), "hologram");
    }
}
