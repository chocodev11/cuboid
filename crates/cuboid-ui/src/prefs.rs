//! User preferences of the interface, stored as one JSON text the app keeps next to the other
//! settings.

use cuboid_ui_kit::{ACCENT_PRESETS, Theme, ThemeMode, UI_SCALE_MAX, UI_SCALE_MIN};
use serde::{Deserialize, Serialize};

use crate::keymap::{KeyOverride, KeymapPreset};
use crate::layout::{Layout, ViewLayout};
use crate::model::ViewSlot;
use crate::request::{Mode, ToolOptions};

/// Which pointer buttons move the camera.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum NavPreset {
    /// Middle drag orbits, Shift + middle drag pans, Ctrl + middle drag zooms.
    #[default]
    Blender,
    /// Right drag orbits, middle drag pans.
    Blockbench,
}

impl NavPreset {
    /// Both presets.
    pub const ALL: [NavPreset; 2] = [NavPreset::Blender, NavPreset::Blockbench];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            NavPreset::Blender => "Blender-style (middle drag)",
            NavPreset::Blockbench => "Blockbench-style (right drag)",
        }
    }
}

/// Stored colour mode.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ColorMode {
    /// Dark theme.
    #[default]
    Dark,
    /// Light theme.
    Light,
    /// Follow the system.
    System,
}

/// Snapping steps of the transform gizmo and numeric fields.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snap {
    /// Snapping is on.
    pub enabled: bool,
    /// Movement step in model units; 1/16 of a block by default.
    pub grid: f32,
    /// Round to whole units instead of `grid` (used by size fields).
    pub whole_units: bool,
    /// Rotation step in degrees.
    pub rotation: f32,
}

impl Default for Snap {
    fn default() -> Self {
        Self {
            enabled: true,
            grid: 1.0 / 16.0,
            whole_units: false,
            rotation: 22.5,
        }
    }
}

impl Snap {
    /// The movement step the fields and gizmos use, `None` when snapping is off.
    pub fn step(&self) -> Option<f32> {
        if !self.enabled {
            return None;
        }
        Some(if self.whole_units {
            1.0
        } else {
            self.grid.max(1.0 / 256.0)
        })
    }
}

/// All interface preferences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Format version of the stored text.
    pub version: u32,
    /// Dark, light or system.
    pub color_mode: ColorMode,
    /// Accent colour.
    pub accent: [u8; 3],
    /// Stronger contrast.
    pub high_contrast: bool,
    /// UI scale, 0.75 to 2.0.
    pub ui_scale: f32,
    /// Camera mouse buttons.
    pub nav: NavPreset,
    /// Zoom the other way.
    pub invert_zoom: bool,
    /// Orbit and pan speed factor.
    pub orbit_speed: f32,
    /// Key preset.
    pub keymap: KeymapPreset,
    /// Changes to the key preset.
    pub key_overrides: Vec<KeyOverride>,
    /// Snapping.
    pub snap: Snap,
    /// Panel placement.
    pub layout: Layout,
    /// Number of views.
    pub view_layout: ViewLayout,
    /// Which scene view each cell shows.
    pub view_cells: [u8; 4],
    /// Workspace at start-up.
    pub mode: Mode,
    /// Tool options.
    pub tool_options: ToolOptions,
    /// Minutes between automatic recovery saves; 0 turns it off. The app owns the timer.
    pub autosave_minutes: u32,
}

/// The current format version.
pub const PREFS_VERSION: u32 = 1;

impl Default for Prefs {
    fn default() -> Self {
        Self {
            version: PREFS_VERSION,
            color_mode: ColorMode::Dark,
            accent: ACCENT_PRESETS[0],
            high_contrast: false,
            ui_scale: 1.0,
            nav: NavPreset::Blender,
            invert_zoom: false,
            orbit_speed: 1.0,
            keymap: KeymapPreset::Blender,
            key_overrides: Vec::new(),
            snap: Snap::default(),
            layout: Layout::default(),
            view_layout: ViewLayout::Single,
            view_cells: [
                ViewSlot::MAIN.0,
                ViewSlot::FRONT.0,
                ViewSlot::TOP.0,
                ViewSlot::SIDE.0,
            ],
            mode: Mode::Model,
            tool_options: ToolOptions::default(),
            autosave_minutes: 3,
        }
    }
}

impl Prefs {
    /// Reads stored text. Anything unreadable gives the defaults for the broken part: a bad
    /// document gives all defaults, a missing field its default, an out-of-range value the nearest
    /// valid one.
    pub fn from_json(text: &str) -> Prefs {
        let mut prefs = match serde_json::from_str::<Prefs>(text) {
            Ok(prefs) => prefs,
            Err(err) => {
                log::warn!("ui: preferences unreadable, using defaults: {err}");
                Prefs::default()
            }
        };
        prefs.sanitize();
        prefs
    }

    /// The text to store.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|err| {
            log::error!("ui: cannot write preferences: {err}");
            String::new()
        })
    }

    /// Forces every value into its valid range.
    pub fn sanitize(&mut self) {
        self.version = PREFS_VERSION;
        self.ui_scale = if self.ui_scale.is_finite() {
            self.ui_scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX)
        } else {
            1.0
        };
        self.orbit_speed = if self.orbit_speed.is_finite() {
            self.orbit_speed.clamp(0.1, 5.0)
        } else {
            1.0
        };
        self.snap.grid = if self.snap.grid.is_finite() && self.snap.grid > 0.0 {
            self.snap.grid.clamp(1.0 / 256.0, 16.0)
        } else {
            Snap::default().grid
        };
        self.snap.rotation = if self.snap.rotation.is_finite() && self.snap.rotation > 0.0 {
            self.snap.rotation.clamp(0.5, 180.0)
        } else {
            Snap::default().rotation
        };
        for cell in &mut self.view_cells {
            *cell = (*cell).min(3);
        }
        self.tool_options.brush = self.tool_options.brush.clamp(1, 64);
        self.autosave_minutes = self.autosave_minutes.min(120);
        self.layout.sanitize();
    }

    /// The kit theme these preferences describe.
    pub fn theme(&self) -> Theme {
        Theme {
            mode: match self.color_mode {
                ColorMode::Dark => ThemeMode::Dark,
                ColorMode::Light => ThemeMode::Light,
                ColorMode::System => ThemeMode::System,
            },
            accent: self.accent,
            high_contrast: self.high_contrast,
            scale: self.ui_scale,
        }
    }

    /// The scene view shown in cell `index`.
    pub fn cell_slot(&self, index: usize) -> ViewSlot {
        ViewSlot(
            self.view_cells
                .get(index)
                .copied()
                .unwrap_or(ViewSlot::MAIN.0)
                .min(3),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_a_round_trip() {
        let prefs = Prefs::default();
        assert_eq!(Prefs::from_json(&prefs.to_json()), prefs);
    }

    #[test]
    fn changes_survive_a_round_trip() {
        let mut prefs = Prefs {
            color_mode: ColorMode::Light,
            ui_scale: 1.5,
            nav: NavPreset::Blockbench,
            keymap: KeymapPreset::Blockbench,
            view_layout: ViewLayout::Quad,
            key_overrides: vec![KeyOverride {
                command: "action:Save".into(),
                chord: Some("Ctrl+Alt+S".into()),
            }],
            ..Prefs::default()
        };
        prefs.layout.zones[2].open = true;
        assert_eq!(Prefs::from_json(&prefs.to_json()), prefs);
    }

    #[test]
    fn broken_text_gives_defaults() {
        assert_eq!(Prefs::from_json(""), Prefs::default());
        assert_eq!(Prefs::from_json("not json"), Prefs::default());
        assert_eq!(Prefs::from_json("[1,2,3]"), Prefs::default());
    }

    #[test]
    fn missing_fields_take_their_defaults_and_unknown_ones_are_ignored() {
        let prefs = Prefs::from_json(r#"{"ui_scale": 1.25, "future_field": [1]}"#);
        assert!((prefs.ui_scale - 1.25).abs() < f32::EPSILON);
        assert_eq!(prefs.nav, NavPreset::Blender);
        assert_eq!(prefs.layout, Layout::default());
    }

    #[test]
    fn out_of_range_values_are_pulled_in() {
        let prefs = Prefs::from_json(
            r#"{"ui_scale": 9.0, "orbit_speed": -3, "view_cells": [9,9,9,9], "autosave_minutes": 99999,
                "snap": {"grid": 0.0, "rotation": -1.0}, "layout": {"zones": [{"size": 5.0}, {}, {}]}}"#,
        );
        assert!((prefs.ui_scale - UI_SCALE_MAX).abs() < f32::EPSILON);
        assert!(prefs.orbit_speed >= 0.1);
        assert_eq!(prefs.view_cells, [3, 3, 3, 3]);
        assert_eq!(prefs.autosave_minutes, 120);
        assert!(prefs.snap.grid > 0.0 && prefs.snap.rotation > 0.0);
        assert!(prefs.layout.zones[0].size >= crate::layout::SIDE_MIN);
    }

    #[test]
    fn snapping_steps() {
        let mut snap = Snap::default();
        assert_eq!(snap.step(), Some(0.0625));
        snap.whole_units = true;
        assert_eq!(snap.step(), Some(1.0));
        snap.enabled = false;
        assert_eq!(snap.step(), None);
    }
}
