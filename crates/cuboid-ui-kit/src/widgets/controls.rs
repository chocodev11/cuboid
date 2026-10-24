//! Form controls: checkbox, toggle, text field, dropdown, slider and the vector field. Checked
//! and selected states are neutral (pressed surface plus strong text or icon), never the accent.

use egui::{
    Align, Id, Layout, Rect, Response, Role, Sense, TextEdit, Ui, UiBuilder, WidgetInfo, pos2, vec2,
};

use crate::icons::Icon;
use crate::metrics::{height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::paint::{galley, paint_icon, paint_left};
use crate::widgets::state::{ControlState, dimmed, focus_ring, look};

/// Side of the checkbox square.
const CHECK_BOX: f32 = 16.0;
/// Size of the toggle switch track.
const TOGGLE_TRACK: egui::Vec2 = vec2(28.0, 16.0);
/// Radius of a pill: half the track height.
const PILL: u8 = 8;

/// A checkbox with a label to its right. Checked is the pressed surface with a strong tick.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let t = Tokens::of(ui.ctx());
    let text = galley(ui, Type::Body, label);
    let label_w = if label.is_empty() {
        0.0
    } else {
        text.size().x + space::SM
    };
    let size = vec2(CHECK_BOX + label_w, height::CONTROL);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    let on = *checked;
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, on, label));
    if ui.is_rect_visible(rect) {
        let look = look(ui, &response, on, false);
        let square = Rect::from_min_size(
            pos2(rect.left(), rect.center().y - CHECK_BOX / 2.0),
            vec2(CHECK_BOX, CHECK_BOX),
        );
        let fill = match look.state {
            ControlState::Pressed | ControlState::Selected => t.bg_active,
            ControlState::Hover => t.bg_hover,
            _ => t.bg_control,
        };
        ui.painter().rect_filled(square, radius::CONTROL, fill);
        if on {
            paint_icon(
                ui,
                Icon::Check,
                square.center(),
                icon_size::CONTROL - 2.0,
                dimmed(t.text_strong, look),
            );
        }
        if !label.is_empty() {
            let ink = if look.state == ControlState::Hover {
                t.text_strong
            } else {
                t.text
            };
            paint_left(
                ui,
                text,
                rect.left() + CHECK_BOX + space::SM,
                rect.center().y,
                dimmed(ink, look),
            );
        }
        if look.focus {
            focus_ring(ui, square, radius::CONTROL, &t);
        }
    }
    response
}

/// A switch with an optional label to its right. On is the pressed surface with the knob at the
/// right in strong colour.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::of(ui.ctx());
    let text = galley(ui, Type::Body, label);
    let label_w = if label.is_empty() {
        0.0
    } else {
        text.size().x + space::SM
    };
    let size = vec2(TOGGLE_TRACK.x + label_w, height::CONTROL);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    let value = *on;
    response.widget_info(|| WidgetInfo::selected(Role::Switch, enabled, value, label));
    if ui.is_rect_visible(rect) {
        let look = look(ui, &response, value, false);
        let track = Rect::from_min_size(
            pos2(rect.left(), rect.center().y - TOGGLE_TRACK.y / 2.0),
            TOGGLE_TRACK,
        );
        let fill = match look.state {
            ControlState::Hover if !value => t.bg_hover,
            ControlState::Pressed | ControlState::Selected | ControlState::Hover => t.bg_active,
            _ => t.bg_control,
        };
        ui.painter().rect_filled(track, PILL, fill);
        let knob_r = TOGGLE_TRACK.y / 2.0 - space::XXS;
        let knob_x = if value {
            track.right() - TOGGLE_TRACK.y / 2.0
        } else {
            track.left() + TOGGLE_TRACK.y / 2.0
        };
        let knob = if value { t.text_strong } else { t.text_weak };
        ui.painter()
            .circle_filled(pos2(knob_x, track.center().y), knob_r, dimmed(knob, look));
        if !label.is_empty() {
            paint_left(
                ui,
                text,
                rect.left() + TOGGLE_TRACK.x + space::SM,
                rect.center().y,
                dimmed(t.text, look),
            );
        }
        if look.focus {
            focus_ring(ui, track, PILL, &t);
        }
    }
    response
}

/// A single-line text input: filled, 24 high, a focus ring while it has focus. `leading` is an
/// icon before the text; `clearable` adds a button that empties the text. The response is the
/// text edit's.
pub fn text_input(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    hint: &str,
    leading: Option<Icon>,
    clearable: bool,
) -> Response {
    let t = Tokens::of(ui.ctx());
    let id: Id = ui.make_persistent_id(id_salt);
    let width = ui.available_width().max(space::XXL * 2.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height::CONTROL), Sense::hover());
    let mut inner = rect.shrink2(vec2(space::SM, 0.0));
    if let Some(icon) = leading {
        paint_icon(
            ui,
            icon,
            pos2(inner.left() + icon_size::CONTROL / 2.0, rect.center().y),
            icon_size::CONTROL,
            t.text_weak,
        );
        inner.min.x += icon_size::CONTROL + space::XS;
    }
    let mut cleared = false;
    if clearable && !text.is_empty() {
        let button = Rect::from_center_size(
            pos2(inner.right() - icon_size::CONTROL / 2.0, rect.center().y),
            vec2(icon_size::CONTROL, icon_size::CONTROL),
        );
        let click = ui.interact(button, id.with("clear"), Sense::click());
        let ink = if click.hovered() {
            t.text_strong
        } else {
            t.text_weak
        };
        paint_icon(
            ui,
            Icon::Close,
            button.center(),
            icon_size::CONTROL - 2.0,
            ink,
        );
        click.widget_info(|| WidgetInfo::labeled(Role::Button, true, "Clear"));
        cleared = click.clicked();
        inner.max.x -= icon_size::CONTROL + space::XS;
    }
    // The fill is added first and set at the end, so it lies behind the text.
    let background = ui.painter().add(egui::Shape::Noop);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let edit = TextEdit::singleline(text)
        .id(id)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(Type::Body.font())
        .hint_text(text_hint(hint))
        .desired_width(inner.width())
        .min_size(vec2(inner.width(), height::CONTROL))
        .align(egui::Align2::LEFT_CENTER)
        .show(&mut child);
    let response = edit.response.response;
    let fill = if response.hovered() && !response.has_focus() {
        t.bg_hover
    } else {
        t.bg_control
    };
    ui.painter().set(
        background,
        egui::Shape::rect_filled(rect, radius::CONTROL, fill),
    );
    if response.has_focus() {
        focus_ring(ui, rect, radius::CONTROL, &t);
    }
    if cleared {
        text.clear();
        response.request_focus();
    }
    response.accessible_name(hint)
}

fn text_hint(hint: &str) -> egui::RichText {
    crate::typography::text(Type::Body, hint).weak()
}

/// A plain text field.
pub fn text_field(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    hint: &str,
) -> Response {
    text_input(ui, id_salt, text, hint, None, false)
}
