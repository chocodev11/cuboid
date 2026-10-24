use super::*;

fn accepted(text: &str) -> Value {
    parse(text).unwrap_or_else(|error| panic!("{text:?} must be accepted: {error}"))
}

fn member<'a>(doc: &'a Value, key: &str) -> &'a Value {
    doc.get(key).unwrap_or_else(|| panic!("missing key {key}"))
}

// ---- interop 10.1: the serializer's standard tolerance ----------------------------------

#[test]
fn comments_single_quotes_and_unquoted_names_are_accepted() {
    let doc = accepted(
        "// header\n{ /* block */ a: 1, 'b': 'two', \"c\": [1, 2,], _d$: {e1: true,},\n  // tail\n}",
    );
    let members = doc.as_object().unwrap();
    assert_eq!(members.keys().collect::<Vec<_>>(), ["a", "b", "c", "_d$"]);
    assert_eq!(member(&doc, "a").as_i32(), 1);
    assert_eq!(member(&doc, "b").as_str(), Some("two"));
    assert_eq!(member(&doc, "c").as_array().map(<[Value]>::len), Some(2));
    assert!(member(member(&doc, "_d$"), "e1").as_bool());
}

#[test]
fn a_legacy_geometry_file_reads_with_its_mixed_notation() {
    // The shape of the old mobs.json: a header member that is not a geometry, a derived key,
    // comments and trailing commas.
    let doc = accepted(
        "{
	\"format_version\": \"1.8.0\", // not a geometry
	'geometry.zombie': {
		texturewidth: 64,
		         \"bones\": [ { \"name\": \"head\", \"pivot\": [0, 24, 0], \"mirror\": true,
		         \"cubes\": [ { \"origin\": [-4, 24, -4], \"size\": [8, 8, 8], \"uv\": [0, 0] }, ] } ]
	},
}",
    );
    let zombie = member(&doc, "geometry.zombie");
    assert_eq!(member(zombie, "texturewidth").as_i32(), 64);
    let bones = member(zombie, "bones").as_array().unwrap();
    let head = bones.first().unwrap();
    assert_eq!(member(head, "pivot").vec3(), Some([0.0, 24.0, 0.0]));
    assert!(member(head, "mirror").as_bool());
    let cube = member(head, "cubes").as_array().unwrap().first().unwrap();
    assert_eq!(member(cube, "origin").vec3(), Some([-4.0, 24.0, -4.0]));
    assert_eq!(member(cube, "uv").as_array().map(<[Value]>::len), Some(2));
}

#[test]
fn quoted_names_keep_the_colon_of_derived_geometries() {
    let doc =
        accepted(r#"{"geometry.husk:geometry.zombie": {"bones": []}, 'geometry.zombie': {}}"#);
    let members = doc.as_object().unwrap();
    assert_eq!(
        members.keys().collect::<Vec<_>>(),
        ["geometry.husk:geometry.zombie", "geometry.zombie"]
    );
}

#[test]
fn escapes_are_decoded() {
    let cases = [
        (r#""a\nb\tc\rd""#, "a\nb\tc\rd"),
        (r#""\"\\\/\'""#, "\"\\/'"),
        (r#""\b\f""#, "\u{8}\u{c}"),
        (r#"'it\'s "fine"'"#, "it's \"fine\""),
        (r#""\u0041\u00e9\u00E9""#, "Aéé"),
        (r#""\uD83D\uDE00""#, "😀"),
        (r#""\uD83D""#, "\u{FFFD}"),
        ("\"raw é 😀 \u{7f}\"", "raw é 😀 \u{7f}"),
        // Other quote kind needs no escape.
        (r#""it's""#, "it's"),
        ("'say \"hi\"'", "say \"hi\""),
    ];
    for (literal, expected) in cases {
        let doc = accepted(&format!("{{s: {literal}}}"));
        assert_eq!(member(&doc, "s").as_str(), Some(expected), "{literal}");
    }
}

#[test]
fn numbers_literals_and_other_roots() {
    let doc = accepted("{a: -1.5, b: 1e3, c: .5, d: 7, e: 5., f: null, g: false, h: 1/*c*/}");
    assert_eq!(Some(member(&doc, "a").as_f32()), Some(-1.5));
    assert_eq!(Some(member(&doc, "b").as_f32()), Some(1000.0));
    assert_eq!(Some(member(&doc, "c").as_f32()), Some(0.5));
    assert_eq!(member(&doc, "d").as_i32(), 7);
    assert_eq!(member(&doc, "e").as_i32(), 5);
    assert_eq!(member(&doc, "f"), &Value::Null);
    assert!(!member(&doc, "g").as_bool());
    assert_eq!(member(&doc, "h").as_i32(), 1);
    // Any value may be the root, and what follows it is not read.
    assert_eq!(accepted("[1, 2]").as_array().map(<[Value]>::len), Some(2));
    assert_eq!(accepted("  42  ").as_i32(), 42);
    assert_eq!(
        accepted(r#"{"a": 1} garbage"#).get("a").map(Value::as_i32),
        Some(1)
    );
}

#[test]
fn the_last_of_repeated_names_wins() {
    let doc = accepted("{a: 1, b: 2, a: 3}");
    assert_eq!(
        doc.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(member(&doc, "a").as_i32(), 3);
}

#[test]
fn malformed_text_is_an_error() {
    let cases = [
        ("", ParseError::End),
        ("   ", ParseError::End),
        ("// only a comment", ParseError::End),
        ("{", ParseError::End),
        (r#"{"a""#, ParseError::End),
        (r#"{"a":"#, ParseError::End),
        (r#"{"a":1"#, ParseError::End),
        (r#"{"a":"open"#, ParseError::End),
        ("{ /* open", ParseError::End),
        (r#"{"a":}"#, ParseError::Unexpected(5)),
        (r#"{"a" 1}"#, ParseError::Unexpected(5)),
        (r#"{"a":x}"#, ParseError::Unexpected(5)),
        (r#"{"a":True}"#, ParseError::Unexpected(5)),
        (r#"{"a":tru}"#, ParseError::Unexpected(5)),
        (r#"{"a":nullx}"#, ParseError::Unexpected(9)),
        // A comma that is missing, doubled or leading.
        (r#"{"a":1 "b":2}"#, ParseError::Unexpected(7)),
        (r#"{"a":1,,}"#, ParseError::Unexpected(7)),
        ("{,}", ParseError::Unexpected(1)),
        ("[1 2]", ParseError::Unexpected(3)),
        // A slash that opens no comment.
        (r#"{"a":1 / 2}"#, ParseError::Unexpected(7)),
        // Names.
        ("{a b: 1}", ParseError::Unexpected(3)),
        ("{-a: 1}", ParseError::Unexpected(1)),
        // Numbers.
        (r#"{"a":1.2.3}"#, ParseError::Number(5)),
        (r#"{"a":-}"#, ParseError::Number(5)),
        (r#"{"a":0x10}"#, ParseError::Number(5)),
        // Escapes.
        (r#"{"a":"\x"}"#, ParseError::Escape(6)),
        (r#"{"a":"\u12"}"#, ParseError::Escape(6)),
        (r#"{"a":"\u12G4"}"#, ParseError::Escape(6)),
    ];
    for (text, error) in cases {
        assert_eq!(parse(text), Err(error), "{text:?}");
    }
}

#[test]
fn error_messages_read_well() {
    assert_eq!(ParseError::End.to_string(), "unexpected end of text");
    assert_eq!(
        ParseError::Unexpected(5).to_string(),
        "unexpected character at offset 5"
    );
    assert_eq!(
        ParseError::Depth.to_string(),
        "nesting deeper than 128 levels"
    );
}

#[test]
fn nesting_is_followed_down_to_the_limit() {
    let nested = |levels: usize| "[".repeat(levels) + &"]".repeat(levels);
    accepted(&nested(MAX_DEPTH));
    assert_eq!(parse(&nested(MAX_DEPTH + 1)), Err(ParseError::Depth));
}

// ---- robustness -------------------------------------------------------------------------

const SAMPLE: &str = r#"// mobs
{ 'geometry.a:geometry.b': { texturewidth: 64, "bones": [ { "name": "head",
    /* pivot */ "pivot": [0, 24, -0.5e1,], "cubes": [ { "uv": [0, 0] } ] } ] }, }"#;

#[test]
fn mutated_documents_never_panic() {
    accepted(SAMPLE);
    for bytes in crate::testgen::mutants(SAMPLE.as_bytes(), 5000) {
        let _ = parse(&String::from_utf8_lossy(&bytes));
    }
}

// ---- differential test against serde_json ------------------------------------------------

#[test]
fn standard_json_reads_as_standard_json_does() {
    let mut rng = crate::testgen::Rng::new(11);
    for _ in 0..500 {
        let json = crate::testgen::rooted(crate::testgen::random_json(&mut rng, 0, true));
        let expected = crate::testgen::reader_value(&json);
        for text in [
            json.to_string(),
            serde_json::to_string_pretty(&json).unwrap(),
        ] {
            assert_eq!(parse(&text), Ok(expected.clone()), "{text}");
        }
    }
}
