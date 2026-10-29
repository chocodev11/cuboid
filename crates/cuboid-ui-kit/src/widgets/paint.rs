//! Small painting helpers shared by the components.

use std::borrow::Cow;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, Galley, Painter, Pos2, Rect, TextureId, Ui, Vec2, pos2, vec2};

use crate::icons::Icon;
use crate::typography::Type;

/// Characters of a text that are laid out; what follows is cut for display.
const DISPLAY_CHARS: usize = 256;

/// `text` as it is shown on one line: cut after 256 characters with an ellipsis, so a label of
/// millions of characters costs no more to hash and lay out than a normal one. Only the display is
/// cut; the caller keeps the full text. Shorter text is returned as it is, without a scan.
pub fn display_cut(text: &str) -> Cow<'_, str> {
    // A character takes at least one byte, so a text of 256 bytes or less has no more characters.
    if text.len() <= DISPLAY_CHARS {
        return Cow::Borrowed(text);
    }
    match text.char_indices().nth(DISPLAY_CHARS) {
        Some((end, _)) => Cow::Owned(format!("{}\u{2026}", &text[..end])),
        None => Cow::Borrowed(text),
    }
}

/// Lays out one line of text in a role. The colour is chosen when it is painted.
pub(crate) fn galley(ui: &Ui, role: Type, text: &str) -> Arc<Galley> {
    ui.painter().layout_no_wrap(
        display_cut(text).into_owned(),
        role.font(),
        Color32::PLACEHOLDER,
    )
}

/// Lays out one line of text in a role, cut with an ellipsis where it is wider than `width`. The
/// colour is chosen when it is painted.
pub(crate) fn galley_cut(ui: &Ui, role: Type, text: &str, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        display_cut(text).into_owned(),
        TextFormat {
            font_id: role.font(),
            color: Color32::PLACEHOLDER,
            line_height: Some(role.line_height()),
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
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
    let galley = galley_cut(ui, role, text, clip.right() - x);
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
    use egui::{Context, RawInput};

    use super::*;
    use crate::fonts::{FontSet, install_fonts};

    #[test]
    fn a_long_text_is_cut_for_display_and_the_text_itself_is_unchanged() {
        let exact = "a".repeat(DISPLAY_CHARS);
        assert!(matches!(display_cut(&exact), Cow::Borrowed(_)));
        // Multi-byte characters: 256 of them are 768 bytes, and the cut must not split one.
        let wide = "\u{1ebf}".repeat(DISPLAY_CHARS);
        assert!(matches!(display_cut(&wide), Cow::Borrowed(_)));

        let label = format!("{}tail", "\u{1ebf}".repeat(DISPLAY_CHARS));
        let shown = display_cut(&label);
        assert_eq!(shown.chars().count(), DISPLAY_CHARS + 1);
        assert_eq!(
            shown,
            format!("{}\u{2026}", "\u{1ebf}".repeat(DISPLAY_CHARS))
        );
        assert!(label.ends_with("tail"));

        // A 50 MB label is cut after a scan of 256 characters.
        let huge = "x".repeat(50_000_000);
        assert_eq!(display_cut(&huge).chars().count(), DISPLAY_CHARS + 1);
        assert_eq!(huge.len(), 50_000_000);
    }

    #[test]
    fn the_galleys_lay_out_the_cut_text_only() {
        let ctx = Context::default();
        install_fonts(&ctx, &FontSet::default());
        let huge = "x".repeat(5_000_000);
        let mut counts = Vec::new();
        ctx.run_ui(RawInput::default(), |ui| {
            counts.push(galley(ui, Type::Body, &huge).text().chars().count());
            counts.push(
                galley_cut(ui, Type::Body, &huge, 120.0)
                    .text()
                    .chars()
                    .count(),
            );
        })
        .drop_without_applying_deltas();
        assert_eq!(counts, [DISPLAY_CHARS + 1; 2]);
    }

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
