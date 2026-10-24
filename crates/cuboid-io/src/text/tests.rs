use super::*;

// ---- interop 3.3: reading ---------------------------------------------------------------

fn utf16(text: &str, little_endian: bool) -> Vec<u8> {
    text.encode_utf16()
        .flat_map(|unit| {
            if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            }
        })
        .collect()
}

#[test]
fn byte_order_marks_are_honoured_and_removed() {
    let text = "{\"name\":\"Bóx 😀\"}";
    let utf32 = |word: fn(u32) -> [u8; 4]| -> Vec<u8> {
        text.chars().flat_map(|c| word(u32::from(c))).collect()
    };
    let cases = [
        ("plain UTF-8", text.as_bytes().to_vec()),
        (
            "UTF-8 BOM",
            [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
        ),
        (
            "UTF-16 LE",
            [&[0xFF, 0xFE][..], &utf16(text, true)].concat(),
        ),
        (
            "UTF-16 BE",
            [&[0xFE, 0xFF][..], &utf16(text, false)].concat(),
        ),
        (
            "UTF-32 LE",
            [&[0xFF, 0xFE, 0, 0][..], &utf32(u32::to_le_bytes)].concat(),
        ),
        (
            "UTF-32 BE",
            [&[0, 0, 0xFE, 0xFF][..], &utf32(u32::to_be_bytes)].concat(),
        ),
    ];
    for (name, bytes) in cases {
        assert_eq!(decode_bom(&bytes), text, "{name}");
    }
}

#[test]
fn malformed_input_becomes_replacement_characters() {
    assert_eq!(decode_bom(b"a\xFFb"), "a\u{FFFD}b");
    // An odd byte at the end of UTF-16 text, and a partial UTF-32 word.
    assert_eq!(decode_bom(&[0xFF, 0xFE, b'a', 0, b'b']), "a\u{FFFD}");
    assert_eq!(
        decode_bom(&[0xFF, 0xFE, 0, 0, b'a', 0, 0, 0, b'b']),
        "a\u{FFFD}"
    );
    // An invalid scalar value.
    assert_eq!(decode_bom(&[0, 0, 0xFE, 0xFF, 0, 0, 0xD8, 0]), "\u{FFFD}");
    assert_eq!(decode_bom(b""), "");
}

#[test]
fn a_utf16_mark_followed_by_zero_bytes_is_not_utf32_without_a_full_word() {
    // FF FE 00 is a UTF-16 LE mark and the unit "\0" cut short.
    assert_eq!(decode_bom(&[0xFF, 0xFE, 0]), "\u{FFFD}");
}

// ---- interop 3.4: string literals -------------------------------------------------------

#[test]
fn strings_are_escaped_per_json_and_non_ascii_stays_raw() {
    let cases = [
        ("plain", "\"plain\""),
        ("a\"b", "\"a\\\"b\""),
        ("a\\b", "\"a\\\\b\""),
        ("l1\nl2\r\n\t", "\"l1\\nl2\\r\\n\\t\""),
        ("\u{8}\u{c}", "\"\\b\\f\""),
        ("\u{0}\u{1f}", "\"\\u0000\\u001f\""),
        ("/ \u{7f} é 😀 \u{2028}", "\"/ \u{7f} é 😀 \u{2028}\""),
        ("", "\"\""),
    ];
    for (raw, literal) in cases {
        let mut out = String::new();
        push_json_string(&mut out, raw);
        assert_eq!(out, literal, "{raw:?}");
    }
}

// ---- native 9.4: numbers ----------------------------------------------------------------

#[test]
fn number_tokens_follow_the_invariant_double_grammar() {
    let accepted = [
        ("0", 0.0),
        ("-0", -0.0),
        ("12", 12.0),
        ("-1.5", -1.5),
        ("1e3", 1000.0),
        ("1E+3", 1000.0),
        ("2.5e-1", 0.25),
        ("5.", 5.0),
        ("-.5", -0.5),
        (".5", 0.5),
        ("007", 7.0),
        // White space around the token, and trailing NULs.
        (" 3 ", 3.0),
        ("\t4\r\n", 4.0),
        ("5\0\0", 5.0),
        ("+6", 6.0),
        ("-16777089", -16_777_089.0),
        ("4294967295", 4_294_967_295.0),
    ];
    for (token, value) in accepted {
        assert_eq!(parse_double(token), Some(value), "{token:?}");
    }
    let rejected = [
        "",
        " ",
        "-",
        "+",
        ".",
        "e5",
        "1e",
        "1e+",
        "1.2.3",
        "--1",
        "-+1",
        "- 1",
        "1 2",
        "0x10",
        "1,5",
        "NaN",
        "Infinity",
        "-Infinity",
        "inf",
        "1_0",
        "١٢",
        "\u{a0}1",
    ];
    for token in rejected {
        assert_eq!(parse_double(token), None, "{token:?}");
    }
}

// ---- robustness -------------------------------------------------------------------------

#[test]
fn mutated_text_never_panics() {
    let seed = "\u{feff}{\"a\": \"x \\\" y\", \"b\": [1, 2.5e3, true]}".as_bytes();
    for bytes in crate::testgen::mutants(seed, 5000) {
        let text = decode_bom(&bytes);
        let _ = mc_json::minify(&text);
    }
}

#[test]
fn a_value_stays_small() {
    // Model files hold many small arrays.
    assert!(std::mem::size_of::<Value>() <= 32);
}
