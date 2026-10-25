//! The command palette overlay (Ctrl+P and F3).

use cuboid_ui_kit::{RADIUS, Tokens, key_chip, search_field};
use egui::{Align2, Context, Key, Modal, Modifiers, Sense, Ui, vec2};

use crate::keymap::{Command, Keymap};
use crate::labels::{Entry, entries, search_hits};
use crate::model::UiCx;
use crate::request::Mode;

/// How many results the list shows.
const MAX_RESULTS: usize = 60;
/// Commands remembered for the empty query.
const MAX_RECENT: usize = 8;

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
        let tokens = Tokens::of(ctx);
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
        let modal = Modal::new(egui::Id::unique("cuboid_command_palette")).area(
            Modal::default_area(egui::Id::unique("cuboid_command_palette"))
                .anchor(Align2::CENTER_TOP, vec2(0.0, 88.0)),
        );
        let response = modal.show(ctx, |ui| {
            ui.set_width(540.0);
            let search_resp = search_field(ui, "palette_query", &mut self.query, "Type a command");
            if self.fresh {
                search_resp.request_focus();
                self.fresh = false;
            }
            if search_resp.changed() {
                self.selected = 0;
                moved_by_key = true;
            }
            ui.add_space(8.0);
            if shown.is_empty() {
                ui.label(egui::RichText::new("No command matches").color(tokens.text_weak));
                return;
            }
            let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
            let mut hover_row: Option<usize> = None;
            egui::ScrollArea::vertical()
                .max_height(380.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for (row, hit) in shown.iter().enumerate() {
                        let entry = &all[hit.index];
                        let runnable = runnable(entry, cx);
                        let hot = row == self.selected;
                        let (clicked, hovered) = entry_row(
                            ui,
                            &tokens,
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
        if response.should_close() {
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

#[allow(clippy::too_many_arguments)]
fn entry_row(
    ui: &mut Ui,
    tokens: &Tokens,
    entry: &Entry,
    keymap: &Keymap,
    mode: Mode,
    hot: bool,
    runnable: bool,
    scroll_to: bool,
    alias: Option<&str>,
) -> (bool, bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, runnable, entry.label.as_str())
    });
    if hot && scroll_to {
        resp.scroll_to_me(None);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if hot {
            painter.rect_filled(rect, RADIUS, tokens.accent_soft);
        } else if resp.hovered() {
            painter.rect_filled(rect, RADIUS, tokens.bg_hover);
        }
        let color = if !runnable {
            tokens.text_weak.gamma_multiply(0.6)
        } else if hot {
            tokens.text_strong
        } else {
            tokens.text
        };
        let galley =
            painter.layout_no_wrap(entry.label.clone(), egui::FontId::proportional(14.0), color);
        let text_pos = egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
        let label_width = galley.size().x;
        painter.galley(text_pos, galley, color);
        painter.text(
            egui::pos2(rect.left() + 10.0 + label_width + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            alias.unwrap_or(entry.category),
            egui::FontId::proportional(12.0),
            tokens.text_weak,
        );
    }
    if let Some(chord) = keymap.shortcut_of(entry.command, mode) {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(8.0, 4.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        key_chip(&mut child, &chord.to_string());
    }
    (resp.clicked(), resp.hovered())
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
