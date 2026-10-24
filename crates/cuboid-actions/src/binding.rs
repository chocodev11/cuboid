//! Shortcut texts and binding strings (menus 6.1, 6.6).

use std::borrow::Cow;

use crate::EngineKey;

/// How a menu shortcut text writes a keypad digit: `1 (Num. keypad)`.
const KEYPAD_SUFFIX: &str = " (Num. keypad)";

/// A canonical chord string such as `"Ctrl+Shift+S"` or `"KeypadPlus"`: the key of a menu
/// binding after the conversion of menus 6.1.
///
/// spec: menus 6.1, arch 6 (cuboid-actions)
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Chord(Box<str>);

impl Chord {
    /// The canonical text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Converts the shortcut text of a menu row to its canonical chord: a single character other
/// than "+" and "-" is upper-cased, "+" and "-" are the keypad keys, `PgUp`, `PgDown`, `Del` and
/// the "(Num. keypad)" digits are renamed, the other tokens are kept or upper-cased.
///
/// spec: menus 6.1
pub fn canonical_shortcut(display: &str) -> Chord {
    let mut chars = display.chars();
    let canonical = match (chars.next(), chars.next()) {
        (Some('+'), None) => "KeypadPlus".to_owned(),
        (Some('-'), None) => "KeypadMinus".to_owned(),
        (Some(_), None) => display.to_uppercase(),
        _ => display
            .split('+')
            .map(canonical_token)
            .collect::<Vec<_>>()
            .join("+"),
    };
    Chord(canonical.into_boxed_str())
}

fn canonical_token(token: &str) -> Cow<'_, str> {
    match token {
        "Ctrl" | "Shift" | "Alt" => return Cow::Borrowed(token),
        "PgUp" => return "PageUp".into(),
        "PgDown" => return "PageDown".into(),
        "Del" => return "Delete".into(),
        _ => {}
    }
    if let Some(digit) = token.strip_suffix(KEYPAD_SUFFIX) {
        return format!("Keypad{digit}").into();
    }
    // The engine key whose name equals the upper-cased token (matched case-insensitively, so
    // "Delete" and "Alpha1" resolve to their key names as menus 6.1 describes); a token without
    // such a key stays upper-cased and never matches a pressed combination.
    EngineKey::from_name(token).map_or_else(|| token.to_uppercase().into(), |key| key.name().into())
}

/// Parses a custom binding string (`advKeysRotate` and its siblings): up to four engine key
/// names separated by commas fill the slots positive, negative, alt-positive, alt-negative; an
/// unknown name leaves its slot empty and tokens after the fourth are ignored.
///
/// spec: menus 6.6
pub fn parse_binding(s: &str) -> [Option<EngineKey>; 4] {
    let mut keys = [None; 4];
    for (slot, token) in keys.iter_mut().zip(s.split(',')) {
        *slot = EngineKey::from_name(token);
    }
    keys
}

/// The canonical text of a binding: the non-empty slots in slot order, separated by `", "`
/// (QUIRK: a leading `", "` appears when the first slot is empty).
///
/// spec: menus 6.6
pub fn canonical_binding(keys: &[Option<EngineKey>; 4]) -> String {
    let mut text = String::new();
    for (slot, key) in keys.iter().enumerate() {
        let Some(key) = key else { continue };
        if slot > 0 {
            text.push_str(", ");
        }
        text.push_str(key.name());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(display: &str) -> String {
        canonical_shortcut(display).as_str().to_owned()
    }

    /// spec: menus 6.1
    #[test]
    fn a_single_character_is_upper_cased_and_plus_and_minus_are_keypad_keys() {
        assert_eq!(canonical("+"), "KeypadPlus");
        assert_eq!(canonical("-"), "KeypadMinus");
        assert_eq!(canonical("n"), "N");
        assert_eq!(canonical("N"), "N");
        assert_eq!(canonical("1"), "1");
    }

    /// spec: menus 6.1
    #[test]
    fn tokens_are_renamed_or_kept() {
        assert_eq!(canonical("PgUp"), "PageUp");
        assert_eq!(canonical("PgDown"), "PageDown");
        assert_eq!(canonical("Ctrl+Del"), "Ctrl+Delete");
        assert_eq!(canonical("Ctrl+Shift+I"), "Ctrl+Shift+I");
        assert_eq!(canonical("Alt+Space"), "Alt+Space");
        assert_eq!(canonical("F12"), "F12");
        assert_eq!(canonical("f5"), "F5");
        assert_eq!(canonical("Ctrl+1 (Num. keypad)"), "Ctrl+Keypad1");
        assert_eq!(canonical("Ctrl+0 (Num. keypad)"), "Ctrl+Keypad0");
        assert_eq!(canonical("Ctrl+Alpha1"), "Ctrl+Alpha1");
        assert_eq!(canonical("Ctrl+Unknown"), "Ctrl+UNKNOWN");
    }

    /// spec: menus 6.6
    #[test]
    fn a_binding_string_fills_the_slots_in_order() {
        use EngineKey::*;
        assert_eq!(
            parse_binding("Alpha3, Keypad3, LeftAlt, RightAlt"),
            [Some(Alpha3), Some(Keypad3), Some(LeftAlt), Some(RightAlt)]
        );
        assert_eq!(
            parse_binding(" mouse2 ,SPACE"),
            [Some(Mouse2), Some(Space), None, None]
        );
        assert_eq!(parse_binding("F"), [Some(F), None, None, None]);
    }

    /// spec: menus 6.6
    #[test]
    fn unknown_names_leave_their_slot_empty_and_extra_tokens_are_ignored() {
        use EngineKey::*;
        assert_eq!(parse_binding(""), [None; 4]);
        assert_eq!(parse_binding("Nope, A"), [None, Some(A), None, None]);
        assert_eq!(
            parse_binding("A, B, C, D, E"),
            [Some(A), Some(B), Some(C), Some(D)]
        );
    }

    /// spec: menus 6.6
    #[test]
    fn the_canonical_binding_lists_the_non_empty_slots_with_the_leading_quirk() {
        use EngineKey::*;
        assert_eq!(
            canonical_binding(&[Some(Alpha1), Some(Keypad1), None, None]),
            "Alpha1, Keypad1"
        );
        assert_eq!(canonical_binding(&[Some(F), None, None, None]), "F");
        assert_eq!(canonical_binding(&[Some(A), None, Some(B), None]), "A, B");
        assert_eq!(canonical_binding(&[None, Some(A), None, None]), ", A");
        assert_eq!(canonical_binding(&[None; 4]), "");
    }

    /// spec: menus 6.6
    #[test]
    fn the_first_run_strings_survive_a_round_trip() {
        for text in [
            "Alpha1, Keypad1",
            "Alpha2, Keypad2",
            "Alpha3, Keypad3, LeftAlt, RightAlt",
            "Mouse2, Space",
            "F",
        ] {
            assert_eq!(canonical_binding(&parse_binding(text)), text);
        }
    }
}
