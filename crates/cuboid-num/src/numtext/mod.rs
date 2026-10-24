//! Number text: the three float forms of interop 3.1 and "try parse, 0 on failure".
//!
//! Everything is invariant-culture: `.` is the decimal separator and nothing is grouped. The
//! digits come from the exact binary value of the f32 through an explicit rounding stage
//! ([`TextRounding`]); the platform's float formatting is never used.

mod digits;
mod parse;
#[cfg(test)]
mod tests;

use digits::Digits;

/// How the formatters turn the exact binary value into the digits they print.
///
/// The runtime's formatter was established from its documented rules and has not been observed
/// on a real export (**UNVERIFIED**, T-NUM-1). This switch exists so that the variant the
/// reference vectors confirm can be selected by changing [`TEXT_ROUNDING`] alone.
///
/// spec: interop 3.1, architecture 7
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRounding {
    /// The digits of the exact binary value are rounded once to the requested count. The first
    /// dropped digit decides and 5 or more rounds up.
    Exact,
    /// The exact value is rounded to 15 significant digits first (the double formatting path),
    /// then to the requested count.
    Via15,
}

/// The rounding stage used by [`g7`], [`x9`] and [`nj`].
pub const TEXT_ROUNDING: TextRounding = TextRounding::Exact;

impl TextRounding {
    /// The digits of `|x|` rounded to `keep` significant digits; `x` is finite and non-zero.
    fn digits(self, x: f32, keep: i32) -> Digits {
        let mut d = Digits::exact(x);
        if self == Self::Via15 {
            d.round(15);
        }
        d.round(keep);
        d
    }
}

/// The default float text (G7): the value rounded to 7 significant digits, trailing zeros
/// dropped. Values with a decimal exponent of -5 or lower, or 7 or higher, use `d.ddddddE+dd`
/// with at least two exponent digits. Negative zero prints as `0` (**UNVERIFIED**); NaN and
/// the infinities print as `NaN`, `Infinity` and `-Infinity`.
///
/// `0.1`, `22.5`, `-8`, `0.0001`, `1E-05`, `1.234568E+07`.
///
/// spec: interop 3.1
pub fn g7(x: f32) -> String {
    general(TEXT_ROUNDING, x, 7)
}

/// The custom pattern `0.#########` (X9): the value rounded to 7 significant digits, then
/// printed fixed-point with at most 9 fractional digits, without trailing zeros or point and
/// never in exponent form. A value that rounds to zero prints as `0`, also a negative one.
///
/// `5.6568` for the stored float 5.65680027, `0.01`, `0`.
///
/// spec: interop 3.1
pub fn x9(x: f32) -> String {
    x9_with(TEXT_ROUNDING, x)
}

/// The float text of the JSON serializer (NJ): the G7 text when it parses back to the same
/// value, otherwise G9, plus `.0` when the text has no `.`, `E` or `e`. Negative zero prints as
/// `0.0` (**UNVERIFIED**). NaN and the infinities keep their G7 text without a decimal place.
///
/// `8.0`, `22.5`, `0.1`, `1E-05`.
///
/// spec: interop 3.1
pub fn nj(x: f32) -> String {
    nj_with(TEXT_ROUNDING, x)
}

/// Parses a float field: "try parse, 0 on failure" with the invariant culture, so empty text,
/// `,5` and `abc` read 0. See [`parse_or_zero_i32`] for integer fields.
///
/// spec: tools 1.6
pub fn parse_or_zero_f32(s: &str) -> f32 {
    parse::float(s).unwrap_or(0.0)
}

/// Parses an integer field: "try parse, 0 on failure". Surrounding white space and a leading
/// sign are accepted; `7.5`, `7,5`, `7.` and empty text read 0.
///
/// spec: tools 1.6, tools 8.10
pub fn parse_or_zero_i32(s: &str) -> i32 {
    parse::int(s).unwrap_or(0)
}

/// The texts that need no digits: NaN, the infinities and zero (also negative zero).
fn special(x: f32) -> Option<&'static str> {
    if x.is_nan() {
        Some("NaN")
    } else if x == f32::INFINITY {
        Some("Infinity")
    } else if x == f32::NEG_INFINITY {
        Some("-Infinity")
    } else if x == 0.0 {
        Some("0")
    } else {
        None
    }
}

/// The "G" format with `precision` significant digits.
fn general(rounding: TextRounding, x: f32, precision: i32) -> String {
    if let Some(text) = special(x) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(24);
    if x.is_sign_negative() {
        out.push('-');
    }
    let d = rounding.digits(x, precision);
    if d.exp >= precision || d.exp < -4 {
        push_scientific(&mut out, &d);
    } else {
        push_fixed(&mut out, &d);
    }
    out
}

fn x9_with(rounding: TextRounding, x: f32) -> String {
    if let Some(text) = special(x) {
        return text.to_owned();
    }
    let mut d = rounding.digits(x, 7);
    // Nine fractional digits keep `exp + 1` integer digits plus nine more.
    d.round(d.exp + 1 + 9);
    if d.is_zero() {
        return "0".to_owned();
    }
    let mut out = String::with_capacity(24);
    if x.is_sign_negative() {
        out.push('-');
    }
    push_fixed(&mut out, &d);
    out
}

fn nj_with(rounding: TextRounding, x: f32) -> String {
    let mut text = general(rounding, x, 7);
    if !x.is_finite() {
        return text;
    }
    if text.parse::<f32>().ok() != Some(x) {
        text = general(rounding, x, 9);
    }
    if !text.contains(['.', 'E', 'e']) {
        text.push_str(".0");
    }
    text
}

fn push_digits(out: &mut String, digits: &[u8]) {
    out.extend(digits.iter().map(|&d| char::from(b'0' + d)));
}

/// Fixed-point notation of the digits, without an exponent.
fn push_fixed(out: &mut String, d: &Digits) {
    let digits = d.as_slice();
    if d.exp < 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', d.exp.unsigned_abs() as usize - 1));
        push_digits(out, digits);
        return;
    }
    let int_len = d.exp as usize + 1;
    push_digits(out, &digits[..digits.len().min(int_len)]);
    out.extend(std::iter::repeat_n(
        '0',
        int_len.saturating_sub(digits.len()),
    ));
    if digits.len() > int_len {
        out.push('.');
        push_digits(out, &digits[int_len..]);
    }
}

/// `d.dddE+dd`: the exponent of an f32 has at most two digits.
fn push_scientific(out: &mut String, d: &Digits) {
    let digits = d.as_slice();
    push_digits(out, &digits[..1]);
    if digits.len() > 1 {
        out.push('.');
        push_digits(out, &digits[1..]);
    }
    out.push('E');
    out.push(if d.exp < 0 { '-' } else { '+' });
    let exp = d.exp.unsigned_abs() as u8;
    out.push(char::from(b'0' + exp / 10));
    out.push(char::from(b'0' + exp % 10));
}
