//! The animated textures in the `cuboid.json` entry: the payload holds the first frame, the entry
//! the strip, and a payload that was changed behind the entry drops the strip.

mod common;

use std::io::Cursor;
use std::sync::Arc;

use common::*;
use cuboid_fmt_cubik::container;
use cuboid_fmt_cubik::{Limits, SaveOptions, load_bytes, save_bytes};
use cuboid_model::image::{Image, RgbaBuf};
use cuboid_model::{Document, FrameStep, Selection, TextureAnimation};

fn parts(file: Vec<u8>) -> (Vec<u8>, Option<Vec<u8>>) {
    let c = container::read(Cursor::new(file), &Limits::default()).unwrap();
    (c.payload, c.extension)
}

fn save(doc: &Document, sel: &Selection) -> Vec<u8> {
    save_bytes(
        &snapshot(doc, sel),
        doc.retained.as_deref(),
        &[],
        &SaveOptions::default(),
    )
    .unwrap()
}

/// The sample document, whose `particle` texture (16 x 16, RGB24 with a mip chain) becomes the
/// first of three frames of a strip: the other two are solid.
fn animated() -> (Document, Selection) {
    let (mut doc, sel) = open(common::load_bytes(&full_payload()));
    let entry = &mut doc.textures.entries[0];
    assert_eq!(&*entry.label, "particle");
    let first = entry.texture.image.rgba().to_vec();
    let mut strip = RgbaBuf::new(16, 48);
    // Rows run bottom to top: tile 0 is the top third, the last rows.
    for (tile, color) in [(1_usize, [200_u8, 10, 10, 255]), (2, [10, 10, 200, 255])] {
        let at = (2 - tile) * 16 * 16 * 4;
        for texel in strip.pixels[at..at + 16 * 16 * 4].chunks_exact_mut(4) {
            texel.copy_from_slice(&color);
        }
    }
    strip.pixels[2 * 16 * 16 * 4..].copy_from_slice(&first);
    entry.texture.image = Arc::new(Image::from_rgba(strip));
    let mut animation = TextureAnimation::new(16, 16);
    animation.frametime = 3;
    animation.interpolate = true;
    animation.frames = Some(vec![
        FrameStep::of(0),
        FrameStep {
            index: 2,
            time: Some(9),
        },
        FrameStep::of(1),
    ]);
    animation.extra = Some(Arc::from("{\"note\":[1]}"));
    entry.animation = Some(animation);
    (doc, sel)
}

#[test]
fn the_strip_goes_into_the_entry_and_the_payload_holds_the_first_frame() {
    let (doc, sel) = animated();
    let (payload, entry) = parts(save(&doc, &sel));
    let entry = entry.expect("an entry");
    assert!(std::str::from_utf8(&entry).unwrap().contains("\"strips\""));

    // The payload is the one of the model with the still texture, mip chain and all.
    let (still, still_sel) = open(common::load_bytes(&full_payload()));
    assert_eq!(
        payload,
        save_payload(&still, &still_sel, &SaveOptions::default())
    );
    assert_eq!(payload, full_payload());
}

#[test]
fn the_strip_and_the_animation_come_back() {
    let (doc, sel) = animated();
    let loaded = load_bytes(&save(&doc, &sel), &Limits::default()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let (back, _) = open(loaded);
    let (was, now) = (&doc.textures.entries[0], &back.textures.entries[0]);
    assert_eq!(was.animation, now.animation);
    assert_eq!((now.texture.image.w, now.texture.image.h), (16, 48));
    assert_eq!(was.texture.image.rgba(), now.texture.image.rgba());
    // The other textures are untouched.
    assert!(back.textures.entries[1].animation.is_none());
    assert_eq!(back.textures.entries[1].texture.image.w, 3);
    cuboid_model::invariants::check_loaded_invariants(&back, &Selection::default()).unwrap();
}

#[test]
fn a_loaded_file_saves_the_same_payload_and_the_same_entry() {
    let (doc, sel) = animated();
    let file = save(&doc, &sel);
    let (back, back_sel) = open(load_bytes(&file, &Limits::default()).unwrap());
    let again = save(&back, &back_sel);
    let (first, second) = (parts(file), parts(again));
    assert_eq!(first.0, second.0, "the payload");
    assert_eq!(first.1, second.1, "the entry");
}

#[test]
fn a_payload_texture_that_was_changed_drops_the_strip_with_a_warning() {
    let (doc, sel) = animated();
    let (_, entry) = parts(save(&doc, &sel));
    // The payload as another program left it: the particle texture has another picture.
    let (mut other, other_sel) = open(common::load_bytes(&full_payload()));
    let mut buf = RgbaBuf::new(16, 16);
    buf.pixels.fill(77);
    other.textures.entries[0].texture.image = Arc::new(Image::from_rgba(buf));
    let payload = save_payload(&other, &other_sel, &SaveOptions::default());
    let file = container::write(&payload, entry.as_deref()).unwrap();

    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    assert!(
        loaded.warnings[0].contains("animated textures"),
        "{:?}",
        loaded.warnings
    );
    let (back, _) = open(loaded);
    let entry = &back.textures.entries[0];
    assert!(entry.animation.is_none());
    assert_eq!((entry.texture.image.w, entry.texture.image.h), (16, 16));
    assert!(entry.texture.image.rgba().iter().all(|b| *b == 77));
}

#[test]
fn a_damaged_strip_is_left_out_with_a_warning() {
    let (doc, sel) = animated();
    let (payload, entry) = parts(save(&doc, &sel));
    let text = String::from_utf8(entry.unwrap()).unwrap();
    for (what, damaged) in [
        (
            "a png that is no picture",
            text.replacen("\"png\":\"", "\"png\":\"AAAA", 1),
        ),
        (
            "a frame height that does not divide",
            text.replacen("\"frame_height\":16", "\"frame_height\":10", 1),
        ),
        (
            "a member of another type",
            text.replacen("\"frametime\":3", "\"frametime\":\"x\"", 1),
        ),
    ] {
        assert_ne!(damaged, text, "{what}");
        let file = container::write(&payload, Some(damaged.as_bytes())).unwrap();
        let loaded = load_bytes(&file, &Limits::default()).unwrap();
        assert_eq!(loaded.warnings.len(), 1, "{what}: {:?}", loaded.warnings);
        let (back, _) = open(loaded);
        assert!(back.textures.entries[0].animation.is_none(), "{what}");
    }
}

#[test]
fn a_file_without_an_animated_texture_has_no_strips_member() {
    let (doc, sel) = open(common::load_bytes(&full_payload()));
    let (_, entry) = parts(save(&doc, &sel));
    assert!(entry.is_none_or(|e| !String::from_utf8_lossy(&e).contains("strips")));
}
