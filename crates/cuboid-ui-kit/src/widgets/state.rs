//! Interaction states shared by the controls: hover, pressed, selected, disabled and focus.

use egui::{Context, Event, Id, Rect, Response, Stroke, StrokeKind, Ui};

use crate::metrics::FOCUS_RING;
use crate::theme::Tokens;

/// The look a control can have.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlState {
    /// At rest.
    Normal,
    /// The pointer is over it.
    Hover,
    /// The pointer button is down on it.
    Pressed,
    /// It is the current choice.
    Selected,
    /// It ignores input and is dimmed.
    Disabled,
    /// It has keyboard focus: the normal look plus the focus ring.
    Focus,
}

impl ControlState {
    /// Every state, in the order of the gallery.
    pub const ALL: [ControlState; 6] = [
        ControlState::Normal,
        ControlState::Hover,
        ControlState::Pressed,
        ControlState::Selected,
        ControlState::Disabled,
        ControlState::Focus,
    ];

    /// Name for captions.
    pub fn name(self) -> &'static str {
        match self {
            ControlState::Normal => "Normal",
            ControlState::Hover => "Hover",
            ControlState::Pressed => "Pressed",
            ControlState::Selected => "Selected",
            ControlState::Disabled => "Disabled",
            ControlState::Focus => "Focus",
        }
    }
}

const FORCED_ID: &str = "cuboid_ui_kit_forced_state";
const KEYBOARD_ID: &str = "cuboid_ui_kit_keyboard_nav";

/// Runs `add` with every kit control inside drawn in `state`, whatever the pointer does. The
/// component gallery and the tests use it to show states that need a pointer or a key.
pub fn forced_state<R>(ui: &mut Ui, state: ControlState, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = Id::unique(FORCED_ID);
    ui.data_mut(|d| d.insert_temp(id, state));
    let out = add(ui);
    ui.data_mut(|d| d.remove::<ControlState>(id));
    out
}

/// What a control is drawn as in this frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Look {
    /// Fill and text state; never `Focus`.
    pub state: ControlState,
    /// Draw the focus ring.
    pub focus: bool,
}

impl Look {
    pub fn disabled(self) -> bool {
        self.state == ControlState::Disabled
    }
}

/// Whether the last input that mattered came from the keyboard: the focus ring of buttons shows
/// only then, so a click does not leave a ring behind.
fn keyboard_nav(ctx: &Context) -> bool {
    let id = Id::unique(KEYBOARD_ID);
    let pass = ctx.cumulative_pass_nr();
    let (keyboard, pointer) = ctx.input(|i| {
        let keyboard = i
            .events
            .iter()
            .any(|e| matches!(e, Event::Key { pressed: true, .. }));
        let pointer = i
            .events
            .iter()
            .any(|e| matches!(e, Event::PointerButton { pressed: true, .. }));
        (keyboard, pointer)
    });
    ctx.data_mut(|d| {
        let (seen, on) = d.get_temp_mut_or_insert_with(id, || (u64::MAX, false));
        if *seen != pass {
            *seen = pass;
            if pointer {
                *on = false;
            } else if keyboard {
                *on = true;
            }
        }
        *on
    })
}

/// The look of a control from its response. `text_like` controls (fields) show the ring for any
/// focus, buttons only after keyboard use.
pub(crate) fn look(ui: &Ui, response: &Response, selected: bool, text_like: bool) -> Look {
    let forced = ui.data(|d| d.get_temp::<ControlState>(Id::unique(FORCED_ID)));
    if let Some(forced) = forced {
        return match forced {
            ControlState::Focus => Look {
                state: if selected {
                    ControlState::Selected
                } else {
                    ControlState::Normal
                },
                focus: true,
            },
            state => Look {
                state,
                focus: false,
            },
        };
    }
    let state = if !ui.is_enabled() {
        ControlState::Disabled
    } else if response.is_pointer_button_down_on() {
        ControlState::Pressed
    } else if selected {
        ControlState::Selected
    } else if response.hovered() {
        ControlState::Hover
    } else {
        ControlState::Normal
    };
    let focus = response.has_focus() && (text_like || keyboard_nav(ui.ctx()));
    Look { state, focus }
}

/// The focus ring: a thin neutral outline inside `rect`.
pub(crate) fn focus_ring(ui: &Ui, rect: Rect, radius: u8, tokens: &Tokens) {
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(FOCUS_RING, tokens.focus),
        StrokeKind::Inside,
    );
}

/// While the widget `id` has focus, egui leaves the arrow keys to it instead of moving the focus
/// to a neighbour.
pub(crate) fn keep_arrows(ui: &Ui, id: Id, horizontal: bool, vertical: bool) {
    ui.memory_mut(|m| {
        if m.has_focus(id) {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    horizontal_arrows: horizontal,
                    vertical_arrows: vertical,
                    ..egui::EventFilter::default()
                },
            );
        }
    });
}

/// The colour of content (text, icon) that is dimmed when the control is disabled.
pub(crate) fn dimmed(color: egui::Color32, look: Look) -> egui::Color32 {
    if look.disabled() {
        color.gamma_multiply(0.45)
    } else {
        color
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, RawInput, Sense, vec2};

    use super::*;

    #[test]
    fn a_forced_state_wins_over_the_pointer_and_is_removed_after() {
        let ctx = Context::default();
        let output = ctx.run_ui(RawInput::default(), |ui| {
            for (forced, want) in [
                (ControlState::Hover, ControlState::Hover),
                (ControlState::Pressed, ControlState::Pressed),
                (ControlState::Disabled, ControlState::Disabled),
                (ControlState::Selected, ControlState::Selected),
                (ControlState::Focus, ControlState::Normal),
            ] {
                let got = forced_state(ui, forced, |ui| {
                    let (_, r) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::click());
                    look(ui, &r, false, false)
                });
                assert_eq!(got.state, want);
                assert_eq!(got.focus, forced == ControlState::Focus);
            }
            let (_, r) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::click());
            assert_eq!(look(ui, &r, false, false).state, ControlState::Normal);
            assert_eq!(look(ui, &r, true, false).state, ControlState::Selected);
        });
        output.drop_without_applying_deltas();
    }
}
