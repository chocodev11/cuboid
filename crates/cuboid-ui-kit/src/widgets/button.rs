//! Buttons: text buttons of four kinds, icon buttons, badges and shortcut chips.

use egui::{Color32, Response, Role, Sense, Ui, Widget, WidgetInfo, pos2, vec2};

use crate::icons::Icon;
use crate::metrics::{height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::{Type, text};
use crate::widgets::paint::{galley, paint_centered, paint_icon, paint_left};
use crate::widgets::state::{ControlState, Look, control_fill, dimmed, focus_ring, look};

/// Which text button it is.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ButtonKind {
    /// Accent fill; at most one per view.
    Primary,
    /// Control fill.
    Secondary,
    /// Transparent, a fill on hover.
    Ghost,
    /// For destructive actions.
    Danger,
}

/// A text button, optionally with a leading icon: `ui.add(Button::new("Save").primary())`.
#[derive(Clone, Debug)]
#[must_use = "add the button with `ui.add`"]
pub struct Button<'a> {
    label: &'a str,
    kind: ButtonKind,
    height: f32,
    icon: Option<Icon>,
    min_width: f32,
    fill_width: bool,
    selected: bool,
    icon_only: bool,
}

impl<'a> Button<'a> {
    /// A secondary button, 24 high.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            kind: ButtonKind::Secondary,
            height: height::CONTROL,
            icon: None,
            min_width: 0.0,
            fill_width: false,
            selected: false,
            icon_only: false,
        }
    }

    /// The accent button.
    pub fn primary(mut self) -> Self {
        self.kind = ButtonKind::Primary;
        self
    }

    /// The transparent button.
    pub fn ghost(mut self) -> Self {
        self.kind = ButtonKind::Ghost;
        self
    }

    /// The destructive button.
    pub fn danger(mut self) -> Self {
        self.kind = ButtonKind::Danger;
        self
    }

    /// 32 high instead of 24, for the main action of a dialog or of the start screen.
    pub fn tall(mut self) -> Self {
        self.height = height::ACTION;
        self
    }

    /// A 16 pt icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The button is at least this wide.
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }

    /// The button takes the whole available width.
    pub fn fill_width(mut self) -> Self {
        self.fill_width = true;
        self
    }

    /// Shows only the icon; the label is the tooltip and the accessible name. Needs an icon.
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    /// Draws the button pressed in, for a button that toggles.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// The height in points.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// The width the button takes by itself, without `fill_width`.
    pub fn natural_width(&self, ui: &Ui) -> f32 {
        let label = galley(ui, self.role(), self.label);
        (self.content_width(label.size().x) + 2.0 * space::MD).max(self.min_width)
    }

    fn role(&self) -> Type {
        match self.kind {
            ButtonKind::Primary | ButtonKind::Danger => Type::Strong,
            ButtonKind::Secondary | ButtonKind::Ghost => Type::Body,
        }
    }

    /// Icon, gap and label together.
    fn content_width(&self, label: f32) -> f32 {
        if self.icon_only {
            return icon_size::CONTROL;
        }
        let icon = if self.icon.is_some() {
            icon_size::CONTROL + space::XS
        } else {
            0.0
        };
        icon + label
    }
}

/// Fill and ink of a text button in a state.
fn colors(t: &Tokens, kind: ButtonKind, look: Look) -> (Color32, Color32) {
    use ControlState::{Disabled, Hover, Pressed, Selected};
    let state = look.state;
    match kind {
        ButtonKind::Primary => {
            let fill = match state {
                Hover => t.accent_hover,
                Pressed => t.accent.lerp_to_gamma(t.on_accent, 0.18),
                Disabled => t.accent.gamma_multiply(0.4),
                _ => t.accent,
            };
            (fill, dimmed(t.on_accent, look))
        }
        ButtonKind::Danger => {
            let fill = match state {
                Hover => t.danger.lerp_to_gamma(t.text_strong, 0.15),
                Pressed => t.danger.lerp_to_gamma(t.on_accent, 0.18),
                Disabled => t.danger.gamma_multiply(0.4),
                _ => t.danger,
            };
            (fill, dimmed(t.on_accent, look))
        }
        ButtonKind::Secondary | ButtonKind::Ghost => {
            let fill = match (kind, state) {
                (ButtonKind::Ghost, Hover) => t.bg_hover,
                (ButtonKind::Ghost, Pressed | Selected) => t.bg_active,
                (ButtonKind::Ghost, _) => Color32::TRANSPARENT,
                _ => control_fill(t, state),
            };
            let ink = match state {
                Hover | Pressed | Selected => t.text_strong,
                _ => t.text,
            };
            (fill, dimmed(ink, look))
        }
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let t = Tokens::of(ui.ctx());
        let label = galley(ui, self.role(), self.label);
        let content = self.content_width(label.size().x);
        let natural = (content + 2.0 * space::MD).max(self.min_width);
        let width = if self.fill_width {
            ui.available_width().max(natural)
        } else {
            natural
        };
        let (rect, response) = ui.allocate_exact_size(vec2(width, self.height), Sense::click());
        let enabled = ui.is_enabled();
        response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, self.label));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let look = look(ui, &response, self.selected, false);
        let (fill, ink) = colors(&t, self.kind, look);
        ui.painter().rect_filled(rect, radius::CONTROL, fill);
        let mut x = rect.center().x - content / 2.0;
        if let Some(icon) = self.icon {
            paint_icon(
                ui,
                icon,
                pos2(x + icon_size::CONTROL / 2.0, rect.center().y),
                icon_size::CONTROL,
                ink,
            );
            x += icon_size::CONTROL + space::XS;
        }
        if !self.icon_only {
            paint_left(ui, label, x, rect.center().y, ink);
        }
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
        if self.icon_only {
            return response.on_hover_text(self.label);
        }
        response
    }
}

/// A square button with an icon, 24 wide or 28 in a toolbar: `ui.add(IconButton::new(..))`.
#[derive(Clone, Debug)]
#[must_use = "add the button with `ui.add`"]
pub struct IconButton<'a> {
    icon: Icon,
    tip: &'a str,
    shortcut: Option<&'a str>,
    toolbar: bool,
    selected: bool,
    active: bool,
}

impl<'a> IconButton<'a> {
    /// A 24 pt button with a 16 pt icon. `tip` is the tooltip and the accessible name.
    pub fn new(icon: Icon, tip: &'a str) -> Self {
        Self {
            icon,
            tip,
            shortcut: None,
            toolbar: false,
            selected: false,
            active: false,
        }
    }

    /// Shows the shortcut as a chip in the tooltip.
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// 28 pt with a 20 pt icon, for toolbars.
    pub fn toolbar(mut self) -> Self {
        self.toolbar = true;
        self
    }

    /// A toggle that is on: the neutral selected state (pressed surface, strong icon).
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// The active tool: the accent tint and an accent icon.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let t = Tokens::of(ui.ctx());
        let (side, glyph) = if self.toolbar {
            (height::BAR, icon_size::TOOLBAR)
        } else {
            (height::CONTROL, icon_size::CONTROL)
        };
        let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
        let enabled = ui.is_enabled();
        let on = self.selected || self.active;
        response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, on, self.tip));
        if ui.is_rect_visible(rect) {
            let look = look(ui, &response, on, false);
            let (fill, ink) = match look.state {
                ControlState::Pressed => (t.bg_active, t.text_strong),
                ControlState::Selected if self.active => (t.accent_soft, t.accent),
                ControlState::Selected => (t.bg_active, t.text_strong),
                ControlState::Hover => (t.bg_hover, t.text_strong),
                _ => (Color32::TRANSPARENT, t.text_weak),
            };
            ui.painter().rect_filled(rect, radius::CONTROL, fill);
            paint_icon(ui, self.icon, rect.center(), glyph, dimmed(ink, look));
            if look.focus {
                focus_ring(ui, rect, radius::CONTROL, &t);
            }
        }
        let (tip, shortcut) = (self.tip, self.shortcut);
        response.on_hover_ui(|ui| {
            ui.horizontal(|ui| {
                ui.label(text(Type::Body, tip));
                if let Some(shortcut) = shortcut {
                    key_chip(ui, shortcut);
                }
            });
        })
    }
}

/// A flat 24 pt button with an icon. `tip` is the tooltip and the accessible name.
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    ui.add(IconButton::new(icon, tip))
}

/// A 28 pt toolbar button; the active tool gets the accent tint.
pub fn tool_button(ui: &mut Ui, icon: Icon, selected: bool, tip: &str) -> Response {
    ui.add(IconButton::new(icon, tip).toolbar().active(selected))
}

/// What a [`badge`] says.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Tone {
    /// Plain information on the pressed surface.
    Neutral,
    /// An error.
    Danger,
    /// A warning.
    Warning,
    /// A success.
    Success,
}

/// Fill and ink of a badge in a tone.
pub(crate) fn badge_colors(t: &Tokens, tone: Tone) -> (Color32, Color32) {
    match tone {
        Tone::Neutral => (t.bg_active, t.text_weak),
        Tone::Danger => (t.bg_panel.lerp_to_gamma(t.danger, 0.16), t.danger),
        Tone::Warning => (t.bg_panel.lerp_to_gamma(t.warn, 0.16), t.warn),
        Tone::Success => (t.bg_panel.lerp_to_gamma(t.ok, 0.16), t.ok),
    }
}

/// A small label: caption text on the pressed surface, radius 3, padding 2 by 6. The status tones
/// are for status only (validator results).
pub fn badge(ui: &mut Ui, label: &str, tone: Tone) -> Response {
    let t = Tokens::of(ui.ctx());
    let label = galley(ui, Type::Caption, label);
    let size = label.size() + vec2(2.0 * 6.0, 2.0 * space::XXS);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(Role::Label, enabled, label.text()));
    if ui.is_rect_visible(rect) {
        let (fill, ink) = badge_colors(&t, tone);
        ui.painter().rect_filled(rect, radius::CONTROL, fill);
        paint_centered(ui, label, rect, ink);
    }
    response
}

/// A small rounded label for a keyboard shortcut.
pub fn key_chip(ui: &mut Ui, shortcut: &str) -> Response {
    badge(ui, shortcut, Tone::Neutral)
}

/// Draws `label` right-aligned inside `rect` in the weak colour; used by menu rows.
pub fn weak_text_right(ui: &Ui, rect: egui::Rect, label: &str) {
    let t = Tokens::of(ui.ctx());
    ui.painter().text(
        pos2(rect.right(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        label,
        Type::Label.font(),
        t.text_weak,
    );
}
