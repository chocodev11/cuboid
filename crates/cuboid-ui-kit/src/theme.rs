//! Themes: colour tokens, spacing and the egui style built from them.
//!
//! The look is graphite with a faint green undertone, small corner radii and crisp 1 px lines.
//! Every colour pair that carries text or a control is checked for WCAG contrast by the tests
//! below, in the dark, light and both high-contrast palettes, for every accent preset.

use egui::{
    Color32, Context, CornerRadius, FontId, Id, Margin, Shadow, Stroke, Style, TextStyle,
    Theme as EguiTheme, Visuals, style::HandleShape, vec2,
};

/// Smallest UI scale.
pub const UI_SCALE_MIN: f32 = 0.75;
/// Largest UI scale.
pub const UI_SCALE_MAX: f32 = 2.0;

/// Corner radius of chips, swatches and thin bars.
pub const RADIUS_SM: u8 = 2;
/// Corner radius of controls and rows.
pub const RADIUS: u8 = 3;
/// Corner radius of windows, menus, cards and toasts.
pub const RADIUS_LG: u8 = 4;

/// The spacing grid in points; paddings, gaps and control heights are multiples of it.
pub const GRID: f32 = 4.0;

/// Accent colours offered by the preferences (Orchid, Fuchsia, Citron, Lagoon, Gold, Mint); the
/// first is the default. The palette fits each one to the surfaces, so a preset keeps its hue in
/// both the dark and the light theme.
pub const ACCENT_PRESETS: [[u8; 3]; 6] = [
    [0xa9, 0x8b, 0xf2],
    [0xe4, 0x6a, 0xb4],
    [0xb6, 0xd6, 0x3c],
    [0x2f, 0xc3, 0xd3],
    [0xe3, 0xbb, 0x3b],
    [0x4f, 0xd1, 0xa1],
];

/// Which palette to use.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ThemeMode {
    /// Dark panels on a darker surround.
    #[default]
    Dark,
    /// Light panels.
    Light,
    /// Follow the operating system; dark when it cannot be told.
    System,
}

/// Everything that decides how the UI looks.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Theme {
    /// Dark, light or system.
    pub mode: ThemeMode,
    /// Accent colour as RGB.
    pub accent: [u8; 3],
    /// Pure black or white surfaces and stronger borders.
    pub high_contrast: bool,
    /// UI scale, clamped to [`UI_SCALE_MIN`] .. [`UI_SCALE_MAX`].
    pub scale: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            accent: ACCENT_PRESETS[0],
            high_contrast: false,
            scale: 1.0,
        }
    }
}

/// Resolved colours; widgets read them with [`Tokens::of`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tokens {
    /// Behind the viewport and between panels.
    pub bg_app: Color32,
    /// Panel surface.
    pub bg_panel: Color32,
    /// Raised surface: menus, cards, popups.
    pub bg_raised: Color32,
    /// Text fields and wells.
    pub bg_input: Color32,
    /// Hovered control.
    pub bg_hover: Color32,
    /// Pressed or selected-neutral control.
    pub bg_active: Color32,
    /// Backdrop of the 3D view while no frame is available; also the recommended clear colour.
    pub bg_viewport: Color32,
    /// Hairlines between panels (decoration only).
    pub border: Color32,
    /// Outlines of controls; at least 3:1 on every control surface.
    pub border_strong: Color32,
    /// Outlines of hovered controls; at least 3:1 on the hover and pressed fills.
    pub border_hover: Color32,
    /// Keyboard focus ring; neutral, so it never reads as the accent or as a selection.
    pub focus: Color32,
    /// Body text.
    pub text: Color32,
    /// Secondary text.
    pub text_weak: Color32,
    /// Headings and emphasised text.
    pub text_strong: Color32,
    /// Accent colour.
    pub accent: Color32,
    /// Accent for hovered controls.
    pub accent_hover: Color32,
    /// Fill of selected rows and tools: a tint of the accent over the panel.
    pub accent_soft: Color32,
    /// Text on an accent fill.
    pub on_accent: Color32,
    /// Destructive actions and errors.
    pub danger: Color32,
    /// Warnings.
    pub warn: Color32,
    /// Success.
    pub ok: Color32,
    /// Dark theme.
    pub dark: bool,
}

const TOKENS_ID: &str = "cuboid_ui_kit_tokens";
const APPLIED_ID: &str = "cuboid_ui_kit_applied";

impl Tokens {
    /// The tokens last applied to this context; the dark default before the first
    /// [`Theme::apply`].
    pub fn of(ctx: &Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(Id::unique(TOKENS_ID)))
            .unwrap_or_else(|| Theme::default().tokens(true))
    }
}

fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t)
}

fn linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
fn luminance(c: Color32) -> f32 {
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

/// WCAG contrast ratio of two opaque colours, 1 to 21.
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Moves `color` toward `toward` in 40 steps until it reaches the ratio against every
/// `(surface, ratio)` pair; the end point when none does.
pub fn fit_contrast(color: Color32, toward: Color32, needs: &[(Color32, f32)]) -> Color32 {
    (0..=40_u8)
        .map(|step| mix(color, toward, f32::from(step) / 40.0))
        .find(|c| {
            needs
                .iter()
                .all(|&(bg, ratio)| contrast_ratio(*c, bg) >= ratio)
        })
        .unwrap_or(toward)
}

impl Theme {
    /// The scale inside the allowed range; a NaN counts as 1.
    pub fn clamped_scale(&self) -> f32 {
        if self.scale.is_nan() {
            return 1.0;
        }
        self.scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX)
    }

    /// Whether the dark palette is used; `system_dark` is the operating system's answer.
    pub fn is_dark(&self, system_dark: Option<bool>) -> bool {
        match self.mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => system_dark.unwrap_or(true),
        }
    }

    /// The colours for the dark or light palette.
    pub fn tokens(&self, dark: bool) -> Tokens {
        let hc = self.high_contrast;
        // Surfaces: app, input, panel, raised, hover, active, viewport.
        let [
            bg_app,
            bg_input,
            bg_panel,
            bg_raised,
            bg_hover,
            bg_active,
            bg_viewport,
        ] = match (dark, hc) {
            (true, false) => [
                0x0f1210, 0x131714, 0x1a1f1c, 0x242a26, 0x2e3530, 0x3a423c, 0x222824,
            ],
            (true, true) => [
                0x000000, 0x000000, 0x060807, 0x0f1311, 0x232a25, 0x38413a, 0x050706,
            ],
            (false, false) => [
                0xd6dbd2, 0xfbfcf9, 0xe9ebe4, 0xf8f9f5, 0xdadfd3, 0xc8cec2, 0xcfd4cb,
            ],
            (false, true) => [
                0xeef1ec, 0xffffff, 0xffffff, 0xffffff, 0xe3e8e1, 0xcdd4cc, 0xffffff,
            ],
        }
        .map(rgb);
        // Hairline, control outline, hovered outline, focus ring.
        let [border, border_strong, border_hover, focus] = match (dark, hc) {
            (true, false) => [0x2c332e, 0x6f7a72, 0xa5b0a8, 0xf6f9f4],
            (true, true) => [0x8f9b92, 0xdfe8e1, 0xffffff, 0xffffff],
            (false, false) => [0xc9cfc3, 0x7b857a, 0x4d574f, 0x0b0e0c],
            (false, true) => [0x4d5750, 0x000000, 0x000000, 0x000000],
        }
        .map(rgb);
        let [text, text_weak, text_strong] = match (dark, hc) {
            (true, false) => [0xdfe5de, 0xaab4ac, 0xf6f9f4],
            (true, true) => [0xffffff, 0xd2dad4, 0xffffff],
            (false, false) => [0x1b201c, 0x4a544c, 0x0b0e0c],
            (false, true) => [0x000000, 0x2d352f, 0x000000],
        }
        .map(rgb);

        // Text needs 4.5:1 (7:1 in high contrast); a fill or an icon 3:1 (4.5:1).
        let (text_ratio, ui_ratio) = if hc { (7.0, 4.5) } else { (4.5, 3.0) };
        let surfaces = [bg_panel, bg_raised, bg_input];
        let pressed = [bg_hover, bg_active];
        let ink = rgb(0x0b0e0c);
        let base = Color32::from_rgb(self.accent[0], self.accent[1], self.accent[2]);
        // Dark themes lighten the accent until dark ink reads on it; light themes darken it until
        // white text reads on it.
        let (accent, on_accent) = if dark {
            let mut needs = surfaces.map(|s| (s, text_ratio)).to_vec();
            needs.extend(pressed.map(|s| (s, ui_ratio)));
            needs.push((ink, text_ratio + 0.2));
            (fit_contrast(base, Color32::WHITE, &needs), ink)
        } else {
            let mut needs = surfaces.map(|s| (s, ui_ratio)).to_vec();
            // The margin keeps it above 3:1 on the accent-tinted selection fill too.
            needs.extend(pressed.map(|s| (s, ui_ratio + 0.3)));
            needs.push((Color32::WHITE, text_ratio + 0.2));
            (fit_contrast(base, Color32::BLACK, &needs), Color32::WHITE)
        };
        let accent_hover = mix(
            accent,
            if dark { Color32::WHITE } else { Color32::BLACK },
            0.16,
        );
        let status = |dark_hex: u32, light_hex: u32| {
            let (base, toward) = if dark {
                (rgb(dark_hex), Color32::WHITE)
            } else {
                (rgb(light_hex), Color32::BLACK)
            };
            let needs = surfaces.map(|s| (s, text_ratio));
            fit_contrast(base, toward, &needs)
        };
        Tokens {
            bg_app,
            bg_panel,
            bg_raised,
            bg_input,
            bg_hover,
            bg_active,
            bg_viewport,
            border,
            border_strong,
            border_hover,
            focus,
            text,
            text_weak,
            text_strong,
            accent,
            accent_hover,
            accent_soft: mix(bg_panel, accent, if hc { 0.2 } else { 0.22 }),
            on_accent,
            danger: status(0xf0766b, 0xb3261e),
            warn: status(0xe8aa3c, 0x8a5a00),
            ok: status(0x5fcb82, 0x16743f),
            dark,
        }
    }

    /// Installs the style, the tokens and the zoom factor. Cheap when nothing changed since the
    /// last call, so the UI calls it every frame.
    ///
    /// The scale is egui's zoom factor, which multiplies the pixel ratio the shell reports.
    pub fn apply(&self, ctx: &Context) {
        let system_dark = ctx.system_theme().map(|t| t == EguiTheme::Dark);
        let dark = self.is_dark(system_dark);
        let applied = (*self, dark);
        if ctx.data(|d| d.get_temp::<(Theme, bool)>(Id::unique(APPLIED_ID))) == Some(applied) {
            return;
        }
        let tokens = self.tokens(dark);
        let egui_theme = if dark {
            EguiTheme::Dark
        } else {
            EguiTheme::Light
        };
        ctx.set_theme(egui_theme);
        ctx.set_style_of(egui_theme, build_style(&tokens, self.high_contrast));
        ctx.set_zoom_factor(self.clamped_scale());
        ctx.data_mut(|d| {
            d.insert_temp(Id::unique(TOKENS_ID), tokens);
            d.insert_temp(Id::unique(APPLIED_ID), applied);
        });
    }
}

fn stroke(w: f32, c: Color32) -> Stroke {
    Stroke::new(w, c)
}

fn build_style(t: &Tokens, hc: bool) -> Style {
    let mut style = Style::default();
    let radius = CornerRadius::same(RADIUS);
    let line = if hc { 2.0 } else { 1.0 };

    style.spacing.item_spacing = vec2(2.0 * GRID, GRID);
    style.spacing.button_padding = vec2(2.0 * GRID, GRID);
    style.spacing.interact_size = vec2(7.0 * GRID, 6.0 * GRID);
    style.spacing.menu_margin = Margin::same(4);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.indent = 4.0 * GRID;
    style.spacing.icon_width = 4.0 * GRID;
    style.spacing.icon_width_inner = 2.0 * GRID;
    style.spacing.slider_width = 30.0 * GRID;
    style.spacing.combo_width = 30.0 * GRID;
    style.spacing.text_edit_width = 40.0 * GRID;
    style.interaction.tooltip_delay = 0.4;
    style.interaction.selectable_labels = false;

    style.text_styles = [
        (TextStyle::Small, FontId::proportional(11.5)),
        (TextStyle::Body, FontId::proportional(13.5)),
        (TextStyle::Button, FontId::proportional(13.5)),
        (TextStyle::Heading, FontId::proportional(17.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();

    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.bg_panel;
    v.window_fill = t.bg_raised;
    v.extreme_bg_color = t.bg_input;
    v.text_edit_bg_color = Some(t.bg_input);
    v.faint_bg_color = t.bg_hover.gamma_multiply(0.45);
    v.code_bg_color = t.bg_input;
    v.window_stroke = stroke(line, t.border_strong);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS_LG);
    v.window_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 140 } else { 50 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 120 } else { 45 }),
    };
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.warn;
    v.error_fg_color = t.danger;
    // Text selection and slider fills use the soft tint; the focus ring is the neutral token.
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = stroke(line, t.focus);
    v.text_cursor.stroke = stroke(2.0, t.accent);
    v.override_text_color = None;
    v.slider_trailing_fill = true;
    v.handle_shape = HandleShape::Rect { aspect_ratio: 0.5 };
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.bg_panel;
    w.noninteractive.weak_bg_fill = t.bg_panel;
    w.noninteractive.bg_stroke = stroke(1.0, t.border);
    w.noninteractive.fg_stroke = stroke(1.0, t.text);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = t.bg_raised;
    w.inactive.weak_bg_fill = t.bg_raised;
    w.inactive.bg_stroke = stroke(1.0, t.border_strong);
    w.inactive.fg_stroke = stroke(1.0, t.text);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = t.bg_hover;
    w.hovered.weak_bg_fill = t.bg_hover;
    w.hovered.bg_stroke = stroke(1.0, t.border_hover);
    w.hovered.fg_stroke = stroke(1.5, t.text_strong);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = t.bg_active;
    w.active.weak_bg_fill = t.bg_active;
    w.active.bg_stroke = stroke(1.0, t.accent);
    w.active.fg_stroke = stroke(1.5, t.text_strong);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = t.bg_active;
    w.open.weak_bg_fill = t.bg_active;
    w.open.bg_stroke = stroke(1.0, t.border_hover);
    w.open.fg_stroke = stroke(1.0, t.text_strong);
    w.open.corner_radius = radius;

    style.visuals = v;
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palettes() -> Vec<(String, Tokens, bool)> {
        let mut all = Vec::new();
        for accent in ACCENT_PRESETS {
            for dark in [true, false] {
                for hc in [false, true] {
                    let theme = Theme {
                        accent,
                        high_contrast: hc,
                        ..Theme::default()
                    };
                    let name = format!(
                        "{} {} {accent:02x?}",
                        if dark { "dark" } else { "light" },
                        if hc { "hc" } else { "normal" },
                    );
                    all.push((name, theme.tokens(dark), hc));
                }
            }
        }
        all
    }

    fn distance(a: Color32, b: Color32) -> f32 {
        let d = |x: u8, y: u8| (f32::from(x) - f32::from(y)).powi(2);
        (d(a.r(), b.r()) + d(a.g(), b.g()) + d(a.b(), b.b())).sqrt()
    }

    #[test]
    fn scale_is_clamped_and_nan_is_one() {
        let with = |scale| Theme {
            scale,
            ..Theme::default()
        };
        assert!((with(5.0).clamped_scale() - UI_SCALE_MAX).abs() < f32::EPSILON);
        assert!((with(0.1).clamped_scale() - UI_SCALE_MIN).abs() < f32::EPSILON);
        assert!((with(f32::NAN).clamped_scale() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn system_mode_follows_the_answer_and_defaults_to_dark() {
        let theme = Theme {
            mode: ThemeMode::System,
            ..Theme::default()
        };
        assert!(theme.is_dark(None));
        assert!(!theme.is_dark(Some(false)));
        assert!(Theme::default().is_dark(Some(false)));
    }

    #[test]
    fn apply_installs_tokens_and_is_idempotent() {
        let ctx = Context::default();
        let theme = Theme {
            mode: ThemeMode::Light,
            accent: ACCENT_PRESETS[3],
            high_contrast: false,
            scale: 1.5,
        };
        theme.apply(&ctx);
        let tokens = Tokens::of(&ctx);
        assert!(!tokens.dark);
        assert_eq!(tokens.bg_panel, rgb(0xe9ebe4));
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        assert!((ctx.zoom_factor() - 1.5).abs() < f32::EPSILON);
        theme.apply(&ctx);
        assert_eq!(Tokens::of(&ctx), tokens);
    }

    #[test]
    fn contrast_ratio_matches_the_wcag_reference_values() {
        assert!((contrast_ratio(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.001);
        // #767676 on white is the classic 4.54:1 edge.
        assert!((contrast_ratio(rgb(0x767676), Color32::WHITE) - 4.54).abs() < 0.01);
    }

    #[test]
    fn text_reaches_aa_on_every_surface_it_sits_on() {
        for (name, t, hc) in palettes() {
            let min = if hc { 7.0 } else { 4.5 };
            let surfaces = [
                ("app", t.bg_app),
                ("panel", t.bg_panel),
                ("raised", t.bg_raised),
                ("input", t.bg_input),
                ("hover", t.bg_hover),
                ("active", t.bg_active),
                ("viewport", t.bg_viewport),
                ("selection", t.accent_soft),
            ];
            for (what, text) in [
                ("text", t.text),
                ("text_weak", t.text_weak),
                ("text_strong", t.text_strong),
            ] {
                for (surface, bg) in surfaces {
                    let ratio = contrast_ratio(text, bg);
                    assert!(ratio >= min, "{name}: {what} on {surface} is {ratio:.2}");
                }
            }
            for (what, fg, bg) in [
                ("on_accent on accent", t.on_accent, t.accent),
                ("on_accent on accent_hover", t.on_accent, t.accent_hover),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(ratio >= min, "{name}: {what} is {ratio:.2}");
            }
        }
    }

    #[test]
    fn status_colours_and_accent_read_as_text_on_panels() {
        for (name, t, hc) in palettes() {
            let min = if hc { 7.0 } else { 4.5 };
            for (what, fg) in [("danger", t.danger), ("warn", t.warn), ("ok", t.ok)] {
                for bg in [t.bg_panel, t.bg_raised, t.bg_input] {
                    let ratio = contrast_ratio(fg, bg);
                    assert!(ratio >= min, "{name}: {what} is {ratio:.2}");
                }
            }
            // The accent as an icon or an outline: 3:1 (4.5:1 in high contrast) everywhere.
            let ui = if hc { 4.5 } else { 3.0 };
            for bg in [
                t.bg_panel,
                t.bg_raised,
                t.bg_input,
                t.bg_hover,
                t.bg_active,
                t.accent_soft,
            ] {
                let ratio = contrast_ratio(t.accent, bg);
                assert!(ratio >= ui, "{name}: accent is {ratio:.2}");
            }
        }
    }

    #[test]
    fn control_outlines_and_focus_reach_three_to_one() {
        for (name, t, hc) in palettes() {
            let min = if hc { 4.5 } else { 3.0 };
            for (what, line, surfaces) in [
                (
                    "border_strong",
                    t.border_strong,
                    vec![t.bg_panel, t.bg_raised, t.bg_input],
                ),
                (
                    "border_hover",
                    t.border_hover,
                    vec![t.bg_hover, t.bg_active, t.bg_panel, t.bg_raised],
                ),
                (
                    "focus",
                    t.focus,
                    vec![
                        t.bg_panel,
                        t.bg_raised,
                        t.bg_input,
                        t.bg_hover,
                        t.bg_active,
                        t.accent_soft,
                    ],
                ),
            ] {
                for bg in surfaces {
                    let ratio = contrast_ratio(line, bg);
                    assert!(ratio >= min, "{name}: {what} on {bg:?} is {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn surface_levels_are_clearly_stepped() {
        for (name, t, hc) in palettes() {
            let steps = [
                ("hover vs panel", t.bg_hover, t.bg_panel),
                ("active vs hover", t.bg_active, t.bg_hover),
            ];
            for (what, a, b) in steps {
                let ratio = contrast_ratio(a, b);
                assert!(ratio >= 1.1, "{name}: {what} is {ratio:.2}");
            }
            // High contrast separates panels with outlines instead of fills.
            if !hc {
                for (what, a, b) in [
                    ("panel vs app", t.bg_panel, t.bg_app),
                    ("raised vs panel", t.bg_raised, t.bg_panel),
                ] {
                    let ratio = contrast_ratio(a, b);
                    assert!(ratio >= 1.1, "{name}: {what} is {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn selection_hover_focus_and_accent_look_different() {
        for (name, t, hc) in palettes() {
            // High contrast tells selection from hover by the accent bar and the outline.
            if !hc {
                assert!(
                    distance(t.accent_soft, t.bg_hover) >= 20.0,
                    "{name}: selection and hover"
                );
            }
            assert!(
                distance(t.accent_soft, t.accent) >= 80.0,
                "{name}: selection and accent"
            );
            assert!(
                distance(t.focus, t.accent) >= 80.0,
                "{name}: focus and accent"
            );
            assert!(
                distance(t.focus, t.accent_soft) >= 120.0,
                "{name}: focus and selection"
            );
            assert!(
                distance(t.focus, t.bg_hover) >= 120.0,
                "{name}: focus and hover"
            );
        }
    }

    #[test]
    fn surfaces_are_not_the_old_blue_grey_theme() {
        let t = Theme::default().tokens(true);
        for old in [
            0x14161a, 0x1e2126, 0x282c33, 0x15171b, 0x30353d, 0x3a404a, 0x2e323a, 0x464c58,
        ] {
            assert!(
                [
                    t.bg_app,
                    t.bg_panel,
                    t.bg_raised,
                    t.bg_input,
                    t.bg_hover,
                    t.bg_active,
                    t.border,
                    t.border_strong
                ]
                .iter()
                .all(|c| *c != rgb(old))
            );
        }
        assert!(!ACCENT_PRESETS.contains(&[0x4f, 0x8c, 0xff]));
    }
}
