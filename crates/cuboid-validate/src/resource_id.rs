//! Namespace and path rules of resource locations (`namespace:path`).
//!
//! A namespace holds lower-case letters, digits, `_`, `-` and `.`; a path also holds `/`. Since
//! 1.11 the game does not load textures with an upper-case letter in the path (interop 7.2).

fn bad_char(text: &str, slash: bool) -> Option<char> {
    text.chars().find(|&c| {
        !(c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || matches!(c, '_' | '-' | '.')
            || (slash && c == '/'))
    })
}

/// What is wrong with a resource location, or `None` when it is fine. A location without a
/// namespace belongs to `minecraft`.
pub(crate) fn problem(id: &str) -> Option<String> {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    if namespace.is_empty() {
        return Some("the namespace is empty".to_owned());
    }
    if let Some(c) = bad_char(namespace, false) {
        return Some(describe(c, "namespace"));
    }
    if path.is_empty() {
        return Some("the path is empty".to_owned());
    }
    if let Some(c) = bad_char(path, true) {
        return Some(describe(c, "path"));
    }
    if path.starts_with('/') || path.contains("//") || path.split('/').any(|part| part == "..") {
        return Some("the path has an empty or parent segment".to_owned());
    }
    None
}

fn describe(c: char, part: &str) -> String {
    if c.is_ascii_uppercase() {
        format!("the {part} has the upper-case letter '{c}'")
    } else {
        format!("the {part} has the character {c:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_locations_pass() {
        for id in [
            "block/stone",
            "minecraft:item/sword_1",
            "my-mod.x:a/b.c/d",
            "a",
        ] {
            assert_eq!(problem(id), None, "{id}");
        }
    }

    #[test]
    fn bad_locations_are_described() {
        assert!(problem("Block/Stone").is_some_and(|p| p.contains("upper-case")));
        assert!(problem("minecraft:").is_some());
        assert!(problem(":a").is_some());
        assert!(problem("My Mod:a").is_some());
        assert!(problem("a b").is_some());
        assert!(problem("a/../b").is_some());
        assert!(problem("a//b").is_some());
        assert!(problem("/a").is_some());
        assert!(problem("a\\b").is_some());
        assert!(problem("ns:a:b").is_some());
    }
}
