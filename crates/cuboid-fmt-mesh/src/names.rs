//! Names for files, materials and objects in the exported files.

use std::collections::HashSet;

/// A name that is safe as a file name and as an MTL token: letters, digits, `-`, `_` and `.`
/// stay, every other character becomes `_`. An empty result is `fallback`.
pub(crate) fn token(text: &str, fallback: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim_matches('.');
    if clean.is_empty() {
        fallback.to_owned()
    } else {
        clean.to_owned()
    }
}

/// An object name for an OBJ `g` line: whitespace runs and control characters become `_`, so the
/// name stays one token.
pub(crate) fn group(text: &str, fallback: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    for c in text.chars() {
        if c.is_whitespace() || c.is_control() {
            gap = true;
            continue;
        }
        if gap && !out.is_empty() {
            out.push('_');
        }
        gap = false;
        out.push(c);
    }
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}

/// Makes names unique in the order they are asked for: a taken name gets `_2`, `_3`, ...
#[derive(Default)]
pub(crate) struct Unique(HashSet<String>);

impl Unique {
    pub(crate) fn make(&mut self, name: String) -> String {
        if self.0.insert(name.clone()) {
            return name;
        }
        let mut n = 2_u32;
        loop {
            let next = format!("{name}_{n}");
            if self.0.insert(next.clone()) {
                return next;
            }
            n += 1;
        }
    }
}

/// A file name or relative path as a URI reference: everything but the unreserved characters is
/// percent-encoded.
pub(crate) fn uri(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_keep_one_word() {
        assert_eq!(token("Stone brick/2", "x"), "Stone_brick_2");
        assert_eq!(token("..", "x"), "x");
        assert_eq!(token("grüne Wand", "x"), "grüne_Wand");
    }

    #[test]
    fn groups_collapse_whitespace() {
        assert_eq!(group("  Arm  left\n", "x"), "Arm_left");
        assert_eq!(group(" \t", "x"), "x");
    }

    #[test]
    fn unique_numbers_repeats() {
        let mut u = Unique::default();
        assert_eq!(u.make("a".into()), "a");
        assert_eq!(u.make("a".into()), "a_2");
        assert_eq!(u.make("a".into()), "a_3");
        assert_eq!(u.make("a_2".into()), "a_2_2");
    }

    #[test]
    fn uris_encode_all_but_unreserved() {
        assert_eq!(uri("a b_ü.png"), "a%20b_%C3%BC.png");
    }
}
