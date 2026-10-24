//! The embedded Phosphor Regular icon font (2.1.2, MIT) and its codepoint map. Sources and
//! checksums are in `THIRD_PARTY.md`.

use crate::phosphor_map::PHOSPHOR_MAP;

/// The Phosphor Regular font file.
pub const PHOSPHOR_REGULAR: &[u8] = include_bytes!("../icons/phosphor/Phosphor-Regular.ttf");

/// The private-use character of the icon called `name` in the official name list, such as
/// `"cube"` or `"caret-right"`.
pub fn codepoint(name: &str) -> Option<char> {
    let index = PHOSPHOR_MAP
        .binary_search_by_key(&name, |&(known, _)| known)
        .ok()?;
    char::from_u32(PHOSPHOR_MAP[index].1)
}

/// Number of icons in the map.
pub const fn icon_count() -> usize {
    PHOSPHOR_MAP.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_to_private_use_characters() {
        assert_eq!(codepoint("cube"), Some('\u{e1da}'));
        assert_eq!(codepoint("x"), Some('\u{e4f6}'));
        assert_eq!(codepoint("no-such-icon"), None);
        assert!(icon_count() > 1500);
    }

    #[test]
    fn the_map_is_sorted_and_unique() {
        assert!(PHOSPHOR_MAP.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }
}
