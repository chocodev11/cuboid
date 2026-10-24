//! Small things the panels share: the request sink, the picture cache and field helpers.

use std::collections::HashMap;

use cuboid_actions::ActionId;
use cuboid_ui_kit::{DragFieldOpts, Tokens, drag_field, fit_contrast};
use egui::{Color32, Context, TextureHandle, TextureId, TextureOptions, Ui};
use glam::Vec3;

use crate::model::ImageData;
use crate::request::{Edit, UiRequest};

/// Collects the requests of one frame.
#[derive(Default)]
pub(crate) struct Sink {
    pub requests: Vec<UiRequest>,
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

/// Colours of the X, Y and Z axis labels, fitted to read on the field surfaces.
pub(crate) fn axis_colors(tokens: &Tokens) -> [Color32; 3] {
    let (bases, toward) = if tokens.dark {
        ([0xe8_6a_62, 0x74_c9_6b, 0x62_9c_e8], Color32::WHITE)
    } else {
        ([0xb8_30_28, 0x2a_8a_2a, 0x20_5c_c0], Color32::BLACK)
    };
    // The label chip tints the field with the colour, so ask for a little more than 4.5.
    let needs = [tokens.bg_panel, tokens.bg_raised, tokens.bg_input].map(|bg| (bg, 5.0));
    bases.map(|hex: u32| {
        let [_, r, g, b] = hex.to_be_bytes();
        fit_contrast(Color32::from_rgb(r, g, b), toward, &needs)
    })
}

/// What a vector row did.
pub(crate) struct VecEdit {
    /// The new vector.
    pub value: Vec3,
    /// A component changed this frame.
    pub changed: bool,
    /// An edit ended this frame.
    pub committed: bool,
}

/// Three drag fields in a row for a vector. `name` is the accessible prefix ("Position").
pub(crate) fn vec3_row(
    ui: &mut Ui,
    id: &str,
    name: &str,
    value: Vec3,
    base: &DragFieldOpts<'_>,
) -> VecEdit {
    let tokens = Tokens::of(ui.ctx());
    let colors = axis_colors(&tokens);
    let letters = ["X", "Y", "Z"];
    let names = [
        format!("{name} X"),
        format!("{name} Y"),
        format!("{name} Z"),
    ];
    let mut v = value.to_array();
    let (mut changed, mut committed) = (false, false);
    let width = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(48.0);
    ui.horizontal(|ui| {
        for i in 0..3 {
            let opts = DragFieldOpts {
                name: &names[i],
                label: Some((letters[i], colors[i])),
                width: Some(width),
                unit: base.unit,
                range: base.range.clone(),
                ..*base
            };
            let r = drag_field(ui, (id, i), &mut v[i], &opts);
            changed |= r.changed;
            committed |= r.committed;
        }
    });
    VecEdit {
        value: Vec3::from_array(v),
        changed,
        committed,
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

    #[test]
    fn axis_labels_are_readable_on_every_field_surface() {
        for dark in [true, false] {
            let t = cuboid_ui_kit::Theme::default().tokens(dark);
            for color in axis_colors(&t) {
                for bg in [t.bg_panel, t.bg_raised, t.bg_input] {
                    let ratio = cuboid_ui_kit::contrast_ratio(color, bg);
                    assert!(
                        ratio >= 4.5,
                        "dark {dark}: {color:?} on {bg:?} is {ratio:.2}"
                    );
                }
            }
        }
    }
}
