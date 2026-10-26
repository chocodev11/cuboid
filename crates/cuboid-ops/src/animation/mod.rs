//! Animation operations: add, change and remove animations, and set, move and remove the
//! keyframes of their tracks (spec: animation A2 4).
//!
//! Keyframes are named by their place: the group, the channel and the position in the channel,
//! which is ordered by time. A place is read at the time the operation runs.

use cuboid_model::{
    Animation, AnimationError, AnimationId, Channel, Document, EditCtx, GroupId, Interp, Keyframe,
    LoopMode, MAX_EXTRA_BYTES, MAX_KEYFRAMES, MAX_LENGTH, SNAPPING_RANGE, TIME_EPSILON, Value,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::groups::clean_name;

/// The fields of an animation that one call changes; `None` keeps a field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationFields {
    /// The name; double quotes are dropped.
    pub name: Option<String>,
    /// The length in seconds, between 0 and one hour.
    pub length: Option<f32>,
    /// What happens after the end.
    pub loop_mode: Option<LoopMode>,
    /// The frame rate of the snapping grid, between 10 and 500.
    pub snapping: Option<u32>,
}

/// A keyframe by its place.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeyRef {
    /// The animated group.
    pub group: GroupId,
    /// The channel.
    pub channel: Channel,
    /// The position among the keyframes of the channel, ordered by time.
    pub index: usize,
}

fn time_ok(time: f32) -> bool {
    time.is_finite() && (0.0..=MAX_LENGTH).contains(&time)
}

/// Adds an animation without tracks. An empty name becomes `animation`.
pub fn add(cx: &mut EditCtx<'_>, name: &str, length: f32) -> Result<AnimationId, AnimationError> {
    cx.doc
        .add_animation(Animation::new(clean_name(name, "animation"), length))
}

/// Removes an animation. Returns whether it was there.
pub fn remove(cx: &mut EditCtx<'_>, id: AnimationId) -> bool {
    if cx.doc.animations.get(id).is_none() {
        return false;
    }
    cx.doc.remove_animations(&FxHashSet::from_iter([id]));
    true
}

/// Changes the fields of an animation. Returns whether anything changed.
pub fn set_fields(
    cx: &mut EditCtx<'_>,
    id: AnimationId,
    fields: &AnimationFields,
) -> Result<bool, AnimationError> {
    let old = cx.doc.animations.get(id).ok_or(AnimationError::Missing)?;
    if fields.length.is_some_and(|l| !time_ok(l))
        || fields
            .snapping
            .is_some_and(|s| !(SNAPPING_RANGE.0..=SNAPPING_RANGE.1).contains(&s))
    {
        return Err(AnimationError::OutOfRange);
    }
    let name = fields.name.as_deref().map(|n| clean_name(n, &old.name));
    let changed = name.as_ref().is_some_and(|n| *n != old.name)
        || fields
            .length
            .is_some_and(|l| (l - old.length).abs() > f32::EPSILON)
        || fields.loop_mode.is_some_and(|m| m != old.loop_mode)
        || fields.snapping.is_some_and(|s| s != old.snapping);
    if !changed {
        return Ok(false);
    }
    cx.doc.edit_animation(id, |animation| {
        if let Some(name) = name {
            animation.name = name;
        }
        if let Some(length) = fields.length {
            animation.length = length;
        }
        if let Some(mode) = fields.loop_mode {
            animation.loop_mode = mode;
        }
        if let Some(snapping) = fields.snapping {
            animation.snapping = snapping;
        }
    });
    Ok(true)
}

/// Where a keyframe at `time` is in a channel: the position of the keyframe that is the same
/// keyframe, or the position to insert at.
fn find(keys: &[Keyframe], time: f32) -> Result<usize, usize> {
    let at = keys.partition_point(|k| k.time < time - TIME_EPSILON);
    match keys.get(at) {
        Some(k) if (k.time - time).abs() < TIME_EPSILON => Ok(at),
        _ => Err(at),
    }
}

/// Sets the value of a channel at a time: a keyframe at that time (within a hundred-thousandth of
/// a second) gets the value and loses a jump it had, any other time gets a new keyframe. The
/// interpolation of a new keyframe is linear. The animation grows to hold a keyframe after its
/// end. Returns whether anything changed.
pub fn set_keyframe(
    cx: &mut EditCtx<'_>,
    id: AnimationId,
    group: GroupId,
    channel: Channel,
    time: f32,
    value: [Value; 3],
    interp: Option<Interp>,
) -> Result<bool, AnimationError> {
    let animation = cx.doc.animations.get(id).ok_or(AnimationError::Missing)?;
    if cx.doc.groups.get(group).is_none() {
        return Err(AnimationError::Missing);
    }
    if !time_ok(time) || !value.iter().all(Value::is_sane) {
        return Err(AnimationError::OutOfRange);
    }
    let found = animation.track(group).and_then(|track| {
        let keys = track.channel(channel);
        find(keys, time).ok().and_then(|at| keys.get(at))
    });
    match found {
        Some(key) => {
            let same =
                key.post.is_none() && key.value == value && interp.is_none_or(|i| i == key.interp);
            if same && time <= animation.length {
                return Ok(false);
            }
        }
        None => {
            if cx.doc.animations.keyframe_count() >= MAX_KEYFRAMES {
                return Err(AnimationError::TooManyKeyframes);
            }
        }
    }
    cx.doc.edit_animation(id, |animation| {
        let keys = animation.track_mut(group).channel_mut(channel);
        match find(keys, time) {
            Ok(at) => {
                let key = &mut keys[at];
                key.value = value;
                key.post = None;
                if let Some(interp) = interp {
                    key.interp = interp;
                }
            }
            Err(at) => {
                let key = Keyframe {
                    value,
                    interp: interp.unwrap_or_default(),
                    ..Keyframe::new(time, [0.0; 3])
                };
                keys.insert(at, key);
            }
        }
        if time > animation.length {
            animation.length = time;
        }
    });
    Ok(true)
}

/// The places of `keys` grouped by track channel, each position once.
fn by_channel(keys: &[KeyRef]) -> FxHashMap<(GroupId, Channel), FxHashSet<usize>> {
    let mut map: FxHashMap<(GroupId, Channel), FxHashSet<usize>> = FxHashMap::default();
    for key in keys {
        map.entry((key.group, key.channel))
            .or_default()
            .insert(key.index);
    }
    map
}

/// Removes keyframes. A track that is left without keyframes goes, unless it holds members of
/// its project. Returns how many keyframes were removed.
pub fn remove_keyframes(cx: &mut EditCtx<'_>, id: AnimationId, keys: &[KeyRef]) -> usize {
    let Some(animation) = cx.doc.animations.get(id) else {
        return 0;
    };
    let places = by_channel(keys);
    let removable: usize = places
        .iter()
        .map(|(&(group, channel), indices)| {
            animation.track(group).map_or(0, |track| {
                let len = track.channel(channel).len();
                indices.iter().filter(|&&i| i < len).count()
            })
        })
        .sum();
    if removable == 0 {
        return 0;
    }
    cx.doc.edit_animation(id, |animation| {
        for ((group, channel), indices) in &places {
            let Some(track) = animation.tracks.iter_mut().find(|t| t.group == *group) else {
                continue;
            };
            let mut at = 0;
            track.channel_mut(*channel).retain(|_| {
                at += 1;
                !indices.contains(&(at - 1))
            });
        }
        animation
            .tracks
            .retain(|track| !track.is_empty() || track.extra.is_some());
    });
    removable
}

/// Moves keyframes in time by `delta` seconds. A moved keyframe replaces a keyframe of the same
/// channel that it lands on. The animation grows to hold a keyframe moved past its end. Returns how
/// many keyframes moved; a move that would leave the range of the animation is refused.
pub fn move_keyframes(
    cx: &mut EditCtx<'_>,
    id: AnimationId,
    keys: &[KeyRef],
    delta: f32,
) -> Result<usize, AnimationError> {
    let animation = cx.doc.animations.get(id).ok_or(AnimationError::Missing)?;
    if !delta.is_finite() {
        return Err(AnimationError::OutOfRange);
    }
    let places = by_channel(keys);
    let mut moving = 0_usize;
    for (&(group, channel), indices) in &places {
        let Some(track) = animation.track(group) else {
            continue;
        };
        let list = track.channel(channel);
        for &i in indices {
            let Some(key) = list.get(i) else {
                continue;
            };
            if !time_ok(key.time + delta) {
                return Err(AnimationError::OutOfRange);
            }
            moving += 1;
        }
    }
    if moving == 0 || delta.abs() < TIME_EPSILON * 0.1 {
        return Ok(0);
    }
    cx.doc.edit_animation(id, |animation| {
        let mut last = animation.length;
        for ((group, channel), indices) in &places {
            let Some(track) = animation.tracks.iter_mut().find(|t| t.group == *group) else {
                continue;
            };
            let list = track.channel_mut(*channel);
            let mut moved: Vec<Keyframe> = Vec::new();
            let mut stay: Vec<Keyframe> = Vec::new();
            for (i, key) in std::mem::take(list).into_iter().enumerate() {
                if indices.contains(&i) {
                    moved.push(key);
                } else {
                    stay.push(key);
                }
            }
            for key in &mut moved {
                key.time += delta;
            }
            moved.sort_by(|a, b| a.time.total_cmp(&b.time));
            // Keyframes that land on one another become the first of them.
            moved.dedup_by(|later, earlier| (later.time - earlier.time).abs() < TIME_EPSILON);
            stay.retain(|s| find(&moved, s.time).is_err());
            if let Some(end) = moved.last() {
                last = last.max(end.time);
            }
            stay.extend(moved);
            stay.sort_by(|a, b| a.time.total_cmp(&b.time));
            *list = stay;
        }
        animation.length = last.min(MAX_LENGTH);
    });
    Ok(moving)
}

/// Changes the interpolation of keyframes. Returns how many keyframes changed.
///
/// spec: animate A3 7
pub fn set_interp(
    cx: &mut EditCtx<'_>,
    id: AnimationId,
    keys: &[KeyRef],
    interp: Interp,
) -> Result<usize, AnimationError> {
    let animation = cx.doc.animations.get(id).ok_or(AnimationError::Missing)?;
    let places = by_channel(keys);
    let changing: usize = places
        .iter()
        .map(|(&(group, channel), indices)| {
            animation.track(group).map_or(0, |track| {
                let list = track.channel(channel);
                indices
                    .iter()
                    .filter(|&&i| list.get(i).is_some_and(|k| k.interp != interp))
                    .count()
            })
        })
        .sum();
    if changing == 0 {
        return Ok(0);
    }
    cx.doc.edit_animation(id, |animation| {
        for ((group, channel), indices) in &places {
            let Some(track) = animation.tracks.iter_mut().find(|t| t.group == *group) else {
                continue;
            };
            for (at, key) in track.channel_mut(*channel).iter_mut().enumerate() {
                if indices.contains(&at) {
                    key.interp = interp;
                }
            }
        }
    });
    Ok(changing)
}

/// Inserts a deep copy of an animation after it, named `<name>_copy`. The copy has no `.bbmodel`
/// identities: its own and those of its keyframes are made when a file is written.
///
/// spec: animate A3 7
pub fn duplicate(cx: &mut EditCtx<'_>, id: AnimationId) -> Result<AnimationId, AnimationError> {
    let source = cx.doc.animations.get(id).ok_or(AnimationError::Missing)?;
    let at = cx.doc.animations.position(id).map_or(0, |p| p + 1);
    if cx.doc.animations.keyframe_count() + source.keyframe_count() > MAX_KEYFRAMES {
        return Err(AnimationError::TooManyKeyframes);
    }
    let kept: usize = cx
        .doc
        .animations
        .items()
        .iter()
        .map(|a| a.extra_bytes())
        .sum();
    if kept + source.extra_bytes() > MAX_EXTRA_BYTES {
        return Err(AnimationError::OutOfRange);
    }
    let mut copy = source.clone();
    copy.name = clean_name(&format!("{}_copy", source.name), "animation");
    copy.uuid = None;
    for track in &mut copy.tracks {
        for channel in Channel::ALL {
            for key in track.channel_mut(channel) {
                key.uuid = None;
            }
        }
    }
    cx.doc.insert_animation(at, copy)
}

/// A keyframe taken off a track, with the bone and the channel it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedKey {
    /// The bone the keyframe belonged to.
    pub group: GroupId,
    /// The channel.
    pub channel: Channel,
    /// The keyframe as it was, time included.
    pub key: Keyframe,
}

/// Copies keyframes by place, earliest first. Places that name no keyframe are skipped.
///
/// spec: animate A3 7
pub fn copy_keys(doc: &Document, id: AnimationId, keys: &[KeyRef]) -> Vec<CopiedKey> {
    let Some(animation) = doc.animations.get(id) else {
        return Vec::new();
    };
    let mut seen = FxHashSet::default();
    let mut out: Vec<CopiedKey> = keys
        .iter()
        .filter(|&&place| seen.insert(place))
        .filter_map(|place| {
            let list = animation.track(place.group)?.channel(place.channel);
            Some(CopiedKey {
                group: place.group,
                channel: place.channel,
                key: list.get(place.index)?.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.key.time.total_cmp(&b.key.time));
    out
}

/// Puts copied keyframes into an animation with their relative times, the earliest at `time`.
/// Each goes to the bone it came from, or all go to `group` when they came from one bone. A
/// pasted keyframe replaces the one it lands on, loses the `.bbmodel` identity of its source and
/// the animation grows to hold it. Keyframes of bones the model no longer has are skipped.
/// Returns how many were put in; a paste that would leave the time range is refused.
///
/// spec: animate A3 7
pub fn paste_keys(
    cx: &mut EditCtx<'_>,
    id: AnimationId,
    clip: &[CopiedKey],
    time: f32,
    group: Option<GroupId>,
) -> Result<usize, AnimationError> {
    let animation = cx.doc.animations.get(id).ok_or(AnimationError::Missing)?;
    if group.is_some_and(|g| cx.doc.groups.get(g).is_none()) {
        return Err(AnimationError::Missing);
    }
    let Some(first) = clip.iter().map(|c| c.key.time).min_by(f32::total_cmp) else {
        return Ok(0);
    };
    let one_bone = clip.iter().all(|c| c.group == clip[0].group);
    let offset = time - first;
    let landing: Vec<(GroupId, Channel, f32, &Keyframe)> = clip
        .iter()
        .map(|c| {
            let target = group.filter(|_| one_bone).unwrap_or(c.group);
            (target, c.channel, c.key.time + offset, &c.key)
        })
        .filter(|(target, ..)| cx.doc.groups.get(*target).is_some())
        .collect();
    if landing.iter().any(|&(_, _, at, _)| !time_ok(at)) {
        return Err(AnimationError::OutOfRange);
    }
    let added = landing
        .iter()
        .filter(|&&(target, channel, at, _)| {
            animation
                .track(target)
                .is_none_or(|t| find(t.channel(channel), at).is_err())
        })
        .count();
    if cx.doc.animations.keyframe_count() + added > MAX_KEYFRAMES {
        return Err(AnimationError::TooManyKeyframes);
    }
    if landing.is_empty() {
        return Ok(0);
    }
    cx.doc.edit_animation(id, |animation| {
        for &(target, channel, at, key) in &landing {
            let keys = animation.track_mut(target).channel_mut(channel);
            let pasted = Keyframe {
                time: at,
                uuid: None,
                ..key.clone()
            };
            match find(keys, at) {
                Ok(i) => keys[i] = pasted,
                Err(i) => keys.insert(i, pasted),
            }
            animation.length = animation.length.max(at);
        }
    });
    Ok(landing.len())
}

#[cfg(test)]
mod tests;
