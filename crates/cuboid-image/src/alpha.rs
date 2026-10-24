//! Alpha statistics: "does this crop hold partial alpha" in constant time.

use crate::contract::check_size;

/// Summed-area table of the texels with `0 < alpha < 255`, answering "does this crop hold
/// partial alpha" in O(1). The renderer uses it to find binary objects whose transparency passes
/// merge into one draw.
///
/// spec: arch 8.3, arch 13.4
#[derive(Clone, Debug, Default)]
pub struct AlphaStats {
    w: u32,
    h: u32,
    /// `(w + 1) * (h + 1)` prefix sums, row after row; empty while the table is not built or when
    /// no texel has partial alpha.
    sat: Vec<u32>,
    /// The image was scanned and holds no partial texel, so no table is kept.
    none: bool,
}

impl AlphaStats {
    /// Builds the table of a decoded `w * h` image, RGBA8, rows bottom to top. A size beyond
    /// [`check_size`] or one that does not match the data builds no table.
    pub fn of(w: u32, h: u32, rgba: &[u8]) -> Self {
        if check_size(w, h).is_err() || rgba.len() != w as usize * h as usize * 4 {
            return Self::default();
        }
        if !rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|&[.., a]| a != 0 && a != 255)
        {
            return Self {
                w,
                h,
                sat: Vec::new(),
                none: true,
            };
        }
        let stride = w as usize + 1;
        let mut sat = vec![0u32; stride * (h as usize + 1)];
        let mut above = vec![0u32; stride];
        let rows = sat.chunks_exact_mut(stride).skip(1);
        for (row, texels) in rows.zip(rgba.chunks((w as usize * 4).max(1))) {
            let mut run = 0;
            let cells = row.iter_mut().skip(1).zip(above.iter().skip(1));
            for ((cell, up), &[.., a]) in cells.zip(texels.as_chunks::<4>().0) {
                run += u32::from(a != 0 && a != 255);
                *cell = up + run;
            }
            above.copy_from_slice(row);
        }
        Self {
            w,
            h,
            sat,
            none: false,
        }
    }

    /// Whether the texel rectangle `[x0, x1) x [y0, y1)` holds a texel with partial alpha. The
    /// rectangle is cut to the image.
    ///
    /// A table that is not built answers `true`, the conservative answer: a caller then draws
    /// the object through the exact translucent path.
    pub fn any_partial(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> bool {
        if self.none {
            return false;
        }
        if self.sat.is_empty() {
            return true;
        }
        let (x1, y1) = (x1.min(self.w), y1.min(self.h));
        if x0 >= x1 || y0 >= y1 {
            return false;
        }
        let at = |x: u32, y: u32| {
            let i = y as usize * (self.w as usize + 1) + x as usize;
            self.sat.get(i).copied().unwrap_or_default()
        };
        at(x1, y1) + at(x0, y0) > at(x0, y1) + at(x1, y0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuboid_num::{EngineRng, trunc_i32};

    fn random_alpha(w: u32, h: u32, rng: &mut EngineRng) -> Vec<u8> {
        (0..w * h)
            .flat_map(|_| {
                // Mostly binary alpha so that crops without partial texels occur.
                let a = match rng.unit_f32() {
                    r if r < 0.45 => 0,
                    r if r < 0.9 => 255,
                    r => u8::try_from(trunc_i32(r * 250.0) % 254 + 1).unwrap(),
                };
                [1, 2, 3, a]
            })
            .collect()
    }

    #[test]
    fn every_crop_agrees_with_counting_the_texels() {
        let (w, h) = (9u32, 7u32);
        let mut rng = EngineRng::seeded(11);
        for _ in 0..4 {
            let rgba = random_alpha(w, h, &mut rng);
            let stats = AlphaStats::of(w, h, &rgba);
            for y0 in 0..h {
                for y1 in y0..=h {
                    for x0 in 0..w {
                        for x1 in x0..=w {
                            let want = (y0..y1).any(|y| {
                                (x0..x1).any(|x| {
                                    let a = rgba[((y * w + x) * 4 + 3) as usize];
                                    a != 0 && a != 255
                                })
                            });
                            assert_eq!(
                                stats.any_partial(x0, y0, x1, y1),
                                want,
                                "{x0},{y0}..{x1},{y1}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn binary_alpha_has_no_partial_texel_and_one_texel_is_enough() {
        let mut rgba = [0, 0, 0, 255, 0, 0, 0, 0].repeat(8);
        let stats = AlphaStats::of(4, 4, &rgba);
        assert!(!stats.any_partial(0, 0, 4, 4));
        rgba[(2 * 4 + 1) * 4 + 3] = 1;
        let stats = AlphaStats::of(4, 4, &rgba);
        assert!(stats.any_partial(0, 0, 4, 4));
        assert!(stats.any_partial(1, 2, 2, 3));
        assert!(!stats.any_partial(0, 0, 4, 2));
        assert!(!stats.any_partial(2, 2, 4, 4));
    }

    #[test]
    fn rectangles_are_cut_to_the_image_and_empty_ones_hold_nothing() {
        let rgba = [9, 9, 9, 128].repeat(4);
        let stats = AlphaStats::of(2, 2, &rgba);
        assert!(stats.any_partial(1, 1, 100, 100));
        assert!(!stats.any_partial(2, 0, 100, 2));
        assert!(!stats.any_partial(1, 1, 1, 2));
        assert!(!stats.any_partial(1, 1, 0, 0));
    }

    #[test]
    fn a_table_that_is_not_built_answers_conservatively() {
        assert!(AlphaStats::default().any_partial(0, 0, 1, 1));
        // Data that does not match the size, and a size beyond the limits, build no table.
        assert!(AlphaStats::of(2, 2, &[0; 4]).any_partial(0, 0, 1, 1));
        assert!(AlphaStats::of(u32::MAX, 1, &[]).any_partial(0, 0, 1, 1));
    }
}
