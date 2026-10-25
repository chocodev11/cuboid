//! The undo history panel.

use cuboid_ui_kit::metrics::height;
use cuboid_ui_kit::{EmptyState, Icon, ListRow};
use egui::{ScrollArea, Ui};

use crate::common::Sink;
use crate::model::HistoryView;
use crate::request::UiRequest;

/// Lists the entries, oldest first, with the current position marked. A click jumps there.
pub(crate) fn show(ui: &mut Ui, history: &HistoryView, sink: &mut Sink) {
    if history.labels.is_empty() {
        EmptyState::new(Icon::History, "No history yet")
            .body("Every change you make adds a step you can jump back to.")
            .show(ui);
        return;
    }
    let applied = history.cursor.min(history.labels.len());
    ui.spacing_mut().item_spacing.y = 0.0;
    // The first row is the model as it was opened; the others are the steps.
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show_rows(ui, height::ROW, history.labels.len() + 1, |ui, range| {
            for target in range {
                let label = match target.checked_sub(1) {
                    Some(step) => &*history.labels[step],
                    None => "Original",
                };
                let current = applied == target;
                let row = ListRow::new(label)
                    .icon(if current { Icon::Check } else { Icon::History })
                    .selected(current)
                    .dimmed(target > applied);
                if ui.add(row).clicked() && !current {
                    sink.push(UiRequest::HistoryJump(target));
                }
            }
        });
}
