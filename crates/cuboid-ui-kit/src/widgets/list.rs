//! The list row: leading icon or thumbnail, title, trailing caption.

use egui::{
    Color32, Rect, Response, Role, Sense, TextureId, Ui, Vec2, Widget, WidgetInfo, pos2, vec2,
};

use crate::icons::Icon;
use crate::metrics::{height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::Type;
use crate::widgets::button::{Tone, badge_colors};
use crate::widgets::paint::{
    fit_centered, galley, paint_centered, paint_clipped, paint_icon, paint_image,
};
use crate::widgets::state::{ControlState, dimmed, focus_ring, look};

/// A full-width row of 28 pt, or 44 pt with a thumbnail. Hover and selected states are drawn; the
/// caller adds the context menu on the response: `ui.add(ListRow::new("Lantern")).context_menu(..)`.
#[derive(Clone, Debug)]
#[must_use = "add the row with `ui.add`"]
pub struct ListRow<'a> {
    title: &'a str,
    icon: Option<Icon>,
    thumbnail: Option<(TextureId, Vec2)>,
    picture_icon: Option<Icon>,
    caption: Option<&'a str>,
    shortcut: Option<&'a str>,
    selected: bool,
    dimmed: bool,
    draggable: bool,
}

impl<'a> ListRow<'a> {
    /// A row with a title.
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            icon: None,
            thumbnail: None,
            picture_icon: None,
            caption: None,
            shortcut: None,
            selected: false,
            dimmed: false,
            draggable: false,
        }
    }

    /// A 16 pt icon before the title.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A picture before the title, fitted into a square; makes the row 44 pt high. `size` is the
    /// picture's size in pixels.
    pub fn thumbnail(mut self, texture: TextureId, size: Vec2) -> Self {
        self.thumbnail = Some((texture, size));
        self
    }

    /// An icon in the place of the picture, for an entry that has none yet; makes the row 44 pt
    /// high like a row with a picture, so a list keeps one row height.
    pub fn picture_icon(mut self, icon: Icon) -> Self {
        self.picture_icon = Some(icon);
        self
    }

    /// A shortcut as a key chip at the right end, before the caption.
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Weak text at the right end.
    pub fn caption(mut self, caption: &'a str) -> Self {
        self.caption = Some(caption);
        self
    }

    /// The row is part of the selection: the accent tint and a strong title.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Draws the text weak, for a row that is out of effect (an undone step) but still works.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    /// The row can be dragged as well as clicked.
    pub fn draggable(mut self) -> Self {
        self.draggable = true;
        self
    }
}

impl Widget for ListRow<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let t = Tokens::of(ui.ctx());
        let pictured = self.thumbnail.is_some() || self.picture_icon.is_some();
        let row_h = if pictured {
            height::ROW_THUMB
        } else {
            height::ROW
        };
        let sense = if self.draggable {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), sense);
        let enabled = ui.is_enabled();
        let selected = self.selected;
        response
            .widget_info(|| WidgetInfo::selected(Role::ListItem, enabled, selected, self.title));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let look = look(ui, &response, self.selected, false);
        let fill = match look.state {
            ControlState::Selected => t.accent_soft,
            ControlState::Hover => t.bg_hover,
            ControlState::Pressed => t.bg_active,
            _ => Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(rect, radius::CONTROL, fill);

        let mut x = rect.left() + space::SM;
        if pictured {
            let side = row_h - 2.0 * space::XS;
            let slot = Rect::from_min_size(
                pos2(x - space::XS, rect.top() + space::XS),
                Vec2::splat(side),
            );
            ui.painter().rect_filled(slot, radius::CONTROL, t.bg_input);
            match (self.thumbnail, self.picture_icon) {
                (Some((texture, size)), _) if size.x > 0.0 && size.y > 0.0 => {
                    paint_image(ui.painter(), texture, fit_centered(slot, size));
                }
                (None, Some(icon)) => {
                    paint_icon(ui, icon, slot.center(), icon_size::TOOLBAR, t.text_weak);
                }
                _ => {}
            }
            x = slot.right() + space::SM;
        } else if let Some(icon) = self.icon {
            let ink = if self.selected {
                t.text_strong
            } else {
                t.text_weak
            };
            paint_icon(
                ui,
                icon,
                pos2(x + icon_size::CONTROL / 2.0, rect.center().y),
                icon_size::CONTROL,
                dimmed(ink, look),
            );
            x += icon_size::CONTROL + space::SM;
        }

        let mut right = rect.right() - space::SM;
        if let Some(shortcut) = self.shortcut {
            let g = galley(ui, Type::Caption, shortcut);
            let size = g.size() + vec2(2.0 * space::SM - space::XS, 2.0 * space::XXS);
            let chip =
                Rect::from_min_size(pos2(right - size.x, rect.center().y - size.y / 2.0), size);
            let (fill, ink) = badge_colors(&t, Tone::Neutral);
            ui.painter().rect_filled(chip, radius::CONTROL, fill);
            paint_centered(ui, g, chip, dimmed(ink, look));
            right = chip.left() - space::SM;
        }
        if let Some(caption) = self.caption {
            let g = galley(ui, Type::Caption, caption);
            let left = right - g.size().x;
            ui.painter().galley(
                pos2(left, rect.center().y - g.size().y / 2.0),
                g,
                dimmed(t.text_weak, look),
            );
            right = left - space::SM;
        }
        let (role, ink) = if self.selected {
            (Type::Strong, t.text_strong)
        } else if look.state == ControlState::Hover {
            (Type::Body, t.text_strong)
        } else if self.dimmed {
            (Type::Body, t.text_weak)
        } else {
            (Type::Body, t.text)
        };
        let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right.max(x), rect.bottom()));
        paint_clipped(ui, role, self.title, clip, x, dimmed(ink, look));
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
        response
    }
}
