use cuboid_image::{Image, RgbaBuf};
use cuboid_model::{
    Document, EditCtx, EditEnv, ElementType, Label, RenderPrefs, Selection, Texture, TextureEntry,
    TextureList,
};
use cuboid_num::EngineRng;
use glam::Vec2;

use super::*;

const INK: [u8; 4] = [200, 30, 40, 255];
const BG: [u8; 4] = [10, 10, 10, 255];
const WALL: [u8; 4] = [250, 250, 250, 255];

fn buf(w: u32, h: u32, color: [u8; 4]) -> RgbaBuf {
    let mut buf = RgbaBuf::new(w, h);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&color);
    }
    buf
}

fn view(buf: &RgbaBuf) -> View<'_> {
    View {
        w: buf.w,
        h: buf.h,
        rgba: &buf.pixels,
    }
}

fn paste(buf: &mut RgbaBuf, patch: &Patch) {
    buf.blit(&patch.texels, patch.x, patch.y);
}

fn count(buf: &RgbaBuf, color: [u8; 4]) -> usize {
    buf.pixels
        .chunks_exact(4)
        .filter(|t| **t == color[..])
        .count()
}

#[test]
fn a_texture_coordinate_picks_its_texel_and_the_far_edge_the_last_one() {
    assert_eq!(texel_at(Vec2::new(0.0, 0.0), 16, 8), Some((0, 0)));
    assert_eq!(texel_at(Vec2::new(0.5, 0.5), 16, 8), Some((8, 4)));
    assert_eq!(texel_at(Vec2::new(0.99, 0.99), 16, 8), Some((15, 7)));
    assert_eq!(texel_at(Vec2::new(1.0, 1.0), 16, 8), Some((15, 7)));
    assert_eq!(texel_at(Vec2::new(-0.2, 3.0), 16, 8), Some((0, 7)));
    assert_eq!(texel_at(Vec2::NAN, 16, 8), None);
    assert_eq!(texel_at(Vec2::ZERO, 0, 8), None);
}

#[test]
fn the_stroke_alpha_scales_the_colour_alpha() {
    assert_eq!(tool_color([1.0, 0.0, 0.5, 1.0], 255), [255, 0, 127, 255]);
    assert_eq!(tool_color([1.0, 1.0, 1.0, 1.0], 128), [255, 255, 255, 128]);
    assert_eq!(tool_color([1.0, 1.0, 1.0, 0.5], 128), [255, 255, 255, 64]);
    assert_eq!(tool_color([1.0, 1.0, 1.0, 0.0], 255)[3], 0);
}

#[test]
fn a_dab_changes_only_the_texels_under_the_brush() {
    let mut texture = buf(32, 32, BG);
    let patch = segment(view(&texture), (10, 10), (10, 10), 1, INK).expect("a change");
    assert!(patch.texels.w < 8 && patch.texels.h < 8, "a small region");
    paste(&mut texture, &patch);
    assert_eq!(count(&texture, INK), 1);
    assert_eq!(texture.get(10, 10), Some(INK));
    // The same colour on the same texel is no change.
    assert!(segment(view(&texture), (10, 10), (10, 10), 1, INK).is_none());
    // A seven texel brush is a disc.
    let patch = segment(view(&texture), (20, 20), (20, 20), 7, WALL).expect("a change");
    paste(&mut texture, &patch);
    let disc = count(&texture, WALL);
    assert!((30..50).contains(&disc), "{disc}");
    assert_eq!(texture.get(20, 20), Some(WALL));
    assert_eq!(texture.get(10, 10), Some(INK), "the first dab is untouched");
}

#[test]
fn a_segment_draws_a_connected_line_clipped_to_the_texture() {
    let mut texture = buf(16, 16, BG);
    let patch = segment(view(&texture), (1, 1), (9, 5), 1, INK).expect("a change");
    paste(&mut texture, &patch);
    assert_eq!(count(&texture, INK), 9, "one texel per column");
    assert_eq!(texture.get(1, 1), Some(INK));
    assert_eq!(texture.get(9, 5), Some(INK));
    // Parts outside the texture are cut off; the rest is drawn.
    let patch = segment(view(&texture), (14, 14), (30, 14), 1, WALL).expect("a change");
    paste(&mut texture, &patch);
    assert_eq!(count(&texture, WALL), 2);
    assert!(segment(view(&texture), (40, 40), (50, 50), 1, WALL).is_none());
}

#[test]
fn a_step_touches_a_region_far_smaller_than_a_big_texture() {
    let texture = buf(2048, 2048, BG);
    let patch = segment(view(&texture), (1000, 1000), (1040, 1030), 3, INK).expect("a change");
    assert!(
        patch.texels.pixels.len() < 60 * 60 * 4,
        "{} bytes",
        patch.texels.pixels.len()
    );
}

#[test]
fn a_flood_fills_the_connected_region_inside_its_walls() {
    // 8 x 8, a wall of WALL at x = 4 splits the texture; the left half is BG.
    let mut texture = buf(8, 8, BG);
    for y in 0..8 {
        texture.set(4, y, WALL);
    }
    let patch = flood(view(&texture), (1, 1), INK, 1000)
        .expect("small")
        .expect("a change");
    paste(&mut texture, &patch);
    assert_eq!(count(&texture, INK), 4 * 8);
    assert_eq!(count(&texture, BG), 3 * 8);
    assert_eq!(count(&texture, WALL), 8);
    // The patch covers the region only.
    assert!(patch.texels.w <= 4);
}

#[test]
fn a_flood_ignores_the_same_colour_and_the_outside() {
    let texture = buf(8, 8, BG);
    assert!(
        flood(view(&texture), (1, 1), BG, 100)
            .expect("small")
            .is_none()
    );
    assert!(
        flood(view(&texture), (-1, 1), INK, 100)
            .expect("small")
            .is_none()
    );
    assert!(
        flood(view(&texture), (1, 8), INK, 100)
            .expect("small")
            .is_none()
    );
}

#[test]
fn a_flood_treats_transparent_texels_as_one_colour() {
    let mut texture = buf(4, 4, [0, 0, 0, 0]);
    texture.set(1, 1, [9, 9, 9, 0]);
    let patch = flood(view(&texture), (0, 0), INK, 100)
        .expect("small")
        .expect("a change");
    paste(&mut texture, &patch);
    assert_eq!(count(&texture, INK), 16);
}

#[test]
fn a_flood_over_the_limit_changes_nothing() {
    let texture = buf(10, 10, BG);
    let result = flood(view(&texture), (0, 0), INK, 99);
    assert!(matches!(result, Err(PaintError::TooLarge(99))));
    let ok = flood(view(&texture), (0, 0), INK, 100)
        .expect("exactly the limit")
        .expect("a change");
    assert_eq!((ok.texels.w, ok.texels.h), (10, 10));
}

#[test]
fn a_flood_is_four_connected() {
    // A diagonal of INK separates the corners; the fill of one corner stays on its side only
    // where the wall is closed diagonally.
    let mut texture = buf(4, 4, BG);
    for i in 0..4 {
        texture.set(i, 3 - i, WALL);
    }
    let patch = flood(view(&texture), (0, 0), INK, 100)
        .expect("small")
        .expect("a change");
    paste(&mut texture, &patch);
    assert_eq!(count(&texture, INK), 6);
    assert_eq!(count(&texture, BG), 6);
}

fn fixture() -> (Document, Selection, EditEnv, EngineRng) {
    let image = Arc::new(Image::from_rgba(buf(8, 8, BG)));
    let list = TextureList {
        entries: vec![TextureEntry {
            label: Label::from("texture"),
            texture: Texture::new(image),
            animation: None,
        }],
    };
    (
        Document::new(list),
        Selection::default(),
        EditEnv {
            restrict_rotations: true,
            render: RenderPrefs::default(),
            code_page: 1252,
            allow_negative_size: false,
        },
        EngineRng::seeded(3),
    )
}

#[test]
fn a_face_names_its_texture_and_a_patch_lands_in_the_image() {
    let (mut doc, mut sel, env, mut rng) = fixture();
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    let id = crate::primitives::add_element(&mut cx, ElementType::Cube);
    let key = TexKey::of_face(cx.doc, id, FaceName::Up).expect("a texture");
    assert_eq!(key, TexKey::Entry(Label::from("texture")));
    assert!(TexKey::of_face(cx.doc, ObjId::next(), FaceName::Up).is_none());

    let before = cx.doc.textures.entries[0].texture.image.clone();
    let patch = segment(
        View::of(key.image(cx.doc).expect("image")),
        (2, 2),
        (2, 2),
        1,
        INK,
    )
    .expect("a change");
    let rev = cx.doc.revision;
    assert!(apply(&mut cx, &key, &patch));
    assert_ne!(cx.doc.revision, rev);
    let image = &cx.doc.textures.entries[0].texture.image;
    assert_eq!(View::of(image).get(2, 2), Some(INK));
    assert_eq!(
        View::of(&before).get(2, 2),
        Some(BG),
        "the shared image is copied, not changed"
    );
    assert!(
        cx.doc
            .journal
            .doc()
            .contains(cuboid_model::DocFlags::TEXTURES)
    );
}

#[test]
fn a_canvas_is_the_frame_the_tick_shows_and_a_stroke_stays_inside_it() {
    let (mut doc, mut sel, env, mut rng) = fixture();
    // Three frames of 8 x 8 texels, two ticks each; tile 0 is the top of the picture, the last
    // rows of the data.
    doc.textures.entries[0].texture.image = Arc::new(Image::from_rgba(buf(8, 24, BG)));
    let mut animation = cuboid_model::TextureAnimation::new(8, 8);
    animation.frametime = 2;
    doc.textures.entries[0].animation = Some(animation);
    let key = TexKey::Entry(Label::from("texture"));
    let top = Canvas::of(&doc, key.clone(), 0).expect("a canvas");
    let middle = Canvas::of(&doc, key.clone(), 2).expect("a canvas");
    assert_eq!((top.y, top.h), (16, 8));
    assert_eq!((middle.y, middle.h), (8, 8));
    assert_eq!(Canvas::of(&doc, key.clone(), 5).map(|c| c.y), Some(0));
    assert_eq!(
        Canvas::of(&doc, key.clone(), 6),
        Some(top.clone()),
        "a cycle is 6 ticks"
    );

    // A fill of the middle frame stays in it and lands on its rows of the strip.
    let patch = flood(middle.view(&doc).expect("view"), (3, 3), INK, FILL_LIMIT)
        .expect("fits")
        .expect("a change");
    assert_eq!((patch.x, patch.y), (0, 0));
    assert_eq!((patch.texels.w, patch.texels.h), (8, 8));
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    assert!(middle.apply(&mut cx, &patch));
    let image = &cx.doc.textures.entries[0].texture.image;
    let at = |y: u32| View::rows(image, y, 1).get(0, 0).expect("a texel");
    assert_eq!(
        [at(0), at(7), at(8), at(15), at(16), at(23)],
        [BG, BG, INK, INK, BG, BG]
    );

    // A fat line at the edge of a frame is cut by the frame: no patch reaches past its rows.
    let view = top.view(cx.doc).expect("view");
    let line = segment(view, (0, 7), (7, 7), 6, INK).expect("a change");
    assert!(line.y + line.texels.h <= 8, "{}", line.y + line.texels.h);

    // A still texture is one canvas of the whole picture.
    cx.doc.textures.entries[0].animation = None;
    let whole = Canvas::of(cx.doc, key, 3).expect("a canvas");
    assert_eq!((whole.y, whole.h), (0, 24));
}

#[test]
fn a_face_with_an_unknown_label_has_no_texture() {
    let (mut doc, mut sel, env, mut rng) = fixture();
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    let id = crate::primitives::add_element(&mut cx, ElementType::Cube);
    cx.doc.textures.entries.clear();
    assert!(TexKey::of_face(cx.doc, id, FaceName::Up).is_none());
    assert!(!apply(
        &mut cx,
        &TexKey::Entry(Label::from("texture")),
        &Patch {
            x: 0,
            y: 0,
            texels: RgbaBuf::new(1, 1)
        }
    ));
}
