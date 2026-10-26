//! The animations in the `cuboid.json` entry (spec: animation A2 5).
//!
//! The payload of a `.cubik` file knows nothing of animations. They go into the entry, which Cubik
//! ignores and drops when it saves the file again, keyed by the references of the group tree
//! ([`crate::tree`]): a track names its group as `g<position in the tree's list>`. A track whose
//! group the tree did not bring back is dropped with a warning; the animation stays. The text that a
//! `.bbmodel` project gave an animation, which the model keeps to write back, goes along as it is.

use std::sync::Arc;

use cuboid_model::{
    Animation, BoneTrack, Channel, Handles, Interp, Keyframe, ListSnap, LoopMode, MAX_ANIMATIONS,
    MAX_EXTRA_BYTES, MAX_KEYFRAMES, Snapshot, Value,
};
use serde::{Deserialize, Serialize};

fn no(flag: &bool) -> bool {
    !*flag
}

/// A value of a keyframe: a number, or text as a file wrote it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Val {
    Num(f32),
    Text(String),
}

impl From<&Value> for Val {
    fn from(value: &Value) -> Self {
        match value {
            Value::Num(n) => Val::Num(*n),
            Value::Text { text, .. } => Val::Text(text.to_string()),
        }
    }
}

impl From<Val> for Value {
    fn from(value: Val) -> Self {
        match value {
            Val::Num(n) => Value::Num(n),
            Val::Text(text) => Value::from_text(&text),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct HandleNode {
    linked: bool,
    left_time: [f32; 3],
    left_value: [f32; 3],
    right_time: [f32; 3],
    right_value: [f32; 3],
}

#[derive(Debug, Serialize, Deserialize)]
struct KeyNode {
    time: f32,
    value: [Val; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    post: Option<[Val; 3]>,
    #[serde(default)]
    interp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bezier: Option<HandleNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uuid: Option<String>,
    #[serde(default = "minus_one", skip_serializing_if = "is_minus_one")]
    color: i8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uniform: Option<bool>,
}

fn minus_one() -> i8 {
    -1
}

fn is_minus_one(color: &i8) -> bool {
    *color == -1
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackNode {
    /// `g<position>` in the list of the group tree.
    group: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rotation: Vec<KeyNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    position: Vec<KeyNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scale: Vec<KeyNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra: Option<String>,
    /// The order a project lists the channels in; absent for the usual one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    order: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnimationNode {
    name: String,
    length: f32,
    #[serde(rename = "loop", default)]
    loop_mode: String,
    snapping: u32,
    #[serde(default, skip_serializing_if = "no")]
    overrides: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uuid: Option<String>,
    #[serde(default)]
    tracks: Vec<TrackNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra: Option<String>,
}

/// The `animations` member of the entry.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct List {
    animations: Vec<AnimationNode>,
}

fn interp_name(interp: Interp) -> &'static str {
    match interp {
        Interp::Linear => "linear",
        Interp::CatmullRom => "catmullrom",
        Interp::Bezier => "bezier",
        Interp::Step => "step",
    }
}

fn interp_of(name: &str) -> Interp {
    match name {
        "catmullrom" => Interp::CatmullRom,
        "bezier" => Interp::Bezier,
        "step" => Interp::Step,
        _ => Interp::Linear,
    }
}

fn key_node(key: &Keyframe) -> KeyNode {
    let triple = |values: &[Value; 3]| {
        let [x, y, z] = values;
        [Val::from(x), Val::from(y), Val::from(z)]
    };
    KeyNode {
        time: key.time,
        value: triple(&key.value),
        post: key.post.as_ref().map(triple),
        interp: interp_name(key.interp).to_owned(),
        bezier: key.bezier.as_ref().map(|h| HandleNode {
            linked: h.linked,
            left_time: h.left_time,
            left_value: h.left_value,
            right_time: h.right_time,
            right_value: h.right_value,
        }),
        uuid: key.uuid.clone(),
        color: key.color,
        uniform: key.uniform,
    }
}

fn key_of(node: KeyNode) -> Keyframe {
    let triple = |values: [Val; 3]| values.map(Value::from);
    Keyframe {
        time: node.time,
        value: triple(node.value),
        post: node.post.map(triple),
        interp: interp_of(&node.interp),
        bezier: node.bezier.map(|h| Handles {
            linked: h.linked,
            left_time: h.left_time,
            left_value: h.left_value,
            right_time: h.right_time,
            right_value: h.right_value,
        }),
        uuid: node.uuid,
        color: node.color,
        uniform: node.uniform,
    }
}

/// The animations of a snapshot as the entry writes them; `None` when it has none. A track whose
/// group the snapshot's tree does not list is left out.
pub(crate) fn encode(snapshot: &Snapshot) -> Option<List> {
    let animations = snapshot.animations.as_ref().filter(|a| !a.ids.is_empty())?;
    let at: std::collections::HashMap<_, _> = snapshot
        .groups
        .iter()
        .flat_map(|g| g.list.ids.iter().enumerate().map(|(at, id)| (*id, at)))
        .collect();
    let nodes = animations
        .items
        .iter()
        .map(|a| AnimationNode {
            name: a.name.to_string(),
            length: a.length,
            loop_mode: match a.loop_mode {
                LoopMode::Once => "once",
                LoopMode::Loop => "loop",
                LoopMode::Hold => "hold",
            }
            .to_owned(),
            snapping: a.snapping,
            overrides: a.overrides,
            uuid: a.uuid.clone(),
            tracks: a
                .tracks
                .iter()
                .filter_map(|t| {
                    let keys = |channel| t.channel(channel).iter().map(key_node).collect();
                    Some(TrackNode {
                        group: format!("g{}", at.get(&t.group)?),
                        rotation: keys(Channel::Rotation),
                        position: keys(Channel::Position),
                        scale: keys(Channel::Scale),
                        extra: t.extra.as_ref().map(|e| e.to_string()),
                        order: if t.listed() == Channel::ALL {
                            Vec::new()
                        } else {
                            t.listed().iter().map(|c| c.key().to_owned()).collect()
                        },
                    })
                })
                .collect(),
            extra: a.extra.as_ref().map(|e| e.to_string()),
        })
        .collect();
    Some(List { animations: nodes })
}

/// What reading the member gave.
pub(crate) struct Read {
    pub animations: Option<ListSnap<Animation>>,
    pub warnings: Vec<String>,
}

fn group_at(text: &str) -> Option<usize> {
    text.strip_prefix('g')?.parse().ok()
}

/// Builds the animations of the member. `group_ids` gives the group each position of the tree's
/// list became, `None` for a group the tree did not bring back. A damaged member gives no
/// animations and a warning; an animation that breaks a bound of the model is left out.
pub(crate) fn decode(
    value: serde_json::Value,
    group_ids: &[Option<cuboid_model::GroupId>],
) -> Read {
    let mut warnings = Vec::new();
    let list: List = match serde_json::from_value(value) {
        Ok(list) => list,
        Err(error) => {
            warnings.push(format!(
                "the animations in the `cuboid.json` entry are damaged and ignored ({error})"
            ));
            return Read {
                animations: None,
                warnings,
            };
        }
    };
    let mut out: Vec<Animation> = Vec::new();
    let (mut keyframes, mut bytes) = (0_usize, 0_usize);
    let (mut lost_tracks, mut left_out) = (0_usize, 0_usize);
    for node in list.animations {
        if out.len() >= MAX_ANIMATIONS {
            left_out += 1;
            continue;
        }
        let mut animation = Animation::new(node.name, node.length);
        animation.loop_mode = match node.loop_mode.as_str() {
            "loop" => LoopMode::Loop,
            "hold" => LoopMode::Hold,
            _ => LoopMode::Once,
        };
        animation.snapping = node.snapping;
        animation.overrides = node.overrides;
        animation.uuid = node
            .uuid
            .filter(|u| u.len() <= 64 && !u.chars().any(char::is_control));
        animation.extra = node.extra.map(Arc::from);
        for track in node.tracks {
            let group = group_at(&track.group)
                .and_then(|at| group_ids.get(at))
                .copied()
                .flatten();
            let Some(group) = group else {
                lost_tracks += 1;
                continue;
            };
            let mut out_track = BoneTrack::new(group);
            out_track.rotation = track.rotation.into_iter().map(key_of).collect();
            out_track.position = track.position.into_iter().map(key_of).collect();
            out_track.scale = track.scale.into_iter().map(key_of).collect();
            out_track.extra = track.extra.map(Arc::from);
            let listed: Vec<Channel> = track
                .order
                .iter()
                .filter_map(|name| Channel::from_key(name))
                .collect();
            if let [a, b, c] = listed.as_slice() {
                out_track.order = [*a, *b, *c];
            }
            animation.tracks.push(out_track);
        }
        let (count, extra) = (animation.keyframe_count(), animation.extra_bytes());
        if animation.check().is_err()
            || keyframes + count > MAX_KEYFRAMES
            || bytes + extra > MAX_EXTRA_BYTES
        {
            left_out += 1;
            continue;
        }
        keyframes += count;
        bytes += extra;
        out.push(animation);
    }
    if lost_tracks > 0 {
        warnings.push(format!(
            "{lost_tracks} animated bones in the `cuboid.json` entry name groups that the file \
             does not have any more and were left out"
        ));
    }
    if left_out > 0 {
        warnings.push(format!(
            "{left_out} animations in the `cuboid.json` entry are damaged or over the limits of a \
             model and were left out"
        ));
    }
    Read {
        animations: (!out.is_empty()).then(|| ListSnap::fresh(out)),
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use cuboid_model::{GroupId, ObjId};

    use super::*;

    fn sample() -> (Snapshot, Vec<GroupId>) {
        use cuboid_model::{Categories, Group, GroupsBuilder, Selection};
        let mut b = GroupsBuilder::new();
        let a = b
            .add_group(Group::new("A", glam::Vec3::ZERO), None)
            .unwrap();
        let c = b
            .add_group(Group::new("C", glam::Vec3::X), Some(a))
            .unwrap();
        let groups = b.finish().snapshot(&Selection::default());
        let mut walk = Animation::new("walk", 1.5);
        walk.loop_mode = LoopMode::Loop;
        walk.snapping = 30;
        walk.overrides = true;
        walk.uuid = Some("a1".to_owned());
        walk.extra = Some(Arc::from("{\"blend_weight\":\"1\"}"));
        let track = walk.track_mut(c);
        let mut curve = Keyframe::new(0.5, [1.0, 2.0, 3.0]);
        curve.interp = Interp::Bezier;
        curve.bezier = Some(Handles::default());
        curve.value[1] = Value::from_text("math.sin(query.anim_time)");
        curve.post = Some([Value::Num(4.0), Value::Num(5.0), Value::from_text("6")]);
        curve.uuid = Some("k1".to_owned());
        curve.color = 2;
        track.rotation = vec![Keyframe::new(0.0, [0.0; 3]), curve];
        track.scale = vec![{
            let mut k = Keyframe::new(0.0, [1.0; 3]);
            k.uniform = Some(true);
            k
        }];
        track.extra = Some(Arc::from("{\"rotation_global\":true}"));
        track.order = [Channel::Position, Channel::Rotation, Channel::Scale];
        walk.track_mut(a).position = vec![Keyframe::new(1.0, [0.0, 4.0, 0.0])];
        let idle = Animation::new("idle", 0.0);
        let ids = groups.list.ids.clone();
        let snapshot = Snapshot {
            categories: Categories::HIERARCHY | Categories::ANIMATION,
            props: None,
            view: None,
            elements: None,
            textures: None,
            voxels: None,
            meshes: None,
            display: None,
            groups: Some(groups),
            animations: Some(ListSnap::fresh([walk, idle])),
        };
        (snapshot, ids)
    }

    #[test]
    fn animations_come_back_with_every_field() {
        let (snapshot, ids) = sample();
        let list = encode(&snapshot).expect("a member");
        let value = serde_json::to_value(&list).unwrap();
        // The loader maps the positions of the tree's list to the ids of the groups it made.
        let new_ids: Vec<Option<GroupId>> = ids.iter().map(|_| Some(ObjId::next())).collect();
        let read = decode(value, &new_ids);
        assert!(read.warnings.is_empty(), "{:?}", read.warnings);
        let back = read.animations.expect("animations");
        let before = snapshot.animations.as_ref().unwrap();
        assert_eq!(back.items.len(), 2);
        for (a, b) in before.items.iter().zip(&back.items) {
            assert_eq!(a.name, b.name);
            assert_eq!(
                (a.length, a.loop_mode, a.snapping, a.overrides),
                (b.length, b.loop_mode, b.snapping, b.overrides)
            );
            assert_eq!((&a.uuid, &a.extra), (&b.uuid, &b.extra));
            assert_eq!(a.tracks.len(), b.tracks.len());
            for (x, y) in a.tracks.iter().zip(&b.tracks) {
                let index = ids.iter().position(|id| *id == x.group).unwrap();
                assert_eq!(Some(y.group), new_ids[index]);
                for channel in Channel::ALL {
                    assert_eq!(x.channel(channel), y.channel(channel), "{channel:?}");
                }
                assert_eq!(x.extra, y.extra);
                assert_eq!(x.order, y.order);
            }
        }
        assert!(back.items.iter().all(|a| a.check().is_ok()));
    }

    #[test]
    fn a_model_without_animations_writes_no_member() {
        let (mut snapshot, _) = sample();
        snapshot.animations = None;
        assert!(encode(&snapshot).is_none());
        snapshot.animations = Some(ListSnap::fresh(Vec::new()));
        assert!(encode(&snapshot).is_none());
    }

    #[test]
    fn a_track_of_a_group_the_tree_lost_is_dropped_and_the_animation_stays() {
        let (snapshot, ids) = sample();
        let value = serde_json::to_value(encode(&snapshot).unwrap()).unwrap();
        let mut new_ids: Vec<Option<GroupId>> = ids.iter().map(|_| Some(ObjId::next())).collect();
        new_ids[1] = None;
        let read = decode(value.clone(), &new_ids);
        assert_eq!(read.warnings.len(), 1, "{:?}", read.warnings);
        let back = read.animations.unwrap();
        assert_eq!(back.items[0].tracks.len(), 1);
        // Without a tree no track can be placed.
        let read = decode(value, &[]);
        assert_eq!(read.animations.unwrap().items[0].tracks.len(), 0);
    }

    #[test]
    fn damaged_and_out_of_bounds_members_are_refused_not_followed() {
        let ids = [Some(ObjId::next())];
        let read = decode(serde_json::json!({"animations": 7}), &ids);
        assert!(read.animations.is_none());
        assert!(read.warnings[0].contains("damaged"));

        // A time outside the range, keyframes out of order and a bad frame rate each cost the
        // animation, and the good one beside them stays.
        let key = |time: f64| serde_json::json!({"time": time, "value": [0.0, 0.0, 0.0], "interp": "linear"});
        let track =
            |keys: Vec<serde_json::Value>| serde_json::json!({"group": "g0", "rotation": keys});
        let animation = |name: &str, length: f64, snapping: u32, keys: Vec<serde_json::Value>| {
            serde_json::json!({"name": name, "length": length, "snapping": snapping,
                "tracks": [track(keys)]})
        };
        let value = serde_json::json!({"animations": [
            animation("late", 1.0, 24, vec![key(4000.0)]),
            animation("backwards", 1.0, 24, vec![key(0.5), key(0.25)]),
            animation("rate", 1.0, 5, vec![key(0.5)]),
            animation("long", 99999.0, 24, vec![]),
            animation("good", 1.0, 24, vec![key(0.25), key(0.5)]),
        ]});
        let read = decode(value, &ids);
        let back = read.animations.unwrap();
        assert_eq!(back.items.len(), 1);
        assert_eq!(&*back.items[0].name, "good");
        assert_eq!(read.warnings.len(), 1);
    }

    #[test]
    fn the_counts_are_bounded() {
        let ids = [Some(ObjId::next())];
        let many: Vec<serde_json::Value> = (0..MAX_ANIMATIONS + 5)
            .map(|i| serde_json::json!({"name": format!("a{i}"), "length": 1.0, "snapping": 24}))
            .collect();
        let read = decode(serde_json::json!({ "animations": many }), &ids);
        assert_eq!(read.animations.unwrap().items.len(), MAX_ANIMATIONS);
        assert_eq!(read.warnings.len(), 1);

        let keys: Vec<serde_json::Value> = (0..MAX_KEYFRAMES / 2 + 1)
            .map(|i| serde_json::json!({"time": i as f64 / 100.0, "value": [0.0, 0.0, 0.0]}))
            .collect();
        let big = |name: &str| {
            serde_json::json!({"name": name, "length": 1.0, "snapping": 24,
                "tracks": [{"group": "g0", "rotation": keys}]})
        };
        let read = decode(
            serde_json::json!({"animations": [big("one"), big("two")]}),
            &ids,
        );
        assert_eq!(read.animations.unwrap().items.len(), 1);
    }
}
