//! The two areas beside the viewports: the groups of panels, the split between two groups and the
//! page of each panel.

use std::cell::Cell;

use cuboid_ui_kit::metrics::{TAB_ICON_WIDTH, gutter, height, space};
use cuboid_ui_kit::{
    Icon, IconButton, MenuItem, PanelFrame, Tokens, tab_button, tab_button_max, tab_icon, tab_width,
};
use egui::{
    Align, CursorIcon, Frame, Id, Layout, Panel, Rect, Sense, Stroke, UiBuilder, pos2, vec2,
};

use crate::animations;
use crate::colors;
use crate::common::Sink;
use crate::history;
use crate::layout::{Group, PanelId, SIDE_MAX, SIDE_MIN, SPLIT_MAX, SPLIT_MIN, Zone, ZoneState};
use crate::library::LibraryView;
use crate::model::DocView;
use crate::props_animate;
use crate::props_area::{self, PropsTab};
use crate::request::Mode;
use crate::textures;
use crate::validator::ValidatorView;

/// Widest an area may be in a window narrower than 1280 pt: the icon strip plus 260.
const NARROW_MAX: f32 = 296.0;
/// Widest the left area may be in a window narrower than 1024 pt.
const NARROWEST_LEFT_MAX: f32 = 200.0;
/// Window widths of the responsive rules (ui_v2_spec 11).
const WIDE_WINDOW: f32 = 1280.0;
const COMPACT_WINDOW: f32 = 1024.0;
/// Height of the grab bar between two groups.
const GRIP: f32 = space::XS;
/// A group is never shorter than this.
const GROUP_MIN: f32 = 96.0;
/// Names of the groups: each salts the ids of its group and names the frame of its tabs.
const GROUP_KEYS: [[&str; 2]; 2] = [["left_top", "left_bottom"], ["right_top", "right_bottom"]];

/// The widest an area may be in a window of this width, in points.
fn side_max(zone: Zone, window: f32) -> f32 {
    if window >= WIDE_WINDOW {
        SIDE_MAX
    } else if window >= COMPACT_WINDOW || zone == Zone::Right {
        NARROW_MAX
    } else {
        NARROWEST_LEFT_MAX
    }
}

fn panel_icon(panel: PanelId) -> Icon {
    match panel {
        PanelId::Outliner => Icon::Outliner,
        PanelId::Properties => Icon::Sliders,
        PanelId::Uv => Icon::FaceUv,
        PanelId::Textures => Icon::Texture,
        PanelId::History => Icon::History,
        PanelId::Library => Icon::Library,
        PanelId::Palette => Icon::Palette,
        PanelId::Animations => Icon::Film,
    }
}

/// The button that sends a panel to the other area.
fn move_button(ui: &mut egui::Ui, zone: Zone) -> bool {
    let (icon, tip) = match zone.other() {
        Zone::Left => (Icon::PanelLeft, "Move to the left area"),
        Zone::Right => (Icon::PanelRight, "Move to the right area"),
    };
    ui.add(IconButton::new(icon, tip)).clicked()
}

/// The tab buttons of a group in `mode`, with `current` shown; a right click offers to move the
/// panel to the other area. The room left of the move button is used in three steps: every title;
/// else the other tabs as icons and the shown tab with its title; else the shown title cut to
/// what is left, or an icon too where not even that fits. The row is clipped at the move button.
fn group_tabs(
    ui: &mut egui::Ui,
    group: &Group,
    mode: Mode,
    current: PanelId,
    chosen: &Cell<usize>,
    other: Zone,
    to_other: &impl Fn(PanelId),
) {
    let room = ui.available_width() - height::CONTROL - gutter::EDGE;
    let mut clip = ui.clip_rect();
    clip.max.x = clip.max.x.min(ui.max_rect().left() + room);
    ui.set_clip_rect(clip);
    let titles: f32 = group
        .shown(mode)
        .map(|(_, panel)| tab_width(ui, panel.title()))
        .sum();
    let icons = (group.shown(mode).count() - 1) as f32 * TAB_ICON_WIDTH;
    for (i, panel) in group.shown(mode) {
        let selected = panel == current;
        let title = panel.title();
        let response = if titles <= room {
            tab_button(ui, title, selected)
        } else if !selected || room - icons < TAB_ICON_WIDTH {
            tab_icon(ui, panel_icon(panel), title, selected)
        } else {
            tab_button_max(ui, title, true, room - icons)
        };
        if response.clicked() {
            chosen.set(i);
        }
        response.context_menu(|ui| {
            let label = format!("Move to the {} area", other.name());
            if ui.add(MenuItem::new(&label)).clicked() {
                to_other(panel);
                ui.close();
            }
        });
    }
}

impl super::Ui {
    /// One of the two areas beside the viewports.
    pub(super) fn zone(
        &mut self,
        ui: &mut egui::Ui,
        zone: Zone,
        doc: &DocView<'_>,
        library: Option<&LibraryView>,
        window: f32,
        sink: &mut Sink,
    ) {
        let tokens = Tokens::of(ui.ctx());
        let state = self.prefs.layout.zone(zone).clone();
        if !state.open || state.shown_groups(self.mode).next().is_none() {
            return;
        }
        let max = side_max(zone, window);
        let id = Id::unique(("cuboid_zone", zone.name(), self.layout_generation));
        let panel = match zone {
            Zone::Left => Panel::left(id),
            Zone::Right => Panel::right(id),
        };
        let moved = Cell::new(None);
        let shown = panel
            .resizable(true)
            .size_range(SIDE_MIN..=max)
            .default_size(state.size.min(max))
            .frame(Frame::new().fill(tokens.bg_panel))
            .show(ui, |ui| {
                self.groups(ui, zone, &state, doc, library, sink, &moved);
            });
        // Only a drag of the user is a new width: a narrow window clamps the shown one.
        let width = shown.response.rect.width();
        let dragging = ui.input(|i| i.pointer.any_down());
        if dragging && width.is_finite() && (width - state.size).abs() > 0.5 {
            self.prefs.layout.zone_mut(zone).size = width;
            self.size_dirty = true;
        }
        if let Some((panel_id, target)) = moved.get() {
            self.move_panel(panel_id, target);
        }
    }

    /// The groups of an area: one fills it, two are split by a bar the user can drag.
    #[allow(clippy::too_many_arguments)]
    fn groups(
        &mut self,
        ui: &mut egui::Ui,
        zone: Zone,
        state: &ZoneState,
        doc: &DocView<'_>,
        library: Option<&LibraryView>,
        sink: &mut Sink,
        moved: &Cell<Option<(PanelId, Zone)>>,
    ) {
        let area = ui.available_rect_before_wrap();
        let shown: Vec<usize> = state.shown_groups(self.mode).map(|(i, _)| i).collect();
        match shown.as_slice() {
            [only] => self.group(ui, zone, *only, area, state, doc, library, sink, moved),
            [top, bottom] => {
                let total = area.height();
                let upper = (total * state.split)
                    .clamp(GROUP_MIN, (total - GROUP_MIN - GRIP).max(GROUP_MIN));
                let top_rect = Rect::from_min_size(area.min, vec2(area.width(), upper));
                let grip = Rect::from_min_size(
                    pos2(area.left(), top_rect.bottom()),
                    vec2(area.width(), GRIP),
                );
                let bottom_rect = Rect::from_min_max(pos2(area.left(), grip.bottom()), area.max);
                let tokens = Tokens::of(ui.ctx());
                let response = ui.interact(
                    grip,
                    ui.make_persistent_id(("cuboid_split", zone.name())),
                    Sense::drag(),
                );
                let active = response.hovered() || response.dragged();
                if active {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                }
                if let (true, Some(pointer)) = (response.dragged(), response.interact_pointer_pos())
                {
                    let split =
                        ((pointer.y - area.top()) / total.max(1.0)).clamp(SPLIT_MIN, SPLIT_MAX);
                    self.prefs.layout.zone_mut(zone).split = split;
                    self.size_dirty = true;
                }
                let line = if active {
                    tokens.border_hover
                } else {
                    tokens.border
                };
                ui.painter()
                    .hline(grip.x_range(), grip.center().y, Stroke::new(1.0, line));
                self.group(ui, zone, *top, top_rect, state, doc, library, sink, moved);
                self.group(
                    ui,
                    zone,
                    *bottom,
                    bottom_rect,
                    state,
                    doc,
                    library,
                    sink,
                    moved,
                );
            }
            _ => {}
        }
        ui.advance_cursor_after_rect(area);
    }

    /// One group: its panels as tabs, the shown panel below them.
    #[allow(clippy::too_many_arguments)]
    fn group(
        &mut self,
        ui: &mut egui::Ui,
        zone: Zone,
        index: usize,
        rect: Rect,
        state: &ZoneState,
        doc: &DocView<'_>,
        library: Option<&LibraryView>,
        sink: &mut Sink,
        moved: &Cell<Option<(PanelId, Zone)>>,
    ) {
        let mode = self.mode;
        let group = &state.groups[index];
        let Some(current) = group.current(mode) else {
            return;
        };
        let key = GROUP_KEYS[zone as usize][index];
        // Each group has an id scope of its own: panels in two groups may hold widgets with the
        // same id salt, such as the scroll areas of the library and the palette.
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(key)
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        let ui = &mut child;
        let chosen = Cell::new(group.active);
        let other = zone.other();
        let to_other = |panel: PanelId| moved.set(Some((panel, other)));

        match (group.shown(mode).nth(1).is_some(), current) {
            (true, PanelId::Properties) => {
                PanelFrame::new(key)
                    .no_scroll()
                    .flush()
                    .tabs(|ui| group_tabs(ui, group, mode, current, &chosen, other, &to_other))
                    .show(
                        ui,
                        |_| {},
                        |ui| self.properties_page(ui, zone, doc, sink, &to_other),
                    );
            }
            (true, panel) => {
                PanelFrame::new(key)
                    .no_scroll()
                    .tabs(|ui| group_tabs(ui, group, mode, current, &chosen, other, &to_other))
                    .show(
                        ui,
                        |ui| {
                            if move_button(ui, zone) {
                                to_other(panel);
                            }
                        },
                        |ui| self.panel_body(ui, panel, doc, library, sink),
                    );
            }
            (false, PanelId::Properties) => self.properties_page(ui, zone, doc, sink, &to_other),
            (false, panel) => {
                PanelFrame::new(panel.title())
                    .icon(panel_icon(panel))
                    .no_scroll()
                    .show(
                        ui,
                        |ui| {
                            if move_button(ui, zone) {
                                to_other(panel);
                            }
                        },
                        |ui| self.panel_body(ui, panel, doc, library, sink),
                    );
            }
        }
        if chosen.get() != group.active {
            self.prefs.layout.zone_mut(zone).groups[index].active = chosen.get();
            self.prefs_dirty = true;
        }
    }

    /// The properties area: the strip of tabs and the page of the chosen one. The strip stands on
    /// the side that faces the viewports. The panel exists once, so its widgets have an id scope of
    /// their own that does not change with the frame around it.
    fn properties_page(
        &mut self,
        ui: &mut egui::Ui,
        zone: Zone,
        doc: &DocView<'_>,
        sink: &mut Sink,
        to_other: &impl Fn(PanelId),
    ) {
        let Self {
            mode,
            props,
            props_tab,
            images,
            validator,
            validator_view,
            project,
            prefs,
            timeline,
            animate_tab,
            ..
        } = self;
        let tab = *props_tab;
        let strip_at = match zone {
            Zone::Right => Align::Min,
            Zone::Left => Align::Max,
        };
        let badge = validator_view.as_ref().and_then(ValidatorView::badge);
        let scope = UiBuilder::new().scope_id(Id::unique("cuboid_properties_page"));
        let clicked = ui
            .scope_builder(scope, |ui| {
                props_area::show(
                    ui,
                    *mode,
                    tab,
                    strip_at,
                    badge,
                    |ui| {
                        if move_button(ui, zone) {
                            to_other(PanelId::Properties);
                        }
                    },
                    |ui| match tab {
                        PropsTab::Validator => {
                            validator.show(
                                ui,
                                validator_view.as_ref(),
                                doc.nodes,
                                doc.nodes_rev,
                                sink,
                            );
                        }
                        PropsTab::Animate => props_animate::show(
                            ui,
                            doc.animations,
                            doc.timeline,
                            timeline,
                            animate_tab,
                            prefs,
                            sink,
                        ),
                        tab => props.show_tab(ui, tab, doc, project, prefs, images, sink),
                    },
                )
            })
            .inner;
        if let Some(tab) = clicked {
            *props_tab = tab;
        }
    }

    /// The page of a panel other than the properties.
    fn panel_body(
        &mut self,
        ui: &mut egui::Ui,
        panel: PanelId,
        doc: &DocView<'_>,
        library: Option<&LibraryView>,
        sink: &mut Sink,
    ) {
        match panel {
            PanelId::Outliner => self.outliner.show(ui, doc, sink),
            PanelId::Uv => {
                let grid = self.prefs.uv_grid;
                self.uv.show(
                    ui,
                    doc,
                    doc.props,
                    &mut self.props.face,
                    &mut self.prefs.uv_grid,
                    &mut self.images,
                    sink,
                );
                self.prefs_dirty |= grid != self.prefs.uv_grid;
            }
            PanelId::Textures => textures::panel(ui, doc, &mut self.images, sink),
            PanelId::History => history::show(ui, doc.history, sink),
            PanelId::Library => self.library.show(ui, library, &mut self.images, sink),
            PanelId::Palette => colors::show(ui, doc.palette, sink),
            PanelId::Animations => {
                animations::show(ui, doc.animations, &mut self.animations_panel, sink);
            }
            PanelId::Properties => {}
        }
    }
}
