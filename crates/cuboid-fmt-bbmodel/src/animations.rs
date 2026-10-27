//! Animations of a `.bbmodel` project (spec: animation A2 5).
//!
//! An animation is read into [`Animation`]: its bone animators become tracks of the groups they
//! name, their keyframes become keyframes with one or two data points (the second one is the value
//! a jump leaves with). What the model has no field for stays JSON text on the animation or the
//! track and is written back unchanged: the fields of the animation that the model does not use,
//! the animators that are not bones of the model (`effects` with sounds and particles, locators,
//! unknown uuids) and the fields of a bone animator besides its keyframes. An animator whose
//! keyframes the model cannot hold (a channel it has none for, more than two data points) is kept
//! whole. An animation the limits of the model leave no room for stays a JSON value of the
//! project's `animations` member, which is written after the others.
//!
//! The keyframes of a track are written channel by channel in the order the project listed them
//! (editors differ: rotation, position, scale or position, rotation, scale), each by time, and a
//! value comes back as the project wrote it: a number as a number, text as the same text. The
//! signs of the values follow [`crate::signs`]: the model has the convention of Blockbench 5, the
//! writer the one of version 4.10. What stays JSON is held in the writer's signs: it is as the
//! project had it when the project is older than 5.0, and converted when it is read otherwise.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cuboid_model::{
    Animation, BoneTrack, Channel, DEFAULT_SNAPPING, Document, GroupId, Groups, Handles, Interp,
    Keyframe, LoopMode, MAX_ANIMATIONS, MAX_KEYFRAMES, MAX_LENGTH, TIME_EPSILON, Value as Data,
};
use glam::Vec3;
use serde_json::{Map, Number, Value, json};

use crate::Notice;
use crate::import::{NAME_CHARS, clipped, uuid_of};
use crate::json::{flag, int_at, list, num, num_at, number, text, vec3_at};
use crate::signs;

/// The members of an animation that the model has fields for.
const OWN: [&str; 7] = [
    "uuid",
    "name",
    "loop",
    "override",
    "length",
    "snapping",
    "animators",
];

/// The members of a bone animator that the model has fields for.
const OWN_ANIMATOR: [&str; 3] = ["name", "type", "keyframes"];

/// The most data points of a keyframe the model keeps: the value and the value a jump leaves with.
const MAX_POINTS: usize = 2;

/// What reading the `animations` member gave.
#[derive(Default)]
pub(crate) struct Read {
    /// The animations of the model.
    pub animations: Vec<Animation>,
    /// The animations the model has no room for, as the project had them, in the signs of the
    /// writer.
    pub leftover: Vec<Value>,
    /// Bytes of JSON text the animations keep.
    pub bytes: u64,
    /// Things the importer changed or left out.
    pub notices: Vec<Notice>,
}

/// The animators as the importer sees them: the group each uuid names, and whether the project
/// has the old signs (see [`signs`]).
struct Rig {
    by_uuid: HashMap<String, GroupId>,
    old: bool,
}

impl Rig {
    fn new(groups: &Groups, old: bool) -> Self {
        let mut by_uuid = HashMap::new();
        for (&id, group) in groups.ids().iter().zip(groups.items()) {
            if let Some(uuid) = &group.uuid {
                by_uuid.entry(uuid.clone()).or_insert(id);
            }
        }
        Self { by_uuid, old }
    }
}

/// How many keyframes of a kind were dropped.
#[derive(Default)]
struct Dropped {
    duplicates: usize,
}

/// The JSON text of a map, `None` when it has no member.
fn json_text(map: Map<String, Value>) -> Option<Arc<str>> {
    (!map.is_empty())
        .then(|| serde_json::to_string(&Value::Object(map)).ok())
        .flatten()
        .map(Arc::from)
}

/// The value of one axis of a data point: a number stays a number, text stays text. `flip` says
/// that the project has the old sign on the axis, see [`signs`].
fn data_of(point: &Value, axis: &str, channel: Channel, flip: bool) -> Data {
    match point.get(axis) {
        Some(Value::String(s)) if flip => signs::read_text(s),
        Some(Value::String(s)) => Data::from_text(s),
        Some(n @ Value::Number(_)) => {
            let n = num(n).unwrap_or(channel.rest());
            Data::Num(if flip { -n } else { n })
        }
        _ => Data::Num(channel.rest()),
    }
}

/// The values of a data point; `old` says that the project has the old signs.
fn point_of(point: &Value, channel: Channel, old: bool) -> [Data; 3] {
    let [fx, fy, fz] = signs::flipped(channel).map(|flip| flip && old);
    [
        data_of(point, "x", channel, fx),
        data_of(point, "y", channel, fy),
        data_of(point, "z", channel, fz),
    ]
}

/// A keyframe of a bone animator, or `None` when the model cannot hold it. `old` says that the
/// project has the old signs.
fn keyframe(item: &Value, old: bool) -> Option<(Channel, Keyframe)> {
    let channel = Channel::from_key(text(item, "channel")?)?;
    let time = num_at(item, "time").unwrap_or(0.0).clamp(0.0, MAX_LENGTH);
    let interp = match text(item, "interpolation") {
        Some("catmullrom") => Interp::CatmullRom,
        Some("bezier") => Interp::Bezier,
        Some("step") => Interp::Step,
        _ => Interp::Linear,
    };
    let points = list(item, "data_points");
    if points.len() > MAX_POINTS {
        return None;
    }
    // Projects of an older version put the values on the keyframe itself.
    let (value, post) = match points {
        [] => (point_of(item, channel, old), None),
        [only] => (point_of(only, channel, old), None),
        [first, second, ..] => (
            point_of(first, channel, old),
            Some(point_of(second, channel, old)),
        ),
    };
    let handles = |key: &str| item.get(key).is_some();
    let bezier = (interp == Interp::Bezier
        || [
            "bezier_linked",
            "bezier_left_time",
            "bezier_left_value",
            "bezier_right_time",
            "bezier_right_value",
        ]
        .iter()
        .any(|key| handles(key)))
    .then(|| {
        let default = Handles::default();
        Handles {
            linked: flag(item, "bezier_linked").unwrap_or(default.linked),
            left_time: vec3_at(item, "bezier_left_time", Vec3::from(default.left_time)).to_array(),
            left_value: vec3_at(item, "bezier_left_value", Vec3::ZERO).to_array(),
            right_time: vec3_at(item, "bezier_right_time", Vec3::from(default.right_time))
                .to_array(),
            right_value: vec3_at(item, "bezier_right_value", Vec3::ZERO).to_array(),
        }
    });
    // The editor converts the handles of a Bezier keyframe only.
    let bezier = bezier.map(|mut handles| {
        if old && interp == Interp::Bezier {
            signs::flip_handles(&mut handles, channel);
        }
        handles
    });
    Some((
        channel,
        Keyframe {
            time,
            value,
            post,
            interp,
            bezier,
            uuid: uuid_of(item),
            color: int_at(item, "color")
                .and_then(|c| i8::try_from(c).ok())
                .filter(|c| (-1..=7).contains(c))
                .unwrap_or(-1),
            uniform: flag(item, "uniform"),
        },
    ))
}

/// The track of a bone animator, or `None` when the model cannot hold its keyframes.
fn track(group: GroupId, animator: &Value, dropped: &mut Dropped, old: bool) -> Option<BoneTrack> {
    let map = animator.as_object()?;
    if text(animator, "type").is_some_and(|kind| kind != "bone") {
        return None;
    }
    let mut out = BoneTrack::new(group);
    let mut keyframes = Vec::new();
    for item in list(animator, "keyframes") {
        keyframes.push(keyframe(item, old)?);
    }
    if map.contains_key("keyframes") && !map["keyframes"].is_array() {
        return None;
    }
    for channel in Channel::ALL {
        let keys: Vec<Keyframe> = keyframes
            .iter()
            .filter(|(c, _)| *c == channel)
            .map(|(_, k)| k.clone())
            .collect();
        *out.channel_mut(channel) = ordered(keys, dropped);
    }
    // The channels in the order the project lists them; those without keyframes follow.
    let mut listed: Vec<Channel> = Vec::with_capacity(3);
    for (channel, _) in &keyframes {
        if !listed.contains(channel) {
            listed.push(*channel);
        }
    }
    for channel in Channel::ALL {
        if !listed.contains(&channel) {
            listed.push(channel);
        }
    }
    if let [a, b, c] = listed.as_slice() {
        out.order = [*a, *b, *c];
    }
    out.extra = json_text(
        map.iter()
            .filter(|(key, _)| !OWN_ANIMATOR.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    );
    Some(out)
}

/// The keyframes by time. Of keyframes at one time the last one in the project stays.
fn ordered(mut keys: Vec<Keyframe>, dropped: &mut Dropped) -> Vec<Keyframe> {
    keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut out: Vec<Keyframe> = Vec::with_capacity(keys.len());
    for key in keys {
        match out.last_mut() {
            Some(last) if key.time - last.time < TIME_EPSILON => {
                *last = key;
                dropped.duplicates += 1;
            }
            _ => out.push(key),
        }
    }
    out
}

/// One animation of the project, with the number of bytes of text it keeps; `None` when the item
/// is no object.
fn animation(item: &Value, rig: &Rig, dropped: &mut Dropped) -> Option<(Animation, u64)> {
    let map = item.as_object()?;
    let (name, _) = clipped(text(item, "name").unwrap_or("animation"), NAME_CHARS);
    let mut out = Animation::new(
        name,
        num_at(item, "length").unwrap_or(0.0).clamp(0.0, MAX_LENGTH),
    );
    out.loop_mode = match text(item, "loop") {
        Some("loop") => LoopMode::Loop,
        Some("hold") => LoopMode::Hold,
        _ => LoopMode::Once,
    };
    out.overrides = flag(item, "override") == Some(true);
    out.snapping = int_at(item, "snapping")
        .and_then(|rate| u32::try_from(rate.clamp(10, 500)).ok())
        .unwrap_or(DEFAULT_SNAPPING);
    out.uuid = uuid_of(item);

    let mut rest: Map<String, Value> = map
        .iter()
        .filter(|(key, _)| !OWN.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    // The animators the model has no track for stay under their keys.
    let mut unknown: Map<String, Value> = Map::new();
    let mut seen: HashSet<GroupId> = HashSet::new();
    if let Some(animators) = item.get("animators").and_then(Value::as_object) {
        for (key, animator) in animators {
            let group = rig.by_uuid.get(key).copied().filter(|g| seen.insert(*g));
            match group.and_then(|group| track(group, animator, dropped, rig.old)) {
                Some(track) => out.tracks.push(track),
                None => {
                    let mut kept = animator.clone();
                    if !rig.old {
                        signs::to_old_animator(&mut kept);
                    }
                    unknown.insert(key.clone(), kept);
                }
            }
        }
    }
    if !unknown.is_empty() {
        rest.insert("animators".to_owned(), Value::Object(unknown));
    }
    out.extra = json_text(rest);
    let bytes = out.extra_bytes() as u64;
    Some((out, bytes))
}

/// An animation that stays JSON, in the signs the writer gives (see [`signs`]).
fn kept_animation(item: &Value, old: bool) -> Value {
    let mut kept = item.clone();
    if !old {
        signs::to_old_animation(&mut kept);
    }
    kept
}

/// Reads the `animations` member of a project against the groups of the model. At most
/// `budget` bytes of text are kept on the animations; the rest of the animations, and those that
/// would pass the limits of the model, are left as they are. `old` says that the project has the
/// signs of a version older than 5.0, which the values are converted from.
pub(crate) fn read(root: &Value, groups: &Groups, budget: u64, old: bool) -> Read {
    let rig = Rig::new(groups, old);
    let mut out = Read::default();
    let mut keyframes = 0_usize;
    let mut dropped = Dropped::default();
    for item in list(root, "animations") {
        if out.animations.len() >= MAX_ANIMATIONS {
            out.leftover.push(kept_animation(item, old));
            continue;
        }
        let mut local = Dropped::default();
        let Some((read, bytes)) = animation(item, &rig, &mut local) else {
            out.leftover.push(kept_animation(item, old));
            continue;
        };
        if keyframes + read.keyframe_count() > MAX_KEYFRAMES || out.bytes + bytes > budget {
            out.leftover.push(kept_animation(item, old));
            continue;
        }
        keyframes += read.keyframe_count();
        out.bytes += bytes;
        dropped.duplicates += local.duplicates;
        out.animations.push(read);
    }
    if dropped.duplicates > 0 {
        out.notices.push(Notice::Skipped {
            what: "keyframes at the same time as another".to_owned(),
            count: dropped.duplicates,
        });
    }
    out
}

/// A whole number as an integer, any other as the shortest decimal that reads back as the same
/// number: the way the editor that wrote the project prints it.
pub(crate) fn count(v: f32) -> Value {
    if v.is_finite() && v.fract().abs() < f32::EPSILON && v.abs() < 1.0e9 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the value is a whole number below a billion"
        )]
        let whole = v as i64;
        return Value::Number(Number::from(whole));
    }
    number(v)
}

/// A value of a data point; `flip` says that the writer gives the axis the old sign.
fn data_json(value: &Data, flip: bool) -> Value {
    match value {
        _ if flip => signs::write_old(value),
        Data::Num(n) => count(*n),
        Data::Text { text, .. } => Value::String(text.to_string()),
    }
}

fn point_json(point: &[Data; 3], channel: Channel) -> Value {
    let [fx, fy, fz] = signs::flipped(channel);
    json!({
        "x": data_json(&point[0], fx),
        "y": data_json(&point[1], fy),
        "z": data_json(&point[2], fz),
    })
}

fn vec_json(v: [f32; 3]) -> Value {
    Value::Array(v.into_iter().map(count).collect())
}

fn keyframe_json(channel: Channel, key: &Keyframe, uuid: &str) -> Value {
    let mut out = Map::new();
    out.insert("channel".to_owned(), json!(channel.key()));
    let mut points = vec![point_json(&key.value, channel)];
    points.extend(key.post.as_ref().map(|post| point_json(post, channel)));
    out.insert("data_points".to_owned(), Value::Array(points));
    out.insert("uuid".to_owned(), json!(uuid));
    out.insert("time".to_owned(), count(key.time));
    out.insert("color".to_owned(), json!(key.color));
    if channel == Channel::Scale
        && let Some(uniform) = key.uniform
    {
        out.insert("uniform".to_owned(), json!(uniform));
    }
    out.insert(
        "interpolation".to_owned(),
        json!(match key.interp {
            Interp::Linear => "linear",
            Interp::CatmullRom => "catmullrom",
            Interp::Bezier => "bezier",
            Interp::Step => "step",
        }),
    );
    if key.interp == Interp::Bezier {
        let mut handles = key.bezier.clone().unwrap_or_default();
        signs::flip_handles(&mut handles, channel);
        out.insert("bezier_linked".to_owned(), json!(handles.linked));
        out.insert("bezier_left_time".to_owned(), vec_json(handles.left_time));
        out.insert("bezier_left_value".to_owned(), vec_json(handles.left_value));
        out.insert("bezier_right_time".to_owned(), vec_json(handles.right_time));
        out.insert(
            "bezier_right_value".to_owned(),
            vec_json(handles.right_value),
        );
    }
    Value::Object(out)
}

/// The members of a JSON text that is an object.
fn members(text: Option<&Arc<str>>) -> Map<String, Value> {
    text.and_then(|t| serde_json::from_str::<Value>(t).ok())
        .and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// Fresh uuids for the animations and keyframes that have none, never one that a node of the
/// document already has.
struct Fresh<'a> {
    taken: &'a HashSet<String>,
    next: usize,
}

impl Fresh<'_> {
    fn uuid(&mut self, kind: u32) -> String {
        loop {
            let uuid = format!("{kind:08x}-0000-4000-8000-{:012x}", self.next);
            self.next += 1;
            if !self.taken.contains(&uuid) {
                return uuid;
            }
        }
    }
}

/// The `animations` member for a document: the animations of the model, then those the importer
/// left as they were. `group_uuid` gives the uuid a group is written with.
pub(crate) fn write(
    doc: &Document,
    leftover: &[Value],
    mut group_uuid: impl FnMut(GroupId) -> String,
    taken: &HashSet<String>,
) -> Vec<Value> {
    let mut used: HashSet<String> = taken.clone();
    for animation in doc.animations.items() {
        used.extend(animation.uuid.iter().cloned());
        for track in &animation.tracks {
            for channel in Channel::ALL {
                used.extend(track.channel(channel).iter().filter_map(|k| k.uuid.clone()));
            }
        }
    }
    let mut fresh = Fresh {
        taken: &used,
        next: 0,
    };
    let mut out = Vec::with_capacity(doc.animations.len() + leftover.len());
    for animation in doc.animations.items() {
        let mut item = members(animation.extra.as_ref());
        let unknown = match item.remove("animators") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut animators: Map<String, Value> = Map::new();
        for track in &animation.tracks {
            let Some(group) = doc.groups.get(track.group) else {
                continue;
            };
            let mut node = members(track.extra.as_ref());
            node.insert("name".to_owned(), json!(&*group.name));
            node.insert("type".to_owned(), json!("bone"));
            let mut keys = Vec::with_capacity(track.keyframe_count());
            for channel in track.listed() {
                for key in track.channel(channel) {
                    let uuid = key.uuid.clone().unwrap_or_else(|| fresh.uuid(3));
                    keys.push(keyframe_json(channel, key, &uuid));
                }
            }
            if !keys.is_empty() {
                node.insert("keyframes".to_owned(), Value::Array(keys));
            }
            animators.insert(group_uuid(track.group), Value::Object(node));
        }
        item.insert(
            "uuid".to_owned(),
            json!(animation.uuid.clone().unwrap_or_else(|| fresh.uuid(4))),
        );
        item.insert("name".to_owned(), json!(&*animation.name));
        item.insert(
            "loop".to_owned(),
            json!(match animation.loop_mode {
                LoopMode::Once => "once",
                LoopMode::Loop => "loop",
                LoopMode::Hold => "hold",
            }),
        );
        item.insert("override".to_owned(), json!(animation.overrides));
        item.insert("length".to_owned(), count(animation.length));
        item.insert("snapping".to_owned(), json!(animation.snapping));
        // An animator of the model wins over an unknown one under the same key.
        let mut all: Map<String, Value> = unknown;
        all.extend(animators);
        if !all.is_empty() {
            item.insert("animators".to_owned(), Value::Object(all));
        }
        out.push(Value::Object(item));
    }
    out.extend(leftover.iter().cloned());
    out
}
