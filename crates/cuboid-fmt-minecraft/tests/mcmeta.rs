//! Animated textures: the `.png.mcmeta` beside a picture, read by the importer and written by the
//! resource pack and the model export.

mod common;

use common::{Tree, elements, import_at, open, png};
use cuboid_fmt_minecraft::json_export::export;
use cuboid_fmt_minecraft::{JsonOptions, McVersion, Notice, PackOptions, export_pack};
use cuboid_model::image::{RgbaBuf, encode_png};
use cuboid_model::{Document, FrameStep, TextureAnimation};
use serde_json::{Value, json};

const PATH: &str = "pack/assets/minecraft/models/block/m.json";
const PICTURE: &str = "pack/assets/minecraft/textures/block/water.png";

fn v(text: &str) -> McVersion {
    McVersion::parse(text).expect("version")
}

const MODEL: &str = r##"{ "textures": { "all": "block/water" },
    "elements": [ { "from": [0, 0, 0], "to": [16, 16, 16],
        "faces": { "up": { "uv": [0, 0, 16, 16], "texture": "#all" } } } ] }"##;

/// A strip of `tiles` frames of `w` x `h` texels, tile 0 on top, each tile of its own red.
fn strip(w: u32, h: u32, tiles: u32) -> Vec<u8> {
    let mut buf = RgbaBuf::new(w, h * tiles);
    for tile in 0..tiles {
        let first = (tiles - 1 - tile) * h;
        for y in first..first + h {
            for x in 0..w {
                let at = (y * w + x) as usize * 4;
                let red = u8::try_from(tile * 30 + 20).expect("a shade");
                buf.pixels[at..at + 4].copy_from_slice(&[red, 0, 0, 255]);
            }
        }
    }
    encode_png(&cuboid_model::image::Image::from_rgba(buf)).expect("encodes")
}

fn tree(picture: Vec<u8>, mcmeta: Option<&str>) -> Tree {
    let tree = Tree::default().with(PICTURE, picture);
    match mcmeta {
        Some(text) => tree.with(&format!("{PICTURE}.mcmeta"), text),
        None => tree,
    }
}

fn animation_of(doc: &Document) -> Option<TextureAnimation> {
    doc.textures.entries[0].animation.clone()
}

#[test]
fn the_mcmeta_beside_a_picture_makes_its_animation() {
    let meta = r#"{ "animation": { "frametime": 4, "interpolate": true,
        "frames": [0, { "index": 2, "time": 9 }, 1] } }"#;
    let imported = import_at(MODEL, PATH, &tree(strip(16, 16, 3), Some(meta)));
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    let animation = animation_of(&doc).expect("animated");
    assert_eq!((animation.frametime, animation.interpolate), (4, true));
    assert_eq!(
        animation.frames,
        Some(vec![
            FrameStep::of(0),
            FrameStep {
                index: 2,
                time: Some(9)
            },
            FrameStep::of(1)
        ])
    );
    // The face covers one frame: its rectangle is relative to a frame, and the picture is the
    // whole strip.
    let image = &doc.textures.entries[0].texture.image;
    assert_eq!((image.w, image.h), (16, 48));
    let uv = elements(&doc)[0].faces[cuboid_model::FaceName::Up as usize].uv;
    assert_eq!(uv.br(), glam::Vec2::new(1.0, 1.0));
}

#[test]
fn a_picture_without_a_mcmeta_or_without_an_animation_is_still() {
    let doc = open(import_at(MODEL, PATH, &tree(strip(16, 16, 3), None)));
    assert!(animation_of(&doc).is_none());
    let other = r#"{ "texture": { "blur": true } }"#;
    let doc = open(import_at(MODEL, PATH, &tree(strip(16, 16, 3), Some(other))));
    assert!(animation_of(&doc).is_none());
}

#[test]
fn a_damaged_mcmeta_gives_a_notice_and_a_still_texture() {
    for meta in [
        "{ not json",
        r#"{ "animation": { "frametime": 0 } }"#,
        r#"{ "animation": { "frames": [5] } }"#,
        // Frames side by side are not a strip.
        r#"{ "animation": { "width": 8 } }"#,
    ] {
        let imported = import_at(MODEL, PATH, &tree(strip(16, 16, 3), Some(meta)));
        assert!(
            imported.notices.contains(&Notice::AnimationDamaged {
                name: "all".to_owned()
            }),
            "{meta}: {:?}",
            imported.notices
        );
        assert!(animation_of(&open(imported)).is_none(), "{meta}");
    }
}

#[test]
fn frames_that_are_not_square_need_the_width_and_the_height_of_the_file() {
    let meta = r#"{ "animation": { "width": 16, "height": 8 } }"#;
    let doc = open(import_at(MODEL, PATH, &tree(strip(16, 8, 4), Some(meta))));
    let animation = animation_of(&doc).expect("animated");
    assert_eq!((animation.frame_width, animation.frame_height), (16, 8));
    assert_eq!(animation.tiles(32), 4);
}

#[test]
fn a_resource_pack_writes_the_strip_and_the_mcmeta_and_reads_back_the_same() {
    let meta = r#"{ "animation": { "frametime": 3, "interpolate": true, "extra_member": [1],
        "frames": [1, 0, { "index": 2, "time": 6 }] } }"#;
    let doc = open(import_at(MODEL, PATH, &tree(strip(16, 16, 3), Some(meta))));
    let animation = animation_of(&doc).expect("animated");
    let pack = export_pack(&doc, &PackOptions::new(v("1.21.3"))).expect("fits");
    let file = |path: &str| {
        pack.files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| {
                panic!(
                    "{path} in {:?}",
                    pack.files.iter().map(|f| &f.path).collect::<Vec<_>>()
                )
            })
    };
    let picture = file("assets/minecraft/textures/block/water.png");
    let written = file("assets/minecraft/textures/block/water.png.mcmeta");
    let text = std::str::from_utf8(&written.bytes).expect("text");
    let json: Value = serde_json::from_str(text).expect("json");
    assert_eq!(
        json,
        json!({"animation": {
            "frametime": 3, "interpolate": true,
            "frames": [1, 0, {"index": 2, "time": 6}],
            "extra_member": [1]
        }})
    );
    assert!(!text.contains('\r'));

    // The pack read as a model folder gives the animation and the strip again.
    let again = Tree::default()
        .with(PICTURE, picture.bytes.clone())
        .with(&format!("{PICTURE}.mcmeta"), written.bytes.clone());
    let back = open(import_at(MODEL, PATH, &again));
    assert_eq!(animation_of(&back), Some(animation));
    assert_eq!(
        back.textures.entries[0].texture.image.rgba(),
        doc.textures.entries[0].texture.image.rgba()
    );
}

#[test]
fn a_still_texture_has_no_mcmeta_and_a_plain_loop_writes_an_empty_member() {
    let doc = open(import_at(
        MODEL,
        PATH,
        &Tree::default().with(PICTURE, png(16, 16, [1, 2, 3, 255])),
    ));
    let pack = export_pack(&doc, &PackOptions::new(v("1.21.3"))).expect("fits");
    assert!(
        !pack
            .files
            .iter()
            .any(|f| f.path.ends_with(".mcmeta") && f.path.contains("textures"))
    );

    let doc = open(import_at(
        MODEL,
        PATH,
        &tree(strip(16, 16, 2), Some(r#"{ "animation": {} }"#)),
    ));
    let pack = export_pack(&doc, &PackOptions::new(v("1.21.3"))).expect("fits");
    let written = pack
        .files
        .iter()
        .find(|f| f.path.ends_with("water.png.mcmeta"))
        .expect("a mcmeta");
    assert_eq!(written.bytes, b"{\n\t\"animation\": {}\n}");
}

#[test]
fn a_texture_without_a_path_gets_its_mcmeta_beside_the_model() {
    let mut doc = open(import_at(
        MODEL,
        PATH,
        &Tree::default().with(PICTURE, png(16, 16, [1, 2, 3, 255])),
    ));
    // A strip of two frames that was never saved: no Minecraft path.
    let mut animated = doc.textures.entries[0].clone();
    animated.label = "glow".into();
    animated.texture.source = Default::default();
    animated.texture.image =
        std::sync::Arc::new(cuboid_model::image::Image::from_rgba(RgbaBuf::new(16, 32)));
    let mut animation = TextureAnimation::new(16, 16);
    animation.frametime = 2;
    animated.animation = Some(animation);
    doc.textures.entries.push(animated);
    let out = export(&doc, &JsonOptions::new(v("1.21.3"))).expect("fits");
    let paths: Vec<&str> = out.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["glow.png", "glow.png.mcmeta"]);
    let meta: Value = serde_json::from_slice(&out.files[1].bytes).expect("json");
    assert_eq!(meta, json!({"animation": {"frametime": 2}}));
    assert!(out.notices.contains(&Notice::UnsavedTextures { count: 1 }));
}
