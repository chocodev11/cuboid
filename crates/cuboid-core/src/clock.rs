//! `Clock`: frame time from injected instants (spec: app 4.5; arch 10.4).

use std::time::Instant;

/// The most time a single frame may count for, in seconds; a longer stall (a drag of the
/// window, a suspended laptop) advances the clock by this much only.
///
/// spec: arch 10.4
pub const MAX_FRAME_DT: f64 = 1.0 / 3.0;

/// Whole seconds after which the status line falls back to "Ready".
///
/// spec: app 19.1, ui 4
pub const STATUS_READY_AFTER_S: u32 = 10;

/// Frame time. Time is injected, never read from the system inside the core, so scenario tests
/// run on a scripted clock.
///
/// spec: arch 10.4, arch 6 (cuboid-core)
#[derive(Clone, Debug, Default)]
pub struct Clock {
    /// Seconds since the first frame, as of the last [`Clock::advance`].
    pub now: f64,
    /// Seconds the last frame took.
    pub frame_dt: f64,
    last: Option<Instant>,
}

impl Clock {
    /// Advances to the frame time `now`. The first call starts the clock at zero; an instant
    /// before the previous one counts as no time.
    pub fn advance(&mut self, now: Instant) {
        let dt = self
            .last
            .map_or(0.0, |last| {
                now.saturating_duration_since(last).as_secs_f64()
            })
            .min(MAX_FRAME_DT);
        self.last = Some(now);
        self.now += dt;
        self.frame_dt = dt;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn time_follows_the_injected_instants() {
        let t0 = Instant::now();
        let mut clock = Clock::default();
        clock.advance(t0);
        assert!(clock.now.abs() < 1e-12);
        clock.advance(t0 + Duration::from_millis(100));
        assert!((clock.now - 0.1).abs() < 1e-9);
        assert!((clock.frame_dt - 0.1).abs() < 1e-9);
    }

    #[test]
    fn a_long_stall_counts_for_at_most_a_third_of_a_second() {
        let t0 = Instant::now();
        let mut clock = Clock::default();
        clock.advance(t0);
        clock.advance(t0 + Duration::from_secs(60));
        assert!((clock.now - MAX_FRAME_DT).abs() < 1e-9);
        clock.advance(t0 + Duration::from_secs(30));
        assert!(
            (clock.now - MAX_FRAME_DT).abs() < 1e-9,
            "going back adds nothing"
        );
    }
}
