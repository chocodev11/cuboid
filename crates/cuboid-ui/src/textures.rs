//! The Textures panel: the texture list with previews, selection and the texture actions.

use cuboid_actions::ActionId;
use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    Button, EmptyState, Icon, IconButton, ListRow, MenuItem, Type, menu_trigger, text,
};
use egui::{Label, ScrollArea, Sides, Ui, vec2};

use crate::common::{ImageCache, Sink};
use crate::model::{DocView, TextureView};
use crate::request::{Edit, FramesRequest, UiRequest};

/// The rows of the context menu of a texture, with the action each one runs.
const CONTEXT_ACTIONS: &[(&str, ActionId)] = &[
    ("Apply to selection", ActionId::ApplyTextureToSelection),
    ("Edit texture", ActionId::EditTexture),
    ("Rename", ActionId::RenameTexture),
    ("Replace from file", ActionId::LoadTexture),
    ("Save to file", ActionId::SaveTexture),
    ("Clear", ActionId::EmptyTexture),
    ("Remove", ActionId::DeleteTexture),
];

/// The actions of the Material menu.
const MATERIAL_ACTIONS: &[(&str, ActionId)] = &[
    ("Add missing textures", ActionId::NewMatAuto),
    ("Remove unused textures", ActionId::NewMatOptim),
    ("Delete all textures", ActionId::NewMatEmpty),
    ("Create atlas, blank textures", ActionId::CreateAtlasBlank),
    ("Create atlas, keep textures", ActionId::CreateAtlasTextures),
];

/// The panel body: the count and the add and import buttons, then the list.
pub(crate) fn panel(ui: &mut Ui, doc: &DocView<'_>, images: &mut ImageCache, sink: &mut Sink) {
    if doc.textures.is_empty() {
        let clicked = EmptyState::new(Icon::Texture, "No textures yet")
            .body("Add a blank texture or import a picture.")
            .action(Button::new("Add texture").icon(Icon::Plus))
            .action(Button::new("Import").icon(Icon::Import))
            .show(ui);
        match clicked {
            Some(0) => sink.action(ActionId::AddTexture),
            Some(_) => sink.action(ActionId::LoadTexture),
            None => {}
        }
        return;
    }
    Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.add(
                Label::new(text(
                    Type::Label,
                    format!("{} textures", doc.textures.len()),
                ))
                .truncate()
                .selectable(false),
            );
        },
        |ui| {
            ui.spacing_mut().item_spacing.x = space::XXS;
            if ui.add(IconButton::new(Icon::Plus, "Add texture")).clicked() {
                sink.action(ActionId::AddTexture);
            }
            if ui
                .add(IconButton::new(
                    Icon::Import,
                    "Import a texture from a file",
                ))
                .clicked()
            {
                sink.action(ActionId::LoadTexture);
            }
            // The selected texture: replace its pixels from a file, or take it out of the list.
            ui.add_enabled_ui(doc.active_texture.is_some(), |ui| {
                for (icon, tip, id) in [
                    (
                        Icon::Replace,
                        "Replace the selected texture from a file",
                        ActionId::LoadTexture,
                    ),
                    (
                        Icon::Trash,
                        "Remove the selected texture",
                        ActionId::DeleteTexture,
                    ),
                ] {
                    if ui.add(IconButton::new(icon, tip)).clicked() {
                        sink.action(id);
                    }
                }
            });
            menu_trigger(ui, Some(Icon::More), "Material", true, |ui| {
                for (label, id) in MATERIAL_ACTIONS {
                    if ui.add(MenuItem::new(label)).clicked() {
                        sink.action(*id);
                        ui.close();
                    }
                }
            });
        },
    );
    ui.add_space(space::XS);
    play_row(ui, doc, sink);
    ui.spacing_mut().item_spacing.y = 0.0;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, height::ROW_THUMB, doc.textures.len(), |ui, range| {
            for index in range {
                row(ui, doc, index, images, sink);
            }
        });
}

/// The play button of the animated textures, with how many there are; nothing without one.
///
/// spec: textures T1 4
fn play_row(ui: &mut Ui, doc: &DocView<'_>, sink: &mut Sink) {
    let animated = doc
        .textures
        .iter()
        .filter(|t| t.animation.is_some())
        .count();
    if animated == 0 {
        return;
    }
    ui.horizontal(|ui| {
        let (icon, tip) = if doc.texture_playing {
            (Icon::Pause, "Pause the animated textures")
        } else {
            (Icon::Play, "Play the animated textures")
        };
        if ui.add(IconButton::new(icon, tip)).clicked() {
            let play = FramesRequest::Play(!doc.texture_playing);
            sink.push(UiRequest::Frames(play));
        }
        ui.add(Label::new(text(Type::Label, format!("{animated} animated"))).selectable(false));
    });
    ui.add_space(space::XS);
}

/// The line under the name of a texture: the size, and the number of frames of an animated one.
fn caption(texture: &TextureView) -> String {
    let [w, h] = texture.size;
    match &texture.animation {
        Some(frames) => {
            let tiles = frames.tiles;
            let plural = if tiles == 1 { "" } else { "s" };
            format!("{w} x {h}, {tiles} frame{plural}")
        }
        None => format!("{w} x {h}"),
    }
}

/// One texture: its picture, its name and its size, with the context menu.
fn row(ui: &mut Ui, doc: &DocView<'_>, index: usize, images: &mut ImageCache, sink: &mut Sink) {
    let Some(texture) = doc.textures.get(index) else {
        return;
    };
    let size = caption(texture);
    let mut row = ListRow::new(&texture.label)
        .caption(&size)
        .selected(doc.active_texture == Some(index))
        .picture_icon(Icon::Texture);
    if let Some(image) = &texture.image {
        let key = format!("texture:{}", texture.label);
        if let Some(id) = images.texture(ui.ctx(), &key, image) {
            #[allow(clippy::cast_precision_loss)]
            let picture = vec2(image.size[0] as f32, image.size[1] as f32);
            row = row.thumbnail(id, picture);
        }
    }
    let response = ui.add(row);
    if response.clicked() {
        sink.edit(Edit::SelectTexture(index));
    }
    response.context_menu(|ui| {
        if doc.active_texture != Some(index) {
            sink.edit(Edit::SelectTexture(index));
        }
        for (label, id) in CONTEXT_ACTIONS {
            if ui.add(MenuItem::new(label)).clicked() {
                sink.action(*id);
                ui.close();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::{FramesView, StepView};

    fn texture(animation: Option<FramesView>) -> TextureView {
        TextureView {
            label: Arc::from("water"),
            size: [16, 8],
            image: None,
            source: None,
            animation,
            animatable: None,
        }
    }

    fn frames(tiles: u32) -> FramesView {
        FramesView {
            frame_height: 8,
            tiles,
            frametime: 1,
            interpolate: false,
            steps: (0..tiles)
                .map(|tile| StepView { tile, time: None })
                .collect(),
            explicit: false,
            shown: 0,
        }
    }

    #[test]
    fn an_animated_texture_names_its_frames_after_its_size() {
        assert_eq!(caption(&texture(None)), "16 x 8");
        assert_eq!(caption(&texture(Some(frames(4)))), "16 x 8, 4 frames");
        assert_eq!(caption(&texture(Some(frames(1)))), "16 x 8, 1 frame");
    }
}
