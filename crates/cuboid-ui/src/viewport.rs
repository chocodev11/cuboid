//! The viewport area: one to four views, their headers, and the translation of pointer input
//! into navigation and tool events.

use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    DragFieldOpts, Icon, MenuItem, Tokens, Type, chip, drag_field, menu_trigger, overflow_menu,
    text,
};
use egui::load::SizedTexture;
use egui::{
    Align, Image, Key, Label, Layout, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2, pos2, vec2,
};
use glam::Vec2 as GVec2;

use crate::common::Sink;
use crate::keymap::{Command, UiCommand};
use crate::labels::command_label;
use crate::layout::ViewLayout;
use crate::model::{AnimationsView, UiCx, ViewSlot};
use crate::prefs::{NavPreset, Prefs};
use crate::project_settings::{ProjectChange, ProjectView};
use crate::request::{
    Button, KeyMods, Mode, ToolOptions, UiRequest, ViewPreset, ViewportEvent, ViewportPlacement,
};
use crate::topbar::shading_items;

/// Gap between cells in points.
const GAP: f32 = 2.0;
/// The Cubik preset turns the view by 0.48 degrees per pixel across and 0.32 down
/// (app_behavior 5.5 d). The scene turns by 0.4 degrees per pixel, so the orbit events carry
/// these factors of the pointer travel.
pub const CUBIK_ORBIT_SCALE: [f32; 2] = [1.2, 0.8];
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
        (NavPreset::Blender, Button::Middle) => {
            if mods.shift {
                DragKind::Pan
            } else if mods.ctrl {
                DragKind::Zoom
            } else {
                DragKind::Orbit
            }
        }
        (NavPreset::Blender | NavPreset::Cubik, Button::Secondary) => DragKind::Tool,
        // The middle button pans; with Alt or Ctrl held it orbits.
        (NavPreset::Cubik, Button::Middle) => {
            if mods.alt || mods.ctrl {
                DragKind::Orbit
            } else {
                DragKind::Pan
            }
        }
        (NavPreset::Blender | NavPreset::Cubik, Button::Primary) => DragKind::Tool,
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
    /// The user picked a shading option in a header.
    pub project_changes: Vec<ProjectChange>,
    /// The user changed the brush or the mirror axes in a header.
    pub tool_options: Option<ToolOptions>,
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
pub(crate) fn wheel_notches(input: &egui::InputState) -> f32 {
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

pub(crate) fn gvec(v: Vec2) -> GVec2 {
    GVec2::new(v.x, v.y)
}

/// Splits a cell into its 28 pt header and the view below it.
fn split_header(cell: Rect) -> (Rect, Rect) {
    let head = Rect::from_min_size(cell.min, vec2(cell.width(), height::BAR.min(cell.height())));
    let view = Rect::from_min_max(pos2(cell.left(), head.bottom()), cell.max);
    (head, view)
}

/// What a drag in a view turns by: the Cubik preset has its own degrees per pixel.
fn orbit_delta(px: GVec2, prefs: &Prefs) -> GVec2 {
    let scale = if prefs.nav == NavPreset::Cubik {
        GVec2::from(CUBIK_ORBIT_SCALE)
    } else {
        GVec2::ONE
    };
    px * scale * prefs.orbit_speed
}

impl ViewportState {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        cx: &UiCx<'_>,
        prefs: &Prefs,
        project: &ProjectView,
        mode: Mode,
        sink: &mut Sink,
        placements: &mut Vec<ViewportPlacement>,
    ) -> ViewportResult {
        let tokens = Tokens::of(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let area = ui.available_rect_before_wrap();
        ui.advance_cursor_after_rect(area);
        let cells = cell_rects(area, prefs.view_layout);
        let rects: Vec<Rect> = cells.iter().map(|c| split_header(*c).1).collect();
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
                    Image::from_texture(SizedTexture::new(texture, rect.size())).paint_at(ui, rect);
                }
                None => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Waiting for the renderer",
                        Type::Body.font(),
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

        // The Cubik preset pans while Space is held, no button needed, and orbits with Alt or
        // Ctrl held too; not while another panel holds the pointer, such as the UV view's pan.
        let space_held =
            ui.input(|i| i.key_down(Key::Space)) && !ui.ctx().egui_wants_keyboard_input();
        if prefs.nav == NavPreset::Cubik
            && space_held
            && self.drag.is_none()
            && ui.ctx().dragged_id().is_none()
            && let Some((slot, _)) = hovered_now
        {
            let px = gvec(delta * ppp);
            if px != GVec2::ZERO {
                let event = if mods_k.alt || mods_k.ctrl {
                    ViewportEvent::Orbit {
                        slot,
                        delta: orbit_delta(px, prefs),
                    }
                } else {
                    ViewportEvent::Pan { slot, delta: px }
                };
                sink.push(UiRequest::Viewport(event));
            }
        }

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

        let preview = cx
            .doc
            .as_ref()
            .and_then(|doc| doc.animations)
            .and_then(AnimationsView::preview_name);
        for (index, cell) in cells.iter().copied().enumerate() {
            let head = split_header(cell).0;
            self.header(
                ui,
                index,
                head,
                (prefs, project, mode),
                preview,
                sink,
                &mut result,
            );
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
                    delta: orbit_delta(px, prefs),
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

    /// The 28 pt header of a cell: the view menu, the shading menu, the camera menu, the name of
    /// the animation whose pose the views show and, in the active view, the options of the voxel
    /// and paint tools.
    #[allow(clippy::too_many_arguments)]
    fn header(
        &mut self,
        ui: &mut Ui,
        index: usize,
        rect: Rect,
        (prefs, project, mode): (&Prefs, &ProjectView, Mode),
        preview: Option<&str>,
        sink: &mut Sink,
        result: &mut ViewportResult,
    ) {
        let tokens = Tokens::of(ui.ctx());
        let slot = prefs.cell_slot(index);
        ui.painter().rect_filled(rect, 0.0, tokens.bg_panel);
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, tokens.border),
        );
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(space::XS, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        bar.set_clip_rect(rect);
        bar.spacing_mut().item_spacing.x = space::XXS;
        let compact = rect.width() < COMPACT_HEADER;
        menu_trigger(&mut bar, None, slot.name(), false, |ui| {
            for s in ViewSlot::ALL {
                if ui.add(MenuItem::new(s.name()).checked(s == slot)).clicked() {
                    result.set_cell = Some((index, s));
                    ui.close();
                }
            }
            ui.separator();
            for layout in ViewLayout::ALL {
                let item = MenuItem::new(layout.name()).checked(layout == prefs.view_layout);
                if ui.add(item).clicked() {
                    result.set_layout = Some(layout);
                    ui.close();
                }
            }
        });
        menu_trigger(&mut bar, Some(Icon::Sun), "Shading", compact, |ui| {
            result.project_changes.extend(shading_items(ui, project));
        });
        menu_trigger(&mut bar, Some(Icon::Camera), "Camera", compact, |ui| {
            for preset in CAMERA_PRESETS {
                let label = command_label(Command::Ui(UiCommand::View(preset)));
                if ui.add(MenuItem::new(&label)).clicked() {
                    sink.push(UiRequest::Viewport(ViewportEvent::SetView { slot, preset }));
                    ui.close();
                }
            }
            ui.separator();
            if ui.add(MenuItem::new("Frame selection")).clicked() {
                sink.push(UiRequest::Viewport(ViewportEvent::Frame { slot }));
                ui.close();
            }
        });
        let active = slot == self.active || prefs.view_layout == ViewLayout::Single;
        if let Some(name) = preview.filter(|_| active) {
            let verb = if mode == Mode::Animate {
                "Animating"
            } else {
                "Previewing"
            };
            bar.add(Label::new(text(Type::Label, format!("{verb} {name}"))).selectable(false));
        }
        if !active || !matches!(mode, Mode::Voxel | Mode::Paint) {
            return;
        }
        // The options need the room the menus leave: with their words, without them, or not at all.
        let (with_words, bare) = if mode == Mode::Voxel {
            (OPTIONS_WORDS_VOXEL, OPTIONS_BARE_VOXEL)
        } else {
            (OPTIONS_WORDS_PAINT, OPTIONS_BARE_PAINT)
        };
        let free = rect.right() - bar.min_rect().right() - space::SM;
        if free < bare {
            // Not even the bare options fit: they move into a menu at the right end.
            bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
                overflow_menu(ui, "Tool options", |ui| {
                    let mut opts = prefs.tool_options;
                    ui.horizontal(|ui| {
                        ui.add(Label::new(text(Type::Label, "Brush")).selectable(false));
                        brush_field(ui, index, &mut opts);
                    });
                    if mode == Mode::Voxel {
                        ui.horizontal(|ui| {
                            ui.add(Label::new(text(Type::Label, "Mirror")).selectable(false));
                            mirror_chips(ui, &mut opts, [0, 1, 2]);
                        });
                    }
                    if opts != prefs.tool_options {
                        result.tool_options = Some(opts);
                    }
                });
            });
            return;
        }
        let words = free >= with_words;
        bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            let mut opts = prefs.tool_options;
            if mode == Mode::Voxel {
                mirror_chips(ui, &mut opts, [2, 1, 0]);
                if words {
                    ui.add(Label::new(text(Type::Label, "Mirror")).selectable(false));
                }
                ui.add_space(space::SM);
            }
            brush_field(ui, index, &mut opts);
            if words {
                ui.add(Label::new(text(Type::Label, "Brush")).selectable(false));
            }
            if opts != prefs.tool_options {
                result.tool_options = Some(opts);
            }
        });
    }
}

/// The brush size field of the voxel and paint tools.
fn brush_field(ui: &mut Ui, index: usize, opts: &mut ToolOptions) {
    let mut brush = f32::from(u16::try_from(opts.brush).unwrap_or(1));
    let field = DragFieldOpts {
        name: "Brush size",
        speed: 0.1,
        decimals: 0,
        range: 1.0..=64.0,
        snap: Some(1.0),
        width: Some(BRUSH_FIELD),
        ..DragFieldOpts::default()
    };
    if drag_field(ui, ("viewport_brush", index), &mut brush, &field).committed {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size = brush.round().max(1.0) as u32;
        opts.brush = size;
    }
}

/// The mirror axis chips of the voxel tools, in the order given (axis indices 0 to 2 for X to Z).
fn mirror_chips(ui: &mut Ui, opts: &mut ToolOptions, order: [usize; 3]) {
    for axis_index in order {
        let on = opts.mirror[axis_index];
        let toggled = chip(ui, ["X", "Y", "Z"][axis_index], on)
            .on_hover_text("Mirror across this axis")
            .clicked();
        if toggled {
            opts.mirror[axis_index] = !on;
        }
    }
}

/// The directions of the camera menu.
const CAMERA_PRESETS: [ViewPreset; 7] = [
    ViewPreset::Front,
    ViewPreset::Back,
    ViewPreset::Right,
    ViewPreset::Left,
    ViewPreset::Top,
    ViewPreset::Bottom,
    ViewPreset::ToggleProjection,
];

/// A header narrower than this shows the shading and camera menus as icons.
const COMPACT_HEADER: f32 = 360.0;

/// Room the tool options of a header need with and without their words, in points.
const OPTIONS_WORDS_VOXEL: f32 = 250.0;
const OPTIONS_BARE_VOXEL: f32 = 160.0;
const OPTIONS_WORDS_PAINT: f32 = 110.0;
const OPTIONS_BARE_PAINT: f32 = 70.0;

/// Width of the brush size field in a header.
const BRUSH_FIELD: f32 = 56.0;

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
