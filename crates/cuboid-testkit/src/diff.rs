//! Diff helpers for the tiers P0, P1 and P4 of architecture 19.1. The raster tiers P2 and P3 are
//! in [`crate::raster`].
//!
//! Every helper returns `Result<(), Mismatch>`; a failing check explains where the values start to
//! differ.

use std::fmt::Debug;

/// Why two values differ: a readable, possibly multi-line explanation.
#[derive(thiserror::Error)]
#[error("{0}")]
pub struct Mismatch(String);

impl Mismatch {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

debug_is_display!(Mismatch);

/// Accepted deviation of a number: within `abs`, or within `rel` of the larger magnitude.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    pub abs: f64,
    pub rel: f64,
}

impl Tolerance {
    /// Identical values only.
    pub const EXACT: Self = Self { abs: 0.0, rel: 0.0 };

    /// Rectangles of the UI tiers: 1/64 px.
    ///
    /// spec: arch 19.1
    pub const RECT: Self = Self {
        abs: 1.0 / 64.0,
        rel: 0.0,
    };

    /// Euler angles in degrees: 1e-4 degrees at any magnitude, no relative part.
    ///
    /// spec: native 4.3.2
    pub const ANGLE: Self = Self {
        abs: 1e-4,
        rel: 0.0,
    };

    /// The other transcendental paths, positions after a rotation about a pivot and camera
    /// quaternion components: 1e-5 relative. The absolute floor of 1e-6 (about 8 `f32` ulps at
    /// 1.0) lets a result that is zero in one implementation and rounding noise in the other
    /// agree. A comparison of large operands whose result cancels to near zero needs a floor of
    /// its own: `Tolerance { abs, ..Tolerance::RELATIVE }`.
    ///
    /// spec: arch 19.1
    pub const RELATIVE: Self = Self {
        abs: 1e-6,
        rel: 1e-5,
    };

    /// NaN matches NaN, an infinity matches the same infinity.
    pub fn accepts(self, expected: f64, actual: f64) -> bool {
        if expected.is_nan() || actual.is_nan() {
            return expected.is_nan() && actual.is_nan();
        }
        if expected.is_infinite() || actual.is_infinite() {
            return expected.to_bits() == actual.to_bits();
        }
        let delta = (expected - actual).abs();
        delta <= self.abs || delta <= self.rel * expected.abs().max(actual.abs())
    }
}

/// P0: identical bytes.
///
/// spec: arch 19.1
pub fn bytes(expected: &[u8], actual: &[u8]) -> Result<(), Mismatch> {
    if expected == actual {
        return Ok(());
    }
    let common = expected.len().min(actual.len());
    let pairs = || expected.iter().zip(actual);
    let first = pairs().position(|(e, a)| e != a).unwrap_or(common);
    let differing = pairs().filter(|(e, a)| e != a).count() + expected.len().abs_diff(actual.len());
    Err(Mismatch::new(format!(
        "bytes differ: expected {} bytes, actual {} bytes, first difference at offset {first} \
         (0x{first:x}), {differing} bytes differ\n  expected: {}\n  actual:   {}",
        expected.len(),
        actual.len(),
        hex_window(expected, first),
        hex_window(actual, first),
    )))
}

/// Up to 8 bytes either side of `at`, the byte at `at` in brackets. `at` never exceeds the length.
fn hex_window(data: &[u8], at: usize) -> String {
    let start = at.saturating_sub(8);
    let end = (at + 9).min(data.len());
    let mut cells: Vec<String> = data[start..end]
        .iter()
        .enumerate()
        .map(|(i, byte)| {
            if start + i == at {
                format!("[{byte:02x}]")
            } else {
                format!("{byte:02x}")
            }
        })
        .collect();
    if at == data.len() {
        cells.push("[end]".into());
    }
    format!("{start:#06x}: {}", cells.join(" "))
}

/// P0: identical `f32` bit patterns. Tells `0.0` from `-0.0` and NaN payloads apart.
///
/// spec: arch 19.1
pub fn f32_bits(expected: &[f32], actual: &[f32]) -> Result<(), Mismatch> {
    same_len("f32 values", expected.len(), actual.len())?;
    let Some(at) = expected
        .iter()
        .zip(actual)
        .position(|(e, a)| e.to_bits() != a.to_bits())
    else {
        return Ok(());
    };
    let (e, a) = (expected[at], actual[at]);
    Err(Mismatch::new(format!(
        "f32 bit patterns differ at index {at}: expected {e:?} (0x{:08x}), actual {a:?} (0x{:08x})",
        e.to_bits(),
        a.to_bits(),
    )))
}

/// P0, text: identical characters, line endings and final newline included (`cubik29` writes
/// CRLF, and several formats end without a newline). The report goes by line and names a
/// difference in the ending alone.
///
/// spec: arch 19.1
pub fn text(expected: &str, actual: &str) -> Result<(), Mismatch> {
    /// The lines of `text` with their terminators.
    fn lines(text: &str) -> impl Iterator<Item = &str> {
        text.split_inclusive('\n')
    }
    let Some(at) = first_difference(lines(expected), lines(actual)) else {
        return Ok(());
    };
    let ending_only = matches!((at.expected, at.actual), (Some(e), Some(a)) if body(e) == body(a));
    Err(Mismatch::new(format!(
        "text differs at line {} (expected {} lines, actual {} lines)\n  expected: {}\n  \
         actual:   {}{}",
        at.index + 1,
        lines(expected).count(),
        lines(actual).count(),
        show(at.expected),
        show(at.actual),
        if ending_only {
            "\n  the lines differ only in their ending (CRLF, LF, or none at the end of the text)"
        } else {
            ""
        },
    )))
}

/// A line without its terminator: LF, CRLF or nothing at the end of the text.
fn body(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// P1: numbers within `tol` of each other, pairwise. Rectangles are compared as flat lists of
/// their four values with [`Tolerance::RECT`]. Integers wider than 32 bits do not convert to `f64`
/// without loss: compare them with [`sequence`].
///
/// spec: arch 19.1
pub fn numbers<T: Copy + Into<f64>>(
    expected: &[T],
    actual: &[T],
    tol: Tolerance,
) -> Result<(), Mismatch> {
    same_len("numbers", expected.len(), actual.len())?;
    let mut outside = expected
        .iter()
        .zip(actual)
        .enumerate()
        .filter(|&(_, (&e, &a))| !tol.accepts(e.into(), a.into()));
    let Some((first, (&e, &a))) = outside.next() else {
        return Ok(());
    };
    let count = 1 + outside.count();
    let (e, a): (f64, f64) = (e.into(), a.into());
    Err(Mismatch::new(format!(
        "{count} of {} numbers outside {tol:?}; first at index {first}: expected {e}, actual {a} \
         (difference {})",
        expected.len(),
        (e - a).abs(),
    )))
}

/// P1: the same members in any order, each as often as expected (selection sets, hit lists).
///
/// spec: arch 19.1
pub fn set<T: Ord + Debug>(expected: &[T], actual: &[T]) -> Result<(), Mismatch> {
    fn sorted<T: Ord>(items: &[T]) -> Vec<&T> {
        let mut refs: Vec<&T> = items.iter().collect();
        refs.sort_unstable();
        refs
    }
    let (e, a) = (sorted(expected), sorted(actual));
    if e == a {
        return Ok(());
    }
    let (mut missing, mut unexpected) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < e.len() && j < a.len() {
        match e[i].cmp(a[j]) {
            std::cmp::Ordering::Less => {
                missing.push(e[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                unexpected.push(a[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    missing.extend(&e[i..]);
    unexpected.extend(&a[j..]);
    Err(Mismatch::new(format!(
        "sets differ: missing {missing:?}, unexpected {unexpected:?}"
    )))
}

/// P4: the same items in the same order (dialogs, labels, statuses, undo labels, selection).
///
/// spec: arch 19.1
pub fn sequence<T: PartialEq + Debug>(expected: &[T], actual: &[T]) -> Result<(), Mismatch> {
    let Some(at) = first_difference(expected.iter(), actual.iter()) else {
        return Ok(());
    };
    Err(Mismatch::new(format!(
        "sequences differ at index {} (expected {} items, actual {} items)\n  expected: {}\n  \
         actual:   {}",
        at.index,
        expected.len(),
        actual.len(),
        show(at.expected),
        show(at.actual),
    )))
}

/// P4, structural: canonical dumps ([`crate::dump`]) that are equal except for the paths in
/// `ignore`, the documented normalisations of semantic round trips. An ignored path covers itself
/// and everything below it: `vp` covers `vp.ctl.p.x`, `el[0]` covers `el[0].f[1].t`. Paths are
/// written as the dump prints them, keys escaped. The header line of a container outside the
/// ignored paths keeps its `len`, so the keys of that container must still match.
///
/// spec: arch 19.1
/// spec: native 10.2
pub fn dumps(expected: &str, actual: &str, ignore: &[&str]) -> Result<(), Mismatch> {
    /// The lines that are not ignored, with their numbers in the whole dump.
    fn kept<'a>(dump: &'a str, ignore: &'a [&str]) -> impl Iterator<Item = (usize, &'a str)> {
        dump.lines()
            .enumerate()
            .filter(move |&(_, line)| !ignored(line, ignore))
    }
    fn numbered(line: Option<(usize, &str)>) -> String {
        line.map_or_else(
            || "<end>".into(),
            |(index, line)| format!("line {}: {line}", index + 1),
        )
    }
    let (mut want, mut got) = (kept(expected, ignore), kept(actual, ignore));
    loop {
        let (e, a) = (want.next(), got.next());
        match (e, a) {
            (None, None) => return Ok(()),
            (Some((_, same)), Some((_, other))) if same == other => {}
            _ => {
                return Err(Mismatch::new(format!(
                    "dumps differ\n  expected {}\n  actual   {}",
                    numbered(e),
                    numbered(a),
                )));
            }
        }
    }
}

/// Whether the path of a dump line (`path: type = value`) lies at or below an ignored path.
fn ignored(line: &str, ignore: &[&str]) -> bool {
    let path = line.split_once(": ").map_or(line, |(path, _)| path);
    ignore.iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
    })
}

fn same_len(what: &str, expected: usize, actual: usize) -> Result<(), Mismatch> {
    if expected == actual {
        return Ok(());
    }
    Err(Mismatch::new(format!(
        "expected {expected} {what}, actual {actual}"
    )))
}

struct Difference<T> {
    index: usize,
    expected: Option<T>,
    actual: Option<T>,
}

/// The first position where the iterators differ; `None` as an item means the iterator ended.
fn first_difference<T: PartialEq>(
    mut expected: impl Iterator<Item = T>,
    mut actual: impl Iterator<Item = T>,
) -> Option<Difference<T>> {
    let mut index = 0;
    loop {
        match (expected.next(), actual.next()) {
            (None, None) => return None,
            (e, a) if e != a => {
                return Some(Difference {
                    index,
                    expected: e,
                    actual: a,
                });
            }
            _ => index += 1,
        }
    }
}

fn show<T: Debug>(item: Option<T>) -> String {
    item.map_or_else(|| "<end>".into(), |item| format!("{item:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump::Dump;

    fn message(result: Result<(), Mismatch>) -> String {
        result.expect_err("the values differ").to_string()
    }

    #[test]
    fn bytes_name_the_first_differing_offset() {
        assert!(bytes(&[1, 2, 3], &[1, 2, 3]).is_ok());
        let msg = message(bytes(&[1, 2, 3, 4], &[1, 2, 9, 4]));
        assert!(msg.contains("first difference at offset 2 (0x2)"), "{msg}");
        assert!(msg.contains("1 bytes differ"), "{msg}");
        assert!(msg.contains("01 02 [03] 04"), "{msg}");
        assert!(msg.contains("01 02 [09] 04"), "{msg}");
    }

    #[test]
    fn bytes_of_different_length_differ_at_the_end_of_the_shorter() {
        let msg = message(bytes(&[1, 2], &[1, 2, 3]));
        assert!(msg.contains("first difference at offset 2"), "{msg}");
        assert!(msg.contains("01 02 [end]"), "{msg}");
        assert!(msg.contains("01 02 [03]"), "{msg}");
    }

    #[test]
    fn f32_bits_tell_signed_zeros_and_nan_payloads_apart() {
        let quiet_nan = f32::from_bits(0x7fc0_0000);
        let other_nan = f32::from_bits(0x7fc0_0001);
        assert!(f32_bits(&[1.5, quiet_nan], &[1.5, quiet_nan]).is_ok());
        assert!(f32_bits(&[0.0], &[-0.0]).is_err());
        let msg = message(f32_bits(&[quiet_nan], &[other_nan]));
        assert!(
            msg.contains("0x7fc00000") && msg.contains("0x7fc00001"),
            "{msg}"
        );
        assert!(f32_bits(&[1.0], &[1.0, 2.0]).is_err());
    }

    #[test]
    fn text_is_exact_down_to_line_endings_and_the_final_newline() {
        for same in ["", "a\nb\n", "a\r\nb", "a\nb\r\n\n"] {
            assert!(text(same, same).is_ok(), "{same:?}");
        }
        for (expected, actual) in [
            ("a\nb\n", "a\r\nb\n"),
            ("a\r\nb\r\n", "a\nb\r\n"),
            ("a\nb\n", "a\nb"),
            ("a\nb", "a\nb\n"),
            ("a\n", "a\n\n"),
            ("a \n", "a\n"),
        ] {
            assert!(
                text(expected, actual).is_err(),
                "{expected:?} against {actual:?}"
            );
        }
    }

    #[test]
    fn text_names_a_difference_in_the_ending_alone() {
        let crlf = message(text("a\r\nb\r\n", "a\nb\r\n"));
        assert!(
            crlf.contains("line 1")
                && crlf.contains("\"a\\r\\n\"")
                && crlf.contains("only in their ending"),
            "{crlf}"
        );
        let last = message(text("a\nb\n", "a\nb"));
        assert!(
            last.contains("line 2") && last.contains("only in their ending"),
            "{last}"
        );
        let content = message(text("a\nb\n", "a\nc\n"));
        assert!(!content.contains("ending"), "{content}");
    }

    #[test]
    fn text_reports_the_first_differing_line() {
        let msg = message(text("one\ntwo\nthree\n", "one\ntwo\ntree\n"));
        assert!(msg.contains("line 3"), "{msg}");
        assert!(
            msg.contains("\"three\\n\"") && msg.contains("\"tree\\n\""),
            "{msg}"
        );
        let short = message(text("one\ntwo\n", "one\n"));
        assert!(
            short.contains("line 2") && short.contains("<end>"),
            "{short}"
        );
    }

    #[test]
    fn angles_are_compared_within_the_spec_limit_at_any_magnitude() {
        let t = Tolerance::ANGLE;
        assert!(t.accepts(10.0, 10.0 + 9e-5));
        assert!(!t.accepts(10.0, 10.0 + 2e-4));
        // a relative bound would accept these: 2e-4 of 300 is 6.7e-7 relative
        assert!(t.accepts(300.0, 300.00009));
        assert!(!t.accepts(300.0, 300.0002));
        assert!(!t.accepts(359.99, 359.9902));
        assert!(!t.accepts(0.0, 2e-4));
    }

    #[test]
    fn transcendental_values_are_compared_relatively_with_a_tiny_floor() {
        let t = Tolerance::RELATIVE;
        assert!(t.accepts(1000.0, 1000.009));
        assert!(!t.accepts(1000.0, 1000.02));
        // a unitless quaternion component gets no absolute slack of 1e-4
        assert!(t.accepts(0.5, 0.500_004));
        assert!(!t.accepts(0.5, 0.500_09));
        assert!(!t.accepts(-0.5, -0.500_09));
        // zero against rounding noise agrees, a real difference does not
        assert!(t.accepts(0.0, 5e-7));
        assert!(!t.accepts(0.0, 1e-5));
    }

    #[test]
    fn rectangles_are_compared_within_a_sixty_fourth() {
        assert!(Tolerance::RECT.accepts(3.0, 3.0 + 1.0 / 64.0));
        assert!(!Tolerance::RECT.accepts(3.0, 3.0 + 1.0 / 32.0));
        assert!(!Tolerance::RECT.accepts(3000.0, 3000.0 + 1.0 / 32.0));
    }

    #[test]
    fn tolerance_handles_zeros_and_non_finite_values() {
        let exact = Tolerance::EXACT;
        assert!(exact.accepts(0.0, -0.0));
        assert!(exact.accepts(f64::NAN, f64::NAN));
        assert!(!exact.accepts(f64::NAN, 0.0));
        assert!(exact.accepts(f64::INFINITY, f64::INFINITY));
        assert!(!exact.accepts(f64::INFINITY, f64::NEG_INFINITY));
        assert!(!Tolerance::RELATIVE.accepts(f64::INFINITY, f64::MAX));
    }

    #[test]
    fn numbers_count_the_failures_and_name_the_first() {
        assert!(numbers(&[1.0_f32, 2.0], &[1.0, 2.0 + 1.0 / 128.0], Tolerance::RECT).is_ok());
        let msg = message(numbers(
            &[1.0_f32, 2.0, 3.0, 4.0],
            &[1.0, 2.5, 3.0, 5.0],
            Tolerance::RECT,
        ));
        assert!(msg.contains("2 of 4 numbers"), "{msg}");
        assert!(msg.contains("index 1: expected 2, actual 2.5"), "{msg}");
        assert!(numbers(&[1_i32], &[1, 2], Tolerance::EXACT).is_err());
    }

    #[test]
    fn set_ignores_order_but_not_multiplicity() {
        assert!(set(&[3, 1, 2], &[1, 2, 3]).is_ok());
        let msg = message(set(&[1, 2, 2, 5], &[2, 1, 5, 7]));
        assert!(
            msg.contains("missing [2]") && msg.contains("unexpected [7]"),
            "{msg}"
        );
    }

    /// A model-like dump; the arguments are the values the tests change.
    fn model(sp: i32, desc: &str, pos: f32, tint: Option<&str>) -> String {
        let mut dump = Dump::new();
        dump.doc("mp", |d| {
            d.i32("sp", sp);
            d.doc("g", |d| d.str("d", desc));
        });
        dump.doc("vp", |d| d.doc("ctl", |d| d.f32("x", pos)));
        dump.array("el", |d| {
            d.doc(0, |d| {
                d.array("f", |d| match tint {
                    Some(tint) => d.str(1, tint),
                    None => d.null(1),
                });
                d.str("n", "Cube");
            });
        });
        dump.i32("elf", 1);
        dump.finish()
    }

    #[test]
    fn dumps_skip_the_ignored_paths_and_everything_below_them() {
        let expected = model(1, "a", 0.0, None);
        let actual = model(2, "b", 1.0, Some("x"));
        let ignore = ["mp.sp", "mp.g.d", "vp", "el[0].f"];
        dumps(&expected, &actual, &ignore).unwrap();

        // the paths not listed still count
        let msg = message(dumps(&expected, &actual, &["mp.sp", "mp.g.d", "vp"]));
        assert!(msg.contains("el[0].f[1]"), "{msg}");

        // `el` covers its indexed children but not the sibling path `elf`
        let renamed = expected.replace("elf: i32 = 1", "elf: i32 = 2");
        assert!(dumps(&expected, &renamed, &["el"]).is_err());
    }

    #[test]
    fn dumps_compare_the_length_of_every_container_outside_the_ignored_paths() {
        let mut with_key = Dump::new();
        with_key.doc("mp", |d| {
            d.i32("sp", 1);
            d.i32("extra", 2);
        });
        let mut without = Dump::new();
        without.doc("mp", |d| d.i32("sp", 1));
        let (with_key, without) = (with_key.finish(), without.finish());
        assert!(dumps(&with_key, &without, &["mp.sp"]).is_err());
        // ignoring the container itself leaves nothing to compare
        dumps(&with_key, &without, &["mp"]).unwrap();
    }

    #[test]
    fn dumps_report_the_lines_of_the_whole_dumps() {
        let expected = "ignored: i32 = 1\nkept: i32 = 2\nsame: i32 = 3\nsub: i32 = 4\n";
        let actual = "ignored: i32 = 5\nignored.deep: i32 = 6\nkept: i32 = 2\nsame: i32 = 3\n\
                      sub: i32 = 9\n";
        let msg = message(dumps(expected, actual, &["ignored"]));
        assert!(
            msg.contains("expected line 4: sub: i32 = 4")
                && msg.contains("actual   line 5: sub: i32 = 9"),
            "{msg}"
        );
        let msg = message(dumps(expected, "kept: i32 = 2\n", &["ignored"]));
        assert!(
            msg.contains("expected line 3: same: i32 = 3") && msg.contains("actual   <end>"),
            "{msg}"
        );
    }

    #[test]
    fn dumps_match_ignored_paths_as_printed_with_escaped_keys() {
        let build = |value: i32| {
            let mut dump = Dump::new();
            dump.i32("a.b", value);
            dump.doc("a", |d| d.i32("b", 1));
            dump.i32("x y", 1);
            dump.finish()
        };
        let (one, two) = (build(1), build(2));
        // `a` is the document, not the first segment of the key `a.b`
        assert!(dumps(&one, &two, &["a"]).is_err());
        dumps(&one, &two, &["a\\.b"]).unwrap();
        // the space of a key does not end the path early
        let spaced = one.replace("x\\ y: i32 = 1", "x\\ y: i32 = 9");
        assert!(dumps(&one, &spaced, &["x"]).is_err());
        dumps(&one, &spaced, &["x\\ y"]).unwrap();
    }

    #[test]
    fn sequence_reports_the_first_divergence() {
        assert!(sequence(&["Save", "Open"], &["Save", "Open"]).is_ok());
        let msg = message(sequence(&["Save", "Open"], &["Save", "Quit"]));
        assert!(
            msg.contains("index 1") && msg.contains("\"Open\"") && msg.contains("\"Quit\""),
            "{msg}"
        );
        let msg = message(sequence(&["Save"], &["Save", "Quit"]));
        assert!(msg.contains("index 1") && msg.contains("<end>"), "{msg}");
    }
}
