#![no_main]

use cuboid_image::{ImageFileKind, decode_file, encode_png};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for kind in [ImageFileKind::Dds, ImageFileKind::Tga, ImageFileKind::Other] {
        let Ok(img) = decode_file(data, kind) else {
            continue;
        };
        // A decoded image holds exactly w * h texels.
        assert_eq!(img.rgba().len(), img.w as usize * img.h as usize * 4);
        if img.w * img.h <= 1 << 20 {
            let _ = img.alpha();
            assert!(encode_png(&img).is_ok());
        }
    }
});
