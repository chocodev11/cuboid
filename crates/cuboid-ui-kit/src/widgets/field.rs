//! The numeric drag field: drag sideways to change, click to type.

use std::ops::RangeInclusive;

use egui::text::{CCursor, CCursorRange};
use egui::{
    Align2, Color32, CursorIcon, FontId, Key, Rect, Response, Sense, Stroke, StrokeKind, TextEdit,
    Ui, WidgetInfo, vec2,
};

use crate::theme::{RADIUS, RADIUS_SM, Tokens};

/// How a [`drag_field`] behaves and looks.
#[derive(Clone, Debug)]
pub struct DragFieldOpts<'a> {
    /// Accessible name, such as "Position X".
    pub name: &'a str,
    /// Value change per dragged point; Shift divides it by 10, Ctrl multiplies it by 10.
    pub speed: f32,
    /// Allowed values; typed and dragged values are clamped to it.
    pub range: RangeInclusive<f32>,
    /// Unit text shown after the value, such as "px" or a degree sign; empty for none.
    pub unit: &'a str,
    /// Most digits after the decimal point; trailing zeros are dropped.
    pub decimals: usize,
    /// Drag results snap to multiples of this (Alt turns it off for one drag).
    pub snap: Option<f32>,
    /// A short label inside the left edge (an axis letter) and its colour.
    pub label: Option<(&'a str, Color32)>,
    /// Width of the field; `None` takes the available width.
    pub width: Option<f32>,
}

impl Default for DragFieldOpts<'_> {
    fn default() -> Self {
        Self {
            name: "",
            speed: 0.1,
            range: f32::MIN..=f32::MAX,
            unit: "",
            decimals: 3,
            snap: None,
            label: None,
            width: Some(78.0),
        }
    }
}

/// What a [`drag_field`] did this frame.
#[derive(Clone, Debug)]
pub struct FieldResponse {
    /// The field's response.
    pub response: Response,
    /// The value changed this frame (each drag step, or a typed value).
    pub changed: bool,
    /// The edit ended this frame: the drag was released or the typed value was accepted. A
    /// preview consumer applies the value on every `changed`; an undo consumer on `committed`.
    pub committed: bool,
}

#[derive(Clone, Debug, Default)]
struct FieldState {
    editing: bool,
    selected_all: bool,
    buffer: String,
    drag_start: f32,
    drag_total: f32,
    drag_value: Option<f32>,
}

/// Formats `value` with at most `decimals` digits, dropping trailing zeros and a negative zero.
pub fn format_number(value: f32, decimals: usize) -> String {
    let mut text = format!("{value:.decimals$}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text.remove(0);
    }
    text
}

/// Parses typed text: a number, a fraction like `1/16`, with a trailing `unit` and a decimal
/// comma allowed. Returns `None` for anything else, including NaN and infinity.
pub fn parse_number(text: &str, unit: &str) -> Option<f32> {
    let mut text = text.trim();
    if !unit.is_empty() {
        text = text.strip_suffix(unit).unwrap_or(text).trim_end();
    }
    let normalized = text.replace(',', ".");
    let value = match normalized.split_once('/') {
        Some((numerator, denominator)) => {
            let denominator: f32 = denominator.trim().parse().ok()?;
            if denominator.abs() < f32::EPSILON {
                return None;
            }
            numerator.trim().parse::<f32>().ok()? / denominator
        }
        None => normalized.parse::<f32>().ok()?,
    };
    value.is_finite().then_some(value)
}

fn snap_to(value: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return value;
    }
    (value / step).round() * step
}

/// A numeric field in the style of a 3D editor: dragging changes the value, a plain click opens
/// a text box (Enter accepts, Escape cancels). While a drag or a text edit is active the field
/// shows its own value, so a caller that re-reads the document every frame does not make it
/// jump back.
pub fn drag_field(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut f32,
    opts: &DragFieldOpts<'_>,
) -> FieldResponse {
    let tokens = Tokens::of(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let mut state: FieldState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let (lo, hi) = (*opts.range.start(), *opts.range.end());

    let width = opts.width.unwrap_or_else(|| ui.available_width()).max(40.0);
    let size = vec2(width, 24.0);
    let enabled = ui.is_enabled();
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut changed = false;
    let mut committed = false;

    if let Some(v) = state.drag_value {
        *value = v;
    }

    let response = if state.editing {
        paint_box(ui, rect, &tokens, true, opts);
        let text_rect = text_area(rect, opts);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
        let escape = child.input(|i| i.key_pressed(Key::Escape));
        let edit_id = id.with("edit");
        let mut output = TextEdit::singleline(&mut state.buffer)
            .id(edit_id)
            .frame(egui::Frame::NONE)
            .align(Align2::CENTER_CENTER)
            .font(FontId::proportional(13.0))
            .desired_width(text_rect.width())
            .show(&mut child);
        if !state.selected_all {
            output.response.request_focus();
            let len = state.buffer.chars().count();
            output
                .state
                .cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
            output.state.store(child.ctx(), edit_id);
            state.selected_all = true;
        } else if output.response.lost_focus() {
            if !escape && let Some(v) = parse_number(&state.buffer, opts.unit) {
                let v = v.clamp(lo, hi);
                changed = (v - *value).abs() > f32::EPSILON;
                *value = v;
                committed = true;
            }
            state.editing = false;
            state.selected_all = false;
        }
        output.response.response.clone().accessible_name(opts.name)
    } else {
        let response = ui.interact(rect, id, Sense::click_and_drag());
        if response.drag_started() {
            state.drag_start = *value;
            state.drag_total = 0.0;
        }
        if response.dragged() {
            state.drag_total += response.drag_delta().x;
            let (shift, ctrl, alt) =
                ui.input(|i| (i.modifiers.shift, i.modifiers.command, i.modifiers.alt));
            let factor = if shift {
                0.1
            } else if ctrl {
                10.0
            } else {
                1.0
            };
            let mut v = state.drag_start + state.drag_total * opts.speed * factor;
            if let (Some(step), false) = (opts.snap, alt) {
                v = snap_to(v, step);
            }
            let v = v.clamp(lo, hi);
            if (v - *value).abs() > f32::EPSILON {
                *value = v;
                changed = true;
            }
            state.drag_value = Some(*value);
        }
        if response.drag_stopped() {
            state.drag_value = None;
            committed = true;
        }
        if response.clicked() {
            state.editing = true;
            state.selected_all = false;
            state.buffer = format_number(*value, opts.decimals);
        }
        if enabled && response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        let hot = response.hovered() || response.dragged();
        paint_box(ui, rect, &tokens, hot, opts);
        let shown = if opts.unit.is_empty() {
            format_number(*value, opts.decimals)
        } else {
            format!("{} {}", format_number(*value, opts.decimals), opts.unit)
        };
        let color = if enabled {
            tokens.text_strong
        } else {
            tokens.text_weak
        };
        let area = text_area(rect, opts);
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            shown,
            FontId::proportional(13.0),
            color,
        );
        let (name, v) = (opts.name, f64::from(*value));
        response.widget_info(|| {
            let mut info = WidgetInfo::drag_value(enabled, v);
            info.label = Some(name.to_owned());
            info
        });
        response
    };

    ui.data_mut(|d| d.insert_temp(id, state));
    FieldResponse {
        response,
        changed,
        committed,
    }
}

fn text_area(rect: Rect, opts: &DragFieldOpts<'_>) -> Rect {
    if opts.label.is_some() {
        Rect::from_min_max(egui::pos2(rect.left() + 20.0, rect.top()), rect.max)
    } else {
        rect
    }
}

fn paint_box(ui: &Ui, rect: Rect, tokens: &Tokens, hot: bool, opts: &DragFieldOpts<'_>) {
    let painter = ui.painter();
    let stroke = if hot {
        Stroke::new(1.0, tokens.focus)
    } else {
        Stroke::new(1.0, tokens.border_strong)
    };
    painter.rect(rect, RADIUS, tokens.bg_input, stroke, StrokeKind::Inside);
    if let Some((text, color)) = opts.label {
        let chip = Rect::from_min_size(rect.min, vec2(16.0, rect.height()));
        painter.rect_filled(chip.shrink(1.0), RADIUS_SM, color.gamma_multiply(0.22));
        painter.text(
            chip.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(11.5),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_drop_trailing_zeros_and_negative_zero() {
        assert_eq!(format_number(1.5, 3), "1.5");
        assert_eq!(format_number(2.0, 3), "2");
        assert_eq!(format_number(-0.0001, 3), "0");
        assert_eq!(format_number(0.0626, 3), "0.063");
        assert_eq!(format_number(-12.25, 2), "-12.25");
        assert_eq!(format_number(7.0, 0), "7");
    }

    #[test]
    fn typed_text_accepts_units_commas_and_fractions() {
        assert_eq!(parse_number("12", ""), Some(12.0));
        assert_eq!(parse_number(" 1,5 ", ""), Some(1.5));
        assert_eq!(parse_number("45 deg", "deg"), Some(45.0));
        assert_eq!(parse_number("1/16", ""), Some(0.0625));
        assert_eq!(parse_number("-3/2", ""), Some(-1.5));
    }

    #[test]
    fn bad_text_is_rejected() {
        assert_eq!(parse_number("", ""), None);
        assert_eq!(parse_number("abc", ""), None);
        assert_eq!(parse_number("1/0", ""), None);
        assert_eq!(parse_number("NaN", ""), None);
        assert_eq!(parse_number("inf", ""), None);
        assert_eq!(parse_number("1e999", ""), None);
    }

    #[test]
    fn snapping_rounds_to_the_step() {
        assert!((snap_to(0.07, 0.0625) - 0.0625).abs() < 1e-6);
        assert!((snap_to(-0.2, 0.25) + 0.25).abs() < 1e-6);
        assert!((snap_to(0.3, 0.0) - 0.3).abs() < 1e-6);
    }
}
