//! Panel placement: which panels sit in which zone, and how large the zones are.
//!
//! The docking crate `egui_dock` needs egui 0.36, so the window has three resizable zones
//! (left, right, bottom). A panel can be moved between them from its tab menu.

use serde::{Deserialize, Serialize};

/// A dockable panel.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    /// The object tree.
    Outliner,
    /// Transform, element, faces, textures and palette of the selection.
    Properties,
    /// The UV editor.
    Uv,
    /// The texture list with previews.
    Textures,
    /// The undo history.
    History,
    /// The asset library.
    Library,
    /// The findings of the model checker.
    Validator,
}

impl PanelId {
    /// Every panel.
    pub const ALL: [PanelId; 7] = [
        PanelId::Outliner,
        PanelId::Properties,
        PanelId::Uv,
        PanelId::Textures,
        PanelId::History,
        PanelId::Library,
        PanelId::Validator,
    ];

    /// Tab title.
    pub fn title(self) -> &'static str {
        match self {
            PanelId::Outliner => "Outliner",
            PanelId::Properties => "Properties",
            PanelId::Uv => "UV editor",
            PanelId::Textures => "Textures",
            PanelId::History => "History",
            PanelId::Library => "Library",
            PanelId::Validator => "Validator",
        }
    }
}

/// One of the three areas around the viewport.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Zone {
    /// Left of the viewport.
    Left,
    /// Right of the viewport.
    Right,
    /// Below the viewport.
    Bottom,
}

impl Zone {
    /// All zones.
    pub const ALL: [Zone; 3] = [Zone::Left, Zone::Right, Zone::Bottom];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            Zone::Left => "left",
            Zone::Right => "right",
            Zone::Bottom => "bottom",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Smallest and largest width of a side zone, and height of the bottom zone, in points.
pub const SIDE_MIN: f32 = 180.0;
/// See [`SIDE_MIN`].
pub const SIDE_MAX: f32 = 560.0;
/// See [`SIDE_MIN`].
pub const BOTTOM_MIN: f32 = 120.0;
/// See [`SIDE_MIN`].
pub const BOTTOM_MAX: f32 = 520.0;

/// The panels of one zone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoneState {
    /// Panels as tabs, in order.
    pub panels: Vec<PanelId>,
    /// Index of the shown tab.
    pub active: usize,
    /// The zone is shown.
    pub open: bool,
    /// Width (side zones) or height (bottom zone) in points.
    pub size: f32,
}

impl Default for ZoneState {
    fn default() -> Self {
        Self {
            panels: Vec::new(),
            active: 0,
            open: true,
            size: 260.0,
        }
    }
}

impl ZoneState {
    /// The shown panel.
    pub fn current(&self) -> Option<PanelId> {
        self.panels.get(self.active).copied()
    }
}

/// Where every panel is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Zones in [`Zone::ALL`] order.
    pub zones: [ZoneState; 3],
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            zones: [
                ZoneState {
                    panels: vec![PanelId::Outliner, PanelId::Library],
                    size: 250.0,
                    ..ZoneState::default()
                },
                ZoneState {
                    panels: vec![PanelId::Properties],
                    size: 310.0,
                    ..ZoneState::default()
                },
                ZoneState {
                    panels: vec![
                        PanelId::Uv,
                        PanelId::Textures,
                        PanelId::History,
                        PanelId::Validator,
                    ],
                    open: false,
                    size: 240.0,
                    ..ZoneState::default()
                },
            ],
        }
    }
}

impl Layout {
    /// The zone holding `panel`.
    pub fn zone_of(&self, panel: PanelId) -> Option<Zone> {
        Zone::ALL
            .into_iter()
            .find(|z| self.zones[z.index()].panels.contains(&panel))
    }

    /// The state of a zone.
    pub fn zone(&self, zone: Zone) -> &ZoneState {
        &self.zones[zone.index()]
    }

    /// The state of a zone, mutable.
    pub fn zone_mut(&mut self, zone: Zone) -> &mut ZoneState {
        &mut self.zones[zone.index()]
    }

    /// Moves a panel to the end of `to` and makes it the shown tab there.
    pub fn move_panel(&mut self, panel: PanelId, to: Zone) {
        for zone in &mut self.zones {
            zone.panels.retain(|p| *p != panel);
            zone.active = zone.active.min(zone.panels.len().saturating_sub(1));
        }
        let target = &mut self.zones[to.index()];
        target.panels.push(panel);
        target.active = target.panels.len() - 1;
        target.open = true;
    }

    /// Shows a panel: opens its zone and selects its tab.
    pub fn reveal(&mut self, panel: PanelId) {
        let Some(zone) = self.zone_of(panel) else {
            return;
        };
        let state = self.zone_mut(zone);
        state.open = true;
        state.active = state.panels.iter().position(|p| *p == panel).unwrap_or(0);
    }

    /// Toggles a panel: hides its zone when the panel is already the shown tab of an open zone,
    /// reveals it otherwise.
    pub fn toggle(&mut self, panel: PanelId) {
        let Some(zone) = self.zone_of(panel) else {
            return;
        };
        let state = self.zone_mut(zone);
        if state.open && state.current() == Some(panel) {
            state.open = false;
        } else {
            self.reveal(panel);
        }
    }

    /// Repairs a layout read from disk: every panel exactly once, sizes in range, tab indices
    /// valid.
    pub fn sanitize(&mut self) {
        let defaults = Layout::default();
        let mut seen = Vec::new();
        for zone in &mut self.zones {
            zone.panels.retain(|p| {
                let fresh = !seen.contains(p);
                if fresh {
                    seen.push(*p);
                }
                fresh
            });
        }
        for panel in PanelId::ALL {
            if !seen.contains(&panel) {
                let home = defaults.zone_of(panel).unwrap_or(Zone::Right);
                self.zones[home.index()].panels.push(panel);
            }
        }
        for (i, zone) in self.zones.iter_mut().enumerate() {
            let (lo, hi) = if i == Zone::Bottom.index() {
                (BOTTOM_MIN, BOTTOM_MAX)
            } else {
                (SIDE_MIN, SIDE_MAX)
            };
            zone.size = if zone.size.is_finite() {
                zone.size.clamp(lo, hi)
            } else {
                260.0
            };
            zone.active = zone.active.min(zone.panels.len().saturating_sub(1));
        }
    }
}

/// How many views the viewport area shows.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ViewLayout {
    /// One view.
    #[default]
    Single,
    /// Two views side by side.
    SplitH,
    /// Two views stacked.
    SplitV,
    /// Four views in a grid.
    Quad,
}

impl ViewLayout {
    /// All layouts.
    pub const ALL: [ViewLayout; 4] = [
        ViewLayout::Single,
        ViewLayout::SplitH,
        ViewLayout::SplitV,
        ViewLayout::Quad,
    ];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            ViewLayout::Single => "Single view",
            ViewLayout::SplitH => "Two views side by side",
            ViewLayout::SplitV => "Two views stacked",
            ViewLayout::Quad => "Four views",
        }
    }

    /// Number of cells.
    pub fn cells(self) -> usize {
        match self {
            ViewLayout::Single => 1,
            ViewLayout::SplitH | ViewLayout::SplitV => 2,
            ViewLayout::Quad => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_every_panel_once() {
        let layout = Layout::default();
        for panel in PanelId::ALL {
            let count: usize = layout
                .zones
                .iter()
                .map(|z| z.panels.iter().filter(|p| **p == panel).count())
                .sum();
            assert_eq!(count, 1, "{panel:?}");
        }
    }

    #[test]
    fn moving_a_panel_removes_it_from_its_old_zone() {
        let mut layout = Layout::default();
        layout.move_panel(PanelId::Outliner, Zone::Bottom);
        assert_eq!(layout.zone_of(PanelId::Outliner), Some(Zone::Bottom));
        assert!(!layout.zone(Zone::Left).panels.contains(&PanelId::Outliner));
        let bottom = layout.zone(Zone::Bottom);
        assert!(bottom.open);
        assert_eq!(bottom.current(), Some(PanelId::Outliner));
    }

    #[test]
    fn toggle_hides_the_shown_panel_and_reveals_a_hidden_one() {
        let mut layout = Layout::default();
        layout.toggle(PanelId::Properties);
        assert!(!layout.zone(Zone::Right).open);
        layout.toggle(PanelId::Properties);
        assert!(layout.zone(Zone::Right).open);
        // A background tab of an open zone is revealed instead of hiding the zone.
        layout.toggle(PanelId::History);
        let bottom = layout.zone(Zone::Bottom);
        assert!(bottom.open);
        assert_eq!(bottom.current(), Some(PanelId::History));
        layout.toggle(PanelId::Uv);
        assert_eq!(layout.zone(Zone::Bottom).current(), Some(PanelId::Uv));
    }

    #[test]
    fn sanitize_repairs_duplicates_missing_panels_and_sizes() {
        let mut layout = Layout::default();
        layout.zones[0].panels = vec![PanelId::Outliner, PanelId::Outliner, PanelId::Uv];
        layout.zones[1].panels = vec![PanelId::Uv];
        layout.zones[0].size = f32::NAN;
        layout.zones[1].size = 9000.0;
        layout.zones[2].active = 40;
        layout.sanitize();
        for panel in PanelId::ALL {
            let count: usize = layout
                .zones
                .iter()
                .map(|z| z.panels.iter().filter(|p| **p == panel).count())
                .sum();
            assert_eq!(count, 1, "{panel:?}");
        }
        assert!(layout.zones[0].size >= SIDE_MIN && layout.zones[0].size <= SIDE_MAX);
        assert!((layout.zones[1].size - SIDE_MAX).abs() < f32::EPSILON);
        assert!(layout.zones[2].active < layout.zones[2].panels.len().max(1));
    }
}
