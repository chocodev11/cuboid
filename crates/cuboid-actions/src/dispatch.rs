//! The shortcut dispatcher (menus 6.2, 6.3, 6.7).

use std::sync::LazyLock;

use crate::{
    ActionId, Chord, DispatchGate, EngineKey, MENUS, MenuId, MenuMask, RowFlags, RowTarget,
    canonical_shortcut,
};

/// A fixed menu binding: the chord of a row's shortcut text, or one of the hidden ones.
///
/// spec: menus 6.3
struct Binding {
    chord: Chord,
    action: ActionId,
    /// The bar menu that registers the binding.
    menu: MenuMask,
    flags: RowFlags,
}

impl Binding {
    fn new(shortcut: &str, action: ActionId, menu: MenuId, flags: RowFlags) -> Self {
        Self {
            chord: canonical_shortcut(shortcut),
            action,
            menu: MenuMask::of(menu),
            flags,
        }
    }

    /// Step 5 of menus 6.2: no keyboard selection in the UI unless the binding works there, and
    /// an enabled menu unless the binding is always active.
    fn allowed(&self, gate: &DispatchGate) -> bool {
        let in_ui = !gate.ui_holds_selection || self.flags.contains(RowFlags::WORKS_IN_UI);
        let enabled = self.flags.contains(RowFlags::ALWAYS_ACTIVE)
            || gate.enabled_menus.intersects(self.menu);
        in_ui && enabled
    }
}

/// The bindings that have no menu row: shortcut text, action and the menu that owns them.
///
/// spec: menus 6.3, menus 2.2, menus 2.3
const HIDDEN: [(&str, ActionId, MenuId); 3] = [
    ("Del", ActionId::DeleteConfirm, MenuId::Edit),
    ("F3", ActionId::SelectName, MenuId::Edit),
    ("F2", ActionId::Rename, MenuId::Objects),
];

/// Every row shortcut of the bar menus plus the hidden bindings: 58 + 3.
static BINDINGS: LazyLock<Box<[Binding]>> = LazyLock::new(|| {
    let rows = MENUS
        .iter()
        .flat_map(|menu| menu.rows.iter().map(|row| (menu.id, row)))
        .filter_map(|(menu, row)| match (row.shortcut, row.target) {
            (Some(shortcut), RowTarget::Action(action)) => {
                Some(Binding::new(shortcut, action, menu, row.flags))
            }
            _ => None,
        });
    let hidden = HIDDEN
        .into_iter()
        .map(|(shortcut, action, menu)| Binding::new(shortcut, action, menu, RowFlags::empty()));
    rows.chain(hidden).collect()
});

/// How the held key list names a key in the combination of menus 6.2 step 4.
fn combo_name(key: EngineKey) -> &'static str {
    match key {
        EngineKey::LeftControl
        | EngineKey::RightControl
        | EngineKey::LeftCommand
        | EngineKey::RightCommand => "Ctrl",
        EngineKey::LeftShift | EngineKey::RightShift => "Shift",
        EngineKey::LeftAlt | EngineKey::RightAlt => "Alt",
        EngineKey::LeftWindows | EngineKey::RightWindows => "Win",
        other => other.name(),
    }
}

/// Turns key events into actions through the chord table of the menus: held keys in press order
/// joined with `+`, gated by [`DispatchGate`]. The default processes each key-down separately
/// in arrival order (the recommendation of menus 6.2); the frame-based quirks are
/// [`Dispatcher::frame_strict`].
///
/// The caller runs the returned actions 0.05 s later (menus 1.5). The shell reports every
/// release, also the synthetic ones of a lost window focus, so no key stays in the list.
///
/// spec: menus 6.2, arch 10.3, arch 6 (cuboid-actions)
#[derive(Debug, Default)]
pub struct Dispatcher {
    /// Held keys in press order.
    held: Vec<EngineKey>,
}

impl Dispatcher {
    /// A key went down: the actions whose chord now matches, in event order.
    ///
    /// Step 1 of menus 6.2 stops everything while the gate is modal and for a left, right or
    /// middle mouse button press, so such a press is not recorded: a modifier pressed under a
    /// dialog does not count after it closes, as in the original.
    pub fn key_down(&mut self, k: EngineKey, gate: &DispatchGate) -> Vec<ActionId> {
        let mouse = matches!(k, EngineKey::Mouse0 | EngineKey::Mouse1 | EngineKey::Mouse2);
        if mouse || gate.modal {
            return Vec::new();
        }
        self.press(k);
        self.fired(gate)
    }

    /// A key went up.
    pub fn key_up(&mut self, k: EngineKey) {
        self.held.retain(|&held| held != k);
    }

    /// The per-frame algorithm of menus 6.2 for parity runs (`Compat::strict()`): stop if a
    /// mouse button went down (`mouse_down`), take only the first key of `downs` in key-code
    /// order. Step 2 of the algorithm is [`Dispatcher::key_up`].
    pub fn frame_strict(
        &mut self,
        downs: &[EngineKey],
        mouse_down: bool,
        gate: &DispatchGate,
    ) -> Vec<ActionId> {
        if mouse_down || gate.modal {
            return Vec::new();
        }
        let Some(&first) = downs.iter().min() else {
            return Vec::new();
        };
        self.press(first);
        self.fired(gate)
    }

    /// Step 3 of menus 6.2: the key goes to the end of the held list.
    fn press(&mut self, k: EngineKey) {
        self.held.retain(|&held| held != k);
        self.held.push(k);
    }

    /// Steps 4 and 5 of menus 6.2.
    fn fired(&self, gate: &DispatchGate) -> Vec<ActionId> {
        let mut combo = String::new();
        for (i, &key) in self.held.iter().enumerate() {
            if i > 0 {
                combo.push('+');
            }
            combo.push_str(combo_name(key));
        }
        BINDINGS
            .iter()
            .filter(|binding| binding.chord.as_str() == combo && binding.allowed(gate))
            .map(|binding| binding.action)
            .collect()
    }
}

#[cfg(test)]
mod tests;
