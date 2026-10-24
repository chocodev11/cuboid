//! Toast notifications: short messages in the bottom right corner that fade out by themselves.

use std::time::Duration;

use egui::{Align2, Context, Frame, Id, Label, Margin, Order, Sense, Stroke, vec2};

use crate::icons::Icon;
use crate::metrics::{TOAST_MAX, TOAST_WIDTH, height, icon_size, radius, space};
use crate::theme::Tokens;
use crate::typography::{Type, text};
use crate::widgets::button::{Button, IconButton};
use crate::widgets::paint::paint_icon;

/// How serious a message is.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Level {
    /// A neutral message.
    Info,
    /// Something finished.
    Success,
    /// Something went wrong but work continues.
    Warning,
    /// An operation failed.
    Error,
}

impl Level {
    /// Seconds a toast of this level stays.
    fn lifetime(self) -> f64 {
        match self {
            Level::Info | Level::Success => 4.0,
            Level::Warning => 7.0,
            Level::Error => 10.0,
        }
    }
}

/// An action button on a toast: its label and the tag [`Toasts::show`] reports when it is clicked.
#[derive(Clone, Debug)]
struct ToastAction {
    label: String,
    tag: u32,
}

#[derive(Clone, Debug)]
struct Toast {
    id: u64,
    level: Level,
    text: String,
    action: Option<ToastAction>,
    /// Time of the first frame it was shown; `None` until then.
    born: Option<f64>,
    repeats: u32,
}

/// The list of live toasts. Push from anywhere, call [`Toasts::show`] once per frame.
#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    /// Adds a message. The same text twice in a row only bumps a counter and restarts the timer.
    pub fn push(&mut self, level: Level, text: impl Into<String>) {
        self.add(level, text.into(), None);
    }

    /// Adds a message with an action button; [`Toasts::show`] returns `tag` when it is clicked.
    pub fn push_with_action(
        &mut self,
        level: Level,
        text: impl Into<String>,
        label: impl Into<String>,
        tag: u32,
    ) {
        let action = ToastAction {
            label: label.into(),
            tag,
        };
        self.add(level, text.into(), Some(action));
    }

    fn add(&mut self, level: Level, text: String, action: Option<ToastAction>) {
        if let Some(last) = self.items.last_mut()
            && last.level == level
            && last.text == text
        {
            last.repeats = last.repeats.saturating_add(1);
            last.born = None;
            return;
        }
        self.next_id += 1;
        self.items.push(Toast {
            id: self.next_id,
            level,
            text,
            action,
            born: None,
            repeats: 1,
        });
        if self.items.len() > TOAST_MAX {
            self.items.remove(0);
        }
    }

    /// Number of live toasts.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// No toast is live.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The texts, oldest first.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|toast| toast.text.as_str())
    }

    /// Removes every toast.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Draws the toasts above everything in the bottom right corner, 320 pt wide, and drops the
    /// expired ones. A hovered toast stays; the close button removes it. Returns the tag of the
    /// action that was clicked, which also removes its toast.
    pub fn show(&mut self, ctx: &Context) -> Option<u32> {
        if self.items.is_empty() {
            return None;
        }
        let now = ctx.input(|i| i.time);
        let t = Tokens::of(ctx);
        let mut dismissed = Vec::new();
        let mut next_expiry: Option<f64> = None;
        let mut action_tag = None;
        let area = egui::Area::new(Id::unique("cuboid_toasts"))
            .anchor(
                Align2::RIGHT_BOTTOM,
                vec2(-space::LG, -(height::CONTROL + space::LG)),
            )
            .order(Order::Tooltip)
            .interactable(true);
        area.show(ctx, |ui| {
            ui.set_width(TOAST_WIDTH);
            ui.spacing_mut().item_spacing.y = space::SM;
            for toast in &mut self.items {
                let born = *toast.born.get_or_insert(now);
                let (tone, icon) = match toast.level {
                    Level::Info => (t.text_weak, Icon::Info),
                    Level::Success => (t.ok, Icon::CheckCircle),
                    Level::Warning => (t.warn, Icon::Warning),
                    Level::Error => (t.danger, Icon::Error),
                };
                let frame = Frame::new()
                    .fill(t.bg_raised)
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(radius::CARD)
                    .inner_margin(Margin::symmetric(12, 8));
                let shown = frame.show(ui, |ui| {
                    ui.set_width(TOAST_WIDTH - 24.0);
                    ui.horizontal(|ui| {
                        let (icon_rect, _) = ui.allocate_exact_size(
                            vec2(icon_size::CONTROL, icon_size::CONTROL),
                            Sense::hover(),
                        );
                        paint_icon(ui, icon, icon_rect.center(), icon_size::CONTROL, tone);
                        let message = if toast.repeats > 1 {
                            format!("{}  (x{})", toast.text, toast.repeats)
                        } else {
                            toast.text.clone()
                        };
                        let reserved = height::CONTROL
                            + toast.action.as_ref().map_or(0.0, |_| 64.0)
                            + space::SM;
                        ui.scope(|ui| {
                            ui.set_max_width((ui.available_width() - reserved).max(80.0));
                            ui.add(
                                Label::new(text(Type::Body, message))
                                    .wrap()
                                    .selectable(false),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let closed = ui.add(IconButton::new(Icon::Close, "Dismiss")).clicked();
                            let acted = toast
                                .action
                                .as_ref()
                                .map(|a| (ui.add(Button::new(&a.label).ghost()).clicked(), a.tag));
                            (closed, acted)
                        })
                        .inner
                    })
                    .inner
                });
                let (closed, acted) = shown.inner;
                if closed {
                    dismissed.push(toast.id);
                }
                if let Some((true, tag)) = acted {
                    action_tag = Some(tag);
                    dismissed.push(toast.id);
                }
                if shown.response.hovered() {
                    toast.born = Some(now);
                }
                let expiry = toast.born.unwrap_or(born) + toast.level.lifetime();
                next_expiry = Some(next_expiry.map_or(expiry, |e| e.min(expiry)));
            }
        });
        self.items.retain(|toast| {
            let alive = toast
                .born
                .is_none_or(|born| now - born < toast.level.lifetime());
            alive && !dismissed.contains(&toast.id)
        });
        if let Some(expiry) = next_expiry {
            ctx.request_repaint_after(Duration::from_secs_f64((expiry - now).clamp(0.05, 1.0)));
        }
        action_tag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_text_is_merged_and_the_stack_holds_three() {
        let mut toasts = Toasts::default();
        toasts.push(Level::Info, "Saved");
        toasts.push(Level::Info, "Saved");
        assert_eq!(toasts.len(), 1);
        toasts.push(Level::Error, "Saved");
        assert_eq!(toasts.len(), 2);
        for i in 0..20 {
            toasts.push(Level::Info, format!("message {i}"));
        }
        assert_eq!(toasts.len(), TOAST_MAX);
        assert_eq!(TOAST_MAX, 3);
        assert_eq!(toasts.texts().last(), Some("message 19"));
    }

    #[test]
    fn toasts_expire_after_their_lifetime() {
        let ctx = Context::default();
        let mut toasts = Toasts::default();
        toasts.push(Level::Info, "Hello");
        let run = |ctx: &Context, toasts: &mut Toasts, time: f64| {
            let input = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            ctx.run_ui(input, |_| {
                toasts.show(ctx);
            })
            .drop_without_applying_deltas();
        };
        run(&ctx, &mut toasts, 1.0);
        assert_eq!(toasts.len(), 1);
        run(&ctx, &mut toasts, 3.0);
        assert_eq!(toasts.len(), 1);
        run(&ctx, &mut toasts, 5.5);
        assert!(toasts.is_empty());
    }
}
