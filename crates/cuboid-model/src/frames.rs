//! Animated textures: a vertical strip of frames, the steps they play in and what a tick shows
//! (spec: textures T1 2).
//!
//! The image of an entry with an animation is the whole strip. Like every image of the model its
//! rows run bottom to top, so tile 0, the first picture of the file, is the top of the picture and
//! the last rows of the data. Face UVs stay relative to one frame, as in Minecraft.

use std::ops::Range;
use std::sync::Arc;

use thiserror::Error;

/// The most frames a strip holds.
pub const MAX_TILES: u32 = 4_096;

/// The most steps an explicit play order holds.
pub const MAX_STEPS: usize = 4_096;

/// The longest time of a frame, in game ticks: one hour.
pub const MAX_STEP_TICKS: u32 = 72_000;

/// Game ticks per second.
pub const TICKS_PER_SECOND: u32 = 20;

/// The most bytes of file text an animation keeps in [`TextureAnimation::extra`].
pub const MAX_FRAMES_EXTRA: usize = 16 << 10;

/// Why an animation does not describe its picture. Nothing is changed in any case.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Error)]
pub enum FramesError {
    /// The frame is not as wide as the picture, or the picture is not a whole number of frames.
    #[error("the frame size does not fit the picture")]
    Size,
    /// The strip has more frames than [`MAX_TILES`].
    #[error("a strip holds at most 4096 frames")]
    TooManyTiles,
    /// The play order has no step or more than [`MAX_STEPS`].
    #[error("a play order has between 1 and 4096 steps")]
    Steps,
    /// A step names a frame the strip does not have.
    #[error("a step names a frame the strip does not have")]
    NoSuchTile,
    /// A time is not between 1 and [`MAX_STEP_TICKS`] ticks.
    #[error("a frame time is between 1 and 72000 ticks")]
    Time,
    /// The kept file text is longer than [`MAX_FRAMES_EXTRA`].
    #[error("the kept file text of an animation is too long")]
    Extra,
}

/// One step of an explicit play order.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FrameStep {
    /// The tile shown, counted from the top of the strip.
    pub index: u32,
    /// How long it stays in ticks; `None` takes [`TextureAnimation::frametime`].
    pub time: Option<u32>,
}

impl FrameStep {
    /// A step that takes the default time.
    pub fn of(index: u32) -> Self {
        Self { index, time: None }
    }
}

/// How an animated texture plays: the frames of its strip and the order and time of the steps.
///
/// spec: textures T1 2
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureAnimation {
    /// Width of a frame: the width of the picture.
    pub frame_width: u32,
    /// Height of a frame; the picture is a whole number of them, one above the other.
    pub frame_height: u32,
    /// Ticks a step lasts that has no time of its own, at least 1.
    pub frametime: u32,
    /// Blend each frame into the next one over its time.
    pub interpolate: bool,
    /// The play order; `None` shows every tile once, in order.
    pub frames: Option<Vec<FrameStep>>,
    /// The members of the file that the model does not use, as JSON text, written back as they
    /// were.
    pub extra: Option<Arc<str>>,
}

/// What a tick shows of an animated texture.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Shown {
    /// The step playing.
    pub step: usize,
    /// The tile of the step.
    pub tile: u32,
    /// The tile of the next step; the same as `tile` when the animation does not interpolate.
    pub next: u32,
    /// Ticks the step has been showing; 0 when the animation does not interpolate.
    pub elapsed: u32,
    /// Ticks the step lasts.
    pub time: u32,
}

impl Shown {
    /// How much of `next` the picture holds, from 0 to just below 1.
    pub fn weight(&self) -> f32 {
        self.elapsed as f32 / self.time as f32
    }

    /// Whether the picture is a mix of two tiles.
    pub fn blends(&self) -> bool {
        self.elapsed > 0 && self.next != self.tile
    }
}

impl TextureAnimation {
    /// A plain loop over the tiles of a picture of this width, one tick each.
    pub fn new(frame_width: u32, frame_height: u32) -> Self {
        Self {
            frame_width,
            frame_height,
            frametime: 1,
            interpolate: false,
            frames: None,
            extra: None,
        }
    }

    /// The number of frames in a strip of this height.
    pub fn tiles(&self, image_h: u32) -> u32 {
        image_h.checked_div(self.frame_height).unwrap_or(0)
    }

    /// Whether the animation describes a picture of `w` x `h` texels.
    pub fn check(&self, w: u32, h: u32) -> Result<(), FramesError> {
        let whole = self.frame_height > 0 && h > 0 && h.is_multiple_of(self.frame_height);
        if self.frame_width != w || !whole {
            return Err(FramesError::Size);
        }
        let tiles = self.tiles(h);
        if tiles > MAX_TILES {
            return Err(FramesError::TooManyTiles);
        }
        let timed = |time: u32| (1..=MAX_STEP_TICKS).contains(&time);
        if !timed(self.frametime) {
            return Err(FramesError::Time);
        }
        if let Some(steps) = &self.frames {
            if steps.is_empty() || steps.len() > MAX_STEPS {
                return Err(FramesError::Steps);
            }
            if steps.iter().any(|step| step.index >= tiles) {
                return Err(FramesError::NoSuchTile);
            }
            if steps.iter().filter_map(|step| step.time).any(|t| !timed(t)) {
                return Err(FramesError::Time);
            }
        }
        if self
            .extra
            .as_ref()
            .is_some_and(|e| e.len() > MAX_FRAMES_EXTRA)
        {
            return Err(FramesError::Extra);
        }
        Ok(())
    }

    /// The number of steps of a strip of `tiles` frames.
    pub fn steps(&self, tiles: u32) -> usize {
        self.frames.as_ref().map_or(tiles as usize, Vec::len)
    }

    /// The tile a step shows, kept inside the strip.
    pub fn step_tile(&self, tiles: u32, step: usize) -> u32 {
        let index = match &self.frames {
            Some(steps) => steps.get(step).map_or(0, |s| s.index),
            None => u32::try_from(step).unwrap_or(0),
        };
        index.min(tiles.saturating_sub(1))
    }

    /// The ticks a step lasts.
    pub fn step_ticks(&self, step: usize) -> u32 {
        let own = self
            .frames
            .as_ref()
            .and_then(|steps| steps.get(step))
            .and_then(|s| s.time);
        own.unwrap_or(self.frametime).max(1)
    }

    /// The ticks of one pass through all steps.
    pub fn cycle(&self, tiles: u32) -> u32 {
        (0..self.steps(tiles)).map(|s| self.step_ticks(s)).sum()
    }

    /// The tick a step starts at within a pass.
    pub fn step_start(&self, step: usize) -> u32 {
        (0..step).map(|s| self.step_ticks(s)).sum()
    }

    /// What the strip shows at a tick: the playing continues after the last step with the first.
    pub fn shown(&self, tiles: u32, tick: u32) -> Shown {
        let steps = self.steps(tiles);
        let mut left = tick.checked_rem(self.cycle(tiles)).unwrap_or(0);
        for step in 0..steps {
            let time = self.step_ticks(step);
            if left >= time {
                left -= time;
                continue;
            }
            let tile = self.step_tile(tiles, step);
            let (next, elapsed) = if self.interpolate {
                (self.step_tile(tiles, (step + 1) % steps), left)
            } else {
                (tile, 0)
            };
            return Shown {
                step,
                tile,
                next,
                elapsed,
                time,
            };
        }
        Shown {
            step: 0,
            tile: 0,
            next: 0,
            elapsed: 0,
            time: 1,
        }
    }

    /// The rows of a tile in the data of the picture, which runs bottom to top.
    pub fn tile_rows(&self, image_h: u32, tile: u32) -> Range<u32> {
        let tiles = self.tiles(image_h);
        let below = tiles.saturating_sub(1).saturating_sub(tile);
        let first = below * self.frame_height;
        first..first + self.frame_height
    }
}

/// How a strip plays, as Blockbench names the orders of a texture.
///
/// spec: textures T1 3
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FrameOrder {
    /// Every tile once, in order.
    Loop,
    /// Every tile once, from the last to the first.
    Backwards,
    /// Up to the last tile and back down to the second one.
    BackAndForth,
    /// Any other steps, with times of their own or not.
    Custom,
}

impl FrameOrder {
    /// The steps of the order for a strip of `tiles` frames; `None` for the loop, which needs no
    /// steps, and for a custom order, which is given.
    pub fn steps(self, tiles: u32) -> Option<Vec<FrameStep>> {
        let up = |range: Range<u32>| range.map(FrameStep::of);
        match self {
            FrameOrder::Loop | FrameOrder::Custom => None,
            FrameOrder::Backwards => Some(up(0..tiles).rev().collect()),
            FrameOrder::BackAndForth if tiles < 2 => None,
            FrameOrder::BackAndForth => Some(up(0..tiles).chain(up(1..tiles - 1).rev()).collect()),
        }
    }

    /// The order a list of steps is, `Custom` when it is none of the other three.
    pub fn of(frames: Option<&[FrameStep]>, tiles: u32) -> FrameOrder {
        let Some(frames) = frames else {
            return FrameOrder::Loop;
        };
        let is = |order: FrameOrder| order.steps(tiles).is_some_and(|steps| steps == frames);
        let plain = frames
            .iter()
            .zip(0..)
            .all(|(step, at)| *step == FrameStep::of(at))
            && frames.len() == tiles as usize;
        if plain {
            FrameOrder::Loop
        } else if is(FrameOrder::Backwards) {
            FrameOrder::Backwards
        } else if is(FrameOrder::BackAndForth) {
            FrameOrder::BackAndForth
        } else {
            FrameOrder::Custom
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(list: &[(u32, Option<u32>)]) -> Vec<FrameStep> {
        list.iter()
            .map(|&(index, time)| FrameStep { index, time })
            .collect()
    }

    fn at(animation: &TextureAnimation, tiles: u32, tick: u32) -> (usize, u32) {
        let shown = animation.shown(tiles, tick);
        (shown.step, shown.tile)
    }

    #[test]
    fn a_plain_loop_shows_each_tile_for_the_frametime_and_starts_again() {
        let mut animation = TextureAnimation::new(16, 16);
        animation.frametime = 2;
        assert_eq!(animation.tiles(48), 3);
        assert_eq!(animation.cycle(3), 6);
        let tiles: Vec<u32> = (0..8).map(|tick| at(&animation, 3, tick).1).collect();
        assert_eq!(tiles, [0, 0, 1, 1, 2, 2, 0, 0]);
        // A late tick wraps by the cycle.
        assert_eq!(at(&animation, 3, 6 * 1000 + 3), (1, 1));
    }

    #[test]
    fn custom_steps_have_times_of_their_own_and_the_frametime_otherwise() {
        let mut animation = TextureAnimation::new(16, 16);
        animation.frametime = 2;
        animation.frames = Some(steps(&[(2, None), (0, Some(3)), (1, None)]));
        // Passes of 2 + 3 + 2 = 7 ticks.
        assert_eq!(animation.cycle(3), 7);
        let shown: Vec<(usize, u32)> = (0..9).map(|tick| at(&animation, 3, tick)).collect();
        assert_eq!(
            shown,
            [
                (0, 2),
                (0, 2),
                (1, 0),
                (1, 0),
                (1, 0),
                (2, 1),
                (2, 1),
                (0, 2),
                (0, 2)
            ]
        );
        assert_eq!(animation.step_start(2), 5);
        assert_eq!(animation.steps(3), 3);
    }

    #[test]
    fn interpolating_blends_into_the_next_step_and_the_last_one_into_the_first() {
        let mut animation = TextureAnimation::new(16, 16);
        animation.frametime = 4;
        animation.interpolate = true;
        // Tick 5 is one tick into the second step of four.
        let shown = animation.shown(3, 5);
        assert_eq!(
            (shown.tile, shown.next, shown.elapsed, shown.time),
            (1, 2, 1, 4)
        );
        assert!((shown.weight() - 0.25).abs() < 1e-6);
        assert!(shown.blends());
        // The first tick of a step is the tile itself.
        assert!(!animation.shown(3, 4).blends());
        let last = animation.shown(3, 11);
        assert_eq!((last.tile, last.next, last.elapsed), (2, 0, 3));
        assert!((last.weight() - 0.75).abs() < 1e-6);
        // Without the flag nothing blends and the shown value does not change within a step.
        animation.interpolate = false;
        assert_eq!(animation.shown(3, 5), animation.shown(3, 6));
        assert_eq!(animation.shown(3, 5).elapsed, 0);
    }

    #[test]
    fn a_step_with_its_own_time_interpolates_over_that_time() {
        let mut animation = TextureAnimation::new(16, 16);
        animation.interpolate = true;
        animation.frames = Some(steps(&[(0, Some(10)), (1, Some(2))]));
        let shown = animation.shown(2, 5);
        assert_eq!(
            (shown.tile, shown.next, shown.elapsed, shown.time),
            (0, 1, 5, 10)
        );
        assert!((shown.weight() - 0.5).abs() < 1e-6);
        let second = animation.shown(2, 11);
        assert_eq!(
            (second.tile, second.next, second.elapsed, second.time),
            (1, 0, 1, 2)
        );
    }

    #[test]
    fn a_step_beyond_the_strip_shows_the_last_tile() {
        let mut animation = TextureAnimation::new(16, 16);
        animation.frames = Some(steps(&[(9, None)]));
        assert_eq!(animation.shown(4, 0).tile, 3);
    }

    #[test]
    fn tiles_count_from_the_top_of_a_picture_whose_rows_run_bottom_to_top() {
        let animation = TextureAnimation::new(16, 16);
        // Four frames: tile 0 is the last 16 rows of the data.
        assert_eq!(animation.tile_rows(64, 0), 48..64);
        assert_eq!(animation.tile_rows(64, 3), 0..16);
        assert_eq!(animation.tile_rows(16, 0), 0..16);
    }

    #[test]
    fn the_frame_size_the_times_and_the_steps_are_bounded() {
        let mut animation = TextureAnimation::new(16, 16);
        assert_eq!(animation.check(16, 64), Ok(()));
        assert_eq!(animation.check(16, 40), Err(FramesError::Size));
        assert_eq!(animation.check(32, 64), Err(FramesError::Size));
        assert_eq!(animation.check(16, 0), Err(FramesError::Size));
        assert_eq!(
            animation.check(16, 16 * 4097),
            Err(FramesError::TooManyTiles)
        );
        assert_eq!(animation.check(16, 16 * 4096), Ok(()));

        animation.frametime = 0;
        assert_eq!(animation.check(16, 64), Err(FramesError::Time));
        animation.frametime = 72_001;
        assert_eq!(animation.check(16, 64), Err(FramesError::Time));
        animation.frametime = 72_000;
        assert_eq!(animation.check(16, 64), Ok(()));

        animation.frames = Some(Vec::new());
        assert_eq!(animation.check(16, 64), Err(FramesError::Steps));
        animation.frames = Some(steps(&[(4, None)]));
        assert_eq!(animation.check(16, 64), Err(FramesError::NoSuchTile));
        animation.frames = Some(steps(&[(3, Some(0))]));
        assert_eq!(animation.check(16, 64), Err(FramesError::Time));
        animation.frames = Some(vec![FrameStep::of(0); 4097]);
        assert_eq!(animation.check(16, 64), Err(FramesError::Steps));
        animation.frames = Some(vec![FrameStep::of(0); 4096]);
        assert_eq!(animation.check(16, 64), Ok(()));
    }

    #[test]
    fn the_blockbench_orders_expand_and_are_found_again() {
        let list = |order: FrameOrder, tiles| {
            order
                .steps(tiles)
                .map(|s| s.iter().map(|step| step.index).collect::<Vec<_>>())
        };
        assert_eq!(list(FrameOrder::Loop, 4), None);
        assert_eq!(list(FrameOrder::Backwards, 4), Some(vec![3, 2, 1, 0]));
        assert_eq!(
            list(FrameOrder::BackAndForth, 4),
            Some(vec![0, 1, 2, 3, 2, 1])
        );
        assert_eq!(list(FrameOrder::BackAndForth, 3), Some(vec![0, 1, 2, 1]));
        assert_eq!(list(FrameOrder::BackAndForth, 2), Some(vec![0, 1]));
        assert_eq!(list(FrameOrder::BackAndForth, 1), None);

        for (order, tiles) in [
            (FrameOrder::Backwards, 4),
            (FrameOrder::BackAndForth, 4),
            (FrameOrder::BackAndForth, 3),
        ] {
            let frames = order.steps(tiles);
            assert_eq!(FrameOrder::of(frames.as_deref(), tiles), order);
        }
        assert_eq!(FrameOrder::of(None, 4), FrameOrder::Loop);
        // Every tile in order is the loop, and a step with a time is never one of the three.
        let plain = steps(&[(0, None), (1, None), (2, None)]);
        assert_eq!(FrameOrder::of(Some(&plain), 3), FrameOrder::Loop);
        let timed = steps(&[(0, Some(2)), (1, None), (2, None)]);
        assert_eq!(FrameOrder::of(Some(&timed), 3), FrameOrder::Custom);
        // The same steps over a longer strip are another order: tile 3 would be left out.
        let backwards = steps(&[(2, None), (1, None), (0, None)]);
        assert_eq!(FrameOrder::of(Some(&backwards), 3), FrameOrder::Backwards);
        assert_eq!(FrameOrder::of(Some(&backwards), 4), FrameOrder::Custom);
    }
}
