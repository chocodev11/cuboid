//! Form controls: checkbox, toggle, text field, dropdown, slider and the vector field. Checked
//! and selected states are neutral (pressed surface plus strong text or icon), never the accent.

use egui::{
    Align, Id, Layout, Rect, Response, Role, Sense, Shape, Stroke, TextEdit, Ui, UiBuilder,
    WidgetInfo, pos2, vec2,
};

use crate::icons::Icon;
use crate::metrics::{height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::paint::{galley, paint_centered, paint_icon, paint_left};
use crate::widgets::state::{ControlState, control_fill, dimmed, focus_ring, look, state_edge};

/// Side of the checkbox square.
const CHECK_BOX: f32 = 16.0;
/// Size of the toggle switch track.
const TOGGLE_TRACK: egui::Vec2 = vec2(28.0, 16.0);
/// Radius of a pill: half the track height.
const PILL: u8 = 8;
/// Thickness of the progress bar.
const PROGRESS: f32 = 6.0;

/// Stroke width of the checkbox tick.
const TICK: f32 = 2.0;

/// A checkbox with a label to its right. Checked is the pressed fill with a strong tick; the box
/// has a 1 pt edge so its state shows at 3:1 on the panel.
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
        ui.painter()
            .rect_filled(square, radius::CONTROL, control_fill(&t, look.state));
        state_edge(ui, square, radius::CONTROL, &t);
        if on {
            let c = square.center();
            ui.painter().add(Shape::line(
                vec![
                    pos2(c.x - 3.5, c.y + 0.5),
                    pos2(c.x - 1.0, c.y + 3.0),
                    pos2(c.x + 3.5, c.y - 2.5),
                ],
                Stroke::new(TICK, dimmed(t.text_strong, look)),
            ));
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

/// A switch with an optional label to its right. On is the pressed fill with the knob at the
/// right in strong colour; the track has a 1 pt edge so its state shows at 3:1 on the panel.
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
            ControlState::Hover if !value => t.control_hover,
            ControlState::Pressed | ControlState::Selected | ControlState::Hover => {
                t.control_active
            }
            _ => t.bg_control,
        };
        ui.painter().rect_filled(track, PILL, fill);
        state_edge(ui, track, PILL, &t);
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
    // The fill is reserved first and set at the end, so it lies behind the icons and the text.
    let background = ui.painter().add(egui::Shape::Noop);
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
        t.control_hover
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

/// A text input of several lines in the look of [`text_input`]: filled, `rows` lines high, a
/// focus ring while it has focus; longer text scrolls inside. The response is the text edit's.
pub fn text_area(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    hint: &str,
    rows: usize,
) -> Response {
    let t = Tokens::of(ui.ctx());
    let id: Id = ui.make_persistent_id(id_salt);
    let width = ui.available_width().max(space::XXL * 2.0);
    let lines = Type::Body.line_height() * rows as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(width, lines + 2.0 * space::XS), Sense::hover());
    let background = ui.painter().add(Shape::Noop);
    let inner = rect.shrink2(vec2(space::SM, space::XS));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    let edit = egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .max_height(inner.height())
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            TextEdit::multiline(text)
                .id(id)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(Type::Body.font())
                .hint_text(text_hint(hint))
                .desired_width(inner.width())
                .desired_rows(rows)
                .show(ui)
        })
        .inner;
    let response = edit.response.response;
    let fill = if response.hovered() && !response.has_focus() {
        t.control_hover
    } else {
        t.bg_control
    };
    ui.painter()
        .set(background, Shape::rect_filled(rect, radius::CONTROL, fill));
    if response.has_focus() {
        focus_ring(ui, rect, radius::CONTROL, &t);
    }
    response.accessible_name(hint)
}

/// A field-looking button that opens a search or a palette: a magnifier, the hint in weak text and
/// the shortcut as a chip at the right end. The hint shows only where it fits whole. With no
/// `width` the field is as wide as its magnifier and chip and leaves the hint out, for a narrow
/// bar; the hint is then its tooltip.
pub fn command_field(
    ui: &mut Ui,
    hint: &str,
    shortcut: Option<&str>,
    width: Option<f32>,
) -> Response {
    let t = Tokens::of(ui.ctx());
    let chip = shortcut.map(|shortcut| galley(ui, Type::Caption, shortcut));
    let chip_size = chip
        .as_ref()
        .map(|chip| chip.size() + vec2(2.0 * space::SM - space::XS, 2.0 * space::XXS));
    let natural =
        2.0 * space::SM + icon_size::CONTROL + chip_size.map_or(0.0, |size| space::XS + size.x);
    let width = width.unwrap_or(natural);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height::CONTROL), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, hint));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let look = look(ui, &response, false, false);
    ui.painter()
        .rect_filled(rect, radius::CONTROL, control_fill(&t, look.state));
    let left = rect.left() + space::SM;
    paint_icon(
        ui,
        Icon::Search,
        pos2(left + icon_size::CONTROL / 2.0, rect.center().y),
        icon_size::CONTROL,
        dimmed(t.text_weak, look),
    );
    let mut right = rect.right() - space::SM;
    if let (Some(chip), Some(size)) = (chip, chip_size) {
        let slot = Rect::from_min_size(pos2(right - size.x, rect.center().y - size.y / 2.0), size);
        ui.painter().rect_filled(slot, radius::CONTROL, t.bg_panel);
        paint_centered(ui, chip, slot, t.text_weak);
        right = slot.left() - space::SM;
    }
    let text_x = left + icon_size::CONTROL + space::XS;
    let hint_text = galley(ui, Type::Body, hint);
    let fits = hint_text.size().x <= right - text_x;
    if fits {
        paint_left(
            ui,
            hint_text,
            text_x,
            rect.center().y,
            dimmed(t.text_weak, look),
        );
    }
    if look.focus {
        focus_ring(ui, rect, radius::CONTROL, &t);
    }
    if fits {
        response
    } else {
        response.on_hover_text(hint)
    }
}

/// A progress bar of 6 pt: the accent over a field-coloured track. `None` is a bar of unknown
/// length that sweeps to and fro; it asks for a repaint while it is shown.
pub fn progress(ui: &mut Ui, fraction: Option<f32>, width: f32) -> Response {
    let t = Tokens::of(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(width, PROGRESS), Sense::hover());
    response.widget_info(|| WidgetInfo::new(Role::ProgressIndicator));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    ui.painter()
        .rect_filled(rect, radius::CONTROL, t.bg_control);
    let (from, to) = match fraction {
        Some(f) => (0.0, f.clamp(0.0, 1.0)),
        None => {
            ui.ctx().request_repaint();
            // A segment of 30 % that bounces; the phase runs 0..2 and folds back at 1.
            let phase = (ui.input(|i| i.time) * 0.8).rem_euclid(2.0);
            #[allow(clippy::cast_possible_truncation)]
            let start = (if phase < 1.0 { phase } else { 2.0 - phase }) as f32 * 0.7;
            (start, start + 0.3)
        }
    };
    if to > from {
        let bar = Rect::from_min_max(
            pos2(rect.left() + rect.width() * from, rect.top()),
            pos2(rect.left() + rect.width() * to, rect.bottom()),
        );
        ui.painter().rect_filled(bar, radius::CONTROL, t.accent);
    }
    response
}

#[cfg(test)]
mod tests {
    use egui::{Context, RawInput, Shape};

    use super::*;
    use crate::fonts::{FontSet, install_fonts};

    /// The index of the first shape that `is` accepts among the shapes of one frame.
    fn first(shapes: &[egui::epaint::ClippedShape], is: impl Fn(&Shape) -> bool) -> Option<usize> {
        shapes.iter().position(|clipped| is(&clipped.shape))
    }

    #[test]
    fn the_icons_of_a_text_input_lie_over_its_fill() {
        let ctx = Context::default();
        install_fonts(&ctx, &FontSet::default());
        let mut typed = "orbit".to_owned();
        let mut frame = || {
            ctx.run_ui(RawInput::default(), |ui| {
                text_input(ui, "find", &mut typed, "Find", Some(Icon::Search), true);
            })
        };
        frame().drop_without_applying_deltas();
        let output = frame();
        let fill = Tokens::of(&ctx).bg_control;
        let rect = first(
            &output.shapes,
            |shape| matches!(shape, Shape::Rect(rect) if rect.fill == fill),
        );
        let icons = |glyph: char| {
            first(
                &output.shapes,
                |shape| matches!(shape, Shape::Text(text) if text.galley.text() == glyph.to_string()),
            )
        };
        let fill_at = rect.expect("the fill is painted");
        for icon in [Icon::Search, Icon::Close] {
            let at = icons(icon.glyph()).expect("the icon is painted");
            assert!(fill_at < at, "{icon:?} is hidden behind the fill");
        }
    }
}
