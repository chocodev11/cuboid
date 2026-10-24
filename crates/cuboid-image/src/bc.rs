//! The block formats BC1 (DXT1) and BC3 (DXT5): 4x4 texels per block, 8 and 16 bytes.
//!
//! Stored block rows run bottom to top, and so do the four texel rows inside a block: the texel
//! at row `r` of block `(bx, by)` is image row `4 * by + r` of the bottom-up buffer. A standard
//! decoder that fills a top-first buffer from the same bytes would produce the same rows, which
//! is the "DDS bottom-up rule": the file's blocks are not flipped, so a DDS shows upside down
//! relative to its own convention, as in the original.
//!
//! spec: native 4.5.1

use crate::format::{low, quant, widen};

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum Bc {
    /// DXT1, 8 bytes per block: a colour block with optional punch-through alpha.
    Bc1,
    /// DXT5, 16 bytes per block: an interpolated alpha block, then a colour block.
    Bc3,
}

/// The 16 texels of a block, row 0 first.
type Block = [[u8; 4]; 16];

fn pick<T: Copy + Default, const N: usize>(table: &[T; N], index: u8) -> T {
    table.get(usize::from(index)).copied().unwrap_or_default()
}

/// Expands a 565 colour to RGB.
fn rgb565(c: u16) -> [u8; 3] {
    let c = u32::from(c);
    [widen(c >> 11, 5), widen(c >> 5, 6), widen(c, 5)]
}

/// The four colours of a colour block. In the three-colour mode index 3 is transparent black.
///
/// The interpolation truncates (`(2a + b) / 3`); the original's rounding is **UNVERIFIED**.
fn palette(c0: u16, c1: u16, four_colours: bool) -> [[u8; 4]; 4] {
    let ([ar, ag, ab], [br, bg, bb]) = (rgb565(c0), rgb565(c1));
    let mix = |wa: u32, wb: u32, div: u32| {
        let channel = |x: u8, y: u8| low((wa * u32::from(x) + wb * u32::from(y)) / div);
        [channel(ar, br), channel(ag, bg), channel(ab, bb), 255]
    };
    let (a, b) = ([ar, ag, ab, 255], [br, bg, bb, 255]);
    if four_colours {
        [a, b, mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [a, b, mix(1, 1, 2), [0, 0, 0, 0]]
    }
}

/// The eight alpha levels of a BC3 alpha block: eight-step interpolation when `a0 > a1`, else
/// six steps plus 0 and 255.
fn alpha_palette(a0: u8, a1: u8) -> [u8; 8] {
    let (x, y) = (u32::from(a0), u32::from(a1));
    let lerp = |i: u32, steps: u32| low(((steps + 1 - i) * x + (i - 1) * y) / steps);
    if a0 > a1 {
        [
            a0,
            a1,
            lerp(2, 7),
            lerp(3, 7),
            lerp(4, 7),
            lerp(5, 7),
            lerp(6, 7),
            lerp(7, 7),
        ]
    } else {
        [
            a0,
            a1,
            lerp(2, 5),
            lerp(3, 5),
            lerp(4, 5),
            lerp(5, 5),
            0,
            255,
        ]
    }
}

/// Decodes a colour block. `punch_through` selects the DXT1 rule: `c0 <= c1` means three colours
/// and a transparent index 3. In a BC3 block the colours always interpolate with four steps.
fn colour_texels([a0, a1, b0, b1, i0, i1, i2, i3]: [u8; 8], punch_through: bool) -> Block {
    let (c0, c1) = (u16::from_le_bytes([a0, a1]), u16::from_le_bytes([b0, b1]));
    let palette = palette(c0, c1, !punch_through || c0 > c1);
    let indices = u32::from_le_bytes([i0, i1, i2, i3]);
    std::array::from_fn(|n| pick(&palette, low((indices >> (2 * n)) & 3)))
}

/// Decodes the alpha of a BC3 block, texel row 0 first.
fn alpha_texels([a0, a1, i0, i1, i2, i3, i4, i5]: [u8; 8]) -> [u8; 16] {
    let palette = alpha_palette(a0, a1);
    let indices = u64::from_le_bytes([i0, i1, i2, i3, i4, i5, 0, 0]);
    std::array::from_fn(|n| {
        let [index, ..] = ((indices >> (3 * n)) & 7).to_le_bytes();
        pick(&palette, index)
    })
}

/// Copies the texels of block `n` (row-major over the block grid) into the RGBA buffer, leaving
/// out those beyond the image edge.
fn store(pixels: &mut [u8], w: usize, h: usize, n: usize, block: &Block) {
    let blocks_x = w.div_ceil(4);
    let (x0, y0) = (n % blocks_x * 4, n / blocks_x * 4);
    for (i, texel) in block.iter().enumerate() {
        let (x, y) = (x0 + i % 4, y0 + i / 4);
        if x >= w || y >= h {
            continue;
        }
        let at = (y * w + x) * 4;
        if let Some(dst) = pixels.get_mut(at..).and_then(|s| s.first_chunk_mut()) {
            *dst = *texel;
        }
    }
}

/// Decodes `data`, whole blocks from the lower-left corner, into `pixels` (`w * h * 4` bytes).
pub(crate) fn decode(bc: Bc, w: usize, h: usize, data: &[u8], pixels: &mut [u8]) {
    let halves = data.as_chunks::<8>().0;
    match bc {
        Bc::Bc1 => {
            for (n, &block) in halves.iter().enumerate() {
                store(pixels, w, h, n, &colour_texels(block, true));
            }
        }
        Bc::Bc3 => {
            for (n, &[alpha, colour]) in halves.as_chunks::<2>().0.iter().enumerate() {
                let mut texels = colour_texels(colour, false);
                for (texel, a) in texels.iter_mut().zip(alpha_texels(alpha)) {
                    texel[3] = a;
                }
                store(pixels, w, h, n, &texels);
            }
        }
    }
}

fn dist(a: [u8; 4], b: [u8; 4]) -> u32 {
    let d = |x: u8, y: u8| u32::from(x.abs_diff(y)).pow(2);
    let ([ar, ag, ab, _], [br, bg, bb, _]) = (a, b);
    d(ar, br) + d(ag, bg) + d(ab, bb)
}

/// The colours at both ends of the principal axis of `colours` (found by power iteration, no
/// square roots), highest projection first. `None` for no colours.
fn axis_ends(colours: impl Iterator<Item = [u8; 3]> + Clone) -> Option<([u8; 3], [u8; 3])> {
    let count = colours.clone().count();
    if count == 0 {
        return None;
    }
    let float = |c: [u8; 3]| c.map(f32::from);
    let mut mean = [0.0f32; 3];
    for c in colours.clone() {
        for (m, v) in mean.iter_mut().zip(float(c)) {
            *m += v;
        }
    }
    mean = mean.map(|m| m / count as f32);
    let mut cov = [[0.0f32; 3]; 3];
    for c in colours.clone() {
        let mut d = float(c);
        for (d, m) in d.iter_mut().zip(mean) {
            *d -= m;
        }
        for (row, di) in cov.iter_mut().zip(d) {
            for (v, dj) in row.iter_mut().zip(d) {
                *v += di * dj;
            }
        }
    }
    // Start from the heaviest column of the covariance: a fixed start vector can be orthogonal to
    // the principal axis (red against blue, for one).
    let weight = |row: &[f32; 3]| row.iter().map(|v| v.abs()).sum::<f32>();
    let mut axis = cov
        .iter()
        .copied()
        .max_by(|a, b| weight(a).total_cmp(&weight(b)))
        .unwrap_or([1.0; 3]);
    for _ in 0..4 {
        let next = cov.map(|row| row.iter().zip(axis).map(|(a, b)| a * b).sum::<f32>());
        let top = next.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if top <= 0.0 {
            break;
        }
        axis = next.map(|v| v / top);
    }
    let project = |c: [u8; 3]| -> f32 {
        let c = float(c);
        c.iter()
            .zip(mean)
            .zip(axis)
            .map(|((c, m), a)| (c - m) * a)
            .sum()
    };
    let (mut lo, mut hi) = (None::<([u8; 3], f32)>, None::<([u8; 3], f32)>);
    for c in colours {
        let p = project(c);
        if lo.is_none_or(|(_, q)| p < q) {
            lo = Some((c, p));
        }
        if hi.is_none_or(|(_, q)| p > q) {
            hi = Some((c, p));
        }
    }
    Some((hi?.0, lo?.0))
}

fn to_565([r, g, b]: [u8; 3]) -> u16 {
    let [lo, hi, ..] = ((quant(r, 5) << 11) | (quant(g, 6) << 5) | quant(b, 5)).to_le_bytes();
    u16::from_le_bytes([lo, hi])
}

/// Fits a colour block to the texels. With `punch_through` the texels with alpha below 128 are
/// transparent (index 3 of a three-colour block) and do not take part in the fit. Endpoints are
/// the extremes of the principal axis, so a block of at most two colours that are exact 565
/// levels is reproduced exactly.
fn encode_colours(texels: &Block, punch_through: bool) -> [u8; 8] {
    let transparent = |t: &[u8; 4]| punch_through && t[3] < 128;
    let live = texels
        .iter()
        .filter(|t| !transparent(t))
        .map(|&[r, g, b, _]| [r, g, b]);
    let Some((hi, lo)) = axis_ends(live) else {
        return [0, 0, 0, 0, 255, 255, 255, 255];
    };
    let (mut c0, mut c1) = (to_565(hi), to_565(lo));
    // Four colours need c0 > c1, three colours c0 <= c1.
    if (punch_through && c0 > c1) || (!punch_through && c0 < c1) {
        (c0, c1) = (c1, c0);
    }
    let four = c0 > c1;
    let palette = palette(c0, c1, four);
    let usable = if four { 4 } else { 3 };
    let mut bits = 0u32;
    for (n, texel) in texels.iter().enumerate() {
        let index = if transparent(texel) {
            3
        } else {
            (0u8..)
                .zip(palette.iter().take(usable))
                .min_by_key(|&(_, &p)| dist(p, *texel))
                .map_or(0, |(i, _)| i)
        };
        bits |= u32::from(index) << (2 * n);
    }
    let [a0, a1] = c0.to_le_bytes();
    let [b0, b1] = c1.to_le_bytes();
    let [i0, i1, i2, i3] = bits.to_le_bytes();
    [a0, a1, b0, b1, i0, i1, i2, i3]
}

/// Fits a BC3 alpha block: the extremes as endpoints with eight steps between them; a block of
/// one alpha uses the six-step form with index 0 everywhere.
fn encode_alpha(texels: &Block) -> [u8; 8] {
    let (lo, hi) = texels
        .iter()
        .fold((255u8, 0u8), |(lo, hi), t| (lo.min(t[3]), hi.max(t[3])));
    let palette = alpha_palette(hi, lo);
    let mut bits = 0u64;
    for (n, texel) in texels.iter().enumerate() {
        let index = (0u8..)
            .zip(palette)
            .min_by_key(|&(_, p)| p.abs_diff(texel[3]))
            .map_or(0, |(i, _)| i);
        bits |= u64::from(index) << (3 * n);
    }
    let [i0, i1, i2, i3, i4, i5, ..] = bits.to_le_bytes();
    [hi, lo, i0, i1, i2, i3, i4, i5]
}

/// Compresses `rgba` (`w * h` texels), whole blocks from the lower-left corner; blocks that stick
/// out past the edge repeat the edge texels.
pub(crate) fn encode(bc: Bc, w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let block_len = if bc == Bc::Bc1 { 8 } else { 16 };
    let mut out = Vec::with_capacity(w.div_ceil(4) * h.div_ceil(4) * block_len);
    for by in 0..h.div_ceil(4) {
        for bx in 0..w.div_ceil(4) {
            let texels: Block = std::array::from_fn(|n| {
                let x = (bx * 4 + n % 4).min(w - 1);
                let y = (by * 4 + n / 4).min(h - 1);
                rgba.get((y * w + x) * 4..)
                    .and_then(|s| s.first_chunk())
                    .copied()
                    .unwrap_or_default()
            });
            if bc == Bc::Bc3 {
                out.extend_from_slice(&encode_alpha(&texels));
            }
            let punch_through = bc == Bc::Bc1 && texels.iter().any(|t| t[3] < 128);
            out.extend_from_slice(&encode_colours(&texels, punch_through));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::RgbaBuf;

    fn enc(bc: Bc, buf: &RgbaBuf) -> Vec<u8> {
        encode(bc, buf.w as usize, buf.h as usize, &buf.pixels)
    }

    fn decode_all(bc: Bc, w: usize, h: usize, data: &[u8]) -> Vec<u8> {
        let mut pixels = vec![0u8; w * h * 4];
        decode(bc, w, h, data, &mut pixels);
        pixels
    }

    fn texel(pixels: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let at = (y * w + x) * 4;
        pixels[at..at + 4].try_into().unwrap()
    }

    /// Endpoints red (0xF800) and blue (0x001F), indices 0, 1, 2, 3 across each row.
    const BC1_RED_BLUE: [u8; 8] = [0x00, 0xF8, 0x1F, 0x00, 0xE4, 0xE4, 0xE4, 0xE4];

    #[test]
    fn bc1_four_colour_block_interpolates_by_thirds() {
        let pixels = decode_all(Bc::Bc1, 4, 4, &BC1_RED_BLUE);
        // c0 > c1: red, blue, (2 red + blue) / 3, (red + 2 blue) / 3, truncated.
        let row = [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [170, 0, 85, 255],
            [85, 0, 170, 255],
        ];
        for y in 0..4 {
            for (x, want) in row.iter().enumerate() {
                assert_eq!(&texel(&pixels, 4, x, y), want, "texel {x},{y}");
            }
        }
    }

    #[test]
    fn bc1_three_colour_block_has_a_transparent_index_3() {
        // c0 = black <= c1 = white: black, white, mid grey (255 / 2 = 127), transparent.
        let block = [0x00, 0x00, 0xFF, 0xFF, 0xE4, 0xE4, 0xE4, 0xE4];
        let pixels = decode_all(Bc::Bc1, 4, 4, &block);
        assert_eq!(texel(&pixels, 4, 0, 0), [0, 0, 0, 255]);
        assert_eq!(texel(&pixels, 4, 1, 0), [255, 255, 255, 255]);
        assert_eq!(texel(&pixels, 4, 2, 0), [127, 127, 127, 255]);
        assert_eq!(texel(&pixels, 4, 3, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn bc3_colour_block_always_uses_four_colours() {
        // The same endpoints in the "wrong" order still give the one-third and two-thirds colours.
        let colour = [0x1F, 0x00, 0x00, 0xF8, 0xE4, 0xE4, 0xE4, 0xE4];
        let mut data = vec![255, 255, 0, 0, 0, 0, 0, 0];
        data.extend_from_slice(&colour);
        let pixels = decode_all(Bc::Bc3, 4, 4, &data);
        assert_eq!(texel(&pixels, 4, 2, 0), [85, 0, 170, 255]);
        assert_eq!(texel(&pixels, 4, 3, 0), [170, 0, 85, 255]);
    }

    #[test]
    fn bc3_alpha_levels_for_both_modes() {
        // a0 = 255 > a1 = 0: eight levels, a_i = ((8 - i) * a0 + (i - 1) * a1) / 7.
        assert_eq!(alpha_palette(255, 0), [255, 0, 218, 182, 145, 109, 72, 36]);
        // a0 = 0 <= a1 = 255: six levels, a_i = ((6 - i) * a0 + (i - 1) * a1) / 5, then 0 and 255.
        assert_eq!(alpha_palette(0, 255), [0, 255, 51, 102, 153, 204, 0, 255]);
        assert_eq!(alpha_palette(7, 7), [7, 7, 7, 7, 7, 7, 0, 255]);
    }

    #[test]
    fn bc3_alpha_indices_are_three_bits_in_little_endian_order() {
        // Texel n gets index n % 8: bits 0..48 hold 000 001 010 011 100 101 110 111 twice.
        let mut bits = 0u64;
        for n in 0..16u64 {
            bits |= (n % 8) << (3 * n);
        }
        let [i0, i1, i2, i3, i4, i5, ..] = bits.to_le_bytes();
        let alpha = alpha_texels([255, 0, i0, i1, i2, i3, i4, i5]);
        let levels = alpha_palette(255, 0);
        for (n, a) in alpha.iter().enumerate() {
            assert_eq!(*a, levels[n % 8], "texel {n}");
        }
    }

    /// The bottom-up rule: the first stored block row is the bottom of the image, and inside a
    /// block the first texel row is the lowest.
    #[test]
    fn blocks_and_texel_rows_run_bottom_to_top() {
        let solid = |c: u16| {
            let [lo, hi] = c.to_le_bytes();
            [lo, hi, lo, hi, 0, 0, 0, 0]
        };
        // 4 x 8: first block red, second block blue.
        let mut data = solid(0xF800).to_vec();
        data.extend_from_slice(&solid(0x001F));
        let pixels = decode_all(Bc::Bc1, 4, 8, &data);
        assert_eq!(texel(&pixels, 4, 0, 0), [255, 0, 0, 255]);
        assert_eq!(texel(&pixels, 4, 3, 3), [255, 0, 0, 255]);
        assert_eq!(texel(&pixels, 4, 0, 4), [0, 0, 255, 255]);
        assert_eq!(texel(&pixels, 4, 3, 7), [0, 0, 255, 255]);
        // Inside a block: row 0 uses index 0 (red), row 1 index 1 (blue), row 2 index 2, row 3 index 3.
        let rows = [0x00, 0x55, 0xAA, 0xFF];
        let block = [0x00, 0xF8, 0x1F, 0x00, rows[0], rows[1], rows[2], rows[3]];
        let pixels = decode_all(Bc::Bc1, 4, 4, &block);
        for (y, want) in [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [170, 0, 85, 255],
            [85, 0, 170, 255],
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(&texel(&pixels, 4, 2, y), want, "row {y}");
        }
    }

    #[test]
    fn blocks_beyond_the_image_edge_are_cut() {
        // 5 x 3 needs 2 x 1 blocks; the texels at x >= 5 or y >= 3 are not stored anywhere.
        let mut data = BC1_RED_BLUE.to_vec();
        data.extend_from_slice(&BC1_RED_BLUE);
        let pixels = decode_all(Bc::Bc1, 5, 3, &data);
        assert_eq!(pixels.len(), 5 * 3 * 4);
        assert_eq!(texel(&pixels, 5, 4, 2), [255, 0, 0, 255]);
        assert_eq!(texel(&pixels, 5, 3, 2), [85, 0, 170, 255]);
    }

    fn gradient(w: u32, h: u32) -> RgbaBuf {
        let mut buf = RgbaBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = u8::try_from(x * 16).unwrap();
                buf.set(
                    i32::try_from(x).unwrap(),
                    i32::try_from(y).unwrap(),
                    [v, 255 - v, 128, 255 - v / 2],
                );
            }
        }
        buf
    }

    #[test]
    fn solid_and_two_colour_blocks_survive_exactly() {
        // Two exact 565 levels: red and blue, and white (all channels at full scale).
        let mut buf = RgbaBuf::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                let c = if (x + y) % 2 == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                buf.set(x, y, c);
            }
        }
        let raw = enc(Bc::Bc1, &buf);
        assert_eq!(decode_all(Bc::Bc1, 4, 4, &raw), buf.pixels);

        let solid = RgbaBuf {
            w: 4,
            h: 4,
            pixels: [255, 255, 255, 255].repeat(16),
        };
        assert_eq!(
            decode_all(Bc::Bc1, 4, 4, &enc(Bc::Bc1, &solid)),
            solid.pixels
        );
        assert_eq!(
            decode_all(Bc::Bc3, 4, 4, &enc(Bc::Bc3, &solid)),
            solid.pixels
        );
    }

    #[test]
    fn a_gradient_stays_within_the_quantisation_error() {
        let buf = gradient(16, 8);
        for bc in [Bc::Bc1, Bc::Bc3] {
            let raw = enc(bc, &buf);
            let back = decode_all(bc, 16, 8, &raw);
            let worst = buf
                .pixels
                .chunks(4)
                .zip(back.chunks(4))
                .flat_map(|(a, b)| a.iter().zip(b).take(if bc == Bc::Bc1 { 3 } else { 4 }))
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(worst <= 12, "worst error {worst}");
        }
    }

    #[test]
    fn punch_through_blocks_keep_transparent_texels_transparent() {
        let mut buf = gradient(4, 4);
        for x in 0..4 {
            buf.set(x, 1, [200, 100, 50, 0]);
        }
        let back = decode_all(Bc::Bc1, 4, 4, &enc(Bc::Bc1, &buf));
        for x in 0..4 {
            assert_eq!(texel(&back, 4, x, 1), [0, 0, 0, 0]);
            assert_eq!(texel(&back, 4, x, 0)[3], 255);
        }
        let empty = RgbaBuf {
            w: 4,
            h: 4,
            pixels: [9, 9, 9, 0].repeat(16),
        };
        let back = decode_all(Bc::Bc1, 4, 4, &enc(Bc::Bc1, &empty));
        assert!(back.chunks(4).all(|t| t == [0, 0, 0, 0]));
    }

    #[test]
    fn bc3_alpha_endpoints_are_exact() {
        let mut buf = RgbaBuf::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                buf.set(x, y, [10, 20, 30, if (x + y) % 2 == 0 { 0 } else { 255 }]);
            }
        }
        let back = decode_all(Bc::Bc3, 4, 4, &enc(Bc::Bc3, &buf));
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(texel(&back, 4, x, y)[3], texel(&buf.pixels, 4, x, y)[3]);
            }
        }
    }

    #[test]
    fn partial_blocks_encode_to_whole_blocks() {
        let buf = gradient(5, 3);
        assert_eq!(enc(Bc::Bc1, &buf).len(), 2 * 8);
        assert_eq!(enc(Bc::Bc3, &buf).len(), 2 * 16);
        assert!(encode(Bc::Bc1, 0, 5, &[]).is_empty());
    }
}
