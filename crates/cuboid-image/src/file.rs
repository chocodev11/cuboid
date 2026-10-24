//! Texture files: the decoders of tools 5.10 and the PNG writer of tools 5.11.
//!
//! DDS and TGA follow the original's own loaders, quirks included; every other extension goes
//! through the general decoder of the `image` crate (PNG and JPEG). Rows are flipped to bottom
//! first when a file is read and back to top first when a PNG is written.

use std::io::Cursor;

use image::codecs::png::PngEncoder;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageReader, Limits};

use crate::contract::{
    Image, ImageError, ImageFileKind, MAX_SIDE, MAX_TEXELS, RgbaBuf, check_size,
};
use crate::format;

/// Decodes a texture file into an image.
///
/// DDS and TGA follow the loaders of the original; `Other` accepts what the `image` crate reads
/// (PNG, JPEG), picked by content. Unlike the original, which turns a failed general decode into
/// a 1x1 image (T6), an undecodable file is an error here; the texture commands substitute that
/// image.
///
/// spec: tools 5.10, native 4.5.1
pub fn decode_file(bytes: &[u8], kind: ImageFileKind) -> Result<Image, ImageError> {
    let buf = match kind {
        ImageFileKind::Dds => decode_dds(bytes)?,
        ImageFileKind::Tga => decode_tga(bytes)?,
        ImageFileKind::Other => decode_other(bytes)?,
    };
    if buf.w == 0 || buf.h == 0 {
        return Err(ImageError::Decode("the image has no texels".into()));
    }
    Ok(Image::from_rgba(buf))
}

/// Reverses the order of the rows of `pixels`, `row_len` bytes each.
fn flip_rows(pixels: &mut [u8], row_len: usize) {
    let mut rest = pixels;
    while rest.len() >= 2 * row_len && row_len > 0 {
        let (top, tail) = rest.split_at_mut(row_len);
        let (middle, bottom) = tail.split_at_mut(tail.len() - row_len);
        top.swap_with_slice(bottom);
        rest = middle;
    }
}

/// The DDS loader of the original: only the first magic bytes and the header size are checked,
/// the size comes from the low halves of the height and width words, the last FourCC character
/// `5` selects DXT5 and anything else DXT1, and everything from byte 128 on is level 0.
///
/// spec: tools 5.10 (`.dds`), native 4.5.1
fn decode_dds(bytes: &[u8]) -> Result<RgbaBuf, ImageError> {
    let invalid = |why: &str| ImageError::Decode(format!("DDS: {why}"));
    let (header, data) = bytes
        .split_first_chunk::<128>()
        .ok_or_else(|| invalid("the header is truncated"))?;
    if !header.starts_with(b"DDS") || header[4] != 124 {
        return Err(invalid("not a DDS header"));
    }
    let h = u32::from(u16::from_le_bytes([header[12], header[13]]));
    let w = u32::from(u16::from_le_bytes([header[16], header[17]]));
    check_size(w, h)?;
    let code = if header[87] == b'5' {
        format::DXT5
    } else {
        format::DXT1
    };
    format::decode(code, w, h, data)
}

/// Reads one TGA pixel of `depth` bits, B G R (A). A depth the original does not know reads as
/// a transparent pixel and consumes no input.
fn tga_pixel(src: &mut &[u8], depth: u8) -> Option<[u8; 4]> {
    match depth {
        32 => {
            let (&[b, g, r, a], rest) = src.split_first_chunk()?;
            *src = rest;
            Some([r, g, b, a])
        }
        24 => {
            let (&[b, g, r], rest) = src.split_first_chunk()?;
            *src = rest;
            Some([r, g, b, 255])
        }
        _ => Some([0; 4]),
    }
}

/// The TGA loader of the original: true-colour types 2 (raw) and 10 (RLE) of 24 or 32 bits, the
/// ID field skipped, pixels in file order from the bottom-left whatever the origin bit says.
/// Other types and colour maps give a blank image of the header size.
///
/// spec: tools 5.10 (`.tga`)
fn decode_tga(bytes: &[u8]) -> Result<RgbaBuf, ImageError> {
    let corrupt =
        || ImageError::Decode("TGA: the pixel data is truncated or overruns the image".into());
    let (header, body) = bytes
        .split_first_chunk::<18>()
        .ok_or_else(|| ImageError::Decode("TGA: the header is truncated".into()))?;
    let [id_len, colour_map, image_type] = [header[0], header[1], header[2]];
    let depth = header[16];
    let w = u32::from(u16::from_le_bytes([header[12], header[13]]));
    let h = u32::from(u16::from_le_bytes([header[14], header[15]]));
    check_size(w, h)?;
    let mut buf = RgbaBuf::new(w, h);
    if colour_map != 0 {
        log::warn!("TGA with indexed colors not supported");
        return Ok(buf);
    }
    if image_type != 2 && image_type != 10 {
        log::warn!("TGA image type {image_type} not supported");
        return Ok(buf);
    }
    if depth != 24 && depth != 32 {
        log::warn!("TGA depth {depth} not supported");
    }
    let mut src = body.get(usize::from(id_len)..).ok_or_else(corrupt)?;
    let mut texels = buf.pixels.as_chunks_mut::<4>().0.iter_mut();
    if image_type == 2 {
        for dst in texels {
            *dst = tga_pixel(&mut src, depth).ok_or_else(corrupt)?;
        }
        return Ok(buf);
    }
    while texels.len() > 0 {
        let (&head, rest) = src.split_first().ok_or_else(corrupt)?;
        src = rest;
        let count = usize::from(head & 0x7F) + 1;
        if count > texels.len() {
            return Err(corrupt());
        }
        if head & 0x80 != 0 {
            let colour = tga_pixel(&mut src, depth).ok_or_else(corrupt)?;
            texels.by_ref().take(count).for_each(|dst| *dst = colour);
        } else {
            for dst in texels.by_ref().take(count) {
                *dst = tga_pixel(&mut src, depth).ok_or_else(corrupt)?;
            }
        }
    }
    Ok(buf)
}

/// The general decoder: PNG and JPEG by content, to RGBA8, within [`check_size`]. The header
/// size is checked before any pixel buffer exists, so a small file cannot make it allocate.
///
/// spec: tools 5.10 (any other extension)
fn decode_other(bytes: &[u8]) -> Result<RgbaBuf, ImageError> {
    let invalid = |err: &dyn std::fmt::Display| ImageError::Decode(err.to_string());
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| invalid(&err))?;
    reader.limits({
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        // A 16-bit RGBA image of MAX_TEXELS (512 MiB) plus its RGBA8 copy fits, so every image
        // that check_size accepts stays readable.
        limits.max_alloc = Some(MAX_TEXELS * 16);
        limits
    });
    let decoder = reader.into_decoder().map_err(|err| invalid(&err))?;
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    let rgba = DynamicImage::from_decoder(decoder)
        .map_err(|err| invalid(&err))?
        .into_rgba8();
    let mut pixels = rgba.into_raw();
    flip_rows(&mut pixels, w as usize * 4);
    Ok(RgbaBuf { w, h, pixels })
}

/// Encodes an image as an 8-bit RGBA PNG (rows flipped to top first).
///
/// spec: tools 5.11, interop 6.4
pub fn encode_png(img: &Image) -> Result<Vec<u8>, ImageError> {
    let fail = |err: &dyn std::fmt::Display| ImageError::Encode(err.to_string());
    if img.w == 0 || img.h == 0 {
        return Err(ImageError::Encode("the image has no texels".into()));
    }
    let row_len = img.w as usize * 4;
    let pixels = img.rgba();
    if pixels.len() != row_len * img.h as usize {
        return Err(ImageError::Encode(
            "the pixel data does not match the size".into(),
        ));
    }
    let mut top_first = Vec::with_capacity(pixels.len());
    for row in pixels.chunks_exact(row_len).rev() {
        top_first.extend_from_slice(row);
    }
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(&top_first, img.w, img.h, ExtendedColorType::Rgba8)
        .map_err(|err| fail(&err))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dds(w: u16, h: u16, fourcc_last: u8, blocks: &[u8]) -> Vec<u8> {
        let mut file = vec![0u8; 128];
        file[..4].copy_from_slice(b"DDS ");
        file[4] = 124;
        file[12..14].copy_from_slice(&h.to_le_bytes());
        file[16..18].copy_from_slice(&w.to_le_bytes());
        file[84..88].copy_from_slice(&[b'D', b'X', b'T', fourcc_last]);
        file.extend_from_slice(blocks);
        file
    }

    /// A solid DXT1 block of one 565 colour.
    fn solid(c: u16) -> Vec<u8> {
        let [lo, hi] = c.to_le_bytes();
        vec![lo, hi, lo, hi, 0, 0, 0, 0]
    }

    fn pixels(img: &Image) -> Vec<[u8; 4]> {
        img.rgba().as_chunks::<4>().0.to_vec()
    }

    #[test]
    fn dds_blocks_are_read_without_flipping() {
        // 4 x 8: the first stored block row is the bottom of the image (the bottom-up rule).
        let mut blocks = solid(0xF800);
        blocks.extend(solid(0x001F));
        let img = decode_file(&dds(4, 8, b'1', &blocks), ImageFileKind::Dds).unwrap();
        assert_eq!((img.w, img.h), (4, 8));
        let px = pixels(&img);
        assert_eq!(px[0], [255, 0, 0, 255]);
        assert_eq!(px[4 * 3 + 3], [255, 0, 0, 255]);
        assert_eq!(px[4 * 4], [0, 0, 255, 255]);
        assert_eq!(px[4 * 7 + 3], [0, 0, 255, 255]);
    }

    #[test]
    fn only_the_last_fourcc_character_5_selects_dxt5() {
        // An alpha block (all 128) followed by a red colour block.
        let mut dxt5 = vec![128, 128, 0, 0, 0, 0, 0, 0];
        dxt5.extend(solid(0xF800));
        let alpha_of = |last: u8, blocks: &[u8]| {
            let img = decode_file(&dds(4, 4, last, blocks), ImageFileKind::Dds).unwrap();
            pixels(&img)[0]
        };
        assert_eq!(alpha_of(b'5', &dxt5), [255, 0, 0, 128]);
        // DXT3, "DX10" and anything else decode as DXT1: the first 8 bytes read as a colour block.
        for last in [b'3', b'0', b'1', b'x'] {
            let first = alpha_of(last, &dxt5);
            assert_eq!(first[3], 255, "fourcc {last}");
            assert_ne!(first, [255, 0, 0, 128]);
        }
    }

    #[test]
    fn dds_headers_are_checked_as_far_as_the_original_does() {
        let blocks = solid(0);
        let good = dds(4, 4, b'1', &blocks);
        assert!(decode_file(&good, ImageFileKind::Dds).is_ok());
        // The fourth magic byte is not checked.
        let mut fourth = good.clone();
        fourth[3] = b'?';
        assert!(decode_file(&fourth, ImageFileKind::Dds).is_ok());
        let mut magic = good.clone();
        magic[0] = b'X';
        assert!(decode_file(&magic, ImageFileKind::Dds).is_err());
        let mut size = good.clone();
        size[4] = 123;
        assert!(decode_file(&size, ImageFileKind::Dds).is_err());
        assert!(decode_file(&good[..100], ImageFileKind::Dds).is_err());
        // Missing blocks are the engine's "not enough data"; surplus bytes (mip levels) are ignored.
        assert!(matches!(
            decode_file(&good[..good.len() - 1], ImageFileKind::Dds),
            Err(ImageError::ShortData { .. })
        ));
        let mut chain = good.clone();
        chain.extend([7; 40]);
        assert!(decode_file(&chain, ImageFileKind::Dds).is_ok());
        // Only the low halves of the size words count: 0x10004 reads as 4.
        let mut high = good.clone();
        high[14] = 1;
        high[18] = 1;
        assert_eq!(decode_file(&high, ImageFileKind::Dds).unwrap().w, 4);
    }

    #[test]
    fn an_empty_or_oversized_dds_is_refused() {
        assert!(decode_file(&dds(0, 4, b'1', &[]), ImageFileKind::Dds).is_err());
        let huge = dds(u16::MAX, 4, b'1', &[]);
        assert!(matches!(
            decode_file(&huge, ImageFileKind::Dds),
            Err(ImageError::TooLarge { .. })
        ));
    }

    fn tga(
        image_type: u8,
        depth: u8,
        w: u16,
        h: u16,
        descriptor: u8,
        id: &[u8],
        data: &[u8],
    ) -> Vec<u8> {
        let mut file = vec![
            u8::try_from(id.len()).unwrap(),
            0,
            image_type,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        file.extend(w.to_le_bytes());
        file.extend(h.to_le_bytes());
        file.extend([depth, descriptor]);
        file.extend(id);
        file.extend(data);
        file
    }

    #[test]
    fn tga_raw_true_colour_reads_bgr_and_bgra_in_file_order() {
        let bgr = [1, 2, 3, 4, 5, 6];
        let img = decode_file(&tga(2, 24, 2, 1, 0, &[], &bgr), ImageFileKind::Tga).unwrap();
        assert_eq!(pixels(&img), [[3, 2, 1, 255], [6, 5, 4, 255]]);
        let bgra = [1, 2, 3, 40, 5, 6, 7, 80];
        let img = decode_file(&tga(2, 32, 1, 2, 0, &[], &bgra), ImageFileKind::Tga).unwrap();
        assert_eq!(pixels(&img), [[3, 2, 1, 40], [7, 6, 5, 80]]);
    }

    #[test]
    fn tga_ignores_the_origin_bit_and_skips_the_id_field() {
        // Two rows; the first row of the file is the bottom row whatever the descriptor says.
        let data = [10, 10, 10, 20, 20, 20];
        let bottom_left =
            decode_file(&tga(2, 24, 1, 2, 0x00, b"id", &data), ImageFileKind::Tga).unwrap();
        let top_left =
            decode_file(&tga(2, 24, 1, 2, 0x20, b"id", &data), ImageFileKind::Tga).unwrap();
        assert_eq!(pixels(&bottom_left), [[10, 10, 10, 255], [20, 20, 20, 255]]);
        assert_eq!(pixels(&top_left), pixels(&bottom_left));
    }

    #[test]
    fn tga_rle_packets_run_across_rows() {
        // A run of 3 red pixels, then a raw packet of 2 pixels (green, blue), 5 pixels in a 5 x 1 image.
        let data = [0x82, 0, 0, 255, 0x01, 0, 255, 0, 255, 0, 0];
        let img = decode_file(&tga(10, 24, 5, 1, 0, &[], &data), ImageFileKind::Tga).unwrap();
        let red = [255, 0, 0, 255];
        assert_eq!(
            pixels(&img),
            [red, red, red, [0, 255, 0, 255], [0, 0, 255, 255]]
        );
    }

    #[test]
    fn corrupt_tga_data_is_an_error() {
        // Missing pixels, a run past the end of the image, a missing packet header.
        assert!(decode_file(&tga(2, 24, 2, 1, 0, &[], &[1, 2, 3]), ImageFileKind::Tga).is_err());
        assert!(
            decode_file(
                &tga(10, 24, 2, 1, 0, &[], &[0x82, 1, 2, 3]),
                ImageFileKind::Tga
            )
            .is_err()
        );
        assert!(
            decode_file(
                &tga(10, 24, 2, 1, 0, &[], &[0x80, 1, 2, 3]),
                ImageFileKind::Tga
            )
            .is_err()
        );
        assert!(decode_file(&tga(10, 24, 2, 1, 0, &[], &[]), ImageFileKind::Tga).is_err());
        assert!(decode_file(&tga(2, 24, 2, 1, 0, &[], &[])[..10], ImageFileKind::Tga).is_err());
        // The ID field must fit.
        assert!(decode_file(&tga(2, 24, 1, 1, 0, &[0; 9], &[]), ImageFileKind::Tga).is_err());
    }

    #[test]
    fn tga_colour_maps_and_other_types_give_a_blank_image_of_the_header_size() {
        let mut mapped = tga(2, 24, 3, 2, 0, &[], &[1, 2, 3]);
        mapped[1] = 1;
        let grey = tga(3, 8, 3, 2, 0, &[], &[1, 2, 3]);
        for file in [mapped, grey] {
            let img = decode_file(&file, ImageFileKind::Tga).unwrap();
            assert_eq!((img.w, img.h), (3, 2));
            assert!(img.rgba().iter().all(|&b| b == 0));
        }
    }

    #[test]
    fn tga_depths_other_than_24_and_32_read_transparent_pixels_without_consuming_input() {
        let raw = decode_file(&tga(2, 16, 2, 2, 0, &[], &[]), ImageFileKind::Tga).unwrap();
        assert!(pixels(&raw).iter().all(|&p| p == [0; 4]));
        // In RLE data the packet headers are still read, from whatever bytes follow: a raw packet of
        // 4 pixels (header 3) fills the 2 x 2 image without reading a pixel.
        let rle = decode_file(&tga(10, 15, 2, 2, 0, &[], &[3]), ImageFileKind::Tga).unwrap();
        assert!(pixels(&rle).iter().all(|&p| p == [0; 4]));
    }

    #[test]
    fn oversized_tga_headers_are_refused_before_allocating() {
        let file = tga(2, 24, u16::MAX, u16::MAX, 0, &[], &[]);
        assert!(matches!(
            decode_file(&file, ImageFileKind::Tga),
            Err(ImageError::TooLarge { .. })
        ));
    }

    fn sample() -> Image {
        let mut buf = RgbaBuf::new(3, 2);
        for y in 0..2 {
            for x in 0..3 {
                buf.set(
                    x,
                    y,
                    [
                        u8::try_from(x * 40).unwrap(),
                        u8::try_from(y * 100).unwrap(),
                        9,
                        200 + u8::try_from(x).unwrap(),
                    ],
                );
            }
        }
        Image::from_rgba(buf)
    }

    #[test]
    fn png_round_trip_keeps_pixels_and_orientation() {
        let img = sample();
        let png = encode_png(&img).unwrap();
        let back = decode_file(&png, ImageFileKind::Other).unwrap();
        assert_eq!((back.w, back.h), (3, 2));
        assert_eq!(back.rgba(), img.rgba());
    }

    #[test]
    fn png_files_store_the_top_row_first() {
        let img = sample();
        let png = encode_png(&img).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        // Row 0 of the file is the last row of the bottom-up image.
        assert_eq!(decoded.get_pixel(0, 0).0, [0, 100, 9, 200]);
        assert_eq!(decoded.get_pixel(0, 1).0, [0, 0, 9, 200]);
    }

    #[test]
    fn rgb_png_gets_opaque_alpha_and_16_bit_png_is_reduced() {
        let rgb = image::RgbImage::from_fn(2, 1, |x, _| {
            image::Rgb([u8::try_from(x).unwrap() * 100, 1, 2])
        });
        let mut bytes = Vec::new();
        rgb.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let img = decode_file(&bytes, ImageFileKind::Other).unwrap();
        assert_eq!(pixels(&img), [[0, 1, 2, 255], [100, 1, 2, 255]]);
        let wide = image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(
            1,
            1,
            vec![65535u16, 0, 257, 32896],
        )
        .unwrap();
        let mut bytes = Vec::new();
        wide.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let img = decode_file(&bytes, ImageFileKind::Other).unwrap();
        assert_eq!(pixels(&img), [[255, 0, 1, 128]]);
    }

    #[test]
    fn jpeg_decodes_to_bottom_up_rows_within_the_codec_error() {
        let rgb = image::RgbImage::from_fn(16, 16, |_, y| {
            if y < 8 {
                image::Rgb([240, 20, 20])
            } else {
                image::Rgb([20, 20, 240])
            }
        });
        let mut bytes = Vec::new();
        rgb.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        // Even when the extension says otherwise, the content decides.
        let img = decode_file(&bytes, ImageFileKind::Other).unwrap();
        let px = pixels(&img);
        // File row 0 is the top: red. In the bottom-up buffer the top is the last row.
        let first = px[0];
        let last = px[16 * 15];
        assert!(first[2] > 200 && first[0] < 60, "{first:?}");
        assert!(last[0] > 200 && last[2] < 60, "{last:?}");
        assert_eq!(first[3], 255);
    }

    #[test]
    fn garbage_and_empty_files_are_errors_not_panics() {
        assert!(decode_file(b"definitely not an image", ImageFileKind::Other).is_err());
        assert!(decode_file(&[], ImageFileKind::Other).is_err());
        assert!(decode_file(&[], ImageFileKind::Dds).is_err());
        assert!(decode_file(&[], ImageFileKind::Tga).is_err());
    }

    /// The corpus replay of the `image` fuzz target on stable: random bytes, and random bytes
    /// behind the signatures and headers of each decoder, never panic.
    #[test]
    fn random_files_never_panic() {
        use cuboid_num::{EngineRng, trunc_i32};
        let mut rng = EngineRng::seeded(4);
        let mut below = |n: i32| trunc_i32(rng.unit_f32() * n as f32);
        let png_signature = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        for round in 0..600 {
            let len = usize::try_from(below(300)).unwrap();
            let mut data: Vec<u8> = (0..len)
                .map(|_| u8::try_from(below(256)).unwrap())
                .collect();
            match round % 4 {
                1 => {
                    // A DDS header with a small size.
                    data.resize(data.len().max(128), 0);
                    data[..4].copy_from_slice(b"DDS ");
                    data[4] = 124;
                    for at in [12, 16] {
                        data[at] = u8::try_from(below(40)).unwrap();
                        data[at + 1] = 0;
                    }
                }
                2 => {
                    // A TGA header of a supported type and a small size.
                    data.resize(data.len().max(18), 0);
                    data[1] = 0;
                    data[2] = [2, 10][usize::try_from(below(2)).unwrap()];
                    for at in [12, 14] {
                        data[at] = u8::try_from(below(20)).unwrap();
                        data[at + 1] = 0;
                    }
                    data[16] = [24, 32, 16][usize::try_from(below(3)).unwrap()];
                }
                3 => {
                    data.splice(..0, png_signature);
                }
                _ => {}
            }
            for kind in [ImageFileKind::Dds, ImageFileKind::Tga, ImageFileKind::Other] {
                let Ok(img) = decode_file(&data, kind) else {
                    continue;
                };
                assert_eq!(img.rgba().len(), img.w as usize * img.h as usize * 4);
                if u64::from(img.w) * u64::from(img.h) < 1 << 16 {
                    let _ = img.alpha();
                    assert!(encode_png(&img).is_ok());
                }
            }
        }
    }

    /// A PNG that declares a size and holds no pixel data: signature, IHDR and an empty IDAT.
    fn png_header_only(w: u32, h: u32) -> Vec<u8> {
        fn crc32(data: &[u8]) -> u32 {
            let mut crc = u32::MAX;
            for &byte in data {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
                }
            }
            !crc
        }
        fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
            let typed = [kind.as_slice(), body].concat();
            file.extend(u32::try_from(body.len()).unwrap().to_be_bytes());
            file.extend(&typed);
            file.extend(crc32(&typed).to_be_bytes());
        }
        let mut ihdr = Vec::new();
        ihdr.extend(w.to_be_bytes());
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 0, 0, 0, 0]);
        let mut file = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        chunk(&mut file, b"IHDR", &ihdr);
        chunk(&mut file, b"IDAT", &[]);
        file
    }

    #[test]
    fn a_small_png_declaring_a_huge_size_is_refused_before_decoding() {
        // 16384 x 4097 is under the side limit but over MAX_TEXELS: no pixel buffer is allocated.
        assert!(matches!(
            decode_file(&png_header_only(16_384, 4_097), ImageFileKind::Other),
            Err(ImageError::TooLarge {
                w: 16_384,
                h: 4_097
            })
        ));
        assert!(matches!(
            decode_file(&png_header_only(16_384, 16_384), ImageFileKind::Other),
            Err(ImageError::TooLarge { .. })
        ));
        // The largest size check_size accepts passes the header check and fails on the missing data.
        assert!(matches!(
            decode_file(&png_header_only(16_384, 4_096), ImageFileKind::Other),
            Err(ImageError::Decode(_))
        ));
    }

    #[test]
    fn rows_flip_in_place_for_even_and_odd_counts() {
        let mut even = [1, 1, 2, 2, 3, 3, 4, 4];
        flip_rows(&mut even, 2);
        assert_eq!(even, [4, 4, 3, 3, 2, 2, 1, 1]);
        let mut odd = [1, 2, 3, 4, 5, 6];
        flip_rows(&mut odd, 2);
        assert_eq!(odd, [5, 6, 3, 4, 1, 2]);
    }

    #[test]
    fn an_empty_image_has_no_png() {
        assert!(encode_png(&Image::from_rgba(RgbaBuf::new(0, 0))).is_err());
    }
}
