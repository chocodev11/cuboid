//! The animation of a project texture: a strip of frames and the fields `frame_time`,
//! `frame_interpolate`, `frame_order_type` and `frame_order` (spec: textures T1 3).
//!
//! A format that animates textures measures the faces of a texture in the UV size of one frame, so
//! a picture that holds a whole number of such frames, at least two, is a strip. The fields give
//! the time of a frame, whether it blends into the next one and the order: one of three named
//! orders, or `custom` with the steps as text, `3` or `3:5` (a tile and its time in ticks) apart
//! by spaces. A project writes all four fields for every texture; the importer keeps them of an
//! animated one as they were, and the exporter writes them back unchanged while they still say
//! what the animation says.

use std::collections::HashMap;

use cuboid_model::image::Image;
use cuboid_model::{FrameOrder, FrameStep, MAX_STEP_TICKS, MAX_STEPS, MAX_TILES, TextureAnimation};
use glam::Vec2;
use serde_json::{Map, Value, json};

use crate::Notice;
use crate::json::{flag, int_at, text};

/// The fields of a project texture that this module reads and writes.
const FIELDS: [&str; 4] = [
    "frame_time",
    "frame_interpolate",
    "frame_order_type",
    "frame_order",
];

/// What the four fields say.
#[derive(Debug, PartialEq)]
struct Fields {
    frametime: u32,
    interpolate: bool,
    frames: Option<Vec<FrameStep>>,
}

/// The strip a picture is when a frame has the UV size `units`: the frame is as wide as the
/// picture and as high as the UV size says, and the picture holds a whole number of them.
fn strip(image: &Image, units: Vec2) -> Option<TextureAnimation> {
    let height = (image.w as f32 * units.y / units.x).round();
    let frame_height = u32::try_from(cuboid_num::trunc_i32(height))
        .ok()
        .filter(|h| *h > 0)?;
    let tiles = image.h.checked_div(frame_height)?;
    let whole = image.h.is_multiple_of(frame_height);
    (whole && (2..=MAX_TILES).contains(&tiles))
        .then(|| TextureAnimation::new(image.w, frame_height))
}

/// The steps of a custom order; `None` when a step is no number.
fn custom(order: &str) -> Option<Vec<FrameStep>> {
    order
        .split_whitespace()
        .take(MAX_STEPS + 1)
        .map(|token| {
            let (index, time) = token.split_once(':').unzip();
            Some(FrameStep {
                index: index.unwrap_or(token).parse().ok()?,
                // A time of zero means none, as in the editor.
                time: time
                    .and_then(|t: &str| t.parse::<u32>().ok())
                    .filter(|t| *t > 0),
            })
        })
        .collect()
}

/// The fields of a texture item for a strip of `tiles` frames. A custom order that is no list of
/// numbers reads as the loop, and `damaged` says so.
fn read_fields(item: &Value, tiles: u32) -> (Fields, bool) {
    let frametime = int_at(item, "frame_time")
        .and_then(|t| u32::try_from(t).ok())
        .filter(|t| *t > 0)
        .map_or(1, |t| t.min(MAX_STEP_TICKS));
    let interpolate = flag(item, "frame_interpolate").unwrap_or(false);
    let (frames, damaged) = match text(item, "frame_order_type") {
        Some("backwards") => (FrameOrder::Backwards.steps(tiles), false),
        Some("back_and_forth") => (FrameOrder::BackAndForth.steps(tiles), false),
        Some("custom") => match text(item, "frame_order").map(custom) {
            Some(Some(steps)) => ((!steps.is_empty()).then_some(steps), false),
            Some(None) => (None, true),
            None => (None, false),
        },
        _ => (None, false),
    };
    (
        Fields {
            frametime,
            interpolate,
            frames,
        },
        damaged,
    )
}

/// The animation a texture item has for its picture, and its four fields as the project had them.
/// `None` for a texture that is not a strip. `name` is what a notice calls the texture.
pub(crate) fn read(
    item: &Value,
    image: &Image,
    units: Vec2,
    name: &str,
    notices: &mut Vec<Notice>,
) -> Option<(TextureAnimation, Value)> {
    let mut animation = strip(image, units)?;
    let (fields, damaged) = read_fields(item, animation.tiles(image.h));
    animation.frametime = fields.frametime;
    animation.interpolate = fields.interpolate;
    animation.frames = fields.frames;
    // A step that names a tile the strip does not have breaks the whole order.
    let unfit = animation.check(image.w, image.h).is_err();
    if damaged || unfit {
        animation.frames = None;
        notices.push(Notice::Skipped {
            what: format!("frame orders of the texture `{name}` that do not fit its picture"),
            count: 1,
        });
    }
    let raw: Map<String, Value> = FIELDS
        .iter()
        .filter_map(|key| Some((key.to_string(), item.get(key)?.clone())))
        .collect();
    Some((animation, Value::Object(raw)))
}

/// The four fields of an animation. `raw` is what the project had for the texture: a field that
/// still says what the animation says is written back as it was, so an unchanged texture keeps its
/// numbers and its text; the others are written from the animation.
pub(crate) fn write(
    out: &mut Map<String, Value>,
    animation: &TextureAnimation,
    tiles: u32,
    raw: Option<&Value>,
) {
    let (time_kept, interpolate_kept, order_kept) = raw.map_or((false, false, false), |raw| {
        let (read, damaged) = read_fields(raw, tiles);
        (
            read.frametime == animation.frametime,
            read.interpolate == animation.interpolate,
            !damaged && read.frames == animation.frames,
        )
    });
    let kept = |key: &str, keep: bool| keep.then(|| raw?.get(key).cloned()).flatten();
    out.insert(
        "frame_time".to_owned(),
        kept("frame_time", time_kept).unwrap_or_else(|| json!(animation.frametime)),
    );
    out.insert(
        "frame_interpolate".to_owned(),
        kept("frame_interpolate", interpolate_kept).unwrap_or_else(|| json!(animation.interpolate)),
    );
    // The type and the text of the order belong together.
    let (kind, steps) = kept("frame_order_type", order_kept)
        .zip(kept("frame_order", order_kept))
        .unwrap_or_else(|| order_fields(animation, tiles));
    out.insert("frame_order_type".to_owned(), kind);
    out.insert("frame_order".to_owned(), steps);
}

/// The type and the text of the order of an animation, as the editor writes them.
fn order_fields(animation: &TextureAnimation, tiles: u32) -> (Value, Value) {
    let order = FrameOrder::of(animation.frames.as_deref(), tiles);
    let (kind, steps) = match order {
        FrameOrder::Loop => ("loop", String::new()),
        FrameOrder::Backwards => ("backwards", String::new()),
        FrameOrder::BackAndForth => ("back_and_forth", String::new()),
        FrameOrder::Custom => (
            "custom",
            animation
                .frames
                .iter()
                .flatten()
                .map(|step| match step.time {
                    None => step.index.to_string(),
                    Some(time) => format!("{}:{time}", step.index),
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
    };
    (json!(kind), json!(steps))
}

/// The raw fields of the animated textures of an import, by texture label.
pub(crate) type Raw = HashMap<String, Value>;

#[cfg(test)]
mod tests {
    use cuboid_model::image::RgbaBuf;

    use super::*;

    fn picture(w: u32, h: u32) -> Image {
        Image::from_rgba(RgbaBuf::new(w, h))
    }

    fn read_item(item: Value, h: u32) -> Option<(TextureAnimation, Value)> {
        read(
            &item,
            &picture(16, h),
            Vec2::splat(16.0),
            "t",
            &mut Vec::new(),
        )
    }

    #[test]
    fn a_picture_of_whole_frames_is_a_strip_and_other_pictures_are_not() {
        let units = Vec2::splat(16.0);
        let frames =
            |w, h, units| strip(&picture(w, h), units).map(|a| (a.frame_height, a.tiles(h)));
        assert_eq!(frames(16, 64, units), Some((16, 4)));
        assert_eq!(frames(16, 32, units), Some((16, 2)));
        // One frame, a picture that is no whole number of frames, and a wide one.
        assert_eq!(frames(16, 16, units), None);
        assert_eq!(frames(16, 40, units), None);
        assert_eq!(frames(32, 16, units), None);
        // The UV size is that of one frame: a picture as large as the UV space is one frame.
        assert_eq!(frames(64, 128, Vec2::new(64.0, 128.0)), None);
        // Frames that are not square follow the UV size.
        assert_eq!(frames(16, 32, Vec2::new(16.0, 8.0)), Some((8, 4)));
    }

    #[test]
    fn the_fields_become_the_animation_and_are_kept() {
        let item = json!({
            "frame_time": 3, "frame_interpolate": true,
            "frame_order_type": "custom", "frame_order": "2 0:7 1"
        });
        let (animation, raw) = read_item(item.clone(), 48).expect("a strip");
        assert_eq!((animation.frametime, animation.interpolate), (3, true));
        let steps: Vec<(u32, Option<u32>)> = animation
            .frames
            .as_ref()
            .expect("an order")
            .iter()
            .map(|s| (s.index, s.time))
            .collect();
        assert_eq!(steps, [(2, None), (0, Some(7)), (1, None)]);
        assert_eq!(raw, item);
    }

    #[test]
    fn the_named_orders_expand_over_the_tiles() {
        let backwards = json!({"frame_order_type": "backwards"});
        let (animation, _) = read_item(backwards, 64).expect("a strip");
        let steps: Vec<u32> = animation.frames.iter().flatten().map(|s| s.index).collect();
        assert_eq!(steps, [3, 2, 1, 0]);
        let both = json!({"frame_order_type": "back_and_forth"});
        let (animation, _) = read_item(both, 64).expect("a strip");
        let steps: Vec<u32> = animation.frames.iter().flatten().map(|s| s.index).collect();
        assert_eq!(steps, [0, 1, 2, 3, 2, 1]);
        let (animation, _) = read_item(json!({}), 64).expect("a strip");
        assert_eq!(animation.frames, None);
        assert_eq!(animation.frametime, 1);
    }

    #[test]
    fn a_damaged_order_reads_as_a_loop_with_a_notice() {
        for order in ["0 x 2", "0 1 9"] {
            let item = json!({"frame_order_type": "custom", "frame_order": order});
            let mut notices = Vec::new();
            let (animation, _) = read(
                &item,
                &picture(16, 48),
                Vec2::splat(16.0),
                "door",
                &mut notices,
            )
            .expect("a strip");
            assert_eq!(animation.frames, None, "{order}");
            assert_eq!(notices.len(), 1, "{order}");
        }
    }

    #[test]
    fn raw_fields_come_back_while_they_say_what_the_animation_says() {
        // A custom order that lists every tile in order: the raw text is the file's own choice.
        let item = json!({
            "frame_time": 2.0, "frame_interpolate": false,
            "frame_order_type": "custom", "frame_order": "0 1 2"
        });
        let (mut animation, raw) = read_item(item.clone(), 48).expect("a strip");
        let mut out = Map::new();
        write(&mut out, &animation, 3, Some(&raw));
        assert_eq!(Value::Object(out), item);

        // After an edit the fields are written from the animation.
        animation.frametime = 4;
        let mut out = Map::new();
        write(&mut out, &animation, 3, Some(&raw));
        assert_eq!(
            Value::Object(out),
            json!({
                "frame_time": 4, "frame_interpolate": false,
                "frame_order_type": "custom", "frame_order": "0 1 2"
            })
        );
    }

    #[test]
    fn the_orders_are_written_by_name_and_custom_steps_with_their_times() {
        let mut animation = TextureAnimation::new(16, 16);
        animation.frames = FrameOrder::BackAndForth.steps(4);
        let mut out = Map::new();
        write(&mut out, &animation, 4, None);
        assert_eq!(out["frame_order_type"], "back_and_forth");
        assert_eq!(out["frame_order"], "");

        animation.frames = Some(vec![
            FrameStep::of(1),
            FrameStep {
                index: 3,
                time: Some(9),
            },
        ]);
        let mut out = Map::new();
        write(&mut out, &animation, 4, None);
        assert_eq!(out["frame_order_type"], "custom");
        assert_eq!(out["frame_order"], "1 3:9");
        // Written, then read: the same animation.
        let (back, _) = read_fields(&Value::Object(out), 4);
        assert_eq!(back.frames, animation.frames);
    }
}
