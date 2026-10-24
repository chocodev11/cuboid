//! Tabs, segmented controls and chips.

use egui::{Color32, Id, Key, Modifiers, Rect, Response, Role, Sense, Ui, WidgetInfo, pos2, vec2};

use crate::metrics::{TAB_UNDERLINE, height, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::paint::{galley, paint_centered};
use crate::widgets::state::{ControlState, dimmed, focus_ring, keep_arrows, look};

/// The index to select when Left or Right is pressed while one of `ids` has focus; the focus
/// moves with it. The arrow keys are consumed.
fn arrow_step(ui: &Ui, ids: &[Id]) -> Option<usize> {
    for id in ids {
        keep_arrows(ui, *id, true, false);
    }
    let focused = ids.iter().position(|id| ui.memory(|m| m.has_focus(*id)))?;
    let step: isize = ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
            1
        } else if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
            -1
        } else {
            0
        }
    });
    if step == 0 {
        return None;
    }
    let last = ids.len() - 1;
    let next = focused.saturating_add_signed(step).min(last);
    ui.memory_mut(|m| m.request_focus(ids[next]));
    Some(next)
}

/// A text tab, 28 high, in the title style, with a 2 pt accent underline when selected. For the
/// mode switch and the panel tabs.
pub fn tab_button(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let t = Tokens::of(ui.ctx());
    let text = galley(ui, Type::Title, label);
    let size = vec2(text.size().x + 2.0 * space::MD, height::BAR);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Tab, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        let look = look(ui, &response, selected, false);
        let ink = match look.state {
            ControlState::Selected | ControlState::Pressed | ControlState::Hover => t.text_strong,
            _ => t.text_weak,
        };
        if matches!(look.state, ControlState::Hover | ControlState::Pressed) {
            let fill = if look.state == ControlState::Pressed {
                t.bg_active
            } else {
                t.bg_hover
            };
            ui.painter()
                .rect_filled(rect.shrink2(vec2(0.0, space::XXS)), radius::CONTROL, fill);
        }
        paint_centered(ui, text, rect, dimmed(ink, look));
        if selected {
            let bar = Rect::from_min_max(
                pos2(rect.left() + space::SM, rect.bottom() - TAB_UNDERLINE),
                pos2(rect.right() - space::SM, rect.bottom()),
            );
            ui.painter().rect_filled(bar, 0.0, t.accent);
        }
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
    }
    response
}

/// A row of tabs; Left and Right move between them. Returns whether the selection changed.
pub fn tabs(ui: &mut Ui, current: &mut usize, labels: &[&str]) -> bool {
    let mut changed = false;
    let mut ids = Vec::with_capacity(labels.len());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.horizontal(|ui| {
            for (i, label) in labels.iter().enumerate() {
                let response = tab_button(ui, label, *current == i);
                ids.push(response.id);
                if response.clicked() && *current != i {
                    *current = i;
                    changed = true;
                }
            }
        });
    });
    if let Some(next) = arrow_step(ui, &ids)
        && *current != next
    {
        *current = next;
        changed = true;
    }
    changed
}

/// A row of mutually exclusive text buttons in a well. The selected one takes the pressed surface
/// and strong text, never the accent. Left and Right move the selection. Returns whether the
/// selection changed.
pub fn segmented(ui: &mut Ui, current: &mut usize, labels: &[&str]) -> bool {
    let t = Tokens::of(ui.ctx());
    let inset = space::XXS;
    // Every segment is as wide as it is with the heavier weight, so a choice does not shift them.
    let widths: Vec<f32> = labels
        .iter()
        .map(|label| galley(ui, Type::Strong, label).size().x + 2.0 * space::SM)
        .collect();
    let total = widths.iter().sum::<f32>() + 2.0 * inset;
    let (rect, whole) = ui.allocate_exact_size(vec2(total, height::CONTROL), Sense::hover());
    ui.painter()
        .rect_filled(rect, radius::CONTROL, t.bg_control);
    let mut changed = false;
    let mut ids = Vec::with_capacity(labels.len());
    let mut x = rect.left() + inset;
    for (i, label) in labels.iter().enumerate() {
        let seg = Rect::from_min_size(
            pos2(x, rect.top() + inset),
            vec2(widths[i], height::CONTROL - 2.0 * inset),
        );
        x += widths[i];
        let id = whole.id.with(i);
        ids.push(id);
        let response = ui.interact(seg, id, Sense::click());
        let selected = *current == i;
        let enabled = ui.is_enabled();
        response.widget_info(|| WidgetInfo::selected(Role::RadioButton, enabled, selected, *label));
        if response.clicked() && !selected {
            *current = i;
            changed = true;
        }
        if !ui.is_rect_visible(seg) {
            continue;
        }
        let look = look(ui, &response, selected, false);
        let (fill, role, ink) = match look.state {
            ControlState::Selected | ControlState::Pressed => {
                (t.bg_active, Type::Strong, t.text_strong)
            }
            ControlState::Hover => (t.bg_hover, Type::Body, t.text_strong),
            _ => (Color32::TRANSPARENT, Type::Body, t.text_weak),
        };
        ui.painter().rect_filled(seg, radius::CONTROL, fill);
        paint_centered(ui, galley(ui, role, label), seg, dimmed(ink, look));
        if look.focus {
            focus_ring(ui, seg, radius::CONTROL, &t);
        }
    }
    if let Some(next) = arrow_step(ui, &ids)
        && *current != next
    {
        *current = next;
        changed = true;
    }
    changed
}

/// A filter chip: a toggle with a text label, 24 high. Selected chips take the pressed surface and
/// strong text, never the accent.
pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let t = Tokens::of(ui.ctx());
    let width = galley(ui, Type::Strong, label).size().x + 2.0 * space::SM;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height::CONTROL), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        let look = look(ui, &response, selected, false);
        let (fill, role, ink) = match look.state {
            ControlState::Selected => (t.bg_active, Type::Strong, t.text_strong),
            ControlState::Pressed => (t.bg_active, Type::Body, t.text_strong),
            ControlState::Hover => (t.bg_hover, Type::Body, t.text_strong),
            _ => (t.bg_control, Type::Body, t.text_weak),
        };
        ui.painter().rect_filled(rect, radius::CONTROL, fill);
        paint_centered(ui, galley(ui, role, label), rect, dimmed(ink, look));
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
    }
    response
}
