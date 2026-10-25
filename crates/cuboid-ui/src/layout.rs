//! Panel placement: which panels sit in which area, how the areas are split and how large they
//! are.
//!
//! The docking crate `egui_dock` needs egui 0.36, so the window has two resizable areas (left and
//! right of the viewports). Each area is cut into two groups, one above the other, and a group
//! shows its panels as tabs. Every panel can be moved to the other area from its tab menu.

use serde::{Deserialize, Serialize};

/// A dockable panel.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    /// The object tree.
    Outliner,
    /// The element, faces, texture, model, display and validator tabs of the selection.
    Properties,
    /// The UV editor.
    Uv,
    /// The texture list with previews.
    Textures,
    /// The undo history.
    History,
    /// The asset library.
    Library,
    /// The colour palette and the two tool colours.
    Palette,
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
        PanelId::Palette,
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
            PanelId::Palette => "Palette",
        }
    }
}

/// One of the two areas beside the viewports.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Zone {
    /// Left of the viewports, after the tool strip.
    Left,
    /// Right of the viewports.
    Right,
}

impl Zone {
    /// Both areas.
    pub const ALL: [Zone; 2] = [Zone::Left, Zone::Right];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            Zone::Left => "left",
            Zone::Right => "right",
        }
    }

    /// The other area.
    pub fn other(self) -> Zone {
        match self {
            Zone::Left => Zone::Right,
            Zone::Right => Zone::Left,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Smallest and largest width of an area in points.
pub const SIDE_MIN: f32 = 180.0;
/// See [`SIDE_MIN`].
pub const SIDE_MAX: f32 = 400.0;
/// Smallest and largest share of an area's height that its upper group takes.
pub const SPLIT_MIN: f32 = 0.15;
/// See [`SPLIT_MIN`].
pub const SPLIT_MAX: f32 = 0.85;

/// The panels of one group, shown as tabs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    /// Panels as tabs, in order.
    pub panels: Vec<PanelId>,
    /// Index of the shown tab.
    pub active: usize,
}

impl Group {
    fn of(panels: &[PanelId]) -> Self {
        Self {
            panels: panels.to_vec(),
            active: 0,
        }
    }

    /// The shown panel.
    pub fn current(&self) -> Option<PanelId> {
        self.panels.get(self.active).copied()
    }
}

/// One area: two groups, one above the other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoneState {
    /// The upper and the lower group; an empty group is not drawn and the other takes the area.
    pub groups: [Group; 2],
    /// The area is shown.
    pub open: bool,
    /// Width in points.
    pub size: f32,
    /// Share of the height the upper group takes while both are shown.
    pub split: f32,
}

impl Default for ZoneState {
    fn default() -> Self {
        Self {
            groups: [Group::default(), Group::default()],
            open: true,
            size: 240.0,
            split: 0.5,
        }
    }
}

impl ZoneState {
    /// The groups that hold panels, with their index.
    pub fn shown_groups(&self) -> impl Iterator<Item = (usize, &Group)> {
        self.groups
            .iter()
            .enumerate()
            .filter(|(_, g)| !g.panels.is_empty())
    }

    /// The panel shown in the first group that holds one, for a caller that needs "the" panel.
    pub fn first_current(&self) -> Option<PanelId> {
        self.shown_groups().find_map(|(_, g)| g.current())
    }

    /// The panel is the shown tab of its group, and the area is open.
    pub fn shows(&self, panel: PanelId) -> bool {
        self.open && self.groups.iter().any(|g| g.current() == Some(panel))
    }
}

/// Which arrangement a layout starts from; the data of the familiarity presets.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Arrangement {
    /// UV editor and textures on the left, outliner above properties on the right.
    #[default]
    Standard,
    /// As in Cubik Studio: properties on the left, outliner on the right.
    Cubik,
}

/// Where every panel is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Areas in [`Zone::ALL`] order.
    pub zones: [ZoneState; 2],
}

impl Default for Layout {
    fn default() -> Self {
        Layout::arranged(Arrangement::Standard)
    }
}

impl Layout {
    /// The layout a familiarity preset starts with.
    pub fn arranged(arrangement: Arrangement) -> Self {
        use PanelId::{History, Library, Outliner, Palette, Properties, Textures, Uv};
        let zones = match arrangement {
            Arrangement::Standard => [
                ZoneState {
                    groups: [
                        Group::of(&[Uv, Library]),
                        Group::of(&[Textures, Palette, History]),
                    ],
                    size: 240.0,
                    split: 0.45,
                    ..ZoneState::default()
                },
                ZoneState {
                    groups: [Group::of(&[Outliner]), Group::of(&[Properties])],
                    size: 336.0,
                    split: 0.4,
                    ..ZoneState::default()
                },
            ],
            Arrangement::Cubik => [
                ZoneState {
                    groups: [
                        Group::of(&[Properties]),
                        Group::of(&[Uv, Textures, Palette, History]),
                    ],
                    size: 336.0,
                    split: 0.55,
                    ..ZoneState::default()
                },
                ZoneState {
                    groups: [Group::of(&[Outliner, Library]), Group::default()],
                    size: 260.0,
                    split: 0.5,
                    ..ZoneState::default()
                },
            ],
        };
        Layout { zones }
    }

    /// The area holding `panel`.
    pub fn zone_of(&self, panel: PanelId) -> Option<Zone> {
        Zone::ALL.into_iter().find(|z| {
            self.zone(*z)
                .groups
                .iter()
                .any(|g| g.panels.contains(&panel))
        })
    }

    /// The state of an area.
    pub fn zone(&self, zone: Zone) -> &ZoneState {
        &self.zones[zone.index()]
    }

    /// The state of an area, mutable.
    pub fn zone_mut(&mut self, zone: Zone) -> &mut ZoneState {
        &mut self.zones[zone.index()]
    }

    /// The panel is the shown tab of an open area.
    pub fn is_shown(&self, panel: PanelId) -> bool {
        self.zone_of(panel)
            .is_some_and(|z| self.zone(z).shows(panel))
    }

    /// Moves a panel to `to` and makes it the shown tab there. It joins the lower group, which
    /// the move creates when the area has only one; an empty area takes it as its only group.
    pub fn move_panel(&mut self, panel: PanelId, to: Zone) {
        self.remove(panel);
        let target = self.zone_mut(to);
        let slot = usize::from(target.groups.iter().any(|g| !g.panels.is_empty()));
        let group = &mut target.groups[slot];
        group.panels.push(panel);
        group.active = group.panels.len() - 1;
        target.open = true;
    }

    fn remove(&mut self, panel: PanelId) {
        for zone in &mut self.zones {
            for group in &mut zone.groups {
                group.panels.retain(|p| *p != panel);
                group.active = group.active.min(group.panels.len().saturating_sub(1));
            }
        }
    }

    /// Shows a panel: opens its area and selects its tab.
    pub fn reveal(&mut self, panel: PanelId) {
        let Some(zone) = self.zone_of(panel) else {
            return;
        };
        let state = self.zone_mut(zone);
        state.open = true;
        Self::select(state, panel);
    }

    /// Brings a panel to the front of its group without opening a closed area.
    pub fn bring_forward(&mut self, panel: PanelId) {
        if let Some(zone) = self.zone_of(panel) {
            Self::select(self.zone_mut(zone), panel);
        }
    }

    fn select(state: &mut ZoneState, panel: PanelId) {
        for group in &mut state.groups {
            if let Some(at) = group.panels.iter().position(|p| *p == panel) {
                group.active = at;
            }
        }
    }

    /// Toggles a panel: hides its area when the panel is already the shown tab of an open area,
    /// reveals it otherwise.
    pub fn toggle(&mut self, panel: PanelId) {
        let Some(zone) = self.zone_of(panel) else {
            return;
        };
        if self.zone(zone).shows(panel) {
            self.zone_mut(zone).open = false;
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
            for group in &mut zone.groups {
                group.panels.retain(|p| {
                    let fresh = !seen.contains(p);
                    if fresh {
                        seen.push(*p);
                    }
                    fresh
                });
            }
        }
        for panel in PanelId::ALL {
            if seen.contains(&panel) {
                continue;
            }
            let home = defaults.zone_of(panel).unwrap_or(Zone::Right);
            let slot = defaults
                .zone(home)
                .groups
                .iter()
                .position(|g| g.panels.contains(&panel))
                .unwrap_or(0);
            self.zone_mut(home).groups[slot].panels.push(panel);
        }
        for zone in &mut self.zones {
            zone.size = if zone.size.is_finite() {
                zone.size.clamp(SIDE_MIN, SIDE_MAX)
            } else {
                ZoneState::default().size
            };
            zone.split = if zone.split.is_finite() {
                zone.split.clamp(SPLIT_MIN, SPLIT_MAX)
            } else {
                0.5
            };
            for group in &mut zone.groups {
                group.active = group.active.min(group.panels.len().saturating_sub(1));
            }
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

    fn count(layout: &Layout, panel: PanelId) -> usize {
        layout
            .zones
            .iter()
            .flat_map(|z| &z.groups)
            .map(|g| g.panels.iter().filter(|p| **p == panel).count())
            .sum()
    }

    #[test]
    fn every_arrangement_has_every_panel_once() {
        for arrangement in [Arrangement::Standard, Arrangement::Cubik] {
            let layout = Layout::arranged(arrangement);
            for panel in PanelId::ALL {
                assert_eq!(count(&layout, panel), 1, "{arrangement:?} {panel:?}");
            }
        }
    }

    #[test]
    fn the_standard_arrangement_keeps_the_outliner_on_the_right_above_the_properties() {
        let layout = Layout::default();
        let right = layout.zone(Zone::Right);
        assert_eq!(right.groups[0].current(), Some(PanelId::Outliner));
        assert_eq!(right.groups[1].current(), Some(PanelId::Properties));
        assert!((right.split - 0.4).abs() < f32::EPSILON);
        let left = layout.zone(Zone::Left);
        assert_eq!(left.groups[0].panels[0], PanelId::Uv);
        assert_eq!(left.groups[1].panels[0], PanelId::Textures);
        assert!(left.groups[0].panels.contains(&PanelId::Library));
        assert!(left.groups[1].panels.contains(&PanelId::History));
        // The palette is one panel of its own, beside the textures.
        assert_eq!(left.groups[1].panels[1], PanelId::Palette);
    }

    #[test]
    fn the_palette_is_in_the_left_area_of_every_arrangement_and_comes_forward() {
        for arrangement in [Arrangement::Standard, Arrangement::Cubik] {
            let mut layout = Layout::arranged(arrangement);
            assert_eq!(layout.zone_of(PanelId::Palette), Some(Zone::Left));
            assert!(!layout.is_shown(PanelId::Palette));
            layout.bring_forward(PanelId::Palette);
            assert!(layout.is_shown(PanelId::Palette));
        }
    }

    #[test]
    fn a_layout_stored_before_the_palette_panel_gets_it_back_in_its_home() {
        let mut layout = Layout::default();
        for zone in &mut layout.zones {
            for group in &mut zone.groups {
                group.panels.retain(|p| *p != PanelId::Palette);
            }
        }
        assert_eq!(layout.zone_of(PanelId::Palette), None);
        layout.sanitize();
        assert_eq!(layout.zone_of(PanelId::Palette), Some(Zone::Left));
    }

    #[test]
    fn the_cubik_arrangement_swaps_properties_and_outliner() {
        let layout = Layout::arranged(Arrangement::Cubik);
        assert_eq!(layout.zone_of(PanelId::Properties), Some(Zone::Left));
        assert_eq!(layout.zone_of(PanelId::Outliner), Some(Zone::Right));
    }

    #[test]
    fn moving_a_panel_removes_it_from_its_old_place() {
        let mut layout = Layout::default();
        layout.move_panel(PanelId::Outliner, Zone::Left);
        assert_eq!(layout.zone_of(PanelId::Outliner), Some(Zone::Left));
        assert_eq!(count(&layout, PanelId::Outliner), 1);
        let left = layout.zone(Zone::Left);
        assert!(left.open);
        assert!(left.shows(PanelId::Outliner));
        // The right area keeps the properties alone.
        assert_eq!(
            layout.zone(Zone::Right).first_current(),
            Some(PanelId::Properties)
        );
    }

    #[test]
    fn a_moved_panel_joins_the_lower_group_or_an_empty_area() {
        let mut layout = Layout::arranged(Arrangement::Cubik);
        // The right area has one group: the move creates the lower one.
        layout.move_panel(PanelId::Textures, Zone::Right);
        let right = layout.zone(Zone::Right);
        assert_eq!(right.groups[1].panels, [PanelId::Textures]);
        let mut layout = Layout::default();
        for panel in [PanelId::Outliner, PanelId::Properties] {
            layout.move_panel(panel, Zone::Left);
        }
        assert!(
            layout
                .zone(Zone::Right)
                .groups
                .iter()
                .all(|g| g.panels.is_empty())
        );
        layout.move_panel(PanelId::Uv, Zone::Right);
        assert_eq!(layout.zone(Zone::Right).groups[0].panels, [PanelId::Uv]);
    }

    #[test]
    fn toggle_hides_the_shown_panel_and_reveals_a_hidden_one() {
        let mut layout = Layout::default();
        layout.toggle(PanelId::Properties);
        assert!(!layout.zone(Zone::Right).open);
        layout.toggle(PanelId::Properties);
        assert!(layout.zone(Zone::Right).open);
        // A background tab is revealed instead of hiding the area.
        layout.toggle(PanelId::History);
        let left = layout.zone(Zone::Left);
        assert!(left.open && left.shows(PanelId::History));
        layout.toggle(PanelId::Library);
        assert!(layout.zone(Zone::Left).shows(PanelId::Library));
        assert!(!layout.zone(Zone::Left).shows(PanelId::Uv));
        assert!(layout.zone(Zone::Left).open);
    }

    #[test]
    fn bring_forward_does_not_open_a_closed_area() {
        let mut layout = Layout::default();
        layout.zone_mut(Zone::Left).open = false;
        layout.bring_forward(PanelId::Library);
        assert!(!layout.zone(Zone::Left).open);
        layout.zone_mut(Zone::Left).open = true;
        assert!(layout.zone(Zone::Left).shows(PanelId::Library));
    }

    #[test]
    fn sanitize_repairs_duplicates_missing_panels_and_sizes() {
        let mut layout = Layout::default();
        layout.zones[0].groups[0].panels = vec![PanelId::Outliner, PanelId::Outliner, PanelId::Uv];
        layout.zones[1].groups[1].panels = vec![PanelId::Uv];
        layout.zones[0].size = f32::NAN;
        layout.zones[1].size = 9000.0;
        layout.zones[1].split = 7.0;
        layout.zones[0].groups[1].active = 40;
        layout.sanitize();
        for panel in PanelId::ALL {
            assert_eq!(count(&layout, panel), 1, "{panel:?}");
        }
        assert!(layout.zones[0].size >= SIDE_MIN && layout.zones[0].size <= SIDE_MAX);
        assert!((layout.zones[1].size - SIDE_MAX).abs() < f32::EPSILON);
        assert!((layout.zones[1].split - SPLIT_MAX).abs() < f32::EPSILON);
        for zone in &layout.zones {
            for group in &zone.groups {
                assert!(group.active < group.panels.len().max(1));
            }
        }
    }
}
