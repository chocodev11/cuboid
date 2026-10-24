//! The texture list: previews, selection and the texture actions.

use cuboid_actions::ActionId;
use cuboid_ui_kit::{Icon, RADIUS, RADIUS_SM, Tokens, icon_button, section};
use egui::{Align2, FontId, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::common::{ImageCache, Sink};
use crate::model::DocView;
use crate::request::Edit;

const CONTEXT_ACTIONS: &[(&str, ActionId)] = &[
    ("Apply to selection", ActionId::ApplyTextureToSelection),
    ("Edit texture", ActionId::EditTexture),
    ("Rename", ActionId::RenameTexture),
    ("Load from file", ActionId::LoadTexture),
    ("Save to file", ActionId::SaveTexture),
    ("Clear", ActionId::EmptyTexture),
    ("Delete", ActionId::DeleteTexture),
];

/// The list body: one row per texture with a preview. `preview` is the edge of the thumbnail.
pub(crate) fn list(
    ui: &mut Ui,
    doc: &DocView<'_>,
    images: &mut ImageCache,
    preview: f32,
    sink: &mut Sink,
) {
    let tokens = Tokens::of(ui.ctx());
    if doc.textures.is_empty() {
        ui.label(egui::RichText::new("No textures yet").color(tokens.text_weak));
        return;
    }
    for (index, texture) in doc.textures.iter().enumerate() {
        let selected = doc.active_texture == Some(index);
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), preview + 8.0), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, true, selected, texture.label.as_ref())
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, RADIUS, tokens.accent_soft);
            } else if resp.hovered() {
                painter.rect_filled(rect, RADIUS, tokens.bg_hover);
            }
            let thumb = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(preview, preview));
            painter.rect(
                thumb,
                RADIUS_SM,
                tokens.bg_input,
                Stroke::new(1.0, tokens.border),
                StrokeKind::Inside,
            );
            if let Some(image) = &texture.image {
                let key = format!("texture:{}", texture.label);
                if let Some(id) = images.texture(ui.ctx(), &key, image) {
                    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(id, thumb.shrink(1.0), uv, egui::Color32::WHITE);
                }
            }
            let text_x = thumb.right() + 10.0;
            painter.text(
                egui::pos2(text_x, rect.center().y - 8.0),
                Align2::LEFT_CENTER,
                texture.label.as_ref(),
                FontId::proportional(13.5),
                if selected {
                    tokens.text_strong
                } else {
                    tokens.text
                },
            );
            painter.text(
                egui::pos2(text_x, rect.center().y + 9.0),
                Align2::LEFT_CENTER,
                format!("{} x {}", texture.size[0], texture.size[1]),
                FontId::proportional(11.5),
                tokens.text_weak,
            );
        }
        if resp.clicked() {
            sink.edit(Edit::SelectTexture(index));
        }
        resp.context_menu(|ui| {
            if doc.active_texture != Some(index) {
                sink.edit(Edit::SelectTexture(index));
            }
            for (label, id) in CONTEXT_ACTIONS {
                if ui.button(*label).clicked() {
                    sink.action(*id);
                    ui.close();
                }
            }
        });
    }
}

/// The dockable panel: header with the add button, then the list.
pub(crate) fn panel(ui: &mut Ui, doc: &DocView<'_>, images: &mut ImageCache, sink: &mut Sink) {
    ui.horizontal(|ui| {
        ui.label(format!("{} textures", doc.textures.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, Icon::Plus, "Add texture").clicked() {
                sink.action(ActionId::AddTexture);
            }
            ui.menu_button("Material", |ui| {
                for (label, id) in [
                    ("Add missing textures", ActionId::NewMatAuto),
                    ("Remove unused textures", ActionId::NewMatOptim),
                    ("Delete all textures", ActionId::NewMatEmpty),
                    ("Create atlas, blank textures", ActionId::CreateAtlasBlank),
                    ("Create atlas, keep textures", ActionId::CreateAtlasTextures),
                ] {
                    if ui.button(label).clicked() {
                        sink.action(id);
                        ui.close();
                    }
                }
            });
        });
    });
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| list(ui, doc, images, 40.0, sink));
}

/// The same list as a collapsible section of the properties panel.
pub(crate) fn section_body(
    ui: &mut Ui,
    doc: &DocView<'_>,
    images: &mut ImageCache,
    sink: &mut Sink,
) {
    section(ui, "textures_section", "Textures", true, |ui| {
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Plus, "Add texture").clicked() {
                sink.action(ActionId::AddTexture);
            }
        });
        list(ui, doc, images, 28.0, sink);
    });
}
