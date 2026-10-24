//! Engine key mapping: winit keys and mouse buttons to the original's key codes.
//!
//! egui-winit folds keypad keys into the top-row keys and has no modifier sides (fact F1), so the
//! shell maps the winit events itself. Letters match by the logical key, so Ctrl+Z works wherever
//! Z is printed; every other key matches by the physical key. [`egui_key`] is the way back to
//! egui's keys for headless frames, with the folding egui-winit applies.

use cuboid_actions::EngineKey;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey};

use crate::EngineKeyEvent;

/// A..Z in alphabetical order: the physical key of each letter and its engine key.
const LETTERS: [(KeyCode, EngineKey); 26] = [
    (KeyCode::KeyA, EngineKey::A),
    (KeyCode::KeyB, EngineKey::B),
    (KeyCode::KeyC, EngineKey::C),
    (KeyCode::KeyD, EngineKey::D),
    (KeyCode::KeyE, EngineKey::E),
    (KeyCode::KeyF, EngineKey::F),
    (KeyCode::KeyG, EngineKey::G),
    (KeyCode::KeyH, EngineKey::H),
    (KeyCode::KeyI, EngineKey::I),
    (KeyCode::KeyJ, EngineKey::J),
    (KeyCode::KeyK, EngineKey::K),
    (KeyCode::KeyL, EngineKey::L),
    (KeyCode::KeyM, EngineKey::M),
    (KeyCode::KeyN, EngineKey::N),
    (KeyCode::KeyO, EngineKey::O),
    (KeyCode::KeyP, EngineKey::P),
    (KeyCode::KeyQ, EngineKey::Q),
    (KeyCode::KeyR, EngineKey::R),
    (KeyCode::KeyS, EngineKey::S),
    (KeyCode::KeyT, EngineKey::T),
    (KeyCode::KeyU, EngineKey::U),
    (KeyCode::KeyV, EngineKey::V),
    (KeyCode::KeyW, EngineKey::W),
    (KeyCode::KeyX, EngineKey::X),
    (KeyCode::KeyY, EngineKey::Y),
    (KeyCode::KeyZ, EngineKey::Z),
];

/// The egui keys of [`LETTERS`], in the same order.
const EGUI_LETTERS: [egui::Key; 26] = {
    use egui::Key as K;
    [
        K::A,
        K::B,
        K::C,
        K::D,
        K::E,
        K::F,
        K::G,
        K::H,
        K::I,
        K::J,
        K::K,
        K::L,
        K::M,
        K::N,
        K::O,
        K::P,
        K::Q,
        K::R,
        K::S,
        K::T,
        K::U,
        K::V,
        K::W,
        K::X,
        K::Y,
        K::Z,
    ]
};

/// The left and right Super keys: Windows keys on Windows and Linux, Command keys on macOS.
const SUPER: [EngineKey; 2] = if cfg!(target_os = "macos") {
    [EngineKey::LeftCommand, EngineKey::RightCommand]
} else {
    [EngineKey::LeftWindows, EngineKey::RightWindows]
};

/// The engine event of a winit keyboard event. `None` for OS key repeat and for a key the
/// original has no code for.
///
/// spec: arch 10.3, menus 6.6
pub fn engine_event(
    physical: PhysicalKey,
    logical: &Key,
    location: KeyLocation,
    state: ElementState,
    repeat: bool,
) -> Option<EngineKeyEvent> {
    if repeat {
        return None;
    }
    let key = engine_key(physical, logical, location)?;
    Some(EngineKeyEvent {
        key,
        down: state.is_pressed(),
    })
}

/// The engine event of a mouse button; the back and forward buttons are Mouse3 and Mouse4.
///
/// spec: menus 6.4, menus 6.6
pub fn mouse_event(button: MouseButton, state: ElementState) -> Option<EngineKeyEvent> {
    let key = match button {
        MouseButton::Left => EngineKey::Mouse0,
        MouseButton::Right => EngineKey::Mouse1,
        MouseButton::Middle => EngineKey::Mouse2,
        MouseButton::Back => EngineKey::Mouse3,
        MouseButton::Forward => EngineKey::Mouse4,
        MouseButton::Other(_) => return None,
    };
    Some(EngineKeyEvent {
        key,
        down: state.is_pressed(),
    })
}

/// The engine key of a key; `location` only matters for a key whose physical code winit could
/// not identify: the logical key then names the key and the location its side or the keypad.
fn engine_key(physical: PhysicalKey, logical: &Key, location: KeyLocation) -> Option<EngineKey> {
    if let Key::Character(text) = logical
        && let Some(letter) = logical_letter(text)
    {
        return Some(letter);
    }
    match physical {
        PhysicalKey::Code(code) => {
            // A letter position that prints punctuation or a digit is not a letter key (Dvorak
            // prints ";" where QWERTY has Z); only a key that prints a non-Latin letter or nothing
            // falls back to its position.
            let prints_other =
                matches!(logical, Key::Character(text) if !text.chars().any(char::is_alphabetic));
            if prints_other && letter_key(code).is_some() {
                return None;
            }
            physical_key(code)
        }
        PhysicalKey::Unidentified(_) => located_key(logical, location),
    }
}

/// The letter at a physical position.
fn letter_key(code: KeyCode) -> Option<EngineKey> {
    LETTERS
        .iter()
        .find(|&&(c, _)| c == code)
        .map(|&(_, key)| key)
}

/// Alt+Enter and F11 toggle borderless full screen.
///
/// spec: app 4.4
pub fn toggles_fullscreen(key: EngineKey, alt: bool) -> bool {
    key == EngineKey::F11 || (alt && matches!(key, EngineKey::Return | EngineKey::KeypadEnter))
}

/// The egui key of an engine key, as egui-winit reports it: the keypad digits and operators fold
/// into the main-block keys (fact F1), the keypad multiply key has no egui key, and the Windows
/// and Command keys are egui's Super keys. `None` for the mouse buttons.
///
/// spec: arch 14.1 (Headless), arch 1.2 (fact F1)
pub fn egui_key(key: EngineKey) -> Option<egui::Key> {
    use EngineKey as E;
    use egui::Key as K;
    if let Some(index) = LETTERS.iter().position(|&(_, letter)| letter == key) {
        return Some(EGUI_LETTERS[index]);
    }
    Some(match key {
        E::Backspace => K::Backspace,
        E::Tab => K::Tab,
        E::Return | E::KeypadEnter => K::Enter,
        E::Pause => K::Pause,
        E::Escape => K::Escape,
        E::Space => K::Space,
        E::Quote => K::Quote,
        E::Comma => K::Comma,
        E::Minus | E::KeypadMinus => K::Minus,
        E::Period | E::KeypadPeriod => K::Period,
        E::Slash | E::KeypadDivide => K::Slash,
        E::Alpha0 | E::Keypad0 => K::Num0,
        E::Alpha1 | E::Keypad1 => K::Num1,
        E::Alpha2 | E::Keypad2 => K::Num2,
        E::Alpha3 | E::Keypad3 => K::Num3,
        E::Alpha4 | E::Keypad4 => K::Num4,
        E::Alpha5 | E::Keypad5 => K::Num5,
        E::Alpha6 | E::Keypad6 => K::Num6,
        E::Alpha7 | E::Keypad7 => K::Num7,
        E::Alpha8 | E::Keypad8 => K::Num8,
        E::Alpha9 | E::Keypad9 => K::Num9,
        E::Semicolon => K::Semicolon,
        E::Equals | E::KeypadEquals => K::Equals,
        E::LeftBracket => K::OpenBracket,
        E::Backslash => K::Backslash,
        E::RightBracket => K::CloseBracket,
        E::BackQuote => K::Backtick,
        E::Delete => K::Delete,
        E::KeypadPlus => K::Plus,
        E::UpArrow => K::ArrowUp,
        E::DownArrow => K::ArrowDown,
        E::RightArrow => K::ArrowRight,
        E::LeftArrow => K::ArrowLeft,
        E::Insert => K::Insert,
        E::Home => K::Home,
        E::End => K::End,
        E::PageUp => K::PageUp,
        E::PageDown => K::PageDown,
        E::F1 => K::F1,
        E::F2 => K::F2,
        E::F3 => K::F3,
        E::F4 => K::F4,
        E::F5 => K::F5,
        E::F6 => K::F6,
        E::F7 => K::F7,
        E::F8 => K::F8,
        E::F9 => K::F9,
        E::F10 => K::F10,
        E::F11 => K::F11,
        E::F12 => K::F12,
        E::F13 => K::F13,
        E::F14 => K::F14,
        E::F15 => K::F15,
        E::Numlock => K::NumLock,
        E::CapsLock => K::CapsLock,
        E::ScrollLock => K::ScrollLock,
        E::RightShift => K::ShiftRight,
        E::LeftShift => K::ShiftLeft,
        E::RightControl => K::ControlRight,
        E::LeftControl => K::ControlLeft,
        E::RightAlt => K::AltRight,
        E::LeftAlt => K::AltLeft,
        E::LeftCommand | E::LeftWindows => K::SuperLeft,
        E::RightCommand | E::RightWindows => K::SuperRight,
        _ => return None,
    })
}

/// The letter a single-character logical key prints, in either case.
fn logical_letter(text: &str) -> Option<EngineKey> {
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let index = u32::from(c.to_ascii_uppercase()).checked_sub(u32::from('A'))?;
    LETTERS.get(index as usize).map(|&(_, key)| key)
}

fn physical_key(code: KeyCode) -> Option<EngineKey> {
    use EngineKey as E;
    use KeyCode as C;
    Some(match code {
        C::Backspace => E::Backspace,
        C::Tab => E::Tab,
        C::Enter => E::Return,
        C::Pause => E::Pause,
        C::Escape => E::Escape,
        C::Space => E::Space,
        C::Quote => E::Quote,
        C::Comma => E::Comma,
        C::Minus => E::Minus,
        C::Period => E::Period,
        C::Slash => E::Slash,
        C::Digit0 => E::Alpha0,
        C::Digit1 => E::Alpha1,
        C::Digit2 => E::Alpha2,
        C::Digit3 => E::Alpha3,
        C::Digit4 => E::Alpha4,
        C::Digit5 => E::Alpha5,
        C::Digit6 => E::Alpha6,
        C::Digit7 => E::Alpha7,
        C::Digit8 => E::Alpha8,
        C::Digit9 => E::Alpha9,
        C::Semicolon => E::Semicolon,
        C::Equal => E::Equals,
        C::BracketLeft => E::LeftBracket,
        C::Backslash => E::Backslash,
        C::BracketRight => E::RightBracket,
        C::Backquote => E::BackQuote,
        C::Delete => E::Delete,
        C::Numpad0 => E::Keypad0,
        C::Numpad1 => E::Keypad1,
        C::Numpad2 => E::Keypad2,
        C::Numpad3 => E::Keypad3,
        C::Numpad4 => E::Keypad4,
        C::Numpad5 => E::Keypad5,
        C::Numpad6 => E::Keypad6,
        C::Numpad7 => E::Keypad7,
        C::Numpad8 => E::Keypad8,
        C::Numpad9 => E::Keypad9,
        C::NumpadDecimal => E::KeypadPeriod,
        C::NumpadDivide => E::KeypadDivide,
        C::NumpadMultiply => E::KeypadMultiply,
        C::NumpadSubtract => E::KeypadMinus,
        C::NumpadAdd => E::KeypadPlus,
        C::NumpadEnter => E::KeypadEnter,
        C::NumpadEqual => E::KeypadEquals,
        C::ArrowUp => E::UpArrow,
        C::ArrowDown => E::DownArrow,
        C::ArrowRight => E::RightArrow,
        C::ArrowLeft => E::LeftArrow,
        C::Insert => E::Insert,
        C::Home => E::Home,
        C::End => E::End,
        C::PageUp => E::PageUp,
        C::PageDown => E::PageDown,
        C::F1 => E::F1,
        C::F2 => E::F2,
        C::F3 => E::F3,
        C::F4 => E::F4,
        C::F5 => E::F5,
        C::F6 => E::F6,
        C::F7 => E::F7,
        C::F8 => E::F8,
        C::F9 => E::F9,
        C::F10 => E::F10,
        C::F11 => E::F11,
        C::F12 => E::F12,
        C::F13 => E::F13,
        C::F14 => E::F14,
        C::F15 => E::F15,
        C::NumLock => E::Numlock,
        C::CapsLock => E::CapsLock,
        C::ScrollLock => E::ScrollLock,
        C::ShiftRight => E::RightShift,
        C::ShiftLeft => E::LeftShift,
        C::ControlRight => E::RightControl,
        C::ControlLeft => E::LeftControl,
        C::AltRight => E::RightAlt,
        C::AltLeft => E::LeftAlt,
        C::SuperLeft => SUPER[0],
        C::SuperRight => SUPER[1],
        // The letters match by their logical key first; this is the layout-independent fallback
        // for a layout that prints no Latin letter on the key.
        _ => return letter_key(code),
    })
}

/// The modifiers and keypad keys of a key winit identified only by its logical key.
fn located_key(logical: &Key, location: KeyLocation) -> Option<EngineKey> {
    use EngineKey as E;
    let side = |left, right| match location {
        KeyLocation::Left => Some(left),
        KeyLocation::Right => Some(right),
        _ => None,
    };
    match logical {
        Key::Named(NamedKey::Shift) => side(E::LeftShift, E::RightShift),
        Key::Named(NamedKey::Control) => side(E::LeftControl, E::RightControl),
        Key::Named(NamedKey::Alt) => side(E::LeftAlt, E::RightAlt),
        Key::Named(NamedKey::Super) => side(SUPER[0], SUPER[1]),
        Key::Named(NamedKey::Enter) if location == KeyLocation::Numpad => Some(E::KeypadEnter),
        Key::Character(text) if location == KeyLocation::Numpad => keypad_character(text),
        _ => None,
    }
}

fn keypad_character(text: &str) -> Option<EngineKey> {
    use EngineKey as E;
    Some(match text {
        "0" => E::Keypad0,
        "1" => E::Keypad1,
        "2" => E::Keypad2,
        "3" => E::Keypad3,
        "4" => E::Keypad4,
        "5" => E::Keypad5,
        "6" => E::Keypad6,
        "7" => E::Keypad7,
        "8" => E::Keypad8,
        "9" => E::Keypad9,
        "." => E::KeypadPeriod,
        "/" => E::KeypadDivide,
        "*" => E::KeypadMultiply,
        "-" => E::KeypadMinus,
        "+" => E::KeypadPlus,
        "=" => E::KeypadEquals,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use winit::keyboard::{NativeKeyCode, SmolStr};

    use super::*;

    /// The key of a press, as winit would deliver it.
    fn press(physical: PhysicalKey, logical: &Key, location: KeyLocation) -> Option<EngineKey> {
        let event = engine_event(physical, logical, location, ElementState::Pressed, false);
        event.map(|event| event.key)
    }

    fn physical(code: KeyCode, logical: Key, location: KeyLocation) -> Option<EngineKey> {
        press(PhysicalKey::Code(code), &logical, location)
    }

    fn character(text: &str) -> Key {
        Key::Character(SmolStr::new(text))
    }

    fn unidentified(logical: &Key, location: KeyLocation) -> Option<EngineKey> {
        let native = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        press(native, logical, location)
    }

    #[test]
    fn keypad_digits_differ_from_top_row_digits() {
        use EngineKey as E;
        use KeyCode as C;
        let table = [
            ("0", C::Digit0, C::Numpad0, E::Alpha0, E::Keypad0),
            ("1", C::Digit1, C::Numpad1, E::Alpha1, E::Keypad1),
            ("2", C::Digit2, C::Numpad2, E::Alpha2, E::Keypad2),
            ("3", C::Digit3, C::Numpad3, E::Alpha3, E::Keypad3),
            ("4", C::Digit4, C::Numpad4, E::Alpha4, E::Keypad4),
            ("5", C::Digit5, C::Numpad5, E::Alpha5, E::Keypad5),
            ("6", C::Digit6, C::Numpad6, E::Alpha6, E::Keypad6),
            ("7", C::Digit7, C::Numpad7, E::Alpha7, E::Keypad7),
            ("8", C::Digit8, C::Numpad8, E::Alpha8, E::Keypad8),
            ("9", C::Digit9, C::Numpad9, E::Alpha9, E::Keypad9),
        ];
        // winit reports the same character for both keys; only the code and the location differ.
        for (text, top_code, pad_code, top, pad) in table {
            let top_key = physical(top_code, character(text), KeyLocation::Standard);
            let pad_key = physical(pad_code, character(text), KeyLocation::Numpad);
            assert_eq!((top_key, pad_key), (Some(top), Some(pad)), "digit {text}");
        }
    }

    #[test]
    fn keypad_operators_and_enter_differ_from_the_main_block() {
        let key = |code, text: &str| physical(code, character(text), KeyLocation::Numpad);
        assert_eq!(key(KeyCode::NumpadAdd, "+"), Some(EngineKey::KeypadPlus));
        assert_eq!(
            key(KeyCode::NumpadSubtract, "-"),
            Some(EngineKey::KeypadMinus)
        );
        assert_eq!(
            key(KeyCode::NumpadDecimal, "."),
            Some(EngineKey::KeypadPeriod)
        );
        assert_eq!(
            key(KeyCode::NumpadDivide, "/"),
            Some(EngineKey::KeypadDivide)
        );
        assert_eq!(
            key(KeyCode::NumpadMultiply, "*"),
            Some(EngineKey::KeypadMultiply)
        );
        assert_eq!(
            key(KeyCode::NumpadEqual, "="),
            Some(EngineKey::KeypadEquals)
        );
        assert_eq!(
            physical(KeyCode::Minus, character("-"), KeyLocation::Standard),
            Some(EngineKey::Minus)
        );
        assert_eq!(
            physical(KeyCode::Equal, character("="), KeyLocation::Standard),
            Some(EngineKey::Equals)
        );
        let enter = Key::Named(NamedKey::Enter);
        assert_eq!(
            physical(KeyCode::NumpadEnter, enter.clone(), KeyLocation::Numpad),
            Some(EngineKey::KeypadEnter)
        );
        assert_eq!(
            physical(KeyCode::Enter, enter, KeyLocation::Standard),
            Some(EngineKey::Return)
        );
    }

    #[test]
    fn modifiers_have_sides() {
        let named = |code, key, location| physical(code, Key::Named(key), location);
        let table = [
            (
                KeyCode::ShiftLeft,
                NamedKey::Shift,
                KeyLocation::Left,
                EngineKey::LeftShift,
            ),
            (
                KeyCode::ShiftRight,
                NamedKey::Shift,
                KeyLocation::Right,
                EngineKey::RightShift,
            ),
            (
                KeyCode::ControlLeft,
                NamedKey::Control,
                KeyLocation::Left,
                EngineKey::LeftControl,
            ),
            (
                KeyCode::ControlRight,
                NamedKey::Control,
                KeyLocation::Right,
                EngineKey::RightControl,
            ),
            (
                KeyCode::AltLeft,
                NamedKey::Alt,
                KeyLocation::Left,
                EngineKey::LeftAlt,
            ),
            (
                KeyCode::AltRight,
                NamedKey::Alt,
                KeyLocation::Right,
                EngineKey::RightAlt,
            ),
        ];
        for (code, key, location, expected) in table {
            assert_eq!(named(code, key, location), Some(expected));
        }
    }

    #[test]
    fn super_keys_are_windows_keys_except_on_macos() {
        let super_key = |code, location| physical(code, Key::Named(NamedKey::Super), location);
        let (left, right) = if cfg!(target_os = "macos") {
            (EngineKey::LeftCommand, EngineKey::RightCommand)
        } else {
            (EngineKey::LeftWindows, EngineKey::RightWindows)
        };
        assert_eq!(super_key(KeyCode::SuperLeft, KeyLocation::Left), Some(left));
        assert_eq!(
            super_key(KeyCode::SuperRight, KeyLocation::Right),
            Some(right)
        );
    }

    #[test]
    fn letters_match_by_the_logical_key() {
        // Dvorak: the physical S key prints O, so it is the O key (Ctrl+O is Open).
        assert_eq!(
            physical(KeyCode::KeyS, character("o"), KeyLocation::Standard),
            Some(EngineKey::O)
        );
        // Shift and Caps Lock change the case, not the key.
        assert_eq!(
            physical(KeyCode::KeyZ, character("Z"), KeyLocation::Standard),
            Some(EngineKey::Z)
        );
        // No Latin letter on the key: the physical position decides.
        assert_eq!(
            physical(KeyCode::KeyZ, character("\u{44f}"), KeyLocation::Standard),
            Some(EngineKey::Z)
        );
        // A letter position that prints punctuation is no letter key: Dvorak prints ";" where
        // QWERTY has Z, AZERTY prints "," where QWERTY has M.
        assert_eq!(
            physical(KeyCode::KeyZ, character(";"), KeyLocation::Standard),
            None
        );
        assert_eq!(
            physical(KeyCode::KeyM, character(","), KeyLocation::Standard),
            None
        );
        // A character that is not a single letter does not turn a digit key into a letter.
        assert_eq!(
            physical(KeyCode::Digit2, character("@"), KeyLocation::Standard),
            Some(EngineKey::Alpha2)
        );
    }

    #[test]
    fn navigation_function_and_lock_keys_match_by_the_physical_key() {
        let named = |code, key| physical(code, Key::Named(key), KeyLocation::Standard);
        assert_eq!(
            named(KeyCode::PageUp, NamedKey::PageUp),
            Some(EngineKey::PageUp)
        );
        assert_eq!(
            named(KeyCode::Delete, NamedKey::Delete),
            Some(EngineKey::Delete)
        );
        assert_eq!(
            named(KeyCode::ArrowLeft, NamedKey::ArrowLeft),
            Some(EngineKey::LeftArrow)
        );
        assert_eq!(named(KeyCode::F15, NamedKey::F15), Some(EngineKey::F15));
        assert_eq!(
            named(KeyCode::NumLock, NamedKey::NumLock),
            Some(EngineKey::Numlock)
        );
        assert_eq!(named(KeyCode::F24, NamedKey::F24), None);
    }

    #[test]
    fn a_key_without_a_physical_code_falls_back_to_its_logical_name_and_location() {
        let shift = Key::Named(NamedKey::Shift);
        assert_eq!(
            unidentified(&shift, KeyLocation::Left),
            Some(EngineKey::LeftShift)
        );
        assert_eq!(
            unidentified(&shift, KeyLocation::Right),
            Some(EngineKey::RightShift)
        );
        let control = Key::Named(NamedKey::Control);
        assert_eq!(
            unidentified(&control, KeyLocation::Right),
            Some(EngineKey::RightControl)
        );
        assert_eq!(unidentified(&shift, KeyLocation::Standard), None);
        assert_eq!(
            unidentified(&character("1"), KeyLocation::Numpad),
            Some(EngineKey::Keypad1)
        );
        assert_eq!(
            unidentified(&character("+"), KeyLocation::Numpad),
            Some(EngineKey::KeypadPlus)
        );
        assert_eq!(unidentified(&character("1"), KeyLocation::Standard), None);
        assert_eq!(
            unidentified(&Key::Named(NamedKey::Enter), KeyLocation::Numpad),
            Some(EngineKey::KeypadEnter)
        );
    }

    #[test]
    fn a_release_is_a_key_up_and_os_repeat_is_ignored() {
        let code = PhysicalKey::Code(KeyCode::Numpad1);
        let key = character("1");
        let event = |state, repeat| engine_event(code, &key, KeyLocation::Numpad, state, repeat);
        let up = EngineKeyEvent {
            key: EngineKey::Keypad1,
            down: false,
        };
        let down = EngineKeyEvent { down: true, ..up };
        assert_eq!(event(ElementState::Pressed, false), Some(down));
        assert_eq!(event(ElementState::Released, false), Some(up));
        assert_eq!(event(ElementState::Pressed, true), None);
    }

    #[test]
    fn egui_keys_fold_the_keypad_like_egui_winit() {
        use EngineKey as E;
        use egui::Key as K;
        let table = [
            (E::Keypad1, Some(K::Num1)),
            (E::Alpha1, Some(K::Num1)),
            (E::KeypadPlus, Some(K::Plus)),
            (E::KeypadMinus, Some(K::Minus)),
            (E::KeypadDivide, Some(K::Slash)),
            (E::KeypadPeriod, Some(K::Period)),
            (E::KeypadEnter, Some(K::Enter)),
            (E::Return, Some(K::Enter)),
            (E::KeypadMultiply, None),
            (E::A, Some(K::A)),
            (E::Z, Some(K::Z)),
            (E::Backspace, Some(K::Backspace)),
            (E::LeftShift, Some(K::ShiftLeft)),
            (E::RightControl, Some(K::ControlRight)),
            (E::LeftWindows, Some(K::SuperLeft)),
            (E::Mouse0, None),
        ];
        for (key, expected) in table {
            assert_eq!(egui_key(key), expected, "{key:?}");
        }
    }

    #[test]
    fn mouse_buttons_follow_the_engine_numbering() {
        let key = |button| mouse_event(button, ElementState::Pressed).map(|event| event.key);
        assert_eq!(key(MouseButton::Left), Some(EngineKey::Mouse0));
        assert_eq!(key(MouseButton::Right), Some(EngineKey::Mouse1));
        assert_eq!(key(MouseButton::Middle), Some(EngineKey::Mouse2));
        assert_eq!(key(MouseButton::Back), Some(EngineKey::Mouse3));
        assert_eq!(key(MouseButton::Forward), Some(EngineKey::Mouse4));
        assert_eq!(key(MouseButton::Other(9)), None);
        let release = mouse_event(MouseButton::Left, ElementState::Released);
        assert_eq!(release.map(|event| event.down), Some(false));
    }

    #[test]
    fn alt_enter_and_f11_toggle_full_screen() {
        assert!(toggles_fullscreen(EngineKey::F11, false));
        assert!(toggles_fullscreen(EngineKey::Return, true));
        assert!(toggles_fullscreen(EngineKey::KeypadEnter, true));
        assert!(!toggles_fullscreen(EngineKey::Return, false));
        assert!(!toggles_fullscreen(EngineKey::F10, true));
    }
}
