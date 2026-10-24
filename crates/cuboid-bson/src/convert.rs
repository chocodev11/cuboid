//! Lenient typed getters: the conversions Cubik's reader performs.
//!
//! spec: native 3.6

use std::borrow::Cow;

use crate::{BsonError, ElemType, RawDoc, RawElem, Value};

impl<'a> RawElem<'a> {
    fn wrong(&self, want: &'static str) -> BsonError {
        BsonError::WrongType { want, got: self.ty }
    }

    /// A double, int32 or int64. Doubles round to the nearest f32, ties to even.
    #[allow(clippy::cast_possible_truncation)]
    pub fn f32(&self) -> Result<f32, BsonError> {
        Ok(match self.ty {
            ElemType::Double => f64::from_le_bytes(self.fixed()?) as f32,
            ElemType::Int32 => i32::from_le_bytes(self.fixed()?) as f32,
            ElemType::Int64 => i64::from_le_bytes(self.fixed()?) as f32,
            _ => return Err(self.wrong("number")),
        })
    }

    /// An int32, an int64 or a double. Doubles round half to even; every value outside the
    /// range of an i32 is an error, as is NaN.
    #[allow(clippy::cast_possible_truncation)]
    pub fn i32(&self) -> Result<i32, BsonError> {
        let out_of_range = || BsonError::OutOfRange { want: "int32" };
        match self.ty {
            ElemType::Int32 => Ok(i32::from_le_bytes(self.fixed()?)),
            ElemType::Int64 => {
                i32::try_from(i64::from_le_bytes(self.fixed()?)).map_err(|_| out_of_range())
            }
            ElemType::Double => {
                let rounded = f64::from_le_bytes(self.fixed()?).round_ties_even();
                if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&rounded) {
                    Ok(rounded as i32)
                } else {
                    Err(out_of_range())
                }
            }
            _ => Err(self.wrong("number")),
        }
    }

    /// The byte field `v`: read as an int32, then 0..=255.
    pub fn u8(&self) -> Result<u8, BsonError> {
        u8::try_from(self.i32()?).map_err(|_| BsonError::OutOfRange { want: "byte" })
    }

    /// A boolean, or a number where anything but zero is true.
    pub fn bool(&self) -> Result<bool, BsonError> {
        Ok(match self.ty {
            ElemType::Bool => self.flag()?,
            ElemType::Int32 => i32::from_le_bytes(self.fixed()?) != 0,
            ElemType::Int64 => i64::from_le_bytes(self.fixed()?) != 0,
            ElemType::Double => f64::from_le_bytes(self.fixed()?) != 0.0,
            _ => return Err(self.wrong("bool")),
        })
    }

    /// An enumeration member: an integer as it is (undefined values included), or a string that
    /// names a member of `names` in any letter case.
    pub fn enum_i32(&self, names: &[(&str, i32)]) -> Result<i32, BsonError> {
        match self.ty {
            ElemType::Int32 | ElemType::Int64 => self.i32(),
            ElemType::String => {
                let name = self.text()?;
                names
                    .iter()
                    .find(|(member, _)| member.eq_ignore_ascii_case(name))
                    .map(|&(_, value)| value)
                    .ok_or_else(|| BsonError::UnknownName {
                        name: name.to_owned(),
                    })
            }
            _ => Err(self.wrong("enumeration")),
        }
    }

    /// A string, or `None` for null. Other scalar types are rejected.
    pub fn str_opt(&self) -> Result<Option<&'a str>, BsonError> {
        match self.ty {
            ElemType::String => self.text().map(Some),
            ElemType::Null => Ok(None),
            _ => Err(self.wrong("string")),
        }
    }

    /// Binary data of any subtype, a base64 string, or `None` for null. Subtype `0x02` carries
    /// an inner length that must match the outer one.
    pub fn bytes_opt(&self) -> Result<Option<Cow<'a, [u8]>>, BsonError> {
        Ok(Some(match self.value()? {
            Value::Null => return Ok(None),
            Value::Binary { subtype: 2, bytes } => match bytes.split_first_chunk() {
                Some((len, data))
                    if usize::try_from(i32::from_le_bytes(*len)) == Ok(data.len()) =>
                {
                    Cow::Borrowed(data)
                }
                _ => {
                    return Err(BsonError::BadLength {
                        at: self.value.start as usize,
                    });
                }
            },
            Value::Binary { bytes, .. } => Cow::Borrowed(bytes),
            Value::String(text) => Cow::Owned(base64(text).ok_or(BsonError::BadBase64)?),
            _ => return Err(self.wrong("binary")),
        }))
    }

    /// An embedded document, or `None` for null.
    pub fn doc(&self) -> Result<Option<RawDoc<'a>>, BsonError> {
        match self.ty {
            ElemType::Doc => self.nested(false).map(Some),
            ElemType::Null => Ok(None),
            _ => Err(self.wrong("document")),
        }
    }

    /// An array, or `None` for null. Its elements are read in order; their keys are ignored.
    pub fn array(&self) -> Result<Option<RawDoc<'a>>, BsonError> {
        match self.ty {
            ElemType::Array => self.nested(true).map(Some),
            ElemType::Null => Ok(None),
            _ => Err(self.wrong("array")),
        }
    }
}

/// Standard alphabet. Like .NET's `Convert.FromBase64String`, it skips white space and needs
/// the padding that completes the last group of four.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits, mut symbols, mut padding) = (0u32, 0u32, 0usize, 0usize);
    for &c in text.as_bytes() {
        let sextet = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                symbols += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if padding > 0 {
            return None;
        }
        symbols += 1;
        acc = acc << 6 | u32::from(sextet);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    (symbols % 4 == 0 && padding <= 2).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{first, hex, root, wrap};
    use crate::{DocWriter, Limits};

    /// Reads the single element of a document made of the encoded element `body`.
    fn read<T>(body: &str, get: impl FnOnce(RawElem<'_>) -> T) -> T {
        let bytes = wrap(&hex(body));
        let doc = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
        get(doc.iter().next().unwrap().unwrap())
    }

    fn double(v: f64) -> impl Fn(&mut DocWriter) {
        move |w| w.f64(b"v\0", v)
    }

    /// Whether `bytes_opt` borrowed the data, and the data.
    fn bytes_of(body: &str) -> Result<Option<(bool, Vec<u8>)>, BsonError> {
        read(body, |e| {
            e.bytes_opt()
                .map(|b| b.map(|b| (matches!(b, Cow::Borrowed(_)), b.into_owned())))
        })
    }

    #[test]
    fn f32_takes_doubles_and_integers() {
        let f32_of = |build: &dyn Fn(&mut DocWriter)| {
            let bytes = root(build);
            let doc = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
            doc.last(b"v").unwrap().unwrap().f32().unwrap()
        };
        // doubles round to the nearest f32, ties to even
        assert_eq!(f32_of(&double(0.1)).to_bits(), 0.1_f32.to_bits());
        assert_eq!(
            f32_of(&double(1.0 + 2.0_f64.powi(-24))).to_bits(),
            1.0_f32.to_bits()
        );
        assert_eq!(
            f32_of(&double(1.0 + 3.0 * 2.0_f64.powi(-24))).to_bits(),
            (1.0_f32 + 2.0_f32.powi(-22)).to_bits()
        );
        assert_eq!(f32_of(&double(1e300)).to_bits(), f32::INFINITY.to_bits());
        assert!(f32_of(&double(f64::NAN)).is_nan());
        // integers convert with one rounding
        assert_eq!(
            f32_of(&|w| w.i32(b"v\0", 16_777_217)).to_bits(),
            16_777_216.0_f32.to_bits()
        );
        assert_eq!(
            f32_of(&|w| w.i64(b"v\0", 16_777_219)).to_bits(),
            16_777_220.0_f32.to_bits()
        );
        assert_eq!(
            f32_of(&|w| w.i64(b"v\0", i64::MAX)).to_bits(),
            9_223_372_036_854_775_808.0_f32.to_bits()
        );
        assert_eq!(
            f32_of(&|w| w.i32(b"v\0", -7)).to_bits(),
            (-7.0_f32).to_bits()
        );
    }

    #[test]
    fn f32_survives_the_widening_to_a_double_and_back() {
        let patterns = (0..=u32::MAX).step_by(8191).chain([
            0,
            0x8000_0000,
            1,
            0x007f_ffff,
            0x0080_0000,
            0x7f7f_ffff,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0xffc0_0001,
        ]);
        // A signalling NaN becomes quiet when it is widened.
        let patterns: Vec<u32> = patterns
            .filter(|&bits| !f32::from_bits(bits).is_nan() || bits & 0x0040_0000 != 0)
            .collect();
        let bytes = root(|w| {
            let array = w.begin_array(b"v\0");
            for (i, &bits) in patterns.iter().enumerate() {
                w.f32(i, f32::from_bits(bits));
            }
            w.end(array);
        });
        let doc = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
        let array = doc.last(b"v").unwrap().unwrap().array().unwrap().unwrap();
        let mut count = 0;
        for (elem, &bits) in array.iter().zip(&patterns) {
            let elem = elem.unwrap();
            let Value::Double(wide) = elem.value().unwrap() else {
                panic!("not a double");
            };
            assert_eq!(wide.to_bits(), f64::from(f32::from_bits(bits)).to_bits());
            assert_eq!(elem.f32().unwrap().to_bits(), bits);
            count += 1;
        }
        assert_eq!(count, patterns.len());
    }

    #[test]
    fn i32_takes_integers_and_rounds_doubles_half_to_even() {
        let double_to_i32 = |v: f64| first(double(v), |e| e.i32().ok());
        let cases = [
            (123.0, Some(123)),
            (2.5, Some(2)),
            (3.5, Some(4)),
            (-2.5, Some(-2)),
            (-3.5, Some(-4)),
            (0.5, Some(0)),
            (-0.5, Some(0)),
            (1e-300, Some(0)),
            (2_147_483_647.4, Some(i32::MAX)),
            (2_147_483_647.5, None),
            (-2_147_483_648.5, Some(i32::MIN)),
            (-2_147_483_648.6, None),
            (f64::NAN, None),
            (f64::INFINITY, None),
            (f64::NEG_INFINITY, None),
        ];
        for (input, expected) in cases {
            assert_eq!(double_to_i32(input), expected, "{input}");
        }
        assert_eq!(first(|w| w.i32(b"v\0", -9), |e| e.i32()), Ok(-9));
        let int64 = |v: i64| first(|w| w.i64(b"v\0", v), |e| e.i32());
        assert_eq!(int64(i64::from(i32::MAX)), Ok(i32::MAX));
        assert_eq!(int64(i64::from(i32::MIN)), Ok(i32::MIN));
        let out_of_range = Err(BsonError::OutOfRange { want: "int32" });
        assert_eq!(int64(i64::from(i32::MAX) + 1), out_of_range);
        assert_eq!(int64(i64::from(i32::MIN) - 1), out_of_range);
    }

    #[test]
    fn the_byte_field_must_fit_a_byte() {
        let byte = |v: i32| first(|w| w.i32(b"v\0", v), |e| e.u8().ok());
        assert_eq!(byte(0), Some(0));
        assert_eq!(byte(255), Some(255));
        assert_eq!(byte(256), None);
        assert_eq!(byte(-1), None);
        assert_eq!(first(double(255.4), |e| e.u8().ok()), Some(255));
        assert_eq!(first(double(255.5), |e| e.u8().ok()), None);
    }

    #[test]
    fn bool_takes_booleans_and_numbers_where_non_zero_is_true() {
        let flag = |build: &dyn Fn(&mut DocWriter)| {
            let bytes = root(build);
            let doc = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
            doc.last(b"v").unwrap().unwrap().bool().unwrap()
        };
        assert!(flag(&|w| w.bool(b"v\0", true)));
        assert!(!flag(&|w| w.bool(b"v\0", false)));
        assert!(flag(&|w| w.i32(b"v\0", -1)));
        assert!(!flag(&|w| w.i32(b"v\0", 0)));
        assert!(flag(&|w| w.i64(b"v\0", 1 << 40)));
        assert!(!flag(&|w| w.i64(b"v\0", 0)));
        assert!(flag(&double(0.5)));
        assert!(flag(&double(f64::NAN)));
        assert!(!flag(&double(0.0)));
        assert!(!flag(&double(-0.0)));
    }

    #[test]
    fn enumerations_take_integers_as_they_are_and_names_in_any_case() {
        let names = [("None", 0), ("Down", 1), ("Up", 2), ("Any", 63)];
        let member = |build: fn(&mut DocWriter)| first(build, |e| e.enum_i32(&names));
        assert_eq!(member(|w| w.i32(b"v\0", 2)), Ok(2));
        // undefined values are kept, not rejected
        assert_eq!(member(|w| w.i32(b"v\0", 99)), Ok(99));
        assert_eq!(member(|w| w.i32(b"v\0", -7)), Ok(-7));
        assert_eq!(member(|w| w.i64(b"v\0", 63)), Ok(63));
        assert_eq!(
            member(|w| w.i64(b"v\0", 1 << 40)),
            Err(BsonError::OutOfRange { want: "int32" })
        );
        assert_eq!(member(|w| w.str(b"v\0", "up")), Ok(2));
        assert_eq!(member(|w| w.str(b"v\0", "ANY")), Ok(63));
        assert_eq!(member(|w| w.str(b"v\0", "Down")), Ok(1));
        assert_eq!(
            member(|w| w.str(b"v\0", "sideways")),
            Err(BsonError::UnknownName {
                name: "sideways".into()
            })
        );
        assert!(matches!(
            member(|w| w.f64(b"v\0", 1.0)),
            Err(BsonError::WrongType { .. })
        ));
        assert!(matches!(
            member(|w| w.bool(b"v\0", true)),
            Err(BsonError::WrongType { .. })
        ));
    }

    #[test]
    fn strings_accept_null_and_nothing_else() {
        let text =
            |build: fn(&mut DocWriter)| first(build, |e| e.str_opt().map(|s| s.map(str::to_owned)));
        assert_eq!(
            text(|w| w.str(b"v\0", "h\u{e9}llo")),
            Ok(Some("h\u{e9}llo".into()))
        );
        assert_eq!(text(|w| w.str(b"v\0", "")), Ok(Some(String::new())));
        assert_eq!(text(|w| w.str(b"v\0", "a\0b")), Ok(Some("a\0b".into())));
        assert_eq!(text(|w| w.null(b"v\0")), Ok(None));
        assert!(matches!(
            text(|w| w.i32(b"v\0", 5)),
            Err(BsonError::WrongType { .. })
        ));
        assert!(matches!(
            text(|w| w.bin(b"v\0", b"x")),
            Err(BsonError::WrongType { .. })
        ));
        let invalid = read("02 76 00 02 00 00 00 FF 00", |e| e.str_opt().map(|_| ()));
        assert_eq!(invalid, Err(BsonError::BadUtf8));
    }

    #[test]
    fn binary_values_of_any_subtype_are_taken_as_they_are() {
        let data = Ok(Some((true, vec![1, 2, 3])));
        assert_eq!(bytes_of("05 76 00 03 00 00 00 00 01 02 03"), data);
        assert_eq!(bytes_of("05 76 00 03 00 00 00 05 01 02 03"), data);
        assert_eq!(
            bytes_of("05 76 00 00 00 00 00 00"),
            Ok(Some((true, vec![])))
        );
        assert_eq!(bytes_of("0A 76 00"), Ok(None));
        assert!(matches!(
            bytes_of("10 76 00 01 00 00 00"),
            Err(BsonError::WrongType { .. })
        ));
    }

    #[test]
    fn subtype_two_carries_an_inner_length_that_must_match() {
        let old = |inner: &str| bytes_of(&format!("05 76 00 07 00 00 00 02 {inner} AA BB CC"));
        assert_eq!(old("03 00 00 00"), Ok(Some((true, vec![0xAA, 0xBB, 0xCC]))));
        for wrong in ["04 00 00 00", "02 00 00 00", "FF FF FF FF"] {
            assert!(
                matches!(old(wrong), Err(BsonError::BadLength { .. })),
                "{wrong}"
            );
        }
        let short = bytes_of("05 76 00 02 00 00 00 02 AA BB");
        assert!(matches!(short, Err(BsonError::BadLength { .. })));
    }

    #[test]
    fn a_string_is_decoded_as_base64() {
        let decode = |text: &str| {
            first(
                |w| w.str(b"v\0", text),
                |e| e.bytes_opt().map(|b| b.map(Cow::into_owned)),
            )
        };
        assert_eq!(decode("AQID"), Ok(Some(vec![1, 2, 3])));
        assert_eq!(decode("AQI="), Ok(Some(vec![1, 2])));
        assert_eq!(decode("AQ=="), Ok(Some(vec![1])));
        assert_eq!(decode("Zm9vYmFy"), Ok(Some(b"foobar".to_vec())));
        assert_eq!(decode("-_+/"), Err(BsonError::BadBase64));
        assert_eq!(decode("+/+/"), Ok(Some(vec![0xFB, 0xFF, 0xBF])));
        assert_eq!(decode(""), Ok(Some(vec![])));
        // white space is skipped
        assert_eq!(decode(" AQ\r\nI\tD "), Ok(Some(vec![1, 2, 3])));
        for bad in ["A", "AQI", "AQ=A", "A===", "AQ===", "AQ!=", "===="] {
            assert_eq!(decode(bad), Err(BsonError::BadBase64), "{bad:?}");
        }
    }

    #[test]
    fn documents_and_arrays_accept_null_and_not_each_other() {
        let bytes = root(|w| {
            let d = w.begin_doc(b"d\0");
            w.end(d);
            let a = w.begin_array(b"a\0");
            w.end(a);
            w.null(b"n\0");
            w.i32(b"i\0", 1);
        });
        let doc = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
        let get = |key: &[u8]| doc.last(key).unwrap().unwrap();
        assert!(get(b"d").doc().unwrap().is_some());
        assert!(get(b"a").array().unwrap().is_some());
        assert!(get(b"n").doc().unwrap().is_none());
        assert!(get(b"n").array().unwrap().is_none());
        assert!(matches!(
            get(b"d").array(),
            Err(BsonError::WrongType { .. })
        ));
        assert!(matches!(get(b"a").doc(), Err(BsonError::WrongType { .. })));
        assert!(matches!(get(b"i").doc(), Err(BsonError::WrongType { .. })));
    }

    #[test]
    fn array_elements_are_read_in_order_whatever_their_keys() {
        let bytes = wrap(&hex(
            "04 61 00 1A 00 00 00 10 35 00 01 00 00 00 10 31 00 02 00 00 00 10 7A 00 03 00 00 00 00",
        ));
        let doc = RawDoc::parse(&bytes, &Limits::DEFAULT).unwrap();
        let array = doc.last(b"a").unwrap().unwrap().array().unwrap().unwrap();
        let values: Vec<_> = array.iter().map(|e| e.unwrap().i32().unwrap()).collect();
        assert_eq!(values, [1, 2, 3]);
    }
}
