//! The slider: a thin track, a round handle and an optional value, in neutral colours.

use std::ops::RangeInclusive;

use egui::{Key, Modifiers, Rect, Response, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};

use crate::metrics::{FOCUS_RING, height, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::field::format_number;
use crate::widgets::paint::{galley, paint_left};
use crate::widgets::state::{ControlState, dimmed, keep_arrows, look, state_edge};

/// Width reserved for the value next to the track.
const VALUE_WIDTH: f32 = 48.0;
/// Diameter of the handle.
const HANDLE: f32 = 12.0;
/// Thickness of the track, edge included.
const TRACK: f32 = 6.0;

/// How a [`slider`] steps and reads out.
#[derive(Clone, Debug)]
pub struct SliderOpts<'a> {
    /// Accessible name.
    pub name: &'a str,
    /// Smallest change by keyboard; a drag snaps to multiples of it when it is above zero.
    pub step: f32,
    /// Digits after the decimal point in the readout.
    pub decimals: usize,
    /// Unit after the readout; empty for none.
    pub unit: &'a str,
    /// Show the value at the right.
    pub show_value: bool,
}

impl Default for SliderOpts<'_> {
    fn default() -> Self {
        Self {
            name: "",
            step: 0.0,
            decimals: 2,
            unit: "",
            show_value: true,
        }
    }
}

/// A slider over `range`. The response is marked changed while the value moves. Left, Right,
/// Home and End work with keyboard focus (Shift: a tenth of the step, Ctrl: ten steps).
pub fn slider(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut f32,
    range: RangeInclusive<f32>,
    opts: &SliderOpts<'_>,
) -> Response {
    let t = Tokens::of(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let (lo, hi) = (*range.start(), *range.end());
    let span = (hi - lo).max(f32::EPSILON);
    let width = ui.available_width().max(space::XXL * 3.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height::CONTROL), Sense::hover());
    let value_w = if opts.show_value {
        VALUE_WIDTH + space::SM
    } else {
        0.0
    };
    let hit = Rect::from_min_max(rect.min, pos2(rect.right() - value_w, rect.bottom()));
    let track = Rect::from_min_max(
        pos2(hit.left() + HANDLE / 2.0, hit.center().y - TRACK / 2.0),
        pos2(hit.right() - HANDLE / 2.0, hit.center().y + TRACK / 2.0),
    );
    let mut response = ui.interact(hit, id, Sense::click_and_drag());
    let enabled = ui.is_enabled();
    let before = *value;

    if enabled && (response.dragged() || response.is_pointer_button_down_on()) {
        if let Some(pos) = response.interact_pointer_pos() {
            let u = ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
            let mut v = lo + u * span;
            if opts.step > 0.0 {
                v = lo + ((v - lo) / opts.step).round() * opts.step;
            }
            *value = v.clamp(lo, hi);
        }
    } else if enabled && response.has_focus() {
        keep_arrows(ui, id, true, true);
        let step = if opts.step > 0.0 {
            opts.step
        } else {
            span / 100.0
        };
        ui.input_mut(|i| {
            for (modifiers, factor) in [
                (Modifiers::NONE, 1.0),
                (Modifiers::SHIFT, 0.1),
                (Modifiers::COMMAND, 10.0),
            ] {
                if i.consume_key(modifiers, Key::ArrowRight)
                    || i.consume_key(modifiers, Key::ArrowUp)
                {
                    *value = (*value + step * factor).min(hi);
                }
                if i.consume_key(modifiers, Key::ArrowLeft)
                    || i.consume_key(modifiers, Key::ArrowDown)
                {
                    *value = (*value - step * factor).max(lo);
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Home) {
                *value = lo;
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                *value = hi;
            }
        });
    }
    if (*value - before).abs() > f32::EPSILON {
        response.mark_changed();
    }
    let shown = *value;
    response.widget_info(|| WidgetInfo::slider(enabled, f64::from(shown), opts.name));

    if ui.is_rect_visible(rect) {
        let look = look(ui, &response, false, false);
        let u = ((*value - lo) / span).clamp(0.0, 1.0);
        let x = track.left() + u * track.width();
        ui.painter().rect_filled(track, TRACK / 2.0, t.bg_control);
        let filled = Rect::from_min_max(track.min, pos2(x, track.max.y));
        ui.painter()
            .rect_filled(filled, TRACK / 2.0, dimmed(t.text_weak, look));
        state_edge(ui, track, radius::CONTROL, &t);
        let centre = pos2(x, hit.center().y);
        let radius = match look.state {
            ControlState::Hover | ControlState::Pressed => HANDLE / 2.0 + 1.0,
            _ => HANDLE / 2.0,
        };
        ui.painter()
            .circle_filled(centre, radius, dimmed(t.text_strong, look));
        if look.focus {
            ui.painter().circle_stroke(
                centre,
                HANDLE / 2.0 + 2.0,
                Stroke::new(FOCUS_RING, t.focus),
            );
        }
        if opts.show_value {
            let mut text = format_number(*value, opts.decimals);
            if !opts.unit.is_empty() {
                text.push(' ');
                text.push_str(opts.unit);
            }
            let label = galley(ui, Type::Body, &text);
            let x = rect.right() - label.size().x;
            paint_left(ui, label, x, rect.center().y, dimmed(t.text, look));
        }
    }
    response
}
