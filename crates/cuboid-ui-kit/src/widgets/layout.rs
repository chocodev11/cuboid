//! Structure components: panel frame, section, property grid, toolbar, card and empty state.

use egui::{
    Align, Frame, Id, Label, Layout, Margin, Rect, Response, Role, ScrollArea, Sense, Shape,
    Stroke, TextureId, Ui, UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};

use crate::icons::Icon;
use crate::metrics::{gutter, height, icon_size, label_column, radius, space};
use crate::theme::Tokens;
use crate::typography::{Type, text};
use crate::widgets::button::{Button, IconButton, Tone, badge};
use crate::widgets::paint::{
    fit_centered, galley, paint_clipped, paint_icon, paint_image, paint_left,
};
use crate::widgets::state::{ControlState, dimmed, focus_ring, look};

/// Draws the left of a panel header: tabs in place of the icon and the title.
type HeaderTabs<'a> = Box<dyn FnOnce(&mut Ui) + 'a>;

/// A panel with a 28 pt header and a padded, scrolling body:
///
/// ```ignore
/// PanelFrame::new("Outliner").icon(Icon::Outliner).show(ui, |ui| { /* header buttons */ }, |ui| { /* body */ });
/// ```
#[must_use = "call `show`"]
pub struct PanelFrame<'a> {
    title: &'a str,
    icon: Option<Icon>,
    collapsible: bool,
    scroll: bool,
    flush: bool,
    tabs: Option<HeaderTabs<'a>>,
}

impl<'a> PanelFrame<'a> {
    /// A panel with a title.
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            icon: None,
            collapsible: false,
            scroll: true,
            flush: false,
            tabs: None,
        }
    }

    /// The body has no padding, for a body that lays itself out edge to edge.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }

    /// Fills the left of the header with `tabs` (such as [`tab_button`](crate::tab_button)s, which
    /// are 28 pt high like the header) instead of the icon and the title. The title is then only
    /// the name that keeps the fold state apart.
    pub fn tabs(mut self, tabs: impl FnOnce(&mut Ui) + 'a) -> Self {
        self.tabs = Some(Box::new(tabs));
        self
    }

    /// A 16 pt icon before the title.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A chevron at the left folds the body; the header click toggles it.
    pub fn collapsible(mut self) -> Self {
        self.collapsible = true;
        self
    }

    /// The body does not scroll; the caller manages overflow.
    pub fn no_scroll(mut self) -> Self {
        self.scroll = false;
        self
    }

    /// Draws the header, then the body unless it is folded. `actions` fills the right end of the
    /// header with icon buttons, the first added being the rightmost.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        actions: impl FnOnce(&mut Ui),
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<R> {
        let t = Tokens::of(ui.ctx());
        let open_id = ui
            .make_persistent_id(("panel_frame", self.title))
            .with("open");
        let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(true);

        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height::BAR), Sense::click());
        if self.collapsible && response.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(open_id, open));
        }
        let enabled = ui.is_enabled();
        response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, open, self.title));
        if ui.is_rect_visible(rect) {
            if let Some(tabs) = self.tabs {
                let mut tabs_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                tabs_ui.spacing_mut().item_spacing.x = 0.0;
                tabs(&mut tabs_ui);
            } else {
                let mut x = rect.left() + gutter::EDGE;
                if self.collapsible {
                    let chevron = if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    };
                    paint_icon(
                        ui,
                        chevron,
                        pos2(x + icon_size::CONTROL / 2.0, rect.center().y),
                        icon_size::CONTROL,
                        t.text_weak,
                    );
                    x += icon_size::CONTROL + space::XS;
                }
                if let Some(icon) = self.icon {
                    paint_icon(
                        ui,
                        icon,
                        pos2(x + icon_size::CONTROL / 2.0, rect.center().y),
                        icon_size::CONTROL,
                        t.text_weak,
                    );
                    x += icon_size::CONTROL + space::SM;
                }
                paint_left(
                    ui,
                    galley(ui, Type::Title, self.title),
                    x,
                    rect.center().y,
                    t.text_strong,
                );
            }
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1.0, t.border),
            );
            let mut actions_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(gutter::EDGE, 2.0)))
                    .layout(Layout::right_to_left(Align::Center)),
            );
            actions_ui.spacing_mut().item_spacing.x = space::XXS;
            actions(&mut actions_ui);
        }
        if !open {
            return None;
        }
        let margin = if self.flush { 0 } else { 8 };
        let inner = |ui: &mut Ui| {
            Frame::new()
                .inner_margin(Margin::same(margin))
                .show(ui, body)
                .inner
        };
        Some(if self.scroll {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .id_salt(open_id)
                .show(ui, inner)
                .inner
        } else {
            inner(ui)
        })
    }
}

/// A titled block, optionally folding. The open state lives in the context under `id_salt`.
#[derive(Clone, Debug)]
#[must_use = "call `show`"]
pub struct Section<'a> {
    title: &'a str,
    collapsible: bool,
    default_open: bool,
}

impl<'a> Section<'a> {
    /// A section that does not fold.
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            collapsible: false,
            default_open: true,
        }
    }

    /// A chevron before the title folds the content.
    pub fn collapsible(mut self, default_open: bool) -> Self {
        self.collapsible = true;
        self.default_open = default_open;
        self
    }

    /// Draws the title row and, while open, the content; then 12 pt of space. Returns the
    /// content's result while it is open.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        add_body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<R> {
        let t = Tokens::of(ui.ctx());
        let id = ui.make_persistent_id(id_salt);
        let open_id = id.with("open");
        let mut open = if self.collapsible {
            ui.data(|d| d.get_temp::<bool>(open_id))
                .unwrap_or(self.default_open)
        } else {
            true
        };
        let sense = if self.collapsible {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height::CONTROL), sense);
        if self.collapsible && response.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(open_id, open));
        }
        let enabled = ui.is_enabled();
        response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, open, self.title));
        if ui.is_rect_visible(rect) {
            let look = look(ui, &response, false, false);
            let mut x = rect.left();
            if self.collapsible {
                let chevron = if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                };
                paint_icon(
                    ui,
                    chevron,
                    pos2(x + icon_size::CONTROL / 2.0, rect.center().y),
                    icon_size::CONTROL,
                    if look.state == ControlState::Hover {
                        t.text_strong
                    } else {
                        t.text_weak
                    },
                );
                x += icon_size::CONTROL + space::XS;
            }
            paint_left(
                ui,
                galley(ui, Type::Title, self.title),
                x,
                rect.center().y,
                t.text_strong,
            );
            if look.focus {
                focus_ring(ui, rect, radius::CONTROL, &t);
            }
        }
        let result = open.then(|| add_body(ui));
        ui.add_space(gutter::SECTION);
        result
    }
}

/// Rows of a label column and a value column: 24 pt rows, a label column of 96 pt (72 to 128).
#[derive(Clone, Debug)]
#[must_use = "call `show`"]
pub struct PropertyGrid {
    id_salt: Id,
    label_width: f32,
}

impl PropertyGrid {
    /// A grid with the default label column.
    pub fn new(id_salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            id_salt: Id::unique(("cuboid_property_grid", id_salt)),
            label_width: label_column::DEFAULT,
        }
    }

    /// The label column width, kept between 72 and 128.
    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = width.clamp(label_column::MIN, label_column::MAX);
        self
    }

    /// Runs `add_rows` with a builder that adds rows to `ui`.
    pub fn show<R>(self, ui: &mut Ui, add_rows: impl FnOnce(&mut PropertyRows<'_>) -> R) -> R {
        ui.push_id(self.id_salt, |ui| {
            ui.spacing_mut().item_spacing.y = space::XS;
            add_rows(&mut PropertyRows {
                ui,
                label_width: self.label_width,
            })
        })
        .inner
    }
}

/// Adds the rows of a [`PropertyGrid`].
pub struct PropertyRows<'a> {
    ui: &'a mut Ui,
    label_width: f32,
}

impl PropertyRows<'_> {
    /// One row: the label at the left, ellipsised when it is too long, and the value widgets
    /// filling the rest. A label that does not fit shows in full as a tooltip.
    pub fn row<R>(&mut self, label: &str, add_value: impl FnOnce(&mut Ui) -> R) -> R {
        let ui = &mut *self.ui;
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), height::CONTROL), Sense::hover());
        let label_rect = Rect::from_min_size(rect.min, vec2(self.label_width, rect.height()));
        let value_rect =
            Rect::from_min_max(pos2(label_rect.right() + space::SM, rect.top()), rect.max);
        let fits = galley(ui, Type::Label, label).size().x <= label_rect.width();
        // A left-aligned child: `Ui::put` would centre the label in its column.
        let mut label_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(label_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let label_response = label_ui.add(
            Label::new(text(Type::Label, label))
                .truncate()
                .selectable(false),
        );
        if !fits {
            label_response.on_hover_text(label);
        }
        let mut value_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(value_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        add_value(&mut value_ui)
    }
}

/// A strip of icon buttons with group separators, horizontal or vertical.
#[derive(Copy, Clone, Debug)]
#[must_use = "call `show`"]
pub struct Toolbar {
    vertical: bool,
}

impl Toolbar {
    /// A row of buttons.
    pub fn horizontal() -> Self {
        Self { vertical: false }
    }

    /// A column of buttons.
    pub fn vertical() -> Self {
        Self { vertical: true }
    }

    /// Runs `add` with a builder that adds buttons to `ui`.
    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut ToolbarUi<'_>) -> R) -> R {
        let run = |ui: &mut Ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(space::XXS);
            add(&mut ToolbarUi {
                ui,
                vertical: self.vertical,
            })
        };
        if self.vertical {
            ui.vertical(run).inner
        } else {
            ui.horizontal(run).inner
        }
    }
}

/// Adds the buttons of a [`Toolbar`].
pub struct ToolbarUi<'a> {
    ui: &'a mut Ui,
    vertical: bool,
}

impl ToolbarUi<'_> {
    /// A 28 pt button that runs a command.
    pub fn button(&mut self, icon: Icon, tip: &str) -> Response {
        self.ui.add(IconButton::new(icon, tip).toolbar())
    }

    /// A 28 pt button for a tool; the active one has the accent tint. The shortcut shows as a
    /// chip in the tooltip.
    pub fn tool(
        &mut self,
        icon: Icon,
        active: bool,
        tip: &str,
        shortcut: Option<&str>,
    ) -> Response {
        let button = IconButton::new(icon, tip).toolbar().active(active);
        match shortcut {
            Some(shortcut) => self.ui.add(button.shortcut(shortcut)),
            None => self.ui.add(button),
        }
    }

    /// A 28 pt toggle; on is the neutral selected state.
    pub fn toggle(&mut self, icon: Icon, on: bool, tip: &str) -> Response {
        self.ui
            .add(IconButton::new(icon, tip).toolbar().selected(on))
    }

    /// A hairline between two groups.
    pub fn separator(&mut self) {
        let t = Tokens::of(self.ui.ctx());
        let long = icon_size::CONTROL;
        let size = if self.vertical {
            vec2(height::BAR, space::SM)
        } else {
            vec2(space::SM, height::BAR)
        };
        let (rect, _) = self.ui.allocate_exact_size(size, Sense::hover());
        let c = rect.center();
        let line = if self.vertical {
            [pos2(c.x - long / 2.0, c.y), pos2(c.x + long / 2.0, c.y)]
        } else {
            [pos2(c.x, c.y - long / 2.0), pos2(c.x, c.y + long / 2.0)]
        };
        self.ui
            .painter()
            .add(Shape::line_segment(line, Stroke::new(1.0, t.border_strong)));
    }

    /// The underlying `Ui`, for a widget that is no icon button.
    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }
}

/// A tile for a recent file or a library entry: a 4:3 thumbnail with the picture fitted, a title,
/// a caption and an optional badge at the top left. An optional pin shows at the top right while
/// the card is hovered or pinned. Hovering lifts the card one surface step.
#[derive(Clone, Debug)]
#[must_use = "call `show`"]
pub struct Card<'a> {
    title: &'a str,
    caption: Option<&'a str>,
    badge: Option<&'a str>,
    thumbnail: Option<(TextureId, Vec2)>,
    placeholder: Icon,
    width: f32,
    selected: bool,
    pin: Option<bool>,
}

/// What a [`Card`] did this frame. It derefs to the response of the whole card.
#[derive(Clone, Debug)]
pub struct CardResponse {
    /// The whole card; a click on the pin is not a click on it.
    pub response: Response,
    /// The pin button, for a card that has one.
    pub pin: Option<Response>,
}

impl std::ops::Deref for CardResponse {
    type Target = Response;

    fn deref(&self) -> &Response {
        &self.response
    }
}

impl<'a> Card<'a> {
    /// A card 200 pt wide.
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            caption: None,
            badge: None,
            thumbnail: None,
            placeholder: Icon::Cube,
            width: 200.0,
            selected: false,
            pin: None,
        }
    }

    /// A second line in the caption style.
    pub fn caption(mut self, caption: &'a str) -> Self {
        self.caption = Some(caption);
        self
    }

    /// A badge at the top left of the thumbnail, such as the file format in lower case.
    pub fn badge(mut self, badge: &'a str) -> Self {
        self.badge = Some(badge);
        self
    }

    /// The picture and its size in pixels; it is fitted inside the 4:3 area, never cropped.
    pub fn thumbnail(mut self, texture: TextureId, size: Vec2) -> Self {
        self.thumbnail = Some((texture, size));
        self
    }

    /// The icon shown while there is no picture.
    pub fn placeholder(mut self, icon: Icon) -> Self {
        self.placeholder = icon;
        self
    }

    /// The card width in points.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// The card is the current choice.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// A pin button at the top right of the thumbnail; `pinned` keeps it visible and strong.
    pub fn pin(mut self, pinned: bool) -> Self {
        self.pin = Some(pinned);
        self
    }

    /// Draws the card; the response senses clicks.
    pub fn show(self, ui: &mut Ui) -> CardResponse {
        let t = Tokens::of(ui.ctx());
        let pad = gutter::EDGE;
        let thumb_w = self.width - 2.0 * pad;
        let thumb_h = (thumb_w * 3.0 / 4.0).round();
        let title_h = 20.0;
        let caption_h = if self.caption.is_some() { 16.0 } else { 0.0 };
        let total = vec2(
            self.width,
            pad + thumb_h + space::SM + title_h + caption_h + pad,
        );
        let (rect, response) = ui.allocate_exact_size(total, Sense::click());
        let enabled = ui.is_enabled();
        let selected = self.selected;
        response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, self.title));
        let area = Rect::from_min_size(
            pos2(rect.left() + pad, rect.top() + pad),
            vec2(thumb_w, thumb_h),
        );
        let pin = self.pin.map(|pinned| {
            let slot = Rect::from_min_size(
                pos2(
                    area.right() - height::CONTROL - space::XS,
                    area.top() + space::XS,
                ),
                vec2(height::CONTROL, height::CONTROL),
            );
            let pin = ui.interact(slot, response.id.with("pin"), Sense::click());
            pin.widget_info(|| {
                WidgetInfo::selected(Role::Button, enabled, pinned, format!("Pin {}", self.title))
            });
            (pin, slot, pinned)
        });
        let pin_response = |pin: Option<(Response, Rect, bool)>| pin.map(|(pin, ..)| pin);
        if !ui.is_rect_visible(rect) {
            return CardResponse {
                response,
                pin: pin_response(pin),
            };
        }
        let look = look(ui, &response, self.selected, false);
        let fill = match look.state {
            ControlState::Hover => t.bg_hover,
            ControlState::Pressed | ControlState::Selected => t.bg_active,
            _ => t.bg_raised,
        };
        ui.painter().rect_filled(rect, radius::CARD, fill);
        ui.painter().rect_filled(area, radius::CONTROL, t.bg_input);
        match self.thumbnail {
            Some((texture, size)) if size.x > 0.0 && size.y > 0.0 => {
                paint_image(ui.painter(), texture, fit_centered(area, size));
            }
            _ => paint_icon(
                ui,
                self.placeholder,
                area.center(),
                icon_size::EMPTY,
                t.text_weak,
            ),
        }
        if let Some(label) = self.badge {
            let mut corner = ui.new_child(
                UiBuilder::new()
                    .max_rect(area.shrink(space::XS))
                    .layout(Layout::left_to_right(Align::Min)),
            );
            badge(&mut corner, label, Tone::Neutral);
        }
        if let Some((pin, slot, pinned)) = &pin
            && (*pinned || response.hovered() || pin.hovered() || pin.has_focus())
        {
            let (backdrop, ink) = if pin.hovered() {
                (t.bg_hover, t.text_strong)
            } else if *pinned {
                (t.bg_panel.gamma_multiply(0.85), t.text_strong)
            } else {
                (t.bg_panel.gamma_multiply(0.85), t.text_weak)
            };
            ui.painter().rect_filled(*slot, radius::CONTROL, backdrop);
            paint_icon(ui, Icon::Pin, slot.center(), icon_size::CONTROL, ink);
            if pin.has_focus() {
                focus_ring(ui, *slot, radius::CONTROL, &t);
            }
        }
        let text_x = rect.left() + pad;
        let title_y = area.bottom() + space::SM + title_h / 2.0;
        let line = |y: f32, height: f32| {
            Rect::from_min_max(
                pos2(text_x, y - height / 2.0),
                pos2(rect.right() - pad, y + height / 2.0),
            )
        };
        paint_clipped(
            ui,
            Type::Title,
            self.title,
            line(title_y, title_h),
            text_x,
            dimmed(t.text_strong, look),
        );
        if let Some(caption) = self.caption {
            let y = title_y + title_h / 2.0 + caption_h / 2.0;
            paint_clipped(
                ui,
                Type::Caption,
                caption,
                line(y, caption_h),
                text_x,
                t.text_weak,
            );
        }
        if look.focus {
            focus_ring(ui, rect, radius::CARD, &t);
        }
        CardResponse {
            response,
            pin: pin_response(pin),
        }
    }
}

/// A centred message for a view with nothing in it: a 40 pt icon, a headline, one line of text
/// and up to three buttons.
#[must_use = "call `show`"]
pub struct EmptyState<'a> {
    icon: Icon,
    heading: &'a str,
    body: Option<&'a str>,
    actions: Vec<Button<'a>>,
}

impl<'a> EmptyState<'a> {
    /// An empty state with an icon and a headline.
    pub fn new(icon: Icon, heading: &'a str) -> Self {
        Self {
            icon,
            heading,
            body: None,
            actions: Vec::new(),
        }
    }

    /// The line under the headline.
    pub fn body(mut self, body: &'a str) -> Self {
        self.body = Some(body);
        self
    }

    /// Adds a button, up to three; the first should be the primary one.
    pub fn action(mut self, button: Button<'a>) -> Self {
        self.actions.push(button);
        self
    }

    /// Draws the state centred in the available width. Returns the index of the clicked button.
    pub fn show(self, ui: &mut Ui) -> Option<usize> {
        let t = Tokens::of(ui.ctx());
        let width = ui.available_width();
        let mut clicked = None;
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(icon_size::EMPTY), Sense::hover());
            paint_icon(ui, self.icon, rect.center(), icon_size::EMPTY, t.text_weak);
            ui.add_space(space::MD);
            ui.label(text(Type::Heading, self.heading));
            if let Some(body) = self.body {
                ui.add_space(space::XS);
                ui.add(Label::new(text(Type::Body, body)).wrap().selectable(false));
            }
            if self.actions.is_empty() {
                return;
            }
            ui.add_space(space::LG);
            let widths: Vec<f32> = self.actions.iter().map(|b| b.natural_width(ui)).collect();
            let total = widths.iter().sum::<f32>() + space::SM * (widths.len() - 1) as f32;
            let height = self
                .actions
                .iter()
                .map(Button::height)
                .fold(0.0_f32, f32::max);
            // Buttons that do not fit side by side stand one above the other.
            let stacked = total > width;
            let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let mut x = row.center().x - total / 2.0;
            let mut y = row.top();
            for (index, (button, w)) in self.actions.into_iter().zip(widths).enumerate() {
                if stacked {
                    if index > 0 {
                        ui.add_space(space::XS);
                        y = ui
                            .allocate_exact_size(vec2(width, height), Sense::hover())
                            .0
                            .top();
                    }
                    x = row.center().x - w / 2.0;
                }
                let slot = Rect::from_min_size(pos2(x, y), vec2(w, height));
                x += w + space::SM;
                if ui.put(slot, button).clicked() {
                    clicked = Some(index);
                }
            }
        });
        clicked
    }
}

/// Runs `add` in the free space of `ui` shrunk by `amount` on every side. The space is used up
/// entirely, so a panel can be padded without a frame margin.
pub fn inset<R>(ui: &mut Ui, amount: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let area = ui.available_rect_before_wrap();
    let result = ui
        .scope_builder(UiBuilder::new().max_rect(area.shrink(amount)), add)
        .inner;
    ui.advance_cursor_after_rect(area);
    result
}

/// Runs `add` on a content surface: a rounded fill of the content level with 8 pt of padding.
pub fn content_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::of(ui.ctx());
    Frame::new()
        .fill(t.bg_raised)
        .corner_radius(radius::CARD)
        .inner_margin(Margin::same(8))
        .show(ui, add)
        .inner
}
