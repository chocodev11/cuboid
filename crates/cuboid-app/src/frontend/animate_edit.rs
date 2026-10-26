//! The changes of the animations that the timeline, the Animate tab and the Animations panel ask
//! for (spec: animate A3 3, 4, 8). Each one becomes a core command, so it is undoable and the
//! document shows it as a change; the player only follows.

use cuboid_core::{AnimationLoop, BoneChannel, Command, KeyAt, KeyInterp, KeyValue, Target};
use cuboid_model::{AnimationId, Document, GroupId, ObjRef, TIME_EPSILON, group_values};
use cuboid_ui::{AnimateEdit, AnimationRequest, ChannelKind, KeyId, KeyKind, LoopKind, NodeId};
use glam::Vec3;

use crate::frontend::Frontend;
use crate::frontend::timeline::{bone_of, channel_of};
use crate::state::{Level, State};
use crate::uimodel::{object_of, target_of};

/// The choices of the UI as the commands have them.
fn interp_of(kind: KeyKind) -> KeyInterp {
    match kind {
        KeyKind::Linear => KeyInterp::Linear,
        KeyKind::CatmullRom => KeyInterp::CatmullRom,
        KeyKind::Bezier => KeyInterp::Bezier,
        KeyKind::Step => KeyInterp::Step,
    }
}

fn loop_of(kind: LoopKind) -> AnimationLoop {
    match kind {
        LoopKind::Once => AnimationLoop::Once,
        LoopKind::Loop => AnimationLoop::Loop,
        LoopKind::Hold => AnimationLoop::Hold,
    }
}

fn bone_channel(kind: ChannelKind) -> BoneChannel {
    channel_of(kind).into()
}

/// The place in the list of the animation the UI names by its id.
fn position_of(doc: &Document, id: u64) -> Option<usize> {
    doc.animations.ids().iter().position(|a| a.get() == id)
}

/// A keyframe by its place, when the animation still has it.
fn key_at(doc: &Document, animation: usize, key: &KeyId) -> Option<KeyAt> {
    let Some(ObjRef::Group(group)) = object_of(doc, key.bone) else {
        return None;
    };
    let list = doc
        .animations
        .items()
        .get(animation)?
        .track(group)?
        .channel(channel_of(key.channel));
    let index = list
        .iter()
        .position(|k| (k.time - key.time).abs() < TIME_EPSILON)?;
    Some(KeyAt {
        group: target_of(doc, key.bone)?,
        channel: bone_channel(key.channel),
        index,
    })
}

/// A name for a new animation that the list does not have yet: `animation`, `animation2`, ...
fn new_name(doc: &Document) -> String {
    let taken = |name: &str| doc.animations.items().iter().any(|a| &*a.name == name);
    let mut name = "animation".to_owned();
    let mut n = 1;
    while taken(&name) {
        n += 1;
        name = format!("animation{n}");
    }
    name
}

/// What a keyframe of the playhead is written to: the chosen animation, the bone and the time.
struct Site {
    animation: usize,
    id: AnimationId,
    time: f32,
}

impl Frontend {
    /// The chosen animation and the frame the playhead stands on.
    fn site(&self, doc: &Document) -> Option<Site> {
        let id = self.player.animation()?;
        let animation = doc.animations.get(id)?;
        Some(Site {
            animation: doc.animations.position(id)?,
            id,
            time: animation.on_frame(self.player.time()),
        })
    }

    /// The command that keys a channel of a bone at the playhead.
    fn key_command(site: &Site, group: Target, kind: ChannelKind, value: Vec3) -> Command {
        Command::SetKeyframe {
            animation: site.animation,
            group,
            channel: bone_channel(kind),
            time: site.time,
            value: KeyValue::nums(value.to_array()),
            interp: None,
        }
    }

    /// Runs commands that are one edit: one undo step.
    fn run_together(&mut self, state: &mut State, commands: Vec<Command>) {
        for command in commands {
            state.run_merged(command, self.gesture);
        }
        self.gesture += 1;
    }

    /// Runs the command of a drag: the previews share one undo step, the committed value ends it.
    fn run_drag(&mut self, state: &mut State, command: Command, commit: bool) {
        state.run_merged(command, self.gesture);
        if commit {
            self.gesture += 1;
        }
    }

    /// The group a node of the UI names.
    fn group_of(doc: &Document, node: NodeId) -> Option<GroupId> {
        match object_of(doc, node)? {
            ObjRef::Group(group) => Some(group),
            _ => None,
        }
    }

    /// The keyframes of a request, by place; the ones the animation no longer has are left out.
    fn places(doc: &Document, animation: usize, keys: &[KeyId]) -> Vec<KeyAt> {
        keys.iter()
            .filter_map(|key| key_at(doc, animation, key))
            .collect()
    }

    pub(crate) fn animate_edit(&mut self, state: &mut State, edit: AnimateEdit) {
        let doc = state.editor.core.doc();
        match edit {
            AnimateEdit::NewAnimation => {
                let name = new_name(doc);
                state.run_command(Command::AddAnimation { name, length: 1.0 });
                self.choose_last(state, 0.0);
            }
            AnimateEdit::DuplicateAnimation { id } => {
                let Some(animation) = position_of(doc, id) else {
                    return;
                };
                state.run_command(Command::DuplicateAnimation { animation });
                let doc = state.editor.core.doc();
                if let Some(copy) = doc.animations.ids().get(animation + 1).copied() {
                    self.player
                        .request(doc, AnimationRequest::Select(copy.get()));
                }
            }
            AnimateEdit::DeleteAnimation { id } => {
                if let Some(animation) = position_of(doc, id) {
                    state.run_command(Command::RemoveAnimation { animation });
                }
            }
            AnimateEdit::RenameAnimation { id, name } => {
                if let Some(animation) = position_of(doc, id) {
                    state.run_command(set_animation(animation, Some(name), None, None, None));
                }
            }
            AnimateEdit::SetLength { id, length, commit } => {
                if let Some(animation) = position_of(doc, id) {
                    let command = set_animation(animation, None, Some(length), None, None);
                    self.run_drag(state, command, commit);
                }
            }
            AnimateEdit::SetLoop { id, kind } => {
                if let Some(animation) = position_of(doc, id) {
                    let mode = Some(loop_of(kind));
                    state.run_command(set_animation(animation, None, None, mode, None));
                }
            }
            AnimateEdit::SetFrameRate { id, fps, commit } => {
                if let Some(animation) = position_of(doc, id) {
                    let command = set_animation(animation, None, None, None, Some(fps));
                    self.run_drag(state, command, commit);
                }
            }
            AnimateEdit::Key {
                bone,
                channel,
                value,
                commit,
            } => {
                // A drag of a field keys one frame: a playing animation stops on a frame first.
                if self.player.playing() {
                    self.player.request(doc, AnimationRequest::Pause);
                }
                let (Some(site), Some(group)) = (self.site(doc), target_of(doc, bone)) else {
                    return;
                };
                let command = Self::key_command(&site, group, channel, value);
                self.run_drag(state, command, commit);
            }
            AnimateEdit::KeyHere { bone, channel } => {
                let (Some(site), Some(group), Some(id)) = (
                    self.site(doc),
                    target_of(doc, bone),
                    Self::group_of(doc, bone),
                ) else {
                    return;
                };
                let Some(animation) = doc.animations.get(site.id) else {
                    return;
                };
                let values = group_values(animation, id, site.time);
                let value = match channel {
                    ChannelKind::Rotation => values.rotation,
                    ChannelKind::Position => values.position,
                    ChannelKind::Scale => values.scale,
                };
                state.run_command(Self::key_command(&site, group, channel, value));
            }
            AnimateEdit::KeySelected => self.key_selected(state),
            AnimateEdit::SetValue { key, value } => {
                let (Some(site), Some(group)) = (self.site(doc), target_of(doc, key.bone)) else {
                    return;
                };
                state.run_command(Command::SetKeyframe {
                    animation: site.animation,
                    group,
                    channel: bone_channel(key.channel),
                    time: key.time,
                    value: value.map(KeyValue::Text),
                    interp: None,
                });
            }
            AnimateEdit::Move {
                keys,
                delta,
                commit,
            } => {
                let Some(site) = self.site(doc) else {
                    return;
                };
                let keys = Self::places(doc, site.animation, &keys);
                let command = Command::MoveKeyframes {
                    animation: site.animation,
                    keys,
                    delta,
                };
                self.run_drag(state, command, commit);
            }
            AnimateEdit::Interp { keys, kind } => {
                let Some(site) = self.site(doc) else {
                    return;
                };
                state.run_command(Command::SetKeyframeInterp {
                    animation: site.animation,
                    keys: Self::places(doc, site.animation, &keys),
                    interp: interp_of(kind),
                });
            }
            AnimateEdit::Delete { keys } => {
                let Some(site) = self.site(doc) else {
                    return;
                };
                state.run_command(Command::RemoveKeyframes {
                    animation: site.animation,
                    keys: Self::places(doc, site.animation, &keys),
                });
            }
            AnimateEdit::Copy { keys } => {
                let Some(site) = self.site(doc) else {
                    return;
                };
                state.run_command(Command::CopyKeyframes {
                    animation: site.animation,
                    keys: Self::places(doc, site.animation, &keys),
                });
            }
            AnimateEdit::Paste { bone } => {
                let Some(site) = self.site(doc) else {
                    return;
                };
                let group = bone
                    .filter(|&b| Self::group_of(doc, b).is_some())
                    .and_then(|b| target_of(doc, b));
                state.run_command(Command::PasteKeyframes {
                    animation: site.animation,
                    time: site.time,
                    group,
                });
            }
        }
    }

    /// Keys every channel of the selected bones at the playhead, with the value it has there:
    /// one undo step.
    fn key_selected(&mut self, state: &mut State) {
        let core = &state.editor.core;
        let doc = core.doc();
        let Some(site) = self.site(doc) else {
            state.toast(Level::Info, "Add an animation to key the pose.".to_owned());
            return;
        };
        let mut bones: Vec<GroupId> = Vec::new();
        for bone in core
            .selection()
            .list
            .iter()
            .filter_map(|&obj| bone_of(doc, obj))
        {
            if !bones.contains(&bone) {
                bones.push(bone);
            }
        }
        let Some(animation) = doc.animations.get(site.id) else {
            return;
        };
        let mut commands = Vec::new();
        for bone in bones {
            let Some(group) = doc.groups.position(bone).map(Target::Group) else {
                continue;
            };
            let values = group_values(animation, bone, site.time);
            for (kind, value) in [
                (ChannelKind::Rotation, values.rotation),
                (ChannelKind::Position, values.position),
                (ChannelKind::Scale, values.scale),
            ] {
                commands.push(Self::key_command(&site, group, kind, value));
            }
        }
        if commands.is_empty() {
            state.toast(Level::Info, "Select a bone to key it.".to_owned());
            return;
        }
        self.run_together(state, commands);
    }

    /// Chooses the animation at the end of the list, at `time`.
    fn choose_last(&mut self, state: &State, time: f32) {
        let doc = state.editor.core.doc();
        if let Some(last) = doc.animations.ids().last().copied() {
            self.player
                .request(doc, AnimationRequest::Select(last.get()));
            self.player.request(doc, AnimationRequest::Seek(time));
        }
    }
}

/// A change of the fields of an animation; `None` keeps a field.
fn set_animation(
    animation: usize,
    name: Option<String>,
    length: Option<f32>,
    loop_mode: Option<AnimationLoop>,
    snapping: Option<u32>,
) -> Command {
    Command::SetAnimation {
        animation,
        name,
        length,
        loop_mode,
        snapping,
    }
}
