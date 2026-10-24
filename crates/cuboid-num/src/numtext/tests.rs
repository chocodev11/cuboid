use proptest::prelude::*;

use super::*;

// ---- spec vectors (interop 3.1) ------------------------------------------------------------

#[test]
fn g7_spec_examples() {
    let cases: [(f32, &str); 10] = [
        (0.1, "0.1"),
        (22.5, "22.5"),
        (-8.0, "-8"),
        (1e-6, "1E-06"),
        (12_345_678.0, "1.234568E+07"),
        (0.623_529_4, "0.6235294"),
        // "0.0001 stays fixed and 0.00001 prints as 1E-05"
        (1e-4, "0.0001"),
        (1e-5, "1E-05"),
        (1_234_567.0, "1234567"),
        (-0.0, "0"),
    ];
    for (x, text) in cases {
        assert_eq!(g7(x), text, "g7({x:e})");
    }
}

#[test]
fn x9_spec_examples() {
    // "5.6568 (stored float 5.65680027), 0.01, 0"
    let stored = 5.656_800_3_f32;
    assert_eq!(stored.to_bits(), 0x40B5_0482);
    let cases: [(f32, &str); 12] = [
        (stored, "5.6568"),
        (0.01, "0.01"),
        (0.0, "0"),
        (-0.0, "0"),
        (1.5, "1.5"),
        (-22.5, "-22.5"),
        (100.0, "100"),
        // The 7 digits are printed in full, then padded with zeros: no exponent form.
        (123_456_792.0, "123456800"),
        // At most nine fractional digits, rounded from the 7 significant digits.
        (0.000_012_345_678, "0.000012346"),
        (6e-10, "0.000000001"),
        (-6e-10, "-0.000000001"),
        (-1e-10, "0"),
    ];
    for (x, text) in cases {
        assert_eq!(x9(x), text, "x9({x:e})");
    }
}

#[test]
fn nj_spec_examples() {
    let cases: [(f32, &str); 8] = [
        (8.0, "8.0"),
        (22.5, "22.5"),
        (0.1, "0.1"),
        (1e-5, "1E-05"),
        (-0.0, "0.0"),
        // 7 digits do not round-trip, so the 9-digit text is used.
        (16_777_216.0, "16777216.0"),
        (1.0 / 3.0, "0.333333343"),
        (f32::MAX, "3.40282347E+38"),
    ];
    for (x, text) in cases {
        assert_eq!(nj(x), text, "nj({x:e})");
    }
}

#[test]
fn nj_exponent_forms_need_no_decimal_place() {
    assert_eq!(nj(1e10), "1E+10");
    assert_eq!(nj(1.5e-7), "1.5E-07");
}

#[test]
fn extreme_values() {
    assert_eq!(g7(f32::MAX), "3.402823E+38");
    assert_eq!(g7(f32::MIN_POSITIVE), "1.175494E-38");
    assert_eq!(g7(f32::from_bits(1)), "1.401298E-45");
    assert_eq!(g7(-f32::from_bits(1)), "-1.401298E-45");
    assert_eq!(x9(f32::MAX), format!("3402823{}", "0".repeat(32)));
    assert_eq!(x9(f32::from_bits(1)), "0");
}

#[test]
fn non_finite_texts() {
    for f in [g7, x9, nj] {
        assert_eq!(f(f32::NAN), "NaN");
        assert_eq!(f(f32::INFINITY), "Infinity");
        assert_eq!(f(f32::NEG_INFINITY), "-Infinity");
    }
}

/// Assumption of the explicit rounding stage (T-NUM-1): the first dropped digit decides, so an
/// exact tie rounds away from zero. 1048576.5 and 0.00048828125 have exactly 8 significant
/// digits ending in 5.
#[test]
fn ties_round_away_from_zero() {
    assert_eq!(g7(1_048_576.5), "1048577");
    assert_eq!(g7(-1_048_576.5), "-1048577");
    assert_eq!(g7(0.000_488_281_25), "0.0004882813");
    // Rounded to 7 digits first, then cut to nine fractional digits.
    assert_eq!(x9(0.000_488_281_25), "0.000488281");
}

/// The two rounding stages disagree on values whose digits after the 15th make a near tie.
#[test]
fn via15_differs_on_near_ties() {
    let x = f32::from_bits(0x36AF_C14F); // 5.2379105e-6
    assert_eq!(general(TextRounding::Exact, x, 7), "5.23791E-06");
    assert_eq!(general(TextRounding::Via15, x, 7), "5.237911E-06");
    let y = f32::from_bits(0x614D_E861); // 2.3739545e20
    assert_eq!(general(TextRounding::Exact, y, 7), "2.373954E+20");
    assert_eq!(general(TextRounding::Via15, y, 7), "2.373955E+20");
    assert_eq!(
        x9_with(TextRounding::Exact, y),
        format!("2373954{}", "0".repeat(14))
    );
    assert_eq!(
        x9_with(TextRounding::Via15, y),
        format!("2373955{}", "0".repeat(14))
    );
}

// ---- reference formatter built on the standard library's exact formatting ------------------

/// Exact significant digits and decimal exponent of `|x|` from the standard library, which
/// prints exact expansions when asked for more digits than exist.
fn std_digits(x: f32) -> (Vec<u8>, i32) {
    let text = format!("{:.130e}", x.abs());
    let (mantissa, exp) = text.split_once('e').unwrap();
    let mut digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    while digits.last() == Some(&0) {
        digits.pop();
    }
    (digits, exp.parse().unwrap())
}

/// Rounds to `keep` digits, a dropped digit of 5 or more rounds up.
fn ref_round(mut digits: Vec<u8>, mut exp: i32, keep: usize) -> (Vec<u8>, i32) {
    if digits.len() > keep {
        let up = digits[keep] >= 5;
        digits.truncate(keep);
        if up {
            let mut i = keep;
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    exp += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
        while digits.last() == Some(&0) {
            digits.pop();
        }
    }
    (digits, exp)
}

fn ref_digits(x: f32, rounding: TextRounding, keep: usize) -> (Vec<u8>, i32) {
    let (digits, exp) = std_digits(x);
    let (digits, exp) = match rounding {
        TextRounding::Exact => (digits, exp),
        TextRounding::Via15 => ref_round(digits, exp, 15),
    };
    ref_round(digits, exp, keep)
}

fn text_of(digits: &[u8]) -> String {
    digits.iter().map(|&d| char::from(b'0' + d)).collect()
}

fn ref_general(x: f32, rounding: TextRounding, precision: i32) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let (digits, exp) = ref_digits(x, rounding, precision as usize);
    let ds = text_of(&digits);
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if exp >= precision || exp < -4 {
        let tail = format!("E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
        if ds.len() > 1 {
            format!("{}.{}{tail}", &ds[..1], &ds[1..])
        } else {
            format!("{ds}{tail}")
        }
    } else if exp < 0 {
        format!("0.{}{ds}", "0".repeat((-exp - 1) as usize))
    } else {
        let int_len = exp as usize + 1;
        if ds.len() <= int_len {
            format!("{ds}{}", "0".repeat(int_len - ds.len()))
        } else {
            format!("{}.{}", &ds[..int_len], &ds[int_len..])
        }
    };
    format!("{sign}{body}")
}

/// X9 through integer arithmetic on units of 1e-9, independent of the digit-array carry logic.
fn ref_x9(x: f32, rounding: TextRounding) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let (digits, exp) = ref_digits(x, rounding, 7);
    let scale = exp - (digits.len() as i32 - 1); // value = digits * 10^scale
    let units = if scale >= -9 {
        format!("{}{}", text_of(&digits), "0".repeat((scale + 9) as usize))
    } else {
        let n: u128 = digits.iter().fold(0, |a, &d| a * 10 + u128::from(d));
        let drop = (-9 - scale) as u32;
        let rounded = if drop > 38 {
            0
        } else {
            (n + 10u128.pow(drop) / 2) / 10u128.pow(drop)
        };
        rounded.to_string()
    };
    let units = format!("{units:0>10}");
    let (int, frac) = units.split_at(units.len() - 9);
    let int = int.trim_start_matches('0');
    let frac = frac.trim_end_matches('0');
    if int.is_empty() && frac.is_empty() {
        return "0".into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let int = if int.is_empty() { "0" } else { int };
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

fn ref_nj(x: f32, rounding: TextRounding) -> String {
    let short = ref_general(x, rounding, 7);
    let mut text = if short.parse::<f32>().ok() == Some(x) {
        short
    } else {
        ref_general(x, rounding, 9)
    };
    if !text.contains(['.', 'E', 'e']) {
        text.push_str(".0");
    }
    text
}

const ROUNDINGS: [TextRounding; 2] = [TextRounding::Exact, TextRounding::Via15];

fn check_against_reference(x: f32) {
    for rounding in ROUNDINGS {
        assert_eq!(
            general(rounding, x, 7),
            ref_general(x, rounding, 7),
            "g7 {rounding:?} {x:e}"
        );
        assert_eq!(
            general(rounding, x, 9),
            ref_general(x, rounding, 9),
            "g9 {rounding:?} {x:e}"
        );
        assert_eq!(
            x9_with(rounding, x),
            ref_x9(x, rounding),
            "x9 {rounding:?} {x:e}"
        );
        assert_eq!(
            nj_with(rounding, x),
            ref_nj(x, rounding),
            "nj {rounding:?} {x:e}"
        );
    }
}

fn finite_nonzero() -> impl Strategy<Value = f32> {
    any::<u32>()
        .prop_map(f32::from_bits)
        .prop_filter("finite and non-zero", |x| x.is_finite() && *x != 0.0)
}

/// Values like the ones documents hold: short decimals and dyadic fractions.
fn model_value() -> impl Strategy<Value = f32> {
    prop_oneof![
        (-2_000_000i32..2_000_000).prop_map(|n| n as f32 / 1000.0),
        (-2_000_000i32..2_000_000).prop_map(|n| n as f32 / 16.0),
        (-2_000_000i32..2_000_000).prop_map(|n| n as f32 / 10_000.0),
        (-1000i32..1000, -30i32..30).prop_map(|(n, e)| n as f32 * 2f32.powi(e)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn matches_reference_on_any_pattern(x in finite_nonzero()) {
        check_against_reference(x);
    }

    #[test]
    fn matches_reference_on_model_values(x in model_value()) {
        check_against_reference(x);
    }

    #[test]
    fn nj_parses_back(x in finite_nonzero()) {
        prop_assert_eq!(nj(x).parse::<f32>().unwrap().to_bits(), x.to_bits());
    }

    #[test]
    fn exact_digits_parse_back(x in finite_nonzero()) {
        let d = Digits::exact(x);
        let text = format!("{}e{}", text_of(d.as_slice()), d.exp - d.len as i32 + 1);
        prop_assert_eq!(text.parse::<f32>().unwrap().to_bits(), x.abs().to_bits());
    }
}

#[test]
fn known_hard_values_match_reference() {
    // Smallest subnormals, the subnormal/normal border, 1.0 and its neighbour, 0.1, the largest
    // float, 2^24 and its predecessor, 2^-24, and the near ties of `via15_differs_on_near_ties`.
    #[rustfmt::skip]
    let patterns = [
        0x0000_0001, 0x0000_0002, 0x0000_0003, 0x007F_FFFF, 0x0080_0000, 0x0080_0001, 0x3F80_0000,
        0x3F80_0001, 0x3DCC_CCCD, 0x7F7F_FFFF, 0x4B80_0000, 0x4B7F_FFFF, 0x3380_0000, 0x36AF_C14F,
        0x614D_E861, 0x651F_8DB3, 0x095F_9550, 0x0B0B_BD52,
    ];
    for bits in patterns {
        for sign in [0, 0x8000_0000] {
            let x = f32::from_bits(bits | sign);
            if x != 0.0 {
                check_against_reference(x);
            }
        }
    }
    // Every binade: smallest, largest and three scattered mantissas.
    for exponent in 0..=254u32 {
        for mantissa in [0, 1, 0x40_0000, 0x7F_FFFF, 0x2A_5A5A, 0x15_3C3C, 0x6F_0F0F] {
            let x = f32::from_bits((exponent << 23) | mantissa);
            if x != 0.0 {
                check_against_reference(x);
            }
        }
    }
}

/// The digits are the exact binary value: they agree with the standard library's exact
/// expansion, and its correctly rounded 7-digit output wherever no tie is involved.
#[test]
fn seven_digits_agree_with_std_rounding() {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..20_000 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let x = f32::from_bits((state >> 32) as u32);
        if !x.is_finite() || x == 0.0 {
            continue;
        }
        let (digits, exp) = std_digits(x);
        let is_tie = digits.len() == 8 && digits[7] == 5;
        let std_text = format!("{:.6e}", x.abs());
        let (mantissa, std_exp) = std_text.split_once('e').unwrap();
        let std_rounded: Vec<u8> = {
            let mut d: Vec<u8> = mantissa
                .bytes()
                .filter(u8::is_ascii_digit)
                .map(|b| b - b'0')
                .collect();
            while d.last() == Some(&0) {
                d.pop();
            }
            d
        };
        let mut d = Digits::exact(x);
        assert_eq!(
            (d.as_slice(), d.exp),
            (digits.as_slice(), exp),
            "exact {x:e}"
        );
        if is_tie {
            continue;
        }
        d.round(7);
        assert_eq!(
            (d.as_slice(), d.exp),
            (std_rounded.as_slice(), std_exp.parse().unwrap()),
            "{x:e}"
        );
    }
}

// ---- NJ round trip (acceptance: sampled in CI, exhaustive in the nightly job) --------------

fn assert_nj_round_trip(bits: u32) {
    let x = f32::from_bits(bits);
    if !x.is_finite() || x == 0.0 {
        return;
    }
    let text = nj(x);
    let back = text
        .parse::<f32>()
        .unwrap_or_else(|_| panic!("{text} does not parse"));
    assert_eq!(back.to_bits(), bits, "{text} for bits {bits:08x}");
}

#[test]
fn nj_round_trip_sampled() {
    for bits in (0..=u32::MAX).step_by(16_411) {
        assert_nj_round_trip(bits);
    }
    // Every binade with its edge mantissas.
    for sign in [0, 0x8000_0000u32] {
        for exponent in 0..=254u32 {
            for mantissa in [
                0, 1, 2, 0x7F_FFFE, 0x7F_FFFF, 0x40_0000, 0x55_5555, 0x2A_AAAA,
            ] {
                assert_nj_round_trip(sign | (exponent << 23) | mantissa);
            }
        }
    }
}

/// All 2^32 patterns. About 2 minutes on 16 cores with `--release`.
///
/// `cargo test -p cuboid-num --release -- --ignored nj_round_trip_exhaustive`
#[test]
#[ignore = "exhaustive, for the nightly job"]
fn nj_round_trip_exhaustive() {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for first in 0..threads {
            scope.spawn(move || {
                for bits in (first as u32..=u32::MAX).step_by(threads) {
                    assert_nj_round_trip(bits);
                }
            });
        }
    });
}

// ---- parsing (tools 1.6, 8.10) --------------------------------------------------------------

#[test]
fn parse_float_accepts_the_invariant_forms() {
    let cases: [(&str, f32); 14] = [
        ("0", 0.0),
        ("7", 7.0),
        ("-7", -7.0),
        ("+7", 7.0),
        ("7.5", 7.5),
        ("7.", 7.0),
        (".5", 0.5),
        ("-.5", -0.5),
        ("0.1", 0.1),
        ("1e3", 1000.0),
        ("1.5E-2", 0.015),
        ("  22.5 ", 22.5),
        ("\t3\r\n", 3.0),
        ("5\0", 5.0),
    ];
    for (text, value) in cases {
        assert_eq!(parse_or_zero_f32(text), value, "{text:?}");
    }
}

#[test]
fn parse_float_reads_zero_on_failure() {
    // Candidates whose permissive reading would not be 0: a comma is not a decimal point and,
    // unlike the OBJ importer, no thousands separator is accepted.
    for text in [
        "",
        " ",
        "-",
        ".",
        "abc",
        "5,5",
        "1,5",
        "1,000.5",
        "5 5",
        "--5",
        "5-",
        "1e",
        "1e+",
        "e5",
        "0x10",
        "NaN",
        "Infinity",
        "-Infinity",
        "inf",
        "1e999",
        "-1e999",
        "3.5e38",
        "٣",
    ] {
        assert_eq!(parse_or_zero_f32(text), 0.0, "{text:?}");
    }
    // Large but representable values still parse.
    assert_eq!(parse_or_zero_f32("3.4e38"), 3.4e38);
}

#[test]
fn parse_float_narrows_through_f64() {
    // Just above the midpoint of 16777216 and 16777218. Read as f64 it is exactly the midpoint,
    // which then ties to even (the UNVERIFIED reading of the original); a direct parse would
    // give 16777218.
    let halfway = "16777217.0000000001";
    assert_eq!(parse_or_zero_f32(halfway), 16_777_216.0);
}

#[test]
fn parse_int_follows_the_integer_style() {
    // tools 8.10: "empty text, 7.5, 7. and 7,5 all read 0; surrounding spaces and a leading
    // sign are accepted, -7 reads -7".
    let cases: [(&str, i32); 12] = [
        ("", 0),
        ("7.5", 0),
        ("7.", 0),
        ("7,5", 0),
        (" 7 ", 7),
        ("+7", 7),
        ("-7", -7),
        ("007", 7),
        ("2147483647", i32::MAX),
        ("-2147483648", i32::MIN),
        ("2147483648", 0),
        ("1e3", 0),
    ];
    for (text, value) in cases {
        assert_eq!(parse_or_zero_i32(text), value, "{text:?}");
    }
    for text in ["-", "+", "- 7", "7-", "abc", "--7", "０"] {
        assert_eq!(parse_or_zero_i32(text), 0, "{text:?}");
    }
}
