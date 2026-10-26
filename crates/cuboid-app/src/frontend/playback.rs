//! The player behind the Animations panel (spec: animation A2 6). It holds the animation being
//! shown, the playhead and the pose at the playhead. It reads the document and never writes it:
//! playing is a way of looking at the model, so the undo history, the unsaved-changes flag and the
//! document revision stay as they are.
//!
//! The pose is computed into buffers the player keeps from frame to frame; a frame that does not
//! move the playhead computes nothing.

use std::sync::Arc;
use std::time::Instant;

use cuboid_model::{Animation, AnimationId, Document, LoopMode, Pose, Rev, Rig};
use cuboid_ui::{AnimationRequest, AnimationRow, AnimationsView, LoopKind};

/// The slowest and the fastest playing.
const SPEED: (f32, f32) = (0.1, 4.0);

/// A frame longer than this advances the playhead by this much, so a stall (a dialog, a drag of
/// the window) does not make the animation jump.
const LONGEST_STEP: f32 = 0.25;

/// What the pose in the buffer was computed from.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Shown {
    animation: AnimationId,
    time: f32,
    animations: Rev,
    groups: Rev,
}

/// The animation being played or looked at, and the pose it gives.
pub struct Player {
    animation: Option<AnimationId>,
    playing: bool,
    /// The views show the pose of the animation.
    previewing: bool,
    time: f32,
    speed: f32,
    /// The user's choice to start again at the end; `None` follows the animation's own mode.
    looping: Option<bool>,
    rig: Option<Rig>,
    pose: Pose,
    shown: Option<Shown>,
    /// Changes whenever the pose the views should show changes.
    pose_rev: u64,
    last: Option<Instant>,
    /// The rows of the list, kept while the animations stay as they are.
    rows: Arc<[AnimationRow]>,
    rows_key: Option<Rev>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            animation: None,
            playing: false,
            previewing: false,
            time: 0.0,
            speed: 1.0,
            looping: None,
            rig: None,
            pose: Pose::new(),
            shown: None,
            pose_rev: 0,
            last: None,
            rows: Arc::from([]),
            rows_key: None,
        }
    }
}

fn loop_kind(mode: LoopMode) -> LoopKind {
    match mode {
        LoopMode::Once => LoopKind::Once,
        LoopMode::Loop => LoopKind::Loop,
        LoopMode::Hold => LoopKind::Hold,
    }
}

impl Player {
    /// The pose the views should show, `None` for the rest pose.
    pub fn pose(&self) -> Option<&Pose> {
        self.previewing.then_some(&self.pose)
    }

    /// Changes whenever [`Player::pose`] does.
    pub fn pose_rev(&self) -> u64 {
        self.pose_rev
    }

    /// The playhead advances with the clock.
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Whether the playhead starts again at the end of the animation.
    fn loops(&self, animation: &Animation) -> bool {
        self.looping
            .unwrap_or(animation.loop_mode == LoopMode::Loop)
    }

    fn show(&mut self, previewing: bool) {
        if self.previewing != previewing {
            self.previewing = previewing;
            self.pose_rev += 1;
        }
    }

    /// Does what the panel asked. Nothing here touches the document.
    pub fn request(&mut self, doc: &Document, request: AnimationRequest) {
        let current = self.animation.and_then(|id| doc.animations.get(id));
        match request {
            AnimationRequest::Select(raw) => {
                let Some(&id) = doc.animations.ids().iter().find(|a| a.get() == raw) else {
                    return;
                };
                self.animation = Some(id);
                self.playing = false;
                self.time = 0.0;
                self.last = None;
                // A pose that is shown follows the choice; a rest pose stays.
                self.shown = None;
                if self.previewing {
                    self.pose_rev += 1;
                }
            }
            AnimationRequest::Play => {
                let animation = current.or_else(|| doc.animations.items().first().map(|a| &**a));
                let Some(animation) = animation else {
                    return;
                };
                if self.animation.is_none() {
                    self.animation = doc.animations.ids().first().copied();
                }
                if !self.loops(animation) && self.time >= animation.length {
                    self.time = 0.0;
                }
                self.playing = true;
                self.last = None;
                self.show(true);
            }
            AnimationRequest::Pause => {
                self.playing = false;
                self.last = None;
            }
            AnimationRequest::Stop => {
                self.playing = false;
                self.last = None;
                self.time = 0.0;
                self.show(false);
            }
            AnimationRequest::Seek(time) => {
                let Some(animation) = current else {
                    return;
                };
                self.time = if time.is_finite() {
                    time.clamp(0.0, animation.length)
                } else {
                    0.0
                };
                self.show(true);
            }
            AnimationRequest::SetSpeed(speed) => {
                if speed.is_finite() {
                    self.speed = speed.clamp(SPEED.0, SPEED.1);
                }
            }
            AnimationRequest::SetLoop(looping) => self.looping = Some(looping),
        }
    }

    /// Moves the playhead to the clock and computes the pose when something it depends on changed:
    /// the playhead, the animation, the groups. The animation that went from the document stops
    /// the player.
    pub fn tick(&mut self, doc: &Document, now: Instant) {
        let Some(id) = self.animation else {
            return;
        };
        let Some(animation) = doc.animations.get(id) else {
            self.animation = None;
            self.playing = false;
            self.last = None;
            self.time = 0.0;
            self.show(false);
            return;
        };
        if self.playing {
            let step = self.last.map_or(0.0, |last| {
                now.saturating_duration_since(last).as_secs_f32()
            });
            self.last = Some(now);
            self.time += step.min(LONGEST_STEP) * self.speed;
            if self.time >= animation.length {
                if self.loops(animation) && animation.length > 0.0 {
                    self.time %= animation.length;
                } else {
                    self.time = animation.length;
                    self.playing = false;
                    self.last = None;
                }
            }
        }
        if !self.previewing {
            return;
        }
        let key = Shown {
            animation: id,
            time: self.time,
            animations: doc.animations.rev(),
            groups: doc.groups.rev(),
        };
        if self.shown == Some(key) {
            return;
        }
        if self
            .rig
            .as_ref()
            .is_none_or(|rig| rig.groups_rev() != key.groups)
        {
            self.rig = Some(Rig::new(&doc.groups));
        }
        if let Some(rig) = &self.rig {
            rig.pose(animation, self.time, &mut self.pose);
        }
        self.shown = Some(key);
        self.pose_rev += 1;
    }

    /// What the Animations panel shows: the list and the state of the player.
    pub fn view(&mut self, doc: &Document) -> AnimationsView {
        let key = doc.animations.rev();
        if self.rows_key != Some(key) {
            self.rows_key = Some(key);
            self.rows = doc
                .animations
                .ids()
                .iter()
                .zip(doc.animations.items())
                .map(|(id, a)| AnimationRow {
                    id: id.get(),
                    name: Arc::from(&*a.name),
                    length: a.length,
                    loop_kind: loop_kind(a.loop_mode),
                })
                .collect();
        }
        let selected = self.animation.and_then(|id| doc.animations.position(id));
        let looping = self.looping.unwrap_or_else(|| {
            self.animation
                .and_then(|id| doc.animations.get(id))
                .is_some_and(|a| a.loop_mode == LoopMode::Loop)
        });
        AnimationsView {
            rows: Arc::clone(&self.rows),
            selected,
            playing: self.playing,
            previewing: self.previewing,
            time: self.time,
            speed: self.speed,
            looping,
        }
    }
}
