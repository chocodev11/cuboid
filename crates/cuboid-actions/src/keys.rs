//! Engine keys and their names (menus 6.1, 6.6).

/// Declares [`EngineKey`] and derives its name table from the variant names, which are the
/// engine's names.
macro_rules! engine_keys {
    (
        $(#[$meta:meta])*
        pub enum EngineKey {
            $($(#[$key_meta:meta])* $key:ident,)+
        }
    ) => {
        $(#[$meta])*
        pub enum EngineKey {
            $($(#[$key_meta])* $key,)+
        }

        impl EngineKey {
            /// Every key in declaration order, which approximates the engine's key-code order.
            pub const ALL: &'static [Self] = &[$(Self::$key),+];

            /// The engine name: `"Alpha1"`, `"Keypad1"`, `"LeftAlt"`, `"Mouse2"`, `"F"`, ...
            ///
            /// spec: menus 6.6
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$key => stringify!($key),)+
                }
            }
        }
    };
}

engine_keys! {
    /// An engine key, named like the original's key codes (menus 6.6), including the mouse
    /// buttons. Keypad digits, keypad plus, minus and enter are distinct from the top-row keys,
    /// and the modifier keys have a left and a right side.
    ///
    /// The declaration order approximates the engine's key-code order, which the quirk of
    /// menus 6.2 step 3 ("first key that went down, in key-code order") depends on
    /// (**UNVERIFIED**).
    ///
    /// spec: menus 6.1, menus 6.6, arch 6 (cuboid-actions), arch 10.3
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
    pub enum EngineKey {
        /// Backspace.
        Backspace,
        /// Tab.
        Tab,
        /// Return (Enter on the main block).
        Return,
        /// Pause.
        Pause,
        /// Escape.
        Escape,
        /// Space.
        Space,
        /// The quote key.
        Quote,
        /// Comma.
        Comma,
        /// Minus on the main block.
        Minus,
        /// Period.
        Period,
        /// Slash.
        Slash,
        /// Top-row digit 0.
        Alpha0,
        /// Top-row digit 1.
        Alpha1,
        /// Top-row digit 2.
        Alpha2,
        /// Top-row digit 3.
        Alpha3,
        /// Top-row digit 4.
        Alpha4,
        /// Top-row digit 5.
        Alpha5,
        /// Top-row digit 6.
        Alpha6,
        /// Top-row digit 7.
        Alpha7,
        /// Top-row digit 8.
        Alpha8,
        /// Top-row digit 9.
        Alpha9,
        /// Semicolon.
        Semicolon,
        /// Equals on the main block.
        Equals,
        /// Left bracket.
        LeftBracket,
        /// Backslash.
        Backslash,
        /// Right bracket.
        RightBracket,
        /// Back quote.
        BackQuote,
        /// Letter A.
        A,
        /// Letter B.
        B,
        /// Letter C.
        C,
        /// Letter D.
        D,
        /// Letter E.
        E,
        /// Letter F.
        F,
        /// Letter G.
        G,
        /// Letter H.
        H,
        /// Letter I.
        I,
        /// Letter J.
        J,
        /// Letter K.
        K,
        /// Letter L.
        L,
        /// Letter M.
        M,
        /// Letter N.
        N,
        /// Letter O.
        O,
        /// Letter P.
        P,
        /// Letter Q.
        Q,
        /// Letter R.
        R,
        /// Letter S.
        S,
        /// Letter T.
        T,
        /// Letter U.
        U,
        /// Letter V.
        V,
        /// Letter W.
        W,
        /// Letter X.
        X,
        /// Letter Y.
        Y,
        /// Letter Z.
        Z,
        /// Delete.
        Delete,
        /// Keypad digit 0.
        Keypad0,
        /// Keypad digit 1.
        Keypad1,
        /// Keypad digit 2.
        Keypad2,
        /// Keypad digit 3.
        Keypad3,
        /// Keypad digit 4.
        Keypad4,
        /// Keypad digit 5.
        Keypad5,
        /// Keypad digit 6.
        Keypad6,
        /// Keypad digit 7.
        Keypad7,
        /// Keypad digit 8.
        Keypad8,
        /// Keypad digit 9.
        Keypad9,
        /// Keypad period.
        KeypadPeriod,
        /// Keypad divide.
        KeypadDivide,
        /// Keypad multiply.
        KeypadMultiply,
        /// Keypad minus.
        KeypadMinus,
        /// Keypad plus.
        KeypadPlus,
        /// Keypad enter.
        KeypadEnter,
        /// Keypad equals.
        KeypadEquals,
        /// Up arrow.
        UpArrow,
        /// Down arrow.
        DownArrow,
        /// Right arrow.
        RightArrow,
        /// Left arrow.
        LeftArrow,
        /// Insert.
        Insert,
        /// Home.
        Home,
        /// End.
        End,
        /// Page up.
        PageUp,
        /// Page down.
        PageDown,
        /// Function key F1.
        F1,
        /// Function key F2.
        F2,
        /// Function key F3.
        F3,
        /// Function key F4.
        F4,
        /// Function key F5.
        F5,
        /// Function key F6.
        F6,
        /// Function key F7.
        F7,
        /// Function key F8.
        F8,
        /// Function key F9.
        F9,
        /// Function key F10.
        F10,
        /// Function key F11.
        F11,
        /// Function key F12.
        F12,
        /// Function key F13.
        F13,
        /// Function key F14.
        F14,
        /// Function key F15.
        F15,
        /// Num lock.
        Numlock,
        /// Caps lock.
        CapsLock,
        /// Scroll lock.
        ScrollLock,
        /// Right shift.
        RightShift,
        /// Left shift.
        LeftShift,
        /// Right control.
        RightControl,
        /// Left control.
        LeftControl,
        /// Right alt.
        RightAlt,
        /// Left alt.
        LeftAlt,
        /// Right command.
        RightCommand,
        /// Left command.
        LeftCommand,
        /// Left Windows key.
        LeftWindows,
        /// Right Windows key.
        RightWindows,
        /// Mouse button 0.
        Mouse0,
        /// Mouse button 1.
        Mouse1,
        /// Mouse button 2.
        Mouse2,
        /// Mouse button 3.
        Mouse3,
        /// Mouse button 4.
        Mouse4,
        /// Mouse button 5.
        Mouse5,
        /// Mouse button 6.
        Mouse6,
    }
}

impl EngineKey {
    /// The key for an engine name, matched case-insensitively after trimming; `None` for an
    /// unknown name.
    ///
    /// spec: menus 6.6
    pub fn from_name(s: &str) -> Option<Self> {
        let s = s.trim();
        Self::ALL
            .iter()
            .copied()
            .find(|key| key.name().eq_ignore_ascii_case(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_engine_names_and_round_trip() {
        for &key in EngineKey::ALL {
            assert_eq!(EngineKey::from_name(key.name()), Some(key));
        }
        let mut names: Vec<_> = EngineKey::ALL.iter().map(|key| key.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EngineKey::ALL.len());
        assert_eq!(EngineKey::Keypad1.name(), "Keypad1");
        assert_eq!(EngineKey::Alpha1.name(), "Alpha1");
        assert_eq!(EngineKey::LeftAlt.name(), "LeftAlt");
        assert_eq!(EngineKey::Mouse2.name(), "Mouse2");
    }

    /// The names that menus 6.6 lists as valid binding names.
    #[test]
    fn the_names_of_the_binding_spec_exist() {
        let digits = (0..=9).flat_map(|d| [format!("Alpha{d}"), format!("Keypad{d}")]);
        let letters = ('A'..='Z').map(String::from);
        let functions = (1..=15).map(|n| format!("F{n}"));
        let mouse = (0..=6).map(|n| format!("Mouse{n}"));
        let others = [
            "Space",
            "Return",
            "LeftAlt",
            "RightAlt",
            "LeftControl",
            "RightControl",
            "LeftShift",
            "RightShift",
            "LeftCommand",
            "RightCommand",
        ]
        .map(String::from);
        for name in digits
            .chain(letters)
            .chain(functions)
            .chain(mouse)
            .chain(others)
        {
            assert!(EngineKey::from_name(&name).is_some(), "{name}");
        }
    }

    #[test]
    fn lookup_trims_and_ignores_case() {
        assert_eq!(EngineKey::from_name("  alpha1 "), Some(EngineKey::Alpha1));
        assert_eq!(EngineKey::from_name("LEFTALT"), Some(EngineKey::LeftAlt));
        assert_eq!(EngineKey::from_name("f"), Some(EngineKey::F));
        assert_eq!(EngineKey::from_name("Ctrl"), None);
        assert_eq!(EngineKey::from_name(""), None);
    }
}
