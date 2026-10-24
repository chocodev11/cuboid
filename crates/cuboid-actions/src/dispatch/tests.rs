//! The dispatcher against menus 6.1-6.3 and 6.7.

use std::collections::HashSet;

use super::*;
use crate::EngineKey::*;
use crate::{canonical_shortcut, spec};

fn open_gate() -> DispatchGate {
    DispatchGate {
        modal: false,
        ui_holds_selection: false,
        enabled_menus: MenuMask::all(),
    }
}

/// Presses the keys one after the other and returns what the last press fired.
fn press(dispatcher: &mut Dispatcher, keys: &[EngineKey], gate: &DispatchGate) -> Vec<ActionId> {
    let mut fired = Vec::new();
    for &key in keys {
        fired = dispatcher.key_down(key, gate);
    }
    fired
}

fn fire(keys: &[EngineKey], gate: &DispatchGate) -> Vec<ActionId> {
    press(&mut Dispatcher::default(), keys, gate)
}

/// One binding of the table in menus 6.3, with the grouped F5 .. F12 row expanded.
struct Expected {
    display: String,
    chord: String,
    action: String,
    menu: MenuMask,
    always_active: bool,
    works_in_ui: bool,
    hidden: bool,
}

fn expected_bindings() -> Vec<Expected> {
    let mut out = Vec::new();
    for row in spec::table(spec::section("6.3")) {
        let hidden = row["Display"].starts_with("(hidden)");
        let display = row["Display"].trim_start_matches("(hidden) ");
        let actions: Vec<_> = row["Action"].split(", ").collect();
        let (displays, chords): (Vec<_>, Vec<_>) = if actions.len() > 1 {
            (5..=12).map(|n| (format!("F{n}"), format!("F{n}"))).unzip()
        } else {
            (vec![display.to_owned()], vec![row["Canonical"].clone()])
        };
        assert_eq!(chords.len(), actions.len());
        let menu = MenuMask::from_name(&row["Menu"].to_uppercase()).expect("a bar menu");
        for ((display, chord), action) in displays.into_iter().zip(chords).zip(actions) {
            out.push(Expected {
                display,
                chord,
                // "Delete (no confirmation)" is the action Delete.
                action: action.split(" (").next().unwrap_or(action).to_owned(),
                menu,
                always_active: row["A"] == "A",
                works_in_ui: row["U"] == "U",
                hidden,
            });
        }
    }
    out
}

/// spec: menus 6.3
#[test]
fn the_default_bindings_equal_the_spec_table() {
    let expected = expected_bindings();
    assert_eq!(expected.len(), 61);
    assert_eq!(expected.iter().filter(|e| e.hidden).count(), 3);
    assert_eq!(BINDINGS.len(), 61);

    let expected: HashSet<_> = expected
        .iter()
        .map(|e| {
            (
                e.chord.clone(),
                e.action.clone(),
                e.menu,
                e.always_active,
                e.works_in_ui,
            )
        })
        .collect();
    let actual: HashSet<_> = BINDINGS
        .iter()
        .map(|b| {
            let flags = b.flags;
            (
                b.chord.as_str().to_owned(),
                b.action.def().name.to_owned(),
                b.menu,
                flags.contains(RowFlags::ALWAYS_ACTIVE),
                flags.contains(RowFlags::WORKS_IN_UI),
            )
        })
        .collect();
    // No two live bindings share a canonical string.
    let chords: HashSet<_> = BINDINGS.iter().map(|b| b.chord.as_str()).collect();
    assert_eq!(chords.len(), 61);
    let missing: Vec<_> = expected.difference(&actual).collect();
    let extra: Vec<_> = actual.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "missing {missing:?}, unexpected {extra:?}"
    );
}

/// spec: menus 6.1
#[test]
fn the_display_texts_canonicalise_to_the_spec_chords() {
    for e in expected_bindings() {
        assert_eq!(
            canonical_shortcut(&e.display).as_str(),
            e.chord,
            "{}",
            e.display
        );
    }
}

/// The key presses that type a canonical chord: modifiers first, in the written order.
fn typed(chord: &str) -> Vec<EngineKey> {
    chord
        .split('+')
        .map(|token| match token {
            "Ctrl" => LeftControl,
            "Shift" => LeftShift,
            "Alt" => LeftAlt,
            name => EngineKey::from_name(name).expect("a key name"),
        })
        .collect()
}

/// spec: menus 6.2, menus 6.3
#[test]
fn every_default_binding_fires_its_action_when_typed() {
    let gate = open_gate();
    for binding in BINDINGS.iter() {
        let keys = typed(binding.chord.as_str());
        let mut dispatcher = Dispatcher::default();
        let (last, modifiers) = keys.split_last().expect("a key");
        for &modifier in modifiers {
            assert!(dispatcher.key_down(modifier, &gate).is_empty());
        }
        let fired = dispatcher.key_down(*last, &gate);
        assert_eq!(fired, [binding.action], "{}", binding.chord.as_str());
    }
}

/// spec: menus 6.2 steps 3-4
#[test]
fn modifiers_must_be_pressed_before_the_key_in_the_written_order() {
    let gate = open_gate();
    assert_eq!(fire(&[LeftControl, N], &gate), [ActionId::NewFile]);
    assert!(fire(&[N, LeftControl], &gate).is_empty());
    assert_eq!(
        fire(&[LeftControl, LeftShift, I], &gate),
        [ActionId::SelectInverse]
    );
    assert!(fire(&[LeftShift, LeftControl, I], &gate).is_empty());
}

/// spec: menus 6.2 step 4
#[test]
fn either_side_of_a_modifier_and_the_command_keys_name_the_same_combination() {
    let gate = open_gate();
    for ctrl in [LeftControl, RightControl, LeftCommand, RightCommand] {
        assert_eq!(fire(&[ctrl, N], &gate), [ActionId::NewFile], "{ctrl:?}");
    }
    for shift in [LeftShift, RightShift] {
        assert_eq!(
            fire(&[shift, P], &gate),
            [ActionId::ElementProperties],
            "{shift:?}"
        );
    }
    // Two keys with the same name make a different combination.
    assert!(fire(&[LeftControl, RightControl, N], &gate).is_empty());
}

/// spec: menus 6.2, menus 6.7 (an extra held key prevents a match)
#[test]
fn an_extra_held_key_prevents_a_match_until_it_is_released() {
    let gate = open_gate();
    let mut dispatcher = Dispatcher::default();
    assert!(press(&mut dispatcher, &[A, LeftControl, N], &gate).is_empty());
    dispatcher.key_up(A);
    dispatcher.key_up(N);
    assert_eq!(dispatcher.key_down(N, &gate), [ActionId::NewFile]);
}

/// spec: menus 6.2 step 2
#[test]
fn releasing_a_key_removes_it_from_the_combination() {
    let gate = open_gate();
    let mut dispatcher = Dispatcher::default();
    assert_eq!(
        press(&mut dispatcher, &[LeftControl, S], &gate),
        [ActionId::Save]
    );
    dispatcher.key_up(S);
    assert_eq!(dispatcher.key_down(N, &gate), [ActionId::NewFile]);
    dispatcher.key_up(N);
    dispatcher.key_up(LeftControl);
    assert!(dispatcher.key_down(N, &gate).is_empty());
}

/// spec: menus 6.1
#[test]
fn the_keypad_keys_differ_from_the_top_row_keys() {
    let gate = open_gate();
    assert_eq!(fire(&[KeypadPlus], &gate), [ActionId::ZoomPlus]);
    assert_eq!(fire(&[KeypadMinus], &gate), [ActionId::ZoomMinus]);
    assert!(fire(&[Equals], &gate).is_empty());
    assert!(fire(&[Minus], &gate).is_empty());
    assert_eq!(
        fire(&[LeftControl, Keypad1], &gate),
        [ActionId::MoveManagersOnLeft]
    );
    assert!(fire(&[LeftControl, Alpha1], &gate).is_empty());
}

/// spec: menus 6.3 (hidden bindings)
#[test]
fn the_hidden_bindings_fire_next_to_the_row_shortcuts() {
    let gate = open_gate();
    assert_eq!(fire(&[Delete], &gate), [ActionId::DeleteConfirm]);
    assert_eq!(fire(&[LeftControl, Delete], &gate), [ActionId::Delete]);
    assert_eq!(fire(&[F3], &gate), [ActionId::SelectName]);
    assert_eq!(fire(&[LeftControl, F], &gate), [ActionId::SelectName]);
    assert_eq!(fire(&[F2], &gate), [ActionId::Rename]);
}

/// spec: menus 6.2 step 1, menus 6.7
#[test]
fn nothing_fires_while_a_modal_window_is_open() {
    let modal = DispatchGate {
        modal: true,
        ..open_gate()
    };
    for binding in BINDINGS.iter() {
        assert!(fire(&typed(binding.chord.as_str()), &modal).is_empty());
    }
}

/// spec: menus 6.2 step 1
#[test]
fn a_key_pressed_under_a_modal_window_is_not_held_after_it_closes() {
    let gate = open_gate();
    let modal = DispatchGate {
        modal: true,
        ..gate
    };
    let mut dispatcher = Dispatcher::default();
    assert!(dispatcher.key_down(LeftControl, &modal).is_empty());
    assert!(dispatcher.key_down(Z, &gate).is_empty());
    dispatcher.key_up(Z);
    // Ctrl was never recorded, so it takes a fresh press to get the combination.
    assert_eq!(
        press(&mut dispatcher, &[LeftControl, Z], &gate),
        [ActionId::Undo]
    );
}

/// spec: menus 6.2 step 5, menus 6.7
#[test]
fn only_works_in_ui_bindings_fire_while_a_control_holds_keyboard_selection() {
    let gate = DispatchGate {
        ui_holds_selection: true,
        ..open_gate()
    };
    assert_eq!(fire(&[LeftControl, S], &gate), [ActionId::Save]);
    assert!(fire(&[LeftControl, N], &gate).is_empty());
    assert!(fire(&[F5], &gate).is_empty());
}

/// spec: menus 6.2 step 5, menus 1.6
#[test]
fn a_disabled_menu_silences_its_bindings_except_the_always_active_ones() {
    let off = DispatchGate {
        enabled_menus: MenuMask::empty(),
        ..open_gate()
    };
    // File: Ctrl+N is always active, Ctrl+E is not.
    assert_eq!(fire(&[LeftControl, N], &off), [ActionId::NewFile]);
    assert!(fire(&[LeftControl, E], &off).is_empty());
    // Objects: PgUp is always active, Shift+P is not.
    assert_eq!(fire(&[PageUp], &off), [ActionId::Next]);
    assert!(fire(&[LeftShift, P], &off).is_empty());
    assert!(fire(&[F2], &off).is_empty());
    assert_eq!(fire(&[F5], &off), [ActionId::ElementTools]);

    let objects = DispatchGate {
        enabled_menus: MenuMask::OBJECTS,
        ..off
    };
    assert_eq!(
        fire(&[LeftShift, P], &objects),
        [ActionId::ElementProperties]
    );
    assert_eq!(fire(&[F2], &objects), [ActionId::Rename]);
    assert!(fire(&[LeftControl, Z], &objects).is_empty());
    let edit = DispatchGate {
        enabled_menus: MenuMask::EDIT,
        ..off
    };
    assert_eq!(fire(&[LeftControl, Z], &edit), [ActionId::Undo]);
}

/// spec: menus 6.2 step 1, menus 6.7 (a mouse press cancels shortcut processing)
#[test]
fn a_press_of_the_left_right_or_middle_button_fires_nothing_and_is_not_held() {
    let gate = open_gate();
    for button in [Mouse0, Mouse1, Mouse2] {
        let mut dispatcher = Dispatcher::default();
        dispatcher.key_down(LeftControl, &gate);
        assert!(dispatcher.key_down(button, &gate).is_empty());
        assert_eq!(
            dispatcher.key_down(N, &gate),
            [ActionId::NewFile],
            "{button:?}"
        );
    }
    // The other buttons are held keys like any other (the combination names them "Mouse3").
    assert!(fire(&[LeftControl, Mouse3, N], &gate).is_empty());
}

/// spec: menus 6.2 (frame algorithm, `Compat::strict`)
#[test]
fn the_strict_frame_takes_one_key_per_frame_in_key_code_order() {
    let gate = open_gate();
    let mut dispatcher = Dispatcher::default();
    assert!(
        dispatcher
            .frame_strict(&[LeftControl], false, &gate)
            .is_empty()
    );
    // N and A went down together: A comes first and N is lost.
    assert_eq!(
        dispatcher.frame_strict(&[N, A], false, &gate),
        [ActionId::SelectAllElements]
    );
    dispatcher.key_up(A);
    assert_eq!(
        dispatcher.frame_strict(&[D], false, &gate),
        [ActionId::SelectNone]
    );

    // Letters come before function keys, so Z wins over F5 and nothing fires.
    assert!(
        Dispatcher::default()
            .frame_strict(&[F5, Z], false, &gate)
            .is_empty()
    );
    assert_eq!(
        Dispatcher::default().key_down(F5, &gate),
        [ActionId::ElementTools]
    );
}

/// spec: menus 6.2 step 1
#[test]
fn the_strict_frame_stops_for_a_mouse_press_a_modal_window_or_no_key() {
    let gate = open_gate();
    let modal = DispatchGate {
        modal: true,
        ..gate
    };
    let mut dispatcher = Dispatcher::default();
    assert!(
        dispatcher
            .frame_strict(&[LeftControl], false, &gate)
            .is_empty()
    );
    assert!(dispatcher.frame_strict(&[N], true, &gate).is_empty());
    assert!(dispatcher.frame_strict(&[N], false, &modal).is_empty());
    assert!(dispatcher.frame_strict(&[], false, &gate).is_empty());
    // None of the stopped frames recorded N, so the next one still sees Ctrl alone.
    assert_eq!(
        dispatcher.frame_strict(&[S], false, &gate),
        [ActionId::Save]
    );
}
