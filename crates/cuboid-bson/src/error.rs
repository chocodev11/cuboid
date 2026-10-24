//! The error type of the crate.

use crate::ElemType;

/// Why a document is malformed, a limit is exceeded or a value does not convert.
///
/// Offsets are relative to the buffer given to [`RawDoc::parse`](crate::RawDoc::parse).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BsonError {
    #[error("document ends inside an element at offset {at}")]
    Truncated { at: usize },
    #[error("size or length field at offset {at} is negative or exceeds its container")]
    BadLength { at: usize },
    #[error("missing NUL terminator before offset {at}")]
    BadTerminator { at: usize },
    #[error("unknown element type 0x{ty:02x} at offset {at}")]
    UnknownType { ty: u8, at: usize },
    #[error("invalid boolean byte 0x{byte:02x} at offset {at}")]
    BadBool { byte: u8, at: usize },
    #[error("document of {size} bytes exceeds the limit of {max}")]
    TooLarge { size: usize, max: u32 },
    #[error("nesting deeper than {max} levels")]
    TooDeep { max: u8 },
    #[error("array with more than {max} elements")]
    TooMany { max: u32 },
    #[error("expected {want}, found {got:?}")]
    WrongType { want: &'static str, got: ElemType },
    #[error("value out of range for {want}")]
    OutOfRange { want: &'static str },
    #[error("string is not valid UTF-8")]
    BadUtf8,
    #[error("string is not valid base64")]
    BadBase64,
    #[error("no enumeration member named {name:?}")]
    UnknownName { name: String },
}
