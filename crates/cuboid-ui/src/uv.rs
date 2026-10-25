//! The UV editor: the texture of the chosen face with the face rectangles drawn over it, and a
//! toolbar with the face picker and the UV actions.

use cuboid_ui_kit::metrics::{radius, space};
use cuboid_ui_kit::{
    EmptyState, Icon, MenuItem, Tokens, Toolbar, Type, menu_trigger, paint_image, text,
};
use egui::{Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};

use crate::common::{ImageCache, Sink};
use crate::model::{DocView, Face, PropsView};
use crate::request::{Edit, FaceChange};

const HANDLE: f32 = 12.0;
/// The toolbar shows the name of the face from this width on.
const TOOLBAR_WORDS: f32 = 220.0;

/// Which part of the rectangle a drag holds.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Grab {
    Body,
    /// Corner: which U (0 or 2) and which V (1 or 3) index of the UV array move.
    Corner(usize, usize),
}

#[derive(Default)]
pub(crate) struct UvState {
    start: [f32; 4],
    total: Vec2,
}

/// The pixel rectangle of a UV array on screen.
fn screen_rect(origin: Pos2, scale: f32, uv: [f32; 4]) -> Rect {
    let a = origin + vec2(uv[0], uv[1]) * scale;
    let b = origin + vec2(uv[2], uv[3]) * scale;
    Rect::from_two_pos(a, b)
}

/// Moves the grabbed part of `start` by `delta` texels, rounded to whole texels.
pub(crate) fn drag_uv(
    start: [f32; 4],
    grab_corner: Option<(usize, usize)>,
    delta: Vec2,
) -> [f32; 4] {
    let (dx, dy) = (delta.x.round(), delta.y.round());
    let mut uv = start;
    match grab_corner {
        None => {
            uv[0] += dx;
            uv[2] += dx;
            uv[1] += dy;
            uv[3] += dy;
        }
        Some((u, v)) => {
            uv[u] += dx;
            uv[v] += dy;
        }
    }
    uv
}

impl UvState {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        doc: &DocView<'_>,
        props: Option<&PropsView>,
        face: &mut Face,
        images: &mut ImageCache,
        sink: &mut Sink,
    ) {
        let tokens = Tokens::of(ui.ctx());
        let Some(faces) = props.and_then(|p| p.faces.as_ref()) else {
            EmptyState::new(Icon::FaceUv, "No element selected")
                .body("Select an element to edit its UV.")
                .show(ui);
            return;
        };
        toolbar(ui, face, sink);
        let face = *face;
        let state = &faces[face.index()];
        let Some(texture) = doc.textures.iter().find(|t| t.label == state.texture) else {
            ui.label(
                text(Type::Body, "The face uses a texture that does not exist.").color(tokens.warn),
            );
            return;
        };
        let [tw, th] = texture.size;
        if tw == 0 || th == 0 {
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let (tw, th) = (tw as f32, th as f32);

        // The picture is as large as the panel allows, in the texture's aspect, and centred;
        // the line under it stays with it.
        let readout = Type::Caption.line_height() + space::XS;
        let avail = ui.available_size();
        let free = vec2(avail.x, (avail.y - readout).max(1.0));
        let scale = (free.x / tw).min(free.y / th).max(0.25);
        let size = vec2(tw * scale, th * scale);
        ui.add_space(((free.y - size.y) / 2.0).max(0.0));
        let rect = ui
            .vertical_centered(|ui| ui.allocate_exact_size(size, Sense::hover()).0)
            .inner;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, radius::CONTROL, tokens.bg_input);
        if let Some(image) = &texture.image {
            let key = format!("texture:{}", texture.label);
            if let Some(id) = images.texture(ui.ctx(), &key, image) {
                paint_image(&painter, id, rect);
            }
        }
        painter.rect_stroke(
            rect,
            radius::CONTROL,
            Stroke::new(1.0, tokens.border_strong),
            StrokeKind::Outside,
        );

        // The other faces, dim.
        for (i, other) in faces.iter().enumerate() {
            if i == face.index() || !other.enabled || other.texture != state.texture {
                continue;
            }
            painter.rect_stroke(
                screen_rect(rect.min, scale, other.uv),
                0.0,
                Stroke::new(1.0, tokens.text_weak.gamma_multiply(0.7)),
                StrokeKind::Middle,
            );
        }

        let uv = state.uv;
        let face_rect = screen_rect(rect.min, scale, uv);
        painter.rect_filled(face_rect, 0.0, tokens.accent.gamma_multiply(0.18));
        painter.rect_stroke(
            face_rect,
            0.0,
            Stroke::new(2.0, tokens.accent),
            StrokeKind::Middle,
        );

        let id = ui.make_persistent_id(("uv_edit", face.index()));
        let mut result: Option<([f32; 4], bool)> = None;
        let mut handle =
            |this: &mut Self, ui: &Ui, rect: Rect, key: usize, grab: Grab, uv: [f32; 4]| {
                let resp = ui.interact(rect, id.with(key), Sense::drag());
                if resp.drag_started() {
                    this.start = uv;
                    this.total = Vec2::ZERO;
                }
                if resp.dragged() || resp.drag_stopped() {
                    this.total += resp.drag_delta() / scale;
                    let corner = match grab {
                        Grab::Body => None,
                        Grab::Corner(u, v) => Some((u, v)),
                    };
                    result = Some((drag_uv(this.start, corner, this.total), resp.drag_stopped()));
                }
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(match grab {
                        Grab::Body => egui::CursorIcon::Move,
                        Grab::Corner(..) => egui::CursorIcon::Crosshair,
                    });
                }
            };
        handle(self, ui, face_rect, 0, Grab::Body, uv);
        let corners = [(0, 1), (2, 1), (0, 3), (2, 3)];
        for (i, (u, v)) in corners.into_iter().enumerate() {
            let center = rect.min + vec2(uv[u], uv[v]) * scale;
            let hr = Rect::from_center_size(center, Vec2::splat(HANDLE));
            handle(self, ui, hr, i + 1, Grab::Corner(u, v), uv);
            painter.rect(
                hr.shrink(space::XXS),
                radius::CONTROL,
                tokens.bg_panel,
                Stroke::new(1.5, tokens.accent),
                StrokeKind::Middle,
            );
        }
        if let Some((uv, commit)) = result {
            sink.edit(Edit::Face {
                face,
                change: FaceChange::Uv { uv, commit },
            });
        }
        ui.vertical_centered(|ui| {
            ui.label(text(
                Type::Caption,
                format!(
                    "{}: {} to {}",
                    face.name(),
                    format_point(uv[0], uv[1]),
                    format_point(uv[2], uv[3])
                ),
            ));
        });
    }
}

/// The face picker and the actions on the UV of the chosen face.
fn toolbar(ui: &mut Ui, face: &mut Face, sink: &mut Sink) {
    let chosen = *face;
    // A narrow area keeps the face picker as an icon.
    let narrow = ui.available_width() < TOOLBAR_WORDS;
    Toolbar::horizontal().show(ui, |bar| {
        menu_trigger(bar.ui(), Some(Icon::FaceUv), chosen.name(), narrow, |ui| {
            for other in Face::ALL {
                if ui
                    .add(MenuItem::new(other.name()).checked(other == chosen))
                    .clicked()
                {
                    *face = other;
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
                sink.edit(Edit::Face {
                    face: chosen,
                    change,
                });
            }
        }
    });
    ui.add_space(space::XS);
}

fn format_point(x: f32, y: f32) -> String {
    format!(
        "({}, {})",
        cuboid_ui_kit::format_number(x, 2),
        cuboid_ui_kit::format_number(y, 2)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn body_drag_moves_both_corners_by_whole_texels() {
        let uv = drag_uv([2.0, 3.0, 6.0, 7.0], None, vec2(1.4, -2.6));
        assert!(close(uv, [3.0, 0.0, 7.0, 4.0]), "{uv:?}");
    }

    #[test]
    fn corner_drag_moves_one_corner_and_keeps_flips() {
        // A flipped rectangle (u2 < u1) keeps its orientation.
        let uv = drag_uv([8.0, 0.0, 4.0, 4.0], Some((2, 3)), vec2(-1.0, 2.0));
        assert!(close(uv, [8.0, 0.0, 3.0, 6.0]), "{uv:?}");
    }
}
