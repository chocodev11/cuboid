//! Colour swatches.

use egui::{Color32, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2};

use crate::metrics::{height, radius};
use crate::theme::Tokens;
use crate::widgets::state::{focus_ring, look};

/// A rounded colour chip. Translucent colours show a checker behind them. Selected swatches get
/// an accent ring (the selection tint).
pub fn color_swatch(ui: &mut Ui, color: Color32, size: Vec2, selected: bool) -> Response {
    let tokens = Tokens::of(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let painter = ui.painter();
    if color.a() < 255 {
        checker(painter, rect, &tokens);
    }
    painter.rect_filled(rect, radius::CONTROL, color);
    let outline = if selected {
        Stroke::new(2.0, tokens.accent)
    } else if resp.hovered() {
        Stroke::new(1.5, tokens.text_strong)
    } else {
        Stroke::new(1.0, tokens.border_strong)
    };
    painter.rect_stroke(rect, radius::CONTROL, outline, StrokeKind::Inside);
    if look(ui, &resp, selected, false).focus {
        focus_ring(ui, rect, radius::CONTROL, &tokens);
    }
    resp
}

fn checker(painter: &Painter, rect: Rect, tokens: &Tokens) {
    const CELL: f32 = 4.0;
    let (a, b) = if tokens.dark {
        (Color32::from_gray(70), Color32::from_gray(100))
    } else {
        (Color32::from_gray(235), Color32::from_gray(200))
    };
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, radius::CONTROL, a);
    let mut odd_row = false;
    let mut y = rect.top();
    while y < rect.bottom() {
        let mut odd = odd_row;
        let mut x = rect.left();
        while x < rect.right() {
            if odd {
                painter.rect_filled(Rect::from_min_size(pos2(x, y), Vec2::splat(CELL)), 0.0, b);
            }
            odd = !odd;
            x += CELL;
        }
        odd_row = !odd_row;
        y += CELL;
    }
}

/// What a [`color_edit`] did this frame.
#[derive(Clone, Debug)]
pub struct ColorEditResponse {
    /// The swatch button.
    pub response: Response,
    /// The colour changed this frame (a preview while the picker is dragged).
    pub changed: bool,
    /// The change ended: the pointer was released after a change.
    pub committed: bool,
}

/// A swatch that opens a colour picker popup when clicked. `alpha` shows the opacity slider.
pub fn color_edit(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    color: &mut Color32,
    alpha: bool,
) -> ColorEditResponse {
    let id = ui.make_persistent_id(id_salt).with("pending");
    let response = color_swatch(ui, *color, Vec2::new(40.0, height::CONTROL), false);
    let mut changed = false;
    egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mode = if alpha {
                egui::color_picker::Alpha::OnlyBlend
            } else {
                egui::color_picker::Alpha::Opaque
            };
            changed = egui::color_picker::color_picker_color32(ui, color, mode);
        });
    let mut pending = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    pending |= changed;
    let committed = pending && !ui.input(|i| i.pointer.any_down());
    if committed {
        pending = false;
    }
    ui.data_mut(|d| d.insert_temp(id, pending));
    ColorEditResponse {
        response,
        changed,
        committed,
    }
}

/// The colour of user data given as bytes, such as a palette entry or an accent preset.
pub fn rgb_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
