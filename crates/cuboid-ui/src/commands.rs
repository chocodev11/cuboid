//! The command palette overlay (Ctrl+P and F3).

use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{ListRow, Modal, Type, search_field, text};
use egui::{Context, Key, Modifiers, ScrollArea, Ui};

use crate::keymap::{Command, Keymap};
use crate::labels::{Entry, entries, search_hits};
use crate::model::UiCx;
use crate::request::Mode;

/// How many results the list shows.
const MAX_RESULTS: usize = 60;
/// Commands remembered for the empty query.
const MAX_RECENT: usize = 8;
/// Rows the list shows before it scrolls.
const VISIBLE_ROWS: f32 = 13.0;

/// State of the palette between frames.
#[derive(Default)]
pub(crate) struct PaletteState {
    open: bool,
    query: String,
    selected: usize,
    fresh: bool,
    recent: Vec<String>,
}

impl PaletteState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.fresh = true;
        self.query.clear();
        self.selected = 0;
    }

    /// Draws the palette when open and returns the command the user picked.
    pub fn show(
        &mut self,
        ctx: &Context,
        keymap: &Keymap,
        mode: Mode,
        cx: &UiCx<'_>,
    ) -> Option<Command> {
        if !self.open {
            return None;
        }
        let all = entries();
        let results = search_hits(all, &self.query, &self.recent);
        let shown = &results[..results.len().min(MAX_RESULTS)];
        self.selected = self.selected.min(shown.len().saturating_sub(1));

        let (down, up, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        if down && !shown.is_empty() {
            self.selected = (self.selected + 1) % shown.len();
        }
        if up && !shown.is_empty() {
            self.selected = (self.selected + shown.len() - 1) % shown.len();
        }

        let mut picked: Option<usize> = None;
        let mut moved_by_key = down || up;
        let outcome = Modal::new("command_palette", "Commands")
            .palette()
            .width(520.0)
            .show(ctx, |ui| {
                let search_resp =
                    search_field(ui, "palette_query", &mut self.query, "Type a command");
                if self.fresh {
                    search_resp.request_focus();
                    self.fresh = false;
                }
                if search_resp.changed() {
                    self.selected = 0;
                    moved_by_key = true;
                }
                ui.add_space(space::SM);
                if shown.is_empty() {
                    ui.label(text(Type::Label, "No command matches"));
                    return;
                }
                let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
                let mut hover_row: Option<usize> = None;
                ScrollArea::vertical()
                    .max_height(height::ROW * VISIBLE_ROWS)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (row, hit) in shown.iter().enumerate() {
                            let entry = &all[hit.index];
                            let runnable = runnable(entry, cx);
                            let hot = row == self.selected;
                            let (clicked, hovered) = entry_row(
                                ui,
                                entry,
                                keymap,
                                mode,
                                hot,
                                runnable,
                                moved_by_key && hot,
                                hit.alias,
                            );
                            if clicked && runnable {
                                picked = Some(hit.index);
                            }
                            if hovered && pointer_moved {
                                hover_row = Some(row);
                            }
                        }
                        if let Some(row) = hover_row {
                            self.selected = row;
                        }
                    });
            });
        if enter && picked.is_none() {
            picked = shown
                .get(self.selected)
                .map(|hit| hit.index)
                .filter(|&i| runnable(&all[i], cx));
        }
        if outcome.cancelled {
            self.open = false;
        }
        let command = picked.map(|i| all[i].command)?;
        self.open = false;
        let id = command.id();
        self.recent.retain(|r| *r != id);
        self.recent.insert(0, id);
        self.recent.truncate(MAX_RECENT);
        Some(command)
    }
}

fn runnable(entry: &Entry, cx: &UiCx<'_>) -> bool {
    match entry.command {
        Command::Action(id) => cx.can_run(id),
        Command::Ui(_) => true,
    }
}

/// One result: its label, the name or category it was found by and its shortcut. Returns whether
/// it was clicked and whether the pointer is over it.
#[allow(clippy::too_many_arguments)]
fn entry_row(
    ui: &mut Ui,
    entry: &Entry,
    keymap: &Keymap,
    mode: Mode,
    hot: bool,
    runnable: bool,
    scroll_to: bool,
    alias: Option<&str>,
) -> (bool, bool) {
    let chord = keymap
        .shortcut_of(entry.command, mode)
        .map(|chord| chord.to_string());
    let mut row = ListRow::new(&entry.label)
        .caption(alias.unwrap_or(entry.category))
        .selected(hot);
    if let Some(chord) = &chord {
        row = row.shortcut(chord);
    }
    let response = ui.add_enabled(runnable, row);
    if scroll_to {
        response.scroll_to_me(None);
    }
    (response.clicked(), response.hovered())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_resets_the_query_and_selection() {
        let mut state = PaletteState {
            query: "old".into(),
            selected: 4,
            ..PaletteState::default()
        };
        assert!(!state.is_open());
        state.open();
        assert!(state.is_open());
        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
    }
}
