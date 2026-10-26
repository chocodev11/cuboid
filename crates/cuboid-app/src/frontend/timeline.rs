//! What the timeline and the Animate tab show: the bones that have keyframes in the chosen
//! animation and the bones that are selected, with their keyframes, and the pose of the primary
//! bone at the playhead (spec: animate A3 3).
//!
//! The rows are rebuilt when the animations, the groups or the bone selection change and kept
//! otherwise; the pose of the primary bone is small and made again each frame.

use std::sync::Arc;

use cuboid_core::Core;
use cuboid_model::{
    Animation, AnimationId, BoneTrack, Channel, Document, GroupId, Interp, Keyframe, ObjRef, Rev,
    TIME_EPSILON, Value, group_values,
};
use cuboid_ui::{BoneRow, ChannelKind, KeyKind, KeyMark, KeyNumber, PoseView, TimelineView};

use crate::uimodel::node_id;

/// What the rows were built from.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    animation: Option<AnimationId>,
    animations: Rev,
    groups: Rev,
    selected: Vec<GroupId>,
}

/// The view of the timeline, kept between frames.
#[derive(Default)]
pub struct TimelineModel {
    key: Option<Key>,
    rev: u64,
    view: TimelineView,
}

/// The bone an object belongs to: a group itself, the group that holds an element or a mesh.
pub fn bone_of(doc: &Document, obj: ObjRef) -> Option<GroupId> {
    match obj {
        ObjRef::Group(id) => Some(id),
        ObjRef::Element(_) | ObjRef::Mesh(_) => doc.groups.parent_of(obj),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

fn kind_of(interp: Interp) -> KeyKind {
    match interp {
        Interp::Linear => KeyKind::Linear,
        Interp::CatmullRom => KeyKind::CatmullRom,
        Interp::Bezier => KeyKind::Bezier,
        Interp::Step => KeyKind::Step,
    }
}

fn number_of(value: &Value) -> KeyNumber {
    match value {
        Value::Num(n) => KeyNumber::Num(*n),
        Value::Text {
            number: Some(n), ..
        } => KeyNumber::Num(*n),
        Value::Text { text, .. } => KeyNumber::Expr(Arc::clone(text)),
    }
}

fn marks(keys: &[Keyframe]) -> Arc<[KeyMark]> {
    keys.iter()
        .map(|key| KeyMark {
            time: key.time,
            kind: kind_of(key.interp),
            value: [0, 1, 2].map(|axis| number_of(&key.value[axis])),
            jump: key.post.is_some(),
        })
        .collect()
}

fn bone_row(
    doc: &Document,
    group: GroupId,
    track: Option<&BoneTrack>,
    selected: bool,
) -> Option<BoneRow> {
    let record = doc.groups.get(group)?;
    let channels = Channel::ALL
        .map(|channel| track.map_or_else(|| Arc::from(Vec::new()), |t| marks(t.channel(channel))));
    Some(BoneRow {
        id: node_id(ObjRef::Group(group))?,
        name: Arc::from(&*record.name),
        depth: doc.groups.depth(group) - 1,
        selected,
        channels,
    })
}

impl TimelineModel {
    /// The view as it was last made.
    pub fn view(&self) -> &TimelineView {
        &self.view
    }

    /// Makes the view for the animation the player has and the playhead it stands at.
    pub fn update(&mut self, core: &Core, animation: Option<AnimationId>, time: f32) {
        let doc = core.doc();
        let mut selected: Vec<GroupId> = Vec::new();
        for bone in core
            .selection()
            .list
            .iter()
            .filter_map(|&obj| bone_of(doc, obj))
        {
            if !selected.contains(&bone) {
                selected.push(bone);
            }
        }
        let key = Key {
            animation,
            animations: doc.animations.rev(),
            groups: doc.groups.rev(),
            selected,
        };
        if self.key.as_ref() != Some(&key) {
            self.rev += 1;
            self.view.rev = self.rev;
            self.view.has_groups = !doc.groups.is_empty();
            let found = animation.and_then(|id| doc.animations.get(id));
            self.view.bones = rows(doc, found, &key.selected);
            self.key = Some(key);
        }
        self.view.pose = found_pose(core, animation, time);
    }
}

/// The bones in outliner order that have a track or are selected.
fn rows(doc: &Document, animation: Option<&Animation>, selected: &[GroupId]) -> Arc<[BoneRow]> {
    let mut out = Vec::new();
    for (obj, _) in doc.outline() {
        let ObjRef::Group(group) = obj else {
            continue;
        };
        let track = animation
            .and_then(|a| a.track(group))
            .filter(|t| !t.is_empty());
        let chosen = selected.contains(&group);
        if (track.is_some() || chosen)
            && let Some(row) = bone_row(doc, group, track, chosen)
        {
            out.push(row);
        }
    }
    out.into()
}

/// The primary bone with the values of its channels at the playhead.
fn found_pose(core: &Core, animation: Option<AnimationId>, time: f32) -> Option<PoseView> {
    let doc = core.doc();
    let animation = doc.animations.get(animation?)?;
    let group = bone_of(doc, core.selection().primary?)?;
    let values = group_values(animation, group, time);
    let keyed = Channel::ALL.map(|channel| {
        animation.track(group).is_some_and(|track| {
            track
                .channel(channel)
                .iter()
                .any(|key| (key.time - time).abs() < TIME_EPSILON)
        })
    });
    Some(PoseView {
        bone: node_id(ObjRef::Group(group))?,
        name: Arc::from(&*doc.groups.get(group)?.name),
        values: [values.rotation, values.position, values.scale],
        keyed,
    })
}

/// The channel of the UI as the model has it.
pub fn channel_of(channel: ChannelKind) -> Channel {
    match channel {
        ChannelKind::Rotation => Channel::Rotation,
        ChannelKind::Position => Channel::Position,
        ChannelKind::Scale => Channel::Scale,
    }
}
