//! Texture list operations: create, remove and assign to faces (spec: tools 5.1, 5.7, 5.9, 10.8).

use std::sync::Arc;

use cuboid_image::{Image, RgbaBuf, check_size};
use cuboid_model::{
    ChangeFlags, DocFlags, EditCtx, FaceName, FrameStep, FramesError, Label, MAX_STEPS, MAX_TILES,
    ObjRef, Rev, Texture, TextureAnimation, TextureEntry, TextureList,
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
            animation: None,
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
        animation: None,
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
        // Frames of the old picture do not describe a picture of another size.
        if entry
            .animation
            .as_ref()
            .is_some_and(|a| a.check(w, h).is_err())
        {
            entry.animation = None;
        }
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

/// Gives the entry with this label an animation, or takes it away with `None`. The animation must
/// describe the picture. The text of the file members that the model keeps stays with the entry.
/// Nothing changes, and the revision stays, when the entry already has this animation.
///
/// spec: textures T1 5
pub fn set_animation(
    cx: &mut EditCtx<'_>,
    label: &str,
    animation: Option<TextureAnimation>,
) -> Result<(), OpError> {
    let Some(entry) = cx
        .doc
        .textures
        .entries
        .iter_mut()
        .find(|e| *e.label == *label)
    else {
        return Err(OpError::NoTexture(label.to_owned()));
    };
    let mut animation = animation;
    if let Some(animation) = &mut animation {
        animation.check(entry.texture.image.w, entry.texture.image.h)?;
        animation.extra = entry.animation.as_ref().and_then(|a| a.extra.clone());
    }
    if entry.animation == animation {
        return Ok(());
    }
    entry.animation = animation;
    cx.doc
        .journal
        .mark_doc(DocFlags::TEXTURES | DocFlags::ATLAS);
    cx.doc.revision = Rev::next();
    Ok(())
}

/// Appends a copy of a tile to the end of the strip of the entry with this label, which grows its
/// picture by a frame. An animation with a play order gets a step for the new tile. Returns the
/// index of the new tile.
///
/// spec: textures T1 5
pub fn add_frame(cx: &mut EditCtx<'_>, label: &str, tile: u32) -> Result<u32, OpError> {
    let Some(entry) = cx
        .doc
        .textures
        .entries
        .iter_mut()
        .find(|e| *e.label == *label)
    else {
        return Err(OpError::NoTexture(label.to_owned()));
    };
    let image = &entry.texture.image;
    let Some(animation) = &mut entry.animation else {
        return Err(OpError::NoAnimation(label.to_owned()));
    };
    let tiles = animation.tiles(image.h);
    if tile >= tiles {
        return Err(FramesError::NoSuchTile.into());
    }
    let grown = (image.w, image.h + animation.frame_height);
    if tiles >= MAX_TILES || check_size(grown.0, grown.1).is_err() {
        return Err(FramesError::TooManyTiles.into());
    }
    if animation
        .frames
        .as_ref()
        .is_some_and(|s| s.len() >= MAX_STEPS)
    {
        return Err(FramesError::Steps.into());
    }
    // The end of the strip is the bottom of the picture, which is the start of the data.
    let rows = animation.tile_rows(image.h, tile);
    let row = image.w as usize * 4;
    let old = image.rgba();
    let copy = old.get(rows.start as usize * row..rows.end as usize * row);
    let mut pixels = Vec::with_capacity(old.len() + row * animation.frame_height as usize);
    pixels.extend_from_slice(copy.unwrap_or_default());
    pixels.extend_from_slice(old);
    if let Some(steps) = &mut animation.frames {
        steps.push(FrameStep::of(tiles));
    }
    entry.texture.image = Arc::new(Image::from_rgba(RgbaBuf {
        w: grown.0,
        h: grown.1,
        pixels,
    }));
    cx.doc
        .journal
        .mark_doc(DocFlags::TEXTURES | DocFlags::ATLAS);
    cx.doc.revision = Rev::next();
    Ok(tiles)
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
    use cuboid_model::{Document, EditEnv, RenderPrefs, Selection};
    use cuboid_num::EngineRng;

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

    /// A document whose `water` texture is a strip of three 2 x 2 frames, tile `t` from the top
    /// filled with the byte `t + 1`.
    fn strip_doc() -> Document {
        let mut buf = RgbaBuf::new(2, 6);
        for tile in 0..3_u8 {
            let first = (2 - usize::from(tile)) * 2 * 2 * 4;
            buf.pixels[first..first + 2 * 2 * 4].fill(tile + 1);
        }
        let mut doc = Document::new(TextureList {
            entries: vec![TextureEntry {
                label: Label::from("water"),
                texture: Texture::new(Arc::new(Image::from_rgba(buf))),
                animation: Some(TextureAnimation::new(2, 2)),
            }],
        });
        doc.textures.entries.push(TextureEntry {
            label: Label::from("stone"),
            texture: Texture::new(Arc::new(Image::from_rgba(RgbaBuf::new(2, 2)))),
            animation: None,
        });
        doc
    }

    fn with_cx<R>(doc: &mut Document, run: impl FnOnce(&mut EditCtx<'_>) -> R) -> R {
        let env = EditEnv {
            restrict_rotations: true,
            render: RenderPrefs::default(),
            code_page: 1252,
            allow_negative_size: false,
        };
        let (mut sel, mut rng) = (Selection::default(), EngineRng::seeded(5));
        run(&mut EditCtx {
            doc,
            sel: &mut sel,
            env: &env,
            rng: &mut rng,
        })
    }

    #[test]
    fn a_frame_is_added_at_the_bottom_of_the_strip_and_the_tiles_keep_their_pictures() {
        let mut doc = strip_doc();
        let before = doc.textures.entries[0].texture.image.clone();
        let tile = with_cx(&mut doc, |cx| add_frame(cx, "water", 1)).expect("added");
        assert_eq!(tile, 3);
        let entry = &doc.textures.entries[0];
        let image = &entry.texture.image;
        assert_eq!((image.w, image.h), (2, 8));
        assert!(!Arc::ptr_eq(image, &before));
        let animation = entry.animation.as_ref().expect("still animated");
        // The new tile is a copy of tile 1; the others are where they were from the top.
        let bytes: Vec<u8> = (0..4)
            .map(|t| image.rgba()[animation.tile_rows(8, t).start as usize * 8])
            .collect();
        assert_eq!(bytes, [1, 2, 3, 2]);
        assert_eq!(animation.frames, None, "a plain loop includes the new tile");
    }

    #[test]
    fn a_frame_joins_an_explicit_play_order_and_the_limits_hold() {
        let mut doc = strip_doc();
        doc.textures.entries[0].animation.as_mut().unwrap().frames =
            Some(vec![FrameStep::of(2), FrameStep::of(0)]);
        with_cx(&mut doc, |cx| add_frame(cx, "water", 0)).expect("added");
        let steps = doc.textures.entries[0]
            .animation
            .as_ref()
            .unwrap()
            .frames
            .clone();
        assert_eq!(
            steps,
            Some(vec![FrameStep::of(2), FrameStep::of(0), FrameStep::of(3)])
        );

        let mut refused =
            |label: &str, tile: u32| with_cx(&mut doc, |cx| add_frame(cx, label, tile));
        assert!(matches!(
            refused("water", 4),
            Err(OpError::Frames(FramesError::NoSuchTile))
        ));
        assert!(matches!(refused("stone", 0), Err(OpError::NoAnimation(_))));
        assert!(matches!(refused("nope", 0), Err(OpError::NoTexture(_))));
    }

    #[test]
    fn an_animation_is_set_checked_and_taken_away() {
        let mut doc = strip_doc();
        let mut slow = TextureAnimation::new(2, 2);
        slow.frametime = 4;
        with_cx(&mut doc, |cx| {
            set_animation(cx, "water", Some(slow.clone()))
        })
        .expect("set");
        assert_eq!(doc.textures.entries[0].animation, Some(slow.clone()));

        // A still texture of a whole number of frames becomes a strip, and one frame is a strip.
        with_cx(&mut doc, |cx| {
            set_animation(cx, "stone", Some(TextureAnimation::new(2, 2)))
        })
        .expect("one frame");
        assert!(doc.textures.entries[1].animation.is_some());

        // The text of the file that the entry keeps survives a new animation.
        doc.textures.entries[0].animation.as_mut().unwrap().extra = Some(Arc::from("{\"a\":1}"));
        with_cx(&mut doc, |cx| {
            set_animation(cx, "water", Some(slow.clone()))
        })
        .expect("set");
        let kept = doc.textures.entries[0]
            .animation
            .as_ref()
            .unwrap()
            .extra
            .clone();
        assert_eq!(kept.as_deref(), Some("{\"a\":1}"));

        // The same animation changes nothing.
        let rev = doc.revision;
        with_cx(&mut doc, |cx| set_animation(cx, "water", doc_animation(cx))).expect("same");
        assert_eq!(doc.revision, rev);

        let wrong = TextureAnimation::new(2, 4);
        let result = with_cx(&mut doc, |cx| set_animation(cx, "water", Some(wrong)));
        assert!(matches!(result, Err(OpError::Frames(FramesError::Size))));
        with_cx(&mut doc, |cx| set_animation(cx, "water", None)).expect("removed");
        assert!(doc.textures.entries[0].animation.is_none());
    }

    fn doc_animation(cx: &EditCtx<'_>) -> Option<TextureAnimation> {
        cx.doc.textures.entries[0].animation.clone()
    }

    #[test]
    fn a_new_picture_of_another_size_ends_the_animation() {
        let mut doc = strip_doc();
        // The same size keeps it, another one cannot.
        let same = RgbaBuf::new(2, 6);
        with_cx(&mut doc, |cx| replace(cx, "water", same, None)).expect("replaced");
        assert!(doc.textures.entries[0].animation.is_some());
        let other = RgbaBuf::new(2, 5);
        with_cx(&mut doc, |cx| replace(cx, "water", other, None)).expect("replaced");
        assert!(doc.textures.entries[0].animation.is_none());
    }
}
