//! The viewport area: one to four views, their headers, and the translation of pointer input
//! into navigation and tool events.

use cuboid_ui_kit::{Icon, RADIUS, Tokens, icon_button};
use egui::{
    Align, Color32, Layout, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder,
    Vec2, pos2, vec2,
};
use glam::Vec2 as GVec2;

use crate::common::Sink;
use crate::layout::ViewLayout;
use crate::model::{UiCx, ViewSlot};
use crate::prefs::{NavPreset, Prefs};
use crate::request::{Button, KeyMods, UiRequest, ViewPreset, ViewportEvent, ViewportPlacement};

/// Gap between cells in points.
const GAP: f32 = 2.0;
/// Pointer travel in points before a camera drag counts as a drag rather than a click.
const CLICK_SLOP: f32 = 4.0;
/// Wheel points per notch for devices that report pixels.
const POINTS_PER_NOTCH: f32 = 50.0;

/// What a pointer drag started as.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DragKind {
    Orbit,
    Pan,
    Zoom,
    /// Passed on to the tools.
    Tool,
}

/// The action of pressing `button` with `mods` in a view, by camera preset. `tool_hot` says that
/// a plain left press would start a tool action (a gizmo handle, or a stroke over the model).
pub(crate) fn drag_kind(nav: NavPreset, button: Button, mods: KeyMods, tool_hot: bool) -> DragKind {
    match (nav, button) {
        // Three-button emulation for touchpads, in both presets.
        (_, Button::Primary) if mods.alt && mods.shift => DragKind::Pan,
        (_, Button::Primary) if mods.alt => DragKind::Orbit,
        (NavPreset::Blockbench, Button::Primary) => {
            // Shift and Ctrl box-select and toggle; a hot tool keeps the drag.
            if tool_hot || mods.shift || mods.ctrl {
                DragKind::Tool
            } else {
                DragKind::Orbit
            }
        }
        (NavPreset::Blender, Button::Primary) => DragKind::Tool,
        (NavPreset::Blender, Button::Middle) => {
            if mods.shift {
                DragKind::Pan
            } else if mods.ctrl {
                DragKind::Zoom
            } else {
                DragKind::Orbit
            }
        }
        (NavPreset::Blender, Button::Secondary) => DragKind::Tool,
        (NavPreset::Blockbench, Button::Secondary | Button::Middle) => {
            if mods.ctrl {
                DragKind::Zoom
            } else {
                DragKind::Pan
            }
        }
    }
}

struct ActiveDrag {
    slot: ViewSlot,
    cell: Rect,
    button: Button,
    kind: DragKind,
    start: Pos2,
    moved: bool,
}

/// What the viewport wants changed beyond requests.
#[derive(Default)]
pub(crate) struct ViewportResult {
    /// A cell asked to show another scene view.
    pub set_cell: Option<(usize, ViewSlot)>,
    /// The layout changed.
    pub set_layout: Option<ViewLayout>,
    /// The active view changed.
    pub activated: Option<ViewSlot>,
    /// The pointer is over a view.
    pub pointer_over_view: bool,
}

/// State between frames.
pub(crate) struct ViewportState {
    drag: Option<ActiveDrag>,
    hover: Option<(ViewSlot, GVec2)>,
    pub active: ViewSlot,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            drag: None,
            hover: None,
            active: ViewSlot::MAIN,
        }
    }
}

/// Wheel movement this frame in notches, positive away from the user.
fn wheel_notches(input: &egui::InputState) -> f32 {
    input
        .events
        .iter()
        .map(|event| match event {
            egui::Event::MouseWheel { unit, delta, .. } => match unit {
                egui::MouseWheelUnit::Line => delta.y,
                egui::MouseWheelUnit::Point => delta.y / POINTS_PER_NOTCH,
                egui::MouseWheelUnit::Page => delta.y * 10.0,
            },
            _ => 0.0,
        })
        .sum()
}

fn button_of(b: PointerButton) -> Option<Button> {
    match b {
        PointerButton::Primary => Some(Button::Primary),
        PointerButton::Secondary => Some(Button::Secondary),
        PointerButton::Middle => Some(Button::Middle),
        _ => None,
    }
}

const BUTTONS: [PointerButton; 3] = [
    PointerButton::Primary,
    PointerButton::Secondary,
    PointerButton::Middle,
];

/// The rectangles of the cells inside `area`.
pub(crate) fn cell_rects(area: Rect, layout: ViewLayout) -> Vec<Rect> {
    let half_w = (area.width() - GAP) / 2.0;
    let half_h = (area.height() - GAP) / 2.0;
    let cell = |col: f32, row: f32, w: f32, h: f32| {
        Rect::from_min_size(
            pos2(
                area.left() + col * (half_w + GAP),
                area.top() + row * (half_h + GAP),
            ),
            vec2(w, h),
        )
    };
    match layout {
        ViewLayout::Single => vec![area],
        ViewLayout::SplitH => vec![
            cell(0.0, 0.0, half_w, area.height()),
            cell(1.0, 0.0, half_w, area.height()),
        ],
        ViewLayout::SplitV => vec![
            cell(0.0, 0.0, area.width(), half_h),
            cell(0.0, 1.0, area.width(), half_h),
        ],
        ViewLayout::Quad => vec![
            cell(0.0, 0.0, half_w, half_h),
            cell(1.0, 0.0, half_w, half_h),
            cell(0.0, 1.0, half_w, half_h),
            cell(1.0, 1.0, half_w, half_h),
        ],
    }
}

/// Size in physical pixels to render a cell at, at least 1 x 1.
fn pixel_size(rect: Rect, ppp: f32) -> [u32; 2] {
    let px = |v: f32| -> u32 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = (v * ppp).round().clamp(1.0, 16384.0) as u32;
        n
    };
    [px(rect.width()), px(rect.height())]
}

fn mods_of(m: egui::Modifiers) -> KeyMods {
    KeyMods {
        ctrl: m.command,
        shift: m.shift,
        alt: m.alt,
    }
}

fn gvec(v: Vec2) -> GVec2 {
    GVec2::new(v.x, v.y)
}

impl ViewportState {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        prefs: &Prefs,
        sink: &mut Sink,
        placements: &mut Vec<ViewportPlacement>,
    ) -> ViewportResult {
        let tokens = Tokens::of(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let area = ui.available_rect_before_wrap();
        ui.advance_cursor_after_rect(area);
        let rects = cell_rects(area, prefs.view_layout);
        let mut result = ViewportResult::default();
        let (pointer, delta, scroll, mods, ctx_pressed, released) = ui.input(|i| {
            let pressed: Vec<PointerButton> = BUTTONS
                .into_iter()
                .filter(|b| i.pointer.button_pressed(*b))
                .collect();
            let released: Vec<PointerButton> = BUTTONS
                .into_iter()
                .filter(|b| i.pointer.button_released(*b))
                .collect();
            (
                i.pointer.latest_pos(),
                i.pointer.delta(),
                wheel_notches(i),
                i.modifiers,
                pressed,
                released,
            )
        });
        let mods_k = mods_of(mods);
        let mut hovered_now: Option<(ViewSlot, GVec2)> = None;

        for (index, rect) in rects.iter().copied().enumerate() {
            let slot = prefs.cell_slot(index);
            let id = ui.make_persistent_id(("viewport_cell", index));
            let resp = ui.interact(rect, id, Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let frame = cx.views.get(usize::from(slot.0)).copied().flatten();
            painter.rect_filled(rect, 0.0, tokens.bg_viewport);
            match frame.and_then(|f| f.texture) {
                Some(texture) => {
                    painter.image(
                        texture,
                        rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                None => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Waiting for the renderer",
                        egui::FontId::proportional(13.0),
                        tokens.text_weak,
                    );
                }
            }
            if let Some(band) = cx.band.filter(|b| b.slot == slot) {
                let a = rect.min + vec2(band.corners[0].x, band.corners[0].y) / ppp;
                let b = rect.min + vec2(band.corners[1].x, band.corners[1].y) / ppp;
                let r = Rect::from_two_pos(a, b);
                painter.rect(
                    r,
                    0.0,
                    tokens.accent.gamma_multiply(0.15),
                    Stroke::new(1.0, tokens.accent),
                    StrokeKind::Middle,
                );
            }
            let border = if slot == self.active && prefs.view_layout != ViewLayout::Single {
                Stroke::new(1.5, tokens.accent)
            } else {
                Stroke::new(1.0, tokens.border)
            };
            painter.rect_stroke(rect, 0.0, border, StrokeKind::Inside);

            placements.push(ViewportPlacement {
                slot,
                rect: [rect.left(), rect.top(), rect.width(), rect.height()],
                pixels_per_point: ppp,
                size_px: pixel_size(rect, ppp),
            });

            let inside = resp.contains_pointer();
            let local = |p: Pos2| gvec((p - rect.min) * ppp);

            if inside && self.drag.is_none() {
                if let Some(p) = pointer {
                    hovered_now = Some((slot, local(p)));
                }
                result.pointer_over_view = true;
                if scroll.abs() > f32::EPSILON {
                    let mut amount = scroll;
                    if prefs.invert_zoom {
                        amount = -amount;
                    }
                    if let Some(p) = pointer {
                        sink.push(UiRequest::Viewport(ViewportEvent::Zoom {
                            slot,
                            amount,
                            pos: local(p),
                        }));
                    }
                }
                for b in &ctx_pressed {
                    let (Some(button), Some(p)) = (button_of(*b), pointer) else {
                        continue;
                    };
                    if slot != self.active {
                        self.active = slot;
                        result.activated = Some(slot);
                        sink.push(UiRequest::Viewport(ViewportEvent::Activate { slot }));
                    }
                    let kind = drag_kind(prefs.nav, button, mods_k, cx.tool_hot);
                    self.drag = Some(ActiveDrag {
                        slot,
                        cell: rect,
                        button,
                        kind,
                        start: p,
                        moved: false,
                    });
                    if kind == DragKind::Tool {
                        sink.push(UiRequest::Viewport(ViewportEvent::Press {
                            slot,
                            button,
                            pos: local(p),
                            mods: mods_k,
                        }));
                    }
                    break;
                }
            }
        }

        self.drive_drag(pointer, delta, mods_k, &released, prefs, ppp, sink);

        // Hover events: only on change.
        match (hovered_now, self.hover) {
            (Some((slot, pos)), previous) if previous != Some((slot, pos)) => {
                sink.push(UiRequest::Viewport(ViewportEvent::Hover {
                    slot,
                    pos: Some(pos),
                }));
            }
            (None, Some((slot, _))) if self.drag.is_none() => {
                sink.push(UiRequest::Viewport(ViewportEvent::Hover {
                    slot,
                    pos: None,
                }));
            }
            _ => {}
        }
        if self.drag.is_none() {
            self.hover = hovered_now;
        }

        // Headers last, so their buttons sit on top of the cells.
        for (index, rect) in rects.iter().copied().enumerate() {
            self.header(ui, index, rect, prefs, sink, &mut result);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_drag(
        &mut self,
        pointer: Option<Pos2>,
        delta: Vec2,
        mods: KeyMods,
        released: &[PointerButton],
        prefs: &Prefs,
        ppp: f32,
        sink: &mut Sink,
    ) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        let slot = drag.slot;
        let local = |p: Pos2| gvec((p - drag.cell.min) * ppp);
        let pos = pointer.map_or(GVec2::ZERO, local);
        let px = gvec(delta * ppp);
        if let Some(p) = pointer
            && !drag.moved
            && (p - drag.start).length() > CLICK_SLOP
        {
            drag.moved = true;
        }
        // A navigation drag only starts after the slop, so a plain click still reaches the tools.
        if drag.moved && px != GVec2::ZERO {
            let event = match drag.kind {
                DragKind::Orbit => Some(ViewportEvent::Orbit {
                    slot,
                    delta: px * prefs.orbit_speed,
                }),
                DragKind::Pan => Some(ViewportEvent::Pan { slot, delta: px }),
                DragKind::Zoom => {
                    let mut amount = -px.y / 40.0;
                    if prefs.invert_zoom {
                        amount = -amount;
                    }
                    Some(ViewportEvent::Zoom { slot, amount, pos })
                }
                DragKind::Tool => Some(ViewportEvent::Drag {
                    slot,
                    button: drag.button,
                    pos,
                    delta: px,
                    mods,
                }),
            };
            if let Some(event) = event {
                sink.push(UiRequest::Viewport(event));
            }
        }
        let wanted = match drag.button {
            Button::Primary => PointerButton::Primary,
            Button::Secondary => PointerButton::Secondary,
            Button::Middle => PointerButton::Middle,
        };
        if released.contains(&wanted) {
            let (kind, button, moved, start) = (drag.kind, drag.button, drag.moved, drag.start);
            let cell = drag.cell;
            self.drag = None;
            match kind {
                DragKind::Tool => sink.push(UiRequest::Viewport(ViewportEvent::Release {
                    slot,
                    button,
                    pos,
                    mods,
                })),
                _ if !moved => {
                    // A click on a navigation button: give it to the tools as a plain click.
                    let start_local = gvec((start - cell.min) * ppp);
                    sink.push(UiRequest::Viewport(ViewportEvent::Press {
                        slot,
                        button,
                        pos: start_local,
                        mods,
                    }));
                    sink.push(UiRequest::Viewport(ViewportEvent::Release {
                        slot,
                        button,
                        pos: start_local,
                        mods,
                    }));
                }
                _ => {}
            }
        }
    }

    fn header(
        &mut self,
        ui: &mut Ui,
        index: usize,
        rect: Rect,
        prefs: &Prefs,
        sink: &mut Sink,
        result: &mut ViewportResult,
    ) {
        let tokens = Tokens::of(ui.ctx());
        let slot = prefs.cell_slot(index);
        let hovered = ui.rect_contains_pointer(rect);
        let bar = Rect::from_min_size(
            rect.min + vec2(8.0, 8.0),
            vec2((rect.width() - 16.0).max(0.0), 28.0),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(bar)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 4.0;
        child.spacing_mut().button_padding = vec2(8.0, 4.0);
        let chip = egui::Frame::new()
            .fill(tokens.bg_panel.gamma_multiply(0.85))
            .corner_radius(RADIUS)
            .inner_margin(egui::Margin::symmetric(4, 0));
        chip.show(&mut child, |ui| {
            ui.menu_button(slot.name(), |ui| {
                for s in ViewSlot::ALL {
                    if ui.selectable_label(s == slot, s.name()).clicked() {
                        result.set_cell = Some((index, s));
                        ui.close();
                    }
                }
                ui.separator();
                for layout in ViewLayout::ALL {
                    if ui
                        .selectable_label(layout == prefs.view_layout, layout.name())
                        .clicked()
                    {
                        result.set_layout = Some(layout);
                        ui.close();
                    }
                }
            });
        });
        if hovered {
            child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let chip = egui::Frame::new()
                    .fill(tokens.bg_panel.gamma_multiply(0.85))
                    .corner_radius(RADIUS)
                    .inner_margin(egui::Margin::symmetric(4, 0));
                chip.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if icon_button(ui, Icon::Frame, "Frame selection").clicked() {
                        sink.push(UiRequest::Viewport(ViewportEvent::Frame { slot }));
                    }
                    if icon_button(ui, Icon::Cube, "Perspective or orthographic").clicked() {
                        sink.push(UiRequest::Viewport(ViewportEvent::SetView {
                            slot,
                            preset: ViewPreset::ToggleProjection,
                        }));
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool, alt: bool) -> KeyMods {
        KeyMods { ctrl, shift, alt }
    }

    #[test]
    fn blender_style_navigation_uses_the_middle_button() {
        let n = NavPreset::Blender;
        let kind = |button, mods| drag_kind(n, button, mods, false);
        assert_eq!(
            kind(Button::Middle, mods(false, false, false)),
            DragKind::Orbit
        );
        assert_eq!(
            kind(Button::Middle, mods(false, true, false)),
            DragKind::Pan
        );
        assert_eq!(
            kind(Button::Middle, mods(true, false, false)),
            DragKind::Zoom
        );
        assert_eq!(
            kind(Button::Secondary, mods(false, false, false)),
            DragKind::Tool
        );
        assert_eq!(
            kind(Button::Primary, mods(false, false, false)),
            DragKind::Tool
        );
    }

    #[test]
    fn blockbench_style_orbits_with_the_left_button_on_empty_space() {
        let n = NavPreset::Blockbench;
        let plain = mods(false, false, false);
        assert_eq!(drag_kind(n, Button::Primary, plain, false), DragKind::Orbit);
        // On a gizmo handle (or a stroke over the model) the drag belongs to the tools.
        assert_eq!(drag_kind(n, Button::Primary, plain, true), DragKind::Tool);
        // Shift box-selects, Ctrl toggles.
        for m in [mods(false, true, false), mods(true, false, false)] {
            assert_eq!(drag_kind(n, Button::Primary, m, false), DragKind::Tool);
        }
        for button in [Button::Secondary, Button::Middle] {
            assert_eq!(drag_kind(n, button, plain, false), DragKind::Pan);
            assert_eq!(
                drag_kind(n, button, mods(true, false, false), false),
                DragKind::Zoom
            );
        }
    }

    #[test]
    fn alt_left_drag_orbits_in_both_presets() {
        for n in NavPreset::ALL {
            for hot in [false, true] {
                assert_eq!(
                    drag_kind(n, Button::Primary, mods(false, false, true), hot),
                    DragKind::Orbit
                );
                assert_eq!(
                    drag_kind(n, Button::Primary, mods(false, true, true), hot),
                    DragKind::Pan
                );
            }
        }
    }

    #[test]
    fn cells_tile_the_area_without_overlap() {
        let area = Rect::from_min_size(pos2(10.0, 20.0), vec2(802.0, 602.0));
        for layout in ViewLayout::ALL {
            let cells = cell_rects(area, layout);
            assert_eq!(cells.len(), layout.cells());
            for (i, a) in cells.iter().enumerate() {
                assert!(area.expand(0.01).contains_rect(*a), "{layout:?}");
                for b in &cells[i + 1..] {
                    assert!(!a.intersects(*b), "{layout:?}");
                }
            }
        }
    }

    #[test]
    fn pixel_size_is_at_least_one_and_scales_with_the_ratio() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.5, 0.0));
        assert_eq!(pixel_size(r, 2.0), [201, 1]);
        let huge = Rect::from_min_size(pos2(0.0, 0.0), vec2(1e9, 1e9));
        assert_eq!(pixel_size(huge, 1.0), [16384, 16384]);
    }
}
