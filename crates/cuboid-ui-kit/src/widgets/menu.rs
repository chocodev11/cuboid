//! Menus: the bar of the top edge, its buttons, the rows of a menu and the rows that open a
//! submenu. Rows are ghost rows: transparent at rest, a quiet fill on hover.

use egui::containers::menu::{MenuBar, MenuButton, MenuConfig, SubMenu, menu_style};
use egui::style::StyleModifier;
use egui::{
    InnerResponse, Popup, PopupCloseBehavior, Response, Role, Sense, Style, Ui, Widget, WidgetInfo,
    pos2, vec2,
};

use crate::icons::Icon;
use crate::metrics::{height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::{Type, text};
use crate::widgets::paint::{galley, paint_icon, paint_left};
use crate::widgets::state::{ControlState, dimmed, focus_ring, look};

/// Smallest width of a menu.
const MENU_MIN: f32 = 176.0;

/// The style of the bar and its menus: no frames at rest, the ghost fills on hover and while open.
fn ghost(tokens: Tokens) -> StyleModifier {
    StyleModifier::new(move |style: &mut Style| {
        menu_style(style);
        style.spacing.button_padding = vec2(space::SM, space::XS);
        let widgets = &mut style.visuals.widgets;
        widgets.hovered.weak_bg_fill = tokens.bg_hover;
        widgets.active.weak_bg_fill = tokens.bg_active;
        widgets.open.weak_bg_fill = tokens.bg_active;
    })
}

/// A bar of menu buttons. Put [`menu_button`]s in it; other widgets may follow them.
pub fn menu_bar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let tokens = Tokens::of(ui.ctx());
    MenuBar::new()
        .style(ghost(tokens))
        .config(MenuConfig::new().style(ghost(tokens)))
        .ui(ui, add)
}

/// A button of the [`menu_bar`] that opens a menu; hovering another button while one is open
/// switches to it. Returns the button's response.
pub fn menu_button(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) -> Response {
    MenuButton::from_button(egui::Button::new(text(Type::Body, label)))
        .ui(ui, add)
        .0
}

/// A row of a menu: an optional check mark column, the label and the shortcut at the right.
#[derive(Clone, Debug)]
#[must_use = "add the row with `ui.add`"]
pub struct MenuItem<'a> {
    label: &'a str,
    shortcut: Option<&'a str>,
    checked: Option<bool>,
    submenu: bool,
}

impl<'a> MenuItem<'a> {
    /// A row with a label.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            shortcut: None,
            checked: None,
            submenu: false,
        }
    }

    /// The shortcut, drawn weak at the right.
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Reserves the check mark column; the mark shows while `checked` is true.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
}

impl Widget for MenuItem<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let t = Tokens::of(ui.ctx());
        let label = galley(ui, Type::Body, self.label);
        let shortcut = self.shortcut.map(|s| galley(ui, Type::Caption, s));
        let lead = if self.checked.is_some() {
            icon_size::CONTROL + space::XS
        } else {
            0.0
        };
        let tail = match (&shortcut, self.submenu) {
            (Some(g), _) => space::LG + g.size().x,
            (None, true) => space::LG + icon_size::CONTROL,
            (None, false) => 0.0,
        };
        let content = 2.0 * space::SM + lead + label.size().x + tail;
        let width = ui.available_width().max(content).max(MENU_MIN);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height::CONTROL), Sense::click());
        let enabled = ui.is_enabled();
        let checked = self.checked.unwrap_or(false);
        response.widget_info(|| WidgetInfo::selected(Role::MenuItem, enabled, checked, self.label));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let look = look(ui, &response, false, false);
        if matches!(look.state, ControlState::Hover | ControlState::Pressed) {
            ui.painter().rect_filled(rect, radius::CONTROL, t.bg_hover);
        }
        let ink = if look.state == ControlState::Hover {
            t.text_strong
        } else {
            t.text
        };
        let mut x = rect.left() + space::SM;
        if checked {
            paint_icon(
                ui,
                Icon::Check,
                pos2(x + icon_size::CONTROL / 2.0, rect.center().y),
                icon_size::CONTROL,
                dimmed(t.text_strong, look),
            );
        }
        x += lead;
        paint_left(ui, label, x, rect.center().y, dimmed(ink, look));
        if let Some(shortcut) = shortcut {
            let left = rect.right() - space::SM - shortcut.size().x;
            paint_left(
                ui,
                shortcut,
                left,
                rect.center().y,
                dimmed(t.text_weak, look),
            );
        } else if self.submenu {
            paint_icon(
                ui,
                Icon::ChevronRight,
                pos2(
                    rect.right() - space::SM - icon_size::CONTROL / 2.0,
                    rect.center().y,
                ),
                icon_size::CONTROL,
                dimmed(t.text_weak, look),
            );
        }
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
        response
    }
}

/// A row that opens a submenu at its side while it is hovered or clicked.
pub fn submenu(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) -> Response {
    let item = MenuItem {
        submenu: true,
        ..MenuItem::new(label)
    };
    let response = ui.add(item);
    SubMenu::new().show(ui, &response, add);
    response
}

/// A ghost button with an optional icon, a label and a caret that opens a menu below it: the
/// view, shading and camera menus of a viewport header. A `compact` button leaves the label out
/// and shows it as the tooltip. Returns the button's response.
pub fn menu_trigger(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    compact: bool,
    add: impl FnOnce(&mut Ui),
) -> Response {
    let t = Tokens::of(ui.ctx());
    let text = galley(ui, Type::Body, if compact { "" } else { label });
    let lead = if icon.is_some() && !compact {
        icon_size::CONTROL + space::XS
    } else if icon.is_some() {
        icon_size::CONTROL
    } else {
        0.0
    };
    let gap = if compact { 0.0 } else { space::XS };
    let width = 2.0 * space::SM + lead + text.size().x + gap + icon_size::CONTROL;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height::CONTROL), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&response));
    if ui.is_rect_visible(rect) {
        let look = look(ui, &response, open, false);
        let (fill, ink) = match look.state {
            ControlState::Selected | ControlState::Pressed => (Some(t.bg_active), t.text_strong),
            ControlState::Hover => (Some(t.bg_hover), t.text_strong),
            _ => (None, t.text),
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, radius::CONTROL, fill);
        }
        let mut x = rect.left() + space::SM;
        if let Some(icon) = icon {
            paint_icon(
                ui,
                icon,
                pos2(x + icon_size::CONTROL / 2.0, rect.center().y),
                icon_size::CONTROL,
                dimmed(t.text_weak, look),
            );
            x += lead;
        }
        let label_width = text.size().x;
        paint_left(ui, text, x, rect.center().y, dimmed(ink, look));
        paint_icon(
            ui,
            Icon::ChevronDown,
            pos2(
                x + label_width + gap + icon_size::CONTROL / 2.0,
                rect.center().y,
            ),
            icon_size::CONTROL,
            dimmed(t.text_weak, look),
        );
        if look.focus {
            focus_ring(ui, rect, radius::CONTROL, &t);
        }
    }
    let response = if compact {
        response.on_hover_text(label)
    } else {
        response
    };
    Popup::menu(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .style(ghost(t))
        .show(add);
    response
}
