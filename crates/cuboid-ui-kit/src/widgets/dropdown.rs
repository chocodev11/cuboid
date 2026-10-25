//! The drop-down list.

use egui::{Popup, PopupCloseBehavior, Response, Role, Sense, Ui, WidgetInfo, pos2, vec2};

use crate::icons::Icon;
use crate::metrics::{height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::paint::{galley, paint_icon, paint_left};
use crate::widgets::state::{ControlState, control_fill, dimmed, focus_ring, keep_arrows, look};

/// A drop-down list: the chosen option and a caret; the list opens below. Returns the button's
/// response, marked changed when another option was chosen. Up and Down on the focused button
/// move the choice without opening the list.
pub fn dropdown(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &mut usize,
    options: &[&str],
) -> Response {
    let t = Tokens::of(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let width = ui.available_width().max(space::XXL * 3.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height::CONTROL), Sense::hover());
    let mut response = ui.interact(rect, id, Sense::click());
    let enabled = ui.is_enabled();
    let shown = options.get(*current).copied().unwrap_or_default();
    response.widget_info(|| WidgetInfo::selected(Role::ComboBox, enabled, false, shown));

    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&response));
    keep_arrows(ui, id, false, true);
    if response.has_focus() && !open {
        let step = ui.input_mut(|i| {
            let down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
            let up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
            isize::from(down) - isize::from(up)
        });
        let next = current
            .saturating_add_signed(step)
            .min(options.len().saturating_sub(1));
        if step != 0 && next != *current {
            *current = next;
            response.mark_changed();
        }
    }

    if ui.is_rect_visible(rect) {
        let mut look = look(ui, &response, false, false);
        if open && look.state == ControlState::Normal {
            look.state = ControlState::Pressed;
        }
        let fill = control_fill(&t, look.state);
        ui.painter().rect_filled(rect, radius::CONTROL, fill);
        let ink = match look.state {
            ControlState::Hover | ControlState::Pressed => t.text_strong,
            _ => t.text,
        };
        let text = galley(ui, Type::Body, shown);
        ui.painter()
            .with_clip_rect(rect.shrink2(vec2(space::SM, 0.0)))
            .galley(
                pos2(
                    rect.left() + space::SM,
                    rect.center().y - text.size().y / 2.0,
                ),
                text,
                dimmed(ink, look),
            );
        paint_icon(
            ui,
            Icon::CaretUpDown,
            pos2(
                rect.right() - space::SM - icon_size::CONTROL / 2.0,
                rect.center().y,
            ),
            icon_size::CONTROL,
            dimmed(t.text_weak, look),
        );
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
    }

    let mut chosen = None;
    Popup::menu(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .width(rect.width())
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (index, label) in options.iter().enumerate() {
                if option_row(ui, label, index == *current).clicked() {
                    chosen = Some(index);
                }
            }
        });
    if let Some(index) = chosen
        && index != *current
    {
        *current = index;
        response.mark_changed();
    }
    response
}

/// One option in a list: a full-width row with a tick on the chosen one.
fn option_row(ui: &mut Ui, label: &str, chosen: bool) -> Response {
    let t = Tokens::of(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height::CONTROL), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::MenuItem, enabled, chosen, label));
    if ui.is_rect_visible(rect) {
        let look = look(ui, &response, false, false);
        if matches!(look.state, ControlState::Hover | ControlState::Pressed) {
            ui.painter().rect_filled(rect, radius::CONTROL, t.bg_hover);
        }
        let ink = if chosen { t.text_strong } else { t.text };
        let role = if chosen { Type::Strong } else { Type::Body };
        paint_left(
            ui,
            galley(ui, role, label),
            rect.left() + space::SM,
            rect.center().y,
            ink,
        );
        if chosen {
            paint_icon(
                ui,
                Icon::Check,
                pos2(
                    rect.right() - space::SM - icon_size::CONTROL / 2.0,
                    rect.center().y,
                ),
                icon_size::CONTROL,
                t.text_strong,
            );
        }
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
    }
    response
}
