//! Icon buttons, tool buttons, tabs, segmented controls and key chips.

use egui::{
    Align2, Color32, FontId, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2,
    vec2,
};

use crate::icons::Icon;
use crate::theme::{RADIUS, RADIUS_SM, Tokens};

const BUTTON_SIDE: f32 = 28.0;
const ICON_SIDE: f32 = 18.0;

/// A flat square button with an icon. `tip` is the tooltip and the accessible name.
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    flat(ui, icon, tip, false).on_hover_text(tip)
}

/// A square toggle for tools and modes: selected buttons get an accent background.
pub fn tool_button(ui: &mut Ui, icon: Icon, selected: bool, tip: &str) -> Response {
    flat(ui, icon, tip, selected).on_hover_text(tip)
}

fn flat(ui: &mut Ui, icon: Icon, tip: &str, selected: bool) -> Response {
    let tokens = Tokens::of(ui.ctx());
    let enabled = ui.is_enabled();
    let (rect, resp) = ui.allocate_exact_size(vec2(BUTTON_SIDE, BUTTON_SIDE), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, tip));
    if ui.is_rect_visible(rect) {
        let (bg, fg) = if selected {
            (tokens.accent_soft, tokens.accent)
        } else if resp.is_pointer_button_down_on() {
            (tokens.bg_active, tokens.text_strong)
        } else if resp.hovered() {
            (tokens.bg_hover, tokens.text_strong)
        } else {
            (Color32::TRANSPARENT, tokens.text)
        };
        let fg = if enabled {
            fg
        } else {
            tokens.text_weak.gamma_multiply(0.6)
        };
        ui.painter().rect_filled(rect, RADIUS, bg);
        let icon_rect = Rect::from_center_size(rect.center(), vec2(ICON_SIDE, ICON_SIDE));
        icon.paint(ui.painter(), icon_rect, fg);
    }
    resp
}

/// A text tab with an accent underline when selected, for the mode switch and panel tabs.
pub fn tab_button(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let tokens = Tokens::of(ui.ctx());
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(13.5), tokens.text);
    let size = vec2(galley.size().x + 24.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::selected(Role::Tab, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        let fg = if selected {
            tokens.text_strong
        } else if resp.hovered() {
            tokens.text
        } else {
            tokens.text_weak
        };
        if resp.hovered() && !selected {
            ui.painter()
                .rect_filled(rect.shrink2(vec2(0.0, 2.0)), RADIUS, tokens.bg_hover);
        }
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, fg);
        if selected {
            let bar = Rect::from_min_max(
                pos2(rect.left() + 8.0, rect.bottom() - 2.5),
                pos2(rect.right() - 8.0, rect.bottom()),
            );
            ui.painter().rect_filled(bar, 0.0, tokens.accent);
        }
    }
    resp
}

/// A row of mutually exclusive text buttons. Returns whether the selection changed.
pub fn segmented(ui: &mut Ui, current: &mut usize, labels: &[&str]) -> bool {
    let tokens = Tokens::of(ui.ctx());
    let mut changed = false;
    egui::Frame::new()
        .fill(tokens.bg_input)
        .corner_radius(RADIUS)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let selected = *current == i;
                    let galley = ui.painter().layout_no_wrap(
                        (*label).to_owned(),
                        FontId::proportional(13.0),
                        tokens.text,
                    );
                    let size = vec2(galley.size().x + 16.0, 24.0);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, *label));
                    let (bg, fg) = if selected {
                        (tokens.accent, tokens.on_accent)
                    } else if resp.hovered() {
                        (tokens.bg_hover, tokens.text_strong)
                    } else {
                        (Color32::TRANSPARENT, tokens.text)
                    };
                    ui.painter().rect_filled(rect, RADIUS_SM, bg);
                    ui.painter()
                        .galley(rect.center() - galley.size() / 2.0, galley, fg);
                    if resp.clicked() && !selected {
                        *current = i;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A small rounded label for a keyboard shortcut.
pub fn key_chip(ui: &mut Ui, text: &str) -> Response {
    let tokens = Tokens::of(ui.ctx());
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(11.5),
        tokens.text_weak,
    );
    let size = galley.size() + vec2(8.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        RADIUS_SM,
        tokens.bg_input,
        Stroke::new(1.0, tokens.border),
        StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        tokens.text_weak,
    );
    resp
}

/// Draws `text` right-aligned inside `rect` in the weak colour; used by menu rows.
pub fn weak_text_right(ui: &Ui, rect: Rect, text: &str) {
    let tokens = Tokens::of(ui.ctx());
    ui.painter().text(
        pos2(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        text,
        FontId::proportional(12.0),
        tokens.text_weak,
    );
}
