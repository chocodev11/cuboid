//! Helpers of the hand-built Minecraft JSON writers: the exporters concatenate their text as
//! interop 7 lays it out (tabs, LF, X9 and G7 numbers from `cuboid-num`) and use
//! [`push_json_string`](super::push_json_string) for every string value, which escapes `"`, `\`
//! and control characters, a fix the reference lacks.
//!
//! spec: interop 3.4, interop 7.9

/// "Compress data": removes every run of white space outside string literals. A string literal
/// is a `"`, then any characters other than `"` and `\` or escape pairs `\<any>`, then a `"`;
/// a `"` that opens no such literal is an ordinary character.
///
/// spec: interop 7.9
pub fn minify(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut rest = json;
    while let Some(c) = rest.chars().next() {
        let len = if c == '"' {
            literal_len(rest).unwrap_or(1)
        } else {
            c.len_utf8()
        };
        let (token, tail) = rest.split_at(len);
        if !c.is_whitespace() {
            out.push_str(token);
        }
        rest = tail;
    }
    out
}

/// The length in bytes of the string literal at the start of `text`, if it has an end.
fn literal_len(text: &str) -> Option<usize> {
    let mut bytes = text.bytes().enumerate().skip(1);
    while let Some((at, byte)) = bytes.next() {
        match byte {
            b'"' => return Some(at + 1),
            // Bytes of a multi-byte character are never `"` or `\`, so the pair can be taken
            // byte-wise.
            b'\\' => {
                bytes.next();
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_space_outside_strings_is_removed() {
        let pretty = "{\n\t\"__comment\": \"Box one\",\n\t\"textures\": {\n\t\t\"a\": \"blocks/x y\"\n\t},\n\t\"elements\": [ \t{ \"from\": [ 0, 0, 0 ] } ]\n}";
        assert_eq!(
            minify(pretty),
            "{\"__comment\":\"Box one\",\"textures\":{\"a\":\"blocks/x y\"},\"elements\":[{\"from\":[0,0,0]}]}"
        );
    }

    #[test]
    fn escape_pairs_do_not_end_a_string() {
        let json = r#"{ "a": "x \" y \\" , "b" : "p  q" }"#;
        assert_eq!(minify(json), r#"{"a":"x \" y \\","b":"p  q"}"#);
    }

    #[test]
    fn unicode_white_space_goes_too_and_unicode_text_stays() {
        assert_eq!(minify("[\u{a0}1,\u{2003}\"é 😀\"\u{3000}]"), "[1,\"é 😀\"]");
    }

    #[test]
    fn a_quote_that_opens_no_literal_is_an_ordinary_character() {
        assert_eq!(minify("a \" b c"), "a\"bc");
        assert_eq!(minify(""), "");
        assert_eq!(minify(" \t\r\n"), "");
    }
}
