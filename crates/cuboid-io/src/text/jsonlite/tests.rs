use super::*;

fn accepted(text: &str) -> Value {
    parse(text).unwrap_or_else(|_| panic!("{text:?} must be accepted"))
}

fn member<'a>(doc: &'a Value, key: &str) -> &'a Value {
    doc.get(key).unwrap_or_else(|| panic!("missing key {key}"))
}

/// The value of `s` in `{"s":<literal>}`.
fn string_of(literal: &str) -> String {
    let doc = accepted(&format!("{{\"s\":{literal}}}"));
    member(&doc, "s").as_str().unwrap().to_owned()
}

// ---- native 9.4 and interop 8.6: accepted and rejected input ----------------------------

#[test]
fn accepted_input() {
    let texts = [
        // The root is an object; Unicode white space is skipped, also before the root.
        "{}",
        " \t\r\n{ \n}\n",
        "\u{a0}\u{2003}{\"a\":1}\u{3000}",
        r#"{ "a" : 1 , "b" : [ 1 , 2 ] }"#,
        // A trailing comma before } or ] is accepted.
        r#"{"a":1,}"#,
        r#"{"a":[1,2,],"b":{"c":null,},}"#,
        r#"{"a":[],"b":{}}"#,
        // Literals.
        r#"{"a":true,"b":false,"c":null}"#,
        // Numbers: leading zeros, exponents, a fraction without digits, white space inside the
        // token.
        r#"{"a":007,"b":-1.5e2,"c":1E+2,"d":5.,"e":-.5,"f": 12 ,"g":
 3
}"#,
        // Nothing is read after the root.
        r#"{"a":1} and some trailing garbage"#,
        // Keys may hold anything a string can.
        r#"{"":1,"é😀":2,"a b":3}"#,
    ];
    for text in texts {
        accepted(text);
    }
}

#[test]
fn rejected_input() {
    let deep_object = |levels: usize| "{\"a\":".repeat(levels) + "1" + &"}".repeat(levels);
    let texts = [
        // The root must be an object.
        "",
        "   ",
        "[]",
        "[{}]",
        "\"s\"",
        "1",
        "null",
        // Unexpected end of text.
        "{",
        r#"{"a""#,
        r#"{"a":"#,
        r#"{"a":1"#,
        r#"{"a":1,"#,
        r#"{"a":[1"#,
        r#"{"a":{"b":1}"#,
        // A leading comma, doubled commas, a comma that is missing.
        r#"{,"a":1}"#,
        r#"{"a":1,,}"#,
        r#"{"a":1,,"b":2}"#,
        r#"{"a":[,1]}"#,
        r#"{"a":[1,,2]}"#,
        r#"{"a":[1 2]}"#,
        r#"{"a":1 "b":2}"#,
        // Comments.
        "{// c\n\"a\":1}",
        r#"{/* c */"a":1}"#,
        r#"{"a":1 /* c */}"#,
        // Quotes and names.
        "{'a':1}",
        r#"{a:1}"#,
        r#"{"a":'x'}"#,
        r#"{1:2}"#,
        r#"{"a" 1}"#,
        r#"{"a"::1}"#,
        // Numbers.
        r#"{"a":+1}"#,
        r#"{"a":.5}"#,
        r#"{"a":-}"#,
        r#"{"a":--1}"#,
        r#"{"a":1.2.3}"#,
        r#"{"a":1e}"#,
        r#"{"a":0x10}"#,
        r#"{"a":NaN}"#,
        r#"{"a":Infinity}"#,
        r#"{"a":-Infinity}"#,
        r#"{"a":1 2}"#,
        // Literals are recognised by the first letter and checked as a whole word.
        r#"{"a":nul}"#,
        r#"{"a":nullx}"#,
        r#"{"a":True}"#,
        r#"{"a":tru}"#,
        r#"{"a":undefined}"#,
        // A quote after a backslash does not end a string.
        r#"{"a":"x}"#,
        r#"{"a":"x\"}"#,
        r#"{"a":"x\\"}"#,
        // Mismatched brackets.
        r#"{"a":[1}"#,
        r#"{"a":{]}"#,
        r#"{"a":1]}"#,
    ];
    for text in texts {
        assert_eq!(parse(text), Err(SyntaxError), "{text:?}");
    }
    // The depth limit is the port's own.
    accepted(&deep_object(MAX_DEPTH));
    assert_eq!(parse(&deep_object(MAX_DEPTH + 1)), Err(SyntaxError));
    let nested_arrays = format!(
        "{{\"a\":{}{}}}",
        "[".repeat(MAX_DEPTH),
        "]".repeat(MAX_DEPTH)
    );
    assert_eq!(parse(&nested_arrays), Err(SyntaxError));
}

#[test]
fn the_error_message_is_fixed() {
    assert_eq!(SyntaxError.to_string(), "Invalid data found");
}

// ---- native 9.4: strings ----------------------------------------------------------------

#[test]
fn only_unicode_escapes_are_decoded() {
    let cases = [
        (r#""plain""#, "plain"),
        (r#""""#, ""),
        ("\"é😀\"", "é😀"),
        // Everything else stays, backslash included.
        (r#""a\"b""#, r#"a\"b"#),
        (r#""a\\b""#, r"a\\b"),
        (r#""l1\nl2\t\/""#, r"l1\nl2\t\/"),
        (r#""\x41""#, r"\x41"),
        // \uXXXX, in both cases of hex digits.
        (r#""\u0041""#, "A"),
        (r#""\u00e9\u00E9""#, "éé"),
        (r#""x\u0041y\u0042""#, "xAyB"),
        // A surrogate pair is one character.
        (r#""\uD83D\uDE00""#, "😀"),
        (r#""\ud83d\ude00""#, "😀"),
        // Not followed by four hex digits: kept.
        (r#""\u12""#, r"\u12"),
        (r#""\u12G4""#, r"\u12G4"),
        (r#""\u+041""#, r"\u+041"),
        (r#""\u 041""#, r"\u 041"),
        (r#""\u""#, r"\u"),
    ];
    for (literal, expected) in cases {
        assert_eq!(string_of(literal), expected, "{literal}");
    }
}

#[test]
fn the_unicode_replacement_also_applies_after_an_escaped_backslash() {
    // The content is delimited first: `\\u0041` is a backslash followed by `A`.
    assert_eq!(string_of(r#""\\u0041""#), r"\A");
}

#[test]
fn a_lone_surrogate_becomes_the_replacement_character() {
    assert_eq!(string_of(r#""a\uD83Db""#), "a\u{FFFD}b");
    assert_eq!(string_of(r#""\uDE00""#), "\u{FFFD}");
}

#[test]
fn keys_decode_unicode_escapes_too() {
    let doc = accepted(r#"{"a\u0062":1}"#);
    assert_eq!(member(&doc, "ab").as_i32(), 1);
}

// ---- native 9.4: members ----------------------------------------------------------------

#[test]
fn the_last_of_repeated_keys_wins_and_the_order_is_the_documents() {
    let doc = accepted(r#"{"a":1,"b":2,"a":3,"c":4}"#);
    let members = doc.as_object().unwrap();
    assert_eq!(members.keys().collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(member(&doc, "a").as_i32(), 3);
}

#[test]
fn typed_lookups_read_the_documented_defaults() {
    let doc = accepted(
        r#"{"i":2.9,"n":-2.9,"big":3e10,"colour":-16777089,"f":0.1,"t":true,"s":"txt",
            "v4":[1,2,3,4],"v2":[1,2],"mixed":[1,"x",null],"o":{"k":1},"z":null}"#,
    );
    // Integers truncate toward zero and keep all the bits of a packed colour.
    assert_eq!(member(&doc, "i").as_i32(), 2);
    assert_eq!(member(&doc, "n").as_i32(), -2);
    assert_eq!(member(&doc, "colour").as_i32(), -16_777_089);
    assert_eq!(member(&doc, "big").as_i32(), i32::MAX);
    assert_eq!(Some(member(&doc, "f").as_f32()), Some(0.1));
    // A value of another type reads as 0, false or no string.
    assert_eq!(Some(member(&doc, "s").as_f32()), Some(0.0));
    assert_eq!(member(&doc, "t").as_i32(), 0);
    assert!(!member(&doc, "i").as_bool());
    assert!(member(&doc, "t").as_bool());
    assert_eq!(member(&doc, "s").as_str(), Some("txt"));
    assert_eq!(member(&doc, "i").as_str(), None);
    assert_eq!(member(&doc, "z").as_str(), None);
    assert!(doc.get("missing").is_none());
    assert!(member(&doc, "s").get("k").is_none());
    // Vectors need three (Vector3) or four (Vector4) items.
    assert_eq!(member(&doc, "v4").vec3(), Some([1.0, 2.0, 3.0]));
    assert_eq!(member(&doc, "v4").vec4(), Some([1.0, 2.0, 3.0, 4.0]));
    assert_eq!(member(&doc, "v2").vec3(), None);
    assert_eq!(member(&doc, "mixed").vec3(), Some([1.0, 0.0, 0.0]));
    assert_eq!(member(&doc, "mixed").vec4(), None);
    assert_eq!(member(&doc, "s").vec3(), None);
    assert_eq!(member(&doc, "o").get("k").map(Value::as_i32), Some(1));
    assert_eq!(member(&doc, "v2").as_array().map(<[Value]>::len), Some(2));
}

// ---- robustness -------------------------------------------------------------------------

const SAMPLE: &str = r##"{"name":"Box","type":1,"elements":[{"from":[0,0,0],"to":[16,16,16],
    "faces":{"down":{"uv":[0,0,16,16],"texture":"#side","cullface":"down"}},"rotation":
    {"origin":[8,8,8],"axis":"y","angle":22.5}}],"textures_path":{"side":"a\u0062c\"d"},}"##;

#[test]
fn mutated_documents_never_panic() {
    accepted(SAMPLE);
    for bytes in crate::testgen::mutants(SAMPLE.as_bytes(), 5000) {
        let _ = parse(&String::from_utf8_lossy(&bytes));
    }
}

// ---- differential test against serde_json ------------------------------------------------

#[test]
fn escape_free_documents_read_as_standard_json_does() {
    let mut rng = crate::testgen::Rng::new(7);
    for _ in 0..500 {
        let json = crate::testgen::rooted(crate::testgen::random_json(&mut rng, 0, false));
        let expected = crate::testgen::reader_value(&json);
        for text in [
            json.to_string(),
            serde_json::to_string_pretty(&json).unwrap(),
        ] {
            assert_eq!(parse(&text), Ok(expected.clone()), "{text}");
        }
    }
}
