//! Keyframe animations: the data, the list and its bounds (spec: animation A2 2).
//!
//! An [`Animation`] holds one [`BoneTrack`] per animated group, and a track holds the keyframes of
//! the three channels of a bone: rotation, position and scale. The values are the numbers of the
//! `.bbmodel` convention (see [`crate::pose`]); a value that a file wrote as text, such as a Molang
//! expression, is kept as it was written. The members of a file that the model has no field for
//! stay as JSON text on the animation or the track and are written back unchanged.
//!
//! Animations get ids from the same counter as objects, so an id is never issued twice in a
//! process, and the list keeps them through undo like every other list.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use thiserror::Error;

use crate::contract::Document;
use crate::groups::{GroupId, Groups};
use crate::ids::{ObjId, Rev};
use crate::journal::{ChangeJournal, DocFlags};
use crate::list::{ListSnap, ObjList};
use crate::objects::Label;

/// Identity of an animation. It is an [`ObjId`].
pub type AnimationId = ObjId;

/// The most animations a document holds.
pub const MAX_ANIMATIONS: usize = 4_096;

/// The most keyframes a document holds, in all animations together.
pub const MAX_KEYFRAMES: usize = 100_000;

/// The longest animation, in seconds. Keyframe times lie between 0 and this.
pub const MAX_LENGTH: f32 = 3_600.0;

/// The most bytes of JSON text the animations keep for a file to write back, in all animations
/// together (the cap of the members that a `.bbmodel` import keeps).
pub const MAX_EXTRA_BYTES: usize = 64 << 20;

/// The lowest and the highest frame rate of the snapping grid, as the project format limits it.
pub const SNAPPING_RANGE: (u32, u32) = (10, 500);

/// The frame rate a new animation snaps to.
pub const DEFAULT_SNAPPING: u32 = 24;

/// Two keyframes closer in time than this are the same keyframe.
pub const TIME_EPSILON: f32 = 1e-4;

/// The longest expression, in bytes, that a keyframe value written by hand may hold.
pub const MAX_EXPRESSION_BYTES: usize = 1_024;

/// Why a change of the animations was refused. The document is untouched in every case.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Error)]
pub enum AnimationError {
    /// The document would hold more animations than [`MAX_ANIMATIONS`].
    #[error("a model holds at most 4096 animations")]
    TooMany,
    /// The document would hold more keyframes than [`MAX_KEYFRAMES`].
    #[error("a model holds at most 100000 keyframes")]
    TooManyKeyframes,
    /// The animation or the group is not in the model.
    #[error("the animation or group is not in the model")]
    Missing,
    /// A time, length or frame rate is out of range.
    #[error("a time, length or frame rate is out of range")]
    OutOfRange,
}

/// What an animation does when the playhead passes its length.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum LoopMode {
    /// Plays once; afterwards the model is at rest.
    #[default]
    Once,
    /// Starts again from the beginning.
    Loop,
    /// Plays once and stays in the last pose.
    Hold,
}

/// How the value changes from a keyframe to the next one.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum Interp {
    /// Straight line.
    #[default]
    Linear,
    /// A smooth curve through the neighbouring keyframes.
    CatmullRom,
    /// A curve shaped by the handles of the two keyframes.
    Bezier,
    /// Keeps the value until the next keyframe.
    Step,
}

/// A channel of a bone.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Channel {
    /// Degrees added to the rotation of the bone.
    Rotation,
    /// Model units added to the position of the bone.
    Position,
    /// Factor applied to the size of the bone about its pivot.
    Scale,
}

impl Channel {
    /// The three channels in file order.
    pub const ALL: [Channel; 3] = [Channel::Rotation, Channel::Position, Channel::Scale];

    /// The name a `.bbmodel` project gives the channel.
    pub fn key(self) -> &'static str {
        match self {
            Channel::Rotation => "rotation",
            Channel::Position => "position",
            Channel::Scale => "scale",
        }
    }

    /// The channel a project name stands for.
    pub fn from_key(key: &str) -> Option<Channel> {
        Channel::ALL.into_iter().find(|c| c.key() == key)
    }

    /// The value of a channel that has no keyframes: 1 for the scale, 0 otherwise.
    pub fn rest(self) -> f32 {
        match self {
            Channel::Scale => 1.0,
            _ => 0.0,
        }
    }
}

/// One component of a keyframe value: a number, or the text a file wrote.
///
/// A file stores most values as text, numbers included. The text is kept as it was, and `number` is
/// what it says when it is a plain number. An expression has none; playing counts it as the last
/// number before it (spec: animation A2 3).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A number.
    Num(f32),
    /// Text as the file wrote it.
    Text {
        /// The text.
        text: Arc<str>,
        /// The number the text is, when it is only a number.
        number: Option<f32>,
    },
}

impl Value {
    /// The value of a text, which may be a number or an expression.
    pub fn from_text(text: &str) -> Value {
        let number = text.trim().parse::<f32>().ok().filter(|n| n.is_finite());
        Value::Text {
            text: Arc::from(text),
            number,
        }
    }

    /// The number the value is; `None` for an expression.
    pub fn number(&self) -> Option<f32> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Text { number, .. } => *number,
        }
    }

    /// Whether the value is an expression that playing cannot evaluate.
    pub fn is_expression(&self) -> bool {
        self.number().is_none()
    }

    /// Whether the value is a finite number or an expression.
    pub fn is_sane(&self) -> bool {
        match self {
            Value::Num(n) => n.is_finite(),
            Value::Text { number, .. } => number.is_none_or(f32::is_finite),
        }
    }
}

/// The handles of a keyframe with the Bezier interpolation, per axis. A handle is an offset from
/// the keyframe: the time offset of the left handle is at most 0, that of the right one at least
/// 0, and playing clamps both to the gap to the neighbouring keyframe.
#[derive(Clone, Debug, PartialEq)]
pub struct Handles {
    /// Moving one handle moves the other one to the opposite side.
    pub linked: bool,
    /// Time offsets of the handle before the keyframe.
    pub left_time: [f32; 3],
    /// Value offsets of the handle before the keyframe.
    pub left_value: [f32; 3],
    /// Time offsets of the handle after the keyframe.
    pub right_time: [f32; 3],
    /// Value offsets of the handle after the keyframe.
    pub right_value: [f32; 3],
}

impl Default for Handles {
    /// Short flat handles, as a new keyframe has them.
    fn default() -> Self {
        Self {
            linked: true,
            left_time: [-0.1; 3],
            left_value: [0.0; 3],
            right_time: [0.1; 3],
            right_value: [0.0; 3],
        }
    }
}

impl Handles {
    fn is_sane(&self) -> bool {
        [
            self.left_time,
            self.left_value,
            self.right_time,
            self.right_value,
        ]
        .iter()
        .flatten()
        .all(|v| v.is_finite())
    }
}

/// A keyframe of one channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    /// Seconds from the start, between 0 and [`MAX_LENGTH`].
    pub time: f32,
    /// The values on X, Y and Z at the keyframe; the values the channel arrives with.
    pub value: [Value; 3],
    /// The values the channel leaves with, when they differ from `value` (a jump).
    pub post: Option<[Value; 3]>,
    /// How the channel goes on to the next keyframe.
    pub interp: Interp,
    /// The handles of the Bezier interpolation; kept for the other interpolations too, so
    /// switching back does not lose them.
    pub bezier: Option<Handles>,
    /// The identity a `.bbmodel` project gave the keyframe.
    pub uuid: Option<String>,
    /// The marker colour index of the timeline, -1 for none.
    pub color: i8,
    /// Whether a scale keyframe's three components move together; `None` when the project that
    /// the keyframe comes from does not say (older projects do not).
    pub uniform: Option<bool>,
}

impl Keyframe {
    /// A linear keyframe of plain numbers.
    pub fn new(time: f32, value: [f32; 3]) -> Self {
        Self {
            time,
            value: value.map(Value::Num),
            post: None,
            interp: Interp::Linear,
            bezier: None,
            uuid: None,
            color: -1,
            uniform: None,
        }
    }

    /// The values the channel leaves the keyframe with.
    pub fn leaving(&self) -> &[Value; 3] {
        self.post.as_ref().unwrap_or(&self.value)
    }

    fn is_sane(&self) -> bool {
        self.time.is_finite()
            && (0.0..=MAX_LENGTH).contains(&self.time)
            && self.value.iter().all(Value::is_sane)
            && self.post.iter().flatten().all(Value::is_sane)
            && self.bezier.as_ref().is_none_or(Handles::is_sane)
    }
}

/// The keyframes of one animated group.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneTrack {
    /// The animated group.
    pub group: GroupId,
    /// Rotation keyframes, ordered by time.
    pub rotation: Vec<Keyframe>,
    /// Position keyframes, ordered by time.
    pub position: Vec<Keyframe>,
    /// Scale keyframes, ordered by time.
    pub scale: Vec<Keyframe>,
    /// The members of the animator in the project that the model does not use, as JSON text.
    pub extra: Option<Arc<str>>,
    /// The order in which a project lists the channels of the animator. Editors differ in it; a
    /// writer keeps the order the project had. See [`BoneTrack::listed`].
    pub order: [Channel; 3],
}

impl BoneTrack {
    /// A track without keyframes.
    pub fn new(group: GroupId) -> Self {
        Self {
            group,
            rotation: Vec::new(),
            position: Vec::new(),
            scale: Vec::new(),
            extra: None,
            order: Channel::ALL,
        }
    }

    /// The channels in the order a project lists them: [`BoneTrack::order`] when it names each
    /// channel once, the usual order otherwise.
    pub fn listed(&self) -> [Channel; 3] {
        let complete = Channel::ALL.iter().all(|c| self.order.contains(c));
        if complete { self.order } else { Channel::ALL }
    }

    /// The keyframes of a channel, ordered by time.
    pub fn channel(&self, channel: Channel) -> &[Keyframe] {
        match channel {
            Channel::Rotation => &self.rotation,
            Channel::Position => &self.position,
            Channel::Scale => &self.scale,
        }
    }

    /// The keyframes of a channel, mutable. Keep them ordered by time.
    pub fn channel_mut(&mut self, channel: Channel) -> &mut Vec<Keyframe> {
        match channel {
            Channel::Rotation => &mut self.rotation,
            Channel::Position => &mut self.position,
            Channel::Scale => &mut self.scale,
        }
    }

    /// Number of keyframes of all channels.
    pub fn keyframe_count(&self) -> usize {
        self.rotation.len() + self.position.len() + self.scale.len()
    }

    /// The track has no keyframes.
    pub fn is_empty(&self) -> bool {
        self.keyframe_count() == 0
    }
}

/// A named set of tracks with a length and a loop mode.
///
/// spec: animation A2 2
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    /// Display name.
    pub name: Label,
    /// Seconds; between 0 and [`MAX_LENGTH`].
    pub length: f32,
    /// What happens after the end.
    pub loop_mode: LoopMode,
    /// The frame rate of the snapping grid of the timeline, within [`SNAPPING_RANGE`].
    pub snapping: u32,
    /// The `override` flag of a project.
    pub overrides: bool,
    /// The identity a `.bbmodel` project gave the animation.
    pub uuid: Option<String>,
    /// One track per animated group, in file order.
    pub tracks: Vec<BoneTrack>,
    /// The members of the animation in the project that the model does not use, such as
    /// `anim_time_update` and `blend_weight`, and its animators that are not bones of the model
    /// (`effects` with sounds and particles, unknown uuids), as the text of a JSON object.
    pub extra: Option<Arc<str>>,
}

impl Animation {
    /// An animation without tracks that plays once and snaps to 24 frames per second.
    pub fn new(name: impl Into<Label>, length: f32) -> Self {
        Self {
            name: name.into(),
            length,
            loop_mode: LoopMode::Once,
            snapping: DEFAULT_SNAPPING,
            overrides: false,
            uuid: None,
            tracks: Vec::new(),
            extra: None,
        }
    }

    /// A time rounded to the nearest frame of the snapping grid, within the animation.
    pub fn on_frame(&self, time: f32) -> f32 {
        let rate = self.snapping as f32;
        ((time * rate).round() / rate).clamp(0.0, self.length)
    }

    /// The track of a group.
    pub fn track(&self, group: GroupId) -> Option<&BoneTrack> {
        self.tracks.iter().find(|t| t.group == group)
    }

    /// The track of a group, made when there is none.
    pub fn track_mut(&mut self, group: GroupId) -> &mut BoneTrack {
        let at = match self.tracks.iter().position(|t| t.group == group) {
            Some(at) => at,
            None => {
                self.tracks.push(BoneTrack::new(group));
                self.tracks.len() - 1
            }
        };
        &mut self.tracks[at]
    }

    /// Number of keyframes in all tracks.
    pub fn keyframe_count(&self) -> usize {
        self.tracks.iter().map(BoneTrack::keyframe_count).sum()
    }

    /// Bytes of JSON text the animation keeps for a file.
    pub fn extra_bytes(&self) -> usize {
        self.extra.as_ref().map_or(0, |e| e.len())
            + self
                .tracks
                .iter()
                .filter_map(|t| t.extra.as_ref())
                .map(|e| e.len())
                .sum::<usize>()
    }

    /// An estimate of the memory the animation owns, for the undo budget.
    pub fn weight(&self) -> usize {
        size_of::<Animation>()
            + self.tracks.len() * size_of::<BoneTrack>()
            + self.keyframe_count() * size_of::<Keyframe>()
            + self.extra_bytes()
    }

    /// Checks the length, the frame rate, the order and the numbers of the keyframes and that no
    /// group has two tracks. Whether the groups exist is a question for the document.
    pub fn check(&self) -> Result<(), String> {
        if !(self.length.is_finite() && (0.0..=MAX_LENGTH).contains(&self.length)) {
            return Err(format!(
                "the animation `{}` has an invalid length",
                self.name
            ));
        }
        if !(SNAPPING_RANGE.0..=SNAPPING_RANGE.1).contains(&self.snapping) {
            return Err(format!(
                "the animation `{}` has an invalid frame rate",
                self.name
            ));
        }
        let mut groups = FxHashSet::default();
        for track in &self.tracks {
            if !groups.insert(track.group) {
                return Err(format!(
                    "the animation `{}` has two tracks for one group",
                    self.name
                ));
            }
            for channel in Channel::ALL {
                let keys = track.channel(channel);
                if !keys.iter().all(Keyframe::is_sane) {
                    return Err(format!(
                        "the animation `{}` has a keyframe with an invalid time or value",
                        self.name
                    ));
                }
                if keys.windows(2).any(|pair| pair[0].time >= pair[1].time) {
                    return Err(format!(
                        "the keyframes of the animation `{}` are not in order",
                        self.name
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The animations of a document, in list order.
///
/// spec: animation A2 2
#[derive(Clone, Debug, Default)]
pub struct Animations {
    list: ObjList<Animation>,
}

impl Animations {
    /// Number of animations.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether the document has no animation.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The animation ids in list order.
    pub fn ids(&self) -> &[AnimationId] {
        self.list.ids()
    }

    /// The animation records in list order, parallel to [`Animations::ids`].
    pub fn items(&self) -> &[Arc<Animation>] {
        self.list.items()
    }

    /// The animation with `id`.
    pub fn get(&self, id: AnimationId) -> Option<&Animation> {
        self.list.get(id)
    }

    /// The position of `id` in list order.
    pub fn position(&self, id: AnimationId) -> Option<usize> {
        self.list.position(id)
    }

    /// Changes whenever an animation is added, removed or edited.
    pub fn rev(&self) -> Rev {
        self.list.rev()
    }

    /// Changes whenever an animation is added or removed.
    pub fn structure_rev(&self) -> Rev {
        self.list.structure_rev()
    }

    /// Number of keyframes in all animations.
    pub fn keyframe_count(&self) -> usize {
        self.list.items().iter().map(|a| a.keyframe_count()).sum()
    }

    /// Mutable access to an animation. The caller marks the document, see
    /// [`Document::edit_animation`].
    pub(crate) fn edit(&mut self, id: AnimationId) -> Option<&mut Animation> {
        let mut scratch = ChangeJournal::default();
        self.list
            .edit_marked(id, crate::journal::ChangeFlags::empty(), &mut scratch)
    }

    /// Appends an animation, refused beyond [`MAX_ANIMATIONS`].
    pub(crate) fn push(&mut self, animation: Animation) -> Result<AnimationId, AnimationError> {
        if self.list.len() >= MAX_ANIMATIONS {
            return Err(AnimationError::TooMany);
        }
        Ok(self.list.push(animation))
    }

    /// Inserts an animation at a position of the list, refused beyond [`MAX_ANIMATIONS`].
    pub(crate) fn insert(
        &mut self,
        at: usize,
        animation: Animation,
    ) -> Result<AnimationId, AnimationError> {
        if self.list.len() >= MAX_ANIMATIONS {
            return Err(AnimationError::TooMany);
        }
        Ok(self.list.insert(at, animation))
    }

    /// Drops the animations with these ids.
    pub(crate) fn remove(&mut self, ids: &FxHashSet<AnimationId>) {
        self.list.remove_set(ids);
    }

    /// The captured list.
    pub fn snapshot(&self) -> ListSnap<Animation> {
        self.list.snapshot()
    }

    /// The animations of a snapshot, keeping their ids.
    pub fn from_snap(snap: &ListSnap<Animation>) -> Animations {
        let mut animations = Animations::default();
        animations.restore(snap);
        animations
    }

    /// Replaces the animations by a snapshot, keeping its ids.
    pub fn restore(&mut self, snap: &ListSnap<Animation>) {
        self.list.restore(snap);
    }

    /// Takes the tracks of the groups in `gone` out of every animation. Returns whether anything
    /// changed.
    pub(crate) fn drop_groups(&mut self, gone: &FxHashSet<GroupId>) -> bool {
        self.retain_tracks(|group| !gone.contains(&group))
    }

    /// Takes the tracks of groups that no longer exist out of every animation. Returns whether
    /// anything changed.
    pub(crate) fn retain_live(&mut self, groups: &Groups) -> bool {
        self.retain_tracks(|group| groups.get(group).is_some())
    }

    fn retain_tracks(&mut self, keep: impl Fn(GroupId) -> bool) -> bool {
        let stale: Vec<AnimationId> = self
            .list
            .ids()
            .iter()
            .zip(self.list.items())
            .filter(|(_, a)| a.tracks.iter().any(|t| !keep(t.group)))
            .map(|(&id, _)| id)
            .collect();
        for &id in &stale {
            if let Some(animation) = self.edit(id) {
                animation.tracks.retain(|t| keep(t.group));
            }
        }
        !stale.is_empty()
    }

    /// Checks the bounds and the order of the data. `groups` says which groups exist.
    pub fn validate(&self, groups: &Groups) -> Result<(), String> {
        if self.list.len() > MAX_ANIMATIONS {
            return Err(format!(
                "the model has more than {MAX_ANIMATIONS} animations"
            ));
        }
        let mut keys = 0_usize;
        let mut extra = 0_usize;
        for animation in self.list.items() {
            animation.check()?;
            if animation
                .tracks
                .iter()
                .any(|t| groups.get(t.group).is_none())
            {
                return Err(format!(
                    "the animation `{}` animates a group that does not exist",
                    animation.name
                ));
            }
            keys += animation.keyframe_count();
            extra += animation.extra_bytes();
        }
        if keys > MAX_KEYFRAMES {
            return Err(format!("the model has more than {MAX_KEYFRAMES} keyframes"));
        }
        if extra > MAX_EXTRA_BYTES {
            return Err("the animations keep too much text of their files".to_owned());
        }
        Ok(())
    }
}

/// The animations of a snapshot, checked for the apply path: ids and records match and no id
/// repeats.
pub(crate) fn snap_animations(snap: &ListSnap<Animation>) -> Result<Animations, String> {
    if snap.ids.len() != snap.items.len() {
        return Err(format!(
            "the animation section has {} ids and {} records",
            snap.ids.len(),
            snap.items.len()
        ));
    }
    let mut seen: FxHashMap<AnimationId, ()> = FxHashMap::default();
    if !snap.ids.iter().all(|id| seen.insert(*id, ()).is_none()) {
        return Err("the animation section repeats an animation id".to_owned());
    }
    if snap.ids.len() > MAX_ANIMATIONS {
        return Err(format!(
            "the model has more than {MAX_ANIMATIONS} animations"
        ));
    }
    Ok(Animations::from_snap(snap))
}

impl Document {
    /// Edits an animation and marks the document: the journal, the revision and the list stamp
    /// change only when the animation exists.
    pub fn edit_animation<R>(
        &mut self,
        id: AnimationId,
        edit: impl FnOnce(&mut Animation) -> R,
    ) -> Option<R> {
        let animation = self.animations.edit(id)?;
        let out = edit(animation);
        self.touched_animations();
        Some(out)
    }

    /// Marks a change of the animations.
    pub fn touched_animations(&mut self) {
        self.journal.mark_doc(DocFlags::ANIMATIONS);
        self.revision = Rev::next();
    }

    /// Adds an animation and returns its id.
    pub fn add_animation(&mut self, animation: Animation) -> Result<AnimationId, AnimationError> {
        if !(animation.length.is_finite() && (0.0..=MAX_LENGTH).contains(&animation.length)) {
            return Err(AnimationError::OutOfRange);
        }
        let id = self.animations.push(animation)?;
        self.touched_animations();
        Ok(id)
    }

    /// Adds an animation at a position of the list and returns its id.
    pub fn insert_animation(
        &mut self,
        at: usize,
        animation: Animation,
    ) -> Result<AnimationId, AnimationError> {
        if !(animation.length.is_finite() && (0.0..=MAX_LENGTH).contains(&animation.length)) {
            return Err(AnimationError::OutOfRange);
        }
        let id = self.animations.insert(at, animation)?;
        self.touched_animations();
        Ok(id)
    }

    /// Removes the animations with these ids.
    pub fn remove_animations(&mut self, ids: &FxHashSet<AnimationId>) {
        if ids.is_empty() {
            return;
        }
        self.animations.remove(ids);
        self.touched_animations();
    }

    /// Takes the tracks of removed groups out of the animations.
    pub(crate) fn drop_animated_groups(&mut self, gone: &FxHashSet<GroupId>) {
        if self.animations.drop_groups(gone) {
            self.touched_animations();
        }
    }

    /// Takes the tracks of groups that no longer exist out of the animations, after an open or a
    /// restore changed the groups.
    pub fn prune_animations(&mut self) {
        if self.animations.retain_live(&self.groups) {
            self.touched_animations();
        }
    }

    /// Checks the animations against the groups of the document.
    pub fn check_animations(&self) -> Result<(), String> {
        self.animations.validate(&self.groups)
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::groups::{Group, GroupsBuilder};
    use crate::setters::fixture::World;

    fn world_with_group() -> (World, GroupId) {
        let mut w = World::new();
        let mut b = GroupsBuilder::new();
        let g = b.add_group(Group::new("bone", Vec3::ZERO), None).unwrap();
        w.doc.groups = b.finish();
        (w, g)
    }

    fn walk(group: GroupId) -> Animation {
        let mut a = Animation::new("walk", 1.0);
        let track = a.track_mut(group);
        track.rotation = vec![
            Keyframe::new(0.0, [0.0; 3]),
            Keyframe::new(1.0, [90.0, 0.0, 0.0]),
        ];
        a
    }

    #[test]
    fn a_value_keeps_its_text_and_knows_whether_it_is_a_number() {
        assert_eq!(Value::from_text("1.5").number(), Some(1.5));
        assert_eq!(Value::from_text(" -2 ").number(), Some(-2.0));
        assert!(Value::from_text("math.sin(query.anim_time * 90)").is_expression());
        assert!(Value::from_text("inf").is_expression());
        assert!(Value::from_text("").is_expression());
        assert_eq!(Value::Num(3.0).number(), Some(3.0));
        match Value::from_text("0.50") {
            Value::Text { text, .. } => assert_eq!(&*text, "0.50"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn channels_have_their_file_names_and_rest_values() {
        for c in Channel::ALL {
            assert_eq!(Channel::from_key(c.key()), Some(c));
        }
        assert_eq!(Channel::from_key("sound"), None);
        assert_eq!(Channel::Scale.rest(), 1.0);
        assert_eq!(Channel::Position.rest(), 0.0);
    }

    #[test]
    fn animations_are_added_edited_and_removed_with_stable_ids() {
        let (mut w, g) = world_with_group();
        let a = w.doc.add_animation(walk(g)).unwrap();
        let b = w.doc.add_animation(Animation::new("idle", 2.0)).unwrap();
        assert_eq!(w.doc.animations.ids(), [a, b]);
        assert!(w.doc.check_animations().is_ok());

        let rev = w.doc.revision;
        w.doc
            .edit_animation(b, |anim| anim.loop_mode = LoopMode::Loop)
            .unwrap();
        assert_ne!(w.doc.revision, rev);
        assert_eq!(w.doc.animations.get(b).unwrap().loop_mode, LoopMode::Loop);
        assert!(w.doc.edit_animation(ObjId::next(), |_| ()).is_none());

        w.doc.remove_animations(&FxHashSet::from_iter([a]));
        assert_eq!(w.doc.animations.ids(), [b]);
        let c = w.doc.add_animation(Animation::new("run", 1.0)).unwrap();
        assert_ne!(c, a, "an id is not issued twice");
        assert_eq!(
            w.doc
                .add_animation(Animation::new("long", MAX_LENGTH + 1.0)),
            Err(AnimationError::OutOfRange)
        );
    }

    #[test]
    fn the_count_of_animations_is_bounded() {
        let mut animations = Animations::default();
        for _ in 0..MAX_ANIMATIONS {
            animations.push(Animation::new("a", 1.0)).unwrap();
        }
        assert_eq!(
            animations.push(Animation::new("one more", 1.0)),
            Err(AnimationError::TooMany)
        );
        assert!(animations.validate(&Groups::default()).is_ok());
    }

    #[test]
    fn validate_catches_bad_times_orders_values_and_missing_groups() {
        let (mut w, g) = world_with_group();
        let id = w.doc.add_animation(walk(g)).unwrap();
        let bad = |w: &mut World, edit: &dyn Fn(&mut Animation)| {
            let id = w.doc.animations.ids()[0];
            w.doc.edit_animation(id, edit).unwrap();
            w.doc.check_animations().is_err()
        };
        assert!(bad(&mut w, &|a| a.length = f32::NAN));
        assert!(bad(&mut w, &|a| a.length = MAX_LENGTH + 1.0));
        w.doc.edit_animation(id, |a| a.length = 1.0).unwrap();
        assert!(bad(&mut w, &|a| a.snapping = 5));
        w.doc.edit_animation(id, |a| a.snapping = 24).unwrap();
        assert!(bad(&mut w, &|a| a.tracks[0].rotation.reverse()));
        w.doc
            .edit_animation(id, |a| a.tracks[0].rotation.reverse())
            .unwrap();
        assert!(bad(&mut w, &|a| a.tracks[0].rotation[1].time = 0.0));
        w.doc
            .edit_animation(id, |a| a.tracks[0].rotation[1].time = 1.0)
            .unwrap();
        assert!(bad(&mut w, &|a| a.tracks[0].rotation[0].value[1] =
            Value::Num(f32::INFINITY)));
        w.doc
            .edit_animation(id, |a| a.tracks[0].rotation[0].value[1] = Value::Num(0.0))
            .unwrap();
        assert!(bad(&mut w, &|a| a.tracks[0].position =
            vec![Keyframe::new(-1.0, [0.0; 3])]));
        w.doc
            .edit_animation(id, |a| a.tracks[0].position.clear())
            .unwrap();
        assert!(bad(&mut w, &|a| a.tracks.push(BoneTrack::new(g))));
        w.doc.edit_animation(id, |a| a.tracks.truncate(1)).unwrap();
        assert!(bad(&mut w, &|a| a
            .tracks
            .push(BoneTrack::new(ObjId::next()))));
    }

    #[test]
    fn the_keyframes_and_the_kept_text_are_bounded_over_all_animations() {
        let (w, g) = world_with_group();
        let mut animations = Animations::default();
        let mut many = Animation::new("many", 10.0);
        many.track_mut(g).rotation = (0..MAX_KEYFRAMES)
            .map(|i| Keyframe::new(i as f32 / 100_000.0, [0.0; 3]))
            .collect();
        animations.push(many).unwrap();
        assert!(animations.validate(&w.doc.groups).is_ok());
        let mut one = Animation::new("one", 1.0);
        one.track_mut(g).scale = vec![Keyframe::new(0.0, [1.0; 3])];
        animations.push(one).unwrap();
        assert!(animations.validate(&w.doc.groups).is_err());

        let mut text = Animations::default();
        let mut big = Animation::new("big", 1.0);
        big.extra = Some(Arc::from("x".repeat(MAX_EXTRA_BYTES + 1)));
        text.push(big).unwrap();
        assert!(text.validate(&w.doc.groups).is_err());
    }

    #[test]
    fn removing_a_group_takes_its_tracks_along() {
        let (mut w, g) = world_with_group();
        let id = w.doc.add_animation(walk(g)).unwrap();
        w.doc.remove_groups(&FxHashSet::from_iter([g]));
        assert!(w.doc.animations.get(id).unwrap().tracks.is_empty());
        assert!(w.doc.check_animations().is_ok());

        // A restore that brings back an older group list prunes what is left over.
        let (mut w, g) = world_with_group();
        let id = w.doc.add_animation(walk(g)).unwrap();
        w.doc.groups = Groups::default();
        assert!(w.doc.check_animations().is_err());
        w.doc.prune_animations();
        assert!(w.doc.animations.get(id).unwrap().tracks.is_empty());
        assert!(w.doc.check_animations().is_ok());
    }

    #[test]
    fn snapshots_keep_ids_and_reject_repeats() {
        let (mut w, g) = world_with_group();
        let a = w.doc.add_animation(walk(g)).unwrap();
        let snap = w.doc.animations.snapshot();
        w.doc.remove_animations(&FxHashSet::from_iter([a]));
        w.doc.animations.restore(&snap);
        assert_eq!(w.doc.animations.ids(), [a]);
        assert!(snap_animations(&snap).is_ok());

        let mut twice = snap.clone();
        twice.ids.push(a);
        twice.items.push(Arc::clone(&snap.items[0]));
        assert!(snap_animations(&twice).is_err());
        let mut short = snap;
        short.ids.pop();
        assert!(snap_animations(&short).is_err());
    }
}
