//! Zero-copy reader: documents, elements, spans and limits.
//!
//! A [`RawDoc`] checks its own size field and terminator when it is created. Everything below it
//! is checked while [`RawIter`] walks the elements, so that nothing is validated twice and a
//! reader that skips a subtree never pays for it.

use std::fmt;
use std::ops::Range;

use crate::BsonError;

/// A byte range of the buffer given to [`RawDoc::parse`]. Offsets stay below `i32::MAX`
/// because the root document size is an int32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// The range for slicing the buffer.
    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// Limits checked while reading, before anything is allocated for the data they bound.
///
/// spec: native 8.1 item 4, arch 12.3
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Size of the root document in bytes.
    pub max_doc: u32,
    /// Nesting depth of documents and arrays; the root document is level 1.
    pub max_depth: u8,
    /// Number of elements of one array. Every element takes at least two bytes but can decode
    /// into a much larger record, so the bytes alone do not bound the memory of a decoder.
    pub max_array: u32,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        max_doc: 2_147_483_647,
        max_depth: 64,
        max_array: 1 << 24,
    };
}

/// The element types of the BSON specification. Cubik writes the first eight of the table in
/// native 3.1 (`Double`, `String`, `Doc`, `Array`, `Binary`, `Bool`, `Null`, `Int32`) and reads
/// `Int64` as well; the rest only exist so that an unknown key of any type can be skipped.
///
/// spec: native 3.1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ElemType {
    Double = 0x01,
    String = 0x02,
    Doc = 0x03,
    Array = 0x04,
    Binary = 0x05,
    Undefined = 0x06,
    ObjectId = 0x07,
    Bool = 0x08,
    DateTime = 0x09,
    Null = 0x0A,
    Regex = 0x0B,
    DbPointer = 0x0C,
    Code = 0x0D,
    Symbol = 0x0E,
    CodeScope = 0x0F,
    Int32 = 0x10,
    Timestamp = 0x11,
    Int64 = 0x12,
    Decimal128 = 0x13,
    MaxKey = 0x7F,
    MinKey = 0xFF,
}

impl ElemType {
    pub fn from_byte(tag: u8) -> Option<Self> {
        Some(match tag {
            0x01 => Self::Double,
            0x02 => Self::String,
            0x03 => Self::Doc,
            0x04 => Self::Array,
            0x05 => Self::Binary,
            0x06 => Self::Undefined,
            0x07 => Self::ObjectId,
            0x08 => Self::Bool,
            0x09 => Self::DateTime,
            0x0A => Self::Null,
            0x0B => Self::Regex,
            0x0C => Self::DbPointer,
            0x0D => Self::Code,
            0x0E => Self::Symbol,
            0x0F => Self::CodeScope,
            0x10 => Self::Int32,
            0x11 => Self::Timestamp,
            0x12 => Self::Int64,
            0x13 => Self::Decimal128,
            0x7F => Self::MaxKey,
            0xFF => Self::MinKey,
            _ => return None,
        })
    }
}

/// One document or array, borrowed from the buffer it was read from.
#[derive(Clone, Copy)]
pub struct RawDoc<'a> {
    bytes: &'a [u8],
    /// Offset of `bytes` in the buffer given to `parse`.
    base: usize,
    depth: u8,
    limits: Limits,
    /// Arrays count their elements against `limits.max_array`.
    array: bool,
}

impl fmt::Debug for RawDoc<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawDoc")
            .field("span", &self.span())
            .field("depth", &self.depth)
            .finish()
    }
}

impl<'a> RawDoc<'a> {
    /// Reads the root document at the start of `bytes`. Bytes after the document are ignored;
    /// [`as_bytes`](Self::as_bytes) tells where it ends.
    ///
    /// spec: native 3.1 (the size fields are exact), 8.1 item 4
    pub fn parse(bytes: &'a [u8], limits: &Limits) -> Result<Self, BsonError> {
        let len = doc_len(bytes, 0)?;
        if len > limits.max_doc as usize {
            return Err(BsonError::TooLarge {
                size: len,
                max: limits.max_doc,
            });
        }
        if limits.max_depth == 0 {
            return Err(BsonError::TooDeep { max: 0 });
        }
        Ok(Self {
            bytes: bytes.get(..len).unwrap_or_default(),
            base: 0,
            depth: 1,
            limits: *limits,
            array: false,
        })
    }

    /// The bytes of the document: size field, elements and terminator.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Where the document sits in the buffer given to [`parse`](Self::parse).
    pub fn span(&self) -> Span {
        Span {
            start: off(self.base),
            end: off(self.base + self.bytes.len()),
        }
    }

    /// The elements in document order, duplicates included.
    pub fn iter(&self) -> RawIter<'a> {
        let body = self.bytes.get(4..self.bytes.len() - 1).unwrap_or_default();
        RawIter {
            rest: body,
            at: self.base + 4,
            depth: self.depth,
            limits: self.limits,
            left: if self.array {
                self.limits.max_array
            } else {
                u32::MAX
            },
        }
    }

    /// The last element named `key`: when a key occurs twice, the last occurrence wins.
    ///
    /// spec: native 3.6
    pub fn last(&self, key: &[u8]) -> Result<Option<RawElem<'a>>, BsonError> {
        let mut found = None;
        for elem in self.iter() {
            let elem = elem?;
            if elem.key == key {
                found = Some(elem);
            }
        }
        Ok(found)
    }
}

/// The elements of a document, in order. After an error the iterator ends.
#[derive(Clone)]
pub struct RawIter<'a> {
    rest: &'a [u8],
    at: usize,
    depth: u8,
    limits: Limits,
    left: u32,
}

impl fmt::Debug for RawIter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawIter")
            .field("at", &self.at)
            .field("remaining", &self.rest.len())
            .finish()
    }
}

impl<'a> Iterator for RawIter<'a> {
    type Item = Result<RawElem<'a>, BsonError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let item = self.read();
        if item.is_err() {
            self.rest = &[];
        }
        Some(item)
    }
}

impl<'a> RawIter<'a> {
    fn read(&mut self) -> Result<RawElem<'a>, BsonError> {
        let start = self.at;
        let Some(left) = self.left.checked_sub(1) else {
            return Err(BsonError::TooMany {
                max: self.limits.max_array,
            });
        };
        self.left = left;

        let (&tag, rest) = self
            .rest
            .split_first()
            .ok_or(BsonError::Truncated { at: start })?;
        let ty = ElemType::from_byte(tag).ok_or(BsonError::UnknownType { ty: tag, at: start })?;
        let key_len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(BsonError::BadTerminator {
                at: start + self.rest.len(),
            })?;
        let value_start = start + 2 + key_len;
        let area = rest.get(key_len + 1..).unwrap_or_default();
        let len = value_len(ty, area, value_start)?;
        let (data, tail) = area
            .split_at_checked(len)
            .ok_or(BsonError::Truncated { at: value_start })?;

        let end = value_start + len;
        self.rest = tail;
        self.at = end;
        Ok(RawElem {
            key: rest.get(..key_len).unwrap_or_default(),
            ty,
            value: Span {
                start: off(value_start),
                end: off(end),
            },
            whole: Span {
                start: off(start),
                end: off(end),
            },
            data,
            depth: self.depth,
            limits: self.limits,
        })
    }
}

/// One element: key, type and the position of its bytes. The lenient getters of native 3.6 are
/// methods of this type.
#[derive(Clone, Copy)]
pub struct RawElem<'a> {
    /// The key without its NUL terminator; not necessarily UTF-8.
    pub key: &'a [u8],
    pub ty: ElemType,
    /// The value bytes: for a string its length prefix and terminator, for a document or array
    /// the whole embedded document, for a binary value its length prefix, subtype and data.
    pub value: Span,
    /// The whole element: type byte, key, terminator and value.
    pub whole: Span,
    pub(crate) data: &'a [u8],
    depth: u8,
    limits: Limits,
}

impl fmt::Debug for RawElem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawElem")
            .field("key", &String::from_utf8_lossy(self.key))
            .field("ty", &self.ty)
            .field("whole", &self.whole)
            .finish()
    }
}

/// An element value without any conversion.
#[derive(Debug, Clone, Copy)]
pub enum Value<'a> {
    Double(f64),
    String(&'a str),
    Doc(RawDoc<'a>),
    Array(RawDoc<'a>),
    /// The bytes as stored; subtype `0x02` keeps its inner length prefix.
    Binary {
        subtype: u8,
        bytes: &'a [u8],
    },
    Bool(bool),
    Null,
    Int32(i32),
    Int64(i64),
    /// A type that Cubik neither writes nor reads; the bytes are at [`RawElem::value_bytes`].
    Other(ElemType),
}

impl<'a> RawElem<'a> {
    /// The bytes of [`value`](Self::value).
    pub fn value_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// The value as stored: `Double` stays a double, an int32 stays an int32. The conversions
    /// of native 3.6 are the getters of this type.
    pub fn value(&self) -> Result<Value<'a>, BsonError> {
        Ok(match self.ty {
            ElemType::Double => Value::Double(f64::from_le_bytes(self.fixed()?)),
            ElemType::String => Value::String(self.text()?),
            ElemType::Doc => Value::Doc(self.nested(false)?),
            ElemType::Array => Value::Array(self.nested(true)?),
            ElemType::Binary => {
                let [subtype] = self.fixed_at(4)?;
                Value::Binary {
                    subtype,
                    bytes: self.data.get(5..).unwrap_or_default(),
                }
            }
            ElemType::Bool => Value::Bool(self.flag()?),
            ElemType::Null => Value::Null,
            ElemType::Int32 => Value::Int32(i32::from_le_bytes(self.fixed()?)),
            ElemType::Int64 => Value::Int64(i64::from_le_bytes(self.fixed()?)),
            other => Value::Other(other),
        })
    }

    /// The embedded document or array of this element, one level deeper than its parent.
    pub(crate) fn nested(&self, array: bool) -> Result<RawDoc<'a>, BsonError> {
        if self.depth >= self.limits.max_depth {
            return Err(BsonError::TooDeep {
                max: self.limits.max_depth,
            });
        }
        Ok(RawDoc {
            bytes: self.data,
            base: self.at(),
            depth: self.depth + 1,
            limits: self.limits,
            array,
        })
    }

    /// The byte of a boolean element, which must be 0 or 1.
    pub(crate) fn flag(&self) -> Result<bool, BsonError> {
        match self.fixed()? {
            [0] => Ok(false),
            [1] => Ok(true),
            [byte] => Err(BsonError::BadBool {
                byte,
                at: self.at(),
            }),
        }
    }

    /// The characters of a string element.
    pub(crate) fn text(&self) -> Result<&'a str, BsonError> {
        let chars = self
            .data
            .get(4..self.data.len() - 1)
            .ok_or(BsonError::Truncated { at: self.at() })?;
        str::from_utf8(chars).map_err(|_| BsonError::BadUtf8)
    }

    pub(crate) fn fixed<const N: usize>(&self) -> Result<[u8; N], BsonError> {
        self.fixed_at(0)
    }

    fn fixed_at<const N: usize>(&self, from: usize) -> Result<[u8; N], BsonError> {
        self.data
            .get(from..)
            .and_then(|tail| tail.first_chunk().copied())
            .ok_or(BsonError::Truncated { at: self.at() })
    }

    /// Offset of the value, for messages.
    fn at(&self) -> usize {
        self.value.start as usize
    }
}

/// The size of the document at the start of `buf` after checking its size field and terminator.
/// `at` is the offset of `buf` for messages.
fn doc_len(buf: &[u8], at: usize) -> Result<usize, BsonError> {
    let len = len_field(buf, at)?;
    if len < 5 {
        return Err(BsonError::BadLength { at });
    }
    fit(len, buf, at)?;
    match buf.get(len - 1) {
        Some(0) => Ok(len),
        _ => Err(BsonError::BadTerminator { at: at + len }),
    }
}

/// The size of a value of type `ty` at the start of `v`, the rest of the enclosing document.
fn value_len(ty: ElemType, v: &[u8], at: usize) -> Result<usize, BsonError> {
    match ty {
        ElemType::Null | ElemType::Undefined | ElemType::MinKey | ElemType::MaxKey => Ok(0),
        ElemType::Bool => fit(1, v, at),
        ElemType::Int32 => fit(4, v, at),
        ElemType::Double | ElemType::DateTime | ElemType::Timestamp | ElemType::Int64 => {
            fit(8, v, at)
        }
        ElemType::ObjectId => fit(12, v, at),
        ElemType::Decimal128 => fit(16, v, at),
        ElemType::Doc | ElemType::Array => doc_len(v, at),
        ElemType::String | ElemType::Code | ElemType::Symbol => string_len(v, at),
        // Length, subtype, data.
        ElemType::Binary => fit(5 + len_field(v, at)?, v, at),
        // A string and the 12 bytes of an object id.
        ElemType::DbPointer => fit(string_len(v, at)? + 12, v, at),
        // Pattern and options, both NUL-terminated.
        ElemType::Regex => {
            let end = |from: usize| {
                v.get(from..)
                    .and_then(|tail| tail.iter().position(|&b| b == 0))
                    .map(|nul| from + nul + 1)
                    .ok_or(BsonError::BadTerminator { at: at + v.len() })
            };
            end(end(0)?)
        }
        // The int32 counts itself, the code string and the scope document: at least 4 + 5 + 5.
        ElemType::CodeScope => match len_field(v, at)? {
            len if len < 14 => Err(BsonError::BadLength { at }),
            len => fit(len, v, at),
        },
    }
}

/// A string: an int32 that counts the characters and the NUL, then both.
fn string_len(v: &[u8], at: usize) -> Result<usize, BsonError> {
    let n = len_field(v, at)?;
    if n == 0 {
        return Err(BsonError::BadLength { at });
    }
    let len = fit(4 + n, v, at)?;
    match v.get(len - 1) {
        Some(0) => Ok(len),
        _ => Err(BsonError::BadTerminator { at: at + len }),
    }
}

/// The int32 at the start of `v` as a length.
fn len_field(v: &[u8], at: usize) -> Result<usize, BsonError> {
    let n = read_i32(v).ok_or(BsonError::Truncated { at })?;
    usize::try_from(n).map_err(|_| BsonError::BadLength { at })
}

/// `len` if `v` holds that many bytes.
fn fit(len: usize, v: &[u8], at: usize) -> Result<usize, BsonError> {
    if len <= v.len() {
        Ok(len)
    } else {
        Err(BsonError::Truncated { at: at + v.len() })
    }
}

fn read_i32(buf: &[u8]) -> Option<i32> {
    buf.first_chunk().copied().map(i32::from_le_bytes)
}

/// Offsets are below `i32::MAX`: `RawDoc::parse` rejects a root document whose size field
/// is larger, and every element lies inside it.
#[allow(clippy::cast_possible_truncation)]
fn off(offset: usize) -> u32 {
    offset as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{first, hex, root, wrap};

    fn parse(bytes: &[u8]) -> Result<RawDoc<'_>, BsonError> {
        RawDoc::parse(bytes, &Limits::DEFAULT)
    }

    fn elems(bytes: &[u8]) -> Result<Vec<RawElem<'_>>, BsonError> {
        parse(bytes)?.iter().collect()
    }

    fn keys<'a>(elems: &[RawElem<'a>]) -> Vec<&'a [u8]> {
        elems.iter().map(|elem| elem.key).collect()
    }

    const VECTOR3: &str = "26 00 00 00
        01 78 00  00 00 00 00 00 00 F0 3F
        01 79 00  00 00 00 00 00 00 00 40
        01 7A 00  00 00 00 00 00 00 08 40
        00";

    #[test]
    fn the_voxel_record_reads_back() {
        let bytes = hex(
            "21 00 00 00 10 78 00 01 00 00 00 10 79 00 02 00 00 00 10 7A 00 03 00 00 00 10 63 00 43 96 5A FF 00",
        );
        let doc = parse(&bytes).unwrap();
        let voxel: Vec<_> = doc.iter().map(|e| e.unwrap().i32().unwrap()).collect();
        assert_eq!(voxel, [1, 2, 3, -10_840_509]);
        assert_eq!(voxel[3], 0xFF5A_9643_u32.cast_signed());
    }

    #[test]
    fn the_empty_document_has_no_elements() {
        let bytes = hex("05 00 00 00 00");
        let doc = parse(&bytes).unwrap();
        assert_eq!(doc.span(), Span { start: 0, end: 5 });
        assert!(doc.iter().next().is_none());
    }

    #[test]
    fn elements_carry_key_type_value_and_exact_spans() {
        let bytes = hex(VECTOR3);
        let elems = elems(&bytes).unwrap();
        assert_eq!(keys(&elems), [b"x", b"y", b"z"]);
        let spans: Vec<_> = elems.iter().map(|e| (e.whole, e.value)).collect();
        let span = |start, end| Span { start, end };
        assert_eq!(
            spans,
            [
                (span(4, 15), span(7, 15)),
                (span(15, 26), span(18, 26)),
                (span(26, 37), span(29, 37)),
            ]
        );
        for (elem, expected) in elems.iter().zip([1.0f64, 2.0, 3.0]) {
            assert_eq!(elem.ty, ElemType::Double);
            let Value::Double(got) = elem.value().unwrap() else {
                panic!("not a double");
            };
            assert_eq!(got.to_bits(), expected.to_bits());
            assert_eq!(elem.value_bytes(), &bytes[elem.value.range()]);
        }
    }

    #[test]
    fn spans_of_nested_elements_are_relative_to_the_payload() {
        let bytes = hex("81 00 00 00
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
        let doc = parse(&bytes).unwrap();
        let uv = doc.last(b"uv").unwrap().unwrap().doc().unwrap().unwrap();
        let br = uv.last(b"br").unwrap().unwrap().doc().unwrap().unwrap();
        let y = br.last(b"y").unwrap().unwrap();
        assert_eq!(
            &bytes[y.whole.range()],
            hex("01 79 00 00 00 00 00 00 00 F0 3F")
        );
        assert_eq!(&bytes[br.span().range()][..4], hex("1B 00 00 00"));
        assert_eq!(
            uv.span(),
            Span {
                start: 54,
                end: 128
            }
        );
        assert_eq!(
            br.span(),
            Span {
                start: 100,
                end: 127
            }
        );
    }

    #[test]
    fn duplicate_keys_iterate_in_order_and_the_last_one_wins() {
        let bytes = root(|w| {
            w.i32(b"a\0", 1);
            w.i32(b"b\0", 2);
            w.i32(b"a\0", 3);
        });
        let doc = parse(&bytes).unwrap();
        let all: Vec<_> = doc.iter().map(|e| e.unwrap().i32().unwrap()).collect();
        assert_eq!(all, [1, 2, 3]);
        assert_eq!(doc.last(b"a").unwrap().unwrap().i32().unwrap(), 3);
        assert!(doc.last(b"c").unwrap().is_none());
    }

    #[test]
    fn bytes_after_the_root_document_are_not_part_of_it() {
        let mut bytes = hex("05 00 00 00 00");
        bytes.extend([1, 2, 3]);
        assert_eq!(parse(&bytes).unwrap().as_bytes().len(), 5);
    }

    #[test]
    fn a_malformed_root_is_rejected() {
        let err = |text: &str| parse(&hex(text)).unwrap_err();
        assert!(matches!(err(""), BsonError::Truncated { .. }));
        assert!(matches!(err("05 00 00"), BsonError::Truncated { .. }));
        assert!(matches!(err("04 00 00 00 00"), BsonError::BadLength { .. }));
        assert!(matches!(err("FF FF FF FF 00"), BsonError::BadLength { .. }));
        assert!(matches!(err("06 00 00 00 00"), BsonError::Truncated { .. }));
        assert!(matches!(
            err("05 00 00 00 01"),
            BsonError::BadTerminator { .. }
        ));
    }

    #[test]
    fn a_malformed_element_ends_the_iteration_with_its_error() {
        let rejected = |body: &str| {
            let bytes = wrap(&hex(body));
            let mut iter = parse(&bytes).unwrap().iter();
            let err = iter.next().unwrap().unwrap_err();
            assert!(iter.next().is_none(), "{body}: the iterator is fused");
            err
        };
        // type bytes
        assert!(matches!(
            rejected("20 61 00"),
            BsonError::UnknownType { ty: 0x20, at: 4 }
        ));
        assert!(matches!(
            rejected("00 00"),
            BsonError::UnknownType { ty: 0, .. }
        ));
        // keys
        assert!(matches!(
            rejected("10 61 62"),
            BsonError::BadTerminator { .. }
        ));
        // fixed-size values
        assert!(matches!(
            rejected("10 61 00 01 00"),
            BsonError::Truncated { .. }
        ));
        assert!(matches!(
            rejected("01 61 00 00 00 00 00"),
            BsonError::Truncated { .. }
        ));
        // strings
        assert!(matches!(
            rejected("02 61 00 00 00 00 00"),
            BsonError::BadLength { .. }
        ));
        assert!(matches!(
            rejected("02 61 00 FF FF FF FF 00"),
            BsonError::BadLength { .. }
        ));
        assert!(matches!(
            rejected("02 61 00 02 00 00 00 41 41"),
            BsonError::BadTerminator { .. }
        ));
        assert!(matches!(
            rejected("02 61 00 FF 00 00 00 41 00"),
            BsonError::Truncated { .. }
        ));
        // binary values
        assert!(matches!(
            rejected("05 61 00 FF FF FF FF 00"),
            BsonError::BadLength { .. }
        ));
        assert!(matches!(
            rejected("05 61 00 05 00 00 00 00 01"),
            BsonError::Truncated { .. }
        ));
        // embedded documents
        assert!(matches!(
            rejected("03 61 00 04 00 00 00"),
            BsonError::BadLength { .. }
        ));
        assert!(matches!(
            rejected("03 61 00 0A 00 00 00 00"),
            BsonError::Truncated { .. }
        ));
        assert!(matches!(
            rejected("03 61 00 05 00 00 00 01"),
            BsonError::BadTerminator { .. }
        ));
        // types of the full specification
        assert!(matches!(
            rejected("0F 61 00 08 00 00 00 00 00 00 00"),
            BsonError::BadLength { .. }
        ));
        assert!(matches!(
            rejected("0B 61 00 61 62 00 69"),
            BsonError::BadTerminator { .. }
        ));
        assert!(matches!(
            rejected("0C 61 00 02 00 00 00 61 00 00 01"),
            BsonError::Truncated { .. }
        ));
    }

    #[test]
    fn a_valid_element_before_the_error_is_still_returned() {
        let bytes = wrap(&hex("0A 61 00 20 62 00"));
        let mut iter = parse(&bytes).unwrap().iter();
        assert_eq!(iter.next().unwrap().unwrap().key, b"a");
        assert!(iter.next().unwrap().is_err());
    }

    #[test]
    fn elements_of_types_cubik_never_writes_are_skipped_by_their_length() {
        let cases = [
            (ElemType::Undefined, "06 6B 00"),
            (
                ElemType::ObjectId,
                "07 6B 00 00 01 02 03 04 05 06 07 08 09 0A 0B",
            ),
            (ElemType::DateTime, "09 6B 00 00 00 00 00 00 00 00 00"),
            (ElemType::Regex, "0B 6B 00 61 62 00 69 00"),
            (
                ElemType::DbPointer,
                "0C 6B 00 02 00 00 00 61 00 00 01 02 03 04 05 06 07 08 09 0A 0B",
            ),
            (ElemType::Code, "0D 6B 00 02 00 00 00 61 00"),
            (ElemType::Symbol, "0E 6B 00 02 00 00 00 61 00"),
            (
                ElemType::CodeScope,
                "0F 6B 00 0F 00 00 00 02 00 00 00 61 00 05 00 00 00 00",
            ),
            (ElemType::Timestamp, "11 6B 00 00 00 00 00 00 00 00 00"),
            (
                ElemType::Decimal128,
                "13 6B 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00",
            ),
            (ElemType::MaxKey, "7F 6B 00"),
            (ElemType::MinKey, "FF 6B 00"),
        ];
        for (ty, text) in cases {
            let skipped = hex(text);
            let mut body = skipped.clone();
            body.extend(hex("10 7A 00 07 00 00 00"));
            let bytes = wrap(&body);
            let got = elems(&bytes).unwrap();
            assert_eq!(got.len(), 2, "{ty:?}");
            assert_eq!(got[0].ty, ty);
            assert_eq!(&bytes[got[0].whole.range()], skipped);
            assert!(matches!(got[0].value().unwrap(), Value::Other(t) if t == ty));
            assert_eq!(got[1].key, b"z");
            assert_eq!(got[1].i32().unwrap(), 7);
        }
    }

    #[test]
    fn every_type_byte_maps_back_to_itself() {
        let types: Vec<ElemType> = (0..=u8::MAX).filter_map(ElemType::from_byte).collect();
        assert_eq!(types.len(), 21);
        assert!(
            types
                .iter()
                .all(|&t| ElemType::from_byte(t as u8) == Some(t))
        );
    }

    #[test]
    fn values_are_read_as_stored() {
        let bytes = root(|w| {
            w.str(b"s\0", "hi");
            w.bool(b"t\0", true);
            w.null(b"n\0");
            w.i32(b"i\0", -5);
            w.i64(b"l\0", i64::MIN);
            w.bin(b"b\0", &[9, 8]);
        });
        let got: Vec<_> = elems(&bytes)
            .unwrap()
            .iter()
            .map(|e| e.value().unwrap())
            .collect();
        assert!(matches!(got[0], Value::String("hi")));
        assert!(matches!(got[1], Value::Bool(true)));
        assert!(matches!(got[2], Value::Null));
        assert!(matches!(got[3], Value::Int32(-5)));
        assert!(matches!(got[4], Value::Int64(i64::MIN)));
        assert!(matches!(
            got[5],
            Value::Binary {
                subtype: 0,
                bytes: [9, 8]
            }
        ));
    }

    #[test]
    fn a_boolean_byte_other_than_zero_and_one_is_rejected_when_read() {
        let bytes = wrap(&hex("08 62 00 02"));
        let elem = elems(&bytes).unwrap().remove(0);
        assert!(matches!(
            elem.value(),
            Err(BsonError::BadBool { byte: 2, at: 7 })
        ));
        assert!(matches!(
            elem.bool(),
            Err(BsonError::BadBool { byte: 2, .. })
        ));
    }

    #[test]
    fn a_string_that_is_not_utf8_is_rejected_when_read() {
        let bytes = wrap(&hex("02 73 00 02 00 00 00 FF 00"));
        let elem = elems(&bytes).unwrap().remove(0);
        assert!(matches!(elem.value(), Err(BsonError::BadUtf8)));
    }

    #[test]
    fn the_limits_are_checked_while_reading() {
        // size of the document
        let flat = root(|w| w.null(b"a\0"));
        assert_eq!(flat.len(), 8);
        let max_doc = |max_doc| Limits {
            max_doc,
            ..Limits::DEFAULT
        };
        assert!(RawDoc::parse(&flat, &max_doc(8)).is_ok());
        assert!(matches!(
            RawDoc::parse(&flat, &max_doc(7)),
            Err(BsonError::TooLarge { size: 8, max: 7 })
        ));

        // nesting: the root is level 1, a is level 2 and b level 3
        let nested = root(|w| {
            let a = w.begin_doc(b"a\0");
            let b = w.begin_doc(b"b\0");
            w.end(b);
            w.end(a);
        });
        let reach_b = |max_depth: u8| -> Result<(), BsonError> {
            let limits = Limits {
                max_depth,
                ..Limits::DEFAULT
            };
            let root = RawDoc::parse(&nested, &limits)?;
            let a = root.last(b"a")?.map(|a| a.doc()).transpose()?.flatten();
            let b = a.map(|a| a.last(b"b")).transpose()?.flatten();
            b.map(|b| b.doc()).transpose().map(|_| ())
        };
        assert!(reach_b(3).is_ok());
        assert!(matches!(reach_b(2), Err(BsonError::TooDeep { max: 2 })));
        assert!(matches!(reach_b(1), Err(BsonError::TooDeep { max: 1 })));
        assert!(matches!(reach_b(0), Err(BsonError::TooDeep { max: 0 })));

        // elements of an array, not of a document
        let array = root(|w| {
            let a = w.begin_array(b"a\0");
            for i in 0..4 {
                w.null(i);
            }
            w.end(a);
        });
        let count = |max_array: u32| -> Result<i32, BsonError> {
            let limits = Limits {
                max_array,
                ..Limits::DEFAULT
            };
            let root = RawDoc::parse(&array, &limits)?;
            let a = root.last(b"a")?.map(|a| a.array()).transpose()?.flatten();
            a.map_or(Ok(0), |a| a.iter().try_fold(0, |n, e| e.map(|_| n + 1)))
        };
        assert_eq!(count(4).unwrap(), 4);
        assert!(matches!(count(3), Err(BsonError::TooMany { max: 3 })));
        let document = root(|w| {
            for key in [b"a\0", b"b\0", b"c\0", b"d\0"] {
                w.null(key);
            }
        });
        let limits = Limits {
            max_array: 3,
            ..Limits::DEFAULT
        };
        assert_eq!(RawDoc::parse(&document, &limits).unwrap().iter().count(), 4);
    }

    #[test]
    fn debug_output_of_a_document_does_not_print_its_bytes() {
        let bytes = hex(VECTOR3);
        let doc = parse(&bytes).unwrap();
        assert_eq!(
            format!("{doc:?}"),
            "RawDoc { span: Span { start: 0, end: 38 }, depth: 1 }"
        );
        let text = first(|w| w.null(b"k\0"), |e| format!("{e:?}"));
        assert!(text.starts_with("RawElem { key: \"k\", ty: Null"));
    }
}
