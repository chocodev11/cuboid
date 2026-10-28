//! Pictures as the tools handle them: RGBA8 with straight alpha and the top row first, the order
//! of a PNG and of the `v down` texels the agent reads. The textures of a document run bottom to
//! top, so the conversions flip.

use cuboid_image::{Image, ImageFileKind, RgbaBuf, decode_file, encode_png};

use crate::error::ToolError;

/// A picture, top row first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes, R, G, B, A.
    pub rgba: Vec<u8>,
}

/// The rows of `pixels` (`row` bytes each) in the opposite order.
fn flipped(pixels: &[u8], row: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len());
    for line in pixels.chunks_exact(row.max(1)).rev() {
        out.extend_from_slice(line);
    }
    out
}

impl Picture {
    /// A picture of one colour.
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Picture {
        let count = width as usize * height as usize;
        Picture {
            width,
            height,
            rgba: fill.repeat(count),
        }
    }

    fn at(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    /// The pixel at a place; transparent outside the picture.
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let at = self.at(x, y);
        self.rgba
            .get(at..at + 4)
            .and_then(|pixel| pixel.try_into().ok())
            .unwrap_or_default()
    }

    /// Sets a pixel; nothing happens outside the picture.
    pub fn set(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = self.at(x, y);
        if let Some(pixel) = self.rgba.get_mut(at..at + 4) {
            pixel.copy_from_slice(&color);
        }
    }

    /// The picture of a texture of the document.
    pub fn of_texture(image: &Image) -> Picture {
        Picture {
            width: image.w,
            height: image.h,
            rgba: flipped(image.rgba(), image.w as usize * 4),
        }
    }

    /// The pixels in the order the commands of the core take them: bottom row first.
    pub fn texture_pixels(&self) -> Vec<u8> {
        flipped(&self.rgba, self.width as usize * 4)
    }

    /// Reads a PNG.
    pub fn from_png(bytes: &[u8]) -> Result<Picture, ToolError> {
        let image = decode_file(bytes, ImageFileKind::Other)
            .map_err(|error| ToolError::input(format!("the file is not a PNG: {error}")))?;
        Ok(Picture::of_texture(&image))
    }

    /// The picture as a PNG file.
    pub fn png(&self) -> Result<Vec<u8>, ToolError> {
        let image = Image::from_rgba(RgbaBuf {
            w: self.width,
            h: self.height,
            pixels: self.texture_pixels(),
        });
        encode_png(&image).map_err(|error| ToolError::io(error.to_string()))
    }

    /// Lays the picture over a colour, which is what a background of a render is.
    pub fn over(&mut self, background: [u8; 4]) {
        for pixel in self.rgba.as_chunks_mut::<4>().0 {
            *pixel = blend(*pixel, background);
        }
    }

    /// Lays the picture over a checkerboard of `cell` pixels, which shows what is transparent.
    pub fn over_checker(&mut self, cell: u32, light: [u8; 4], dark: [u8; 4]) {
        let width = self.width.max(1) as usize;
        for (index, pixel) in self.rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = (index % width, index / width);
            let odd = (x / cell as usize + y / cell as usize) % 2 == 1;
            *pixel = blend(*pixel, if odd { dark } else { light });
        }
    }

    /// The smallest box around the pixels that are not fully transparent, `[x0, y0, x1, y1]`
    /// with the end exclusive, and the share of the picture they cover.
    pub fn coverage(&self) -> (Option<[u32; 4]>, f32) {
        let mut bounds: Option<[u32; 4]> = None;
        let mut covered = 0_u64;
        for (index, pixel) in self.rgba.as_chunks::<4>().0.iter().enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            covered += 1;
            let (x, y) = (index as u32 % self.width, index as u32 / self.width);
            let [x0, y0, x1, y1] = bounds.get_or_insert([x, y, x + 1, y + 1]);
            (*x0, *y0, *x1, *y1) = (
                (*x0).min(x),
                (*y0).min(y),
                (*x1).max(x + 1),
                (*y1).max(y + 1),
            );
        }
        let total = (u64::from(self.width) * u64::from(self.height)).max(1);
        (bounds, covered as f32 / total as f32)
    }

    /// The picture with every pixel `scale` times as large.
    pub fn enlarged(&self, scale: u32) -> Picture {
        let mut out = Picture::new(self.width * scale, self.height * scale, [0; 4]);
        for y in 0..out.height {
            for x in 0..out.width {
                out.set(x, y, self.get(x / scale, y / scale));
            }
        }
        out
    }

    /// The picture reduced to at most `side` pixels on its longer side, by the nearest pixel;
    /// the same picture when it fits.
    pub fn fitted(&self, side: u32) -> Picture {
        let longest = self.width.max(self.height);
        if longest <= side {
            return self.clone();
        }
        let width = (u64::from(self.width) * u64::from(side) / u64::from(longest)).max(1) as u32;
        let height = (u64::from(self.height) * u64::from(side) / u64::from(longest)).max(1) as u32;
        let mut out = Picture::new(width, height, [0; 4]);
        for y in 0..height {
            for x in 0..width {
                let from_x = (u64::from(x) * u64::from(self.width) / u64::from(width)) as u32;
                let from_y = (u64::from(y) * u64::from(self.height) / u64::from(height)) as u32;
                out.set(x, y, self.get(from_x, from_y));
            }
        }
        out
    }

    /// Copies `part` into the picture with its top left corner at `(x, y)`.
    pub fn paste(&mut self, part: &Picture, x: u32, y: u32) {
        for row in 0..part.height {
            for column in 0..part.width {
                self.set(x + column, y + row, part.get(column, row));
            }
        }
    }
}

/// A pixel laid over a background, both with straight alpha: the colour of the result weighs each
/// side by what it covers.
fn blend(pixel: [u8; 4], background: [u8; 4]) -> [u8; 4] {
    let alpha = u32::from(pixel[3]);
    let behind = u32::from(background[3]);
    let out = alpha * 255 + behind * (255 - alpha);
    let mut result = [0; 4];
    for channel in 0..3 {
        let mixed = u32::from(pixel[channel]) * alpha * 255
            + u32::from(background[channel]) * behind * (255 - alpha);
        result[channel] = (mixed / out.max(1)).min(255) as u8;
    }
    result[3] = (out / 255).min(255) as u8;
    result
}

/// A colour as `#RRGGBB` or `#RRGGBBAA`.
pub fn parse_color(text: &str) -> Result<[u8; 4], ToolError> {
    let bad = || {
        ToolError::input(format!(
            "`{text}` is not a colour like #RRGGBB or #RRGGBBAA"
        ))
    };
    let digits = text.strip_prefix('#').ok_or_else(bad)?;
    if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    let alpha = if digits.len() == 8 {
        channel(6)
    } else {
        Some(255)
    };
    match (channel(0), channel(2), channel(4), alpha) {
        (Some(r), Some(g), Some(b), Some(a)) => Ok([r, g, b, a]),
        _ => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_have_six_or_eight_hex_digits() {
        assert_eq!(parse_color("#1d1b24"), Ok([0x1d, 0x1b, 0x24, 255]));
        assert_eq!(parse_color("#ff000080"), Ok([255, 0, 0, 0x80]));
        for text in ["", "red", "#fff", "#12345g", "ff0000", "#ff0000ff0"] {
            assert!(parse_color(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_picture_survives_a_png_and_a_background_fills_what_is_transparent() {
        let mut picture = Picture::new(3, 2, [0; 4]);
        picture.set(0, 0, [10, 20, 30, 255]);
        picture.set(2, 1, [200, 0, 0, 255]);
        let back = Picture::from_png(&picture.png().expect("encodes")).expect("decodes");
        assert_eq!(back, picture);
        assert_eq!(picture.coverage(), (Some([0, 0, 3, 2]), 2.0 / 6.0));
        picture.over([1, 2, 3, 255]);
        assert_eq!(picture.get(1, 0), [1, 2, 3, 255]);
        assert_eq!(picture.get(0, 0), [10, 20, 30, 255]);
    }

    #[test]
    fn a_large_picture_is_cut_down_to_the_side_asked_for() {
        let picture = Picture::new(300, 100, [9, 9, 9, 255]);
        let small = picture.fitted(150);
        assert_eq!((small.width, small.height), (150, 50));
        assert_eq!(picture.fitted(400), picture);
    }
}
