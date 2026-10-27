//! Tabs, segmented controls and chips.

use egui::{
    Align, Color32, Id, Key, Modifiers, Rect, Response, Role, Sense, Ui, WidgetInfo, pos2, vec2,
};

use crate::icons::Icon;
use crate::metrics::{TAB_ICON_WIDTH, TAB_UNDERLINE, height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::button::{Tone, badge_colors};
use crate::widgets::paint::{galley, galley_cut, paint_centered, paint_icon};
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

/// Draws a horizontal tab into `rect`: the hover fill, the content in its ink, the underline of the
/// selected tab and the focus ring.
fn paint_tab(
    ui: &Ui,
    rect: Rect,
    response: &Response,
    selected: bool,
    content: impl FnOnce(Color32),
) {
    let t = Tokens::of(ui.ctx());
    let look = look(ui, response, selected, false);
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
    content(dimmed(ink, look));
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

/// The width of a [`tab_button`] with this label.
pub fn tab_width(ui: &Ui, label: &str) -> f32 {
    galley(ui, Type::Title, label).size().x + 2.0 * space::MD
}

/// A text tab, 28 high, in the title style, with a 2 pt accent underline when selected. For the
/// mode switch and the panel tabs.
pub fn tab_button(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let text = galley(ui, Type::Title, label);
    let size = vec2(tab_width(ui, label), height::BAR);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Tab, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        paint_tab(ui, rect, &response, selected, |ink| {
            paint_centered(ui, text, rect, ink);
        });
    }
    response
}

/// A [`tab_button`] no wider than `largest`: a title that does not fit is cut with an ellipsis, and
/// the tooltip and the accessible name stay the whole title. For a row of tabs that is too narrow
/// for its words.
pub fn tab_button_max(ui: &mut Ui, label: &str, selected: bool, largest: f32) -> Response {
    let natural = tab_width(ui, label);
    if natural <= largest {
        return tab_button(ui, label, selected);
    }
    let text = galley_cut(ui, Type::Title, label, largest - 2.0 * space::MD);
    let (rect, response) = ui.allocate_exact_size(vec2(largest, height::BAR), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Tab, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        paint_tab(ui, rect, &response, selected, |ink| {
            paint_centered(ui, text, rect, ink);
        });
    }
    response.on_hover_text(label)
}

/// A tab that shows an icon in place of its label, for a row of tabs that is too narrow for
/// words. The tooltip is the label and the accessible name.
pub fn tab_icon(ui: &mut Ui, icon: Icon, tip: &str, selected: bool) -> Response {
    let size = vec2(TAB_ICON_WIDTH, height::BAR);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Tab, enabled, selected, tip));
    if ui.is_rect_visible(rect) {
        paint_tab(ui, rect, &response, selected, |ink| {
            paint_icon(ui, icon, rect.center(), icon_size::CONTROL, ink);
        });
    }
    response.on_hover_text(tip)
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
                (t.control_active, Type::Strong, t.text_strong)
            }
            ControlState::Hover => (t.control_hover, Type::Body, t.text_strong),
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
            ControlState::Selected => (t.control_active, Type::Strong, t.text_strong),
            ControlState::Pressed => (t.control_active, Type::Body, t.text_strong),
            ControlState::Hover => (t.control_hover, Type::Body, t.text_strong),
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

/// A tab of a vertical strip: a 16 pt icon in a 36 by 32 cell with a 2 pt accent bar at the edge
/// of the strip while selected. The tooltip is the accessible name.
pub fn icon_tab(ui: &mut Ui, icon: Icon, tip: &str, selected: bool) -> Response {
    icon_tab_badged(ui, icon, tip, selected, None, Align::Min)
}

/// An [`icon_tab`] with a count at the top right of the icon, such as the issues of the validator.
/// A count of zero shows no badge and the count shows up to 99; the tooltip names it. `side` is the
/// edge of the cell that carries the selection bar: `Align::Min` the left one, `Align::Max` the
/// right one, for a strip that stands at the right of its page.
pub fn icon_tab_badged(
    ui: &mut Ui,
    icon: Icon,
    tip: &str,
    selected: bool,
    badge: Option<(usize, Tone)>,
    side: Align,
) -> Response {
    let t = Tokens::of(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(height::STRIP, height::ACTION), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Tab, enabled, selected, tip));
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
            ui.painter().rect_filled(
                rect.shrink2(vec2(space::XS, space::XXS)),
                radius::CONTROL,
                fill,
            );
        }
        paint_icon(
            ui,
            icon,
            rect.center(),
            icon_size::CONTROL,
            dimmed(ink, look),
        );
        if selected {
            let edge = if side == Align::Max {
                rect.right() - TAB_UNDERLINE
            } else {
                rect.left()
            };
            let bar = Rect::from_min_max(
                pos2(edge, rect.top() + space::XS),
                pos2(edge + TAB_UNDERLINE, rect.bottom() - space::XS),
            );
            ui.painter().rect_filled(bar, 0.0, t.accent);
        }
        if let Some((count, tone)) = badge.filter(|(count, _)| *count > 0) {
            let text = if count > 99 {
                "99+".to_owned()
            } else {
                count.to_string()
            };
            let label = galley(ui, Type::Caption, &text);
            let size = label.size() + vec2(2.0 * space::XS, 0.0);
            let size = vec2(size.x.max(size.y), size.y);
            let pill = Rect::from_min_size(
                pos2(
                    rect.center().x + icon_size::CONTROL / 2.0 - size.x + space::XS,
                    rect.center().y - icon_size::CONTROL / 2.0 - space::XS,
                ),
                size,
            );
            let (fill, ink) = badge_colors(&t, tone);
            ui.painter().rect_filled(pill, radius::CONTROL, fill);
            paint_centered(ui, label, pill, ink);
        }
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
    }
    let tip = match badge {
        Some((count, _)) if count > 0 => format!("{tip}: {count}"),
        _ => tip.to_owned(),
    };
    response.on_hover_text(tip)
}
