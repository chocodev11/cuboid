//! Small painting helpers shared by the components.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, Galley, Painter, Pos2, Rect, TextureId, Ui, Vec2, pos2, vec2};

use crate::icons::Icon;
use crate::typography::Type;

/// Lays out one line of text in a role. The colour is chosen when it is painted.
pub(crate) fn galley(ui: &Ui, role: Type, text: &str) -> Arc<Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), role.font(), Color32::PLACEHOLDER)
}

/// Paints a laid-out line with its left edge at `x` and its middle at `y`.
pub(crate) fn paint_left(ui: &Ui, galley: Arc<Galley>, x: f32, y: f32, color: Color32) {
    let top_left = Pos2::new(x, y - galley.size().y / 2.0);
    ui.painter().galley(top_left, galley, color);
}

/// Paints a laid-out line centred in `rect`.
pub(crate) fn paint_centered(ui: &Ui, galley: Arc<Galley>, rect: Rect, color: Color32) {
    let top_left = rect.center() - galley.size() / 2.0;
    ui.painter().galley(top_left, galley, color);
}

/// Paints one line of text in a role with its left edge at `x`, centred on the middle of `clip`
/// and cut with an ellipsis at the right edge of `clip`.
pub(crate) fn paint_clipped(ui: &Ui, role: Type, text: &str, clip: Rect, x: f32, color: Color32) {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: role.font(),
            color,
            line_height: Some(role.line_height()),
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping::truncate_at_width((clip.right() - x).max(0.0));
    let galley = ui.painter().layout_job(job);
    let top_left = Pos2::new(x, clip.center().y - galley.size().y / 2.0);
    ui.painter().galley(top_left, galley, color);
}

/// Paints an icon of `size` centred at `center`.
pub(crate) fn paint_icon(ui: &Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    icon.paint(
        ui.painter(),
        Rect::from_center_size(center, vec2(size, size)),
        color,
    );
}

/// Paints a whole texture into `rect`, without a tint.
pub fn paint_image(painter: &Painter, texture: TextureId, rect: Rect) {
    painter.image(
        texture,
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// The largest rectangle with the aspect of `size` that fits inside `area`, centred in it.
pub fn fit_centered(area: Rect, size: Vec2) -> Rect {
    let scale = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_fitted_and_centred_in_its_frame() {
        let frame = Rect::from_min_size(pos2(10.0, 20.0), vec2(176.0, 132.0));
        // A 3:2 picture in a 4:3 frame fills the width and leaves equal bands above and below.
        let wide = fit_centered(frame, vec2(96.0, 64.0));
        assert!((wide.width() - 176.0).abs() < 1e-3);
        assert!((wide.height() - 176.0 * 64.0 / 96.0).abs() < 1e-3);
        assert!((wide.top() - frame.top() - (frame.bottom() - wide.bottom())).abs() < 1e-3);
        assert!(wide.top() > frame.top());
        // A tall picture fills the height and leaves equal bands at the sides.
        let tall = fit_centered(frame, vec2(32.0, 64.0));
        assert!((tall.height() - 132.0).abs() < 1e-3);
        assert!((tall.left() - frame.left() - (frame.right() - tall.right())).abs() < 1e-3);
        assert_eq!(tall.center(), frame.center());
    }
}
