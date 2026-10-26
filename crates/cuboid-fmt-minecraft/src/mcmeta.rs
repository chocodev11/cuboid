//! The `.png.mcmeta` file beside an animated texture (spec: textures T1 3).
//!
//! The game reads its `animation` member: `frametime`, `interpolate`, the frame size `width` and
//! `height`, and the play order `frames`, a list of tile indices or of `{"index", "time"}`. A
//! picture is a vertical strip, so a frame as wide as the picture is the only layout read. The
//! other members of `animation` are kept as text and written back.

use cuboid_io::text::push_json_string;
use cuboid_model::{FrameOrder, FrameStep, MAX_FRAMES_EXTRA, MAX_STEPS, TextureAnimation};
use serde_json::{Map, Value};

use crate::jv::{int, int_at};

/// The members of `animation` the model has a field for.
const KNOWN: [&str; 5] = ["frametime", "interpolate", "width", "height", "frames"];

/// What a `.mcmeta` says about a picture.
#[derive(Debug, PartialEq)]
pub(crate) enum Meta {
    /// It has no animation.
    Still,
    /// The animation of the strip.
    Animated(TextureAnimation),
    /// The `animation` member cannot be read, or does not fit the picture.
    Damaged,
}

/// Reads the `animation` member of a `.mcmeta` for a picture of `w` x `h` texels.
pub(crate) fn read(root: &Value, w: u32, h: u32) -> Meta {
    match root.get("animation") {
        None => Meta::Still,
        Some(member) => animation(member, w, h).map_or(Meta::Damaged, Meta::Animated),
    }
}

fn animation(member: &Value, w: u32, h: u32) -> Option<TextureAnimation> {
    let map = member.as_object()?;
    // A member that is there but is no whole number is damage, one that is not there is a default.
    let number = |key: &str| match map.get(key) {
        None => Some(None),
        Some(_) => u32::try_from(int_at(member, key)?).ok().map(Some),
    };
    let frametime = number("frametime")?.unwrap_or(1);
    // The size of a frame as the game works it out from what the file gives and the picture.
    let side = w.min(h);
    let (frame_width, frame_height) = match (number("width")?, number("height")?) {
        (Some(fw), Some(fh)) => (fw, fh),
        (Some(fw), None) => (fw, h),
        (None, Some(fh)) => (w, fh),
        (None, None) => (side, side),
    };
    let interpolate = match map.get("interpolate") {
        None => false,
        Some(flag) => flag.as_bool()?,
    };
    let frames = match map.get("frames") {
        None => None,
        Some(list) => steps(list.as_array()?)?,
    };
    let rest: Map<String, Value> = map
        .iter()
        .filter(|(key, _)| !KNOWN.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let extra = (!rest.is_empty())
        .then(|| Value::Object(rest).to_string())
        .filter(|text| text.len() <= MAX_FRAMES_EXTRA)
        .map(Into::into);
    let animation = TextureAnimation {
        frame_width,
        frame_height,
        frametime,
        interpolate,
        frames,
        extra,
    };
    animation.check(w, h).ok()?;
    Some(animation)
}

/// The steps of a `frames` list; `None` for a list that is damaged, `Some(None)` for an empty one,
/// which plays every tile.
fn steps(list: &[Value]) -> Option<Option<Vec<FrameStep>>> {
    if list.len() > MAX_STEPS {
        return None;
    }
    let steps = list
        .iter()
        .map(|item| match item.as_object() {
            None => Some(FrameStep::of(u32::try_from(int(item)?).ok()?)),
            Some(map) => Some(FrameStep {
                index: u32::try_from(int_at(item, "index")?).ok()?,
                time: match map.get("time") {
                    None => None,
                    // A time of zero or less is damage, like one that is no number.
                    Some(_) => Some(
                        u32::try_from(int_at(item, "time")?)
                            .ok()
                            .filter(|t| *t > 0)?,
                    ),
                },
            }),
        })
        .collect::<Option<Vec<_>>>()?;
    Some((!steps.is_empty()).then_some(steps))
}

/// The text of the `.mcmeta` of an animated texture whose picture is `image_h` texels high. Only
/// what differs from the defaults of the game is written; `frames` is written for every order but
/// the plain loop.
pub(crate) fn write(animation: &TextureAnimation, image_h: u32) -> String {
    let tiles = animation.tiles(image_h);
    let mut lines: Vec<String> = Vec::new();
    if animation.frametime != 1 {
        lines.push(format!("\"frametime\": {}", animation.frametime));
    }
    if animation.interpolate {
        lines.push("\"interpolate\": true".to_owned());
    }
    if animation.frame_height != animation.frame_width {
        lines.push(format!("\"width\": {}", animation.frame_width));
        lines.push(format!("\"height\": {}", animation.frame_height));
    }
    let frames = animation.frames.as_deref();
    if let Some(steps) = frames.filter(|_| FrameOrder::of(frames, tiles) != FrameOrder::Loop) {
        let items: Vec<String> = steps
            .iter()
            .map(|step| match step.time {
                None => step.index.to_string(),
                Some(time) => format!("{{ \"index\": {}, \"time\": {time} }}", step.index),
            })
            .collect();
        lines.push(format!("\"frames\": [{}]", items.join(", ")));
    }
    // What the file had besides, as it was written.
    let rest = animation
        .extra
        .as_deref()
        .and_then(|text| serde_json::from_str::<Map<String, Value>>(text).ok())
        .unwrap_or_default();
    for (key, value) in &rest {
        let mut line = String::new();
        push_json_string(&mut line, key);
        lines.push(format!("{line}: {value}"));
    }
    if lines.is_empty() {
        return "{\n\t\"animation\": {}\n}".to_owned();
    }
    format!(
        "{{\n\t\"animation\": {{\n\t\t{}\n\t}}\n}}",
        lines.join(",\n\t\t")
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn animated(root: &Value, w: u32, h: u32) -> TextureAnimation {
        match read(root, w, h) {
            Meta::Animated(animation) => animation,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_member_with_no_fields_is_a_plain_loop_of_square_frames() {
        let animation = animated(&json!({"animation": {}}), 16, 64);
        assert_eq!(animation, TextureAnimation::new(16, 16));
        assert_eq!(read(&json!({}), 16, 64), Meta::Still);
        assert_eq!(
            read(&json!({"texture": {"blur": true}}), 16, 64),
            Meta::Still
        );
    }

    #[test]
    fn frames_are_numbers_or_objects_with_a_time() {
        let root = json!({"animation": {
            "frametime": 3, "interpolate": true,
            "frames": [0, {"index": 2, "time": 7}, {"index": 1}, 3]
        }});
        let animation = animated(&root, 16, 64);
        assert_eq!((animation.frametime, animation.interpolate), (3, true));
        let steps = animation.frames.expect("an order");
        let list: Vec<(u32, Option<u32>)> = steps.iter().map(|s| (s.index, s.time)).collect();
        assert_eq!(list, [(0, None), (2, Some(7)), (1, None), (3, None)]);
    }

    #[test]
    fn an_empty_list_plays_every_tile() {
        let animation = animated(&json!({"animation": {"frames": []}}), 8, 16);
        assert_eq!(animation.frames, None);
    }

    #[test]
    fn the_frame_size_comes_from_the_file_or_the_picture() {
        // Frames as wide as the picture and not square.
        let wide = json!({"animation": {"width": 16, "height": 8}});
        let animation = animated(&wide, 16, 32);
        assert_eq!((animation.frame_width, animation.frame_height), (16, 8));
        assert_eq!(animation.tiles(32), 4);
        // Only the height: the width is the picture's.
        let animation = animated(&json!({"animation": {"height": 4}}), 16, 32);
        assert_eq!((animation.frame_width, animation.frame_height), (16, 4));
        // A frame narrower than the picture lays frames side by side, which a strip is not.
        assert_eq!(
            read(&json!({"animation": {"width": 8}}), 16, 32),
            Meta::Damaged
        );
        // A picture wider than it is high, with no size, is such a layout too.
        assert_eq!(read(&json!({"animation": {}}), 32, 16), Meta::Damaged);
    }

    #[test]
    fn damaged_members_are_reported() {
        for animation in [
            json!(true),
            json!({"frametime": 0}),
            json!({"frametime": "x"}),
            json!({"frametime": -2}),
            json!({"interpolate": 1}),
            json!({"frames": 3}),
            json!({"frames": [0, "a"]}),
            json!({"frames": [{"time": 2}]}),
            json!({"frames": [{"index": 0, "time": 0}]}),
            json!({"frames": [9]}),
            json!({"height": 5}),
        ] {
            let root = json!({ "animation": animation });
            assert_eq!(read(&root, 16, 32), Meta::Damaged, "{animation}");
        }
    }

    #[test]
    fn other_members_are_kept_and_written_back() {
        let root = json!({"animation": {"frametime": 2, "note": {"a": [1, 2]}}});
        let animation = animated(&root, 16, 32);
        assert_eq!(animation.extra.as_deref(), Some(r#"{"note":{"a":[1,2]}}"#));
        let text = write(&animation, 32);
        let back: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(back, root);
    }

    #[test]
    fn only_what_differs_from_the_defaults_is_written() {
        let plain = TextureAnimation::new(16, 16);
        assert_eq!(write(&plain, 64), "{\n\t\"animation\": {}\n}");

        let mut animation = TextureAnimation::new(16, 8);
        animation.frametime = 4;
        animation.interpolate = true;
        animation.frames = FrameOrder::Backwards.steps(4);
        let text = write(&animation, 32);
        let back: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(
            back,
            json!({"animation": {
                "frametime": 4, "interpolate": true, "width": 16, "height": 8,
                "frames": [3, 2, 1, 0]
            }})
        );

        // A list that plays every tile in order is the default and is not written.
        animation.frames = Some((0..4).map(FrameStep::of).collect());
        assert!(!write(&animation, 32).contains("frames"));
        // A step with a time is written as an object.
        animation.frames = Some(vec![FrameStep {
            index: 1,
            time: Some(9),
        }]);
        let back: Value = serde_json::from_str(&write(&animation, 32)).expect("json");
        assert_eq!(
            back["animation"]["frames"],
            json!([{"index": 1, "time": 9}])
        );
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let mut animation = TextureAnimation::new(16, 16);
        animation.frametime = 5;
        animation.frames = Some(vec![
            FrameStep::of(2),
            FrameStep {
                index: 0,
                time: Some(11),
            },
            FrameStep::of(1),
        ]);
        let text = write(&animation, 48);
        let root: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(animated(&root, 16, 48), animation);
    }
}
