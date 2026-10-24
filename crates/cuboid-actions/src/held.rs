//! The held-key bindings of the input map (menus 6.4).

use crate::{EngineKey, HeldBinding, HeldKeys, parse_binding};

/// The built-in keys of a binding, up to four. The customisable bindings list their slots in the
/// canonical order of menus 6.6.
///
/// spec: menus 6.4, menus 6.6
fn builtin(b: HeldBinding) -> [Option<EngineKey>; 4] {
    use EngineKey::*;
    match b {
        HeldBinding::ModeRotate => [Some(Alpha1), Some(Keypad1), None, None],
        HeldBinding::ModeScale => [Some(Alpha2), Some(Keypad2), None, None],
        HeldBinding::ModeClone => [Some(Alpha3), Some(Keypad3), Some(RightAlt), Some(LeftAlt)],
        HeldBinding::PanView => [Some(Mouse2), Some(Space), None, None],
        HeldBinding::Focus => [Some(F), None, None, None],
        HeldBinding::ModifySelection => [
            Some(RightControl),
            Some(LeftControl),
            Some(RightCommand),
            Some(LeftCommand),
        ],
        HeldBinding::ModifySelectionMultiple => [Some(RightShift), Some(LeftShift), None, None],
        HeldBinding::RotateView => [
            Some(LeftAlt),
            Some(LeftControl),
            Some(RightAlt),
            Some(RightControl),
        ],
        HeldBinding::MoveRight => [Some(RightArrow), None, None, None],
        HeldBinding::MoveLeft => [Some(LeftArrow), None, None, None],
        HeldBinding::MoveUp => [Some(UpArrow), None, None, None],
        HeldBinding::MoveDown => [Some(DownArrow), None, None, None],
        HeldBinding::Submit => [Some(Return), Some(KeypadEnter), None, None],
        HeldBinding::Cancel => [Some(Escape), None, None, None],
    }
}

/// The key table of the held bindings, up to four keys each, with the five customisable
/// bindings replaceable from the settings.
///
/// spec: menus 6.4, menus 6.6, arch 6 (cuboid-actions)
#[derive(Clone, Debug)]
pub struct HeldMap {
    /// Indexed by [`HeldBinding`] in declaration order.
    keys: [[Option<EngineKey>; 4]; HeldBinding::ALL.len()],
}

impl Default for HeldMap {
    /// The built-in bindings of menus 6.4.
    fn default() -> Self {
        Self {
            keys: HeldBinding::ALL.map(builtin),
        }
    }
}

impl HeldMap {
    /// Whether any key of binding `b` is held.
    pub fn is_held(&self, b: HeldBinding, held: &HeldKeys) -> bool {
        self.keys(b).iter().flatten().any(|&key| held.is_down(key))
    }

    /// The current keys of a binding, for Preferences to show in canonical form.
    pub fn keys(&self, b: HeldBinding) -> &[Option<EngineKey>; 4] {
        &self.keys[b as usize]
    }

    /// Replaces the keys of a binding exactly as given; all-`None` leaves it without a key.
    /// Preferences only edits the five customisable ones (menus 6.6).
    pub fn set_custom(&mut self, b: HeldBinding, keys: [Option<EngineKey>; 4]) {
        self.keys[b as usize] = keys;
    }

    /// Applies a stored binding string: empty (after trimming) restores the built-in keys, any
    /// other text is parsed, so a text of unknown names leaves the binding without a key.
    ///
    /// spec: menus 6.6
    pub fn apply_custom(&mut self, b: HeldBinding, text: &str) {
        self.keys[b as usize] = if text.trim().is_empty() {
            builtin(b)
        } else {
            parse_binding(text)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineKey::*;
    use crate::canonical_binding;

    fn held(keys: &[EngineKey]) -> HeldKeys {
        let mut held = HeldKeys::default();
        for &key in keys {
            held.set(key, true);
        }
        held
    }

    /// spec: menus 6.4
    #[test]
    fn a_built_in_binding_is_held_when_any_one_of_its_keys_is() {
        use HeldBinding::*;
        let table: [(HeldBinding, &[EngineKey]); 14] = [
            (ModeRotate, &[Alpha1, Keypad1]),
            (ModeScale, &[Alpha2, Keypad2]),
            (ModeClone, &[Alpha3, Keypad3, RightAlt, LeftAlt]),
            (PanView, &[Mouse2, Space]),
            (Focus, &[F]),
            (
                ModifySelection,
                &[RightControl, LeftControl, RightCommand, LeftCommand],
            ),
            (ModifySelectionMultiple, &[RightShift, LeftShift]),
            (RotateView, &[LeftAlt, LeftControl, RightAlt, RightControl]),
            (MoveRight, &[RightArrow]),
            (MoveLeft, &[LeftArrow]),
            (MoveUp, &[UpArrow]),
            (MoveDown, &[DownArrow]),
            (Submit, &[Return, KeypadEnter]),
            (Cancel, &[Escape]),
        ];
        let map = HeldMap::default();
        for (binding, keys) in table {
            for &key in EngineKey::ALL {
                assert_eq!(
                    map.is_held(binding, &held(&[key])),
                    keys.contains(&key),
                    "{binding:?} {key:?}"
                );
            }
        }
        // Several held keys of one binding, or none at all.
        assert!(map.is_held(ModeClone, &held(&[Alpha3, LeftAlt])));
        assert!(!map.is_held(ModeClone, &HeldKeys::default()));
    }

    /// spec: menus 6.6
    #[test]
    fn the_built_in_keys_read_as_the_preferences_text() {
        let map = HeldMap::default();
        for (binding, text) in [
            (HeldBinding::ModeRotate, "Alpha1, Keypad1"),
            (HeldBinding::ModeScale, "Alpha2, Keypad2"),
            (HeldBinding::ModeClone, "Alpha3, Keypad3, RightAlt, LeftAlt"),
            (HeldBinding::PanView, "Mouse2, Space"),
            (HeldBinding::Focus, "F"),
        ] {
            assert_eq!(canonical_binding(map.keys(binding)), text);
        }
    }

    /// spec: menus 6.6
    #[test]
    fn custom_keys_replace_the_built_in_ones_exactly_as_given() {
        let mut map = HeldMap::default();
        map.set_custom(HeldBinding::Focus, [Some(G), Some(H), None, None]);
        assert!(map.is_held(HeldBinding::Focus, &held(&[H])));
        assert!(!map.is_held(HeldBinding::Focus, &held(&[F])));
        // Other bindings are untouched.
        assert!(map.is_held(HeldBinding::ModeRotate, &held(&[Alpha1])));
        map.set_custom(HeldBinding::Focus, [None; 4]);
        assert!(!map.is_held(HeldBinding::Focus, &held(&[F])));
        assert_eq!(map.keys(HeldBinding::Focus), &[None; 4]);
    }

    /// spec: menus 6.6
    #[test]
    fn an_empty_text_restores_the_built_in_keys_and_unknown_names_leave_no_key() {
        let mut map = HeldMap::default();
        map.apply_custom(HeldBinding::Focus, "G, H");
        assert!(map.is_held(HeldBinding::Focus, &held(&[H])));
        assert!(!map.is_held(HeldBinding::Focus, &held(&[F])));

        map.apply_custom(HeldBinding::Focus, "Nope");
        assert_eq!(map.keys(HeldBinding::Focus), &[None; 4]);
        assert!(!map.is_held(HeldBinding::Focus, &held(&[F])));
        assert_eq!(canonical_binding(map.keys(HeldBinding::Focus)), "");

        for empty in ["", "  "] {
            map.apply_custom(HeldBinding::Focus, "G");
            map.apply_custom(HeldBinding::Focus, empty);
            assert_eq!(map.keys(HeldBinding::Focus), &builtin(HeldBinding::Focus));
        }
    }
}
