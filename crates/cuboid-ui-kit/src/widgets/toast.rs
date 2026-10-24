//! Toast notifications: short messages in a corner that fade out by themselves.

use std::time::Duration;

use egui::{Align2, Context, FontId, Frame, Id, Margin, Order, Sense, Stroke, vec2};

use crate::icons::Icon;
use crate::theme::{RADIUS_LG, Tokens};

/// Longest list kept; older toasts are dropped first.
const MAX_TOASTS: usize = 6;

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

#[derive(Clone, Debug)]
struct Toast {
    id: u64,
    level: Level,
    text: String,
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
        let text = text.into();
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
            born: None,
            repeats: 1,
        });
        if self.items.len() > MAX_TOASTS {
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

    /// Draws the toasts above everything in the bottom right corner and drops the expired ones.
    /// A hovered toast stays; the close button removes it.
    pub fn show(&mut self, ctx: &Context) {
        if self.items.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let tokens = Tokens::of(ctx);
        let mut dismissed = Vec::new();
        let mut next_expiry: Option<f64> = None;
        let area = egui::Area::new(Id::unique("cuboid_toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -40.0))
            .order(Order::Tooltip)
            .interactable(true);
        area.show(ctx, |ui| {
            ui.set_max_width(340.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            for toast in &mut self.items {
                let born = *toast.born.get_or_insert(now);
                let (accent, icon) = match toast.level {
                    Level::Info => (tokens.accent, Icon::Info),
                    Level::Success => (tokens.ok, Icon::Check),
                    Level::Warning => (tokens.warn, Icon::Warning),
                    Level::Error => (tokens.danger, Icon::Error),
                };
                let frame = Frame::new()
                    .fill(tokens.bg_raised)
                    .stroke(Stroke::new(1.0, tokens.border_strong))
                    .corner_radius(RADIUS_LG)
                    .inner_margin(Margin::symmetric(12, 8));
                let shown = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        icon.paint(ui.painter(), icon_rect, accent);
                        let text = if toast.repeats > 1 {
                            format!("{}  (x{})", toast.text, toast.repeats)
                        } else {
                            toast.text.clone()
                        };
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(text)
                                    .font(FontId::proportional(13.0))
                                    .color(tokens.text),
                            )
                            .wrap(),
                        );
                        let (close, resp) =
                            ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                        let color = if resp.hovered() {
                            tokens.text_strong
                        } else {
                            tokens.text_weak
                        };
                        Icon::Close.paint(ui.painter(), close.shrink(3.0), color);
                        resp.clicked()
                    })
                    .inner
                });
                if shown.inner {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_text_is_merged_and_the_list_is_capped() {
        let mut toasts = Toasts::default();
        toasts.push(Level::Info, "Saved");
        toasts.push(Level::Info, "Saved");
        assert_eq!(toasts.len(), 1);
        toasts.push(Level::Error, "Saved");
        assert_eq!(toasts.len(), 2);
        for i in 0..20 {
            toasts.push(Level::Info, format!("message {i}"));
        }
        assert_eq!(toasts.len(), MAX_TOASTS);
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
            ctx.run_ui(input, |_| toasts.show(ctx))
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
