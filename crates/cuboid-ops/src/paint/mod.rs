//! Paint operations on textures: stroke segments, flood fill and colour picking, computed on a
//! copy of the touched region only (spec: tools 13.1, 13.2, 13.4).
//!
//! A stroke step is planned as a [`Patch`] against the current texels and applied afterwards, so
//! the caller learns whether anything would change before it records an undo step, and a step
//! costs the size of the region it touches, never the size of the texture.

use std::sync::Arc;

use cuboid_image::raster::{self, TexelRect};
use cuboid_image::{Image, RgbaBuf};
use cuboid_model::{ChangeFlags, DocFlags, Document, EditCtx, FaceName, Label, ObjId, Rev};
use cuboid_num::{pack::to_byte, trunc_i32};
use glam::Vec2;

/// The default most texels a flood fill may change; a larger region leaves the texture alone.
pub const FILL_LIMIT: usize = 1 << 22;

/// The largest brush diameter in texels.
pub const MAX_BRUSH: i32 = 128;

/// The longest stretch of a stroke that one step draws. A longer move is cut into steps of this
/// size, so no step touches a region larger than a few thousand texels.
pub const MAX_STEP: i32 = 64;

/// Why a paint operation refused. The texture is untouched in every case.
#[derive(Debug, thiserror::Error)]
pub enum PaintError {
    /// A flood fill region is larger than the limit.
    #[error("more than {0} texels would change, nothing was changed")]
    TooLarge(usize),
}

/// A texture of the document: an entry of the texture list or the texture of a mesh.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TexKey {
    /// The entry with this label in the texture list.
    Entry(Label),
    /// The main texture of this mesh.
    Mesh(ObjId),
}

impl TexKey {
    /// The texture a face of an element shows; `None` when the element, the face or the label
    /// is not in the document.
    pub fn of_face(doc: &Document, element: ObjId, face: FaceName) -> Option<TexKey> {
        let label = &doc.elements.get(element)?.faces.get(face as usize)?.texture;
        doc.textures
            .entries
            .iter()
            .any(|e| e.label == *label)
            .then(|| TexKey::Entry(label.clone()))
    }

    /// The main texture of a mesh; `None` when the mesh has none.
    pub fn of_mesh(doc: &Document, mesh: ObjId) -> Option<TexKey> {
        doc.meshes.get(mesh)?.texture.as_ref()?;
        Some(TexKey::Mesh(mesh))
    }

    /// The image of the texture.
    pub fn image<'a>(&self, doc: &'a Document) -> Option<&'a Arc<Image>> {
        match self {
            TexKey::Entry(label) => doc
                .textures
                .entries
                .iter()
                .find(|e| e.label == *label)
                .map(|e| &e.texture.image),
            TexKey::Mesh(id) => doc
                .meshes
                .get(*id)?
                .texture
                .as_ref()
                .map(|texture| &texture.image),
        }
    }
}

/// The texels of an image, read only.
#[derive(Copy, Clone)]
pub struct View<'a> {
    /// Width in texels.
    pub w: u32,
    /// Height in texels.
    pub h: u32,
    /// `w * h * 4` bytes, rows from the bottom.
    pub rgba: &'a [u8],
}

impl<'a> View<'a> {
    /// The level 0 texels of an image.
    pub fn of(image: &'a Image) -> Self {
        Self {
            w: image.w,
            h: image.h,
            rgba: image.rgba(),
        }
    }

    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        (x < self.w && y < self.h).then(|| (y as usize * self.w as usize + x as usize) * 4)
    }

    /// The colour of a texel; `None` outside the image.
    pub fn get(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        let at = self.offset(x, y)?;
        self.rgba.get(at..)?.first_chunk().copied()
    }

    /// A copy of the texels `[x0, x1) x [y0, y1)`, which lie inside the image.
    fn region(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> RgbaBuf {
        let mut buf = RgbaBuf::new(x1 - x0, y1 - y0);
        let row = (x1 - x0) as usize * 4;
        let rows = buf.pixels.chunks_exact_mut(row.max(1));
        for (y, dst) in (y0..y1).zip(rows) {
            let at = (y as usize * self.w as usize + x0 as usize) * 4;
            if let Some(src) = self.rgba.get(at..at + row) {
                dst.copy_from_slice(src);
            }
        }
        buf
    }
}

/// New texels for a rectangle of a texture.
#[derive(Clone, Debug)]
pub struct Patch {
    /// Column of the lower-left texel.
    pub x: u32,
    /// Row of the lower-left texel.
    pub y: u32,
    /// The texels that replace the rectangle.
    pub texels: RgbaBuf,
}

impl Patch {
    /// The rectangle the patch covers.
    pub fn rect(&self) -> TexelRect {
        let at = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        TexelRect {
            x0: at(self.x),
            y0: at(self.y),
            x1: at(self.x.saturating_add(self.texels.w)),
            y1: at(self.y.saturating_add(self.texels.h)),
        }
    }
}

/// The texel under a texture coordinate: `(trunc(u * W), trunc(v * H))`, pulled inside the
/// texture so the far edge `u = 1` lands on the last texel. `None` for a non-finite coordinate or
/// an empty texture.
///
/// spec: tools 13.2
pub fn texel_at(uv: Vec2, w: u32, h: u32) -> Option<(i32, i32)> {
    if !uv.is_finite() || w == 0 || h == 0 {
        return None;
    }
    let side = |u: f32, n: u32| trunc_i32(u * n as f32).clamp(0, i32::try_from(n - 1).unwrap_or(0));
    Some((side(uv.x, w), side(uv.y, h)))
}

/// The colour of a tool as texel bytes, with the stroke alpha (0 to 255) applied to its alpha.
pub fn tool_color(color: [f32; 4], alpha: u8) -> [u8; 4] {
    let [r, g, b, a] = color.map(to_byte);
    // a * alpha / 255, rounded.
    let a = (u32::from(a) * u32::from(alpha) + 127) / 255;
    [r, g, b, u8::try_from(a).unwrap_or(u8::MAX)]
}

/// Plans a stroke step: a line of `diameter` texels from `from` to `to` in `color` (a dab when
/// both are the same texel). `None` when no texel would change.
///
/// spec: tools 13.2 (Pen, Line, Brush)
pub fn segment(
    src: View<'_>,
    from: (i32, i32),
    to: (i32, i32),
    diameter: i32,
    color: [u8; 4],
) -> Option<Patch> {
    let diameter = diameter.clamp(1, MAX_BRUSH);
    let margin = diameter + 1;
    let lo = |a: i32, b: i32, n: u32| {
        u32::try_from(a.min(b).saturating_sub(margin).max(0))
            .unwrap_or(0)
            .min(n)
    };
    let hi = |a: i32, b: i32, n: u32| {
        u32::try_from(a.max(b).saturating_add(margin + 1).max(0))
            .unwrap_or(0)
            .min(n)
    };
    let (x0, x1) = (lo(from.0, to.0, src.w), hi(from.0, to.0, src.w));
    let (y0, y1) = (lo(from.1, to.1, src.h), hi(from.1, to.1, src.h));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let before = src.region(x0, y0, x1, y1);
    let mut texels = before.clone();
    let (ox, oy) = (
        i32::try_from(x0).unwrap_or(0),
        i32::try_from(y0).unwrap_or(0),
    );
    let shift = |p: (i32, i32)| (p.0 - ox, p.1 - oy);
    let clip = TexelRect::of(&texels);
    raster::line(&mut texels, shift(from), shift(to), diameter, color, clip);
    (texels != before).then_some(Patch {
        x: x0,
        y: y0,
        texels,
    })
}

/// The same colour for the purpose of a fill: equal bytes, or both fully transparent.
fn alike(a: [u8; 4], b: [u8; 4]) -> bool {
    a == b || (a[3] == 0 && b[3] == 0)
}

/// Plans a flood fill: the texels connected (four ways) to `seed` that look like it take
/// `color`. `Ok(None)` when the seed is outside, already has the colour, or nothing changes; an
/// error without changes when the region has more than `limit` texels.
///
/// spec: tools 13.2 (Fill with tolerance 0)
pub fn flood(
    src: View<'_>,
    seed: (i32, i32),
    color: [u8; 4],
    limit: usize,
) -> Result<Option<Patch>, PaintError> {
    let Some(target) = src.get(seed.0, seed.1) else {
        return Ok(None);
    };
    if alike(target, color) {
        return Ok(None);
    }
    // One bit per texel; a texel is marked when it joins the region.
    let mut seen = vec![0_u64; (src.w as usize * src.h as usize).div_ceil(64)];
    let mut join = |x: i32, y: i32| -> bool {
        let Some(at) = src.offset(x, y).map(|offset| offset / 4) else {
            return false;
        };
        let Some(word) = seen.get_mut(at / 64) else {
            return false;
        };
        let bit = 1_u64 << (at % 64);
        let fresh = *word & bit == 0 && src.get(x, y).is_some_and(|c| alike(c, target));
        *word |= bit * u64::from(fresh);
        fresh
    };
    join(seed.0, seed.1);
    // The region is its own queue.
    let mut region = vec![seed];
    let (mut min, mut max) = (seed, seed);
    let mut next = 0;
    while let Some(&(x, y)) = region.get(next) {
        next += 1;
        for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if !join(nx, ny) {
                continue;
            }
            if region.len() >= limit {
                return Err(PaintError::TooLarge(limit));
            }
            region.push((nx, ny));
            min = (min.0.min(nx), min.1.min(ny));
            max = (max.0.max(nx), max.1.max(ny));
        }
    }
    let to_u32 = |v: i32| u32::try_from(v).unwrap_or(0);
    let (x0, y0) = (to_u32(min.0), to_u32(min.1));
    let mut texels = src.region(x0, y0, to_u32(max.0 + 1), to_u32(max.1 + 1));
    for (x, y) in region {
        texels.set(x - min.0, y - min.1, color);
    }
    Ok(Some(Patch {
        x: x0,
        y: y0,
        texels,
    }))
}

/// Writes a patch into the texture and tells the journal. The image is copied first if an undo
/// step still shares it. Returns whether the texture exists.
pub fn apply(cx: &mut EditCtx<'_>, key: &TexKey, patch: &Patch) -> bool {
    let doc = &mut *cx.doc;
    if key.image(doc).is_none() {
        return false;
    }
    let texture = match key {
        TexKey::Entry(label) => doc
            .textures
            .entries
            .iter_mut()
            .find(|e| e.label == *label)
            .map(|e| &mut e.texture),
        TexKey::Mesh(id) => doc
            .meshes
            .edit_marked(*id, ChangeFlags::FACES, &mut doc.journal)
            .and_then(|mesh| mesh.texture.as_mut()),
    };
    let Some(texture) = texture else {
        return false;
    };
    Arc::make_mut(&mut texture.image)
        .edit()
        .blit(&patch.texels, patch.x, patch.y);
    doc.journal.mark_doc(DocFlags::TEXTURES);
    doc.revision = Rev::next();
    true
}

#[cfg(test)]
mod tests;
