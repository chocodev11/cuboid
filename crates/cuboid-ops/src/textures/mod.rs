//! Texture list operations: create, remove and assign to faces (spec: tools 5.1, 5.7, 5.9, 10.8).

use std::sync::Arc;

use cuboid_image::{Image, RgbaBuf, check_size};
use cuboid_model::{
    ChangeFlags, DocFlags, EditCtx, FaceName, Label, ObjRef, Rev, Texture, TextureEntry,
    TextureList,
};

use crate::OpError;

/// The label rule: only ASCII letters, digits, `-` and `_` are kept.
///
/// spec: tools 5.9
pub fn sanitize_label(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

fn filled(w: u32, h: u32, color: [u8; 4]) -> Result<Arc<Image>, OpError> {
    if w == 0 || h == 0 || check_size(w, h).is_err() {
        return Err(OpError::TextureSize { w, h });
    }
    let mut buf = RgbaBuf::new(w, h);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&color);
    }
    Ok(Arc::new(Image::from_rgba(buf)))
}

/// The list of a new model: `particle` and `texture`, 16 x 16 mid-grey checkerboards.
///
/// spec: tools 5.1
pub fn default_list() -> TextureList {
    let entry = |label: &str, dark: u8, light: u8| {
        let mut buf = RgbaBuf::new(16, 16);
        for y in 0..16_i32 {
            for x in 0..16_i32 {
                let v = if (x / 4 + y / 4) % 2 == 0 {
                    dark
                } else {
                    light
                };
                buf.set(x, y, [v, v, v, 255]);
            }
        }
        TextureEntry {
            label: Label::from(label),
            texture: Texture::new(Arc::new(Image::from_rgba(buf))),
        }
    };
    TextureList {
        entries: vec![entry("particle", 96, 128), entry("texture", 128, 160)],
    }
}

/// The first of `particle`, `texture`, `texture1`, `texture2`, ... that the list lacks.
///
/// spec: tools 5.9
fn free_label(list: &TextureList) -> String {
    let taken = |name: &str| list.entries.iter().any(|e| &*e.label == name);
    ["particle".to_owned(), "texture".to_owned()]
        .into_iter()
        .chain((1..).map(|n| format!("texture{n}")))
        .find(|name| !taken(name))
        .unwrap_or_default()
}

/// Appends a texture of `w` x `h` texels filled with `color`. Without a label the first free
/// default name is used; a given label goes through the label rule and must be new. Returns the
/// label.
///
/// spec: tools 5.7 ("+"), tools 5.9
pub fn create(
    cx: &mut EditCtx<'_>,
    label: Option<&str>,
    w: u32,
    h: u32,
    color: [u8; 4],
) -> Result<Label, OpError> {
    let name = match label {
        Some(text) => {
            let name = sanitize_label(text);
            if name.is_empty() {
                return Err(OpError::BadLabel(text.to_owned()));
            }
            if cx.doc.textures.entries.iter().any(|e| *e.label == *name) {
                return Err(OpError::TextureExists(name));
            }
            name
        }
        None => free_label(&cx.doc.textures),
    };
    let image = filled(w, h, color)?;
    let label = Label::from(name);
    cx.doc.textures.entries.push(TextureEntry {
        label: label.clone(),
        texture: Texture::new(image),
    });
    cx.doc
        .journal
        .mark_doc(DocFlags::TEXTURES | DocFlags::ATLAS);
    cx.doc.revision = Rev::next();
    Ok(label)
}

/// Removes the entry with this label. Faces that name it keep the label and draw the error
/// texture.
///
/// spec: tools 5.1 (Deleting a texture)
pub fn remove(cx: &mut EditCtx<'_>, label: &str) -> Result<(), OpError> {
    let Some(at) = cx
        .doc
        .textures
        .entries
        .iter()
        .position(|e| *e.label == *label)
    else {
        return Err(OpError::NoTexture(label.to_owned()));
    };
    cx.doc.textures.entries.remove(at);
    cx.doc
        .journal
        .mark_doc(DocFlags::TEXTURES | DocFlags::ATLAS);
    cx.doc.revision = Rev::next();
    Ok(())
}

/// Replaces the pixels of the entry with this label. The label, the image name and the
/// Minecraft path stay, so the faces that use the entry keep it. `file` is the signature and the
/// length of the file the pixels were read from, which the entry then records; `None` leaves the
/// provenance as it is. Nothing changes, and the revision stays, when the entry already holds
/// these pixels and this provenance.
///
/// spec: tools 5.10 (entry load), tools 5.12 (reload from disk)
pub fn replace(
    cx: &mut EditCtx<'_>,
    label: &str,
    pixels: RgbaBuf,
    file: Option<(&str, u64)>,
) -> Result<(), OpError> {
    let (w, h) = (pixels.w, pixels.h);
    if w == 0 || h == 0 || check_size(w, h).is_err() {
        return Err(OpError::TextureSize { w, h });
    }
    let Some(entry) = cx
        .doc
        .textures
        .entries
        .iter_mut()
        .find(|e| *e.label == *label)
    else {
        return Err(OpError::NoTexture(label.to_owned()));
    };
    let texture = &mut entry.texture;
    let same_pixels =
        texture.image.w == w && texture.image.h == h && texture.image.rgba() == pixels.pixels;
    let same_file = file.is_none_or(|(signature, size)| {
        texture.source.signature.as_deref() == Some(signature) && texture.source.file_size == size
    });
    if same_pixels && same_file {
        return Ok(());
    }
    if !same_pixels {
        texture.image = Arc::new(Image::from_rgba(pixels));
    }
    if let Some((signature, size)) = file {
        texture.source.signature = Some(signature.to_owned());
        texture.source.file_size = size;
    }
    cx.doc
        .journal
        .mark_doc(DocFlags::TEXTURES | DocFlags::ATLAS);
    cx.doc.revision = Rev::next();
    Ok(())
}

/// Points the given faces (all six when `faces` is empty) of every element among `objs` at the
/// texture `label`, which must exist. Returns how many elements changed.
///
/// spec: tools 5.7 (Apply to selection), tools 10.8
pub fn assign(
    cx: &mut EditCtx<'_>,
    objs: &[ObjRef],
    label: &str,
    faces: &[FaceName],
) -> Result<usize, OpError> {
    let Some(entry) = cx.doc.textures.entries.iter().find(|e| *e.label == *label) else {
        return Err(OpError::NoTexture(label.to_owned()));
    };
    let label = entry.label.clone();
    let wanted = |name: FaceName| faces.is_empty() || faces.contains(&name);
    let mut changed = 0;
    for &obj in objs {
        let ObjRef::Element(id) = obj else {
            continue;
        };
        let differs = cx.doc.elements.get(id).is_some_and(|el| {
            el.faces
                .iter()
                .any(|f| wanted(f.name) && f.texture != label)
        });
        if !differs {
            continue;
        }
        let doc = &mut *cx.doc;
        if let Some(el) = doc
            .elements
            .edit_marked(id, ChangeFlags::FACES, &mut doc.journal)
        {
            for face in el.faces.iter_mut().filter(|f| wanted(f.name)) {
                face.texture = label.clone();
            }
            changed += 1;
        }
    }
    if changed > 0 {
        cx.doc.revision = Rev::next();
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_keep_only_the_allowed_characters() {
        assert_eq!(sanitize_label("my tex-1_é!"), "mytex-1_");
        assert_eq!(sanitize_label("???"), "");
    }

    #[test]
    fn default_names_skip_the_ones_in_use() {
        let mut list = default_list();
        assert_eq!(free_label(&list), "texture1");
        list.entries.remove(0);
        assert_eq!(free_label(&list), "particle");
    }

    #[test]
    fn the_default_list_has_particle_and_texture() {
        let list = default_list();
        let labels: Vec<_> = list.entries.iter().map(|e| e.label.to_string()).collect();
        assert_eq!(labels, ["particle", "texture"]);
        assert_eq!(list.default_face_label().to_string(), "texture");
    }
}
