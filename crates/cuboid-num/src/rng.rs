//! The deterministic random generator behind every "random" of the editor (tools 16.5).

/// Marsaglia's xorshift128 with a seed expanded by splitmix64.
///
/// `Core` owns one generator and hands it to every operation that draws random values (random
/// element colours, blank atlas colours, "Randomize", noise jitter, z-fighting offsets).
/// Production seeds it from the clock; tests seed a fixed value or mask the affected fields.
/// The sequence for a seed is part of the recorded scenarios, so the algorithm must not change.
///
/// spec: tools 16.5, architecture 7
#[derive(Clone, Debug)]
pub struct EngineRng {
    state: [u32; 4],
}

impl EngineRng {
    /// Creates a generator whose sequence depends only on `seed`.
    pub fn seeded(seed: u64) -> Self {
        let mut mix = seed;
        let mut next = || {
            mix = mix.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = mix;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        // Two consecutive splitmix64 outputs are never both zero, so the state is never zero.
        let [a, b] = [next(), next()];
        Self {
            state: [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32],
        }
    }

    fn next_u32(&mut self) -> u32 {
        let [x, y, z, w] = self.state;
        let t = x ^ (x << 11);
        let next = w ^ (w >> 19) ^ t ^ (t >> 8);
        self.state = [y, z, w, next];
        next
    }

    /// A uniform float in `[0, 1)` with 24 random bits.
    pub fn unit_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// A uniform float between `lo` and `hi`; `hi` itself is reached only by rounding.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_SEED_0: [u32; 4] = [4_221_392_575, 471_550_101, 1_994_856_487, 3_703_984_991];
    const GOLDEN_SEED_1: [u32; 4] = [630_467_305, 3_845_195_514, 4_157_481_967, 363_478_578];
    const GOLDEN_SEED_MAX: [u32; 4] = [3_685_338_456, 4_107_554_195, 2_606_463_242, 2_366_764_742];

    fn take(seed: u64, count: usize) -> Vec<u32> {
        let mut rng = EngineRng::seeded(seed);
        (0..count).map(|_| rng.next_u32()).collect()
    }

    /// The sequence of a seed is part of the recorded scenarios and must never change.
    #[test]
    fn sequences_are_pinned() {
        assert_eq!(take(0, 4), GOLDEN_SEED_0);
        assert_eq!(take(1, 4), GOLDEN_SEED_1);
        assert_eq!(take(u64::MAX, 4), GOLDEN_SEED_MAX);
    }

    /// Marsaglia's published xorshift128 sequence for his initial state.
    #[test]
    fn recurrence_is_marsaglias_xorshift128() {
        let mut rng = EngineRng {
            state: [123_456_789, 362_436_069, 521_288_629, 88_675_123],
        };
        let out: Vec<u32> = (0..5).map(|_| rng.next_u32()).collect();
        assert_eq!(
            out,
            [
                3_701_687_786,
                458_299_110,
                2_500_872_618,
                3_633_119_408,
                516_391_518
            ]
        );
    }

    #[test]
    fn same_seed_same_sequence_and_seeds_differ() {
        assert_eq!(take(7, 64), take(7, 64));
        assert_ne!(take(7, 8), take(8, 8));
        assert_ne!(take(0, 8), take(1, 8));
        let mut a = EngineRng::seeded(9);
        let mut b = a.clone();
        assert_eq!(a.unit_f32(), b.unit_f32());
    }

    #[test]
    fn unit_floats_cover_zero_to_one_evenly() {
        let mut rng = EngineRng::seeded(42);
        let mut buckets = [0u32; 10];
        for _ in 0..100_000 {
            let x = rng.unit_f32();
            assert!((0.0..1.0).contains(&x), "{x}");
            buckets[(x * 10.0) as usize] += 1;
        }
        for (i, count) in buckets.into_iter().enumerate() {
            assert!((9_500..10_500).contains(&count), "bucket {i} holds {count}");
        }
    }

    #[test]
    fn ranges_stay_inside_their_bounds() {
        let mut rng = EngineRng::seeded(3);
        for _ in 0..10_000 {
            // Random element colours: each channel in [0.6, 1].
            let c = rng.range_f32(0.6, 1.0);
            assert!((0.6..=1.0).contains(&c), "{c}");
            let offset = rng.range_f32(-0.0005, 0.0005);
            assert!((-0.0005..=0.0005).contains(&offset), "{offset}");
        }
        assert_eq!(rng.range_f32(2.0, 2.0), 2.0);
    }
}
