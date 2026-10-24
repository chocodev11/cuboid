//! Deterministic rectangle packing for the atlases that end up in files or exports, and the
//! gutter fill that follows it.
//!
//! The original packs with the engine's built-in packer, whose layout cannot be reproduced; the
//! specifications ask for a deterministic packer and define parity by sampled pixels. This one
//! depends only on the sizes and their order, on the padding and on the maximum side.
//!
//! spec: tools 5.4, tools 5.2 (packing algorithm), interop 12.2, interop 16.7.7

use std::cmp::Reverse;

use etagere::{AtlasAllocator, size2};

use crate::contract::{ImageError, MAX_SIDE, RgbaBuf};

/// A packed rectangle in texels; `y` counts from the bottom row of the atlas.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PackedRect {
    /// Left column.
    pub x: u32,
    /// Lowest row.
    pub y: u32,
    /// Width in texels.
    pub w: u32,
    /// Height in texels.
    pub h: u32,
}

/// An atlas layout: its size and one rectangle per input size, in input order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Packing {
    /// Atlas width in texels.
    pub w: u32,
    /// Atlas height in texels.
    pub h: u32,
    /// The rectangles, `rects[i]` for `sizes[i]`.
    pub rects: Vec<PackedRect>,
}

impl Packing {
    /// Draws `images`, in the order given to [`pack`], into a transparent atlas.
    pub fn compose(&self, images: &[&RgbaBuf]) -> RgbaBuf {
        let mut atlas = RgbaBuf::new(self.w, self.h);
        for (rect, image) in self.rects.iter().zip(images) {
            atlas.blit(image, rect.x, rect.y);
        }
        atlas
    }
}

/// Packs rectangles of the given `(w, h)` sizes into one atlas of at most `max_side` texels
/// across.
///
/// Each rectangle claims `padding` extra texels above and to the right of itself, so
/// neighbours are at least `padding` apart. The atlas is the smallest power-of-two size (or the
/// maximum) that holds the shelf packing of the rectangles, tallest first. A rectangle without
/// texels takes no space.
///
/// spec: tools 5.2 (packing algorithm), interop 12.2
pub fn pack(sizes: &[(u32, u32)], padding: u32, max_side: u32) -> Result<Packing, ImageError> {
    let max_side = max_side.min(MAX_SIDE);
    let mut items: Vec<(usize, (u32, u32))> = sizes
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, (w, h))| w > 0 && h > 0)
        .collect();
    let padded = |w: u32, h: u32| (w.saturating_add(padding), h.saturating_add(padding));
    items.sort_by_key(|&(i, (w, h))| (Reverse(h), Reverse(w), i));
    let area: u64 = items
        .iter()
        .map(|&(_, (w, h))| {
            let (w, h) = padded(w, h);
            u64::from(w) * u64::from(h)
        })
        .sum();
    let widest = items
        .iter()
        .map(|&(_, (w, h))| padded(w, h).0)
        .max()
        .unwrap_or(0);
    let tallest = items
        .iter()
        .map(|&(_, (w, h))| padded(w, h).1)
        .max()
        .unwrap_or(0);

    let at_origin: Vec<PackedRect> = sizes
        .iter()
        .map(|&(w, h)| PackedRect { x: 0, y: 0, w, h })
        .collect();
    if items.is_empty() {
        return Ok(Packing {
            w: 0,
            h: 0,
            rects: at_origin,
        });
    }

    let sides: Vec<u32> = std::iter::successors(Some(1u32), |s| s.checked_mul(2))
        .take_while(|&s| s < max_side)
        .chain([max_side])
        .filter(|&s| s > 0)
        .collect();
    let mut candidates: Vec<(u32, u32)> = sides
        .iter()
        .flat_map(|&w| sides.iter().map(move |&h| (w, h)))
        .filter(|&(w, h)| w >= widest && h >= tallest && u64::from(w) * u64::from(h) >= area)
        .collect();
    candidates.sort_by_key(|&(w, h)| (u64::from(w) * u64::from(h), w.max(h), Reverse(w)));

    for (w, h) in candidates {
        let mut atlas = AtlasAllocator::new(size2(w as i32, h as i32));
        let mut rects = at_origin.clone();
        let placed = items.iter().all(|&(i, (rw, rh))| {
            let (pw, ph) = padded(rw, rh);
            let Some(spot) = atlas.allocate(size2(pw as i32, ph as i32)) else {
                return false;
            };
            let at = spot.rectangle.min;
            rects.get_mut(i).is_some_and(|rect| {
                rect.x = at.x.unsigned_abs();
                rect.y = at.y.unsigned_abs();
                true
            })
        });
        if placed {
            return Ok(Packing { w, h, rects });
        }
    }
    Err(ImageError::DoesNotFit(max_side))
}

/// The gutter fill after packing: every fully transparent texel outside all `rects` takes the
/// colour of the first neighbour with alpha above 0, tried left, right, below, above in the
/// flattened bottom-up texel array.
///
/// The quirks of the original are kept: left and right are neighbours in the flat array, so
/// they wrap across row ends; texel 0 is never a source; and one pass reads the unmodified
/// atlas, so only a one-texel ring is filled.
///
/// spec: tools 5.4 (step 3), interop 16.7.7 (gutter fill)
pub fn bleed(atlas: &mut RgbaBuf, rects: &[PackedRect]) {
    let (w, h) = (atlas.w as usize, atlas.h as usize);
    if w == 0 || h == 0 {
        return;
    }
    let mut inside = vec![false; w * h];
    for rect in rects {
        let rows = inside
            .chunks_exact_mut(w)
            .skip(rect.y as usize)
            .take(rect.h as usize);
        for row in rows {
            let x = rect.x as usize;
            if let Some(span) = row.get_mut(x..x.saturating_add(rect.w as usize).min(w)) {
                span.fill(true);
            }
        }
    }
    let source = atlas.pixels.clone();
    let source = source.as_chunks::<4>().0;
    let count = w * h;
    let texels = atlas.pixels.as_chunks_mut::<4>().0.iter_mut();
    for (i, (texel, &inside)) in texels.zip(&inside).enumerate() {
        if inside || texel[3] != 0 {
            continue;
        }
        let neighbours = [
            i.checked_sub(1).filter(|&j| j > 0),
            (i + 1 < count).then_some(i + 1),
            i.checked_sub(w).filter(|&j| j > 0),
            (i + w < count).then_some(i + w),
        ];
        let found = neighbours
            .into_iter()
            .flatten()
            .filter_map(|j| source.get(j))
            .find(|n| n[3] > 0);
        if let Some(&colour) = found {
            *texel = colour;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuboid_num::{EngineRng, trunc_i32};

    fn random_sizes(count: usize, seed: u64) -> Vec<(u32, u32)> {
        let mut rng = EngineRng::seeded(seed);
        let mut side = || u32::try_from(trunc_i32(rng.range_f32(1.0, 24.0))).unwrap();
        (0..count).map(|_| (side(), side())).collect()
    }

    #[test]
    fn rectangles_land_inside_the_atlas_apart_by_the_padding() {
        for padding in [0, 1, 2] {
            let sizes = random_sizes(60, 3 + u64::from(padding));
            let packing = pack(&sizes, padding, 1024).unwrap();
            assert!(packing.w.is_power_of_two() && packing.h.is_power_of_two());
            assert_eq!(packing.rects.len(), sizes.len());
            for (rect, &(w, h)) in packing.rects.iter().zip(&sizes) {
                assert_eq!((rect.w, rect.h), (w, h));
                assert!(rect.x + w <= packing.w && rect.y + h <= packing.h);
            }
            for (i, a) in packing.rects.iter().enumerate() {
                for b in &packing.rects[i + 1..] {
                    let apart_x = a.x + a.w + padding <= b.x || b.x + b.w + padding <= a.x;
                    let apart_y = a.y + a.h + padding <= b.y || b.y + b.h + padding <= a.y;
                    assert!(apart_x || apart_y, "{a:?} and {b:?}");
                }
            }
        }
    }

    #[test]
    fn the_layout_depends_only_on_the_input() {
        let sizes = random_sizes(40, 9);
        assert_eq!(pack(&sizes, 2, 512).unwrap(), pack(&sizes, 2, 512).unwrap());
        // The order of the input is the order of the rectangles, whatever the packing order.
        let mut reversed = sizes.clone();
        reversed.reverse();
        let back = pack(&reversed, 2, 512).unwrap();
        assert_eq!(back.rects[0].w, sizes[sizes.len() - 1].0);
    }

    #[test]
    fn the_atlas_is_the_smallest_power_of_two_that_fits() {
        assert_eq!(pack(&[(16, 16)], 0, 1024).unwrap().w, 16);
        let four = pack(&[(16, 16); 4], 0, 1024).unwrap();
        assert_eq!((four.w, four.h), (32, 32));
        let wide = pack(&[(40, 8)], 0, 1024).unwrap();
        assert_eq!((wide.w, wide.h), (64, 8));
        // Padding counts: two 16 x 16 rectangles with a gutter no longer fit in 32 x 16.
        let padded = pack(&[(16, 16); 2], 1, 1024).unwrap();
        assert!(padded.w * padded.h > 32 * 16);
    }

    #[test]
    fn rectangles_that_do_not_fit_are_an_error() {
        assert!(matches!(
            pack(&[(100, 100)], 0, 64),
            Err(ImageError::DoesNotFit(64))
        ));
        assert!(matches!(
            pack(&[(40, 40); 5], 0, 64),
            Err(ImageError::DoesNotFit(64))
        ));
        assert!(matches!(
            pack(&[(1, 1)], 0, 0),
            Err(ImageError::DoesNotFit(0))
        ));
    }

    #[test]
    fn empty_rectangles_take_no_space() {
        let packing = pack(&[(0, 5), (8, 8), (3, 0)], 0, 64).unwrap();
        assert_eq!((packing.w, packing.h), (8, 8));
        assert_eq!(
            packing.rects[0],
            PackedRect {
                x: 0,
                y: 0,
                w: 0,
                h: 5
            }
        );
        assert_eq!((packing.rects[1].x, packing.rects[1].y), (0, 0));
        let none = pack(&[], 2, 64).unwrap();
        assert_eq!((none.w, none.h, none.rects.len()), (0, 0, 0));
    }

    #[test]
    fn composing_puts_each_image_in_its_rectangle() {
        let sizes = [(3, 2), (2, 4), (5, 1)];
        let images: Vec<RgbaBuf> = sizes
            .iter()
            .zip(1u8..)
            .map(|(&(w, h), n)| RgbaBuf {
                w,
                h,
                pixels: [n * 10, 0, 0, 255].repeat((w * h) as usize),
            })
            .collect();
        let packing = pack(&sizes, 1, 64).unwrap();
        let refs: Vec<&RgbaBuf> = images.iter().collect();
        let atlas = packing.compose(&refs);
        assert_eq!((atlas.w, atlas.h), (packing.w, packing.h));
        let mut covered = 0;
        for (rect, n) in packing.rects.iter().zip(1u8..) {
            for y in rect.y..rect.y + rect.h {
                for x in rect.x..rect.x + rect.w {
                    assert_eq!(atlas.get(x as i32, y as i32), Some([n * 10, 0, 0, 255]));
                    covered += 1;
                }
            }
        }
        let opaque = atlas
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|t| t[3] == 255)
            .count();
        assert_eq!(opaque, covered);
    }

    fn atlas_with(w: u32, h: u32, texels: &[(i32, i32)]) -> RgbaBuf {
        let mut atlas = RgbaBuf::new(w, h);
        for &(x, y) in texels {
            atlas.set(x, y, [9, 8, 7, 255]);
        }
        atlas
    }

    fn filled(atlas: &RgbaBuf) -> Vec<(i32, i32)> {
        (0..atlas.h as i32)
            .flat_map(|y| (0..atlas.w as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| atlas.get(x, y).is_some_and(|t| t[3] > 0))
            .collect()
    }

    #[test]
    fn bleed_fills_a_one_texel_ring_and_wraps_across_row_ends() {
        // A single opaque texel in the last column of a 5 x 2 atlas.
        let mut atlas = atlas_with(5, 2, &[(4, 0)]);
        bleed(
            &mut atlas,
            &[PackedRect {
                x: 4,
                y: 0,
                w: 1,
                h: 1,
            }],
        );
        // Left neighbour (3, 0) finds it on its right; (4, 1) finds it below; (0, 1) finds it
        // on its left because texel 4 precedes texel 5 in the flat array. Nothing is two
        // texels away.
        assert_eq!(filled(&atlas), [(3, 0), (4, 0), (0, 1), (4, 1)]);
        assert_eq!(atlas.get(3, 0), Some([9, 8, 7, 255]));
    }

    #[test]
    fn texel_0_is_never_a_source() {
        let mut atlas = atlas_with(5, 2, &[(0, 0)]);
        bleed(
            &mut atlas,
            &[PackedRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            }],
        );
        assert_eq!(filled(&atlas), [(0, 0)]);
    }

    #[test]
    fn bleed_leaves_texels_inside_rectangles_and_opaque_texels_alone() {
        let mut atlas = atlas_with(4, 2, &[(1, 0)]);
        // The transparent texel (2, 0) lies inside a rectangle, so it stays transparent although
        // its left neighbour has alpha; (0, 0) and (1, 1) lie outside and are filled.
        bleed(
            &mut atlas,
            &[PackedRect {
                x: 1,
                y: 0,
                w: 2,
                h: 1,
            }],
        );
        assert_eq!(filled(&atlas), [(0, 0), (1, 0), (1, 1)]);
    }
}
