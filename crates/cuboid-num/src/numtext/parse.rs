//! "Try parse, 0 on failure" with the invariant culture (tools 1.6).

/// White space that surrounds a number: the characters 0x09 to 0x0D and the space. After the
/// number the runtime also tolerates trailing NUL characters.
fn is_white(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ')
}

fn trim(s: &str) -> &str {
    s.trim_start_matches(is_white)
        .trim_end_matches(|c| is_white(c) || c == '\0')
}

/// Parses an f32 the way the runtime's float parser does with the invariant culture: optional
/// surrounding white space, an optional sign, digits with an optional `.` and an optional
/// exponent. The text is read as an f64 and then narrowed (**UNVERIFIED** that the original
/// narrows too; the two readings differ only for texts of 16 or more significant digits). A thousands
/// separator, a comma, `NaN`, `Infinity` and values beyond the f32 range fail.
pub(super) fn float(s: &str) -> Option<f32> {
    let t = trim(s);
    let bytes = t.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };

    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let int_digits = digits(i);
    i += int_digits;
    let mut frac_digits = 0;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = digits(i);
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        i += usize::from(matches!(bytes.get(i), Some(b'+' | b'-')));
        let exp_digits = digits(i);
        if exp_digits == 0 {
            return None;
        }
        i += exp_digits;
    }
    if i != bytes.len() {
        return None;
    }
    let narrowed = t.parse::<f64>().ok()? as f32;
    narrowed.is_finite().then_some(narrowed)
}

/// Parses an i32 with the integer style of the runtime: optional surrounding white space, an
/// optional sign and at least one digit. Anything else, including `7.5`, `7,5` and overflow,
/// fails.
pub(super) fn int(s: &str) -> Option<i32> {
    trim(s).parse().ok()
}
