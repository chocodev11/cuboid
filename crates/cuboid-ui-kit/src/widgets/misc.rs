//! Collapsing sections and the search field.

use egui::{Align2, FontId, Frame, Id, Margin, Response, Sense, Stroke, TextEdit, Ui, vec2};

use crate::icons::Icon;
use crate::theme::{RADIUS, Tokens};

/// A titled block that folds. The open state lives in the context under `id_salt`.
/// Returns the body's result while it is open.
pub fn section<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    default_open: bool,
    add_body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let tokens = Tokens::of(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let open_id = id.with("open");
    let mut open = ui
        .data(|d| d.get_temp::<bool>(open_id))
        .unwrap_or(default_open);

    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, title));
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(open_id, open));
    }
    if ui.is_rect_visible(rect) {
        let bg = if resp.hovered() {
            tokens.bg_hover
        } else {
            tokens.bg_raised
        };
        ui.painter().rect_filled(rect, RADIUS, bg);
        let icon = if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        icon.paint(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 14.0, rect.center().y),
                vec2(12.0, 12.0),
            ),
            tokens.text_weak,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 28.0, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(13.5),
            tokens.text_strong,
        );
    }
    if !open {
        return None;
    }
    let inner = Frame::new()
        .inner_margin(Margin {
            left: 4,
            right: 4,
            top: 4,
            bottom: 8,
        })
        .show(ui, add_body)
        .inner;
    Some(inner)
}

/// A single-line search box with a magnifier and a clear button. The returned response is the
/// text edit's.
pub fn search_field(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    hint: &str,
) -> Response {
    let tokens = Tokens::of(ui.ctx());
    let id: Id = ui.make_persistent_id(id_salt);
    let mut cleared = false;
    let inner = Frame::new()
        .fill(tokens.bg_input)
        .stroke(Stroke::new(1.0, tokens.border_strong))
        .corner_radius(RADIUS)
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                Icon::Search.paint(
                    ui.painter(),
                    egui::Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
                    tokens.text_weak,
                );
                let width = (ui.available_width() - 22.0).max(40.0);
                let edit = TextEdit::singleline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .hint_text(hint)
                    .desired_width(width)
                    .show(ui);
                if !text.is_empty() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    let color = if resp.hovered() {
                        tokens.text_strong
                    } else {
                        tokens.text_weak
                    };
                    Icon::Close.paint(ui.painter(), rect.shrink(4.0), color);
                    cleared = resp.clicked();
                }
                edit.response.response.accessible_name(hint)
            })
            .inner
        });
    if cleared {
        text.clear();
    }
    inner.inner
}
