//! The animated textures in the views: the texture tick that every animated texture shows, the
//! clock that moves it while they play, and the requests of the Textures panel and the Frames
//! section (spec: textures T1 4, 5). Like the pose of the player, the tick is a way of looking at
//! the model: the document does not hold it, so playing leaves the undo history, the unsaved
//! changes flag and the revision as they are.

use std::time::{Duration, Instant};

use cuboid_core::{Command, FrameSpec, TextureFrames};
use cuboid_model::TICKS_PER_SECOND;
use cuboid_ui::{FramesEdit, FramesRequest};

use crate::frontend::Frontend;
use crate::state::State;

/// The time of one game tick.
const TICK: Duration = Duration::from_millis(1000 / TICKS_PER_SECOND as u64);

/// A frame longer than this moves the tick by this much, so a stall (a dialog, a drag of the
/// window) does not skip a stretch of the animation.
const LONGEST_STEP: Duration = Duration::from_millis(250);

/// The texture tick and the clock behind it. By default the tick stays at 0, where every animated
/// texture shows its first step; playing moves it at 20 ticks a second.
#[derive(Debug, Default)]
pub struct TextureClock {
    tick: u32,
    playing: bool,
    /// When the clock was last asked, and the time since the last whole tick.
    last: Option<Instant>,
    rest: Duration,
}

impl TextureClock {
    /// The tick the views show.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// The tick moves with the clock.
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Starts or stops the tick moving; it stays where it is.
    pub fn play(&mut self, on: bool) {
        self.playing = on;
        self.rest = Duration::ZERO;
        self.last = None;
    }

    /// Puts the tick where it is, and stops.
    pub fn show(&mut self, tick: u32) {
        self.tick = tick;
        self.play(false);
    }

    /// Moves the tick to the time `now` while it plays.
    pub fn step(&mut self, now: Instant) {
        let last = self.last.replace(now);
        let Some(last) = last.filter(|_| self.playing) else {
            return;
        };
        self.rest += now.saturating_duration_since(last).min(LONGEST_STEP);
        let ticks = u32::try_from(self.rest.as_millis() / TICK.as_millis()).unwrap_or(0);
        self.rest -= TICK * ticks;
        self.tick = self.tick.wrapping_add(ticks);
    }

    /// How long until the next tick, while the clock runs: the frame loop wakes up then and at
    /// no other time.
    pub fn wake(&self) -> Option<Duration> {
        self.playing.then(|| TICK.saturating_sub(self.rest))
    }
}

/// What the Frames section sets, as the command has it.
fn frames_of(edit: FramesEdit) -> TextureFrames {
    TextureFrames {
        frame_height: edit.frame_height,
        frametime: edit.frametime,
        interpolate: edit.interpolate,
        frames: edit
            .steps
            .into_iter()
            .map(|(index, time)| FrameSpec { index, time })
            .collect(),
    }
}

impl Frontend {
    /// Moves the texture tick to the clock of the frame. Nothing plays in a model without an
    /// animated texture.
    pub(crate) fn step_textures(&mut self, state: &State) {
        let animated = state
            .editor
            .core
            .doc()
            .textures
            .entries
            .iter()
            .any(|entry| entry.animation.is_some());
        if !animated {
            self.textures.play(false);
        }
        self.textures.step(state.now);
        self.model.tick = self.textures.tick();
    }

    /// Hands the core and the extractor the tick, after the requests of the frame ran.
    pub(crate) fn send_texture_tick(&mut self, state: &mut State) {
        let tick = self.textures.tick();
        self.model.tick = tick;
        state.editor.core.set_texture_tick(tick);
        self.extractor.set_texture_tick(tick);
    }

    /// How long until the next frame the animated textures need, `None` while they stand still.
    pub fn texture_wake(&self) -> Option<Duration> {
        self.textures.wake()
    }

    /// The requests of the Textures panel and the Frames section.
    pub(crate) fn frames_request(&mut self, state: &mut State, request: FramesRequest) {
        let doc = state.editor.core.doc();
        let texture = |at: usize| doc.textures.entries.get(at);
        match request {
            FramesRequest::Play(on) => self.textures.play(on),
            FramesRequest::Show { texture: at, step } => {
                if let Some(animation) = texture(at).and_then(|e| e.animation.as_ref()) {
                    self.textures.show(animation.step_start(step));
                }
            }
            FramesRequest::Set {
                texture: at,
                animation,
            } => {
                let Some(entry) = texture(at) else {
                    return;
                };
                let command = Command::SetTextureAnimation {
                    label: entry.label.to_string(),
                    animation: animation.map(frames_of),
                };
                state.run_command(command);
            }
            FramesRequest::AddFrame { texture: at } => {
                let Some(entry) = texture(at) else {
                    return;
                };
                let Some(animation) = &entry.animation else {
                    return;
                };
                let tiles = animation.tiles(entry.texture.image.h);
                let tile = animation.shown(tiles, self.textures.tick()).tile;
                let command = Command::AddTextureFrame {
                    label: entry.label.to_string(),
                    tile,
                };
                state.run_command(command);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tick_stands_still_until_it_plays_and_moves_by_whole_ticks() {
        let start = Instant::now();
        let mut clock = TextureClock::default();
        clock.step(start);
        clock.step(start + Duration::from_secs(1));
        assert_eq!(clock.tick(), 0);
        assert_eq!(clock.wake(), None);

        clock.play(true);
        clock.step(start + Duration::from_secs(2));
        assert_eq!(clock.tick(), 0, "the first frame only starts the clock");
        // 120 ms are two ticks and 20 ms of the third.
        clock.step(start + Duration::from_millis(2120));
        assert_eq!(clock.tick(), 2);
        assert_eq!(clock.wake(), Some(Duration::from_millis(30)));
        // Another 30 ms complete the third tick.
        clock.step(start + Duration::from_millis(2150));
        assert_eq!(clock.tick(), 3);
    }

    #[test]
    fn a_stall_moves_the_tick_by_a_quarter_second_at_most_and_pausing_keeps_the_tick() {
        let start = Instant::now();
        let mut clock = TextureClock::default();
        clock.play(true);
        clock.step(start);
        clock.step(start + Duration::from_secs(60));
        assert_eq!(clock.tick(), 5, "250 ms are five ticks");
        clock.play(false);
        clock.step(start + Duration::from_secs(61));
        assert_eq!(clock.tick(), 5);
        assert!(!clock.playing());
        // Playing again starts from where it stopped, without a jump.
        clock.play(true);
        clock.step(start + Duration::from_secs(90));
        clock.step(start + Duration::from_millis(90_100));
        assert_eq!(clock.tick(), 7);
    }

    #[test]
    fn showing_a_tick_stops_the_clock_there() {
        let mut clock = TextureClock::default();
        clock.play(true);
        clock.show(40);
        assert_eq!((clock.tick(), clock.playing()), (40, false));
        assert_eq!(clock.wake(), None);
    }
}
