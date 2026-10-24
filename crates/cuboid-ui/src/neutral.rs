//! The neutral highlight of a fresh install: until the user picks an accent, selected and
//! focused states use the theme's strong text instead of an accent colour.

use cuboid_ui_kit::{Theme, Tokens};
use egui::Context;

/// Replaces the accent of the tokens that `Theme::apply` installed. Cheap when done already.
pub(crate) fn apply(ctx: &Context) {
    let tokens = Tokens::of(ctx);
    let neutral = tokens.neutral_highlight();
    if tokens == neutral {
        return;
    }
    neutral.store(ctx);
    let theme = if neutral.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.style_mut_of(theme, |style| {
        let v = &mut style.visuals;
        v.hyperlink_color = neutral.accent;
        v.selection.bg_fill = neutral.accent_soft;
        v.text_cursor.stroke.color = neutral.accent;
        v.widgets.active.bg_stroke.color = neutral.accent;
    });
}

/// Makes the next `Theme::apply` install the real tokens again.
pub(crate) fn forget(ctx: &Context) {
    Theme::forget_applied(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;

    #[test]
    fn the_override_reaches_the_kit_tokens() {
        let ctx = Context::default();
        let theme = Prefs::default().theme();
        theme.apply(&ctx);
        let real = Tokens::of(&ctx);
        apply(&ctx);
        let neutral = Tokens::of(&ctx);
        assert_eq!(neutral.accent, real.text_strong);
        assert_ne!(neutral.accent, real.accent);
        assert_eq!(neutral.bg_panel, real.bg_panel);
        // Applying the same theme again keeps the neutral tokens.
        theme.apply(&ctx);
        apply(&ctx);
        assert_eq!(Tokens::of(&ctx).accent, real.text_strong);
        // After forgetting, the same theme installs the real accent again.
        forget(&ctx);
        theme.apply(&ctx);
        assert_eq!(Tokens::of(&ctx).accent, real.accent);
    }

    #[test]
    fn the_neutral_highlight_reads_on_every_surface() {
        for dark in [true, false] {
            let t = Prefs::default().theme().tokens(dark).neutral_highlight();
            for bg in [t.bg_panel, t.bg_raised, t.bg_input] {
                assert!(
                    cuboid_ui_kit::contrast_ratio(t.accent, bg) >= 4.5,
                    "dark {dark}"
                );
            }
            assert!(
                cuboid_ui_kit::contrast_ratio(t.on_accent, t.accent) >= 4.5,
                "dark {dark}"
            );
        }
    }
}
