#![no_main]

use arbitrary::Arbitrary;
use cuboid_image::{Image, StoredPixels, format};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input {
    w: u8,
    h: u8,
    bc3: bool,
    /// Block bytes for the decoder.
    blocks: Vec<u8>,
    /// Texel bytes for the encoder.
    texels: Vec<u8>,
}

fuzz_target!(|input: Input| {
    let code = if input.bc3 {
        format::DXT5
    } else {
        format::DXT1
    };
    let (w, h) = (u32::from(input.w), u32::from(input.h));
    let level = format::level_len(code, w, h).expect("a block format has a size");
    let texels = w as usize * h as usize * 4;

    // Arbitrary blocks decode to w * h texels, or read as a placeholder when they are short, and
    // the compressor takes the result back to blocks of the right size.
    let img = Image::from_stored(
        w,
        h,
        StoredPixels {
            format: code,
            raw: input.blocks.into(),
        },
    );
    let decoded = img.rgba();
    assert_eq!(decoded.len(), texels);
    let raw = format::encode(code, w, h, decoded).expect("a block format encodes any image");
    assert_eq!(raw.len(), level);

    // Arbitrary texels compress and decompress.
    let mut pixels = input.texels;
    pixels.resize(texels, 0);
    let raw = format::encode(code, w, h, &pixels).expect("a block format encodes any image");
    assert_eq!(raw.len(), level);
    assert!(format::decode(code, w, h, &raw).is_ok());
});
