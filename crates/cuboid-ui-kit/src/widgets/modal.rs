//! The modal dialog: heading, body, and a footer with buttons on the right.

use egui::{
    Align, Align2, Color32, Context, Frame, Id, Key, Layout, Margin, Modifiers, Shadow, Stroke, Ui,
    vec2,
};

use crate::metrics::{BACKDROP_ALPHA, DIALOG_WIDTHS, radius, space};
use crate::theme::Tokens;
use crate::typography::{Type, text};
use crate::widgets::button::{Button, ButtonKind};

/// Smallest width of a footer button.
const FOOTER_BUTTON: f32 = 72.0;
/// Distance of a palette from the top edge of the window.
const PALETTE_TOP: f32 = space::XXL * 2.0 + space::XL;

/// A footer button.
#[derive(Copy, Clone, Debug)]
pub struct ModalButton<'a> {
    /// The label.
    pub label: &'a str,
    /// Primary, secondary, ghost or danger.
    pub kind: ButtonKind,
    /// The button can be pressed; Enter does nothing while the last button cannot.
    pub enabled: bool,
}

impl<'a> ModalButton<'a> {
    /// A secondary button.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            kind: ButtonKind::Secondary,
            enabled: true,
        }
    }

    /// The accent button; it belongs last.
    pub fn primary(label: &'a str) -> Self {
        Self {
            label,
            kind: ButtonKind::Primary,
            enabled: true,
        }
    }

    /// The destructive button.
    pub fn danger(label: &'a str) -> Self {
        Self {
            label,
            kind: ButtonKind::Danger,
            enabled: true,
        }
    }

    /// Dims the button and makes it ignore clicks while `enabled` is false.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// What a modal did this frame.
#[derive(Clone, Debug)]
pub struct ModalOutcome<R> {
    /// The body's result.
    pub inner: R,
    /// The footer button that was clicked, as an index into the buttons given.
    pub button: Option<usize>,
    /// Esc was pressed.
    pub cancelled: bool,
    /// Enter was pressed with no text box in use: the same as the last button.
    pub confirmed: bool,
}

/// A dialog over a 40 % backdrop, centred, 360, 520 or 720 pt wide, with a 6 pt radius, 16 pt
/// padding and one strong border. Esc cancels; Enter confirms, which is the last button. It
/// never resizes while open because its width is fixed. Clicking the backdrop does nothing.
#[derive(Clone, Debug)]
#[must_use = "call `show`"]
pub struct Modal<'a> {
    id: Id,
    title: &'a str,
    width: f32,
    buttons: &'a [ModalButton<'a>],
    palette: bool,
}

impl<'a> Modal<'a> {
    /// A 360 pt dialog with no buttons.
    pub fn new(id_salt: impl std::hash::Hash + std::fmt::Debug, title: &'a str) -> Self {
        Self {
            id: Id::unique(("cuboid_modal", id_salt)),
            title,
            width: DIALOG_WIDTHS[0],
            buttons: &[],
            palette: false,
        }
    }

    /// 360, 520 or 720 pt wide; any other value is rounded to the nearest of them.
    pub fn width(mut self, width: f32) -> Self {
        self.width = DIALOG_WIDTHS
            .into_iter()
            .min_by(|a, b| (a - width).abs().total_cmp(&(b - width).abs()))
            .unwrap_or(DIALOG_WIDTHS[0]);
        self
    }

    /// A palette instead of a dialog: no heading, anchored near the top edge, and a click on the
    /// backdrop cancels as well as Esc. The title only names the dialog.
    pub fn palette(mut self) -> Self {
        self.palette = true;
        self
    }

    /// The footer buttons from left to right; the last one is the confirming one.
    pub fn buttons(mut self, buttons: &'a [ModalButton<'a>]) -> Self {
        self.buttons = buttons;
        self
    }

    /// Shows the dialog above everything for this frame. Call it every frame while it is open.
    pub fn show<R>(self, ctx: &Context, body: impl FnOnce(&mut Ui) -> R) -> ModalOutcome<R> {
        let t = Tokens::of(ctx);
        let frame = Frame::new()
            .fill(t.bg_raised)
            .stroke(Stroke::new(1.0, t.border_strong))
            .corner_radius(radius::MODAL)
            .inner_margin(Margin::same(16))
            .shadow(Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(if t.dark { 150 } else { 60 }),
            });
        let mut button = None;
        let mut modal = egui::Modal::new(self.id)
            .backdrop_color(Color32::from_black_alpha(BACKDROP_ALPHA))
            .frame(frame);
        if self.palette {
            let area = egui::Modal::default_area(self.id)
                .anchor(Align2::CENTER_TOP, vec2(0.0, PALETTE_TOP));
            modal = modal.area(area);
        }
        let response = modal.show(ctx, |ui| {
            let inner = self.width - 2.0 * space::LG;
            ui.set_width(inner);
            if !self.palette {
                ui.label(text(Type::Heading, self.title));
                ui.add_space(space::MD);
            }
            let result = body(ui);
            if !self.buttons.is_empty() {
                ui.add_space(space::LG);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for (index, spec) in self.buttons.iter().enumerate().rev() {
                        let mut b = Button::new(spec.label).min_width(FOOTER_BUTTON);
                        b = match spec.kind {
                            ButtonKind::Primary => b.primary(),
                            ButtonKind::Danger => b.danger(),
                            ButtonKind::Ghost => b.ghost(),
                            ButtonKind::Secondary => b,
                        };
                        if ui.add_enabled(spec.enabled, b.tall()).clicked() {
                            button = Some(index);
                        }
                    }
                });
            }
            result
        });
        let top = response.is_top_modal && !response.any_popup_open;
        let escape = top && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let cancelled = escape || (self.palette && response.backdrop_response.clicked());
        let confirmed = top
            && self.buttons.last().is_some_and(|b| b.enabled)
            && !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        ModalOutcome {
            inner: response.inner,
            button,
            cancelled,
            confirmed,
        }
    }
}
