//! Panel placement: which panels sit in which area, how the areas are split and how large they
//! are.
//!
//! The docking crate `egui_dock` needs egui 0.36, so the window has two resizable areas (left and
//! right of the viewports). Each area is cut into two groups, one above the other, and a group
//! shows its panels as tabs. Every panel can be moved to the other area from its tab menu.
//!
//! The layout is one for all modes and a mode never rewrites it: the panels that do not serve the
//! mode are left out when the areas are drawn (`mode_layouts.md`).

use serde::{Deserialize, Serialize};

use crate::request::Mode;

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
    /// The animations of the model and the controls that play them.
    Animations,
}

impl PanelId {
    /// Every panel.
    pub const ALL: [PanelId; 8] = [
        PanelId::Outliner,
        PanelId::Properties,
        PanelId::Uv,
        PanelId::Textures,
        PanelId::History,
        PanelId::Library,
        PanelId::Palette,
        PanelId::Animations,
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
            PanelId::Animations => "Animations",
        }
    }

    /// The panel is part of the mode; in any other it is never shown.
    pub fn serves(self, mode: Mode) -> bool {
        match self {
            PanelId::Properties => true,
            PanelId::Outliner => mode != Mode::Display,
            PanelId::Uv | PanelId::Textures => matches!(mode, Mode::Model | Mode::Paint),
            PanelId::Palette => matches!(mode, Mode::Voxel | Mode::Paint),
            PanelId::Library | PanelId::History => {
                matches!(mode, Mode::Model | Mode::Voxel | Mode::Paint)
            }
            PanelId::Animations => mode == Mode::Animate,
        }
    }
}

/// The tip of a menu or palette row for a panel that the mode does not use.
pub(crate) fn not_used_in(mode: Mode) -> String {
    format!("Not used in {}", mode.name())
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

    /// The panels that serve `mode`, in order, with their place in the list.
    pub fn shown(&self, mode: Mode) -> impl Iterator<Item = (usize, PanelId)> {
        self.panels
            .iter()
            .copied()
            .enumerate()
            .filter(move |(_, panel)| panel.serves(mode))
    }

    /// The shown panel: the active one when it serves `mode`, else the first that does.
    pub fn current(&self, mode: Mode) -> Option<PanelId> {
        self.panels
            .get(self.active)
            .copied()
            .filter(|panel| panel.serves(mode))
            .or_else(|| self.shown(mode).next().map(|(_, panel)| panel))
    }
}

/// One area: two groups, one above the other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoneState {
    /// The upper and the lower group; a group with no panel for the mode is not drawn and the
    /// other takes the area.
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
    /// The groups that hold a panel of `mode`, with their index.
    pub fn shown_groups(&self, mode: Mode) -> impl Iterator<Item = (usize, &Group)> {
        self.groups
            .iter()
            .enumerate()
            .filter(move |(_, g)| g.current(mode).is_some())
    }

    /// The panel is the shown tab of its group in `mode`, and the area is open.
    pub fn shows(&self, panel: PanelId, mode: Mode) -> bool {
        self.open && self.groups.iter().any(|g| g.current(mode) == Some(panel))
    }
}

/// Which arrangement a layout starts from; the data of the familiarity presets.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Arrangement {
    /// UV editor and textures on the left, properties above the outliner on the right, as in
    /// Blockbench.
    #[default]
    Blockbench,
    /// UV editor and textures on the left, outliner above properties on the right, as in Blender.
    Blender,
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
        Layout::arranged(Arrangement::default())
    }
}

impl Layout {
    /// The layout a familiarity preset starts with.
    pub fn arranged(arrangement: Arrangement) -> Self {
        use PanelId::{Animations, History, Library, Outliner, Palette, Properties, Textures, Uv};
        let zones = match arrangement {
            Arrangement::Blockbench | Arrangement::Blender => {
                // The properties are the larger panel and take the larger share of the area.
                let (groups, split) = if arrangement == Arrangement::Blockbench {
                    ([Group::of(&[Properties]), Group::of(&[Outliner])], 0.6)
                } else {
                    ([Group::of(&[Outliner]), Group::of(&[Properties])], 0.4)
                };
                [
                    ZoneState {
                        groups: [
                            Group::of(&[Uv, Library]),
                            Group::of(&[Textures, Palette, History, Animations]),
                        ],
                        size: 240.0,
                        split: 0.45,
                        ..ZoneState::default()
                    },
                    ZoneState {
                        groups,
                        size: 336.0,
                        split,
                        ..ZoneState::default()
                    },
                ]
            }
            Arrangement::Cubik => [
                ZoneState {
                    groups: [
                        Group::of(&[Properties]),
                        Group::of(&[Uv, Textures, Palette, History, Animations]),
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

    /// The panel is the shown tab of an open area in `mode`.
    pub fn is_shown(&self, panel: PanelId, mode: Mode) -> bool {
        self.zone_of(panel)
            .is_some_and(|z| self.zone(z).shows(panel, mode))
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

    /// Brings a panel to the front of its group without opening a closed area. A group that
    /// already shows the panel in `mode` is left alone, so the tab that the other modes read stays.
    /// Returns whether a tab moved.
    pub fn bring_forward(&mut self, panel: PanelId, mode: Mode) -> bool {
        let Some(zone) = self.zone_of(panel) else {
            return false;
        };
        let state = self.zone_mut(zone);
        if state.groups.iter().any(|g| g.current(mode) == Some(panel)) {
            return false;
        }
        Self::select(state, panel)
    }

    /// Makes the panel the active tab of its group. Returns whether that changed the tab.
    fn select(state: &mut ZoneState, panel: PanelId) -> bool {
        let mut moved = false;
        for group in &mut state.groups {
            if let Some(at) = group.panels.iter().position(|p| *p == panel) {
                moved = group.active != at;
                group.active = at;
            }
        }
        moved
    }

    /// Toggles a panel: hides its area when the panel is already the shown tab of an open area in
    /// `mode`, reveals it otherwise.
    pub fn toggle(&mut self, panel: PanelId, mode: Mode) {
        let Some(zone) = self.zone_of(panel) else {
            return;
        };
        if self.zone(zone).shows(panel, mode) {
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

    const ARRANGEMENTS: [Arrangement; 3] = [
        Arrangement::Blockbench,
        Arrangement::Blender,
        Arrangement::Cubik,
    ];

    #[test]
    fn every_arrangement_has_every_panel_once() {
        for arrangement in ARRANGEMENTS {
            let layout = Layout::arranged(arrangement);
            for panel in PanelId::ALL {
                assert_eq!(count(&layout, panel), 1, "{arrangement:?} {panel:?}");
            }
        }
    }

    #[test]
    fn the_blockbench_arrangement_puts_the_properties_above_the_outliner_on_the_right() {
        let layout = Layout::arranged(Arrangement::Blockbench);
        assert_eq!(layout, Layout::default());
        let right = layout.zone(Zone::Right);
        assert_eq!(
            right.groups[0].current(Mode::Model),
            Some(PanelId::Properties)
        );
        assert_eq!(
            right.groups[1].current(Mode::Model),
            Some(PanelId::Outliner)
        );
        assert!((right.split - 0.6).abs() < f32::EPSILON);
        let left = layout.zone(Zone::Left);
        assert_eq!(left.groups[0].panels[0], PanelId::Uv);
        assert_eq!(left.groups[1].panels[0], PanelId::Textures);
        assert!(left.groups[0].panels.contains(&PanelId::Library));
        assert!(left.groups[1].panels.contains(&PanelId::History));
        // The palette is one panel of its own, beside the textures.
        assert_eq!(left.groups[1].panels[1], PanelId::Palette);
    }

    #[test]
    fn the_blender_arrangement_keeps_the_outliner_on_the_right_above_the_properties() {
        let layout = Layout::arranged(Arrangement::Blender);
        let right = layout.zone(Zone::Right);
        assert_eq!(
            right.groups[0].current(Mode::Model),
            Some(PanelId::Outliner)
        );
        assert_eq!(
            right.groups[1].current(Mode::Model),
            Some(PanelId::Properties)
        );
        assert!((right.split - 0.4).abs() < f32::EPSILON);
        // The left area is the same as in the Blockbench arrangement.
        assert_eq!(
            layout.zone(Zone::Left),
            Layout::arranged(Arrangement::Blockbench).zone(Zone::Left)
        );
    }

    #[test]
    fn the_palette_is_in_the_left_area_of_every_arrangement_and_comes_forward() {
        for arrangement in ARRANGEMENTS {
            let mut layout = Layout::arranged(arrangement);
            assert_eq!(layout.zone_of(PanelId::Palette), Some(Zone::Left));
            assert!(!layout.is_shown(PanelId::Palette, Mode::Paint));
            assert!(layout.bring_forward(PanelId::Palette, Mode::Paint));
            assert!(layout.is_shown(PanelId::Palette, Mode::Paint));
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
        assert!(left.shows(PanelId::Outliner, Mode::Model));
        // The right area keeps the properties alone.
        assert_eq!(
            layout.zone(Zone::Right).groups[0].current(Mode::Model),
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
        let mode = Mode::Model;
        layout.toggle(PanelId::Properties, mode);
        assert!(!layout.zone(Zone::Right).open);
        layout.toggle(PanelId::Properties, mode);
        assert!(layout.zone(Zone::Right).open);
        // A background tab is revealed instead of hiding the area.
        layout.toggle(PanelId::History, mode);
        let left = layout.zone(Zone::Left);
        assert!(left.open && left.shows(PanelId::History, mode));
        layout.toggle(PanelId::Library, mode);
        assert!(layout.zone(Zone::Left).shows(PanelId::Library, mode));
        assert!(!layout.zone(Zone::Left).shows(PanelId::Uv, mode));
        assert!(layout.zone(Zone::Left).open);
    }

    #[test]
    fn bring_forward_does_not_open_a_closed_area() {
        let mut layout = Layout::default();
        layout.zone_mut(Zone::Left).open = false;
        layout.bring_forward(PanelId::Library, Mode::Model);
        assert!(!layout.zone(Zone::Left).open);
        layout.zone_mut(Zone::Left).open = true;
        assert!(layout.zone(Zone::Left).shows(PanelId::Library, Mode::Model));
    }

    #[test]
    fn the_modes_serve_the_panels_of_the_table() {
        use PanelId::{Animations, History, Library, Outliner, Palette, Properties, Textures, Uv};
        let table = [
            (
                Mode::Model,
                vec![Outliner, Properties, Uv, Textures, History, Library],
            ),
            (
                Mode::Voxel,
                vec![Outliner, Properties, Palette, History, Library],
            ),
            (
                Mode::Paint,
                vec![
                    Outliner, Properties, Uv, Textures, History, Library, Palette,
                ],
            ),
            (Mode::Display, vec![Properties]),
            (Mode::Animate, vec![Outliner, Properties, Animations]),
        ];
        for (mode, served) in table {
            for panel in PanelId::ALL {
                assert_eq!(
                    panel.serves(mode),
                    served.contains(&panel),
                    "{mode:?} {panel:?}"
                );
            }
        }
    }

    #[test]
    fn a_group_shows_its_active_panel_when_it_serves_the_mode_else_the_first_that_does() {
        let group = Group {
            panels: vec![
                PanelId::Textures,
                PanelId::Palette,
                PanelId::History,
                PanelId::Animations,
            ],
            active: 1,
        };
        assert_eq!(group.current(Mode::Paint), Some(PanelId::Palette));
        // Model has no palette: the first panel that serves it comes forward, nothing is stored.
        assert_eq!(group.current(Mode::Model), Some(PanelId::Textures));
        assert_eq!(group.current(Mode::Animate), Some(PanelId::Animations));
        assert_eq!(group.current(Mode::Display), None);
        assert_eq!(group.active, 1);
        let tabs: Vec<_> = group.shown(Mode::Model).collect();
        assert_eq!(tabs, [(0, PanelId::Textures), (2, PanelId::History)]);
        let stale = Group {
            active: 40,
            ..group
        };
        assert_eq!(stale.current(Mode::Voxel), Some(PanelId::Palette));
    }

    #[test]
    fn a_group_or_an_area_with_no_panel_for_the_mode_is_not_shown() {
        let layout = Layout::arranged(Arrangement::Blockbench);
        let left = layout.zone(Zone::Left);
        let right = layout.zone(Zone::Right);
        // Animate: the upper group of the left area has no panel for it, and the lower one has one.
        let groups: Vec<usize> = left.shown_groups(Mode::Animate).map(|(i, _)| i).collect();
        assert_eq!(groups, [1]);
        assert_eq!(
            left.groups[1].current(Mode::Animate),
            Some(PanelId::Animations)
        );
        // Display: nothing on the left, the properties alone on the right.
        assert_eq!(left.shown_groups(Mode::Display).count(), 0);
        assert!(!layout.is_shown(PanelId::Uv, Mode::Display));
        let groups: Vec<usize> = right.shown_groups(Mode::Display).map(|(i, _)| i).collect();
        assert_eq!(groups, [0]);
        // Model has the groups of both areas.
        assert_eq!(left.shown_groups(Mode::Model).count(), 2);
        assert_eq!(right.shown_groups(Mode::Model).count(), 2);
    }

    #[test]
    fn a_panel_of_another_mode_is_never_shown() {
        let mut layout = Layout::default();
        layout.reveal(PanelId::Animations);
        assert!(layout.is_shown(PanelId::Animations, Mode::Animate));
        assert!(!layout.is_shown(PanelId::Animations, Mode::Model));
        // The stored tab stays: the group shows the first panel that serves the mode.
        assert_eq!(
            layout.zone(Zone::Left).groups[1].current(Mode::Model),
            Some(PanelId::Textures)
        );
        assert_eq!(layout.zone(Zone::Left).groups[1].active, 3);
    }

    #[test]
    fn bring_forward_leaves_a_group_alone_that_already_shows_the_panel_in_the_mode() {
        let mut layout = Layout::default();
        // History is the stored tab of the lower left group; Animate shows Animations there anyway.
        layout.zones[0].groups[1].active = 2;
        assert!(!layout.bring_forward(PanelId::Animations, Mode::Animate));
        assert_eq!(layout.zone(Zone::Left).groups[1].active, 2);
        // Paint does not show the palette yet: it comes forward once.
        assert!(layout.bring_forward(PanelId::Palette, Mode::Paint));
        assert_eq!(layout.zone(Zone::Left).groups[1].active, 1);
        assert!(!layout.bring_forward(PanelId::Palette, Mode::Paint));
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
