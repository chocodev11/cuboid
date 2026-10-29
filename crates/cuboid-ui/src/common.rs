//! Small things the panels share: the request sink and the picture cache.

use std::collections::HashMap;

use cuboid_actions::ActionId;
use cuboid_ui_kit::{DragFieldOpts, PropertyRows, vector_field};
use egui::load::SizedTexture;
use egui::{ColorImage, Context, Id, Image, TextureHandle, TextureId, TextureOptions, Ui, vec2};
use glam::Vec3;

use crate::model::{ImageData, LogoSet};
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
/// The key of the count of GPU setups that the shell keeps in the data of the egui context
/// (`cuboid_shell::GPU_EPOCH`, which this crate cannot depend on). A new renderer starts without
/// textures and egui sends none again, so the cache uploads its pictures again when it changes.
const GPU_EPOCH: &str = "cuboid.gpu_epoch";

struct Slot {
    rev: u64,
    handle: TextureHandle,
    used: u64,
}

/// GPU copies of the pictures the app hands in, keyed by a caller-chosen name. A picture is
/// uploaded again only when its `rev` changes, or when the GPU device was set up again.
#[derive(Default)]
pub(crate) struct ImageCache {
    map: HashMap<String, Slot>,
    frame: u64,
    /// The GPU epoch the pictures in `map` were uploaded in.
    epoch: u64,
    /// The rasters of the logo, which [`ImageCache::logo`] uploads smooth.
    logo: LogoSet,
}

/// `image` as an egui image of at most `limit` texels a side: a larger picture keeps every
/// `k`-th texel in both directions (nearest).
fn color_image(image: &ImageData, limit: usize) -> ColorImage {
    let [w, h] = image.size;
    let step = w.max(h).div_ceil(limit.max(1)).max(1);
    if step == 1 {
        return ColorImage::from_rgba_unmultiplied(image.size, &image.rgba);
    }
    let mut texels = Vec::new();
    for row in image.rgba.chunks_exact(w * 4).step_by(step) {
        for texel in row.chunks_exact(4).step_by(step) {
            texels.extend_from_slice(texel);
        }
    }
    ColorImage::from_rgba_unmultiplied([w.div_ceil(step), h.div_ceil(step)], &texels)
}

impl ImageCache {
    /// The texture of `image`, uploading it when new or changed. `None` for a malformed picture.
    /// A picture larger than the device takes is uploaded cut down. Pixel art: the texels stay
    /// sharp when it is drawn larger.
    pub fn texture(&mut self, ctx: &Context, key: &str, image: &ImageData) -> Option<TextureId> {
        self.upload(ctx, key, image, TextureOptions::NEAREST)
    }

    /// Hands in the rasters of the logo, which the app decoded once.
    pub fn set_logo(&mut self, logo: LogoSet) {
        self.logo = logo;
    }

    /// The texture of the logo raster for a logo of `points` on the screen, uploaded smooth: a
    /// raster is drawn a little smaller than it is, and nearest sampling would alias it. `None`
    /// when the app handed in no logo.
    ///
    /// spec: logo 5
    pub fn logo(&mut self, ctx: &Context, points: f32) -> Option<TextureId> {
        let raster = self.logo.pick(points * ctx.pixels_per_point())?.clone();
        let key = format!("logo:{}", raster.size[0]);
        self.upload(ctx, &key, &raster, TextureOptions::LINEAR)
    }

    fn upload(
        &mut self,
        ctx: &Context,
        key: &str,
        image: &ImageData,
        options: TextureOptions,
    ) -> Option<TextureId> {
        let [w, h] = image.size;
        let expected = w.checked_mul(h)?.checked_mul(4)?;
        if w == 0 || h == 0 || w > MAX_EDGE || h > MAX_EDGE || image.rgba.len() != expected {
            return None;
        }
        let epoch = ctx
            .data(|data| data.get_temp::<u64>(Id::unique(GPU_EPOCH)))
            .unwrap_or_default();
        if epoch != self.epoch {
            self.epoch = epoch;
            self.map.clear();
        }
        let limit = ctx.input(|input| input.max_texture_side);
        let frame = self.frame;
        if let Some(slot) = self.map.get_mut(key) {
            slot.used = frame;
            if slot.rev != image.rev {
                slot.handle.set(color_image(image, limit), options);
                slot.rev = image.rev;
            }
            return Some(slot.handle.id());
        }
        let handle = ctx.load_texture(
            format!("cuboid-ui/{key}"),
            color_image(image, limit),
            options,
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

/// Draws the logo `points` wide and high at `opacity` (1 is opaque); nothing while the app has
/// handed in no logo. It is a picture, not a button: it senses hover only, so a press on it
/// reaches what lies behind it, such as the drag area of the top bar.
///
/// spec: logo 5
pub(crate) fn logo(ui: &mut Ui, images: &mut ImageCache, points: f32, opacity: f32) {
    let Some(texture) = images.logo(ui.ctx(), points) else {
        return;
    };
    let size = vec2(points, points);
    let picture = Image::new(SizedTexture::new(texture, size)).alt_text("cuboid logo");
    ui.scope(|ui| {
        ui.set_opacity(opacity);
        ui.add(picture);
    });
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
        // Pixel art: only the logo is smooth.
        let options = first.and_then(|id| ctx.tex_manager().read().meta(id).map(|m| m.options));
        assert_eq!(options, Some(TextureOptions::NEAREST));
    }

    #[test]
    fn a_picture_above_the_device_limit_is_uploaded_cut_down() {
        let ctx = Context::default();
        // egui's font atlas needs at least 1024 texels a side.
        let raw = egui::RawInput {
            max_texture_side: Some(1024),
            ..egui::RawInput::default()
        };
        ctx.begin_pass(raw);
        let mut cache = ImageCache::default();
        // Every other texel of the 2048 x 2 picture: 1024 x 1.
        let id = cache.texture(&ctx, "wide", &image(2048, 2, 2048 * 2 * 4, 1));
        let uploaded = id.and_then(|id| ctx.tex_manager().read().meta(id).map(|meta| meta.size));
        assert_eq!(uploaded, Some([1024, 1]));
        let small = cache.texture(&ctx, "small", &image(4, 4, 64, 1));
        let uploaded = small.and_then(|id| ctx.tex_manager().read().meta(id).map(|m| m.size));
        assert_eq!(uploaded, Some([4, 4]));
        ctx.end_pass().drop_without_applying_deltas();
    }

    #[test]
    fn a_new_gpu_epoch_uploads_every_picture_again() {
        let ctx = Context::default();
        let mut cache = ImageCache::default();
        let picture = image(2, 2, 16, 1);
        let first = cache.texture(&ctx, "a", &picture);
        assert_eq!(cache.texture(&ctx, "a", &picture), first);
        // The shell gave egui a new renderer, which has none of the textures.
        ctx.data_mut(|data| data.insert_temp(Id::unique(GPU_EPOCH), 1_u64));
        let again = cache.texture(&ctx, "a", &picture);
        assert!(again.is_some());
        assert_ne!(
            again, first,
            "a new texture, which egui sends to the new renderer"
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.texture(&ctx, "a", &picture), again, "and it stays");
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
