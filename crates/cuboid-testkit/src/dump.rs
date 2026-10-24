//! Canonical text dumps for goldens and readable diffs (architecture 19.3).
//!
//! One line per entry, in the order written: `path: type = value`. A document or an array starts
//! with a header line, `path: doc len=3` or `path: array len=0`, where `len` counts its entries,
//! so an empty container differs from a missing one. The width is part of the type: `i8`, `i16`,
//! `i32` and `i64` are the NBT byte, short, int and long (BSON int32 and int64 are `i32` and
//! `i64`), `f32` and `f64` the floats and doubles. The other types are `doc`, `array`, `null`,
//! `bool`, `str` and `bytes`.
//!
//! Integers print in decimal, floats as their hex bit pattern followed by the decimal for the
//! reader, strings in double quotes and binary values as their length and SHA-256. A path joins
//! keys with dots and attaches indices as `el[3]`. In a key a backslash escapes `\`, `.`, `[`, `]`
//! and a space, and control characters print as `\n`, `\t` or `\u{1}`, so every entry is one line
//! and its path ends at the first `: `.
//!
//! spec: native 10.2

use std::fmt::{self, Write as _};
use std::mem;

use sha2::{Digest, Sha256};

/// Where a value sits in its container: under a key (any string reference) or at an array index
/// (`usize`).
pub trait At {
    /// Appends the segment to a path: a key with its leading dot, an index in brackets.
    fn append_to(self, path: &mut String);
}

impl<S: AsRef<str> + ?Sized> At for &S {
    fn append_to(self, path: &mut String) {
        path.push('.');
        escape(path, self.as_ref(), KEY_SPECIAL);
    }
}

impl At for usize {
    fn append_to(self, path: &mut String) {
        let _ = write!(path, "[{self}]");
    }
}

/// The characters of a key that a backslash escapes besides the backslash itself.
const KEY_SPECIAL: &[char] = &['.', '[', ']', ' '];

/// Accumulates the lines of a canonical dump. Writing to a `String` cannot fail, so the results
/// of `write!` are dropped throughout.
#[derive(Default)]
pub struct Dump {
    text: String,
    /// The path of the open container, each key with its leading dot (see [`shown`]).
    path: String,
    /// The entries written so far in the open container.
    entries: usize,
}

impl Dump {
    pub fn new() -> Self {
        Self::default()
    }

    /// A document: its header line, then the entries that `body` writes below it.
    pub fn doc(&mut self, at: impl At, body: impl FnOnce(&mut Self)) {
        self.container(at, "doc", body);
    }

    /// An array: its header line, then the entries that `body` writes below it at indices.
    pub fn array(&mut self, at: impl At, body: impl FnOnce(&mut Self)) {
        self.container(at, "array", body);
    }

    pub fn null(&mut self, at: impl At) {
        self.line(at, "null");
        self.text.push('\n');
    }

    pub fn bool(&mut self, at: impl At, value: bool) {
        self.value(at, "bool", format_args!("{value}"));
    }

    /// NBT byte.
    pub fn i8(&mut self, at: impl At, value: i8) {
        self.value(at, "i8", format_args!("{value}"));
    }

    /// NBT short.
    pub fn i16(&mut self, at: impl At, value: i16) {
        self.value(at, "i16", format_args!("{value}"));
    }

    /// BSON int32, NBT int.
    pub fn i32(&mut self, at: impl At, value: i32) {
        self.value(at, "i32", format_args!("{value}"));
    }

    /// BSON int64, NBT long.
    pub fn i64(&mut self, at: impl At, value: i64) {
        self.value(at, "i64", format_args!("{value}"));
    }

    pub fn f32(&mut self, at: impl At, value: f32) {
        let bits = value.to_bits();
        self.value(at, "f32", format_args!("0x{bits:08x} ({value:?})"));
    }

    /// BSON double.
    pub fn f64(&mut self, at: impl At, value: f64) {
        let bits = value.to_bits();
        self.value(at, "f64", format_args!("0x{bits:016x} ({value:?})"));
    }

    /// Strings print in double quotes; `"`, `\` and control characters are escaped.
    pub fn str(&mut self, at: impl At, value: &str) {
        self.line(at, "str");
        self.text.push_str(" = \"");
        escape(&mut self.text, value, &['"']);
        self.text.push_str("\"\n");
    }

    pub fn bytes(&mut self, at: impl At, value: &[u8]) {
        let (len, sha256) = (value.len(), sha256_hex(value));
        self.value(at, "bytes", format_args!("len={len} sha256={sha256}"));
    }

    pub fn finish(self) -> String {
        self.text
    }

    /// Writes the header line of a container and then its entries. The line is complete only
    /// after the body ran, because `len` is the number of entries the body wrote.
    fn container(&mut self, at: impl At, ty: &str, body: impl FnOnce(&mut Self)) {
        let mark = self.enter(at);
        let _ = write!(self.text, "{}: {ty} len=", shown(&self.path));
        let len_at = self.text.len();
        self.text.push('\n');
        let outer = mem::take(&mut self.entries);
        body(self);
        let len = mem::replace(&mut self.entries, outer);
        self.path.truncate(mark);
        self.text.insert_str(len_at, &len.to_string());
    }

    fn value(&mut self, at: impl At, ty: &str, value: fmt::Arguments<'_>) {
        self.line(at, ty);
        let _ = writeln!(self.text, " = {value}");
    }

    /// Starts the line `path: ty`; the caller completes it.
    fn line(&mut self, at: impl At, ty: &str) {
        let mark = self.enter(at);
        let _ = write!(self.text, "{}: {ty}", shown(&self.path));
        self.path.truncate(mark);
    }

    /// Counts the entry in its container and extends the open path by `at`. Returns the length
    /// to truncate the path to when the entry is done.
    fn enter(&mut self, at: impl At) -> usize {
        self.entries += 1;
        let mark = self.path.len();
        at.append_to(&mut self.path);
        mark
    }
}

/// A path as printed: without the dot of its first key. The dot of an empty first key stays, so
/// that it is not mistaken for the root or for an index.
fn shown(path: &str) -> &str {
    match path.strip_prefix('.') {
        Some(key) if !key.is_empty() && !key.starts_with(['.', '[']) => key,
        _ => path,
    }
}

/// The SHA-256 of `data` as 64 lower-case hex digits.
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Appends `text` with a backslash before `\` and the characters of `special`, and control
/// characters as `\n`, `\r`, `\t` or `\u{..}`.
fn escape(out: &mut String, text: &str, special: &[char]) {
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c if c == '\\' || special.contains(&c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dumped(build: impl FnOnce(&mut Dump)) -> String {
        let mut dump = Dump::new();
        build(&mut dump);
        dump.finish()
    }

    #[test]
    fn lines_follow_the_documented_format() {
        let text = dumped(|d| {
            d.doc("mp", |d| {
                d.doc("g", |d| {
                    d.str("n", "Untitled");
                    d.i32("o", -3);
                });
            });
            d.f32("zoom", 1.5);
            d.f64("pos", -1.0);
            d.array("el", |d| {
                d.doc(0, |d| {
                    d.bool("hidden", false);
                    d.array("f", |d| d.doc(0, |d| d.null("t")));
                });
            });
            d.array("tx", |d| d.doc(0, |d| d.bytes("r", b"abc")));
            d.array("m", |d| d.array(0, |d| d.i32(0, 1)));
        });
        let expected = "\
mp: doc len=1
mp.g: doc len=2
mp.g.n: str = \"Untitled\"
mp.g.o: i32 = -3
zoom: f32 = 0x3fc00000 (1.5)
pos: f64 = 0xbff0000000000000 (-1.0)
el: array len=1
el[0]: doc len=2
el[0].hidden: bool = false
el[0].f: array len=1
el[0].f[0]: doc len=1
el[0].f[0].t: null
tx: array len=1
tx[0]: doc len=1
tx[0].r: bytes = len=3 sha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
m: array len=1
m[0]: array len=1
m[0][0]: i32 = 1
";
        assert_eq!(text, expected);
    }

    #[test]
    fn an_empty_container_differs_from_a_missing_key_and_from_the_other_kind() {
        let missing = dumped(|d| d.i32("a", 1));
        let empty_array = dumped(|d| {
            d.i32("a", 1);
            d.array("el", |_| {});
        });
        let empty_doc = dumped(|d| {
            d.i32("a", 1);
            d.doc("el", |_| {});
        });
        assert_eq!(missing, "a: i32 = 1\n");
        assert_eq!(empty_array, "a: i32 = 1\nel: array len=0\n");
        assert_eq!(empty_doc, "a: i32 = 1\nel: doc len=0\n");
    }

    #[test]
    fn the_integer_width_is_part_of_the_type() {
        let text = dumped(|d| {
            d.i8("byte", -1);
            d.i16("short", 300);
            d.i32("int", 7);
            d.i64("long", 7);
        });
        assert_eq!(
            text,
            "byte: i8 = -1\nshort: i16 = 300\nint: i32 = 7\nlong: i64 = 7\n"
        );
    }

    #[test]
    fn keys_with_separators_stay_one_line_and_never_merge_with_nested_paths() {
        let text = dumped(|d| {
            d.i32("x: str = \"y\"", 1);
            d.i32("a.b", 2);
            d.doc("a", |d| d.i32("b", 3));
            d.i32("[3]", 4);
            d.array("v", |d| d.i32(3, 5));
            d.i32("two\nlines\\", 6);
        });
        let expected = "\
x:\\ str\\ =\\ \"y\": i32 = 1
a\\.b: i32 = 2
a: doc len=1
a.b: i32 = 3
\\[3\\]: i32 = 4
v: array len=1
v[3]: i32 = 5
two\\nlines\\\\: i32 = 6
";
        assert_eq!(text, expected);
        // the path of every line ends at its first `: `
        let paths: Vec<_> = text
            .lines()
            .map(|line| line.split_once(": ").expect("every line has a type").0)
            .collect();
        assert_eq!(
            paths,
            [
                "x:\\ str\\ =\\ \"y\"",
                "a\\.b",
                "a",
                "a.b",
                "\\[3\\]",
                "v",
                "v[3]",
                "two\\nlines\\\\"
            ]
        );
    }

    #[test]
    fn an_empty_first_key_keeps_its_dot_apart_from_the_root_and_from_indices() {
        let text = dumped(|d| {
            d.doc("", |d| {
                d.i32("x", 7);
                d.array("v", |d| d.i32(0, 8));
                d.array("", |d| d.i32(0, 9));
            });
            d.i32(0, 10);
        });
        let expected = "\
.: doc len=3
..x: i32 = 7
..v: array len=1
..v[0]: i32 = 8
..: array len=1
..[0]: i32 = 9
[0]: i32 = 10
";
        assert_eq!(text, expected);
    }

    #[test]
    fn floats_keep_their_bit_pattern_where_the_decimal_is_ambiguous() {
        let text = dumped(|d| {
            d.f32("neg_zero", -0.0);
            d.f32("nan", f32::from_bits(0x7fc0_0001));
            d.f64("inf", f64::NEG_INFINITY);
        });
        let expected = "\
neg_zero: f32 = 0x80000000 (-0.0)
nan: f32 = 0x7fc00001 (NaN)
inf: f64 = 0xfff0000000000000 (-inf)
";
        assert_eq!(text, expected);
    }

    #[test]
    fn strings_escape_quotes_backslashes_and_control_characters() {
        let text = dumped(|d| d.str("s", "a\"b\\c\n\t\u{1}\u{7f}é"));
        assert_eq!(text, "s: str = \"a\\\"b\\\\c\\n\\t\\u{1}\\u{7f}é\"\n");
    }

    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
