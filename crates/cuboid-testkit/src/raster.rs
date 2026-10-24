//! Pictures and the raster tiers P2 and P3 of architecture 19.1.

use std::fmt;

use image::codecs::png::PngEncoder;
use image::{GrayImage, ImageEncoder, ImageFormat};

use crate::diff::Mismatch;

/// SSIM threshold of tier P3, to be calibrated (W7.4). It is a default: a scene class whose
/// calibration needs another value passes its own `min` to [`ssim`].
///
/// spec: arch 19.1
pub const SSIM_MIN: f64 = 0.98;

/// An RGBA8 picture, rows from top to bottom as in a PNG file. Bottom-up pictures of the editor
/// are flipped by the caller.
#[derive(Clone, PartialEq, Eq)]
pub struct Raster {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(thiserror::Error)]
pub enum RasterError {
    #[error("{width} x {height} pixels need {wanted} bytes of RGBA, got {len}")]
    Size {
        width: u32,
        height: u32,
        wanted: u64,
        len: usize,
    },
    #[error("PNG: {0}")]
    Png(#[from] image::ImageError),
}

debug_is_display!(RasterError);

impl fmt::Debug for Raster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Raster({} x {})", self.width, self.height)
    }
}

impl Raster {
    /// `rgba` holds four bytes per pixel, row after row.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, RasterError> {
        let wanted = u64::from(width) * u64::from(height) * 4;
        if u64::try_from(rgba.len()) != Ok(wanted) {
            return Err(RasterError::Size {
                width,
                height,
                wanted,
                len: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Builds a picture from a pixel function `(x, y) -> RGBA`, for synthetic test patterns.
    pub fn from_fn(width: u32, height: u32, mut pixel: impl FnMut(u32, u32) -> [u8; 4]) -> Self {
        let rgba = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| pixel(x, y))
            .collect();
        Self {
            width,
            height,
            rgba,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Decodes a PNG of any colour type to RGBA8.
    pub fn from_png(bytes: &[u8]) -> Result<Self, RasterError> {
        let decoded = image::load_from_memory_with_format(bytes, ImageFormat::Png)?.into_rgba8();
        let (width, height) = decoded.dimensions();
        Ok(Self {
            width,
            height,
            rgba: decoded.into_raw(),
        })
    }

    /// Encodes the picture as an RGBA8 PNG.
    pub fn to_png(&self) -> Result<Vec<u8>, RasterError> {
        let mut out = Vec::new();
        PngEncoder::new(&mut out).write_image(
            &self.rgba,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(out)
    }

    fn pixel(&self, x: usize, y: usize) -> &[u8] {
        let at = (y * self.width as usize + x) * 4;
        &self.rgba[at..at + 4]
    }

    fn channel(&self, channel: usize) -> GrayImage {
        let plane = self.rgba.iter().skip(channel).step_by(4).copied().collect();
        GrayImage::from_raw(self.width, self.height, plane).expect("one byte per pixel")
    }
}

/// A rectangle of pixels, top-left origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Tolerance of tier P2: the largest accepted difference per channel, the share of pixels that may
/// exceed it on edges, and the areas left out of the comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterTolerance {
    /// Largest accepted difference per channel (alpha included).
    pub max_delta: u8,
    /// Share of the compared pixels that may differ by more, on edges only.
    pub edge_fraction: f64,
    /// Areas left out of the comparison, for example text (judged at tier P3 instead).
    pub masks: Vec<Rect>,
}

impl Default for RasterTolerance {
    /// At most 2/255 per channel, and at most 0.1 % of the compared pixels may exceed that, only
    /// on edges.
    ///
    /// spec: arch 19.1
    fn default() -> Self {
        Self {
            max_delta: 2,
            edge_fraction: 0.001,
            masks: Vec::new(),
        }
    }
}

impl RasterTolerance {
    /// Leaves `area` out of the comparison; areas beyond the picture are clipped.
    pub fn mask(mut self, area: Rect) -> Self {
        self.masks.push(area);
        self
    }

    /// One flag per pixel, set inside a mask; `None` without masks.
    fn mask_bits(&self, width: usize, height: usize) -> Option<Vec<bool>> {
        if self.masks.is_empty() {
            return None;
        }
        let mut bits = vec![false; width * height];
        for area in &self.masks {
            let x0 = (area.x as usize).min(width);
            let x1 = (area.x as usize + area.w as usize).min(width);
            let y1 = (area.y as usize + area.h as usize).min(height);
            for y in (area.y as usize).min(height)..y1 {
                bits[y * width + x0..y * width + x1].fill(true);
            }
        }
        Some(bits)
    }
}

/// P2: no channel differs by more than `tol.max_delta` except on a few edge pixels. A pixel
/// counts as an edge when its 3 x 3 neighbourhood in the expected picture holds a contrast above
/// `max_delta`. Differences away from edges, such as a stray pixel or a drawn line the reference
/// lacks, are never accepted.
///
/// spec: arch 19.1
pub fn within(expected: &Raster, actual: &Raster, tol: &RasterTolerance) -> Result<(), Mismatch> {
    same_size(expected, actual)?;
    let (width, height) = (expected.width as usize, expected.height as usize);
    let masked = tol.mask_bits(width, height);
    let (mut compared, mut edge, mut interior) = (0_usize, 0_usize, 0_usize);
    let mut first_interior = None;
    for y in 0..height {
        for x in 0..width {
            if masked.as_ref().is_some_and(|bits| bits[y * width + x]) {
                continue;
            }
            compared += 1;
            let (e, a) = (expected.pixel(x, y), actual.pixel(x, y));
            if delta(e, a) <= tol.max_delta {
                continue;
            }
            if near_edge(expected, x, y, tol.max_delta) {
                edge += 1;
            } else {
                interior += 1;
                first_interior.get_or_insert((x, y, e, a));
            }
        }
    }
    if let Some((x, y, e, a)) = first_interior {
        return Err(Mismatch::new(format!(
            "raster differs: {interior} of {compared} pixels differ by more than {}/255 away from \
             edges, first at ({x}, {y}): expected {e:?}, actual {a:?} ({edge} more on edges)",
            tol.max_delta,
        )));
    }
    if edge as f64 > tol.edge_fraction * compared as f64 {
        return Err(Mismatch::new(format!(
            "raster differs: {edge} of {compared} pixels differ by more than {}/255 on edges, \
             above the budget of {}",
            tol.max_delta, tol.edge_fraction,
        )));
    }
    Ok(())
}

/// P3: the structural similarity (SSIM) of two pictures. The score is the lowest of the four
/// channel scores and 1.0 for identical pictures.
///
/// The metric is the MSSIM of `image-compare` over 8 x 8 windows. It sums the window variances
/// instead of averaging them, so it is harsher on noise than textbook SSIM: a checkerboard of
/// one level in a flat area scores about 0.79, far below [`SSIM_MIN`], although tier P2 accepts
/// such noise. The threshold has to be calibrated with this metric, per scene class.
///
/// spec: arch 19.1
pub fn ssim_score(expected: &Raster, actual: &Raster) -> Result<f64, Mismatch> {
    same_size(expected, actual)?;
    if expected.rgba == actual.rgba {
        return Ok(1.0);
    }
    let mut lowest = 1.0_f64;
    for channel in 0..4 {
        let similarity = image_compare::gray_similarity_structure(
            &image_compare::Algorithm::MSSIMSimple,
            &expected.channel(channel),
            &actual.channel(channel),
        )
        .map_err(|err| Mismatch::new(format!("SSIM failed: {err}")))?;
        lowest = lowest.min(similarity.score);
    }
    Ok(lowest)
}

/// P3: [`ssim_score`] of at least `min`. [`SSIM_MIN`] is the default; every scene class passes the
/// `min` its calibration (W7.4) settles on.
///
/// spec: arch 19.1
pub fn ssim(expected: &Raster, actual: &Raster, min: f64) -> Result<(), Mismatch> {
    let score = ssim_score(expected, actual)?;
    if score >= min {
        return Ok(());
    }
    Err(Mismatch::new(format!(
        "raster differs: SSIM {score:.4} is below {min}"
    )))
}

fn same_size(expected: &Raster, actual: &Raster) -> Result<(), Mismatch> {
    if (expected.width, expected.height) == (actual.width, actual.height) {
        return Ok(());
    }
    Err(Mismatch::new(format!(
        "raster sizes differ: expected {} x {}, actual {} x {}",
        expected.width, expected.height, actual.width, actual.height
    )))
}

/// The largest channel difference of two RGBA pixels.
fn delta(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

fn near_edge(picture: &Raster, x: usize, y: usize, max_delta: u8) -> bool {
    let (width, height) = (picture.width as usize, picture.height as usize);
    let rows = y.saturating_sub(1)..=(y + 1).min(height - 1);
    let mut neighbours =
        rows.flat_map(|ny| (x.saturating_sub(1)..=(x + 1).min(width - 1)).map(move |nx| (nx, ny)));
    let centre = picture.pixel(x, y);
    neighbours.any(|(nx, ny)| delta(centre, picture.pixel(nx, ny)) > max_delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREY: [u8; 4] = [128, 128, 128, 255];

    fn byte(value: u32) -> u8 {
        u8::try_from(value).expect("test values fit a byte")
    }

    fn flat(width: u32, height: u32) -> Raster {
        Raster::from_fn(width, height, |_, _| GREY)
    }

    /// Black left half, white right half: one vertical edge between x = 7 and x = 8.
    fn halves() -> Raster {
        Raster::from_fn(16, 16, |x, _| if x < 8 { [0, 0, 0, 255] } else { [255; 4] })
    }

    fn with_pixel(mut raster: Raster, x: usize, y: usize, rgba: [u8; 4]) -> Raster {
        let at = (y * raster.width as usize + x) * 4;
        raster.rgba[at..at + 4].copy_from_slice(&rgba);
        raster
    }

    #[test]
    fn new_checks_the_buffer_length() {
        assert!(Raster::new(2, 2, vec![0; 16]).is_ok());
        let err = Raster::new(2, 2, vec![0; 15]).expect_err("one byte short");
        assert!(err.to_string().contains("need 16 bytes"), "{err}");
    }

    #[test]
    fn png_round_trip_keeps_every_channel() {
        let picture = Raster::from_fn(5, 3, |x, y| [byte(x * 40), byte(y * 80), 7, byte(200 + x)]);
        let again = Raster::from_png(&picture.to_png().unwrap()).unwrap();
        assert_eq!(picture, again);
    }

    #[test]
    fn within_accepts_small_noise_everywhere() {
        let noisy = Raster::from_fn(32, 32, |x, y| {
            let d = byte((x + y) % 3);
            [128 - d, 128 + d, 128, 255]
        });
        within(&flat(32, 32), &noisy, &RasterTolerance::default()).unwrap();
    }

    #[test]
    fn within_rejects_a_difference_away_from_edges() {
        let wrong = with_pixel(flat(16, 16), 4, 9, [128, 140, 128, 255]);
        let msg = within(&flat(16, 16), &wrong, &RasterTolerance::default())
            .expect_err("interior pixel differs by 12")
            .to_string();
        assert!(
            msg.contains("1 of 256 pixels") && msg.contains("(4, 9)"),
            "{msg}"
        );
    }

    #[test]
    fn within_allows_a_few_edge_pixels_but_not_a_row_of_them() {
        // The edge pixel flips to the other side's colour: a 1 px shift of the edge.
        let one = with_pixel(halves(), 7, 3, [255; 4]);
        let generous = RasterTolerance {
            edge_fraction: 0.01,
            ..RasterTolerance::default()
        };
        within(&halves(), &one, &generous).unwrap();
        assert!(within(&halves(), &one, &RasterTolerance::default()).is_err());

        let shifted = Raster::from_fn(16, 16, |x, _| if x < 7 { [0, 0, 0, 255] } else { [255; 4] });
        let msg = within(&halves(), &shifted, &generous)
            .expect_err("16 edge pixels are 6 % of 256")
            .to_string();
        assert!(
            msg.contains("16 of 256") && msg.contains("on edges"),
            "{msg}"
        );
    }

    #[test]
    fn within_skips_masked_areas() {
        let wrong = with_pixel(flat(16, 16), 4, 9, [255, 0, 0, 255]);
        let masked = RasterTolerance::default().mask(Rect {
            x: 2,
            y: 8,
            w: 4,
            h: 4,
        });
        within(&flat(16, 16), &wrong, &masked).unwrap();
        // a mask reaching past the picture is clipped, not an error
        let past = RasterTolerance::default().mask(Rect {
            x: 0,
            y: 0,
            w: 99,
            h: 99,
        });
        within(&flat(16, 16), &wrong, &past).unwrap();
    }

    #[test]
    fn size_differences_fail_every_raster_tier() {
        let tol = RasterTolerance::default();
        assert!(within(&flat(4, 4), &flat(4, 5), &tol).is_err());
        assert!(ssim_score(&flat(4, 4), &flat(5, 4)).is_err());
    }

    /// Pins the property the P3 threshold is calibrated against (see [`ssim_score`]).
    #[test]
    fn ssim_is_harsh_on_noise_in_flat_areas() {
        let flat = Raster::from_fn(64, 64, |_, _| [100, 100, 100, 255]);
        let checker = Raster::from_fn(64, 64, |x, y| {
            [100 + u8::from((x + y) % 2 == 1), 100, 100, 255]
        });
        let score = ssim_score(&flat, &checker).unwrap();
        assert!((0.7..0.9).contains(&score), "{score}");
    }

    #[test]
    fn ssim_is_one_for_identical_pictures_and_low_for_unrelated_ones() {
        assert!((ssim_score(&halves(), &halves()).unwrap() - 1.0).abs() < 1e-12);
        let noise = Raster::from_fn(16, 16, |x, y| {
            let v = byte((x * 37 + y * 91) % 256);
            [v, 255 - v, v / 2, 255]
        });
        ssim(&halves(), &halves(), SSIM_MIN).unwrap();
        let msg = ssim(&halves(), &noise, SSIM_MIN)
            .expect_err("unrelated")
            .to_string();
        assert!(msg.contains("below 0.98"), "{msg}");
    }
}
