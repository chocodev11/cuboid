//! The Texture tab: the properties of the selected texture. The texture list is the Textures
//! panel.

use cuboid_actions::ActionId;
use cuboid_ui_kit::metrics::{height, radius, space};
use cuboid_ui_kit::{
    Button, EmptyState, Icon, PropertyGrid, Section, Tokens, Type, fit_centered, paint_image, text,
};
use egui::{Label, Sense, Ui, vec2};

use crate::common::{ImageCache, Sink};
use crate::model::{DocView, TextureView};
use crate::request::Edit;

/// Tallest the preview grows, in points.
const PREVIEW_MAX: f32 = 160.0;

/// The page of the tab.
pub(crate) fn show(ui: &mut Ui, doc: &DocView<'_>, images: &mut ImageCache, sink: &mut Sink) {
    let Some((index, texture)) = doc
        .active_texture
        .and_then(|i| doc.textures.get(i).map(|t| (i, t)))
    else {
        let add = EmptyState::new(Icon::Texture, "No texture selected")
            .body("Add a texture, or pick one in the Textures panel.")
            .action(Button::new("Add texture").icon(Icon::Plus))
            .show(ui);
        if add.is_some() {
            sink.action(ActionId::AddTexture);
        }
        return;
    };
    preview(ui, texture, images);
    ui.add_space(space::MD);
    Section::new("Details").show(ui, "props_texture", |ui| {
        PropertyGrid::new("props_texture").show(ui, |grid| {
            grid.row("Name", |ui| {
                ui.add(Label::new(text(Type::Body, &*texture.label)).truncate());
            });
            grid.row("Size", |ui| {
                let [w, h] = texture.size;
                ui.add(Label::new(text(Type::Body, format!("{w} x {h} px"))).selectable(false));
            });
            grid.row("Source", |ui| match &texture.source {
                Some(path) => {
                    ui.add(Label::new(text(Type::Body, &**path)).truncate())
                        .on_hover_text(&**path);
                }
                None => {
                    ui.add(Label::new(text(Type::Label, "Not loaded from a file")).truncate());
                }
            });
        });
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(Button::new("Replace").icon(Icon::Replace))
            .on_hover_text("Replace the pixels with a picture from a file")
            .clicked()
        {
            sink.action(ActionId::LoadTexture);
        }
        let reload = ui.add_enabled(
            texture.source.is_some(),
            Button::new("Reload").icon(Icon::Reload),
        );
        if reload
            .on_hover_text("Read the file the texture came from again")
            .clicked()
        {
            sink.edit(Edit::ReloadTexture(index));
        }
        if ui
            .add(Button::new("Export").icon(Icon::Export))
            .on_hover_text("Save the texture as a picture file")
            .clicked()
        {
            sink.action(ActionId::SaveTexture);
        }
    });
}

/// The picture of the texture fitted into a frame as wide as the panel.
fn preview(ui: &mut Ui, texture: &TextureView, images: &mut ImageCache) {
    let tokens = Tokens::of(ui.ctx());
    let width = ui.available_width();
    let (frame, _) = ui.allocate_exact_size(
        vec2(width, (width * 3.0 / 4.0).clamp(height::BAR, PREVIEW_MAX)),
        Sense::hover(),
    );
    ui.painter()
        .rect_filled(frame, radius::CONTROL, tokens.bg_input);
    let Some(image) = &texture.image else { return };
    let key = format!("texture:{}", texture.label);
    let Some(id) = images.texture(ui.ctx(), &key, image) else {
        return;
    };
    let [w, h] = image.size;
    #[allow(clippy::cast_precision_loss)]
    let picture = vec2(w as f32, h as f32);
    paint_image(
        ui.painter(),
        id,
        fit_centered(frame.shrink(space::XS), picture),
    );
}
