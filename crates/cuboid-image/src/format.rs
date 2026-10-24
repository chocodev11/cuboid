//! Stored texture formats (`RawTexture.f`): their codes, sizes, decoding to RGBA8 and
//! re-encoding from it.
//!
//! The codes are the engine's texture format ids. Other codes exist; a reader keeps their bytes
//! and substitutes a placeholder image ([`crate::Image::rgba`]).
//!
//! spec: native 4.5.1

use crate::bc::{self, Bc};
use crate::contract::{ImageError, RgbaBuf};

/// One byte per pixel: A. Decoded as white with that alpha (**UNVERIFIED**: the original's
/// colour for the missing channels).
pub const ALPHA8: i32 = 1;
/// 16-bit little-endian word: A bits 12-15, R 8-11, G 4-7, B 0-3 (**UNVERIFIED**).
pub const ARGB4444: i32 = 2;
/// Three bytes per pixel: R, G, B.
pub const RGB24: i32 = 3;
/// Four bytes per pixel: R, G, B, A.
pub const RGBA32: i32 = 4;
/// Four bytes per pixel: A, R, G, B.
pub const ARGB32: i32 = 5;
/// 16-bit little-endian word: R bits 11-15, G 5-10, B 0-4.
pub const RGB565: i32 = 7;
/// DXT1 (BC1): 8 bytes per 4x4 block, block rows bottom to top.
pub const DXT1: i32 = 10;
/// DXT5 (BC3): 16 bytes per 4x4 block, block rows bottom to top.
pub const DXT5: i32 = 12;
/// 16-bit little-endian word: R bits 12-15, G 8-11, B 4-7, A 0-3 (**UNVERIFIED**).
pub const RGBA4444: i32 = 13;
/// Four bytes per pixel: B, G, R, A.
pub const BGRA32: i32 = 14;

/// The low byte of `v`; callers keep `v` below 256.
pub(crate) fn low(v: u32) -> u8 {
    let [byte, ..] = v.to_le_bytes();
    byte
}

/// The two bytes of a little-endian 16-bit word.
fn le16(v: u32) -> [u8; 2] {
    let [lo, hi, ..] = v.to_le_bytes();
    [lo, hi]
}

fn word(bytes: [u8; 2]) -> u32 {
    u32::from(u16::from_le_bytes(bytes))
}

/// The low `bits` bits of `v` widened to 8 bits by repeating the high bits (5 bits: `v << 3 |
/// v >> 2`), so that full scale stays 255.
pub(crate) fn widen(v: u32, bits: u32) -> u8 {
    let v = v & ((1 << bits) - 1);
    low((v << (8 - bits)) | (v >> (2 * bits - 8)))
}

/// An 8-bit channel reduced to `bits` bits, to the nearest level.
pub(crate) fn quant(v: u8, bits: u32) -> u32 {
    (u32::from(v) * ((1 << bits) - 1) + 127) / 255
}

/// Bytes of level 0 of a `w` x `h` image in format `code`.
///
/// Fails for an unknown code and for sizes whose byte count overflows.
pub fn level_len(code: i32, w: u32, h: u32) -> Result<usize, ImageError> {
    let (wide, high) = (w as usize, h as usize);
    let len = match code {
        ALPHA8 => texels(w, h).ok(),
        ARGB4444 | RGB565 | RGBA4444 => texels(w, h).ok().and_then(|n| n.checked_mul(2)),
        RGB24 => texels(w, h).ok().and_then(|n| n.checked_mul(3)),
        RGBA32 | ARGB32 | BGRA32 => texels(w, h).ok().and_then(|n| n.checked_mul(4)),
        DXT1 | DXT5 => {
            let block = if code == DXT1 { 8 } else { 16 };
            let blocks = wide.div_ceil(4).checked_mul(high.div_ceil(4));
            blocks.and_then(|n| n.checked_mul(block))
        }
        _ => return Err(ImageError::UnknownFormat(code)),
    };
    len.ok_or(ImageError::TooLarge { w, h })
}

fn texels(w: u32, h: u32) -> Result<usize, ImageError> {
    (w as usize)
        .checked_mul(h as usize)
        .ok_or(ImageError::TooLarge { w, h })
}

/// Writes the RGBA texel `f` makes of every `N`-byte source texel.
fn map_texels<const N: usize>(data: &[u8], pixels: &mut [u8], f: impl Fn([u8; N]) -> [u8; 4]) {
    let dst = pixels.as_chunks_mut::<4>().0.iter_mut();
    for (dst, &src) in dst.zip(data.as_chunks::<N>().0) {
        *dst = f(src);
    }
}

/// Decodes level 0 of `raw` to RGBA8, rows bottom to top. Bytes behind level 0 (a mip chain)
/// are ignored.
///
/// Fails for an unknown code and for data shorter than level 0, the engine's "not enough data
/// provided".
///
/// spec: native 4.5.1
pub fn decode(code: i32, w: u32, h: u32, raw: &[u8]) -> Result<RgbaBuf, ImageError> {
    let needed = level_len(code, w, h)?;
    let data = raw.get(..needed).ok_or(ImageError::ShortData {
        needed,
        got: raw.len(),
    })?;
    let count = texels(w, h)?;
    let mut pixels = vec![0u8; count * 4];
    match code {
        ALPHA8 => map_texels(data, &mut pixels, |[a]| [255, 255, 255, a]),
        ARGB4444 => map_texels(data, &mut pixels, |bytes| {
            let v = word(bytes);
            [
                widen(v >> 8, 4),
                widen(v >> 4, 4),
                widen(v, 4),
                widen(v >> 12, 4),
            ]
        }),
        RGB24 => map_texels(data, &mut pixels, |[r, g, b]| [r, g, b, 255]),
        RGBA32 => pixels.copy_from_slice(data),
        ARGB32 => map_texels(data, &mut pixels, |[a, r, g, b]| [r, g, b, a]),
        RGB565 => map_texels(data, &mut pixels, |bytes| {
            let v = word(bytes);
            [widen(v >> 11, 5), widen(v >> 5, 6), widen(v, 5), 255]
        }),
        DXT1 => bc::decode(Bc::Bc1, w as usize, h as usize, data, &mut pixels),
        DXT5 => bc::decode(Bc::Bc3, w as usize, h as usize, data, &mut pixels),
        RGBA4444 => map_texels(data, &mut pixels, |bytes| {
            let v = word(bytes);
            [
                widen(v >> 12, 4),
                widen(v >> 8, 4),
                widen(v >> 4, 4),
                widen(v, 4),
            ]
        }),
        BGRA32 => map_texels(data, &mut pixels, |[b, g, r, a]| [r, g, b, a]),
        _ => return Err(ImageError::UnknownFormat(code)),
    }
    Ok(RgbaBuf { w, h, pixels })
}

/// The `N` bytes `f` makes of every texel of `rgba`.
fn collect_texels<const N: usize>(rgba: &[u8], f: impl Fn([u8; 4]) -> [u8; N]) -> Vec<u8> {
    let src = rgba.as_chunks::<4>().0;
    let mut out = vec![0u8; src.len() * N];
    for (dst, &texel) in out.as_chunks_mut::<N>().0.iter_mut().zip(src) {
        *dst = f(texel);
    }
    out
}

/// Encodes `rgba`, `w * h` texels with rows bottom to top, as level 0 of format `code` without
/// a mip chain: [`level_len`] bytes.
///
/// Formats without alpha drop it; narrow formats round each channel to the nearest level. The
/// block formats use a small deterministic compressor, so they are lossy; an image that was read
/// from DXT data keeps its bytes until it is edited ([`crate::Image::stored_bytes`]).
///
/// spec: native 4.5.1
pub fn encode(code: i32, w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, ImageError> {
    Ok(match code {
        ALPHA8 => collect_texels(rgba, |[_, _, _, a]| [a]),
        ARGB4444 => collect_texels(rgba, |[r, g, b, a]| {
            let (r, g, b, a) = (quant(r, 4), quant(g, 4), quant(b, 4), quant(a, 4));
            le16((a << 12) | (r << 8) | (g << 4) | b)
        }),
        RGB24 => collect_texels(rgba, |[r, g, b, _]| [r, g, b]),
        RGBA32 => rgba.to_vec(),
        ARGB32 => collect_texels(rgba, |[r, g, b, a]| [a, r, g, b]),
        RGB565 => collect_texels(rgba, |[r, g, b, _]| {
            le16((quant(r, 5) << 11) | (quant(g, 6) << 5) | quant(b, 5))
        }),
        DXT1 => bc::encode(Bc::Bc1, w as usize, h as usize, rgba),
        DXT5 => bc::encode(Bc::Bc3, w as usize, h as usize, rgba),
        RGBA4444 => collect_texels(rgba, |[r, g, b, a]| {
            let (r, g, b, a) = (quant(r, 4), quant(g, 4), quant(b, 4), quant(a, 4));
            le16((r << 12) | (g << 8) | (b << 4) | a)
        }),
        BGRA32 => collect_texels(rgba, |[r, g, b, a]| [b, g, r, a]),
        _ => return Err(ImageError::UnknownFormat(code)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuboid_num::{EngineRng, trunc_i32};

    const ALL: [i32; 10] = [
        ALPHA8, ARGB4444, RGB24, RGBA32, ARGB32, RGB565, DXT1, DXT5, RGBA4444, BGRA32,
    ];

    fn enc(code: i32, buf: &RgbaBuf) -> Result<Vec<u8>, ImageError> {
        encode(code, buf.w, buf.h, &buf.pixels)
    }

    fn buf(w: u32, h: u32, pixels: &[u8]) -> RgbaBuf {
        RgbaBuf {
            w,
            h,
            pixels: pixels.to_vec(),
        }
    }

    /// One texel per format with the byte order of the table in native 4.5.1, chosen so that
    /// every channel differs.
    #[test]
    fn every_format_reads_its_byte_order() {
        let cases: [(i32, &[u8], [u8; 4]); 8] = [
            (ALPHA8, &[0x80], [255, 255, 255, 0x80]),
            (RGB24, &[1, 2, 3], [1, 2, 3, 255]),
            (RGBA32, &[1, 2, 3, 4], [1, 2, 3, 4]),
            (ARGB32, &[4, 1, 2, 3], [1, 2, 3, 4]),
            (BGRA32, &[3, 2, 1, 4], [1, 2, 3, 4]),
            // Little-endian word 0xA5C3: A=10 R=5 G=12 B=3 for ARGB4444 (A bits 12-15).
            (ARGB4444, &[0xC3, 0xA5], [0x55, 0xCC, 0x33, 0xAA]),
            // The same word for RGBA4444: R=10 G=5 B=12 A=3.
            (RGBA4444, &[0xC3, 0xA5], [0xAA, 0x55, 0xCC, 0x33]),
            // Word 0xF81F: R=31 G=0 B=31.
            (RGB565, &[0x1F, 0xF8], [255, 0, 255, 255]),
        ];
        for (code, raw, want) in cases {
            let got = decode(code, 1, 1, raw).unwrap();
            assert_eq!(got.pixels, want, "format {code}");
        }
    }

    #[test]
    fn rgb565_keeps_green_at_six_bits() {
        // 0x07E0 is full green; 0x0020 is the lowest green level, 4 after widening.
        assert_eq!(
            decode(RGB565, 1, 1, &[0xE0, 0x07]).unwrap().pixels,
            [0, 255, 0, 255]
        );
        assert_eq!(
            decode(RGB565, 1, 1, &[0x20, 0x00]).unwrap().pixels,
            [0, 4, 0, 255]
        );
    }

    #[test]
    fn rows_stay_bottom_to_top() {
        let raw = [1, 1, 1, 255, 2, 2, 2, 255];
        let got = decode(RGBA32, 1, 2, &raw).unwrap();
        assert_eq!(got.get(0, 0), Some([1, 1, 1, 255]));
        assert_eq!(got.get(0, 1), Some([2, 2, 2, 255]));
    }

    #[test]
    fn every_16_bit_word_survives_a_decode_and_encode() {
        for code in [RGB565, ARGB4444, RGBA4444] {
            for v in 0..=u16::MAX {
                let raw = v.to_le_bytes();
                let decoded = decode(code, 1, 1, &raw).unwrap();
                assert_eq!(
                    enc(code, &decoded).unwrap(),
                    raw,
                    "format {code} word {v:#06x}"
                );
            }
        }
    }

    /// Every code of the table encodes an image to `level_len` bytes, and what it decodes from
    /// them encodes to the same bytes again: the first encode is the only lossy step.
    #[test]
    fn every_format_decodes_and_reencodes() {
        let mut rng = EngineRng::seeded(77);
        for (w, h) in [(1, 1), (2, 3), (4, 4), (5, 9), (7, 5)] {
            let mut src = RgbaBuf::new(w, h);
            for byte in &mut src.pixels {
                *byte = u8::try_from(trunc_i32(rng.unit_f32() * 256.0)).unwrap();
            }
            for code in ALL {
                let raw = enc(code, &src).unwrap();
                assert_eq!(
                    raw.len(),
                    level_len(code, w, h).unwrap(),
                    "format {code} {w}x{h}"
                );
                let back = decode(code, w, h, &raw).unwrap();
                assert_eq!((back.w, back.h), (w, h));
                assert_eq!(enc(code, &back).unwrap(), raw, "format {code} {w}x{h}");
            }
        }
    }

    #[test]
    fn lossless_formats_survive_an_encode_and_decode() {
        let pixels = [10, 20, 30, 40, 250, 0, 7, 255, 0, 0, 0, 0, 1, 2, 3, 4];
        let src = buf(2, 2, &pixels);
        for code in [RGBA32, ARGB32, BGRA32] {
            let raw = enc(code, &src).unwrap();
            assert_eq!(decode(code, 2, 2, &raw).unwrap(), src, "format {code}");
        }
        // Formats without alpha come back opaque; ALPHA8 comes back white.
        let rgb = decode(RGB24, 2, 2, &enc(RGB24, &src).unwrap()).unwrap();
        assert_eq!(rgb.get(0, 0), Some([10, 20, 30, 255]));
        let alpha = decode(ALPHA8, 2, 2, &enc(ALPHA8, &src).unwrap()).unwrap();
        assert_eq!(alpha.get(1, 0), Some([255, 255, 255, 255]));
        assert_eq!(alpha.get(0, 1), Some([255, 255, 255, 0]));
    }

    #[test]
    fn level_sizes_follow_the_format_table() {
        let sizes = [
            (ALPHA8, 16 * 16),
            (ARGB4444, 16 * 16 * 2),
            (RGB24, 16 * 16 * 3),
            (RGBA32, 16 * 16 * 4),
            (ARGB32, 16 * 16 * 4),
            (RGB565, 16 * 16 * 2),
            (DXT1, 16 * 8),
            (DXT5, 16 * 16),
            (RGBA4444, 16 * 16 * 2),
            (BGRA32, 16 * 16 * 4),
        ];
        for (code, want) in sizes {
            assert_eq!(level_len(code, 16, 16).unwrap(), want, "format {code}");
        }
        // Partial blocks round up; an empty image has no blocks.
        assert_eq!(level_len(DXT1, 5, 3).unwrap(), 2 * 8);
        assert_eq!(level_len(DXT5, 1, 1).unwrap(), 16);
        assert_eq!(level_len(DXT1, 0, 7).unwrap(), 0);
    }

    #[test]
    fn unknown_codes_and_huge_sizes_are_errors() {
        for code in [0, 6, 8, 9, 11, 15, 63, -1] {
            assert!(
                matches!(level_len(code, 4, 4), Err(ImageError::UnknownFormat(c)) if c == code)
            );
        }
        assert!(matches!(
            level_len(RGBA32, u32::MAX, u32::MAX),
            Err(ImageError::TooLarge { .. })
        ));
        assert!(matches!(
            decode(9, 1, 1, &[0; 8]),
            Err(ImageError::UnknownFormat(9))
        ));
        assert!(matches!(
            enc(9, &RgbaBuf::new(1, 1)),
            Err(ImageError::UnknownFormat(9))
        ));
    }

    #[test]
    fn data_shorter_than_level_0_is_an_error() {
        for code in ALL {
            let needed = level_len(code, 8, 8).unwrap();
            let raw = vec![0u8; needed - 1];
            assert!(
                matches!(decode(code, 8, 8, &raw), Err(ImageError::ShortData { needed: n, got }) if n == needed && got == needed - 1),
                "format {code}"
            );
        }
    }

    /// The default textures of native 4.5.2: 16 x 16 RGB24 with a full chain is 1023 bytes, and a
    /// reader that only wants level 0 sees the same pixels with or without the chain.
    #[test]
    fn a_mip_chain_behind_level_0_is_tolerated_and_dropped_by_the_encoder() {
        let chain: usize = [16u32, 8, 4, 2, 1]
            .iter()
            .map(|&s| level_len(RGB24, s, s).unwrap())
            .sum();
        assert_eq!(chain, 1023);
        let level0: Vec<u8> = (0..768).map(|i| low(i * 7)).collect();
        let mut with_chain = level0.clone();
        with_chain.extend((0..255).map(|i| low(i * 3)));
        assert_eq!(with_chain.len(), chain);
        let plain = decode(RGB24, 16, 16, &level0).unwrap();
        let chained = decode(RGB24, 16, 16, &with_chain).unwrap();
        assert_eq!(plain, chained);
        assert_eq!(enc(RGB24, &chained).unwrap(), level0);
    }

    #[test]
    fn narrow_channels_widen_to_full_scale_and_quantise_to_the_nearest_level() {
        assert_eq!(widen(31, 5), 255);
        assert_eq!(widen(63, 6), 255);
        assert_eq!(widen(15, 4), 255);
        assert_eq!(widen(0, 5), 0);
        for bits in [4, 5, 6] {
            for v in 0..(1u32 << bits) {
                assert_eq!(quant(widen(v, bits), bits), v);
            }
        }
        assert_eq!(quant(255, 5), 31);
        assert_eq!(quant(128, 4), 8);
    }
}
