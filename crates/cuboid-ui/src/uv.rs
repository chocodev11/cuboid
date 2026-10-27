//! The UV editor: the texture of the chosen face in a view that zooms and pans, with the pixel
//! grid and the faces of the primary element drawn over it, the numbers of the chosen face under
//! it, and two toolbars with the face picker, the UV actions and the view controls
//! (spec: uv editor v2).

use std::cmp::Ordering;
use std::ops::RangeInclusive;
use std::sync::Arc;

use cuboid_ui_kit::metrics::{height, radius, space};
use cuboid_ui_kit::{
    Button, DragFieldOpts, EmptyState, Icon, MenuItem, Tokens, Toolbar, Type, axis_field_min,
    drag_field, format_number, menu_trigger, paint_image, text,
};
use egui::{
    Align, Color32, CursorIcon, Key, Layout, Modifiers, Painter, PointerButton, Pos2, Rect,
    Response, Role, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, vec2,
};
use glam::Vec3;

use crate::common::{ImageCache, Sink};
use crate::model::{DocView, Face, FaceView, PropsView, UvOutline};
use crate::properties::rotation_picker;
use crate::request::{Edit, FaceChange};
use crate::viewport::{gvec, wheel_notches};

const HANDLE: f32 = 12.0;
/// The toolbar shows the name of the face from this width on.
const TOOLBAR_WORDS: f32 = 220.0;
/// The zoom range in points to a texel: 25 % to 3 200 %.
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 32.0;
/// What one wheel notch multiplies the zoom by.
const ZOOM_STEP: f32 = 1.15;
/// The grid shows from this many points to a texel, and has its full strength from `GRID_FULL`.
const GRID_MIN: f32 = 6.0;
const GRID_FULL: f32 = 10.0;
/// A stronger grid line stands at every multiple of this many texels.
const GRID_BLOCK: u32 = 16;
/// How much of the texture stays in the view when it is panned away, in points.
const KEEP: f32 = 32.0;
/// The room around a fitted texture, in points.
const FIT_MARGIN: f32 = space::XS;
/// How far outside a face the pointer still holds it, in points.
const HIT_SLOP: f32 = 2.0;
/// Pointer travel below which a press and release is a click, in points.
const CLICK_SLOP: f32 = 4.0;
/// The step of Shift, in texels.
const FINE: f32 = 0.25;
/// Texels an arrow key moves the faces; Shift moves `NUDGE_FAR` times as far.
const NUDGE: f32 = 1.0;
const NUDGE_FAR: f32 = 4.0;
/// The least height of the view, in points.
const VIEW_MIN: f32 = 48.0;
/// The corners of a UV array: which U (0 or 2) and which V (1 or 3) index of the array they move.
const CORNERS: [(usize, usize); 4] = [(0, 1), (2, 1), (0, 3), (2, 3)];

/// Where the texture is in the view: points to a texel, and the top left corner of the texture
/// seen from the top left corner of the view.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
struct View {
    scale: f32,
    offset: Vec2,
}

impl View {
    /// The texture as large as the view allows, centred.
    fn fit(area: Vec2, tex: Vec2) -> Self {
        let room = area - Vec2::splat(2.0 * FIT_MARGIN);
        let scale = (room.x / tex.x)
            .min(room.y / tex.y)
            .clamp(ZOOM_MIN, ZOOM_MAX);
        Self {
            scale,
            offset: (area - tex * scale) / 2.0,
        }
    }

    /// The view at `scale` with the texel under `about` (a point of the view) staying there.
    fn zoomed(self, scale: f32, about: Vec2) -> Self {
        let texel = (about - self.offset) / self.scale;
        Self {
            scale,
            offset: about - texel * scale,
        }
    }

    /// The view moved so that some of the texture stays in sight.
    fn clamped(self, area: Vec2, tex: Vec2) -> Self {
        let size = tex * self.scale;
        let keep = size.min(Vec2::splat(KEEP));
        Self {
            scale: self.scale,
            offset: self.offset.max(keep - size).min(area - keep),
        }
    }
}

/// The zoom one step on from `scale`, on a screen of `ppp` pixels to a point. A whole notch from
/// 100 % up lands on a whole number of pixels to a texel, so that texels keep one size; going up
/// from below, 100 % is a stop. A fraction of a notch (a touchpad) is a plain factor.
fn zoom_step(scale: f32, notches: f32, ppp: f32) -> f32 {
    // A pixel count that is whole up to rounding errors counts as whole.
    const SLACK: f32 = 1e-3;
    let target = scale * ZOOM_STEP.powf(notches);
    let pixels = scale * ppp;
    let next = if notches.abs() < 1.0 || (target < 1.0 && scale <= 1.0) {
        target
    } else if scale < 1.0 {
        1.0
    } else if notches > 0.0 {
        (target * ppp).round().max((pixels + SLACK).floor() + 1.0) / ppp
    } else {
        (target * ppp).round().min((pixels - SLACK).ceil() - 1.0) / ppp
    };
    next.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The texel boundaries 1 to `count - 1` of one axis that lie between the points `lo` and `hi`
/// of the view, for a texture whose edge is at `origin` and whose texels are `scale` points wide.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn grid_range(origin: f32, scale: f32, count: u32, (lo, hi): (f32, f32)) -> RangeInclusive<u32> {
    let first = ((lo - origin) / scale).ceil().max(1.0) as u32;
    let last = (((hi - origin) / scale).floor().max(0.0) as u32).min(count.saturating_sub(1));
    first..=last
}

/// `delta` rounded to the step of the modifiers: whole texels, quarters with Shift, none with Alt.
fn snapped(delta: Vec2, mods: Modifiers) -> Vec2 {
    if mods.alt {
        return delta;
    }
    let step = if mods.shift { FINE } else { 1.0 };
    (delta / step).round() * step
}

/// The lowest and the highest offset, in texels, that keeps all the rectangles on a texture of
/// size `tex`.
fn move_range(rects: impl Iterator<Item = [f32; 4]>, tex: Vec2) -> (Vec2, Vec2) {
    let open = (Vec2::splat(f32::NEG_INFINITY), Vec2::splat(f32::INFINITY));
    rects.fold(open, |(lowest, highest), uv| {
        let (a, b) = (vec2(uv[0], uv[1]), vec2(uv[2], uv[3]));
        (lowest.max(-a.min(b)), highest.min(tex - a.max(b)))
    })
}

/// `start` with one corner moved by `delta` texels and kept on a texture of size `tex`; the other
/// corner stays, so a flipped rectangle keeps its orientation.
fn drag_corner(start: [f32; 4], (u, v): (usize, usize), delta: Vec2, tex: Vec2) -> [f32; 4] {
    let mut uv = start;
    uv[u] = (uv[u] + delta.x).clamp(0.0, tex.x);
    uv[v] = (uv[v] + delta.y).clamp(0.0, tex.y);
    uv
}

/// The rectangle `[u0, v0, u1, v1]` of the numbers X, Y, width and height, with the flips of `uv`.
fn with_numbers(uv: [f32; 4], [x, y, w, h]: [f32; 4]) -> [f32; 4] {
    let (u0, u1) = if uv[2] < uv[0] {
        (x + w, x)
    } else {
        (x, x + w)
    };
    let (v0, v1) = if uv[3] < uv[1] {
        (y + h, y)
    } else {
        (y, y + h)
    };
    [u0, v0, u1, v1]
}

/// Texels to a block along the two axes of a face of an element of this size (16 units to a
/// block): across and down as the face is seen, so a UV turned a quarter reads the other way
/// round. `None` for a face with no length on an axis.
fn density(size: Vec3, face: Face, uv: [f32; 4], rotation: i32) -> Option<[f32; 2]> {
    let (across, down) = match face {
        Face::Up | Face::Down => (size.x, size.z),
        Face::North | Face::South => (size.x, size.y),
        Face::West | Face::East => (size.z, size.y),
    };
    if across.abs() < 1e-3 || down.abs() < 1e-3 {
        return None;
    }
    let (du, dv) = ((uv[2] - uv[0]).abs(), (uv[3] - uv[1]).abs());
    let (texels_across, texels_down) = if rotation.rem_euclid(180) == 90 {
        (dv, du)
    } else {
        (du, dv)
    };
    Some([
        texels_across * 16.0 / across.abs(),
        texels_down * 16.0 / down.abs(),
    ])
}

/// Whether the two densities of a face differ, so that the texture is stretched on it.
fn stretched([across, down]: [f32; 2]) -> bool {
    (across - down).abs() > 0.01 * across.max(down)
}

/// A set with this one face.
fn only(face: Face) -> [bool; 6] {
    let mut set = [false; 6];
    set[face.index()] = true;
    set
}

/// The pixel rectangle of a UV array on screen.
fn screen_rect(origin: Pos2, scale: f32, uv: [f32; 4]) -> Rect {
    let a = origin + vec2(uv[0], uv[1]) * scale;
    let b = origin + vec2(uv[2], uv[3]) * scale;
    Rect::from_two_pos(a, b)
}

/// What the pointer holds since it went down in the view.
enum Drag {
    /// The picture moves with the pointer.
    Pan,
    /// The chosen faces move together.
    Move {
        faces: Vec<Face>,
        /// Where the pointer went down.
        from: Pos2,
        /// Where the first corner of the first face was when the pointer went down, in texels:
        /// what the faces have moved since is read from the document.
        start: Vec2,
        /// The offsets, in texels, that keep the faces on the texture.
        range: (Vec2, Vec2),
        /// The move since the pointer went down that was last asked for.
        sent: Vec2,
        /// The pointer went further than a click.
        travelled: bool,
        /// The keys held when the pointer went down on a chosen face: a click, not a drag,
        /// decides the choice on release.
        click: Option<Modifiers>,
    },
    /// A corner of a face moves.
    Corner {
        face: Face,
        corner: (usize, usize),
        /// The rectangle when the pointer went down.
        start: [f32; 4],
        /// The size of the texture, in texels.
        tex: Vec2,
        /// The move of the corner, in texels, last sent.
        sent: Vec2,
        from: Pos2,
    },
}

/// What the view shows in one frame.
struct Scene<'a> {
    faces: &'a [FaceView; 6],
    /// The screen rectangle of each face drawn in the view.
    rects: [Option<Rect>; 6],
    /// Where the top left corner of the texture is on the screen.
    origin: Pos2,
    /// Points to a texel.
    scale: f32,
    /// The size of the texture in texels.
    texels: [u32; 2],
    tex: Vec2,
}

impl Scene<'_> {
    fn point(&self, u: f32, v: f32) -> Pos2 {
        self.origin + vec2(u, v) * self.scale
    }
}

#[derive(Default)]
pub(crate) struct UvState {
    /// The faces chosen in the view; the primary one, which the editor's `Face` names, is among
    /// them.
    chosen: [bool; 6],
    /// The texture the view is for; another texture starts fitted.
    shown: Option<(Arc<str>, [u32; 2])>,
    view: View,
    /// The user zoomed or panned: the view stays as it is instead of following the panel.
    free: bool,
    /// The faces of the other elements are outlined too.
    pub all_uvs: bool,
    drag: Option<Drag>,
    /// The pointer is over the editor or holds something in it: + and - zoom the view and the
    /// arrow keys move the faces, not the 3D views and the timeline.
    pub hovered: bool,
}

impl UvState {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        doc: &DocView<'_>,
        props: Option<&PropsView>,
        primary: &mut Face,
        grid: &mut bool,
        images: &mut ImageCache,
        sink: &mut Sink,
    ) {
        let tokens = Tokens::of(ui.ctx());
        self.end_stale_drag(ui, sink);
        let Some((props, faces)) = props.and_then(|p| Some((p, p.faces.as_ref()?))) else {
            self.hovered = false;
            EmptyState::new(Icon::FaceUv, "No element selected")
                .body("Select an element to edit its UV.")
                .show(ui);
            return;
        };
        self.keep_chosen(faces, *primary);
        let editor = ui.max_rect();
        self.face_bar(ui, primary, sink);
        let face = *primary;
        let state = &faces[face.index()];
        let Some(texture) = doc.textures.iter().find(|t| t.label == state.texture) else {
            self.hovered = false;
            ui.label(
                text(Type::Body, "The face uses a texture that does not exist.").color(tokens.warn),
            );
            return;
        };
        let [tw, th] = texture.size;
        if tw == 0 || th == 0 {
            self.hovered = false;
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let tex = vec2(tw as f32, th as f32);
        let (_, bar_rect) = ui.allocate_space(vec2(ui.available_width(), height::BAR));
        ui.add_space(space::XS);

        // The numbers stand under the view and are added from the bottom up; the view takes
        // what is left. Its id is its own: a drag must survive a change in the widgets above.
        let view_id = ui.make_persistent_id("uv_view");
        let (rect, caption_rect) = ui
            .with_layout(Layout::bottom_up(Align::Min), |ui| {
                density_line(ui, props.size, face, state);
                if let Some(change) = rotation_picker(ui, state.rotation) {
                    sink.edit(Edit::FaceOfPrimary { face, change });
                }
                rect_fields(ui, face, state.uv, tex, sink);
                let (_, caption) =
                    ui.allocate_space(vec2(ui.available_width(), Type::Caption.line_height()));
                let size = vec2(ui.available_width(), ui.available_height().max(VIEW_MIN));
                (ui.allocate_space(size).1, caption)
            })
            .inner;
        let resp = ui.interact(rect, view_id, Sense::drag());
        resp.widget_info(|| WidgetInfo::labeled(Role::Image, true, "UV view"));
        self.hovered = ui.ctx().rect_contains_pointer(ui.layer_id(), editor) || self.drag.is_some();

        let key = (state.texture.clone(), texture.size);
        if self.shown.as_ref() != Some(&key) {
            self.shown = Some(key);
            self.free = false;
        }
        let hover = ui
            .input(|i| i.pointer.latest_pos())
            .filter(|p| rect.contains(*p));
        self.update_view(ui, &resp, rect, tex, hover);
        self.nudge(ui, sink);
        self.view_bar(ui, bar_rect, rect.size(), tex, grid);

        let origin = rect.min + self.view.offset;
        let scale = self.view.scale;
        let scene = Scene {
            faces,
            rects: std::array::from_fn(|i| {
                let drawn =
                    self.chosen[i] || (faces[i].enabled && faces[i].texture == state.texture);
                drawn.then(|| screen_rect(origin, scale, faces[i].uv))
            }),
            origin,
            scale,
            texels: texture.size,
            tex,
        };
        if self.handle_pointer(ui, &resp, &scene, primary, sink) {
            ui.ctx().request_repaint();
        }
        if let Some(icon) = self.cursor(ui, &scene, *primary, hover) {
            ui.ctx().set_cursor_icon(icon);
        }

        let painter = ui.painter_at(rect);
        let image = texture.image.as_ref().and_then(|image| {
            let key = format!("texture:{}", texture.label);
            images.texture(ui.ctx(), &key, image)
        });
        self.paint_picture(&painter, &tokens, &scene, image, *grid);
        let others = if self.all_uvs { doc.uv_outlines } else { &[] };
        let others = others.iter().filter(|o| o.texture == state.texture);
        self.paint_faces(&painter, &tokens, &scene, others);

        caption(ui, caption_rect, &scene, hover, self.view.scale);
    }

    /// The faces that are chosen and drawn: those of the texture of the primary face, and the
    /// primary face itself.
    fn keep_chosen(&mut self, faces: &[FaceView; 6], primary: Face) {
        if !self.chosen[primary.index()] {
            self.chosen = only(primary);
        }
        let texture = &faces[primary.index()].texture;
        for face in Face::ALL {
            let other = &faces[face.index()];
            self.chosen[face.index()] &=
                face == primary || (other.enabled && other.texture == *texture);
        }
    }

    fn chosen_faces(&self) -> Vec<Face> {
        Face::ALL
            .into_iter()
            .filter(|f| self.chosen[f.index()])
            .collect()
    }

    /// The face picker and the actions on the UV of the chosen faces.
    fn face_bar(&mut self, ui: &mut Ui, primary: &mut Face, sink: &mut Sink) {
        let current = *primary;
        // A narrow area keeps the face picker as an icon.
        let narrow = ui.available_width() < TOOLBAR_WORDS;
        let label = match self.chosen.iter().filter(|chosen| **chosen).count() {
            0 | 1 => current.name().to_owned(),
            n => format!("{} +{}", current.name(), n - 1),
        };
        Toolbar::horizontal().show(ui, |bar| {
            menu_trigger(bar.ui(), Some(Icon::FaceUv), &label, narrow, |ui| {
                for other in Face::ALL {
                    if ui
                        .add(MenuItem::new(other.name()).checked(other == current))
                        .clicked()
                    {
                        *primary = other;
                        self.chosen = only(other);
                        ui.close();
                    }
                }
            });
            bar.separator();
            for (icon, tip, change) in [
                (Icon::Wand, "Apply auto UV", FaceChange::AutoUv),
                (
                    Icon::RotateRight,
                    "Rotate the UV a quarter turn",
                    FaceChange::RotateUv,
                ),
                (
                    Icon::Mirror,
                    "Mirror the UV left to right",
                    FaceChange::FlipU,
                ),
                (
                    Icon::FlipVertical,
                    "Flip the UV top to bottom",
                    FaceChange::FlipV,
                ),
            ] {
                if bar.button(icon, tip).clicked() {
                    sink.edit(Edit::Faces {
                        faces: self.chosen_faces(),
                        change,
                    });
                }
            }
        });
        ui.add_space(space::XS);
    }

    /// The view controls: All UVs, the grid, Fit and the zoom. Drawn after the view has taken
    /// this frame's input, so the zoom it shows is the current one.
    fn view_bar(&mut self, ui: &mut Ui, bar_rect: Rect, area: Vec2, tex: Vec2, grid: &mut bool) {
        let readout = format!("{}%", format_number(self.view.scale * 100.0, 0));
        let mut fit = false;
        let mut outlines = false;
        ui.scope_builder(UiBuilder::new().max_rect(bar_rect), |ui| {
            Toolbar::horizontal().show(ui, |bar| {
                if bar
                    .toggle(Icon::Layers, self.all_uvs, "Show all UVs on this texture")
                    .clicked()
                {
                    self.all_uvs = !self.all_uvs;
                    outlines = true;
                }
                if bar
                    .toggle(Icon::Grid, *grid, "Show the pixel grid")
                    .clicked()
                {
                    *grid = !*grid;
                }
                bar.separator();
                fit |= bar
                    .button(Icon::Frame, "Fit the texture to the view")
                    .clicked();
                fit |= bar
                    .ui()
                    .add(Button::new(&readout).ghost())
                    .on_hover_text("Fit the texture to the view")
                    .clicked();
            });
        });
        if fit {
            self.free = false;
            self.view = View::fit(area, tex);
        }
        // The numbers drawn this frame still show the old zoom, and the app lists the faces of the
        // other elements only from the next frame on.
        if fit || outlines {
            ui.ctx().request_repaint();
        }
    }

    /// Takes the wheel, + and - and the dragging of the picture into the view.
    fn update_view(
        &mut self,
        ui: &Ui,
        resp: &Response,
        rect: Rect,
        tex: Vec2,
        hover: Option<Pos2>,
    ) {
        let area = rect.size();
        let typing = ui.ctx().egui_wants_keyboard_input();
        if resp.drag_started() {
            let pans = ui.input(|i| {
                i.pointer.button_down(PointerButton::Middle)
                    || i.pointer.button_down(PointerButton::Secondary)
                    || (i.key_down(Key::Space) && !typing)
            });
            self.drag = pans.then_some(Drag::Pan);
        }
        if !self.free {
            self.view = View::fit(area, tex);
        }
        // The wheel zooms over the view only; + and - anywhere over the editor.
        let notches = ui.input_mut(|i| {
            let mut notches = if hover.is_some() {
                wheel_notches(i)
            } else {
                0.0
            };
            if self.hovered && !typing {
                if i.consume_key(Modifiers::NONE, Key::Plus)
                    || i.consume_key(Modifiers::NONE, Key::Equals)
                {
                    notches += 1.0;
                }
                if i.consume_key(Modifiers::NONE, Key::Minus) {
                    notches -= 1.0;
                }
            }
            notches
        });
        if notches.abs() > f32::EPSILON {
            let about = hover.map_or(area / 2.0, |p| p - rect.min);
            let scale = zoom_step(self.view.scale, notches, ui.pixels_per_point());
            self.view = self.view.zoomed(scale, about);
            self.free = true;
        }
        if matches!(self.drag, Some(Drag::Pan)) && resp.dragged() {
            self.view.offset += resp.drag_delta();
            self.free = true;
        }
        if self.free {
            self.view = self.view.clamped(area, tex);
        }
    }

    /// The arrow keys move the chosen faces a texel, Shift four, while the pointer is over the
    /// editor and holds nothing in it: during a drag the pointer decides.
    fn nudge(&self, ui: &Ui, sink: &mut Sink) {
        if !self.hovered || self.drag.is_some() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let step = ui.input_mut(|i| {
            let mut step = Vec2::ZERO;
            for (mods, texels) in [(Modifiers::SHIFT, NUDGE_FAR), (Modifiers::NONE, NUDGE)] {
                for (key, dir) in [
                    (Key::ArrowLeft, vec2(-1.0, 0.0)),
                    (Key::ArrowRight, vec2(1.0, 0.0)),
                    (Key::ArrowUp, vec2(0.0, -1.0)),
                    (Key::ArrowDown, vec2(0.0, 1.0)),
                ] {
                    if i.consume_key(mods, key) {
                        step += dir * texels;
                    }
                }
            }
            step
        });
        if step != Vec2::ZERO {
            sink.edit(Edit::MoveFaces {
                faces: self.chosen_faces(),
                delta: gvec(step),
                commit: true,
            });
        }
    }

    /// The face under `pos`: the smaller of faces that overlap, the chosen ones first when
    /// `chosen_first`, as a press grabs them.
    fn face_at(&self, scene: &Scene<'_>, pos: Pos2, chosen_first: bool) -> Option<Face> {
        let key = |face: &Face| {
            let area = scene.rects[face.index()].map_or(0.0, |r| r.area());
            (chosen_first && !self.chosen[face.index()], area)
        };
        Face::ALL
            .into_iter()
            .filter(|f| scene.rects[f.index()].is_some_and(|r| r.expand(HIT_SLOP).contains(pos)))
            .min_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(Ordering::Equal))
    }

    /// Whether the faces have room for corner handles: a face smaller than a handle has none.
    fn has_handles(scene: &Scene<'_>, face: Face) -> bool {
        scene.rects[face.index()].is_some_and(|r| r.width().min(r.height()) >= HANDLE)
    }

    /// The corner of a chosen face under `pos`, the primary face's first.
    fn corner_at(
        &self,
        scene: &Scene<'_>,
        primary: Face,
        pos: Pos2,
    ) -> Option<(Face, (usize, usize))> {
        std::iter::once(primary)
            .chain(Face::ALL.into_iter().filter(|f| *f != primary))
            .filter(|f| self.chosen[f.index()] && Self::has_handles(scene, *f))
            .find_map(|face| {
                let uv = scene.faces[face.index()].uv;
                CORNERS.into_iter().find_map(|(u, v)| {
                    let handle =
                        Rect::from_center_size(scene.point(uv[u], uv[v]), Vec2::splat(HANDLE));
                    handle.contains(pos).then_some((face, (u, v)))
                })
            })
    }

    /// Starts, runs and ends what the primary button does in the view. Returns whether the choice
    /// of faces changed.
    fn handle_pointer(
        &mut self,
        ui: &Ui,
        resp: &Response,
        scene: &Scene<'_>,
        primary: &mut Face,
        sink: &mut Sink,
    ) -> bool {
        let (pointer, mods) = ui.input(|i| (i.pointer.latest_pos(), i.modifiers));
        let Some(pos) = pointer else {
            return false;
        };
        let mut changed = false;
        if resp.drag_started() && self.drag.is_none() {
            changed = self.press(scene, pos, mods, primary);
        } else if resp.dragged() || resp.drag_stopped() {
            changed = self.drive(scene, pos, mods, resp.drag_stopped(), primary, sink);
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        changed
    }

    /// The button went down: a corner is grabbed, or a face is grabbed. A face that is not chosen
    /// is chosen (Shift or Ctrl adds it); a chosen one grabs the whole choice, and what a click
    /// without a drag does to the choice is decided on release. A press on empty texture keeps
    /// the choice.
    fn press(&mut self, scene: &Scene<'_>, pos: Pos2, mods: Modifiers, primary: &mut Face) -> bool {
        if let Some((face, corner)) = self.corner_at(scene, *primary, pos) {
            let start = scene.faces[face.index()].uv;
            self.drag = Some(Drag::Corner {
                face,
                corner,
                start,
                tex: scene.tex,
                sent: Vec2::ZERO,
                from: pos,
            });
            return false;
        }
        let Some(hit) = self.face_at(scene, pos, true) else {
            return false;
        };
        let before = (self.chosen, *primary);
        let click = self.chosen[hit.index()].then_some(mods);
        if click.is_some() {
            *primary = hit;
        } else {
            self.pick(hit, mods, primary);
        }
        let faces = self.chosen_faces();
        let range = move_range(faces.iter().map(|f| scene.faces[f.index()].uv), scene.tex);
        let [u, v, ..] = scene.faces[faces[0].index()].uv;
        self.drag = Some(Drag::Move {
            faces,
            from: pos,
            start: vec2(u, v),
            range,
            sent: Vec2::ZERO,
            travelled: false,
            click,
        });
        (self.chosen, *primary) != before
    }

    /// Chooses `face`; with Shift or Ctrl it is added to the choice, or taken out when it is
    /// chosen already.
    fn pick(&mut self, face: Face, mods: Modifiers, primary: &mut Face) {
        if mods.shift || mods.command {
            // One face always stays chosen.
            let others = (0..6).any(|i| self.chosen[i] && i != face.index());
            if !self.chosen[face.index()] {
                self.chosen[face.index()] = true;
            } else if others {
                self.chosen[face.index()] = false;
            }
        } else {
            self.chosen = only(face);
        }
        if self.chosen[face.index()] {
            *primary = face;
        } else if !self.chosen[primary.index()] {
            *primary = self.chosen_faces()[0];
        }
    }

    /// The pointer moved with the button down, or let go: asks for the next step of the move of
    /// the faces or sends the new rectangle of the corner. Returns whether the choice of faces
    /// changed.
    fn drive(
        &mut self,
        scene: &Scene<'_>,
        pos: Pos2,
        mods: Modifiers,
        stopped: bool,
        primary: &mut Face,
        sink: &mut Sink,
    ) -> bool {
        match self.drag.as_mut() {
            Some(Drag::Move {
                faces,
                from,
                start,
                range,
                sent,
                travelled,
                click,
            }) => {
                *travelled |= (pos - *from).length() > CLICK_SLOP;
                if !*travelled {
                    // A click: it chooses the smallest face under the pointer, not the one a drag
                    // would have grabbed.
                    let Some(mods) = click.filter(|_| stopped) else {
                        return false;
                    };
                    let Some(face) = self.face_at(scene, pos, false) else {
                        return false;
                    };
                    let before = (self.chosen, *primary);
                    self.pick(face, mods, primary);
                    return (self.chosen, *primary) != before;
                }
                let total = snapped((pos - *from) / scene.scale, mods)
                    .max(range.0)
                    .min(range.1);
                // Nothing new to ask, and nothing to end.
                if total == *sent && (!stopped || total == Vec2::ZERO) {
                    return false;
                }
                // The step is worked out against where the faces are in the document, so a step
                // the core cut short is made up for and nothing drifts.
                let [u, v, ..] = scene.faces[faces[0].index()].uv;
                let step = if total == *sent {
                    Vec2::ZERO
                } else {
                    total - (vec2(u, v) - *start)
                };
                sink.edit(Edit::MoveFaces {
                    faces: faces.clone(),
                    delta: gvec(step),
                    commit: stopped,
                });
                *sent = total;
            }
            Some(Drag::Corner {
                face,
                corner,
                start,
                tex,
                sent,
                from,
            }) => {
                let delta = snapped((pos - *from) / scene.scale, mods);
                let uv = drag_corner(*start, *corner, delta, *tex);
                if stopped {
                    if delta != Vec2::ZERO || *sent != Vec2::ZERO {
                        sink.edit(Edit::FaceOfPrimary {
                            face: *face,
                            change: FaceChange::Uv { uv, commit: true },
                        });
                    }
                } else if delta != *sent {
                    sink.edit(Edit::FaceOfPrimary {
                        face: *face,
                        change: FaceChange::Uv { uv, commit: false },
                    });
                    *sent = delta;
                }
            }
            _ => {}
        }
        false
    }

    /// Ends a drag whose button came up while the editor was not drawn, such as when the panel was
    /// closed or the selection cleared in the middle of it, so that its undo step ends too.
    fn end_stale_drag(&mut self, ui: &Ui, sink: &mut Sink) {
        if ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
            return;
        }
        match self.drag.take() {
            Some(Drag::Move { faces, sent, .. }) if sent != Vec2::ZERO => {
                sink.edit(Edit::MoveFaces {
                    faces,
                    delta: gvec(Vec2::ZERO),
                    commit: true,
                });
            }
            Some(Drag::Corner {
                face,
                corner,
                start,
                tex,
                sent,
                ..
            }) if sent != Vec2::ZERO => {
                sink.edit(Edit::FaceOfPrimary {
                    face,
                    change: FaceChange::Uv {
                        uv: drag_corner(start, corner, sent, tex),
                        commit: true,
                    },
                });
            }
            _ => {}
        }
    }

    /// The cursor for what the pointer is over or holds.
    fn cursor(
        &self,
        ui: &Ui,
        scene: &Scene<'_>,
        primary: Face,
        hover: Option<Pos2>,
    ) -> Option<CursorIcon> {
        match &self.drag {
            Some(Drag::Pan) => return Some(CursorIcon::Grabbing),
            Some(Drag::Move { .. }) => return Some(CursorIcon::Move),
            Some(Drag::Corner { .. }) => return Some(CursorIcon::Crosshair),
            None => {}
        }
        let pos = hover?;
        if ui.input(|i| i.key_down(Key::Space)) {
            return Some(CursorIcon::Grab);
        }
        if self.corner_at(scene, primary, pos).is_some() {
            return Some(CursorIcon::Crosshair);
        }
        let face = self.face_at(scene, pos, true)?;
        Some(if self.chosen[face.index()] {
            CursorIcon::Move
        } else {
            CursorIcon::PointingHand
        })
    }

    /// The well, the picture, the pixel grid and the edge of the texture.
    fn paint_picture(
        &self,
        painter: &Painter,
        tokens: &Tokens,
        scene: &Scene<'_>,
        image: Option<egui::TextureId>,
        grid: bool,
    ) {
        let tex_rect = Rect::from_min_size(scene.origin, scene.tex * scene.scale);
        painter.rect_filled(painter.clip_rect(), radius::CONTROL, tokens.bg_viewport);
        painter.rect_filled(tex_rect, 0.0, tokens.bg_input);
        if let Some(id) = image {
            paint_image(painter, id, tex_rect);
        }
        if grid && scene.scale >= GRID_MIN {
            paint_grid(painter, tokens, scene, tex_rect);
        }
        painter.rect_stroke(
            tex_rect,
            0.0,
            Stroke::new(1.0, tokens.border_strong),
            StrokeKind::Outside,
        );
    }

    /// The faces of the other elements, faint, then every face of the primary element with its
    /// letter, the chosen ones filled and with handles.
    fn paint_faces<'a>(
        &self,
        painter: &Painter,
        tokens: &Tokens,
        scene: &Scene<'_>,
        others: impl Iterator<Item = &'a UvOutline>,
    ) {
        for other in others {
            let rect = screen_rect(scene.origin, scene.scale, other.uv);
            if rect.intersects(painter.clip_rect()) {
                paint_outline(painter, tokens, rect, 0.5);
            }
        }
        for face in Face::ALL {
            if let (Some(rect), false) = (scene.rects[face.index()], self.chosen[face.index()]) {
                paint_outline(painter, tokens, rect, 0.9);
            }
        }
        for face in Face::ALL {
            let Some(rect) = scene.rects[face.index()].filter(|_| self.chosen[face.index()]) else {
                continue;
            };
            painter.rect_filled(rect, 0.0, tokens.accent.gamma_multiply(0.25));
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(2.0, tokens.accent),
                StrokeKind::Middle,
            );
        }
        for face in Face::ALL {
            if let Some(rect) = scene.rects[face.index()] {
                paint_letter(painter, tokens, rect, face, self.chosen[face.index()]);
            }
        }
        for face in Face::ALL {
            if !self.chosen[face.index()] || !Self::has_handles(scene, face) {
                continue;
            }
            let uv = scene.faces[face.index()].uv;
            for (u, v) in CORNERS {
                let handle = Rect::from_center_size(scene.point(uv[u], uv[v]), Vec2::splat(HANDLE));
                painter.rect(
                    handle.shrink(space::XXS),
                    radius::CONTROL,
                    tokens.bg_panel,
                    Stroke::new(1.5, tokens.accent),
                    StrokeKind::Middle,
                );
            }
        }
    }
}

/// The outline of a face that is not chosen: a dark line and a light one side by side, so that it
/// reads on any texture in both themes; `strength` is how opaque they are.
fn paint_outline(painter: &Painter, tokens: &Tokens, rect: Rect, strength: f32) {
    let line = |color: Color32| Stroke::new(1.0, color.gamma_multiply(strength));
    painter.rect_stroke(rect, 0.0, line(tokens.text_strong), StrokeKind::Inside);
    painter.rect_stroke(rect, 0.0, line(tokens.bg_panel), StrokeKind::Outside);
}

/// The texel lines inside the visible part of the texture, stronger at every block of texels.
/// They fade in as the texels grow.
fn paint_grid(painter: &Painter, tokens: &Tokens, scene: &Scene<'_>, tex_rect: Rect) {
    let fade = ((scene.scale - GRID_MIN) / (GRID_FULL - GRID_MIN)).clamp(0.0, 1.0);
    let weak = Stroke::new(1.0, tokens.text_weak.gamma_multiply(0.12 + 0.18 * fade));
    let strong = Stroke::new(1.0, tokens.text_weak.gamma_multiply(0.3 + 0.3 * fade));
    let seen = tex_rect.intersect(painter.clip_rect());
    let [across, down] = scene.texels;
    for k in grid_range(
        tex_rect.left(),
        scene.scale,
        across,
        (seen.left(), seen.right()),
    ) {
        let x = tex_rect.left() + k as f32 * scene.scale;
        let stroke = if k % GRID_BLOCK == 0 { strong } else { weak };
        painter.vline(x, seen.y_range(), stroke);
    }
    for k in grid_range(
        tex_rect.top(),
        scene.scale,
        down,
        (seen.top(), seen.bottom()),
    ) {
        let y = tex_rect.top() + k as f32 * scene.scale;
        let stroke = if k % GRID_BLOCK == 0 { strong } else { weak };
        painter.hline(seen.x_range(), y, stroke);
    }
}

/// The first letter of a face (N, E, S, W, U, D) in the top left corner of its rectangle, when
/// the rectangle is large enough to carry it.
fn paint_letter(painter: &Painter, tokens: &Tokens, rect: Rect, face: Face, chosen: bool) {
    let (back, ink) = if chosen {
        (tokens.accent, tokens.on_accent)
    } else {
        (tokens.bg_panel.gamma_multiply(0.85), tokens.text_strong)
    };
    let galley = painter.layout_no_wrap(face.name()[..1].to_owned(), Type::Caption.font(), ink);
    let size = galley.size() + Vec2::splat(2.0 * space::XXS);
    let room = Vec2::splat(2.0 * space::XXS);
    if rect.width() < size.x + room.x || rect.height() < size.y + room.y {
        return;
    }
    let label = Rect::from_min_size(rect.min + Vec2::splat(space::XXS), size);
    painter.rect_filled(label, radius::CONTROL, back);
    painter.galley(label.min + Vec2::splat(space::XXS), galley, ink);
}

/// The line under the picture: the texel under the pointer, and the zoom.
fn caption(ui: &mut Ui, rect: Rect, scene: &Scene<'_>, hover: Option<Pos2>, zoom: f32) {
    let texel = hover
        .map(|p| ((p - scene.origin) / scene.scale).floor())
        .filter(|t| t.x >= 0.0 && t.y >= 0.0 && t.x < scene.tex.x && t.y < scene.tex.y);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            if let Some(t) = texel {
                let (x, y) = (format_number(t.x, 0), format_number(t.y, 0));
                ui.label(text(Type::Caption, format!("Texel {x}, {y}")));
            }
        },
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            let zoom = format_number(zoom * 100.0, 0);
            ui.label(text(Type::Caption, format!("{zoom}%")));
        },
    );
}

/// X, Y, width and height of the primary face in texels, in a row, or two rows when the area is
/// narrow. Added from the bottom up like the rest under the view.
fn rect_fields(ui: &mut Ui, face: Face, uv: [f32; 4], tex: Vec2, sink: &mut Sink) {
    const NAMES: [(&str, &str); 4] = [
        ("X", "UV X"),
        ("Y", "UV Y"),
        ("W", "UV width"),
        ("H", "UV height"),
    ];
    let tokens = Tokens::of(ui.ctx());
    let (x, y) = (uv[0].min(uv[2]), uv[1].min(uv[3]));
    let (w, h) = ((uv[2] - uv[0]).abs(), (uv[3] - uv[1]).abs());
    let mut values = [x, y, w, h];
    let ranges = [
        0.0..=(tex.x - w).max(0.0),
        0.0..=(tex.y - h).max(0.0),
        0.0..=(tex.x - x).max(0.0),
        0.0..=(tex.y - y).max(0.0),
    ];
    let gap = space::XS;
    let wide = ui.available_width() >= 4.0 * axis_field_min(ui) + 3.0 * gap;
    let (columns, rows): (f32, &[(usize, usize)]) = if wide {
        (4.0, &[(0, 4)])
    } else {
        (2.0, &[(2, 4), (0, 2)])
    };
    let width = (ui.available_width() - (columns - 1.0) * gap) / columns;
    let (mut changed, mut committed) = (false, false);
    for &(from, to) in rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for i in from..to {
                let (caption, name) = NAMES[i];
                let opts = DragFieldOpts {
                    name,
                    speed: 0.25,
                    decimals: 2,
                    range: ranges[i].clone(),
                    snap: Some(1.0),
                    label: Some((caption, tokens.text_weak)),
                    width: Some(width),
                    ..DragFieldOpts::default()
                };
                let r = drag_field(ui, ("uv_rect", i), &mut values[i], &opts);
                changed |= r.changed;
                committed |= r.committed;
            }
        });
    }
    if changed || committed {
        sink.edit(Edit::FaceOfPrimary {
            face,
            change: FaceChange::Uv {
                uv: with_numbers(uv, values),
                commit: committed,
            },
        });
    }
}

/// The texel density of the face in texels to a block on each axis; the warning colour when the
/// two differ. Added from the bottom up like the rest under the view.
fn density_line(ui: &mut Ui, size: Vec3, face: Face, state: &FaceView) {
    let tokens = Tokens::of(ui.ctx());
    let Some(texels) = density(size, face, state.uv, state.rotation) else {
        ui.label(text(Type::Caption, "Density: this face has no area"));
        return;
    };
    let [across, down] = texels;
    let words = format!(
        "Density {} x {} texels per block",
        format_number(across, 1),
        format_number(down, 1)
    );
    if stretched(texels) {
        ui.label(text(Type::Caption, words).color(tokens.warn))
            .on_hover_text("The two axes differ: the texture is stretched on this face.");
    } else {
        ui.label(text(Type::Caption, words));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn a_corner_drag_moves_one_corner_and_keeps_flips() {
        // A flipped rectangle (u2 < u1) keeps its orientation.
        let uv = drag_corner(
            [8.0, 0.0, 4.0, 4.0],
            (2, 3),
            vec2(-1.0, 2.0),
            vec2(16.0, 16.0),
        );
        assert!(close(uv, [8.0, 0.0, 3.0, 6.0]), "{uv:?}");
    }

    #[test]
    fn a_corner_stays_on_the_texture() {
        let tex = vec2(16.0, 8.0);
        let uv = drag_corner([2.0, 1.0, 6.0, 5.0], (2, 3), vec2(40.0, 40.0), tex);
        assert!(close(uv, [2.0, 1.0, 16.0, 8.0]), "{uv:?}");
        let uv = drag_corner([2.0, 1.0, 6.0, 5.0], (0, 1), vec2(-40.0, -40.0), tex);
        assert!(close(uv, [0.0, 0.0, 6.0, 5.0]), "{uv:?}");
    }

    #[test]
    fn the_snap_is_a_texel_a_quarter_with_shift_and_none_with_alt() {
        let delta = vec2(1.4, -2.6);
        assert_eq!(snapped(delta, Modifiers::NONE), vec2(1.0, -3.0));
        assert_eq!(snapped(delta, Modifiers::SHIFT), vec2(1.5, -2.5));
        assert_eq!(snapped(delta, Modifiers::ALT), delta);
    }

    #[test]
    fn faces_may_move_as_far_as_the_one_nearest_the_edge_allows() {
        let tex = vec2(16.0, 16.0);
        let rects = [[2.0, 4.0, 6.0, 8.0], [12.0, 0.0, 10.0, 3.0]];
        // The first can go 2 left and 10 right, 4 up and 8 down; the second, flipped, 10 left and
        // 4 right, none up and 13 down.
        let (lowest, highest) = move_range(rects.into_iter(), tex);
        assert_eq!((lowest, highest), (vec2(-2.0, 0.0), vec2(4.0, 8.0)));
    }

    #[test]
    fn the_numbers_keep_the_flips_of_the_rectangle() {
        assert!(close(
            with_numbers([1.0, 2.0, 5.0, 4.0], [3.0, 2.0, 4.0, 2.0]),
            [3.0, 2.0, 7.0, 4.0]
        ));
        assert!(close(
            with_numbers([5.0, 4.0, 1.0, 2.0], [3.0, 2.0, 4.0, 2.0]),
            [7.0, 4.0, 3.0, 2.0]
        ));
    }

    #[test]
    fn the_density_counts_texels_to_a_block_along_each_axis_of_the_face() {
        // A 16 by 8 by 4 element: a south face is 16 wide and 8 high, an east face 4 and 8.
        let size = Vec3::new(16.0, 8.0, 4.0);
        let at = |face, uv, rotation| density(size, face, uv, rotation).expect("an area");
        let south = at(Face::South, [0.0, 0.0, 16.0, 8.0], 0);
        let east = at(Face::East, [0.0, 0.0, 4.0, 4.0], 0);
        assert!(close(south, [16.0, 16.0]) && close(east, [16.0, 8.0]));
        assert!(!stretched(south) && stretched(east));
        // Turned a quarter, the width of the rectangle runs down the face.
        let turned = at(Face::East, [0.0, 0.0, 8.0, 4.0], 90);
        let upright = at(Face::East, [0.0, 0.0, 8.0, 4.0], 0);
        assert!(close(turned, [16.0, 16.0]) && close(upright, [32.0, 8.0]));
        // A flat element has no area on its thin faces.
        assert!(density(Vec3::new(16.0, 0.0, 16.0), Face::North, [0.0; 4], 0).is_none());
    }

    #[test]
    fn a_wheel_notch_from_100_percent_lands_on_whole_pixels_to_a_texel() {
        assert!((zoom_step(1.0, 1.0, 1.0) - 2.0).abs() < 1e-4);
        assert!((zoom_step(2.0, -1.0, 1.0) - 1.0).abs() < 1e-4);
        // A fitted zoom with a fraction goes to the next whole number either way.
        assert!((zoom_step(14.375, 1.0, 1.0) - 17.0).abs() < 1e-4);
        assert!((zoom_step(14.375, -1.0, 1.0) - 13.0).abs() < 1e-4);
        // Below 100 % it is a plain factor, with 100 % as a stop on the way up.
        assert!((zoom_step(0.5, 1.0, 1.0) - 0.575).abs() < 1e-4);
        assert!((zoom_step(0.9, 1.0, 1.0) - 1.0).abs() < 1e-4);
        assert!((zoom_step(1.0, -1.0, 1.0) - 1.0 / ZOOM_STEP).abs() < 1e-4);
        // A touchpad's fraction of a notch does not jump to a whole number.
        assert!((zoom_step(4.0, 0.1, 1.0) - 4.0 * ZOOM_STEP.powf(0.1)).abs() < 1e-4);
        // The range ends at 25 % and 3 200 %.
        assert!((zoom_step(0.26, -3.0, 1.0) - ZOOM_MIN).abs() < 1e-4);
        assert!((zoom_step(31.0, 3.0, 1.0) - ZOOM_MAX).abs() < 1e-4);
    }

    #[test]
    fn at_a_scaled_screen_a_wheel_notch_still_lands_on_whole_pixels() {
        // At 150 % a zoom of 9 points is 13.5 pixels: a notch lands on 16 pixels up, 12 down.
        let ppp = 1.5;
        assert!((zoom_step(9.0, 1.0, ppp) * ppp - 16.0).abs() < 1e-3);
        assert!((zoom_step(9.0, -1.0, ppp) * ppp - 12.0).abs() < 1e-3);
        // Every stop on the way up from 100 % and back is a whole number of pixels.
        let mut scale = 1.0;
        for _ in 0..8 {
            scale = zoom_step(scale, 1.0, ppp);
            let pixels = scale * ppp;
            assert!((pixels - pixels.round()).abs() < 1e-3, "{pixels}");
        }
        for _ in 0..8 {
            scale = zoom_step(scale, -1.0, ppp);
            let pixels = scale * ppp;
            assert!(
                (pixels - pixels.round()).abs() < 1e-3 || scale < 1.0,
                "{pixels}"
            );
        }
    }

    #[test]
    fn zooming_keeps_the_texel_under_the_pointer_in_place() {
        let tex = vec2(64.0, 32.0);
        let view = View::fit(vec2(200.0, 150.0), tex);
        let about = vec2(130.0, 60.0);
        let texel = (about - view.offset) / view.scale;
        let zoomed = view.zoomed(view.scale * 3.0, about);
        let again = (about - zoomed.offset) / zoomed.scale;
        assert!((texel - again).length() < 1e-3, "{texel:?} {again:?}");
    }

    #[test]
    fn a_fitted_texture_fills_the_view_and_is_centred() {
        let view = View::fit(vec2(208.0, 108.0), vec2(16.0, 16.0));
        // 100 points high after the margin: 6.25 to a texel.
        assert!((view.scale - 6.25).abs() < 1e-4, "{view:?}");
        let size = vec2(16.0, 16.0) * view.scale;
        assert!((view.offset.x - (208.0 - size.x) / 2.0).abs() < 1e-3);
        assert!((view.offset.y - (108.0 - size.y) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn the_picture_may_leave_the_view_but_never_entirely() {
        let tex = vec2(64.0, 64.0);
        let area = vec2(200.0, 150.0);
        let view = View {
            scale: 4.0,
            offset: vec2(-5000.0, 9000.0),
        };
        let kept = view.clamped(area, tex);
        let size = tex * kept.scale;
        // KEEP points of the texture stay in the view on each axis.
        assert!((kept.offset.x + size.x - KEEP).abs() < 1e-3, "{kept:?}");
        assert!((area.y - kept.offset.y - KEEP).abs() < 1e-3, "{kept:?}");
    }

    #[test]
    fn grid_lines_are_the_visible_texel_boundaries_inside_the_texture() {
        // A 32 texel wide texture at 8 points to a texel with its edge at 100: 100 to 356.
        let all = grid_range(100.0, 8.0, 32, (0.0, 1000.0));
        assert_eq!(all, 1..=31);
        // Only the lines in sight: from 132 to 180 are the boundaries 4 to 10.
        let some = grid_range(100.0, 8.0, 32, (132.0, 180.0));
        assert_eq!(some, 4..=10);
        // A view that does not meet the texture has none.
        assert!(grid_range(100.0, 8.0, 32, (400.0, 500.0)).is_empty());
        assert!(grid_range(100.0, 8.0, 32, (0.0, 50.0)).is_empty());
        // The edges of the texture are not grid lines.
        assert_eq!(grid_range(0.0, 8.0, 1, (0.0, 100.0)).count(), 0);
    }

    #[test]
    fn the_smaller_of_overlapping_faces_is_hit_and_the_chosen_ones_come_first() {
        let faces: [FaceView; 6] = std::array::from_fn(|_| FaceView {
            enabled: true,
            texture: Arc::from("t"),
            uv: [0.0; 4],
            tint: -1,
            rotation: 0,
            cull: None,
        });
        let rect = |size: f32| Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(size)));
        let mut rects = [None; 6];
        rects[Face::Up.index()] = rect(40.0);
        rects[Face::North.index()] = rect(10.0);
        rects[Face::East.index()] = rect(20.0);
        let scene = Scene {
            faces: &faces,
            rects,
            origin: Pos2::ZERO,
            scale: 1.0,
            texels: [64, 64],
            tex: vec2(64.0, 64.0),
        };
        let mut state = UvState::default();
        let pos = Pos2::new(5.0, 5.0);
        assert_eq!(state.face_at(&scene, pos, true), Some(Face::North));
        state.chosen = only(Face::East);
        assert_eq!(state.face_at(&scene, pos, true), Some(Face::East));
        // A click takes the smallest face whether it is chosen or not.
        assert_eq!(state.face_at(&scene, pos, false), Some(Face::North));
        // Outside every face, but within the slop of the large one.
        let edge = Pos2::new(41.0, 5.0);
        assert_eq!(state.face_at(&scene, edge, true), Some(Face::Up));
        assert_eq!(state.face_at(&scene, Pos2::new(50.0, 5.0), true), None);
    }
}
