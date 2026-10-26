//! Runtime atlas layout on etagere; never persisted (spec: tools 5.2; arch 13.6).
//!
//! Textures are placed with one transparent texel of padding on every side and never moved: an
//! added texture takes free space, a replaced texture of the same size is rewritten in place, a
//! removed one gives its space back. When a page is full a new page starts. Rows of a page run
//! bottom to top like the rows of [`Image::rgba`], and so do the `y` coordinates here and in
//! [`AtlasUpdate`], so a texture coordinate `v` (up) is `y / page_size`.
//!
//! An animated texture holds one frame: its rectangle has the size of a frame, and what it shows
//! is the tile that its animation plays at the texture tick (spec: textures T1 4). A new tick
//! rewrites the rectangle only when it shows another tile or another blend of two tiles.

use std::borrow::Cow;
use std::sync::Arc;

use cuboid_image::{Image, Rev};
use cuboid_model::{Label, Shown, TextureEntry, TextureList};
use etagere::{AllocId, AtlasAllocator, size2};
use glam::Vec2;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::extract::AtlasUpdate;

/// Transparent texels around every texture.
pub const PADDING: u32 = 1;

/// The label faces fall back to when their texture is missing; the entry is built in and not part
/// of the texture list.
///
/// spec: tools 4.6
pub const ERROR_LABEL: &str = "error";

/// Edge of the built-in error texture.
const ERROR_EDGE: u32 = 16;

/// Default page edge in texels.
pub const DEFAULT_PAGE_SIZE: u32 = 2048;

/// Where a texture sits in the atlas, in texels, without its padding.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AtlasRect {
    /// The page.
    pub page: u32,
    /// Left texel.
    pub x: u32,
    /// Lowest texel row.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

struct Placed {
    id: AllocId,
    rect: AtlasRect,
}

struct Entry {
    /// `None` while the texture does not fit a page or has no texels.
    placed: Option<Placed>,
    /// The image the pixels came from, `None` for the built-in error texture.
    image: Option<Arc<Image>>,
    rev: Rev,
    /// Some texel has partial alpha; for a strip, in some frame.
    partial: bool,
    /// The size the rectangle was asked for: a frame of a strip, or the whole picture.
    size: (u32, u32),
    /// What the rectangle holds of an animated texture; `None` for a still one.
    shown: Option<Shown>,
}

/// The layout of the texture atlas.
///
/// spec: tools 5.2, arch 13.6
pub struct AtlasLayout {
    page_size: u32,
    pages: Vec<AtlasAllocator>,
    entries: FxHashMap<Label, Entry>,
    error: Entry,
    error_sent: bool,
    version: u64,
    generation: u64,
    /// The texture tick the animated textures show.
    tick: u32,
}

impl std::fmt::Debug for AtlasLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtlasLayout")
            .field("page_size", &self.page_size)
            .field("pages", &self.pages.len())
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Default for AtlasLayout {
    fn default() -> Self {
        Self::new(DEFAULT_PAGE_SIZE)
    }
}

/// The error texture: a magenta and black checker of 8-texel squares.
fn error_pixels() -> Vec<u8> {
    let mut px = Vec::with_capacity((ERROR_EDGE * ERROR_EDGE * 4) as usize);
    for y in 0..ERROR_EDGE {
        for x in 0..ERROR_EDGE {
            let lit = (x / 8 + y / 8) % 2 == 0;
            px.extend(if lit {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            });
        }
    }
    px
}

impl AtlasLayout {
    /// An atlas of pages with `page_size` texels per edge (at least 64), holding the error
    /// texture.
    pub fn new(page_size: u32) -> Self {
        let page_size = page_size.max(64);
        let mut layout = Self {
            page_size,
            pages: Vec::new(),
            entries: FxHashMap::default(),
            error: Entry {
                placed: None,
                image: None,
                rev: Rev::NONE,
                partial: false,
                size: (ERROR_EDGE, ERROR_EDGE),
                shown: None,
            },
            error_sent: false,
            version: 0,
            generation: 0,
            tick: 0,
        };
        layout.error.placed = layout.allocate(ERROR_EDGE, ERROR_EDGE);
        layout
    }

    /// Texels per page edge.
    pub fn page_size(&self) -> u32 {
        self.page_size
    }

    /// The number of pages in use.
    pub fn pages(&self) -> u32 {
        u32::try_from(self.pages.len()).unwrap_or(u32::MAX)
    }

    /// Changes whenever a texture is placed, moved or removed, so cached texture coordinates are
    /// stale. Rewriting a texture in place does not change it.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Changes whenever something an object's instance record or alpha class reads changes: a
    /// placement, or a rewrite of a texture that has or had partial alpha.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Sets the texture tick that animated textures show; the next [`AtlasLayout::sync`] rewrites
    /// the rectangles whose picture it changes.
    pub fn set_tick(&mut self, tick: u32) {
        self.tick = tick;
    }

    /// Whether the texels of a texture that the rectangle `tl` to `br` (texture coordinates, `v`
    /// up) covers may hold partial alpha. A strip answers for every frame, so the answer does not
    /// change with the tick. `false` for the error texture and unknown labels.
    pub fn any_partial(&self, label: &str, tl: Vec2, br: Vec2) -> bool {
        let Some(entry) = self.entries.get(label) else {
            return false;
        };
        let Some(image) = &entry.image else {
            return false;
        };
        if entry.shown.is_some() {
            return entry.partial;
        }
        let (lo, hi) = (tl.min(br), tl.max(br));
        let texel = |v: f32, edge: u32, up: bool| {
            let t = v.clamp(0.0, 1.0) * edge as f32;
            cuboid_num::trunc_i32(if up { t.ceil() } else { t.floor() })
                .max(0)
                .unsigned_abs()
        };
        image.alpha().any_partial(
            texel(lo.x, image.w, false),
            texel(lo.y, image.h, false),
            texel(hi.x, image.w, true),
            texel(hi.y, image.h, true),
        )
    }

    /// The rectangle of a texture; the error texture for an empty, unknown or unplaceable label.
    ///
    /// spec: tools 4.6
    pub fn rect(&self, label: &str) -> AtlasRect {
        self.entries
            .get(label)
            .and_then(|e| e.placed.as_ref())
            .or(self.error.placed.as_ref())
            .map_or(
                AtlasRect {
                    page: 0,
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
                |p| p.rect,
            )
    }

    /// Takes free space of `w` x `h` texels plus padding, in the first page that has it or a new
    /// page.
    fn allocate(&mut self, w: u32, h: u32) -> Option<Placed> {
        let (pw, ph) = (w.checked_add(2 * PADDING)?, h.checked_add(2 * PADDING)?);
        if w == 0 || h == 0 || pw > self.page_size || ph > self.page_size {
            return None;
        }
        let size = size2(i32::try_from(pw).ok()?, i32::try_from(ph).ok()?);
        let edge = i32::try_from(self.page_size).ok()?;
        let mut found = None;
        for (page, atlas) in self.pages.iter_mut().enumerate() {
            if let Some(a) = atlas.allocate(size) {
                found = Some((page, a));
                break;
            }
        }
        if found.is_none() {
            let mut atlas = AtlasAllocator::new(size2(edge, edge));
            let a = atlas.allocate(size)?;
            self.pages.push(atlas);
            found = Some((self.pages.len() - 1, a));
        }
        let (page, a) = found?;
        Some(Placed {
            id: a.id,
            rect: AtlasRect {
                page: u32::try_from(page).ok()?,
                x: a.rectangle.min.x.unsigned_abs() + PADDING,
                y: a.rectangle.min.y.unsigned_abs() + PADDING,
                w,
                h,
            },
        })
    }

    fn release(&mut self, placed: &Placed) {
        if let Some(page) = self.pages.get_mut(placed.rect.page as usize) {
            page.deallocate(placed.id);
        }
    }

    /// Brings the layout up to date with the texture list and appends the rectangles to upload:
    /// the error texture on the first call, new and changed textures afterwards. An unchanged
    /// list appends nothing.
    pub fn sync(&mut self, textures: &TextureList, out: &mut Vec<AtlasUpdate>) {
        if !self.error_sent {
            self.error_sent = true;
            if let Some(p) = &self.error.placed {
                out.push(padded_update(
                    p.rect,
                    &error_pixels(),
                    ERROR_EDGE,
                    ERROR_EDGE,
                ));
            }
        }
        let live: FxHashSet<&Label> = textures.entries.iter().map(|e| &e.label).collect();
        let gone: Vec<Label> = self
            .entries
            .keys()
            .filter(|label| !live.contains(label))
            .cloned()
            .collect();
        for label in gone {
            if let Some(entry) = self.entries.remove(&label) {
                if let Some(placed) = &entry.placed {
                    self.release(placed);
                }
                self.version += 1;
                self.generation += 1;
            }
        }
        for texture in &textures.entries {
            let image = &texture.texture.image;
            let size = texture.frame_size();
            let shown = texture
                .animation
                .as_ref()
                .map(|a| a.shown(a.tiles(image.h), self.tick));
            let known = self.entries.get(&texture.label);
            let same_image = known.is_some_and(|e| {
                e.rev == image.rev && e.image.as_ref().is_some_and(|i| Arc::ptr_eq(i, image))
            });
            if same_image
                && known.is_some_and(|e| e.size == size && picture(e.shown) == picture(shown))
            {
                continue;
            }
            // A picture that found no room in the atlas is not tried again on every tick.
            if same_image && known.is_some_and(|e| e.size == size && e.placed.is_none()) {
                continue;
            }
            let same_size = known
                .and_then(|e| e.placed.as_ref())
                .is_some_and(|p| (p.rect.w, p.rect.h) == size);
            let was_partial = known.is_some_and(|e| e.partial);
            // Another tile of the same picture does not change what the faces are drawn with.
            let partial = if same_image {
                was_partial
            } else {
                image.alpha().any_partial(0, 0, image.w, image.h)
            };
            let placed = if same_size {
                // Rewritten in place; the rectangle stays.
                self.entries.remove(&texture.label).and_then(|e| e.placed)
            } else {
                if let Some(old) = self.entries.remove(&texture.label).and_then(|e| e.placed) {
                    self.release(&old);
                }
                self.version += 1;
                self.generation += 1;
                let placed = self.allocate(size.0, size.1);
                if placed.is_none() {
                    log::warn!(
                        "texture {:?} of {} x {} texels does not fit an atlas page of {}; faces show the error texture",
                        &*texture.label,
                        size.0,
                        size.1,
                        self.page_size
                    );
                }
                placed
            };
            if same_size && !same_image && (was_partial || partial) {
                self.generation += 1;
            }
            if let Some(p) = &placed {
                out.push(padded_update(
                    p.rect,
                    &frame_texels(texture, shown),
                    size.0,
                    size.1,
                ));
            }
            self.entries.insert(
                texture.label.clone(),
                Entry {
                    placed,
                    image: Some(Arc::clone(image)),
                    rev: image.rev,
                    partial,
                    size,
                    shown,
                },
            );
        }
    }
}

/// What the rectangle of a texture shows at a tick: the tile, and while a blend is under way the
/// next tile, the ticks it has been showing and the ticks of the step.
type Picture = (u32, Option<(u32, u32, u32)>);

/// The picture a tick shows. Two ticks with the same picture upload nothing.
fn picture(shown: Option<Shown>) -> Option<Picture> {
    shown.map(|s| (s.tile, s.blends().then_some((s.next, s.elapsed, s.time))))
}

/// The texels of an entry's rectangle: the picture of a still texture, else the tile the
/// animation shows, mixed with the next tile while the animation interpolates.
fn frame_texels(entry: &TextureEntry, shown: Option<Shown>) -> Cow<'_, [u8]> {
    let image = &entry.texture.image;
    let (Some(animation), Some(shown)) = (&entry.animation, shown) else {
        return Cow::Borrowed(image.rgba());
    };
    let row = image.w as usize * 4;
    let tile = |tile: u32| {
        let rows = animation.tile_rows(image.h, tile);
        image
            .rgba()
            .get(rows.start as usize * row..rows.end as usize * row)
            .unwrap_or_default()
    };
    if !shown.blends() {
        return Cow::Borrowed(tile(shown.tile));
    }
    let (from, to) = (tile(shown.tile), tile(shown.next));
    let (keep, take) = (shown.time - shown.elapsed, shown.elapsed);
    Cow::Owned(
        from.iter()
            .zip(to)
            .map(|(&a, &b)| {
                let mixed =
                    (u32::from(a) * keep + u32::from(b) * take + shown.time / 2) / shown.time;
                u8::try_from(mixed).unwrap_or(u8::MAX)
            })
            .collect(),
    )
}

/// The upload of a texture with its padding: the rectangle grown by [`PADDING`] on every side,
/// transparent around the pixels.
fn padded_update(rect: AtlasRect, rgba: &[u8], w: u32, h: u32) -> AtlasUpdate {
    let (pw, ph) = ((w + 2 * PADDING) as usize, (h + 2 * PADDING) as usize);
    let mut buf = vec![0_u8; pw * ph * 4];
    let row = w as usize * 4;
    if row > 0 {
        for (y, src) in rgba.chunks_exact(row).take(h as usize).enumerate() {
            let at = ((y + PADDING as usize) * pw + PADDING as usize) * 4;
            buf[at..at + row].copy_from_slice(src);
        }
    }
    AtlasUpdate {
        page: rect.page,
        x: rect.x - PADDING,
        y: rect.y - PADDING,
        w: w + 2 * PADDING,
        h: h + 2 * PADDING,
        rgba: Arc::from(buf),
    }
}

#[cfg(test)]
mod tests {
    use cuboid_image::RgbaBuf;
    use cuboid_model::{Texture, TextureAnimation, TextureEntry};

    use super::*;

    fn image(w: u32, h: u32, value: u8) -> Arc<Image> {
        let mut buf = RgbaBuf::new(w, h);
        buf.pixels.fill(value);
        Arc::new(Image::from_rgba(buf))
    }

    fn list(entries: &[(&str, &Arc<Image>)]) -> TextureList {
        TextureList {
            entries: entries
                .iter()
                .map(|(label, img)| TextureEntry {
                    label: Label::from(*label),
                    texture: Texture::new(Arc::clone(img)),
                    animation: None,
                })
                .collect(),
        }
    }

    fn overlaps(a: AtlasRect, b: AtlasRect) -> bool {
        a.page == b.page && a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn the_first_sync_uploads_the_error_texture_and_each_new_texture() {
        let mut atlas = AtlasLayout::new(256);
        let mut out = Vec::new();
        atlas.sync(&TextureList::default(), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].w, out[0].h), (18, 18));
        let a = image(16, 16, 200);
        let tex = list(&[("stone", &a)]);
        out.clear();
        atlas.sync(&tex, &mut out);
        assert_eq!(out.len(), 1);
        let rect = atlas.rect("stone");
        assert_eq!((rect.w, rect.h), (16, 16));
        assert_eq!((out[0].x + 1, out[0].y + 1), (rect.x, rect.y));
        // The padding is transparent, the pixels are copied.
        assert_eq!(&out[0].rgba[0..4], &[0, 0, 0, 0]);
        let at = (18 + 1) * 4;
        assert_eq!(&out[0].rgba[at..at + 4], &[200, 200, 200, 200]);
        out.clear();
        atlas.sync(&tex, &mut out);
        assert!(out.is_empty(), "an unchanged list uploads nothing");
    }

    #[test]
    fn textures_never_overlap_and_unknown_labels_fall_back_to_the_error_texture() {
        let mut atlas = AtlasLayout::new(128);
        let images: Vec<Arc<Image>> = (0..20).map(|i| image(10 + i, 12 + i, 1)).collect();
        let labels: Vec<String> = (0..20).map(|i| format!("t{i}")).collect();
        let tex = list(
            &labels
                .iter()
                .map(String::as_str)
                .zip(&images)
                .collect::<Vec<_>>(),
        );
        atlas.sync(&tex, &mut Vec::new());
        assert!(
            atlas.pages() >= 2,
            "20 textures need more than one 128 page"
        );
        let rects: Vec<AtlasRect> = labels.iter().map(|l| atlas.rect(l)).collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.x + a.w <= 128 && a.y + a.h <= 128);
            for b in &rects[i + 1..] {
                assert!(!overlaps(*a, *b), "{a:?} {b:?}");
            }
        }
        let error = atlas.rect(ERROR_LABEL);
        assert_eq!(atlas.rect("missing"), error);
        assert_eq!(atlas.rect(""), error);
        assert_eq!((error.w, error.h), (16, 16));
    }

    #[test]
    fn a_texture_of_the_same_size_is_rewritten_in_place() {
        let mut atlas = AtlasLayout::new(256);
        let a = image(16, 16, 10);
        atlas.sync(&list(&[("t", &a)]), &mut Vec::new());
        let (rect, version) = (atlas.rect("t"), atlas.version());
        let b = image(16, 16, 99);
        let mut out = Vec::new();
        atlas.sync(&list(&[("t", &b)]), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!((atlas.rect("t"), atlas.version()), (rect, version));
    }

    #[test]
    fn a_resized_texture_moves_and_the_version_changes() {
        let mut atlas = AtlasLayout::new(256);
        let a = image(16, 16, 10);
        let other = image(30, 30, 3);
        atlas.sync(&list(&[("t", &a), ("o", &other)]), &mut Vec::new());
        let before = atlas.version();
        let bigger = image(40, 20, 99);
        let mut out = Vec::new();
        atlas.sync(&list(&[("t", &bigger), ("o", &other)]), &mut out);
        assert_eq!(out.len(), 1, "only the changed texture uploads");
        assert!(atlas.version() > before);
        let rect = atlas.rect("t");
        assert_eq!((rect.w, rect.h), (40, 20));
        assert!(!overlaps(rect, atlas.rect("o")));
    }

    #[test]
    fn removing_a_texture_returns_its_space() {
        let mut atlas = AtlasLayout::new(64);
        // One 60 x 60 texture fills the page (with padding and the error texture, almost).
        let big = image(40, 40, 1);
        atlas.sync(&list(&[("a", &big)]), &mut Vec::new());
        let before = atlas.pages();
        atlas.sync(&TextureList::default(), &mut Vec::new());
        assert_eq!(atlas.rect("a"), atlas.rect(ERROR_LABEL));
        let again = image(40, 40, 2);
        atlas.sync(&list(&[("b", &again)]), &mut Vec::new());
        assert_eq!(atlas.pages(), before, "the freed space was reused");
    }

    #[test]
    fn oversized_and_empty_textures_show_the_error_texture() {
        let mut atlas = AtlasLayout::new(64);
        let huge = image(100, 10, 1);
        let empty = Arc::new(Image::from_rgba(RgbaBuf::new(0, 0)));
        let mut out = Vec::new();
        atlas.sync(&list(&[("huge", &huge), ("empty", &empty)]), &mut out);
        assert_eq!(atlas.rect("huge"), atlas.rect(ERROR_LABEL));
        assert_eq!(atlas.rect("empty"), atlas.rect(ERROR_LABEL));
        assert_eq!(out.len(), 1, "only the error texture uploads");
        out.clear();
        atlas.sync(&list(&[("huge", &huge), ("empty", &empty)]), &mut out);
        assert!(
            out.is_empty(),
            "an unplaceable texture is not retried every frame"
        );
    }

    /// A strip of `tiles` frames of 4 x 4 texels; every byte of tile `t` (from the top) is
    /// `(t + 1) * 10`.
    fn strip(tiles: u32) -> Arc<Image> {
        let mut buf = RgbaBuf::new(4, 4 * tiles);
        for tile in 0..tiles {
            let first = (tiles - 1 - tile) * 4;
            let at = (first * 4 * 4) as usize;
            buf.pixels[at..at + 4 * 4 * 4].fill(u8::try_from((tile + 1) * 10).unwrap());
        }
        Arc::new(Image::from_rgba(buf))
    }

    fn animated(image: &Arc<Image>, edit: impl FnOnce(&mut TextureAnimation)) -> TextureList {
        let mut animation = TextureAnimation::new(4, 4);
        edit(&mut animation);
        let mut list = list(&[("water", image)]);
        list.entries[0].animation = Some(animation);
        list
    }

    /// The byte of the first texel inside the padding of an upload.
    fn first_texel(update: &AtlasUpdate) -> u8 {
        let at = ((update.w + 1) * 4) as usize;
        update.rgba[at]
    }

    #[test]
    fn an_animated_texture_has_the_rectangle_of_a_frame_and_uploads_the_tile_it_shows() {
        let image = strip(3);
        let tex = animated(&image, |a| a.frametime = 2);
        let mut atlas = AtlasLayout::new(256);
        let mut out = Vec::new();
        atlas.sync(&tex, &mut out);
        let rect = atlas.rect("water");
        assert_eq!((rect.w, rect.h), (4, 4), "one frame, not the strip");
        let uploads: Vec<(u32, u32, u8)> = out
            .iter()
            .skip(1)
            .map(|u| (u.w, u.h, first_texel(u)))
            .collect();
        assert_eq!(uploads, [(6, 6, 10)], "the first tile, with its padding");

        let version = atlas.version();
        let shown = |tick: u32, atlas: &mut AtlasLayout| {
            atlas.set_tick(tick);
            let mut out = Vec::new();
            atlas.sync(&tex, &mut out);
            out.iter().map(first_texel).collect::<Vec<u8>>()
        };
        // Two ticks a tile: nothing new at tick 1, the second tile at tick 2.
        assert_eq!(shown(0, &mut atlas), Vec::<u8>::new());
        assert_eq!(shown(1, &mut atlas), Vec::<u8>::new());
        assert_eq!(shown(2, &mut atlas), [20]);
        assert_eq!(shown(3, &mut atlas), Vec::<u8>::new());
        assert_eq!(shown(4, &mut atlas), [30]);
        assert_eq!(shown(6, &mut atlas), [10], "the cycle starts again");
        assert_eq!(atlas.rect("water"), rect, "the rectangle never moved");
        assert_eq!(atlas.version(), version);
    }

    #[test]
    fn the_rectangle_that_changes_is_the_frame_rectangle_only() {
        let image = strip(3);
        let tex = animated(&image, |_| {});
        let mut atlas = AtlasLayout::new(256);
        atlas.sync(&tex, &mut Vec::new());
        atlas.set_tick(1);
        let mut out = Vec::new();
        atlas.sync(&tex, &mut out);
        let rect = atlas.rect("water");
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].x + 1, out[0].y + 1), (rect.x, rect.y));
        assert_eq!(
            out[0].rgba.len(),
            6 * 6 * 4,
            "a frame and its padding, not the strip"
        );
    }

    #[test]
    fn interpolating_uploads_the_blend_of_two_tiles_every_tick() {
        let image = strip(3);
        let tex = animated(&image, |a| {
            a.frametime = 4;
            a.interpolate = true;
        });
        let mut atlas = AtlasLayout::new(256);
        atlas.sync(&tex, &mut Vec::new());
        let mut texel = |tick: u32| {
            atlas.set_tick(tick);
            let mut out = Vec::new();
            atlas.sync(&tex, &mut out);
            out.iter().map(first_texel).collect::<Vec<u8>>()
        };
        // Tile 0 is 10 and tile 1 is 20, so (10 * (4 - e) + 20 * e + 2) / 4 after e ticks.
        assert_eq!(texel(0), Vec::<u8>::new());
        assert_eq!(texel(1), [13]);
        assert_eq!(texel(2), [15]);
        assert_eq!(texel(3), [18]);
        assert_eq!(texel(4), [20]);
        // The last tile blends into the first: three ticks in, (30 * 1 + 10 * 3 + 2) / 4.
        assert_eq!(texel(11), [15]);
    }

    #[test]
    fn a_new_tick_does_not_make_the_faces_rebuild_but_a_new_picture_does() {
        let mut buf = RgbaBuf::new(4, 8);
        buf.pixels.fill(100);
        buf.pixels[3] = 128;
        let image = Arc::new(Image::from_rgba(buf));
        let tex = animated(&image, |_| {});
        let mut atlas = AtlasLayout::new(256);
        atlas.sync(&tex, &mut Vec::new());
        let generation = atlas.generation();
        atlas.set_tick(1);
        atlas.sync(&tex, &mut Vec::new());
        assert_eq!(atlas.generation(), generation);
        // Partial alpha in any frame counts for every face, whatever the tile.
        assert!(atlas.any_partial("water", Vec2::ZERO, Vec2::splat(0.5)));

        let mut again = RgbaBuf::new(4, 8);
        again.pixels.fill(255);
        let replaced = animated(&Arc::new(Image::from_rgba(again)), |_| {});
        atlas.sync(&replaced, &mut Vec::new());
        assert!(atlas.generation() > generation);
        assert!(!atlas.any_partial("water", Vec2::ZERO, Vec2::ONE));
    }

    #[test]
    fn a_strip_that_finds_no_room_is_not_tried_again_on_every_tick() {
        let mut buf = RgbaBuf::new(128, 256);
        buf.pixels.fill(50);
        let image = Arc::new(Image::from_rgba(buf));
        let mut tex = list(&[("water", &image)]);
        tex.entries[0].animation = Some(TextureAnimation::new(128, 128));
        // A frame of 128 texels does not fit a page of 64.
        let mut atlas = AtlasLayout::new(64);
        atlas.sync(&tex, &mut Vec::new());
        let before = (atlas.version(), atlas.generation());
        for tick in 1..5 {
            atlas.set_tick(tick);
            let mut out = Vec::new();
            atlas.sync(&tex, &mut out);
            assert!(out.is_empty(), "tick {tick}");
        }
        assert_eq!((atlas.version(), atlas.generation()), before);
    }

    #[test]
    fn a_tick_that_shows_the_same_picture_uploads_nothing() {
        let one = strip(1);
        let two = strip(2);
        let held = vec![
            cuboid_model::FrameStep::of(0),
            cuboid_model::FrameStep::of(0),
            cuboid_model::FrameStep::of(1),
        ];
        // One frame that blends into itself, and a tile held over two steps.
        for tex in [
            animated(&one, |a| {
                a.frametime = 4;
                a.interpolate = true;
            }),
            animated(&two, |a| a.frames = Some(held.clone())),
        ] {
            let mut atlas = AtlasLayout::new(256);
            atlas.sync(&tex, &mut Vec::new());
            atlas.set_tick(1);
            let mut out = Vec::new();
            atlas.sync(&tex, &mut out);
            assert!(out.is_empty());
        }
    }

    #[test]
    fn taking_the_animation_away_gives_the_whole_picture_a_rectangle_again() {
        let image = strip(3);
        let mut tex = animated(&image, |_| {});
        let mut atlas = AtlasLayout::new(256);
        atlas.sync(&tex, &mut Vec::new());
        let version = atlas.version();
        tex.entries[0].animation = None;
        let mut out = Vec::new();
        atlas.sync(&tex, &mut out);
        let rect = atlas.rect("water");
        assert_eq!((rect.w, rect.h), (4, 12));
        assert_eq!(out.len(), 1);
        assert!(atlas.version() > version);
    }
}
