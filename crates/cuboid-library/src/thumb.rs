//! The 160 px pictures of the start screen and the library panel: where they are kept, how a
//! texture is reduced, and how a stored one is read back.

use std::path::{Path, PathBuf};

use cuboid_image::{ImageFileKind, RgbaBuf, decode_file, encode_png};
use sha2::{Digest, Sha256};

use crate::store::hex;

pub use cuboid_image::Image;

/// Longest edge of a stored picture.
pub const THUMB_EDGE: u32 = 160;
/// Bytes of a stored picture that are read back.
const MAX_THUMB_FILE: u64 = 4 << 20;
/// Bytes of a reference picture that are decoded for its thumbnail.
const MAX_REFERENCE_FILE: u64 = 32 << 20;

/// A picture to show: `rgba` is `w * h * 4` straight RGBA bytes, rows top to bottom.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pixels {
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
    /// The texels.
    pub rgba: Vec<u8>,
}

/// Where the picture of a recent file is kept: a name made from the path, inside `cache`.
pub fn recent_thumb_path(cache: &Path, file: &Path) -> PathBuf {
    let digest = Sha256::digest(file.to_string_lossy().as_bytes());
    cache.join(format!("{}.png", hex(&digest[..8])))
}

/// An image reduced so that its longest edge is at most [`THUMB_EDGE`]: a box filter that weighs
/// the colour by alpha. A smaller image is returned as it is, never enlarged.
pub fn downscale(image: &Image) -> Image {
    let (w, h) = (image.w, image.h);
    let longest = w.max(h);
    if longest <= THUMB_EDGE {
        return image.clone();
    }
    let new_w = (u64::from(w) * u64::from(THUMB_EDGE) / u64::from(longest)).max(1) as u32;
    let new_h = (u64::from(h) * u64::from(THUMB_EDGE) / u64::from(longest)).max(1) as u32;
    let src = image.rgba();
    let mut out = RgbaBuf::new(new_w, new_h);
    for y in 0..new_h {
        let y0 = (u64::from(y) * u64::from(h) / u64::from(new_h)) as usize;
        let y1 = (((u64::from(y) + 1) * u64::from(h)).div_ceil(u64::from(new_h)) as usize)
            .clamp(y0 + 1, h as usize);
        for x in 0..new_w {
            let x0 = (u64::from(x) * u64::from(w) / u64::from(new_w)) as usize;
            let x1 = (((u64::from(x) + 1) * u64::from(w)).div_ceil(u64::from(new_w)) as usize)
                .clamp(x0 + 1, w as usize);
            let (mut rgb, mut alpha, mut count) = ([0_u64; 3], 0_u64, 0_u64);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = (sy * w as usize + sx) * 4;
                    let a = u64::from(src[at + 3]);
                    for (sum, value) in rgb.iter_mut().zip(&src[at..at + 3]) {
                        *sum += u64::from(*value) * a;
                    }
                    alpha += a;
                    count += 1;
                }
            }
            let to = (y as usize * new_w as usize + x as usize) * 4;
            for (texel, sum) in out.pixels[to..to + 3].iter_mut().zip(rgb) {
                *texel = sum.checked_div(alpha).unwrap_or(0) as u8;
            }
            out.pixels[to + 3] = (alpha / count) as u8;
        }
    }
    Image::from_rgba(out)
}

/// The PNG of the reduced picture of a PNG file.
pub fn thumb_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = decode_file(bytes, ImageFileKind::Other).map_err(|e| e.to_string())?;
    encode_png(&downscale(&image)).map_err(|e| e.to_string())
}

/// The texels of an image with the top row first.
fn pixels_of(image: &Image) -> Pixels {
    let row = image.w as usize * 4;
    let rgba: Vec<u8> = image
        .rgba()
        .chunks_exact(row)
        .rev()
        .flatten()
        .copied()
        .collect();
    Pixels {
        w: image.w,
        h: image.h,
        rgba,
    }
}

/// Reads a stored picture. `None` when the file is missing, too large or not a PNG.
pub fn read_thumb(path: &Path) -> Option<Pixels> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_THUMB_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let image = decode_file(&bytes, ImageFileKind::Other).ok()?;
    if image.w.max(image.h) > THUMB_EDGE * 2 {
        return None;
    }
    Some(pixels_of(&image))
}

/// The reduced picture of a reference image (PNG or JPEG), read from a stored copy. `None` for a
/// file that is missing, too large or in a format the decoder does not read (WebP, GIF).
pub fn reference_thumb(path: &Path) -> Option<Pixels> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_REFERENCE_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let image = decode_file(&bytes, ImageFileKind::Other).ok()?;
    Some(pixels_of(&downscale(&image)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, color: [u8; 4]) -> Image {
        let mut buf = RgbaBuf::new(w, h);
        for texel in buf.pixels.chunks_exact_mut(4) {
            texel.copy_from_slice(&color);
        }
        Image::from_rgba(buf)
    }

    #[test]
    fn a_small_image_is_kept_and_a_large_one_is_reduced_keeping_the_aspect() {
        let small = solid(16, 16, [1, 2, 3, 255]);
        let kept = downscale(&small);
        assert_eq!((kept.w, kept.h), (16, 16));
        let wide = downscale(&solid(640, 320, [10, 20, 30, 255]));
        assert_eq!((wide.w, wide.h), (160, 80));
        assert_eq!(&wide.rgba()[..4], &[10, 20, 30, 255]);
        let thin = downscale(&solid(1000, 3, [9, 9, 9, 255]));
        assert_eq!((thin.w, thin.h), (160, 1));
    }

    #[test]
    fn the_box_filter_averages_and_ignores_transparent_colour() {
        // Left half red, right half transparent black: the reduced pixels stay pure red or clear.
        let mut buf = RgbaBuf::new(320, 160);
        for y in 0..160_u32 {
            for x in 0..320_u32 {
                let at = (y * 320 + x) as usize * 4;
                if x < 160 {
                    buf.pixels[at..at + 4].copy_from_slice(&[255, 0, 0, 255]);
                }
            }
        }
        let small = downscale(&Image::from_rgba(buf));
        assert_eq!((small.w, small.h), (160, 80));
        assert_eq!(&small.rgba()[..4], &[255, 0, 0, 255]);
        let right = (79 * 160 + 159) * 4;
        assert_eq!(small.rgba()[right + 3], 0);
    }

    #[test]
    fn a_stored_picture_reads_back_top_row_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut buf = RgbaBuf::new(2, 2);
        // Row 0 is the bottom row in memory.
        buf.pixels[..4].copy_from_slice(&[1, 0, 0, 255]);
        buf.pixels[8..12].copy_from_slice(&[2, 0, 0, 255]);
        let png = encode_png(&Image::from_rgba(buf)).expect("encodes");
        let path = dir.path().join("t.png");
        std::fs::write(&path, png).expect("write");
        let pixels = read_thumb(&path).expect("reads");
        assert_eq!((pixels.w, pixels.h), (2, 2));
        assert_eq!(pixels.rgba[0], 2, "the top row comes first");
        assert!(read_thumb(&dir.path().join("none.png")).is_none());
        std::fs::write(&path, b"not a png").expect("write");
        assert!(read_thumb(&path).is_none());
    }

    #[test]
    fn recent_pictures_are_named_by_path() {
        let cache = Path::new("cache");
        let a = recent_thumb_path(cache, Path::new("C:/a/tower.cubik"));
        let b = recent_thumb_path(cache, Path::new("C:/b/tower.cubik"));
        assert_ne!(a, b);
        assert_eq!(a, recent_thumb_path(cache, Path::new("C:/a/tower.cubik")));
        assert!(a.starts_with(cache) && a.extension().is_some_and(|e| e == "png"));
    }
}
