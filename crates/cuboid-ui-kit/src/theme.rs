//! Themes: colour tokens, spacing and the egui style built from them.
//!
//! Two families: Catppuccin (the default; Latte for the light theme, Frappé, Macchiato or Mocha for
//! the dark one) and the cuboid Graphite palette. Small corner radii and crisp 1 px lines in both.
//! Every colour pair that carries text or a control is checked for WCAG contrast by the tests
//! below, in every family, flavour, accent preset and both high-contrast variants.

use catppuccin::{Color as CatColor, Flavor as CatFlavor, PALETTE};
use egui::{
    Color32, Context, CornerRadius, Id, Margin, Shadow, Stroke, Style, TextStyle,
    Theme as EguiTheme, Visuals, style::HandleShape, vec2,
};
use serde::{Deserialize, Serialize};

use crate::metrics::{FOCUS_RING, radius, space};
use crate::typography::Type;

/// Smallest UI scale.
pub const UI_SCALE_MIN: f32 = 0.75;
/// Largest UI scale.
pub const UI_SCALE_MAX: f32 = 2.0;

/// Corner radius of chips, swatches and thin bars; the same 3 as the controls.
pub const RADIUS_SM: u8 = radius::CONTROL;
/// Corner radius of controls and rows.
pub const RADIUS: u8 = radius::CONTROL;
/// Corner radius of cards, panels, menus and toasts.
pub const RADIUS_LG: u8 = radius::CARD;
/// Corner radius of modals.
pub const RADIUS_MODAL: u8 = radius::MODAL;

/// The spacing grid in points; paddings, gaps and control heights are multiples of it.
pub const GRID: f32 = space::XS;

/// Accent colours of the Graphite family (Gold, Orchid, Fuchsia, Citron, Lagoon, Mint); the first
/// is the default. The palette fits each one to the surfaces, so a preset keeps its hue in both
/// the dark and the light theme.
pub const GRAPHITE_ACCENTS: [[u8; 3]; 6] = [
    [0xe3, 0xbb, 0x3b],
    [0xa9, 0x8b, 0xf2],
    [0xe4, 0x6a, 0xb4],
    [0xb6, 0xd6, 0x3c],
    [0x2f, 0xc3, 0xd3],
    [0x4f, 0xd1, 0xa1],
];

const CATPPUCCIN_ACCENT_NAMES: [&str; 6] = ["Yellow", "Peach", "Mauve", "Green", "Teal", "Blue"];
const GRAPHITE_ACCENT_NAMES: [&str; 6] = ["Gold", "Orchid", "Fuchsia", "Citron", "Lagoon", "Mint"];

/// Smallest RGB distance between the warning colour and the accent or the danger colour. Peach,
/// Yellow and Red are close neighbours in every flavour, so this is a small step.
const WARN_APART: f32 = 30.0;

/// Which palette family to use.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ThemeFamily {
    /// The official Catppuccin palette: Latte for light, a dark flavour for dark.
    #[default]
    Catppuccin,
    /// The cuboid graphite palette with a faint green undertone.
    Graphite,
}

impl ThemeFamily {
    /// Both families.
    pub const ALL: [ThemeFamily; 2] = [ThemeFamily::Catppuccin, ThemeFamily::Graphite];

    /// Label in the preferences.
    pub fn name(self) -> &'static str {
        match self {
            ThemeFamily::Catppuccin => "Catppuccin",
            ThemeFamily::Graphite => "Graphite",
        }
    }

    /// The accent a new choice of this family starts with.
    pub fn default_accent(self) -> [u8; 3] {
        match self {
            ThemeFamily::Catppuccin => catppuccin_accents(&PALETTE.mocha)[0],
            ThemeFamily::Graphite => GRAPHITE_ACCENTS[0],
        }
    }

    /// Names of the six accent presets, in the order of [`Theme::accent_presets`].
    pub fn accent_names(self) -> [&'static str; 6] {
        match self {
            ThemeFamily::Catppuccin => CATPPUCCIN_ACCENT_NAMES,
            ThemeFamily::Graphite => GRAPHITE_ACCENT_NAMES,
        }
    }
}

/// The dark Catppuccin flavours; the light theme always uses Latte.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum DarkFlavor {
    /// The lightest dark flavour.
    Frappe,
    /// The medium dark flavour.
    Macchiato,
    /// The darkest dark flavour.
    #[default]
    Mocha,
}

impl DarkFlavor {
    /// The three dark flavours.
    pub const ALL: [DarkFlavor; 3] = [DarkFlavor::Frappe, DarkFlavor::Macchiato, DarkFlavor::Mocha];

    /// Label in the preferences.
    pub fn name(self) -> &'static str {
        match self {
            DarkFlavor::Frappe => "Frappé",
            DarkFlavor::Macchiato => "Macchiato",
            DarkFlavor::Mocha => "Mocha",
        }
    }

    fn palette(self) -> &'static CatFlavor {
        match self {
            DarkFlavor::Frappe => &PALETTE.frappe,
            DarkFlavor::Macchiato => &PALETTE.macchiato,
            DarkFlavor::Mocha => &PALETTE.mocha,
        }
    }
}

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
    /// Palette family.
    pub family: ThemeFamily,
    /// Dark flavour of the Catppuccin family; Graphite ignores it.
    pub flavor: DarkFlavor,
    /// Dark, light or system.
    pub mode: ThemeMode,
    /// Accent colour as RGB: one of the presets of the family in any flavour, which the active
    /// flavour maps to its own shade; anything else counts as the first preset.
    pub accent: [u8; 3],
    /// Pure black or white surfaces and stronger borders.
    pub high_contrast: bool,
    /// UI scale, clamped to [`UI_SCALE_MIN`] .. [`UI_SCALE_MAX`].
    pub scale: f32,
}

impl Default for Theme {
    fn default() -> Self {
        let family = ThemeFamily::default();
        Self {
            family,
            flavor: DarkFlavor::default(),
            mode: ThemeMode::Dark,
            accent: family.default_accent(),
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
    /// Wells that hold a picture or a viewport-like area.
    pub bg_input: Color32,
    /// Fill of fields and secondary buttons: at least 1.3:1 from the panel and from the raised
    /// surface, so it shows on both without an outline.
    pub bg_control: Color32,
    /// A filled control while it is hovered: a step beyond [`Tokens::bg_control`].
    pub control_hover: Color32,
    /// A filled control while it is pressed or selected-neutral (checked box, chosen segment).
    pub control_active: Color32,
    /// Hovered row, tab or ghost button: a quiet step from the panel.
    pub bg_hover: Color32,
    /// Pressed or selected-neutral row, tab or ghost button.
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
    /// Warnings; an orange that stays apart from the gold accent.
    pub warn: Color32,
    /// Success.
    pub ok: Color32,
    /// Axis colours X, Y and Z, fitted to the surfaces; for axis labels and the gizmo only.
    pub axis: [Color32; 3],
    /// Dark theme.
    pub dark: bool,
}

/// The surface levels of the specification (section 4), back to front.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Surface {
    /// Behind the viewport and between panels.
    App,
    /// Panels.
    Region,
    /// Cards, list areas, menus.
    Content,
    /// Fill of fields and secondary buttons.
    Control,
    /// Hovered row, tab or ghost button.
    Hover,
    /// Pressed row, tab or ghost button, and the neutral selected state.
    Pressed,
}

impl Surface {
    /// Every level.
    pub const ALL: [Surface; 6] = [
        Surface::App,
        Surface::Region,
        Surface::Content,
        Surface::Control,
        Surface::Hover,
        Surface::Pressed,
    ];
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

    /// Replaces the tokens that widgets read from this context, e.g. with a neutral highlight
    /// in place of the accent. The next [`Theme::apply`] of a changed theme installs its own
    /// tokens again; [`Theme::forget_applied`] makes it do so for an unchanged theme too.
    pub fn store(self, ctx: &Context) {
        ctx.data_mut(|d| d.insert_temp(Id::unique(TOKENS_ID), self));
    }

    /// The colour of a surface level.
    pub fn surface(&self, level: Surface) -> Color32 {
        match level {
            Surface::App => self.bg_app,
            Surface::Region => self.bg_panel,
            Surface::Content => self.bg_raised,
            Surface::Control => self.bg_control,
            Surface::Hover => self.bg_hover,
            Surface::Pressed => self.bg_active,
        }
    }

    /// The tokens with the accent replaced by the strong text colour, for a fresh install that
    /// has not picked an accent: selected and focused states stay neutral. `on_accent` is the
    /// panel colour, so a label on the highlight reads at 4.5:1.
    #[must_use]
    pub fn neutral_highlight(self) -> Tokens {
        Tokens {
            accent: self.text_strong,
            accent_hover: self.text,
            accent_soft: self.bg_active,
            on_accent: self.bg_panel,
            ..self
        }
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

/// The colours a palette decides; the accent and the status colours are fitted on top of them.
struct Base {
    app: Color32,
    input: Color32,
    panel: Color32,
    raised: Color32,
    hover: Color32,
    active: Color32,
    viewport: Color32,
    /// Where the field fill moves to: further from the panel, in the hue of the palette.
    away: Color32,
    border: Color32,
    border_strong: Color32,
    border_hover: Color32,
    focus: Color32,
    text: Color32,
    text_weak: Color32,
    text_strong: Color32,
    /// Text on an accent fill.
    on_accent: Color32,
    /// How much of the accent the selection fill takes from the panel.
    selection_mix: f32,
    danger: Color32,
    /// The warning colour, and the one to use when the first is too close to the accent.
    warn: [Color32; 2],
    ok: Color32,
    /// X, Y and Z before fitting.
    axis: [Color32; 3],
}

fn graphite_base(dark: bool, hc: bool) -> Base {
    // Surfaces: app, input, panel, raised, hover, active, viewport.
    let [app, input, panel, raised, hover, active, viewport] = match (dark, hc) {
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
    let [danger, warn, ok] = if dark {
        [0xf0766b, 0xf47a24, 0x5fcb82]
    } else {
        [0xb3261e, 0xc24e00, 0x16743f]
    }
    .map(rgb);
    let axis = if dark {
        [0xf0766b, 0x6fd68f, 0x6aa3f5]
    } else {
        [0xb3261e, 0x1d7a45, 0x1a5fc4]
    }
    .map(rgb);
    Base {
        app,
        input,
        panel,
        raised,
        hover,
        active,
        viewport,
        away: border_strong,
        border,
        border_strong,
        border_hover,
        focus,
        text,
        text_weak,
        text_strong,
        on_accent: if dark { rgb(0x0b0e0c) } else { Color32::WHITE },
        selection_mix: if hc { 0.2 } else { 0.22 },
        danger,
        warn: [warn, warn],
        ok,
        axis,
    }
}

fn cat(color: &CatColor) -> Color32 {
    Color32::from_rgb(color.rgb.r, color.rgb.g, color.rgb.b)
}

/// The accent presets of a flavour: Yellow, Peach, Mauve, Green, Teal, Blue.
fn catppuccin_accents(f: &CatFlavor) -> [[u8; 3]; 6] {
    let c = &f.colors;
    [&c.yellow, &c.peach, &c.mauve, &c.green, &c.teal, &c.blue].map(|c| [c.rgb.r, c.rgb.g, c.rgb.b])
}

/// Maps the flavour's colours to the tokens. The palette's backgrounds sit close together (Crust,
/// Mantle and Base differ by less than 1.2:1), so panels are also told apart by their hairlines.
/// High contrast pushes the surfaces toward black (white) and the text toward white (black).
fn catppuccin_base(f: &CatFlavor, dark: bool, hc: bool) -> Base {
    let c = &f.colors;
    let (far, near) = if dark {
        (Color32::BLACK, Color32::WHITE)
    } else {
        (Color32::WHITE, Color32::BLACK)
    };
    let push = |color: &CatColor, t: f32| mix(cat(color), far, t);
    let pull = |color: &CatColor, t: f32| mix(cat(color), near, t);
    let [app, input, panel, raised, hover, active, viewport] = match (dark, hc) {
        (true, false) => [
            &c.crust,
            &c.crust,
            &c.mantle,
            &c.base,
            &c.surface0,
            &c.surface1,
            &c.base,
        ]
        .map(cat),
        (true, true) => [
            far,
            far,
            push(&c.crust, 0.6),
            push(&c.mantle, 0.4),
            push(&c.surface0, 0.3),
            push(&c.surface1, 0.35),
            push(&c.crust, 0.8),
        ],
        // Latte's hover fill is its darkest background, so text keeps 4.5:1 on a pressed control.
        (false, false) => [
            &c.crust,
            &c.base,
            &c.mantle,
            &c.base,
            &c.crust,
            &c.surface0,
            &c.crust,
        ]
        .map(cat),
        (false, true) => [
            push(&c.crust, 0.5),
            far,
            far,
            far,
            cat(&c.crust),
            cat(&c.surface0),
            far,
        ],
    };
    let [border, border_strong, border_hover, focus] = if hc {
        [pull(&c.overlay0, 0.3), pull(&c.subtext1, 0.5), near, near]
    } else {
        [
            cat(&c.surface0),
            cat(if dark { &c.overlay1 } else { &c.overlay2 }),
            cat(&c.subtext0),
            near,
        ]
    };
    let [text, text_weak, text_strong] = if hc {
        [pull(&c.text, 0.6), pull(&c.text, 0.4), near]
    } else {
        [cat(&c.text), cat(&c.subtext1), cat(&c.text)]
    };
    let on_accent = match (dark, hc) {
        (true, false) => cat(&c.crust),
        (false, false) => cat(&c.base),
        (_, true) => far,
    };
    Base {
        app,
        input,
        panel,
        raised,
        hover,
        active,
        viewport,
        away: if hc { border_strong } else { cat(&c.overlay0) },
        border,
        border_strong,
        border_hover,
        focus,
        text,
        text_weak,
        text_strong,
        on_accent,
        // The pastel dark flavours need more accent for the tint to stand out from hover.
        selection_mix: match (dark, hc) {
            (_, true) => 0.2,
            (true, false) => 0.28,
            (false, false) => 0.22,
        },
        danger: cat(&c.red),
        warn: [cat(&c.peach), cat(&c.yellow)],
        ok: cat(&c.green),
        axis: [cat(&c.red), cat(&c.green), cat(&c.blue)],
    }
}

fn distance(a: Color32, b: Color32) -> f32 {
    let d = |x: u8, y: u8| (f32::from(x) - f32::from(y)).powi(2);
    (d(a.r(), b.r()) + d(a.g(), b.g()) + d(a.b(), b.b())).sqrt()
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

/// Field fill against the panel and the raised surface (specification 4, 1.3:1, with a margin
/// for the rounding to bytes).
const FIELD_RATIO: f32 = 1.32;
/// Contrast between a filled control and the same control hovered or pressed.
const CONTROL_STEP: f32 = 1.2;

/// Moves `from` toward `toward` until it stands `ratio` apart from `from`, but never to a colour
/// the text can no longer be read on (`text_min`); the last colour that was fine.
fn step_from(from: Color32, toward: Color32, ratio: f32, text: Color32, text_min: f32) -> Color32 {
    let mut last = from;
    for step in 1..=40_u8 {
        let c = mix(from, toward, f32::from(step) / 40.0);
        if contrast_ratio(text, c) < text_min {
            break;
        }
        last = c;
        if contrast_ratio(c, from) >= ratio {
            break;
        }
    }
    last
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

    /// The Catppuccin flavour in use.
    fn flavor_palette(&self, dark: bool) -> &'static CatFlavor {
        if dark {
            self.flavor.palette()
        } else {
            &PALETTE.latte
        }
    }

    /// The six accent presets of the family as shown in the dark or light theme.
    pub fn accent_presets(&self, dark: bool) -> [[u8; 3]; 6] {
        match self.family {
            ThemeFamily::Catppuccin => catppuccin_accents(self.flavor_palette(dark)),
            ThemeFamily::Graphite => GRAPHITE_ACCENTS,
        }
    }

    /// Which preset the accent is, counted in any flavour of the family; `None` for a colour that
    /// is none of them.
    pub fn accent_slot(&self) -> Option<usize> {
        match self.family {
            ThemeFamily::Catppuccin => PALETTE
                .all_flavors()
                .into_iter()
                .find_map(|f| catppuccin_accents(f).iter().position(|a| *a == self.accent)),
            ThemeFamily::Graphite => GRAPHITE_ACCENTS.iter().position(|a| *a == self.accent),
        }
    }

    /// The colours for the dark or light palette.
    pub fn tokens(&self, dark: bool) -> Tokens {
        let hc = self.high_contrast;
        let b = match self.family {
            ThemeFamily::Catppuccin => catppuccin_base(self.flavor_palette(dark), dark, hc),
            ThemeFamily::Graphite => graphite_base(dark, hc),
        };
        let (bg_panel, bg_raised, bg_input) = (b.panel, b.raised, b.input);
        // Text needs 4.5:1 (7:1 in high contrast); a fill or an icon 3:1 (4.5:1).
        let (text_ratio, ui_ratio) = if hc { (7.0, 4.5) } else { (4.5, 3.0) };
        // A field has no outline, so its fill stands 1.3:1 from the panel and from the raised
        // surface (menus, dialogs) it can sit on. Its hovered and pressed fills are steps beyond
        // it that keep the text readable: lighter on hover and darker on press in a light theme,
        // where the field is already the darkest level the text can sit on.
        let bg_control = fit_contrast(
            b.hover,
            b.away,
            &[(bg_panel, FIELD_RATIO), (bg_raised, FIELD_RATIO)],
        );
        let step = |from, toward| step_from(from, toward, CONTROL_STEP, b.text, text_ratio + 0.05);
        let (control_hover, control_active) = if dark {
            let hover = step(bg_control, b.away);
            (hover, step(hover, b.away))
        } else {
            (step(bg_control, bg_raised), step(bg_control, b.away))
        };
        let surfaces = [bg_panel, bg_raised, bg_input, bg_control];
        let pressed = [b.hover, b.active];
        let toward = if dark { Color32::WHITE } else { Color32::BLACK };
        let preset = self.accent_presets(dark)[self.accent_slot().unwrap_or(0)];
        let base = Color32::from_rgb(preset[0], preset[1], preset[2]);
        // Dark themes lighten the accent until the dark ink reads on it; light themes darken it
        // until the light ink does.
        let mut needs = [bg_panel, bg_raised, bg_input]
            .map(|s| (s, if dark { text_ratio } else { ui_ratio }))
            .to_vec();
        // The light margin keeps the accent above 3:1 on the accent-tinted selection fill too.
        let pressed_ratio = if dark { ui_ratio } else { ui_ratio + 0.3 };
        needs.extend(pressed.map(|s| (s, pressed_ratio)));
        needs.push((b.on_accent, text_ratio + 0.2));
        let accent = fit_contrast(base, toward, &needs);
        let accent_hover = mix(accent, toward, 0.16);
        let status = |base: Color32| {
            let needs = surfaces.map(|s| (s, text_ratio));
            fit_contrast(base, toward, &needs)
        };
        let [warn, warn_alt] = b.warn.map(status);
        // The second choice when the first is no better than the accent.
        let warn = if distance(warn, accent) >= WARN_APART
            || distance(warn, accent) >= distance(warn_alt, accent)
        {
            warn
        } else {
            warn_alt
        };
        let axis = b.axis.map(status);
        Tokens {
            bg_app: b.app,
            bg_panel,
            bg_raised,
            bg_input,
            bg_control,
            control_hover,
            control_active,
            bg_hover: b.hover,
            bg_active: b.active,
            bg_viewport: b.viewport,
            border: b.border,
            border_strong: b.border_strong,
            border_hover: b.border_hover,
            focus: b.focus,
            text: b.text,
            text_weak: b.text_weak,
            text_strong: b.text_strong,
            accent,
            accent_hover,
            accent_soft: mix(bg_panel, accent, b.selection_mix),
            on_accent: b.on_accent,
            danger: status(b.danger),
            warn,
            ok: status(b.ok),
            axis,
            dark,
        }
    }

    /// Makes the next [`Theme::apply`] install its tokens and style again, whatever the theme.
    pub fn forget_applied(ctx: &Context) {
        ctx.data_mut(|d| d.remove::<(Theme, bool)>(Id::unique(APPLIED_ID)));
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
    // Controls have no outline; high contrast brings it back.
    let outline = if hc {
        stroke(line, t.border_strong)
    } else {
        Stroke::NONE
    };

    style.spacing.item_spacing = vec2(space::SM, space::XS);
    style.spacing.button_padding = vec2(space::SM, space::XS);
    style.spacing.interact_size = vec2(7.0 * GRID, 6.0 * GRID);
    style.spacing.menu_margin = Margin::same(4);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.indent = space::MD;
    style.spacing.icon_width = space::LG;
    style.spacing.icon_width_inner = space::SM;
    style.spacing.slider_width = 30.0 * GRID;
    style.spacing.combo_width = 30.0 * GRID;
    style.spacing.text_edit_width = 40.0 * GRID;
    style.interaction.tooltip_delay = 0.4;
    style.interaction.selectable_labels = false;

    style.text_styles = [
        (TextStyle::Small, Type::Caption.font()),
        (TextStyle::Body, Type::Body.font()),
        (TextStyle::Button, Type::Body.font()),
        (TextStyle::Heading, Type::Heading.font()),
        (TextStyle::Monospace, Type::Body.font()),
    ]
    .into();

    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.bg_panel;
    v.window_fill = t.bg_raised;
    v.extreme_bg_color = t.bg_control;
    v.text_edit_bg_color = Some(t.bg_control);
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
    v.weak_text_color = Some(t.text_weak);
    // Text selection and slider fills use the soft tint; the focus ring is the neutral token.
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = stroke(if hc { line } else { FOCUS_RING }, t.focus);
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

    w.inactive.bg_fill = t.bg_control;
    w.inactive.weak_bg_fill = t.bg_control;
    w.inactive.bg_stroke = outline;
    w.inactive.fg_stroke = stroke(1.0, t.text);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = t.control_hover;
    w.hovered.weak_bg_fill = t.control_hover;
    w.hovered.bg_stroke = if hc {
        stroke(line, t.border_hover)
    } else {
        Stroke::NONE
    };
    w.hovered.fg_stroke = stroke(1.0, t.text_strong);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = t.control_active;
    w.active.weak_bg_fill = t.control_active;
    w.active.bg_stroke = if hc {
        stroke(line, t.border_hover)
    } else {
        Stroke::NONE
    };
    w.active.fg_stroke = stroke(1.0, t.text_strong);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = t.control_active;
    w.open.weak_bg_fill = t.control_active;
    w.open.bg_stroke = outline;
    w.open.fg_stroke = stroke(1.0, t.text_strong);
    w.open.corner_radius = radius;

    style.visuals = v;
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One palette under test.
    struct Case {
        name: String,
        tokens: Tokens,
        hc: bool,
        family: ThemeFamily,
    }

    /// Every family, flavour, accent preset and contrast option.
    fn palettes() -> Vec<Case> {
        let mut all = Vec::new();
        for family in ThemeFamily::ALL {
            let flavors: &[DarkFlavor] = match family {
                ThemeFamily::Catppuccin => &DarkFlavor::ALL,
                ThemeFamily::Graphite => &[DarkFlavor::Mocha],
            };
            for &flavor in flavors {
                for dark in [true, false] {
                    for hc in [false, true] {
                        let mut theme = Theme {
                            family,
                            flavor,
                            high_contrast: hc,
                            ..Theme::default()
                        };
                        // Latte is the light flavour, so the dark flavours repeat once.
                        if !dark && flavor != DarkFlavor::Mocha {
                            continue;
                        }
                        for (slot, accent) in theme.accent_presets(dark).into_iter().enumerate() {
                            theme.accent = accent;
                            let name = format!(
                                "{} {} {} {}",
                                family.name(),
                                if dark { flavor.name() } else { "Latte" },
                                if hc { "hc" } else { "normal" },
                                family.accent_names()[slot],
                            );
                            all.push(Case {
                                name,
                                tokens: theme.tokens(dark),
                                hc,
                                family,
                            });
                        }
                    }
                }
            }
        }
        all
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
            accent: catppuccin_accents(&PALETTE.latte)[4],
            scale: 1.5,
            ..Theme::default()
        };
        theme.apply(&ctx);
        let tokens = Tokens::of(&ctx);
        assert!(!tokens.dark);
        assert_eq!(tokens.bg_panel, rgb(0xe6e9ef));
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
        for Case {
            name,
            tokens: t,
            hc,
            family,
        } in palettes()
        {
            let min = if hc { 7.0 } else { 4.5 };
            let surfaces = [
                ("app", t.bg_app),
                ("panel", t.bg_panel),
                ("raised", t.bg_raised),
                ("input", t.bg_input),
                ("control", t.bg_control),
                ("control_hover", t.control_hover),
                ("control_active", t.control_active),
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
                    // Secondary text only keeps 3:1 on the pressed and selected fills of the
                    // Catppuccin flavours and on the hovered and pressed control fills (their
                    // labels switch to the strong colour), and on the field fill in Latte, where
                    // the 1.3:1 field sits far down the lightness the text can be read on.
                    let weak_floor = what == "text_weak"
                        && match surface {
                            "active" | "selection" => !hc && family == ThemeFamily::Catppuccin,
                            "control_hover" | "control_active" => true,
                            "control" => !hc && family == ThemeFamily::Catppuccin && !t.dark,
                            _ => false,
                        };
                    let min = match (weak_floor, hc) {
                        (true, true) => 4.5,
                        (true, false) => 3.0,
                        (false, _) => min,
                    };
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
    fn the_close_button_glyph_reads_on_the_danger_fill() {
        for Case {
            name,
            tokens: t,
            hc,
            ..
        } in palettes()
        {
            let min = if hc { 7.0 } else { 4.5 };
            let ratio = contrast_ratio(t.on_accent, t.danger);
            assert!(ratio >= min, "{name}: on_accent on danger is {ratio:.2}");
        }
    }

    #[test]
    fn status_colours_and_accent_read_as_text_on_panels() {
        for Case {
            name,
            tokens: t,
            hc,
            ..
        } in palettes()
        {
            let min = if hc { 7.0 } else { 4.5 };
            for (what, fg) in [("danger", t.danger), ("warn", t.warn), ("ok", t.ok)] {
                for bg in [t.bg_panel, t.bg_raised, t.bg_input, t.bg_control] {
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
        for Case {
            name,
            tokens: t,
            hc,
            ..
        } in palettes()
        {
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
        for Case {
            name,
            tokens: t,
            hc,
            family,
        } in palettes()
        {
            // The Catppuccin backgrounds are only about 1.07:1 apart, by design.
            let min = match family {
                ThemeFamily::Catppuccin => 1.05,
                ThemeFamily::Graphite => 1.1,
            };
            let steps = [
                ("hover vs panel", t.bg_hover, t.bg_panel),
                ("active vs hover", t.bg_active, t.bg_hover),
            ];
            for (what, a, b) in steps {
                let ratio = contrast_ratio(a, b);
                assert!(ratio >= min, "{name}: {what} is {ratio:.2}");
            }
            // High contrast separates panels with outlines instead of fills.
            if !hc {
                for (what, a, b) in [
                    ("panel vs app", t.bg_panel, t.bg_app),
                    ("raised vs panel", t.bg_raised, t.bg_panel),
                ] {
                    let ratio = contrast_ratio(a, b);
                    assert!(ratio >= min, "{name}: {what} is {ratio:.2}");
                }
            }
        }
    }

    /// The contrast rules of specification 4 on the token pairs, in every palette.
    #[test]
    fn fields_state_parts_and_text_meet_the_specified_contrast() {
        for Case {
            name,
            tokens: t,
            hc,
            ..
        } in palettes()
        {
            let ratio = contrast_ratio;
            for (what, surface) in [("region", t.bg_panel), ("content", t.bg_raised)] {
                // A field has no outline: its fill carries it.
                assert!(
                    ratio(t.bg_control, surface) >= 1.3,
                    "{name}: field fill on {what} is {:.2}",
                    ratio(t.bg_control, surface)
                );
                // State parts (box, track, slider track) are drawn with this 1 pt edge.
                assert!(
                    ratio(t.border_strong, surface) >= 3.0,
                    "{name}: state edge on {what} is {:.2}",
                    ratio(t.border_strong, surface)
                );
            }
            // The tick, the knob and the slider fill sit on the control fills at 3:1.
            for (what, ink, fill) in [
                ("tick", t.text_strong, t.control_active),
                ("tick on hover", t.text_strong, t.control_hover),
                ("knob on", t.text_strong, t.control_active),
                ("knob off", t.text_weak, t.bg_control),
                ("slider fill", t.text_weak, t.bg_control),
            ] {
                assert!(
                    ratio(ink, fill) >= 3.0,
                    "{name}: {what} is {:.2}",
                    ratio(ink, fill)
                );
            }
            // The steps from the field fill can be told apart.
            for (what, a) in [("hover", t.control_hover), ("pressed", t.control_active)] {
                assert!(
                    ratio(a, t.bg_control) >= 1.05,
                    "{name}: {what} fill is no step from the field: {:.2}",
                    ratio(a, t.bg_control)
                );
            }
            // Body text, on every surface it can sit on.
            let min = if hc { 7.0 } else { 4.5 };
            for (what, surface) in [
                ("region", t.bg_panel),
                ("content", t.bg_raised),
                ("control", t.bg_control),
                ("control hover", t.control_hover),
                ("control pressed", t.control_active),
            ] {
                for (ink_name, ink) in [("text", t.text), ("text_strong", t.text_strong)] {
                    assert!(
                        ratio(ink, surface) >= min,
                        "{name}: {ink_name} on {what} is {:.2}",
                        ratio(ink, surface)
                    );
                }
            }
        }
    }

    #[test]
    fn selection_hover_focus_and_accent_look_different() {
        for Case {
            name,
            tokens: t,
            hc,
            ..
        } in palettes()
        {
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
            // High contrast pushes every status colour toward white, so they sit closer.
            let apart = if hc { WARN_APART - 5.0 } else { WARN_APART };
            assert!(
                distance(t.warn, t.accent) >= apart,
                "{name}: warning and accent"
            );
            assert!(
                distance(t.warn, t.danger) >= apart,
                "{name}: warning and danger"
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
        let t = Theme {
            family: ThemeFamily::Graphite,
            accent: GRAPHITE_ACCENTS[0],
            ..Theme::default()
        }
        .tokens(true);
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
        assert!(!GRAPHITE_ACCENTS.contains(&[0x4f, 0x8c, 0xff]));
        assert_eq!(ThemeFamily::Graphite.default_accent(), [0xe3, 0xbb, 0x3b]);
    }

    #[test]
    fn the_default_is_mocha_with_the_yellow_accent() {
        let theme = Theme::default();
        assert_eq!(theme.family, ThemeFamily::Catppuccin);
        assert_eq!(theme.flavor, DarkFlavor::Mocha);
        assert_eq!(theme.accent, [0xf9, 0xe2, 0xaf]);
        let t = theme.tokens(true);
        assert_eq!(t.bg_panel, rgb(0x181825));
        assert_eq!(t.bg_raised, rgb(0x1e1e2e));
        assert_eq!(t.text, rgb(0xcdd6f4));
        assert_eq!(t.accent, rgb(0xf9e2af));
        assert_eq!(t.danger, rgb(0xf38ba8));
        assert_eq!(t.ok, rgb(0xa6e3a1));
        assert_eq!(t.warn, rgb(0xfab387));
    }

    #[test]
    fn the_light_theme_is_latte_and_keeps_the_yellow() {
        let theme = Theme::default();
        let t = theme.tokens(false);
        assert_eq!(t.bg_panel, rgb(0xe6e9ef));
        assert_eq!(t.text, rgb(0x4c4f69));
        assert_eq!(theme.accent_presets(false)[0], [0xdf, 0x8e, 0x1d]);
        // The accent is the same preset in every flavour, so a saved Mocha yellow is Latte's yellow.
        assert_eq!(theme.accent_slot(), Some(0));
        // Darkened to read on the light surfaces, but still the amber hue.
        assert!(t.accent.r() > t.accent.g() && t.accent.g() > t.accent.b());
    }

    #[test]
    fn the_flavours_use_the_official_values() {
        for (flavor, base, text, blue) in [
            (DarkFlavor::Frappe, 0x303446, 0xc6d0f5, [0x8c, 0xaa, 0xee]),
            (
                DarkFlavor::Macchiato,
                0x24273a,
                0xcad3f5,
                [0x8a, 0xad, 0xf4],
            ),
            (DarkFlavor::Mocha, 0x1e1e2e, 0xcdd6f4, [0x89, 0xb4, 0xfa]),
        ] {
            let theme = Theme {
                flavor,
                ..Theme::default()
            };
            let t = theme.tokens(true);
            assert_eq!(t.bg_raised, rgb(base), "{}", flavor.name());
            assert_eq!(t.text, rgb(text), "{}", flavor.name());
            assert_eq!(theme.accent_presets(true)[5], blue, "{}", flavor.name());
        }
        assert_eq!(Theme::default().tokens(false).bg_raised, rgb(0xeff1f5));
    }

    #[test]
    fn accent_presets_are_the_catppuccin_accents_with_yellow_first() {
        let theme = Theme::default();
        assert_eq!(
            ThemeFamily::Catppuccin.accent_names(),
            ["Yellow", "Peach", "Mauve", "Green", "Teal", "Blue"]
        );
        for dark in [true, false] {
            let flavor = theme.flavor_palette(dark);
            let c = &flavor.colors;
            let want = [&c.yellow, &c.peach, &c.mauve, &c.green, &c.teal, &c.blue]
                .map(|c| [c.rgb.r, c.rgb.g, c.rgb.b]);
            assert_eq!(theme.accent_presets(dark), want);
        }
        let graphite = Theme {
            family: ThemeFamily::Graphite,
            accent: GRAPHITE_ACCENTS[2],
            ..Theme::default()
        };
        assert_eq!(graphite.accent_slot(), Some(2));
        // A Catppuccin colour is no Graphite preset, and the other way round.
        assert_eq!(Theme::default().accent_slot(), Some(0));
        assert_eq!(
            Theme {
                accent: GRAPHITE_ACCENTS[0],
                ..Theme::default()
            }
            .accent_slot(),
            None
        );
    }
}
