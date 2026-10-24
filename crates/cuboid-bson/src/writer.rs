//! Size-patching writer.
//!
//! Elements are appended in call order, which is the member order of native 3.3. A document
//! reserves its size field when it opens and patches it when it closes, so a payload is written
//! in one pass.

use crate::{BsonError, Limits};

/// The key of an element, written with its terminating NUL.
pub trait Key {
    fn put(self, out: &mut Vec<u8>);
}

/// A key that already ends with its NUL, such as `b"uv\0"`. Constants of this type are the
/// keys of the schema tables: one copy writes the key and its terminator.
impl Key for &'static [u8] {
    fn put(self, out: &mut Vec<u8>) {
        debug_assert!(
            self.split_last()
                .is_some_and(|(nul, name)| *nul == 0 && !name.contains(&0)),
            "a key is NUL-terminated and has no other NUL"
        );
        out.extend_from_slice(self);
    }
}

/// A byte string literal such as `b"uv\0"`.
impl<const N: usize> Key for &'static [u8; N] {
    fn put(self, out: &mut Vec<u8>) {
        let key: &'static [u8] = self;
        key.put(out);
    }
}

/// The key of an array element: its index in decimal.
impl Key for usize {
    #[allow(clippy::cast_possible_truncation)]
    fn put(self, out: &mut Vec<u8>) {
        // The digits come out last first.
        let mut digits = [0u8; 20];
        let mut count = 0;
        let mut rest = self;
        for slot in &mut digits {
            *slot = b'0' + (rest % 10) as u8;
            count += 1;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        out.extend(digits.iter().take(count).rev());
        out.push(0);
    }
}

/// An open document or array; [`DocWriter::end`] closes it.
#[derive(Debug)]
#[must_use = "an open document must be closed with `DocWriter::end`"]
pub struct Mark(usize);

/// Appends elements to a buffer. The buffer holds a whole payload or only a fragment of
/// elements (a worker's share of an array) that the owner of the payload splices in with
/// [`raw`](Self::raw).
///
/// spec: native 3.1, 3.2, 8.5
#[derive(Debug, Default)]
pub struct DocWriter {
    buf: Vec<u8>,
}

impl DocWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    /// The bytes written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// The finished buffer. The size fields of a buffer above `i32::MAX` bytes would not be
    /// exact, so such a buffer is an error.
    pub fn finish(self) -> Result<Vec<u8>, BsonError> {
        if i32::try_from(self.buf.len()).is_err() {
            return Err(BsonError::TooLarge {
                size: self.buf.len(),
                max: Limits::DEFAULT.max_doc,
            });
        }
        Ok(self.buf)
    }

    /// Opens the outermost document, which has neither type byte nor key.
    #[inline]
    pub fn begin_root(&mut self) -> Mark {
        self.open()
    }

    /// Opens an embedded document.
    pub fn begin_doc(&mut self, key: impl Key) -> Mark {
        self.element(0x03, key);
        self.open()
    }

    /// Opens an array; give its elements the keys `0usize`, `1`, `2`, ...
    pub fn begin_array(&mut self, key: impl Key) -> Mark {
        self.element(0x04, key);
        self.open()
    }

    /// Writes the terminator and patches the size field of the document that `mark` opened.
    #[inline]
    pub fn end(&mut self, mark: Mark) {
        self.buf.push(0);
        let size = len32(self.buf.len() - mark.0).to_le_bytes();
        if let Some(field) = self.buf.get_mut(mark.0..mark.0 + 4) {
            field.copy_from_slice(&size);
        }
    }

    /// A double.
    pub fn f64(&mut self, key: impl Key, value: f64) {
        self.element(0x01, key);
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// An f32 widened exactly to a double.
    ///
    /// spec: native 3.2
    pub fn f32(&mut self, key: impl Key, value: f32) {
        self.f64(key, f64::from(value));
    }

    pub fn i32(&mut self, key: impl Key, value: i32) {
        self.element(0x10, key);
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn i64(&mut self, key: impl Key, value: i64) {
        self.element(0x12, key);
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn bool(&mut self, key: impl Key, value: bool) {
        self.element(0x08, key);
        self.buf.push(u8::from(value));
    }

    pub fn null(&mut self, key: impl Key) {
        self.element(0x0A, key);
    }

    pub fn str(&mut self, key: impl Key, value: &str) {
        self.element(0x02, key);
        self.buf
            .extend_from_slice(&len32(value.len() + 1).to_le_bytes());
        self.buf.extend_from_slice(value.as_bytes());
        self.buf.push(0);
    }

    /// Binary data of the generic subtype `0x00`, the only one Cubik writes.
    pub fn bin(&mut self, key: impl Key, value: &[u8]) {
        self.element(0x05, key);
        self.buf
            .extend_from_slice(&len32(value.len()).to_le_bytes());
        self.buf.push(0);
        self.buf.extend_from_slice(value);
    }

    /// Appends encoded elements unchanged: a retained span of a loaded payload, or the buffer
    /// of another writer.
    pub fn raw(&mut self, elements: &[u8]) {
        self.buf.extend_from_slice(elements);
    }

    fn open(&mut self) -> Mark {
        let mark = Mark(self.buf.len());
        self.buf.extend_from_slice(&[0; 4]);
        mark
    }

    fn element(&mut self, ty: u8, key: impl Key) {
        self.buf.push(ty);
        key.put(&mut self.buf);
    }
}

/// A length for a size field. Lengths of a buffer that [`DocWriter::finish`] accepts fit.
fn len32(len: usize) -> i32 {
    i32::try_from(len).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{hex, root, wrap};

    // The byte examples of native 3.5, in the notation of the specification.

    #[test]
    fn the_empty_document() {
        assert_eq!(root(|_| {}), hex("05 00 00 00 00"));
    }

    #[test]
    fn vector3_of_one_two_three() {
        let bytes = root(|w| {
            w.f32(b"x\0", 1.0);
            w.f32(b"y\0", 2.0);
            w.f32(b"z\0", 3.0);
        });
        let expected = hex("26 00 00 00
            01 78 00  00 00 00 00 00 00 F0 3F
            01 79 00  00 00 00 00 00 00 00 40
            01 7A 00  00 00 00 00 00 00 08 40
            00");
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len(), 38);
    }

    #[test]
    fn vector2_shows_the_exact_widening_of_a_float() {
        let bytes = root(|w| {
            w.f32(b"x\0", 0.1);
            w.f32(b"y\0", 0.5);
        });
        let expected = hex("1B 00 00 00
            01 78 00  00 00 00 A0 99 99 B9 3F
            01 79 00  00 00 00 00 00 00 E0 3F
            00");
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len(), 27);
    }

    #[test]
    fn the_face_record_of_the_bottom_face() {
        let bytes = root(|w| {
            w.str(b"n\0", "down");
            w.bool(b"a\0", true);
            w.i32(b"ti\0", -1);
            w.i32(b"c\0", 0);
            w.str(b"t\0", "texture");
            let uv = w.begin_doc(b"uv\0");
            w.i32(b"r\0", 90);
            for (key, x, y) in [(&b"tl\0"[..], 0.0, 0.0), (&b"br\0"[..], 1.0, 1.0)] {
                let corner = w.begin_doc(key);
                w.f32(b"x\0", x);
                w.f32(b"y\0", y);
                w.end(corner);
            }
            w.end(uv);
        });
        let expected = hex("81 00 00 00
            02 6E 00 05 00 00 00 64 6F 77 6E 00
            08 61 00 01
            10 74 69 00 FF FF FF FF
            10 63 00 00 00 00 00
            02 74 00 08 00 00 00 74 65 78 74 75 72 65 00
            03 75 76 00 4A 00 00 00
               10 72 00 5A 00 00 00
               03 74 6C 00 1B 00 00 00 01 78 00 00 00 00 00 00 00 00 00
                                       01 79 00 00 00 00 00 00 00 00 00 00
               03 62 72 00 1B 00 00 00 01 78 00 00 00 00 00 00 00 F0 3F
                                       01 79 00 00 00 00 00 00 00 F0 3F 00
               00
            00");
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len(), 129);
    }

    #[test]
    fn the_voxel_record() {
        let colour = 0xFF5A_9643_u32.cast_signed();
        assert_eq!(colour, -10_840_509);
        let bytes = root(|w| {
            w.i32(b"x\0", 1);
            w.i32(b"y\0", 2);
            w.i32(b"z\0", 3);
            w.i32(b"c\0", colour);
        });
        let expected = hex(
            "21 00 00 00 10 78 00 01 00 00 00 10 79 00 02 00 00 00 10 7A 00 03 00 00 00 10 63 00 43 96 5A FF 00",
        );
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len(), 33);
    }

    // The sizes of native 3.4 (Vector2 and Vector3 are the vectors above).

    #[test]
    fn a_quaternion_with_its_euler_angles_is_100_bytes() {
        let bytes = root(|w| {
            for axis in [b"x\0", b"y\0", b"z\0", b"w\0"] {
                w.f32(axis, 0.0);
            }
            let euler = w.begin_doc(b"eulerAngles\0");
            for axis in [b"x\0", b"y\0", b"z\0"] {
                w.f32(axis, 0.0);
            }
            w.end(euler);
        });
        assert_eq!(bytes.len(), 100);
    }

    // Writer rules.

    #[test]
    fn members_keep_the_order_of_the_calls() {
        let bytes = root(|w| {
            w.null(b"b\0");
            w.null(b"a\0");
            w.null(b"b\0");
        });
        assert_eq!(bytes, hex("0E 00 00 00 0A 62 00 0A 61 00 0A 62 00 00"));
    }

    #[test]
    fn scalar_types_use_the_type_bytes_of_the_subset() {
        let bytes = root(|w| {
            w.f64(b"d\0", -0.0);
            w.i64(b"l\0", -2);
            w.bin(b"b\0", &[1, 2]);
            w.str(b"s\0", "");
            w.bool(b"f\0", false);
        });
        let expected = hex("31 00 00 00
            01 64 00 00 00 00 00 00 00 00 80
            12 6C 00 FE FF FF FF FF FF FF FF
            05 62 00 02 00 00 00 00 01 02
            02 73 00 01 00 00 00 00
            08 66 00 00
            00");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn string_lengths_count_bytes_and_the_terminator() {
        let bytes = root(|w| w.str(b"s\0", "\u{e9}\u{1f600}"));
        let expected = hex("13 00 00 00 02 73 00 07 00 00 00 C3 A9 F0 9F 98 80 00 00");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn array_elements_are_keyed_by_their_index() {
        let indices = [0, 9, 10, 99, 100, 12_345, 4_096];
        let bytes = root(|w| {
            let array = w.begin_array(b"v\0");
            for i in indices {
                w.null(i);
            }
            w.end(array);
        });
        let mut elements = Vec::new();
        for i in indices {
            elements.push(0x0A);
            elements.extend_from_slice(i.to_string().as_bytes());
            elements.push(0);
        }
        let mut member = vec![0x04, b'v', 0];
        member.extend(wrap(&elements));
        assert_eq!(bytes, wrap(&member));
    }

    #[test]
    fn index_keys_cover_the_largest_value() {
        let mut key = Vec::new();
        usize::MAX.put(&mut key);
        let mut expected = usize::MAX.to_string().into_bytes();
        expected.push(0);
        assert_eq!(key, expected);
    }

    #[test]
    fn nested_sizes_are_patched_when_the_documents_close() {
        let bytes = root(|w| {
            let outer = w.begin_doc(b"o\0");
            let inner = w.begin_array(b"i\0");
            w.end(inner);
            w.end(outer);
        });
        // inner 5, outer 4 + (1 + 2 + 5) + 1 = 13, root 4 + (1 + 2 + 13) + 1 = 21
        assert_eq!(
            bytes,
            hex("15 00 00 00 03 6F 00 0D 00 00 00 04 69 00 05 00 00 00 00 00 00")
        );
    }

    #[test]
    fn raw_splices_the_buffer_of_another_writer() {
        let mut part = DocWriter::new();
        part.i32(0usize, 7);
        part.i32(1usize, 8);
        let spliced = root(|w| {
            let array = w.begin_array(b"v\0");
            w.raw(part.as_bytes());
            w.end(array);
        });
        let direct = root(|w| {
            let array = w.begin_array(b"v\0");
            w.i32(0usize, 7);
            w.i32(1usize, 8);
            w.end(array);
        });
        assert_eq!(spliced, direct);
    }

    #[test]
    fn a_fragment_is_finished_without_a_document() {
        let mut part = DocWriter::with_capacity(16);
        part.null(b"k\0");
        assert_eq!(part.finish().unwrap(), hex("0A 6B 00"));
    }
}
