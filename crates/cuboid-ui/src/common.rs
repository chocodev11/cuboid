//! Small things the panels share: the request sink and the picture cache.

use std::collections::HashMap;

use cuboid_actions::ActionId;
use cuboid_ui_kit::{DragFieldOpts, PropertyRows, vector_field};
use egui::{Context, TextureHandle, TextureId, TextureOptions};
use glam::Vec3;

use crate::model::ImageData;
use crate::request::{Edit, UiRequest};

/// Collects the requests of one frame.
#[derive(Default)]
pub(crate) struct Sink {
    pub requests: Vec<UiRequest>,
    /// A panel asked for the project settings window.
    pub project_settings: bool,
}

impl Sink {
    pub fn push(&mut self, request: UiRequest) {
        self.requests.push(request);
    }

    pub fn edit(&mut self, edit: Edit) {
        self.requests.push(UiRequest::Edit(edit));
    }

    pub fn action(&mut self, id: ActionId) {
        self.requests.push(UiRequest::Action(id));
    }
}

/// What a vector row changed: the new vector and whether the edit ended.
pub(crate) struct VecEdit {
    pub value: Vec3,
    pub commit: bool,
}

/// A row of a property grid with three drag fields for a vector. Returns the new vector while a
/// component changes and once more when the edit ends. `opts` gives the speed, range, unit and
/// snap; the label is the accessible name of the fields.
pub(crate) fn vector_row(
    grid: &mut PropertyRows<'_>,
    label: &str,
    value: Vec3,
    opts: &DragFieldOpts<'_>,
) -> Option<VecEdit> {
    let mut components = value.to_array();
    let field = DragFieldOpts {
        name: label,
        ..opts.clone()
    };
    let edit = grid.row(label, |ui| vector_field(ui, label, &mut components, &field));
    (edit.changed || edit.committed).then(|| VecEdit {
        value: Vec3::from_array(components),
        commit: edit.committed,
    })
}

/// Largest picture edge the cache uploads.
const MAX_EDGE: usize = 16384;
/// Frames a picture may go unused before its GPU copy is dropped.
const KEEP_FRAMES: u64 = 600;

struct Slot {
    rev: u64,
    handle: TextureHandle,
    used: u64,
}

/// GPU copies of the pictures the app hands in, keyed by a caller-chosen name. A picture is
/// uploaded again only when its `rev` changes.
#[derive(Default)]
pub(crate) struct ImageCache {
    map: HashMap<String, Slot>,
    frame: u64,
}

impl ImageCache {
    /// The texture of `image`, uploading it when new or changed. `None` for a malformed picture.
    pub fn texture(&mut self, ctx: &Context, key: &str, image: &ImageData) -> Option<TextureId> {
        let [w, h] = image.size;
        let expected = w.checked_mul(h)?.checked_mul(4)?;
        if w == 0 || h == 0 || w > MAX_EDGE || h > MAX_EDGE || image.rgba.len() != expected {
            return None;
        }
        let frame = self.frame;
        if let Some(slot) = self.map.get_mut(key) {
            slot.used = frame;
            if slot.rev != image.rev {
                slot.handle.set(
                    egui::ColorImage::from_rgba_unmultiplied(image.size, &image.rgba),
                    TextureOptions::NEAREST,
                );
                slot.rev = image.rev;
            }
            return Some(slot.handle.id());
        }
        let handle = ctx.load_texture(
            format!("cuboid-ui/{key}"),
            egui::ColorImage::from_rgba_unmultiplied(image.size, &image.rgba),
            TextureOptions::NEAREST,
        );
        let id = handle.id();
        self.map.insert(
            key.to_owned(),
            Slot {
                rev: image.rev,
                handle,
                used: frame,
            },
        );
        Some(id)
    }

    /// Call once per frame: drops pictures nobody asked for in a long time.
    pub fn end_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        if frame.is_multiple_of(120) {
            self.map.retain(|_, slot| frame - slot.used < KEEP_FRAMES);
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn image(w: usize, h: usize, bytes: usize, rev: u64) -> ImageData {
        ImageData {
            size: [w, h],
            rgba: Arc::from(vec![255_u8; bytes]),
            rev,
        }
    }

    #[test]
    fn malformed_pictures_are_refused_and_good_ones_are_cached() {
        let ctx = Context::default();
        let mut cache = ImageCache::default();
        assert!(cache.texture(&ctx, "a", &image(2, 2, 15, 1)).is_none());
        assert!(cache.texture(&ctx, "a", &image(0, 4, 0, 1)).is_none());
        assert!(
            cache
                .texture(&ctx, "a", &image(usize::MAX, 2, 8, 1))
                .is_none()
        );
        assert_eq!(cache.len(), 0);
        let first = cache.texture(&ctx, "a", &image(2, 2, 16, 1));
        let again = cache.texture(&ctx, "a", &image(2, 2, 16, 1));
        let changed = cache.texture(&ctx, "a", &image(2, 2, 16, 2));
        assert!(first.is_some());
        assert_eq!(first, again);
        assert_eq!(first, changed);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn unused_pictures_are_dropped_eventually() {
        let ctx = Context::default();
        let mut cache = ImageCache::default();
        let _ = cache.texture(&ctx, "a", &image(1, 1, 4, 1));
        for _ in 0..(KEEP_FRAMES + 240) {
            cache.end_frame();
        }
        assert_eq!(cache.len(), 0);
    }
}
