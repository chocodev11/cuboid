//! The preferences dialog.

use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    Button, DarkFlavor, DragFieldOpts, Icon, MenuItem, Modal, ModalButton, PropertyGrid,
    SliderOpts, ThemeFamily, Tokens, Type, UI_SCALE_MAX, UI_SCALE_MIN, color_swatch, drag_field,
    dropdown, icon_button, menu_trigger, rgb_color, search_field, segmented, slider, tabs, text,
    toggle,
};
use egui::{
    Align, Context, Key, Label, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::dialogs::fixed_body;
use crate::keymap::{Chord, Command, Keymap, KeymapPreset, diff_from_preset, pressed_chords};
use crate::labels::{command_label, entries, search};
use crate::layout::Layout as PanelLayout;
use crate::prefs::{ColorMode, ComingFrom, NavPreset, Prefs};
use crate::request::Mode;

/// Height of the body under the tabs, the same for every tab.
const BODY: f32 = height::ROW * 12.0;
/// Width of the keymap preset drop-down.
const PRESET_WIDTH: f32 = 200.0;
/// Width of a key button in the keymap list.
const KEY_WIDTH: f32 = 136.0;
/// Width of the number fields.
const FIELD_WIDTH: f32 = 88.0;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
enum Tab {
    #[default]
    Interface,
    Navigation,
    Keymap,
    Editing,
    General,
}

impl Tab {
    const ALL: [Tab; 5] = [
        Tab::Interface,
        Tab::Navigation,
        Tab::Keymap,
        Tab::Editing,
        Tab::General,
    ];

    fn name(self) -> &'static str {
        match self {
            Tab::Interface => "Interface",
            Tab::Navigation => "Navigation",
            Tab::Keymap => "Keymap",
            Tab::Editing => "Editing",
            Tab::General => "General",
        }
    }
}

/// State of the window.
#[derive(Default)]
pub(crate) struct PrefsWindow {
    open: bool,
    tab: Tab,
    /// UI scale being dragged; applied on release because rescaling under the pointer would make
    /// the slider run away.
    pending_scale: Option<f32>,
    filter: String,
    capture: Option<Command>,
    notice: String,
}

/// What the window changed this frame.
#[derive(Default)]
pub(crate) struct PrefsOutcome {
    /// Any preference changed.
    pub changed: bool,
    /// The key preset or the overrides changed, so the keymap must be rebuilt.
    pub keymap: bool,
}

impl PrefsWindow {
    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn open_keymap(&mut self) {
        self.open = true;
        self.tab = Tab::Keymap;
    }

    /// A key is being recorded for a binding: the global shortcuts must stay quiet.
    pub fn capturing(&self) -> bool {
        self.open && self.capture.is_some()
    }

    /// Draws the dialog. The changes apply at once; Done, Enter and Esc close it, except that
    /// Esc and Enter belong to the key that is being recorded.
    pub fn show(&mut self, ctx: &Context, prefs: &mut Prefs, keymap: &mut Keymap) -> PrefsOutcome {
        let mut outcome = PrefsOutcome::default();
        if !self.open {
            return outcome;
        }
        let recording = self.capture.is_some();
        if recording {
            self.record_chord(ctx, prefs, keymap, &mut outcome);
        }
        let buttons = [ModalButton::primary("Done")];
        let names = Tab::ALL.map(Tab::name);
        let result = Modal::new("preferences", "Preferences")
            .width(720.0)
            .buttons(&buttons)
            .show(ctx, |ui| {
                let mut at = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
                if tabs(ui, &mut at, &names) {
                    self.tab = Tab::ALL[at.min(Tab::ALL.len() - 1)];
                }
                ui.add_space(space::SM);
                if self.tab == Tab::Keymap {
                    ui.allocate_ui(vec2(ui.available_width(), BODY), |ui| {
                        ui.set_min_height(BODY);
                        self.keymap_tab(ui, prefs, keymap, &mut outcome);
                    });
                    return;
                }
                fixed_body(ui, "prefs_body", BODY, |ui| match self.tab {
                    Tab::Interface => self.interface(ui, prefs, &mut outcome),
                    Tab::Navigation => navigation(ui, prefs, &mut outcome),
                    Tab::Editing => editing(ui, prefs, &mut outcome),
                    Tab::General => general(ui, prefs, keymap, &mut outcome),
                    Tab::Keymap => {}
                });
            });
        let leave = result.button.is_some() || !recording && (result.cancelled || result.confirmed);
        if leave {
            self.open = false;
            self.capture = None;
        }
        outcome
    }

    fn interface(&mut self, ui: &mut Ui, prefs: &mut Prefs, out: &mut PrefsOutcome) {
        let tokens = Tokens::of(ui.ctx());
        PropertyGrid::new("prefs_interface").show(ui, |grid| {
            grid.row("Theme", |ui| {
                let mut mode = match prefs.color_mode {
                    ColorMode::Dark => 0,
                    ColorMode::Light => 1,
                    ColorMode::System => 2,
                };
                if segmented(ui, &mut mode, &["Dark", "Light", "System"]) {
                    prefs.color_mode =
                        [ColorMode::Dark, ColorMode::Light, ColorMode::System][mode.min(2)];
                    out.changed = true;
                }
            });
            grid.row("Colour scheme", |ui| {
                let mut family = ThemeFamily::ALL
                    .iter()
                    .position(|f| *f == prefs.family)
                    .unwrap_or(0);
                let names = ThemeFamily::ALL.map(ThemeFamily::name);
                if segmented(ui, &mut family, &names) {
                    prefs.family = ThemeFamily::ALL[family.min(1)];
                    prefs.accent = prefs.family.default_accent();
                    out.changed = true;
                }
            });
            if prefs.family == ThemeFamily::Catppuccin {
                grid.row("Dark flavour", |ui| {
                    let mut flavor = DarkFlavor::ALL
                        .iter()
                        .position(|f| *f == prefs.flavor)
                        .unwrap_or(0);
                    let names = DarkFlavor::ALL.map(DarkFlavor::name);
                    if segmented(ui, &mut flavor, &names) {
                        prefs.flavor = DarkFlavor::ALL[flavor.min(2)];
                        out.changed = true;
                    }
                });
            }
            grid.row("Accent", |ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let theme = prefs.theme();
                let slot = theme.accent_slot();
                let presets = theme.accent_presets(tokens.dark);
                for (i, (preset, name)) in presets
                    .into_iter()
                    .zip(prefs.family.accent_names())
                    .enumerate()
                {
                    let swatch = color_swatch(
                        ui,
                        rgb_color(preset),
                        Vec2::splat(height::CONTROL),
                        slot == Some(i),
                    );
                    if swatch.on_hover_text(name).clicked() {
                        prefs.accent = preset;
                        prefs.accent_chosen = true;
                        out.changed = true;
                    }
                }
            });
            grid.row("Contrast", |ui| {
                if toggle(ui, &mut prefs.high_contrast, "High contrast").changed() {
                    out.changed = true;
                }
            });
            grid.row("UI scale", |ui| self.scale_slider(ui, prefs, out));
            grid.row("Viewport", |ui| {
                if toggle(ui, &mut prefs.antialiasing, "Anti-aliasing (4x MSAA)").changed() {
                    out.changed = true;
                }
            });
            // macOS keeps the system title bar, so there is nothing to choose.
            if !cfg!(target_os = "macos") {
                grid.row("Window", |ui| {
                    if toggle(ui, &mut prefs.system_title_bar, "Use the system title bar").changed()
                    {
                        out.changed = true;
                    }
                });
            }
        });
        if prefs.family == ThemeFamily::Catppuccin {
            ui.add(Label::new(text(Type::Caption, "The light theme is Latte.")).wrap());
        }
    }

    /// The UI scale in percent. It applies when the pointer lets go, not while it drags.
    fn scale_slider(&mut self, ui: &mut Ui, prefs: &mut Prefs, out: &mut PrefsOutcome) {
        let mut percent = self.pending_scale.unwrap_or(prefs.ui_scale) * 100.0;
        let opts = SliderOpts {
            name: "UI scale",
            step: 5.0,
            decimals: 0,
            unit: "%",
            show_value: true,
        };
        let range = UI_SCALE_MIN * 100.0..=UI_SCALE_MAX * 100.0;
        let response = slider(ui, "prefs_scale", &mut percent, range, &opts);
        if response.changed() {
            self.pending_scale = Some(percent / 100.0);
        }
        let held = response.is_pointer_button_down_on() || response.dragged();
        if let Some(scale) = self.pending_scale
            && !held
        {
            prefs.ui_scale = scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
            self.pending_scale = None;
            out.changed = true;
        }
    }

    fn keymap_tab(
        &mut self,
        ui: &mut Ui,
        prefs: &mut Prefs,
        keymap: &mut Keymap,
        out: &mut PrefsOutcome,
    ) {
        let tokens = Tokens::of(ui.ctx());
        PropertyGrid::new("prefs_keymap").show(ui, |grid| {
            grid.row("Preset", |ui| {
                let mut at = KeymapPreset::ALL
                    .iter()
                    .position(|p| *p == prefs.keymap)
                    .unwrap_or(0);
                let names = KeymapPreset::ALL.map(KeymapPreset::name);
                let size = vec2(PRESET_WIDTH, height::CONTROL);
                let chosen = ui
                    .allocate_ui(size, |ui| dropdown(ui, "keymap_preset", &mut at, &names))
                    .inner;
                let reset = ui.add(Button::new("Reset all keys")).clicked();
                if chosen.changed() {
                    prefs.keymap = KeymapPreset::ALL[at.min(KeymapPreset::ALL.len() - 1)];
                }
                if chosen.changed() || reset {
                    prefs.key_overrides.clear();
                    *keymap = Keymap::preset(prefs.keymap);
                    out.changed = true;
                    out.keymap = true;
                }
            });
        });
        ui.add_space(space::SM);
        search_field(ui, "keymap_filter", &mut self.filter, "Find a command");
        if !self.notice.is_empty() {
            let notice = Label::new(text(Type::Caption, &self.notice).color(tokens.warn));
            ui.add(notice.wrap());
        }
        ui.add_space(space::SM);
        let all = entries();
        let order = search(all, &self.filter, &[]);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, height::ROW, order.len(), |ui, range| {
                for &index in &order[range] {
                    self.key_row(ui, &all[index], prefs, keymap, out);
                }
            });
    }

    /// One command: its name, the button that records its key, and Clear.
    fn key_row(
        &mut self,
        ui: &mut Ui,
        entry: &crate::labels::Entry,
        prefs: &mut Prefs,
        keymap: &mut Keymap,
        out: &mut PrefsOutcome,
    ) {
        let mode = match entry.command {
            Command::Ui(crate::keymap::UiCommand::SetTool(t)) => t.mode(),
            _ => Mode::Model,
        };
        let shortcut = keymap.shortcut_of(entry.command, mode);
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), height::ROW), Sense::hover());
        let label_width = (rect.width() - KEY_WIDTH - height::CONTROL - 2.0 * space::SM).max(0.0);
        let label_rect = Rect::from_min_size(rect.min, vec2(label_width, rect.height()));
        let keys_rect =
            Rect::from_min_max(pos2(label_rect.right() + space::SM, rect.top()), rect.max);
        let layout = Layout::left_to_right(Align::Center);
        let mut name = ui.new_child(UiBuilder::new().max_rect(label_rect).layout(layout));
        name.add(Label::new(text(Type::Body, &entry.label)).truncate());
        let mut row = ui.new_child(UiBuilder::new().max_rect(keys_rect).layout(layout));
        let waiting = self.capture == Some(entry.command);
        let caption = if waiting {
            "Press a key...".to_owned()
        } else {
            shortcut.map_or_else(|| "Not set".to_owned(), |c| c.to_string())
        };
        let button = Button::new(&caption).min_width(KEY_WIDTH).selected(waiting);
        if row.add(button).clicked() {
            self.capture = if waiting { None } else { Some(entry.command) };
            self.notice.clear();
        }
        if shortcut.is_some() && icon_button(&mut row, Icon::Close, "Clear").clicked() {
            keymap.unbind(entry.command);
            self.store_overrides(prefs, keymap, out);
        }
    }

    fn record_chord(
        &mut self,
        ctx: &Context,
        prefs: &mut Prefs,
        keymap: &mut Keymap,
        out: &mut PrefsOutcome,
    ) {
        let Some(command) = self.capture else { return };
        let chords = ctx.input(pressed_chords);
        let Some(chord) = chords.into_iter().next() else {
            return;
        };
        if chord == Chord::plain(Key::Escape) {
            self.capture = None;
            return;
        }
        let displaced = keymap.bind(command, chord);
        self.notice = match displaced {
            Some(other) => format!(
                "{chord} was used by \"{}\", which is now unbound.",
                command_label(other)
            ),
            None => String::new(),
        };
        self.capture = None;
        self.store_overrides(prefs, keymap, out);
    }

    fn store_overrides(&mut self, prefs: &mut Prefs, keymap: &Keymap, out: &mut PrefsOutcome) {
        let commands: Vec<Command> = entries().iter().map(|e| e.command).collect();
        prefs.key_overrides = diff_from_preset(prefs.keymap, keymap, &commands);
        out.changed = true;
        out.keymap = true;
    }
}

fn navigation(ui: &mut Ui, prefs: &mut Prefs, out: &mut PrefsOutcome) {
    PropertyGrid::new("prefs_navigation").show(ui, |grid| {
        grid.row("Camera buttons", |ui| {
            let mut at = NavPreset::ALL
                .iter()
                .position(|p| *p == prefs.nav)
                .unwrap_or(0);
            let names = NavPreset::ALL.map(NavPreset::name);
            if dropdown(ui, "prefs_nav", &mut at, &names).changed() {
                prefs.nav = NavPreset::ALL[at.min(NavPreset::ALL.len() - 1)];
                prefs.nav_chosen = true;
                out.changed = true;
            }
        });
        grid.row("Zoom", |ui| {
            if toggle(ui, &mut prefs.invert_zoom, "Invert zoom").changed() {
                out.changed = true;
            }
        });
        grid.row("Orbit speed", |ui| {
            let opts = SliderOpts {
                name: "Orbit speed",
                step: 0.1,
                decimals: 1,
                ..SliderOpts::default()
            };
            if slider(ui, "prefs_orbit", &mut prefs.orbit_speed, 0.2..=3.0, &opts).changed() {
                out.changed = true;
            }
        });
    });
    ui.add(
        Label::new(text(
            Type::Caption,
            "Alt + left drag always orbits; Alt + Shift + left drag pans.",
        ))
        .wrap(),
    );
}

fn editing(ui: &mut Ui, prefs: &mut Prefs, out: &mut PrefsOutcome) {
    PropertyGrid::new("prefs_editing").show(ui, |grid| {
        grid.row("Snapping", |ui| {
            if toggle(ui, &mut prefs.snap.enabled, "Snap while moving and typing").changed() {
                out.changed = true;
            }
        });
        grid.row("Whole units", |ui| {
            let label = "Snap to whole units instead of the grid step";
            if toggle(ui, &mut prefs.snap.whole_units, label).changed() {
                out.changed = true;
            }
        });
        grid.row("Grid step", |ui| {
            let opts = DragFieldOpts {
                name: "Grid step",
                speed: 0.005,
                decimals: 4,
                range: (1.0 / 256.0)..=16.0,
                width: Some(FIELD_WIDTH),
                ..DragFieldOpts::default()
            };
            if drag_field(ui, "prefs_grid", &mut prefs.snap.grid, &opts).committed {
                out.changed = true;
            }
            menu_trigger(ui, None, "Presets", false, |ui| {
                for (label, value) in [
                    ("1/32", 1.0 / 32.0),
                    ("1/16", 1.0 / 16.0),
                    ("1/8", 0.125),
                    ("1/4", 0.25),
                    ("1/2", 0.5),
                    ("1", 1.0),
                ] {
                    if ui.add(MenuItem::new(label)).clicked() {
                        prefs.snap.grid = value;
                        out.changed = true;
                        ui.close();
                    }
                }
            });
        });
        grid.row("Rotation step", |ui| {
            let opts = DragFieldOpts {
                name: "Rotation step",
                speed: 0.5,
                decimals: 2,
                range: 0.5..=180.0,
                unit: "\u{b0}",
                width: Some(FIELD_WIDTH),
                ..DragFieldOpts::default()
            };
            if drag_field(ui, "prefs_rot", &mut prefs.snap.rotation, &opts).committed {
                out.changed = true;
            }
            menu_trigger(ui, None, "Presets", false, |ui| {
                for value in [5.0, 11.25, 15.0, 22.5, 45.0, 90.0] {
                    if ui.add(MenuItem::new(&format!("{value}\u{b0}"))).clicked() {
                        prefs.snap.rotation = value;
                        out.changed = true;
                        ui.close();
                    }
                }
            });
        });
    });
}

fn general(ui: &mut Ui, prefs: &mut Prefs, keymap: &mut Keymap, out: &mut PrefsOutcome) {
    PropertyGrid::new("prefs_general").show(ui, |grid| {
        grid.row("Start in", |ui| {
            let mut at = Mode::ALL.iter().position(|m| *m == prefs.mode).unwrap_or(0);
            let names = Mode::ALL.map(Mode::name);
            let size = vec2(PRESET_WIDTH, height::CONTROL);
            let chosen = ui
                .allocate_ui(size, |ui| dropdown(ui, "start_mode", &mut at, &names))
                .inner;
            if chosen.changed() {
                prefs.mode = Mode::ALL[at.min(Mode::ALL.len() - 1)];
                out.changed = true;
            }
        });
        grid.row("Autosave", |ui| {
            let mut minutes = f32::from(u16::try_from(prefs.autosave_minutes).unwrap_or(0));
            let opts = DragFieldOpts {
                name: "Autosave minutes",
                speed: 0.1,
                decimals: 0,
                range: 0.0..=120.0,
                snap: Some(1.0),
                unit: "min",
                width: Some(FIELD_WIDTH),
                ..DragFieldOpts::default()
            };
            if drag_field(ui, "prefs_autosave", &mut minutes, &opts).committed {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let value = minutes.round().max(0.0) as u32;
                prefs.autosave_minutes = value;
                out.changed = true;
            }
            ui.label(text(Type::Caption, "0 turns the automatic save off."));
        });
        grid.row("Coming from", |ui| {
            let mut at = ComingFrom::ALL
                .iter()
                .position(|f| *f == prefs.coming_from)
                .unwrap_or(0);
            let names = ComingFrom::ALL.map(ComingFrom::name);
            if segmented(ui, &mut at, &names) {
                prefs.apply_coming_from(ComingFrom::ALL[at.min(ComingFrom::ALL.len() - 1)]);
                *keymap = Keymap::with_overrides(prefs.keymap, &prefs.key_overrides);
                out.changed = true;
                out.keymap = true;
            }
        });
        grid.row("Panels", |ui| {
            if ui.add(Button::new("Reset panel layout")).clicked() {
                prefs.layout = PanelLayout::arranged(prefs.coming_from.arrangement());
                out.changed = true;
            }
        });
    });
    let blurb = format!(
        "{}. The choice sets the camera buttons, the keymap and the panel arrangement together; \
         each can be changed on its own.",
        prefs.coming_from.blurb()
    );
    ui.add(Label::new(text(Type::Caption, blurb)).wrap());
    let hint = "Panels can also be moved with the button in the header of each panel.";
    ui.add(Label::new(text(Type::Caption, hint)).wrap());
}

#[cfg(test)]
mod tests {
    use egui::{Key, Modifiers, Role};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    struct State {
        window: PrefsWindow,
        prefs: Prefs,
        keymap: Keymap,
        changed: bool,
    }

    fn harness(tab: Tab) -> Harness<'static, State> {
        let prefs = Prefs::default();
        let state = State {
            window: PrefsWindow {
                open: true,
                tab,
                ..PrefsWindow::default()
            },
            keymap: Keymap::preset(prefs.keymap),
            prefs,
            changed: false,
        };
        let mut harness = Harness::builder()
            .with_context_setup(crate::dialogs::prepare)
            .with_size(vec2(1280.0, 800.0))
            .build_ui_state(
                |ui, state: &mut State| {
                    let out = state
                        .window
                        .show(ui.ctx(), &mut state.prefs, &mut state.keymap);
                    state.changed |= out.changed;
                },
                state,
            );
        harness.run_steps(3);
        harness
    }

    #[test]
    fn the_anti_aliasing_toggle_changes_the_preference() {
        let mut harness = harness(Tab::Interface);
        assert!(harness.state().prefs.antialiasing);
        harness.get_by_label("Anti-aliasing (4x MSAA)").click();
        harness.run_steps(3);
        assert!(!harness.state().prefs.antialiasing);
        assert!(harness.state().changed);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn the_system_title_bar_toggle_changes_the_preference() {
        let mut harness = harness(Tab::Interface);
        assert!(!harness.state().prefs.system_title_bar);
        harness.get_by_label("Use the system title bar").click();
        harness.run_steps(3);
        assert!(harness.state().prefs.system_title_bar);
        assert!(harness.state().changed);
    }

    #[test]
    fn choosing_the_app_you_know_sets_navigation_keymap_and_panels() {
        let mut harness = harness(Tab::General);
        harness.get_by_label("Cubik Studio").click();
        harness.run_steps(3);
        let state = harness.state();
        assert_eq!(state.prefs.coming_from, ComingFrom::Cubik);
        assert_eq!(state.prefs.nav, NavPreset::Cubik);
        assert_eq!(state.prefs.keymap, KeymapPreset::Cubik);
        assert_eq!(
            state.prefs.layout,
            PanelLayout::arranged(ComingFrom::Cubik.arrangement())
        );
    }

    #[test]
    fn escape_ends_a_key_capture_first_and_closes_the_dialog_next() {
        let mut harness = harness(Tab::Keymap);
        let command = entries()[0].command;
        harness.state_mut().window.capture = Some(command);
        harness.run_steps(2);
        harness.key_press(Key::Escape);
        harness.run_steps(3);
        assert!(harness.state().window.capture.is_none());
        assert!(harness.state().window.open, "Esc only left the capture");
        harness.key_press(Key::Escape);
        harness.run_steps(3);
        assert!(!harness.state().window.open);
    }

    #[test]
    fn done_and_enter_close_the_dialog() {
        let mut harness = harness(Tab::Navigation);
        harness.get_by_role_and_label(Role::Button, "Done").click();
        harness.run_steps(3);
        assert!(!harness.state().window.open);
        harness.state_mut().window.open();
        harness.run_steps(3);
        harness.key_press_modifiers(Modifiers::NONE, Key::Enter);
        harness.run_steps(3);
        assert!(!harness.state().window.open);
    }

    #[test]
    fn the_dialog_keeps_its_size_on_every_tab() {
        let mut harness = harness(Tab::Interface);
        let done = |harness: &Harness<'static, State>| {
            harness.get_by_role_and_label(Role::Button, "Done").rect()
        };
        let first = done(&harness);
        for tab in Tab::ALL {
            harness.get_by_role_and_label(Role::Tab, tab.name()).click();
            harness.run_steps(3);
            assert_eq!(done(&harness), first, "{}", tab.name());
        }
    }
}
