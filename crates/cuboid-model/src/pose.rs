//! Evaluation of animations: the value of a channel at a time, and the pose of the groups at a
//! time (spec: animation A2 3).
//!
//! The conventions are those of the `.bbmodel` project format, which the importer already uses
//! for the rest rotation of a group:
//!
//! - Position is in model units and moves the bone inside the frame of its parent group.
//! - Rotation is in degrees and is added, component by component, to the Euler angles of the
//!   bone; the angles turn about Z, then Y, then X ([`crate::mc::rotation_m`]). The sum is not a
//!   composition of rotations: a bone that rests at 30 degrees about Y and plays 60 degrees about
//!   Y stands at 90.
//! - Scale is a factor that applies to the bone in its own, turned axes about its pivot, so a
//!   scaled bone scales everything below it.
//! - A channel without keyframes keeps its rest value: 0, or 1 for the scale.
//!
//! The model space is the space of a project mirrored on Z. The values of a track are read in the
//! space of the project and the result is mirrored back, as the importer does.
//!
//! [`Rig`] holds the rest pose of the groups, [`Pose`] what an animation does to them at one time:
//! per group the motion that carries a point of the rest pose to its place in the pose, in model
//! space, composed down the hierarchy.

use std::sync::Arc;

use cuboid_num::euler_to_quat;
use glam::{Affine3A, Mat3, Vec3};
use rustc_hash::FxHashMap;

use crate::animation::{Animation, BoneTrack, Channel, Interp, Keyframe, LoopMode};
use crate::contract::Document;
use crate::groups::{GroupId, Groups};
use crate::ids::{ObjRef, Rev};
use crate::mc::{angles_m, flip_z, rotation_m};

/// A time this close to a keyframe is the keyframe: playing at 1/1200 of a second.
const KEY_EPSILON: f32 = 1.0 / 1200.0;

/// Steps of the search for the point of a Bezier segment at a time. The error after 24 halvings
/// of a unit interval is far below what a value shows.
const BEZIER_STEPS: u32 = 24;

/// A scale of exactly 0 would make the bone impossible to invert.
const MIN_SCALE: f32 = 0.000_01;

/// The value of a keyframe on one axis, on the side the channel arrives (`leaving` false) or leaves
/// with. An expression counts as the last number before it, in time order, and as 0 when there is
/// none.
fn value_at(keys: &[Keyframe], i: usize, axis: usize, leaving: bool) -> f32 {
    let Some(key) = keys.get(i) else {
        return 0.0;
    };
    if leaving && let Some(n) = key.post.as_ref().and_then(|post| post[axis].number()) {
        return n;
    }
    if let Some(n) = key.value[axis].number() {
        return n;
    }
    keys[..i]
        .iter()
        .rev()
        .find_map(|earlier| {
            earlier
                .post
                .as_ref()
                .and_then(|post| post[axis].number())
                .or_else(|| earlier.value[axis].number())
        })
        .unwrap_or(0.0)
}

fn arriving(keys: &[Keyframe], i: usize, axis: usize) -> f32 {
    value_at(keys, i, axis, false)
}

fn leaving(keys: &[Keyframe], i: usize, axis: usize) -> f32 {
    value_at(keys, i, axis, true)
}

/// The uniform Catmull-Rom curve through `p1` and `p2` with the neighbours `p0` and `p3`, at the
/// share `s` of the segment.
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, s: f32) -> f32 {
    let v0 = (p2 - p0) * 0.5;
    let v1 = (p3 - p1) * 0.5;
    ((2.0 * p1 - 2.0 * p2 + v0 + v1) * s + (-3.0 * p1 + 3.0 * p2 - 2.0 * v0 - v1)) * s * s
        + v0 * s
        + p1
}

/// A cubic Bezier curve of one coordinate at the parameter `s`.
fn bezier(p0: f32, p1: f32, p2: f32, p3: f32, s: f32) -> f32 {
    let u = 1.0 - s;
    u * u * u * p0 + 3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s * p3
}

/// The value of the Bezier segment from `before` to `after` at `t`: the curve through the two
/// values with the handles of the keyframes, solved for the time.
fn bezier_segment(
    before: &Keyframe,
    after: &Keyframe,
    (v0, v1): (f32, f32),
    t: f32,
    axis: usize,
) -> f32 {
    let default = crate::animation::Handles::default();
    let right = before.bezier.as_ref().unwrap_or(&default);
    let left = after.bezier.as_ref().unwrap_or(&default);
    let gap = after.time - before.time;
    let x1 = right.right_time[axis].clamp(0.0, gap);
    let x2 = gap + left.left_time[axis].clamp(-gap, 0.0);
    let target = t - before.time;
    // The handles stay inside the segment, so x grows with the parameter.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..BEZIER_STEPS {
        let mid = 0.5 * (lo + hi);
        if bezier(0.0, x1, x2, gap, mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let s = 0.5 * (lo + hi);
    bezier(
        v0,
        v0 + right.right_value[axis],
        v1 + left.left_value[axis],
        v1,
        s,
    )
}

/// The value of one axis of a channel at `t`; `keys` is not empty and ordered by time.
fn sample_axis(keys: &[Keyframe], t: f32, axis: usize) -> f32 {
    let after_at = keys.partition_point(|k| k.time < t);
    let before = after_at.checked_sub(1);
    let after = (after_at < keys.len()).then_some(after_at);
    // Close to a keyframe the value is the keyframe's own, the one it arrives with.
    if let Some(b) = before.filter(|&b| t - keys[b].time < KEY_EPSILON) {
        return arriving(keys, b, axis);
    }
    if let Some(a) = after.filter(|&a| keys[a].time - t < KEY_EPSILON) {
        return arriving(keys, a, axis);
    }
    let (b, a) = match (before, after) {
        (Some(b), Some(a)) => (b, a),
        (Some(b), None) => return leaving(keys, b, axis),
        (None, Some(a)) => return arriving(keys, a, axis),
        (None, None) => return 0.0,
    };
    let (kb, ka) = (&keys[b], &keys[a]);
    if kb.interp == Interp::Step {
        return leaving(keys, b, axis);
    }
    let (v0, v1) = (leaving(keys, b, axis), arriving(keys, a, axis));
    let alpha = (t - kb.time) / (ka.time - kb.time);
    if kb.interp == Interp::Linear && matches!(ka.interp, Interp::Linear | Interp::Step) {
        return v0 + (v1 - v0) * alpha;
    }
    if kb.interp == Interp::CatmullRom || ka.interp == Interp::CatmullRom {
        // The neighbours of the segment, the end of the channel repeated; a jump on the segment's
        // side of a neighbour cuts the curve there.
        let p0 = match b.checked_sub(1) {
            Some(n) if kb.post.is_none() => leaving(keys, n, axis),
            _ => v0,
        };
        let p3 = if a + 1 < keys.len() && ka.post.is_none() {
            arriving(keys, a + 1, axis)
        } else {
            v1
        };
        return catmull_rom(p0, v0, v1, p3, alpha);
    }
    bezier_segment(kb, ka, (v0, v1), t, axis)
}

/// The value of a channel of a track at `t` seconds of the animation: on X, Y and Z. `None` when
/// the channel has no keyframes. `t` is a time inside the animation, see [`animation_time`].
///
/// Linear keyframes interpolate straight; a segment with a Catmull-Rom keyframe at one end follows
/// the curve through the neighbouring keyframes; otherwise a segment with a Bezier keyframe follows
/// the curve of the handles; a keyframe that steps keeps its value. A keyframe with a `post` value
/// jumps: before it the channel arrives at `value`, after it leaves from `post`. A value that is an
/// expression counts as the last number before it.
pub fn sample(track: &BoneTrack, channel: Channel, t: f32) -> Option<Vec3> {
    let keys = track.channel(channel);
    if keys.is_empty() {
        return None;
    }
    Some(Vec3::from_array(std::array::from_fn(|axis| {
        sample_axis(keys, t, axis)
    })))
}

/// The time of the animation at the time `t` of a player, or `None` while the model is at rest.
///
/// A negative or invalid `t` is 0. `Loop` wraps `t` into the length, `Hold` stops at the length
/// and `Once` leaves the animation after the length: the model returns to its rest pose.
pub fn animation_time(animation: &Animation, t: f32) -> Option<f32> {
    let t = if t.is_finite() { t.max(0.0) } else { 0.0 };
    let length = animation.length;
    match animation.loop_mode {
        LoopMode::Loop if length > 0.0 => Some(t % length),
        LoopMode::Loop => Some(0.0),
        LoopMode::Hold => Some(t.min(length)),
        LoopMode::Once => (t <= length).then_some(t),
    }
}

/// What the three channels of a bone say at one time, in the space of a project.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BoneValues {
    /// Model units added to the position.
    pub position: Vec3,
    /// Degrees added to the Euler angles.
    pub rotation: Vec3,
    /// Factors of the size about the pivot.
    pub scale: Vec3,
}

/// The values of the channels of a track at `t`; a channel without keyframes keeps its rest value.
pub fn bone_values(track: &BoneTrack, t: f32) -> BoneValues {
    BoneValues {
        position: sample(track, Channel::Position, t).unwrap_or(Vec3::ZERO),
        rotation: sample(track, Channel::Rotation, t).unwrap_or(Vec3::ZERO),
        scale: sample(track, Channel::Scale, t).unwrap_or(Vec3::ONE),
    }
}

/// The Z mirror of a point or an offset.
fn mirror(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, -v.z)
}

/// The rest pose of the groups of a document: the frames the animations move. Build it again when
/// [`Groups::rev`] differs from [`Rig::groups_rev`].
#[derive(Clone, Debug)]
pub struct Rig {
    groups_rev: Rev,
    ids: Vec<GroupId>,
    index: Arc<FxHashMap<GroupId, u32>>,
    /// Indices into `ids`, every group after its parent.
    order: Vec<u32>,
    parent: Vec<Option<u32>>,
    /// The pivot of each group in the frame of its parent.
    rel: Vec<Vec3>,
    /// The rotation of each group relative to its parent.
    rest_rot: Vec<Mat3>,
    /// The Euler angles of a project that stand for `rest_rot`.
    rest_euler: Vec<Vec3>,
    /// The frame of each group in the model.
    rest: Vec<Affine3A>,
    rest_inv: Vec<Affine3A>,
}

impl Rig {
    /// The rest pose of these groups.
    pub fn new(groups: &Groups) -> Rig {
        let ids: Vec<GroupId> = groups.ids().to_vec();
        let index: FxHashMap<GroupId, u32> = ids
            .iter()
            .enumerate()
            .map(|(at, &id)| (id, u32::try_from(at).unwrap_or(u32::MAX)))
            .collect();
        let parent: Vec<Option<u32>> = ids
            .iter()
            .map(|&id| {
                groups
                    .parent_of(ObjRef::Group(id))
                    .and_then(|p| index.get(&p).copied())
            })
            .collect();
        let n = ids.len();
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut roots: Vec<u32> = Vec::new();
        for (at, p) in parent.iter().enumerate() {
            let at = u32::try_from(at).unwrap_or(u32::MAX);
            match p.and_then(|p| children.get_mut(p as usize)) {
                Some(list) => list.push(at),
                None => roots.push(at),
            }
        }
        let mut order = Vec::with_capacity(n);
        let mut stack: Vec<u32> = roots.into_iter().rev().collect();
        while let Some(at) = stack.pop() {
            order.push(at);
            if let Some(list) = children.get(at as usize) {
                stack.extend(list.iter().rev().copied());
            }
        }

        let mut rig = Rig {
            groups_rev: groups.rev(),
            ids,
            index: Arc::new(index),
            order,
            parent,
            rel: vec![Vec3::ZERO; n],
            rest_rot: vec![Mat3::IDENTITY; n],
            rest_euler: vec![Vec3::ZERO; n],
            rest: vec![Affine3A::IDENTITY; n],
            rest_inv: vec![Affine3A::IDENTITY; n],
        };
        for k in 0..rig.order.len() {
            let at = rig.order[k] as usize;
            let Some(group) = groups.get(rig.ids[at]) else {
                continue;
            };
            let turn = if group.rotation == Vec3::ZERO {
                Mat3::IDENTITY
            } else {
                Mat3::from_quat(euler_to_quat(group.rotation))
            };
            let pivot = group.origin;
            let above = rig.parent[at].map(|p| rig.rest[p as usize]);
            let (rel, frame) = match above {
                None => (pivot, Affine3A::from_mat3_translation(turn, pivot)),
                Some(above) => {
                    let m = Mat3::from(above.matrix3);
                    let rel = m.transpose() * (pivot - Vec3::from(above.translation));
                    (rel, Affine3A::from_mat3_translation(m * turn, pivot))
                }
            };
            rig.rel[at] = rel;
            rig.rest_rot[at] = turn;
            rig.rest_euler[at] = if turn == Mat3::IDENTITY {
                Vec3::ZERO
            } else {
                angles_m(flip_z(turn))
            };
            rig.rest[at] = frame;
            rig.rest_inv[at] = frame.inverse();
        }
        rig
    }

    /// The [`Groups::rev`] the rig was built for.
    pub fn groups_rev(&self) -> Rev {
        self.groups_rev
    }

    /// Number of groups.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the rig has no group.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The groups in list order.
    pub fn ids(&self) -> &[GroupId] {
        &self.ids
    }

    /// The position of a group in [`Rig::ids`].
    pub fn index_of(&self, group: GroupId) -> Option<usize> {
        self.index.get(&group).map(|&at| at as usize)
    }

    /// The frame of a group at rest: its rotation and, as translation, its pivot.
    pub fn rest_frame(&self, group: GroupId) -> Option<Affine3A> {
        self.rest.get(self.index_of(group)?).copied()
    }

    /// The pose of `animation` at the time `t` of a player: into `out`, which keeps its buffers
    /// from call to call. A group moves when its track has keyframes or a group above it moves.
    pub fn pose(&self, animation: &Animation, t: f32, out: &mut Pose) {
        out.reset(self);
        let Some(time) = animation_time(animation, t) else {
            return;
        };
        for (k, track) in animation.tracks.iter().enumerate() {
            if let Some(at) = self.index_of(track.group) {
                out.track_of[at] = u32::try_from(k).unwrap_or(u32::MAX);
            }
        }
        for &g in &self.order {
            let at = g as usize;
            let moved_above = self.parent[at].is_some_and(|p| out.moved[p as usize]);
            let track = animation
                .tracks
                .get(out.track_of[at] as usize)
                .filter(|track| !track.is_empty());
            if track.is_none() && !moved_above {
                continue;
            }
            let values = track.map(|track| bone_values(track, time));
            let above = self.parent[at].map_or(Affine3A::IDENTITY, |p| out.world[p as usize]);
            let local = self.animated_local(at, values);
            let world = above * local;
            out.world[at] = world;
            out.delta[at] = world * self.rest_inv[at];
            out.moved[at] = true;
        }
    }

    /// The frame of group `at` inside its parent's animated frame: its pivot and rotation with the
    /// values of its channels applied.
    fn animated_local(&self, at: usize, values: Option<BoneValues>) -> Affine3A {
        let values = values.unwrap_or(BoneValues {
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        });
        let turn = if values.rotation == Vec3::ZERO {
            self.rest_rot[at]
        } else {
            flip_z(rotation_m(self.rest_euler[at] + values.rotation))
        };
        let scale = values.scale.map(|s| if s == 0.0 { MIN_SCALE } else { s });
        Affine3A::from_mat3_translation(
            turn * Mat3::from_diagonal(scale),
            self.rel[at] + mirror(values.position),
        )
    }
}

/// What an animation does to the groups of a [`Rig`] at one time.
#[derive(Clone, Debug, Default)]
pub struct Pose {
    index: Arc<FxHashMap<GroupId, u32>>,
    /// The motion of each group, in model space: it carries a point of the rest pose to its place
    /// in the pose. The identity for a group that does not move.
    delta: Vec<Affine3A>,
    moved: Vec<bool>,
    /// The animated frame of each group, the parent's composed with its own.
    world: Vec<Affine3A>,
    /// The track of each group in the animation being evaluated.
    track_of: Vec<u32>,
}

impl Pose {
    /// A pose that moves nothing.
    pub fn new() -> Pose {
        Pose::default()
    }

    /// Sizes the buffers for a rig and puts every group at rest.
    fn reset(&mut self, rig: &Rig) {
        if !Arc::ptr_eq(&self.index, &rig.index) {
            self.index = Arc::clone(&rig.index);
        }
        let n = rig.len();
        self.delta.clear();
        self.delta.resize(n, Affine3A::IDENTITY);
        self.moved.clear();
        self.moved.resize(n, false);
        self.world.clear();
        self.world.extend_from_slice(&rig.rest);
        self.track_of.clear();
        self.track_of.resize(n, u32::MAX);
    }

    /// Becomes a copy of `other`, reusing the buffers.
    pub fn copy_from(&mut self, other: &Pose) {
        if !Arc::ptr_eq(&self.index, &other.index) {
            self.index = Arc::clone(&other.index);
        }
        self.delta.clone_from(&other.delta);
        self.moved.clone_from(&other.moved);
    }

    /// Number of groups the pose has a place for.
    pub fn len(&self) -> usize {
        self.delta.len()
    }

    /// Whether the pose has no place for a group.
    pub fn is_empty(&self) -> bool {
        self.delta.is_empty()
    }

    /// Whether no group moves.
    pub fn is_rest(&self) -> bool {
        !self.moved.iter().any(|&m| m)
    }

    /// Puts every group at rest.
    pub fn clear(&mut self) {
        self.moved.fill(false);
    }

    /// Identifies the set of groups the pose has a place for: it changes when the pose is made for
    /// another [`Rig`], so a cache of positions in the pose knows when to start again.
    pub fn shape(&self) -> usize {
        Arc::as_ptr(&self.index).addr()
    }

    /// The position of a group in the pose.
    pub fn index_of(&self, group: GroupId) -> Option<usize> {
        self.index.get(&group).map(|&at| at as usize)
    }

    /// The motion of the group at a position of the rig, `None` when it does not move.
    pub fn delta_at(&self, at: usize) -> Option<&Affine3A> {
        if *self.moved.get(at)? {
            self.delta.get(at)
        } else {
            None
        }
    }

    /// The motion of a group, `None` when it does not move.
    pub fn delta_of(&self, group: GroupId) -> Option<&Affine3A> {
        self.delta_at(self.index_of(group)?)
    }
}

/// The pose of `animation` at the time `t` of a player, built from scratch. Playing keeps a [`Rig`]
/// and a [`Pose`] instead.
pub fn pose(doc: &Document, animation: &Animation, t: f32) -> Pose {
    let rig = Rig::new(&doc.groups);
    let mut out = Pose::new();
    rig.pose(animation, t, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Handles, Value};
    use crate::groups::{Group, GroupsBuilder};
    use crate::mc::{Rigid, place_group};
    use crate::setters::fixture::World;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.abs_diff_eq(b, 1e-4)
    }

    fn track(channel: Channel, keys: Vec<Keyframe>) -> BoneTrack {
        let mut t = BoneTrack::new(crate::ObjId::next());
        *t.channel_mut(channel) = keys;
        t
    }

    fn key(time: f32, x: f32) -> Keyframe {
        Keyframe::new(time, [x, 0.0, 0.0])
    }

    fn x(track: &BoneTrack, t: f32) -> f32 {
        sample(track, Channel::Rotation, t).unwrap().x
    }

    #[test]
    fn linear_keyframes_interpolate_on_every_axis_and_hold_outside() {
        let t = track(
            Channel::Rotation,
            vec![
                Keyframe::new(0.0, [0.0, 0.0, 0.0]),
                Keyframe::new(2.0, [10.0, 20.0, 30.0]),
            ],
        );
        assert!(close(
            sample(&t, Channel::Rotation, 0.5).unwrap(),
            Vec3::new(2.5, 5.0, 7.5)
        ));
        assert!(close(
            sample(&t, Channel::Rotation, 1.5).unwrap(),
            Vec3::new(7.5, 15.0, 22.5)
        ));
        // Before the first and after the last keyframe the channel keeps that keyframe's value.
        assert!(close(
            sample(&t, Channel::Rotation, 9.0).unwrap(),
            Vec3::new(10.0, 20.0, 30.0)
        ));
        assert!(sample(&t, Channel::Position, 0.5).is_none());
        let lone = track(Channel::Scale, vec![Keyframe::new(1.0, [2.0; 3])]);
        assert!(close(
            sample(&lone, Channel::Scale, 0.0).unwrap(),
            Vec3::splat(2.0)
        ));
    }

    #[test]
    fn a_step_keeps_the_value_until_the_next_keyframe() {
        let mut a = key(0.0, 3.0);
        a.interp = Interp::Step;
        let t = track(Channel::Rotation, vec![a, key(2.0, 9.0)]);
        assert_eq!(x(&t, 1.99), 3.0);
        assert_eq!(x(&t, 2.0), 9.0);
        // A linear keyframe that goes on to a step keyframe still interpolates towards it.
        let mut b = key(2.0, 9.0);
        b.interp = Interp::Step;
        let t = track(Channel::Rotation, vec![key(0.0, 3.0), b]);
        assert!((x(&t, 1.0) - 6.0).abs() < 1e-5);
    }

    #[test]
    fn catmull_rom_follows_the_neighbours_and_repeats_the_ends() {
        let keys: Vec<Keyframe> = [0.0_f32, 1.0, 4.0, 9.0]
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let mut k = key(i as f32, v);
                k.interp = Interp::CatmullRom;
                k
            })
            .collect();
        let t = track(Channel::Rotation, keys);
        // The keys lie on x^2 with an even spacing: the middle segment is that parabola.
        assert!((x(&t, 1.5) - 2.25).abs() < 1e-5, "{}", x(&t, 1.5));
        // The first segment repeats its start: p0 = p1 = 0, p2 = 1, p3 = 4.
        assert!((x(&t, 0.5) - 0.3125).abs() < 1e-5, "{}", x(&t, 0.5));
        // The last segment repeats its end: p0 = 1, p1 = 4, p2 = p3 = 9, at the share 0.5: the
        // tangents are 4 and 2.5, so y = -3.5 * 0.125 + 4.5 * 0.25 + 4 * 0.5 + 4.
        assert!((x(&t, 2.5) - 6.6875).abs() < 1e-4, "{}", x(&t, 2.5));
        // A linear keyframe next to a curved one makes the segment a curve too.
        let mut mixed: Vec<Keyframe> = (0..3).map(|i| key(i as f32, (i * i) as f32)).collect();
        mixed[1].interp = Interp::CatmullRom;
        let t = track(Channel::Rotation, mixed);
        assert!((x(&t, 0.5) - 0.3125).abs() < 1e-5);
        // The second segment: p0 = 0, p1 = 1, p2 = 4, p3 = 4 (repeated) at the share 0.5.
        assert!((x(&t, 1.5) - 2.5625).abs() < 1e-5, "{}", x(&t, 1.5));
    }

    #[test]
    fn a_bezier_segment_is_solved_for_the_time() {
        let mut a = key(0.0, 0.0);
        a.interp = Interp::Bezier;
        a.bezier = Some(Handles {
            right_time: [0.25; 3],
            ..Handles::default()
        });
        let mut b = key(1.0, 10.0);
        b.interp = Interp::Bezier;
        b.bezier = Some(Handles {
            left_time: [-0.25; 3],
            ..Handles::default()
        });
        let t = track(Channel::Rotation, vec![a, b]);
        // Control points (0,0), (0.25,0), (0.75,10), (1,10): symmetric, so the middle is 5.
        assert!((x(&t, 0.5) - 5.0).abs() < 1e-3, "{}", x(&t, 0.5));
        // At the parameter 0.25 the curve is at time 29/128 and value 1.5625.
        assert!(
            (x(&t, 0.226_562_5) - 1.562_5).abs() < 1e-3,
            "{}",
            x(&t, 0.226_562_5)
        );
        // The curve starts and ends flat, so it stays below a straight line early on.
        assert!(x(&t, 0.1) < 1.0);

        // Handles with a value offset bend the curve; the time offset is clamped to the gap.
        let mut a = key(0.0, 0.0);
        a.interp = Interp::Bezier;
        a.bezier = Some(Handles {
            right_time: [5.0; 3],
            right_value: [4.0; 3],
            ..Handles::default()
        });
        let mut b = key(1.0, 0.0);
        b.interp = Interp::Bezier;
        b.bezier = Some(Handles {
            left_time: [-5.0; 3],
            left_value: [4.0; 3],
            ..Handles::default()
        });
        // The times clamp to the gap, so the control points are (0,0), (1,4), (0,4), (1,0) and
        // x(s) = 3(1-s)^2 s + s^3 reaches the middle at s = 0.5, where y = 12 s (1-s) = 3.
        let t = track(Channel::Rotation, vec![a, b]);
        let mid = x(&t, 0.5);
        assert!((mid - 3.0).abs() < 0.01, "{mid}");
    }

    #[test]
    fn a_jump_arrives_with_the_value_and_leaves_with_the_post_value() {
        let mut jump = key(1.0, 5.0);
        jump.post = Some([Value::Num(8.0), Value::Num(0.0), Value::Num(0.0)]);
        let t = track(Channel::Rotation, vec![key(0.0, 0.0), jump, key(2.0, 10.0)]);
        assert!(
            (x(&t, 0.5) - 2.5).abs() < 1e-5,
            "towards the arriving value"
        );
        assert_eq!(x(&t, 1.0), 5.0, "at the jump the channel has arrived");
        assert!((x(&t, 1.5) - 9.0).abs() < 1e-5, "from the leaving value");
        assert!((x(&t, 1.01) - 8.02).abs() < 1e-3);
    }

    #[test]
    fn an_expression_counts_as_the_last_number_before_it() {
        let mut expr = key(1.0, 0.0);
        expr.value[0] = Value::from_text("math.sin(query.anim_time * 90)");
        let t = track(Channel::Rotation, vec![key(0.0, 4.0), expr, key(2.0, 10.0)]);
        assert_eq!(x(&t, 1.0), 4.0);
        assert!(
            (x(&t, 1.5) - 7.0).abs() < 1e-5,
            "from 4 (the expression) to 10"
        );
        assert!((x(&t, 0.5) - 4.0).abs() < 1e-5, "from 4 to 4");

        // Nothing before it: 0.
        let mut first = key(0.0, 0.0);
        first.value[0] = Value::from_text("variable.x");
        let t = track(Channel::Rotation, vec![first, key(1.0, 10.0)]);
        assert_eq!(x(&t, 0.0), 0.0);
        assert!((x(&t, 0.5) - 5.0).abs() < 1e-5);

        // A number written as text is a number.
        let mut text = key(0.0, 0.0);
        text.value[0] = Value::from_text("2.5");
        let t = track(Channel::Rotation, vec![text]);
        assert_eq!(x(&t, 0.0), 2.5);

        // An expression in the leaving value falls back on the value before it in the same key.
        let mut jump = key(1.0, 6.0);
        jump.post = Some([Value::from_text("q.x"), Value::Num(0.0), Value::Num(0.0)]);
        let t = track(Channel::Rotation, vec![jump, key(2.0, 10.0)]);
        assert!((x(&t, 1.5) - 8.0).abs() < 1e-5);
    }

    #[test]
    fn keys_are_found_in_a_long_channel() {
        let keys: Vec<Keyframe> = (0..10_000)
            .map(|i| key(i as f32 * 0.01, i as f32))
            .collect();
        let t = track(Channel::Rotation, keys);
        assert!((x(&t, 50.005) - 5000.5).abs() < 0.1);
        assert!(x(&t, 1.0e9).is_finite());
        assert_eq!(x(&t, -5.0), 0.0);
    }

    fn timed(mode: LoopMode) -> Animation {
        let mut a = Animation::new("a", 2.0);
        a.loop_mode = mode;
        a
    }

    #[test]
    fn the_loop_modes_map_the_player_time() {
        assert_eq!(animation_time(&timed(LoopMode::Loop), 2.5), Some(0.5));
        assert_eq!(animation_time(&timed(LoopMode::Loop), 4.0), Some(0.0));
        assert_eq!(animation_time(&timed(LoopMode::Hold), 2.5), Some(2.0));
        assert_eq!(animation_time(&timed(LoopMode::Hold), 1.0), Some(1.0));
        assert_eq!(animation_time(&timed(LoopMode::Once), 2.0), Some(2.0));
        assert_eq!(animation_time(&timed(LoopMode::Once), 2.5), None);
        // Bad times are the start.
        for mode in [LoopMode::Once, LoopMode::Loop, LoopMode::Hold] {
            assert_eq!(animation_time(&timed(mode), -1.0), Some(0.0));
            assert_eq!(animation_time(&timed(mode), f32::NAN), Some(0.0));
        }
        let mut empty = timed(LoopMode::Loop);
        empty.length = 0.0;
        assert_eq!(animation_time(&empty, 3.0), Some(0.0));
    }

    /// A model whose groups are `(pivot, parent)` in the model's space, with the animation `a`
    /// that gives the group at each position the channel values of `tracks`.
    fn rigged(
        groups: &[(Vec3, Option<usize>, Vec3)],
        tracks: &[(usize, Channel, Vec<Keyframe>)],
    ) -> (World, Animation, Vec<GroupId>) {
        let mut w = World::new();
        let mut b = GroupsBuilder::new();
        let mut ids: Vec<GroupId> = Vec::new();
        for &(pivot, parent, rotation) in groups {
            let mut g = Group::new("bone", pivot);
            g.rotation = rotation;
            ids.push(b.add_group(g, parent.map(|p| ids[p])).unwrap());
        }
        w.doc.groups = b.finish();
        let mut a = Animation::new("a", 1.0);
        for (at, channel, keys) in tracks {
            *a.track_mut(ids[*at]).channel_mut(*channel) = keys.clone();
        }
        (w, a, ids)
    }

    fn constant(value: [f32; 3]) -> Vec<Keyframe> {
        vec![Keyframe::new(0.0, value), Keyframe::new(1.0, value)]
    }

    fn moved(p: &Pose, id: GroupId, point: Vec3) -> Vec3 {
        p.delta_of(id).unwrap().transform_point3(point)
    }

    #[test]
    fn a_rotation_turns_the_bone_about_its_pivot() {
        // About Z the project and the model agree; the pivot is (0, 0, 5).
        let (w, anim, ids) = rigged(
            &[(Vec3::new(0.0, 0.0, 5.0), None, Vec3::ZERO)],
            &[(
                0,
                Channel::Rotation,
                vec![
                    Keyframe::new(0.0, [0.0; 3]),
                    Keyframe::new(1.0, [0.0, 0.0, 90.0]),
                ],
            )],
        );
        let p = pose(&w.doc, &anim, 0.5);
        let r = std::f32::consts::FRAC_1_SQRT_2;
        assert!(close(
            moved(&p, ids[0], Vec3::new(1.0, 0.0, 5.0)),
            Vec3::new(r, r, 5.0)
        ));
        let p = pose(&w.doc, &anim, 1.0);
        assert!(close(
            moved(&p, ids[0], Vec3::new(1.0, 0.0, 5.0)),
            Vec3::new(0.0, 1.0, 5.0)
        ));
        // The pivot stays.
        assert!(close(
            moved(&p, ids[0], Vec3::new(0.0, 0.0, 5.0)),
            Vec3::new(0.0, 0.0, 5.0)
        ));
    }

    #[test]
    fn rotations_about_x_and_y_follow_the_mirror_of_the_model() {
        let (w, anim, ids) = rigged(
            &[(Vec3::ZERO, None, Vec3::ZERO)],
            &[(0, Channel::Rotation, constant([90.0, 0.0, 0.0]))],
        );
        // In a project +90 about X sends +Y to +Z; the model's Z is the project's mirrored.
        let p = pose(&w.doc, &anim, 0.5);
        assert!(close(moved(&p, ids[0], Vec3::Y), Vec3::new(0.0, 0.0, -1.0)));
        let (w, anim, ids) = rigged(
            &[(Vec3::ZERO, None, Vec3::ZERO)],
            &[(0, Channel::Rotation, constant([0.0, 90.0, 0.0]))],
        );
        // +90 about Y sends the project's +X to its -Z, which is the model's +Z.
        let p = pose(&w.doc, &anim, 0.5);
        assert!(close(moved(&p, ids[0], Vec3::X), Vec3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn the_angles_are_added_to_the_angles_of_the_bone() {
        // The bone rests at 30 degrees about Y in the project; the animation adds 60.
        let rest = place_group(Vec3::ZERO, Vec3::new(0.0, 30.0, 0.0), Rigid::IDENTITY).rotation;
        let (w, anim, ids) = rigged(
            &[(Vec3::ZERO, None, rest)],
            &[(0, Channel::Rotation, constant([0.0, 60.0, 0.0]))],
        );
        let p = pose(&w.doc, &anim, 0.5);
        let (s, c) = 30.0_f32.to_radians().sin_cos();
        // A point of the rest pose: the project's (1, 0, 0) turned by 30 degrees, Z mirrored.
        let at_rest = Vec3::new(c, 0.0, s);
        // In the pose it is at 90 degrees: the project's (0, 0, -1), the model's (0, 0, 1).
        assert!(close(moved(&p, ids[0], at_rest), Vec3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn position_and_scale_move_the_bone_inside_its_frame() {
        let (w, anim, ids) = rigged(
            &[(Vec3::new(4.0, 0.0, 0.0), None, Vec3::ZERO)],
            &[
                (0, Channel::Position, constant([1.0, 2.0, 3.0])),
                (0, Channel::Scale, constant([2.0, 1.0, 1.0])),
            ],
        );
        let p = pose(&w.doc, &anim, 0.5);
        // Scaled about the pivot: (6,0,0) -> (8,0,0); then moved by the position with Z mirrored.
        assert!(close(
            moved(&p, ids[0], Vec3::new(6.0, 0.0, 0.0)),
            Vec3::new(9.0, 2.0, -3.0)
        ));
        assert!(close(
            moved(&p, ids[0], Vec3::new(4.0, 0.0, 0.0)),
            Vec3::new(5.0, 2.0, -3.0)
        ));
    }

    #[test]
    fn three_bones_compose_down_the_chain() {
        // Pivots at x = 0, 10 and 20; the first two turn 90 degrees about Z.
        let (w, anim, ids) = rigged(
            &[
                (Vec3::ZERO, None, Vec3::ZERO),
                (Vec3::new(10.0, 0.0, 0.0), Some(0), Vec3::ZERO),
                (Vec3::new(20.0, 0.0, 0.0), Some(1), Vec3::ZERO),
            ],
            &[
                (0, Channel::Rotation, constant([0.0, 0.0, 90.0])),
                (1, Channel::Rotation, constant([0.0, 0.0, 90.0])),
            ],
        );
        let p = pose(&w.doc, &anim, 0.5);
        let q = Vec3::new(25.0, 0.0, 0.0);
        // The second bone turns it about (10,0,0) to (10,15,0), the first about the origin to
        // (-15,10,0); the third has no track of its own and follows.
        assert!(close(moved(&p, ids[2], q), Vec3::new(-15.0, 10.0, 0.0)));
        assert!(close(moved(&p, ids[1], q), Vec3::new(-15.0, 10.0, 0.0)));
        // Only the first bone turns a point that is on its own side.
        assert!(close(moved(&p, ids[0], q), Vec3::new(0.0, 25.0, 0.0)));
    }

    #[test]
    fn a_child_of_an_animated_bone_moves_with_it_and_a_root_without_a_track_stays() {
        let (w, anim, ids) = rigged(
            &[
                (Vec3::ZERO, None, Vec3::ZERO),
                (Vec3::new(10.0, 0.0, 0.0), Some(0), Vec3::ZERO),
                (Vec3::new(30.0, 0.0, 0.0), None, Vec3::ZERO),
            ],
            &[(0, Channel::Position, constant([0.0, 5.0, 0.0]))],
        );
        let p = pose(&w.doc, &anim, 0.0);
        assert!(close(moved(&p, ids[1], Vec3::X), Vec3::new(1.0, 5.0, 0.0)));
        assert!(
            p.delta_of(ids[2]).is_none(),
            "an unrelated bone does not move"
        );
        assert!(!p.is_rest());
    }

    #[test]
    fn a_rotated_parent_turns_the_position_of_its_child() {
        // The parent rests turned 90 degrees about Z; the child's position offset follows.
        let rest = place_group(Vec3::ZERO, Vec3::new(0.0, 0.0, 90.0), Rigid::IDENTITY).rotation;
        let (w, anim, ids) = rigged(
            &[
                (Vec3::ZERO, None, rest),
                (Vec3::new(0.0, 10.0, 0.0), Some(0), Vec3::ZERO),
            ],
            &[(1, Channel::Position, constant([2.0, 0.0, 0.0]))],
        );
        let p = pose(&w.doc, &anim, 0.5);
        // The child's frame is turned by 90 degrees: its +X is the model's +Y.
        assert!(close(
            moved(&p, ids[1], Vec3::new(0.0, 10.0, 0.0)),
            Vec3::new(0.0, 12.0, 0.0)
        ));
    }

    #[test]
    fn once_returns_to_the_rest_pose_and_hold_keeps_the_last_pose() {
        let (w, mut anim, ids) = rigged(
            &[(Vec3::ZERO, None, Vec3::ZERO)],
            &[(
                0,
                Channel::Position,
                vec![
                    Keyframe::new(0.0, [0.0; 3]),
                    Keyframe::new(1.0, [4.0, 0.0, 0.0]),
                ],
            )],
        );
        let p = pose(&w.doc, &anim, 1.5);
        assert!(p.is_rest(), "after the end of a Once animation");
        let p = pose(&w.doc, &anim, 1.0);
        assert!(close(
            moved(&p, ids[0], Vec3::ZERO),
            Vec3::new(4.0, 0.0, 0.0)
        ));
        anim.loop_mode = LoopMode::Hold;
        let p = pose(&w.doc, &anim, 1.5);
        assert!(close(
            moved(&p, ids[0], Vec3::ZERO),
            Vec3::new(4.0, 0.0, 0.0)
        ));
        anim.loop_mode = LoopMode::Loop;
        let p = pose(&w.doc, &anim, 1.5);
        assert!(close(
            moved(&p, ids[0], Vec3::ZERO),
            Vec3::new(2.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn the_rig_survives_deep_chains_stale_tracks_and_reuse() {
        let mut b = GroupsBuilder::new();
        let mut parent = None;
        let mut ids = Vec::new();
        for level in 0..crate::MAX_DEPTH {
            let g = Group::new("g", Vec3::new(level as f32, 0.0, 0.0));
            parent = Some(b.add_group(g, parent).unwrap());
            ids.push(parent.unwrap());
        }
        let mut w = World::new();
        w.doc.groups = b.finish();
        let mut anim = Animation::new("a", 1.0);
        anim.track_mut(ids[0]).position = constant([1.0, 0.0, 0.0]);
        // A track of a group the rig does not know is ignored.
        anim.track_mut(crate::ObjId::next()).scale = constant([2.0; 3]);
        let rig = Rig::new(&w.doc.groups);
        assert_eq!(rig.len(), crate::MAX_DEPTH);
        let mut out = Pose::new();
        rig.pose(&anim, 0.5, &mut out);
        let deepest = *ids.last().unwrap();
        assert!(close(moved(&out, deepest, Vec3::ZERO), Vec3::X));
        // The same buffers serve the next evaluation, which can end at rest.
        rig.pose(&anim, 5.0, &mut out);
        assert!(out.is_rest());
        let mut copy = Pose::new();
        rig.pose(&anim, 0.5, &mut out);
        copy.copy_from(&out);
        assert!(close(moved(&copy, deepest, Vec3::ZERO), Vec3::X));
        assert_eq!(rig.groups_rev(), w.doc.groups.rev());
    }

    #[test]
    fn the_rest_frames_put_each_pivot_where_the_group_is() {
        let rest = place_group(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0), Rigid::IDENTITY).rotation;
        let (w, _, ids) = rigged(
            &[
                (Vec3::new(1.0, 2.0, 3.0), None, rest),
                (Vec3::new(4.0, 5.0, 6.0), Some(0), Vec3::ZERO),
            ],
            &[],
        );
        let rig = Rig::new(&w.doc.groups);
        for (&id, pivot) in ids
            .iter()
            .zip([Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0)])
        {
            let frame = rig.rest_frame(id).unwrap();
            assert!(close(Vec3::from(frame.translation), pivot));
        }
    }
}
