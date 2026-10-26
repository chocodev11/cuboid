//! Standard base64, for the binary data that a text format embeds: the `data:` URLs of `.bbmodel`
//! textures and the picture strips of the `.cubik` extension entry.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut group = [0u8; 3];
        for (slot, byte) in group.iter_mut().zip(chunk) {
            *slot = *byte;
        }
        let word = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        for i in 0..4 {
            // A chunk of n bytes yields n + 1 symbols; the rest is padding.
            if i <= chunk.len() {
                let index = (word >> (18 - 6 * i)) & 63;
                out.push(char::from(
                    ALPHABET.get(index as usize).copied().unwrap_or(b'A'),
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard base64. White space is skipped and the padding is optional. `None` for any
/// other character, or when the result would be longer than `max` bytes; nothing is allocated
/// for the latter.
pub fn decode(text: &str, max: usize) -> Option<Vec<u8>> {
    let symbols = text.len();
    if symbols / 4 * 3 > max.saturating_add(3) {
        return None;
    }
    let mut out = Vec::with_capacity(symbols / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut padded = false;
    for &c in text.as_bytes() {
        let sextet = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padded = true;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if padded {
            return None;
        }
        acc = acc << 6 | u32::from(sextet);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits & 0xFF).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    (out.len() <= max).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_4648_vectors() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded, 100).as_deref(), Some(plain.as_bytes()));
        }
    }

    #[test]
    fn decoding_skips_white_space_and_accepts_missing_padding() {
        assert_eq!(decode("Zm9v\r\nYmE", 100).as_deref(), Some(&b"fooba"[..]));
    }

    #[test]
    fn bad_symbols_and_text_after_the_padding_are_refused() {
        assert_eq!(decode("Zm9v*", 100), None);
        assert_eq!(decode("Zg==Zg==", 100), None);
    }

    #[test]
    fn the_size_limit_is_checked() {
        assert_eq!(decode("Zm9vYmFy", 5), None);
        assert_eq!(decode("Zm9vYmFy", 6).map(|v| v.len()), Some(6));
        let long = "A".repeat(1 << 20);
        assert_eq!(decode(&long, 1000), None);
    }
}
