//! Colour conversion and packing (native 4.10, tools 1.6 and 7.3, interop 2.6).
//!
//! A packed colour is `0xAARRGGBB`; the signed `i32` forms are the same bits. Byte and float
//! colours are in RGBA order.

use crate::eval::{Real, eval};

/// Converts a float channel to a byte: `trunc(clamp01(c) * 255)` (**UNVERIFIED** for the
/// engine's own float-to-byte conversion; the explicit packers of the specifications truncate).
///
/// spec: tools 1.6, native 4.10
pub fn to_byte(c: f32) -> u8 {
    eval!(to_byte_in(c))
}

fn to_byte_in<R: Real>(c: f32) -> u8 {
    scale_in::<R>(c.clamp(0.0, 1.0)) as u8
}

/// `trunc(c * 255)` without a clamp.
fn scale_in<R: Real>(c: f32) -> i32 {
    (R::from_f32(c) * R::from_f32(255.0)).trunc().to_i32()
}

/// Converts a byte to a float channel: `b / 255`. `to_byte(from_byte(b)) == b` for every byte.
///
/// spec: interop 2.6
pub fn from_byte(b: u8) -> f32 {
    eval!(from_byte_in(b))
}

fn from_byte_in<R: Real>(b: u8) -> f32 {
    (R::from_i32(i32::from(b)) / R::from_f32(255.0)).to_f32()
}

/// Packs byte channels, keeping the alpha. A fully transparent colour packs to 0, the empty
/// voxel.
///
/// spec: tools 7.3, interop 2.6
pub fn argb(c: [u8; 4]) -> u32 {
    let [r, g, b, a] = c;
    if a == 0 {
        return 0;
    }
    u32::from_be_bytes([a, r, g, b])
}

/// Packs the colour of a voxel tool, fill or preset: alpha is forced to 255 unless it is 0,
/// which gives 0 (erase).
///
/// spec: tools 7.3, interop 2.6
pub fn voxel_tool(c: [u8; 4]) -> u32 {
    let [r, g, b, a] = c;
    argb([r, g, b, if a == 0 { 0 } else { 255 }])
}

/// Splits a packed colour into its bytes, keeping the stored alpha.
///
/// spec: interop 2.6
pub fn unpack(v: u32) -> [u8; 4] {
    let [a, r, g, b] = v.to_be_bytes();
    [r, g, b, a]
}

/// Splits a packed colour into its bytes with the alpha forced to 255, the decoding for
/// display.
///
/// spec: tools 7.3, interop 2.6
pub fn unpack_opaque(v: u32) -> [u8; 4] {
    let [r, g, b, _] = unpack(v);
    [r, g, b, 255]
}

/// Packs an element colour (`el[].c`): channels clamped to [0, 1] and truncated after the
/// multiplication by 255; alpha is forced to 255 unless its byte is 0, which gives 0.
///
/// spec: native 4.10
pub fn element(c: [f32; 4]) -> i32 {
    voxel_tool(c.map(to_byte)) as i32
}

/// Unpacks an element colour: RGB from the low 24 bits and alpha always 1, so 0 loads as
/// opaque black.
///
/// spec: native 4.10
pub fn unpack_element(v: i32) -> [f32; 4] {
    unpack_opaque(v as u32).map(from_byte)
}

/// Packs a sub-mesh colour (`me[].sm[].c`): each channel is multiplied by 255, truncated and
/// kept modulo 256 without clamping. A truncated alpha of 0 gives 0, otherwise the alpha byte
/// is kept as computed.
///
/// spec: native 4.10
pub fn submesh(c: [f32; 4]) -> i32 {
    eval!(submesh_in(c))
}

fn submesh_in<R: Real>(c: [f32; 4]) -> i32 {
    let [r, g, b, a] = c.map(scale_in::<R>);
    if a == 0 {
        return 0;
    }
    // `& 0xFF` on a two's complement integer is the remainder modulo 256 for negatives too.
    let byte = |v: i32| (v & 0xFF) as u8;
    u32::from_be_bytes([byte(a), byte(r), byte(g), byte(b)]) as i32
}

/// Unpacks a sub-mesh colour: all four bytes, including alpha, divided by 255.
///
/// spec: native 4.10
pub fn unpack_submesh(v: i32) -> [f32; 4] {
    unpack(v as u32).map(from_byte)
}

/// Formats a packed colour as `#RRGGBBAA`, uppercase with the alpha last.
///
/// spec: tools 7.3, interop 2.6
pub fn hex(v: u32) -> String {
    let [r, g, b, a] = unpack(v);
    format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
}

/// Parses colour text the forgiving way of the colour fields: only the hex digits are kept
/// (`#`, spaces and other letters disappear) and missing digits up to 6 count as 0. The alpha
/// is the fourth digit pair when exactly 8 digits remain and 255 otherwise (a seventh digit
/// and digits beyond the eighth are ignored). Empty text is fully transparent.
///
/// spec: tools 7.3, ui 8.9
pub fn parse_hex(s: &str) -> [u8; 4] {
    if s.is_empty() {
        return [0; 4];
    }
    let mut digits = [0u8; 8];
    let mut count = 0;
    for d in s.chars().filter_map(|c| c.to_digit(16)) {
        if let Some(slot) = digits.get_mut(count) {
            *slot = d as u8;
        }
        count += 1;
    }
    let pair = |i: usize| (digits[2 * i] << 4) | digits[2 * i + 1];
    [
        pair(0),
        pair(1),
        pair(2),
        if count == 8 { pair(3) } else { 255 },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample voxel colour of native 3.5: 0xFF5A9643, stored as the signed -10840509.
    const SAMPLE: u32 = 0xFF5A_9643;

    #[test]
    fn bytes_and_floats_round_trip_under_both_policies() {
        // interop 2.6: a byte goes to float (b/255) and back exactly, in single or double precision.
        for b in 0..=255u8 {
            assert_eq!(to_byte_in::<f32>(from_byte_in::<f32>(b)), b);
            assert_eq!(to_byte_in::<f64>(from_byte_in::<f64>(b)), b);
            assert_eq!(to_byte(from_byte(b)), b);
            assert_eq!(from_byte_in::<f32>(b), from_byte_in::<f64>(b), "byte {b}");
        }
        assert_eq!(from_byte(0), 0.0);
        assert_eq!(from_byte(255), 1.0);
    }

    #[test]
    fn float_channels_are_clamped_then_truncated() {
        // tools 1.6: `trunc(clamp01(c) * 255)`.
        let cases = [
            (0.0, 0),
            (1.0, 255),
            (0.5, 127),
            (0.999_999, 254),
            (1.0 / 255.0, 1),
            (-0.5, 0),
            (2.0, 255),
            (f32::NAN, 0),
        ];
        for (c, expected) in cases {
            assert_eq!(to_byte_in::<f32>(c), expected, "to_byte({c})");
            assert_eq!(to_byte_in::<f64>(c), expected, "to_byte({c}) wide");
        }
    }

    #[test]
    fn packs_bytes_with_alpha_zero_as_empty() {
        assert_eq!(argb([0x5A, 0x96, 0x43, 0xFF]), SAMPLE);
        assert_eq!(argb([0x5A, 0x96, 0x43, 0x80]), 0x805A_9643);
        assert_eq!(argb([0x5A, 0x96, 0x43, 0x00]), 0);
        assert_eq!(SAMPLE as i32, -10_840_509);
    }

    #[test]
    fn tool_colours_are_opaque_unless_erasing() {
        // tools 7.3: "alpha forced to 255 (opaque) unless its alpha is 0, which yields 0".
        assert_eq!(voxel_tool([0x5A, 0x96, 0x43, 0xFF]), SAMPLE);
        assert_eq!(voxel_tool([0x5A, 0x96, 0x43, 0x01]), SAMPLE);
        assert_eq!(voxel_tool([0x5A, 0x96, 0x43, 0x00]), 0);
        assert_eq!(voxel_tool([0, 0, 0, 0xFF]), 0xFF00_0000);
    }

    #[test]
    fn unpacking_keeps_or_forces_alpha() {
        assert_eq!(unpack(0x805A_9643), [0x5A, 0x96, 0x43, 0x80]);
        assert_eq!(unpack_opaque(0x805A_9643), [0x5A, 0x96, 0x43, 0xFF]);
        assert_eq!(unpack(0), [0, 0, 0, 0]);
        assert_eq!(unpack_opaque(0), [0, 0, 0, 255]);
    }

    #[test]
    fn element_colours() {
        // native 4.10: clamp, multiply by 255, truncate; alpha forced to 255 unless its byte is 0.
        let sample = [from_byte(0x5A), from_byte(0x96), from_byte(0x43), 1.0];
        assert_eq!(element(sample), -10_840_509);
        assert_eq!(element([1.0, 0.0, 0.0, 1.0]), 0xFFFF_0000u32 as i32);
        assert_eq!(element([0.0, 0.0, 0.0, 0.5]), 0xFF00_0000u32 as i32);
        assert_eq!(
            element([1.0, 1.0, 1.0, 0.003]),
            0,
            "alpha byte 0 packs to 0"
        );
        assert_eq!(element([2.0, -1.0, 0.5, 1.0]), 0xFFFF_007Fu32 as i32);
        assert_eq!(element([1.0, 1.0, 1.0, 0.0]), 0);
    }

    #[test]
    fn element_colours_unpack_opaque() {
        // "RGB from the low 24 bits, alpha always 255 (so 0 loads as opaque black)".
        assert_eq!(unpack_element(0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(unpack_element(0x0000_00FF), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            unpack_element(-10_840_509),
            [from_byte(0x5A), from_byte(0x96), from_byte(0x43), 1.0]
        );
        // Every opaque colour survives a pack and unpack.
        for b in 0..=255u8 {
            let c = [from_byte(b), from_byte(255 - b), from_byte(b / 2), 1.0];
            assert_eq!(unpack_element(element(c)), c);
        }
    }

    #[test]
    fn submesh_colours_wrap_modulo_256() {
        // native 4.10: truncated after the multiplication by 255, kept modulo 256, no clamp.
        assert_eq!(submesh([1.0, 0.5, 0.25, 1.0]), 0xFFFF_7F3Fu32 as i32);
        assert_eq!(submesh([0.0, 0.0, 0.0, 0.5]), 0x7F00_0000);
        // Alpha keeps its computed value instead of being forced to 255.
        assert_eq!(submesh([1.0, 1.0, 1.0, 0.2]), 0x33FF_FFFF);
        // A truncated alpha of 0 gives 0.
        assert_eq!(submesh([1.0, 1.0, 1.0, 0.003]), 0);
        // 1.5 * 255 = 382 -> 126; -0.1 * 255 = -25 -> 231; no clamping.
        assert_eq!(submesh([1.5, -0.1, 0.0, 1.0]), 0xFF7E_E700u32 as i32);
        for c in [
            [0.3, 0.6, 0.9, 1.0],
            [1.5, -0.1, 0.0, 1.0],
            [0.0, 0.0, 0.0, 0.7],
        ] {
            assert_eq!(submesh_in::<f32>(c), submesh_in::<f64>(c));
        }
    }

    #[test]
    fn submesh_colours_unpack_all_four_bytes() {
        assert_eq!(unpack_submesh(0), [0.0; 4]);
        assert_eq!(
            unpack_submesh(0x8040_2010u32 as i32),
            [
                from_byte(0x40),
                from_byte(0x20),
                from_byte(0x10),
                from_byte(0x80)
            ]
        );
    }

    #[test]
    fn hex_text_is_uppercase_rgba() {
        assert_eq!(hex(SAMPLE), "#5A9643FF");
        assert_eq!(hex(0), "#00000000");
        assert_eq!(hex(0x0102_0304), "#02030401");
        assert_eq!(hex(argb([0xAB, 0xCD, 0xEF, 0x12])), "#ABCDEF12");
    }

    #[test]
    fn parse_hex_follows_the_colour_field_table() {
        // ui 8.9: typed text -> resulting colour.
        let cases: [(&str, [u8; 4]); 10] = [
            ("#FF8800", [0xFF, 0x88, 0x00, 0xFF]),
            ("FF880080", [0xFF, 0x88, 0x00, 0x80]),
            ("F80", [0xF8, 0x00, 0x00, 0xFF]),
            ("#FFF", [0xFF, 0xF0, 0x00, 0xFF]),
            ("0xFF8800", [0x0F, 0xF8, 0x80, 0xFF]),
            ("1234567", [0x12, 0x34, 0x56, 0xFF]),
            ("12345678", [0x12, 0x34, 0x56, 0x78]),
            ("#", [0, 0, 0, 0xFF]),
            ("xyz", [0, 0, 0, 0xFF]),
            ("123456789", [0x12, 0x34, 0x56, 0xFF]),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_hex(text), expected, "{text:?}");
        }
    }

    #[test]
    fn parse_hex_is_case_insensitive_and_empty_is_transparent() {
        // tools 7.3: "empty -> fully transparent".
        assert_eq!(parse_hex(""), [0, 0, 0, 0]);
        assert_eq!(parse_hex("#ff8800aa"), [0xFF, 0x88, 0x00, 0xAA]);
        assert_eq!(parse_hex(" #5a 96 43 "), [0x5A, 0x96, 0x43, 0xFF]);
        for v in [0u32, SAMPLE, 0x8001_0203, 0x0100_0000] {
            let [r, g, b, a] = parse_hex(&hex(v));
            assert_eq!(u32::from_be_bytes([a, r, g, b]), v);
        }
    }
}
