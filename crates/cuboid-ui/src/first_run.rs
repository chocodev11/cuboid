//! The first-run dialog over the start screen: which app the user knows, and an accent colour.
//! The first choice sets the camera buttons, the keymap and the panel arrangement together
//! (ui_v2_spec 8.1).

use cuboid_ui_kit::metrics::{radius, space};
use cuboid_ui_kit::{Icon, Modal, ModalButton, Tokens, Type, key_chip, rgb_color, text};
use egui::{
    Align, Context, Label, Layout, Painter, Pos2, Rect, Role, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, WidgetInfo, pos2, vec2,
};

use crate::common::ImageCache;
use crate::prefs::{ComingFrom, Prefs};
use crate::start::StartEvent;

/// Width of the dialog.
const WIDTH: f32 = 720.0;
/// Edge of the logo above the question (logo.md 5).
const LOGO: f32 = 64.0;
/// Height of a choice card.
const CARD_HEIGHT: f32 = 172.0;
/// Height of the drawing in a card.
const DRAWING_HEIGHT: f32 = 100.0;
/// Edge of an accent swatch.
const SWATCH: f32 = 28.0;
/// Width of the cell of an accent swatch and its name.
const SWATCH_CELL: f32 = 64.0;

/// The three mouse buttons a gesture can use.
#[derive(Copy, Clone, Eq, PartialEq)]
enum Button {
    Left,
    Middle,
    Right,
}

/// What one gesture of a card looks like: the button held, a key held with it, and what it does.
struct Gesture {
    name: &'static str,
    icon: Icon,
    button: Button,
    key: Option<&'static str>,
}

/// The orbit and pan gestures of an app.
fn gestures(from: ComingFrom) -> [Gesture; 2] {
    let orbit = |button, key| Gesture {
        name: "Orbit",
        icon: Icon::Rotate,
        button,
        key,
    };
    let pan = |button, key| Gesture {
        name: "Pan",
        icon: Icon::Move,
        button,
        key,
    };
    match from {
        ComingFrom::Blockbench => [orbit(Button::Left, None), pan(Button::Right, None)],
        ComingFrom::Cubik => [
            orbit(Button::Middle, Some("Alt")),
            pan(Button::Middle, None),
        ],
        ComingFrom::Blender => [
            orbit(Button::Middle, None),
            pan(Button::Middle, Some("Shift")),
        ],
    }
}

/// Draws a mouse with one button highlighted.
fn mouse(painter: &Painter, center: Pos2, held: Button, t: &Tokens) {
    let body = Rect::from_center_size(center, vec2(28.0, 44.0));
    let waist = body.top() + 18.0;
    let ink = Stroke::new(1.5, t.text_weak);
    let lit = t.text_strong;
    let (x0, x1, mid) = (body.left() + 2.0, body.right() - 2.0, body.center().x);
    let pressed = match held {
        Button::Left => Rect::from_min_max(pos2(x0, body.top() + 2.0), pos2(mid - 1.0, waist)),
        Button::Right => Rect::from_min_max(pos2(mid + 1.0, body.top() + 2.0), pos2(x1, waist)),
        Button::Middle => Rect::from_min_max(
            pos2(mid - 2.5, body.top() + 3.0),
            pos2(mid + 2.5, waist - 3.0),
        ),
    };
    painter.rect_filled(pressed, radius::CONTROL, lit);
    painter.rect_stroke(body, radius::MODAL * 2, ink, StrokeKind::Inside);
    painter.hline(body.x_range(), waist, ink);
    painter.vline(mid, body.top()..=waist, ink);
}

/// One choice card. Returns whether it was clicked.
fn choice_card(ui: &mut Ui, from: ComingFrom, selected: bool, width: f32) -> bool {
    let t = Tokens::of(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, from.name()));
    let fill = if selected {
        t.bg_active
    } else if response.hovered() {
        t.bg_hover
    } else {
        t.bg_panel
    };
    ui.painter().rect_filled(rect, radius::CARD, fill);
    if selected {
        ui.painter().rect_stroke(
            rect,
            radius::CARD,
            Stroke::new(2.0, t.text_strong),
            StrokeKind::Inside,
        );
    }
    let drawing = Rect::from_min_size(
        rect.min + vec2(space::SM, space::SM),
        vec2(rect.width() - 2.0 * space::SM, DRAWING_HEIGHT),
    );
    let columns = gestures(from);
    let column_width = drawing.width() / 2.0;
    for (i, gesture) in columns.iter().enumerate() {
        let column = Rect::from_min_size(
            pos2(drawing.left() + i as f32 * column_width, drawing.top()),
            vec2(column_width, drawing.height()),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(column)
                .layout(Layout::top_down(Align::Center)),
        );
        let (mouse_rect, _) = child.allocate_exact_size(vec2(column_width, 52.0), Sense::hover());
        mouse(
            ui.painter(),
            mouse_rect.center() - vec2(10.0, 0.0),
            gesture.button,
            &t,
        );
        gesture.icon.paint(
            ui.painter(),
            Rect::from_center_size(mouse_rect.center() + vec2(18.0, 0.0), vec2(20.0, 20.0)),
            if selected { t.text_strong } else { t.text },
        );
        child.add(Label::new(text(Type::Label, gesture.name)).selectable(false));
        if let Some(key) = gesture.key {
            key_chip(&mut child, key);
        }
    }
    let label = Rect::from_min_max(
        pos2(rect.left() + space::SM, drawing.bottom() + space::SM),
        pos2(rect.right() - space::SM, rect.bottom() - space::SM),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(label)
            .layout(Layout::top_down(Align::Min)),
    );
    child.add(Label::new(text(Type::Title, from.name())).selectable(false));
    child.add(
        Label::new(text(Type::Caption, from.blurb()))
            .truncate()
            .selectable(false),
    );
    response.clicked()
}

/// One accent swatch with its name below. Returns whether it was clicked.
fn accent_swatch(ui: &mut Ui, rgb: [u8; 3], name: &str, selected: bool) -> bool {
    let t = Tokens::of(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(SWATCH_CELL, 52.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, name));
    let centre = pos2(rect.center().x, rect.top() + SWATCH / 2.0 + space::XS);
    ui.painter()
        .circle_filled(centre, SWATCH / 2.0, rgb_color(rgb));
    if selected {
        ui.painter().circle_stroke(
            centre,
            SWATCH / 2.0 + space::XS,
            Stroke::new(2.0, t.text_strong),
        );
    } else if response.hovered() {
        ui.painter().circle_stroke(
            centre,
            SWATCH / 2.0 + space::XS,
            Stroke::new(1.0, t.border_hover),
        );
    }
    let ink = if selected { t.text_strong } else { t.text_weak };
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - space::SM),
        egui::Align2::CENTER_CENTER,
        name,
        Type::Caption.font(),
        ink,
    );
    response.clicked()
}

/// Draws the dialog and reports the choices in `events`.
pub(crate) fn show(
    ctx: &Context,
    prefs: &Prefs,
    images: &mut ImageCache,
    events: &mut Vec<StartEvent>,
) {
    let buttons = [ModalButton::primary("Continue")];
    let mut modal = Modal::new("first_run", "Which app are you used to?")
        .width(WIDTH)
        .buttons(&buttons);
    if let Some(logo) = images.logo(ctx, LOGO) {
        modal = modal.mark(logo, LOGO);
    }
    let outcome = modal.show(ctx, |ui| {
        let intro = "This sets the camera buttons, the keys and where the panels sit. You can change each of them later in Preferences.";
        let accent_note = "Optional. Until you pick one, selections use a neutral highlight.";
        ui.add(
            Label::new(text(Type::Body, intro))
                .wrap()
                .selectable(false),
        );
        ui.add_space(space::MD);
        let gap = space::SM;
        let width = (ui.available_width() - 2.0 * gap) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for from in ComingFrom::ALL {
                if choice_card(ui, from, prefs.coming_from == from, width) {
                    events.push(StartEvent::ComingFrom(from));
                }
            }
        });
        ui.add_space(space::MD);
        ui.label(text(Type::Title, "Accent colour"));
        ui.add_space(space::XS);
        let theme = prefs.theme();
        let system_dark = ui.ctx().system_theme().map(|t| t == egui::Theme::Dark);
        let presets = theme.accent_presets(theme.is_dark(system_dark));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (preset, name) in presets.into_iter().zip(prefs.family.accent_names()) {
                let on = prefs.accent_chosen && prefs.accent == preset;
                if accent_swatch(ui, preset, name, on) {
                    events.push(StartEvent::Accent(preset));
                }
            }
        });
        ui.add(Label::new(text(Type::Caption, accent_note)).selectable(false));
    });
    if outcome.button == Some(0) || outcome.confirmed || outcome.cancelled {
        events.push(StartEvent::Continue);
    }
}
