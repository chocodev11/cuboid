//! The content signature of a texture's source file (`cs` of a texture record).

use md5::{Digest, Md5};

/// The content signature of a source file: the MD5 digest bytes decoded as text with the ANSI
/// code page `code_page`, not as hexadecimal. The text is machine dependent and only compared
/// for equality. An unknown code page falls back to Windows-1252.
///
/// The decoder is the WHATWG one of `encoding_rs`, which maps the bytes a Windows code page
/// leaves undefined to the C1 control characters, as Windows does for 1252. Whether the original's
/// own tables agree for the other pages is **UNVERIFIED**.
///
/// spec: native 4.5 (`cs`), tools 5.12
pub fn signature(file_bytes: &[u8], code_page: u16) -> String {
    let digest = Md5::digest(file_bytes);
    let encoding = codepage::to_encoding(code_page).unwrap_or(encoding_rs::WINDOWS_1252);
    encoding.decode_without_bom_handling(&digest).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected texts come from the code page tables of Python's codecs for the defined bytes;
    // the undefined bytes 0x8F, 0x90 and 0x9D map to the C1 controls U+008F, U+0090 and U+009D.

    // MD5 "" = d41d8cd98f00b204e9800998ecf8427e. The digests of 1252 and 1258 differ at 0xec.
    const EMPTY_1252: &str =
        "\u{d4}\u{1d}\u{152}\u{d9}\u{8f}\u{0}\u{b2}\u{4}\u{e9}\u{20ac}\u{9}\u{2dc}\u{ec}\u{f8}B~";
    const EMPTY_1258: &str =
        "\u{d4}\u{1d}\u{152}\u{d9}\u{8f}\u{0}\u{b2}\u{4}\u{e9}\u{20ac}\u{9}\u{2dc}\u{301}\u{f8}B~";
    // MD5 "abc" = 900150983cd24fb0d6963f7d28e17f72; the digests differ at 0xd2.
    const ABC_1252: &str = "\u{90}\u{1}P\u{2dc}<\u{d2}O\u{b0}\u{d6}\u{2013}?}(\u{e1}\u{7f}r";
    const ABC_1258: &str = "\u{90}\u{1}P\u{2dc}<\u{309}O\u{b0}\u{d6}\u{2013}?}(\u{e1}\u{7f}r";

    #[test]
    fn vectors_of_windows_1252() {
        assert_eq!(signature(b"", 1252), EMPTY_1252);
        assert_eq!(signature(b"abc", 1252), ABC_1252);
    }

    #[test]
    fn vectors_of_windows_1258_with_its_combining_marks() {
        assert_eq!(signature(b"", 1258), EMPTY_1258);
        assert_eq!(signature(b"abc", 1258), ABC_1258);
        // One character per digest byte, although the marks combine with their neighbours.
        assert_eq!(signature(b"abc", 1258).chars().count(), 16);
    }

    /// MD5 "The quick brown fox jumps over the lazy dog" = 9e107d9d372bb6826bd81d3542a419d6. The
    /// byte 0x9d is undefined in both pages and 0x9e only in 1258: encoding_rs reads them as the
    /// C1 controls of the same number.
    #[test]
    fn bytes_a_code_page_leaves_undefined_read_as_c1_controls() {
        let fox = b"The quick brown fox jumps over the lazy dog";
        let tail = "\u{10}}\u{9d}7+\u{b6}\u{201a}k\u{d8}\u{1d}5B\u{a4}\u{19}\u{d6}";
        assert_eq!(signature(fox, 1252), format!("\u{17e}{tail}"));
        assert_eq!(signature(fox, 1258), format!("\u{9e}{tail}"));
    }

    #[test]
    fn an_unknown_code_page_reads_as_windows_1252() {
        assert_eq!(signature(b"abc", 0), ABC_1252);
        assert_eq!(signature(b"abc", 9999), ABC_1252);
    }

    #[test]
    fn the_signature_follows_the_file_content() {
        assert_ne!(signature(b"abc", 1252), signature(b"abd", 1252));
        assert_eq!(signature(b"abc", 1252), signature(b"abc", 1252));
    }
}
