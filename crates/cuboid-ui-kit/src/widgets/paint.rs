//! Small painting helpers shared by the components.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, Galley, Pos2, Rect, Ui, vec2};

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
