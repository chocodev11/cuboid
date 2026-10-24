//! The preferences window.

use cuboid_ui_kit::{
    DarkFlavor, DragFieldOpts, ThemeFamily, Tokens, UI_SCALE_MAX, UI_SCALE_MIN, color_swatch,
    drag_field, search_field, segmented, tab_button,
};
use egui::{Color32, ComboBox, Context, Key, Slider, Ui, Vec2, Window};

use crate::keymap::{Chord, Command, Keymap, KeymapPreset, diff_from_preset, pressed_chords};
use crate::labels::{command_label, entries, search};
use crate::layout::Layout;
use crate::prefs::{ColorMode, NavPreset, Prefs};
use crate::request::Mode;

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

    pub fn show(&mut self, ctx: &Context, prefs: &mut Prefs, keymap: &mut Keymap) -> PrefsOutcome {
        let mut outcome = PrefsOutcome::default();
        if !self.open {
            return outcome;
        }
        if self.capture.is_some() {
            self.record_chord(ctx, prefs, keymap, &mut outcome);
        }
        let mut open = self.open;
        Window::new("Preferences")
            .open(&mut open)
            .collapsible(false)
            .default_size([640.0, 480.0])
            .min_size([460.0, 320.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for tab in Tab::ALL {
                        if tab_button(ui, tab.name(), self.tab == tab).clicked() {
                            self.tab = tab;
                        }
                    }
                });
                ui.separator();
                ui.add_space(4.0);
                match self.tab {
                    Tab::Interface => self.interface(ui, prefs, &mut outcome),
                    Tab::Navigation => navigation(ui, prefs, &mut outcome),
                    Tab::Keymap => self.keymap_tab(ui, prefs, keymap, &mut outcome),
                    Tab::Editing => editing(ui, prefs, &mut outcome),
                    Tab::General => general(ui, prefs, &mut outcome),
                }
            });
        self.open = open;
        if !self.open {
            self.capture = None;
        }
        outcome
    }

    fn interface(&mut self, ui: &mut Ui, prefs: &mut Prefs, out: &mut PrefsOutcome) {
        let tokens = Tokens::of(ui.ctx());
        ui.label("Theme");
        let mut mode = match prefs.color_mode {
            ColorMode::Dark => 0,
            ColorMode::Light => 1,
            ColorMode::System => 2,
        };
        if segmented(ui, &mut mode, &["Dark", "Light", "System"]) {
            prefs.color_mode = [ColorMode::Dark, ColorMode::Light, ColorMode::System][mode.min(2)];
            out.changed = true;
        }
        ui.add_space(8.0);
        ui.label("Colour scheme");
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
        if prefs.family == ThemeFamily::Catppuccin {
            ui.add_space(8.0);
            ui.label("Dark flavour");
            let mut flavor = DarkFlavor::ALL
                .iter()
                .position(|f| *f == prefs.flavor)
                .unwrap_or(0);
            let names = DarkFlavor::ALL.map(DarkFlavor::name);
            if segmented(ui, &mut flavor, &names) {
                prefs.flavor = DarkFlavor::ALL[flavor.min(2)];
                out.changed = true;
            }
            ui.label(egui::RichText::new("The light theme is Latte.").color(tokens.text_weak));
        }
        ui.add_space(8.0);
        ui.label("Accent colour");
        let theme = prefs.theme();
        let slot = theme.accent_slot();
        ui.horizontal(|ui| {
            let presets = theme.accent_presets(tokens.dark);
            for (i, (preset, name)) in presets
                .into_iter()
                .zip(prefs.family.accent_names())
                .enumerate()
            {
                let color = Color32::from_rgb(preset[0], preset[1], preset[2]);
                if color_swatch(ui, color, Vec2::splat(24.0), slot == Some(i))
                    .on_hover_text(name)
                    .clicked()
                {
                    prefs.accent = preset;
                    prefs.accent_chosen = true;
                    out.changed = true;
                }
            }
        });
        ui.add_space(8.0);
        if ui
            .checkbox(&mut prefs.high_contrast, "High contrast")
            .changed()
        {
            out.changed = true;
        }
        ui.add_space(8.0);
        ui.label("UI scale");
        let mut percent = self.pending_scale.unwrap_or(prefs.ui_scale) * 100.0;
        let resp = ui.add(
            Slider::new(&mut percent, UI_SCALE_MIN * 100.0..=UI_SCALE_MAX * 100.0)
                .step_by(5.0)
                .suffix(" %")
                .fixed_decimals(0),
        );
        if resp.dragged() || resp.has_focus() {
            self.pending_scale = Some(percent / 100.0);
        }
        if resp.drag_stopped() || resp.lost_focus() || (resp.changed() && !resp.dragged()) {
            prefs.ui_scale = (percent / 100.0).clamp(UI_SCALE_MIN, UI_SCALE_MAX);
            self.pending_scale = None;
            out.changed = true;
        }
        ui.add_space(12.0);
        ui.separator();
        if ui.button("Reset panel layout").clicked() {
            prefs.layout = Layout::default();
            out.changed = true;
        }
        ui.label(
            egui::RichText::new("Panels can also be moved from the tab menu of each panel.")
                .color(tokens.text_weak),
        );
    }

    fn keymap_tab(
        &mut self,
        ui: &mut Ui,
        prefs: &mut Prefs,
        keymap: &mut Keymap,
        out: &mut PrefsOutcome,
    ) {
        let tokens = Tokens::of(ui.ctx());
        ui.horizontal(|ui| {
            ui.label("Preset");
            let before = prefs.keymap;
            ComboBox::from_id_salt("keymap_preset")
                .selected_text(prefs.keymap.name())
                .show_ui(ui, |ui| {
                    for preset in KeymapPreset::ALL {
                        ui.selectable_value(&mut prefs.keymap, preset, preset.name());
                    }
                });
            if prefs.keymap != before {
                prefs.key_overrides.clear();
                *keymap = Keymap::preset(prefs.keymap);
                out.changed = true;
                out.keymap = true;
            }
            if ui.button("Reset all keys").clicked() {
                prefs.key_overrides.clear();
                *keymap = Keymap::preset(prefs.keymap);
                out.changed = true;
                out.keymap = true;
            }
        });
        search_field(ui, "keymap_filter", &mut self.filter, "Find a command");
        if !self.notice.is_empty() {
            ui.label(egui::RichText::new(&self.notice).color(tokens.warn));
        }
        let all = entries();
        let order = search(all, &self.filter, &[]);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 26.0, order.len(), |ui, range| {
                for &index in &order[range] {
                    let entry = &all[index];
                    let mode = match entry.command {
                        Command::Ui(crate::keymap::UiCommand::SetTool(t)) => t.mode(),
                        _ => Mode::Model,
                    };
                    let shortcut = keymap.shortcut_of(entry.command, mode);
                    ui.horizontal(|ui| {
                        ui.set_height(24.0);
                        ui.add_sized([230.0, 22.0], egui::Label::new(&entry.label).truncate());
                        let waiting = self.capture == Some(entry.command);
                        let text = if waiting {
                            "Press a key...".to_owned()
                        } else {
                            shortcut.map_or_else(|| "Not set".to_owned(), |c| c.to_string())
                        };
                        if ui
                            .add_sized([130.0, 22.0], egui::Button::new(text).selected(waiting))
                            .clicked()
                        {
                            self.capture = if waiting { None } else { Some(entry.command) };
                            self.notice.clear();
                        }
                        if shortcut.is_some() && ui.small_button("Clear").clicked() {
                            keymap.unbind(entry.command);
                            self.store_overrides(prefs, keymap, out);
                        }
                    });
                }
            });
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
    ui.label("Camera buttons");
    for preset in NavPreset::ALL {
        if ui
            .radio_value(&mut prefs.nav, preset, preset.name())
            .changed()
        {
            prefs.nav_chosen = true;
            out.changed = true;
        }
    }
    ui.add_space(8.0);
    if ui.checkbox(&mut prefs.invert_zoom, "Invert zoom").changed() {
        out.changed = true;
    }
    ui.add_space(8.0);
    ui.label("Orbit speed");
    if ui
        .add(Slider::new(&mut prefs.orbit_speed, 0.2..=3.0).fixed_decimals(1))
        .changed()
    {
        out.changed = true;
    }
    ui.add_space(8.0);
    ui.label("Alt + left drag always orbits; Alt + Shift + left drag pans.");
}

fn editing(ui: &mut Ui, prefs: &mut Prefs, out: &mut PrefsOutcome) {
    if ui
        .checkbox(&mut prefs.snap.enabled, "Snap while moving and typing")
        .changed()
    {
        out.changed = true;
    }
    if ui
        .checkbox(
            &mut prefs.snap.whole_units,
            "Snap to whole units instead of the grid step",
        )
        .changed()
    {
        out.changed = true;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label("Grid step");
        let opts = DragFieldOpts {
            name: "Grid step",
            speed: 0.005,
            decimals: 4,
            range: (1.0 / 256.0)..=16.0,
            width: Some(90.0),
            ..DragFieldOpts::default()
        };
        if drag_field(ui, "prefs_grid", &mut prefs.snap.grid, &opts).committed {
            out.changed = true;
        }
        ComboBox::from_id_salt("grid_presets")
            .selected_text("Presets")
            .show_ui(ui, |ui| {
                for (label, value) in [
                    ("1/32", 1.0 / 32.0),
                    ("1/16", 1.0 / 16.0),
                    ("1/8", 0.125),
                    ("1/4", 0.25),
                    ("1/2", 0.5),
                    ("1", 1.0),
                ] {
                    if ui.selectable_label(false, label).clicked() {
                        prefs.snap.grid = value;
                        out.changed = true;
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("Rotation step");
        let opts = DragFieldOpts {
            name: "Rotation step",
            speed: 0.5,
            decimals: 2,
            range: 0.5..=180.0,
            unit: "\u{b0}",
            width: Some(90.0),
            ..DragFieldOpts::default()
        };
        if drag_field(ui, "prefs_rot", &mut prefs.snap.rotation, &opts).committed {
            out.changed = true;
        }
        ComboBox::from_id_salt("rot_presets")
            .selected_text("Presets")
            .show_ui(ui, |ui| {
                for value in [5.0, 11.25, 15.0, 22.5, 45.0, 90.0] {
                    if ui
                        .selectable_label(false, format!("{value}\u{b0}"))
                        .clicked()
                    {
                        prefs.snap.rotation = value;
                        out.changed = true;
                    }
                }
            });
    });
}

fn general(ui: &mut Ui, prefs: &mut Prefs, out: &mut PrefsOutcome) {
    ui.horizontal(|ui| {
        ui.label("Start in");
        ComboBox::from_id_salt("start_mode")
            .selected_text(prefs.mode.name())
            .show_ui(ui, |ui| {
                for mode in Mode::ALL {
                    if ui
                        .selectable_value(&mut prefs.mode, mode, mode.name())
                        .changed()
                    {
                        out.changed = true;
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("Automatic recovery save every");
        let mut minutes = f32::from(u16::try_from(prefs.autosave_minutes).unwrap_or(0));
        let opts = DragFieldOpts {
            name: "Autosave minutes",
            speed: 0.1,
            decimals: 0,
            range: 0.0..=120.0,
            snap: Some(1.0),
            unit: "min",
            width: Some(80.0),
            ..DragFieldOpts::default()
        };
        if drag_field(ui, "prefs_autosave", &mut minutes, &opts).committed {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let value = minutes.round().max(0.0) as u32;
            prefs.autosave_minutes = value;
            out.changed = true;
        }
    });
    ui.label("0 turns the automatic save off.");
}
