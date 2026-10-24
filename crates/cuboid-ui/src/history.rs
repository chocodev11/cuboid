//! The undo history panel.

use cuboid_ui_kit::{Icon, RADIUS, Tokens};
use egui::{Align2, FontId, Sense, Ui, vec2};

use crate::common::Sink;
use crate::model::HistoryView;
use crate::request::UiRequest;

/// Lists the entries, oldest first, with the current position marked. A click jumps there.
pub(crate) fn show(ui: &mut Ui, history: &HistoryView, sink: &mut Sink) {
    let tokens = Tokens::of(ui.ctx());
    let applied = history.cursor.min(history.labels.len());
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            row(ui, &tokens, "Original", applied == 0, false, 0, sink);
            for (i, label) in history.labels.iter().enumerate() {
                row(
                    ui,
                    &tokens,
                    label,
                    applied == i + 1,
                    i >= applied,
                    i + 1,
                    sink,
                );
            }
        });
}

fn row(
    ui: &mut Ui,
    tokens: &Tokens,
    label: &str,
    current: bool,
    undone: bool,
    target: usize,
    sink: &mut Sink,
) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if current {
            painter.rect_filled(rect.shrink2(vec2(2.0, 0.0)), RADIUS, tokens.accent_soft);
        } else if resp.hovered() {
            painter.rect_filled(rect.shrink2(vec2(2.0, 0.0)), RADIUS, tokens.bg_hover);
        }
        let color = if undone {
            tokens.text_weak.gamma_multiply(0.8)
        } else {
            tokens.text
        };
        if current {
            Icon::History.paint(
                painter,
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 14.0, rect.center().y),
                    vec2(14.0, 14.0),
                ),
                tokens.accent,
            );
        }
        painter.text(
            egui::pos2(rect.left() + 28.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.5),
            if current { tokens.text_strong } else { color },
        );
    }
    if resp.clicked() && !current {
        sink.push(UiRequest::HistoryJump(target));
    }
}
