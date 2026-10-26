//! Animated textures of a project: strips of frames, the frame fields and their round trip.

mod common;

use std::sync::Arc;

use common::{import_json, open, png_of, text_of};
use cuboid_fmt_bbmodel::data_url::png_url;
use cuboid_fmt_bbmodel::{Notice, export, import};
use cuboid_model::image::{Image, RgbaBuf};
use cuboid_model::{Document, FrameStep, TextureAnimation};
use serde_json::{Value, json};

/// A strip of `tiles` frames of `w` x `h` texels, each frame of one colour, tile 0 on top.
fn strip(w: u32, h: u32, tiles: u32) -> Arc<Image> {
    let mut buf = RgbaBuf::new(w, h * tiles);
    for tile in 0..tiles {
        // Rows run bottom to top, so the first tile is the last rows of the data.
        let first = (tiles - 1 - tile) * h;
        for y in first..first + h {
            for x in 0..w {
                let at = (y * w + x) as usize * 4;
                let shade = u8::try_from(tile * 40 + 10).expect("a shade");
                buf.pixels[at..at + 4].copy_from_slice(&[shade, 0, 0, 255]);
            }
        }
    }
    Arc::new(Image::from_rgba(buf))
}

fn texture_item(image: &Image, units: [u32; 2], fields: Value) -> Value {
    let mut item = json!({
        "name": "water.png", "folder": "block", "id": "water",
        "uv_width": units[0], "uv_height": units[1],
        "source": png_url(&png_of(image)), "uuid": "t0"
    });
    if let (Some(item), Some(fields)) = (item.as_object_mut(), fields.as_object()) {
        item.extend(fields.clone());
    }
    item
}

fn project(model_format: &str, texture: Value) -> Value {
    json!({
        "meta": {"format_version": "4.10", "model_format": model_format, "box_uv": false},
        "name": "water",
        "resolution": {"width": 16, "height": 16},
        "elements": [{
            "name": "Box", "from": [0, 0, 0], "to": [16, 16, 16], "origin": [8, 8, 8],
            "faces": {"up": {"uv": [0, 0, 16, 16], "texture": 0}},
            "type": "cube", "uuid": "e0"
        }],
        "textures": [texture],
    })
}

fn opened(project: &Value) -> Document {
    open(import_json(project))
}

fn entry_animation(doc: &Document) -> Option<TextureAnimation> {
    doc.textures.entries[0].animation.clone()
}

fn exported(doc: &Document) -> Value {
    serde_json::from_slice(&export(doc).expect("exports").bytes).expect("json")
}

#[test]
fn a_tall_picture_is_a_strip_and_the_faces_stay_relative_to_one_frame() {
    let image = strip(16, 16, 4);
    let fields = json!({"frame_time": 3, "frame_interpolate": true});
    let doc = opened(&project(
        "java_block",
        texture_item(&image, [16, 16], fields),
    ));
    let entry = &doc.textures.entries[0];
    let animation = entry.animation.as_ref().expect("animated");
    assert_eq!((animation.frame_width, animation.frame_height), (16, 16));
    assert_eq!((animation.frametime, animation.interpolate), (3, true));
    assert_eq!(animation.frames, None);
    // The whole strip is the image, and the picture is unchanged.
    assert_eq!((entry.texture.image.w, entry.texture.image.h), (16, 64));
    assert_eq!(entry.texture.image.rgba(), image.rgba());
    // A face over 0..16 of the 16 x 16 units covers one frame, not the strip.
    let face = &doc.elements.items()[0].faces[cuboid_model::FaceName::Up as usize];
    assert_eq!(face.uv.tl(), glam::Vec2::new(0.0, 0.0));
    assert_eq!(face.uv.br(), glam::Vec2::new(1.0, 1.0));
}

#[test]
fn the_named_orders_and_custom_steps_become_the_play_order() {
    let image = strip(16, 16, 4);
    let order = |kind: &str, text: &str| {
        let fields = json!({"frame_order_type": kind, "frame_order": text});
        let doc = opened(&project(
            "java_block",
            texture_item(&image, [16, 16], fields),
        ));
        entry_animation(&doc).expect("animated").frames
    };
    assert_eq!(order("loop", ""), None);
    let indices = |frames: Option<Vec<FrameStep>>| -> Vec<u32> {
        frames.iter().flatten().map(|s| s.index).collect()
    };
    assert_eq!(indices(order("backwards", "")), [3, 2, 1, 0]);
    assert_eq!(indices(order("back_and_forth", "")), [0, 1, 2, 3, 2, 1]);
    let custom = order("custom", "2 0:7 1");
    assert_eq!(
        custom,
        Some(vec![
            FrameStep::of(2),
            FrameStep {
                index: 0,
                time: Some(7)
            },
            FrameStep::of(1)
        ])
    );
}

#[test]
fn stills_and_formats_that_do_not_animate_are_not_strips() {
    let tall = strip(16, 16, 4);
    // One frame of the UV size, or a picture that is not a whole number of frames.
    let doc = opened(&project("free", texture_item(&tall, [16, 64], json!({}))));
    assert!(entry_animation(&doc).is_none());
    let odd = Arc::new(Image::from_rgba(RgbaBuf::new(16, 40)));
    let doc = opened(&project(
        "java_block",
        texture_item(&odd, [16, 16], json!({})),
    ));
    assert!(entry_animation(&doc).is_none());
    // A skin or a modded entity measures its UV space by the picture; no strips there.
    let doc = opened(&project(
        "modded_entity",
        texture_item(&tall, [16, 16], json!({})),
    ));
    assert!(entry_animation(&doc).is_none());
}

#[test]
fn a_damaged_order_is_a_loop_with_a_notice() {
    let image = strip(16, 16, 3);
    let fields = json!({"frame_order_type": "custom", "frame_order": "0 7"});
    let project = project("java_block", texture_item(&image, [16, 16], fields));
    let imported = import_json(&project);
    assert!(
        imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::Skipped { what, .. } if what.contains("frame orders"))),
        "{:?}",
        imported.notices
    );
    assert_eq!(
        entry_animation(&open(imported)).expect("strip").frames,
        None
    );
}

#[test]
fn the_strip_and_the_fields_survive_a_round_trip() {
    let image = strip(16, 16, 4);
    let fields = json!({
        "frame_time": 5, "frame_interpolate": true,
        "frame_order_type": "custom", "frame_order": "3 1:9 0"
    });
    let doc = opened(&project(
        "java_block",
        texture_item(&image, [16, 16], fields),
    ));
    let written = exported(&doc);
    let texture = &written["textures"][0];
    assert_eq!(texture["frame_time"], 5);
    assert_eq!(texture["frame_interpolate"], true);
    assert_eq!(texture["frame_order_type"], "custom");
    assert_eq!(texture["frame_order"], "3 1:9 0");
    assert_eq!(
        (texture["width"].as_u64(), texture["height"].as_u64()),
        (Some(16), Some(64))
    );
    assert_eq!(written["meta"]["model_format"], "java_block");

    let again = open(
        import(
            &text_of(&written),
            std::path::Path::new("dir/Model.bbmodel"),
            &common::NoFiles,
        )
        .expect("imports"),
    );
    assert_eq!(entry_animation(&again), entry_animation(&doc));
    assert_eq!(
        again.textures.entries[0].texture.image.rgba(),
        image.rgba(),
        "the strip is written as it is"
    );
}

#[test]
fn fields_that_say_what_the_animation_says_are_written_as_the_file_had_them() {
    // A custom order that lists every tile in order, a float time and the order text as typed.
    let image = strip(16, 16, 3);
    let fields = json!({
        "frame_time": 2.0, "frame_interpolate": false,
        "frame_order_type": "custom", "frame_order": "0  1 2"
    });
    let doc = opened(&project(
        "java_block",
        texture_item(&image, [16, 16], fields.clone()),
    ));
    let written = exported(&doc);
    for key in [
        "frame_time",
        "frame_interpolate",
        "frame_order_type",
        "frame_order",
    ] {
        assert_eq!(written["textures"][0][key], fields[key], "{key}");
    }

    // An edit makes the text of that field stale; the others stay as they were.
    let mut edited = doc.clone();
    if let Some(animation) = &mut edited.textures.entries[0].animation {
        animation.frametime = 7;
    }
    let written = exported(&edited);
    assert_eq!(written["textures"][0]["frame_time"], 7);
    assert_eq!(written["textures"][0]["frame_order"], "0  1 2");
    // A new order is written by name or as steps.
    if let Some(animation) = &mut edited.textures.entries[0].animation {
        animation.frames = Some(vec![FrameStep::of(2), FrameStep::of(1), FrameStep::of(0)]);
    }
    let written = exported(&edited);
    assert_eq!(written["textures"][0]["frame_order_type"], "backwards");
    assert_eq!(written["textures"][0]["frame_order"], "");
}

#[test]
fn a_strip_with_frames_that_are_not_square_writes_a_uv_size_that_reads_back() {
    // A free project with a UV size of 16 x 8 per frame.
    let image = strip(16, 8, 4);
    let item = texture_item(&image, [16, 8], json!({}));
    let mut source = project("free", item);
    source["elements"][0]["faces"]["up"]["uv"] = json!([0, 0, 16, 8]);
    let doc = opened(&source);
    let animation = entry_animation(&doc).expect("a strip");
    assert_eq!((animation.frame_width, animation.frame_height), (16, 8));
    let face = &doc.elements.items()[0].faces[cuboid_model::FaceName::Up as usize];
    assert_eq!(face.uv.br(), glam::Vec2::new(1.0, 1.0));

    let written = exported(&doc);
    assert_eq!(written["meta"]["model_format"], "free");
    assert_eq!(written["textures"][0]["uv_height"], 8.0);
    assert_eq!(
        written["elements"][0]["faces"]["up"]["uv"],
        json!([0.0, 0.0, 16.0, 8.0])
    );
    let again = open(
        import(
            &text_of(&written),
            std::path::Path::new("dir/Model.bbmodel"),
            &common::NoFiles,
        )
        .expect("imports"),
    );
    assert_eq!(entry_animation(&again), Some(animation));
}
