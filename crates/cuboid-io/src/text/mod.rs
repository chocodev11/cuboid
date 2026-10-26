//! Text dialects and emitters (native 9.4; interop 3, 7.9, 8.6, 10.1, 11.1).
//!
//! Readers: [`jsonlite`] is the permissive dialect of the Minecraft JSON and `.cub` importers,
//! [`tolerant`] the lenient JSON of the `.mcpe` importer; both build a [`Value`]. Emitters:
//! [`NetJson`] (the original serializer's layout), [`mc_json`] (the hand-built Minecraft writer)
//! and [`Lines`] (line-oriented writers). Every emitter produces UTF-8 without a byte-order
//! mark; [`decode_bom`] is the matching reader.

use std::borrow::Cow;
use std::fmt::Write;

use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE};

pub mod base64;
pub mod jsonlite;
mod lines;
pub mod mc_json;
mod net_json;
pub mod tolerant;
mod value;

pub use lines::Lines;
pub use net_json::NetJson;
pub use value::Value;

/// How deep the readers follow arrays and objects. The reference has no limit; real models stay
/// below 10 levels, and a limit keeps hostile input from exhausting the stack.
const MAX_DEPTH: usize = 128;

/// Decodes a text file the way the reference reads one: a UTF-8, UTF-16 or UTF-32 byte-order
/// mark is honoured and removed, anything else is UTF-8. Malformed sequences become U+FFFD.
///
/// spec: interop 3.3, native 9.2
pub fn decode_bom(bytes: &[u8]) -> Cow<'_, str> {
    match bytes {
        [0xFF, 0xFE, 0, 0, rest @ ..] => decode_utf32(rest, u32::from_le_bytes),
        [0, 0, 0xFE, 0xFF, rest @ ..] => decode_utf32(rest, u32::from_be_bytes),
        [0xFF, 0xFE, rest @ ..] => UTF_16LE.decode_without_bom_handling(rest).0,
        [0xFE, 0xFF, rest @ ..] => UTF_16BE.decode_without_bom_handling(rest).0,
        [0xEF, 0xBB, 0xBF, rest @ ..] => UTF_8.decode_without_bom_handling(rest).0,
        _ => UTF_8.decode_without_bom_handling(bytes).0,
    }
}

fn decode_utf32(bytes: &[u8], word: fn([u8; 4]) -> u32) -> Cow<'static, str> {
    let chunks = bytes.chunks_exact(4);
    let partial = !chunks.remainder().is_empty();
    let mut text: String = chunks
        .map(|chunk| {
            <[u8; 4]>::try_from(chunk)
                .ok()
                .and_then(|quad| char::from_u32(word(quad)))
                .unwrap_or(char::REPLACEMENT_CHARACTER)
        })
        .collect();
    if partial {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    Cow::Owned(text)
}

/// Appends `s` as a JSON string literal. `"`, `\` and the control characters are escaped
/// (`\n`, `\r`, `\t`, `\b`, `\f`, otherwise `\u00xx`); everything else, non-ASCII included, is
/// written as it is.
///
/// spec: interop 3.4
pub fn push_json_string(out: &mut String, s: &str) {
    out.reserve(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c < ' ' => {
                // Writing to a String cannot fail.
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The value of four hexadecimal digits, the `XXXX` of `\uXXXX`.
fn hex4(digits: &str) -> Option<u16> {
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(digits, 16).ok()
}

/// White space around a number: the characters 0x09 to 0x0D and the space.
fn is_number_white(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ')
}

/// Parses a number token as an invariant-culture double: optional surrounding white space (the
/// runtime also tolerates trailing NUL characters), an optional sign, digits with an optional
/// `.` and an optional exponent. `NaN`, `Infinity` and hexadecimal forms fail.
///
/// spec: native 9.4, interop 8.6
fn parse_double(token: &str) -> Option<f64> {
    let text = token
        .trim_start_matches(is_number_white)
        .trim_end_matches(|c| is_number_white(c) || c == '\0');
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes.get(from..).map_or(0, |rest| {
            rest.iter().take_while(|b| b.is_ascii_digit()).count()
        })
    };

    let mut at = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let int_digits = digits(at);
    at += int_digits;
    let mut frac_digits = 0;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        frac_digits = digits(at);
        at += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        at += usize::from(matches!(bytes.get(at), Some(b'+' | b'-')));
        let exp_digits = digits(at);
        if exp_digits == 0 {
            return None;
        }
        at += exp_digits;
    }
    if at != bytes.len() {
        return None;
    }
    text.parse().ok()
}

#[cfg(test)]
mod tests;
