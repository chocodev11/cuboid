//! Chords, commands and the keymap presets.
//!
//! The UI reads egui key events, so a chord is an egui key plus Ctrl (Command on macOS), Shift
//! and Alt. Keypad digits cannot be told from the top row there; the view shortcuts therefore use
//! the digit keys, like the "emulate numpad" option of other editors.

use std::fmt;

use cuboid_actions::{ACTIONS, ActionId, ActionKind, MENUS, RowTarget, canonical_shortcut};
use egui::{Event, InputState, Key};
use serde::{Deserialize, Serialize};

use crate::layout::{PanelId, ViewLayout};
use crate::request::{Mode, Tool, ViewPreset};

/// A key with modifiers.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Chord {
    /// The key.
    pub key: Key,
    /// Ctrl, or Command on macOS.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Alt.
    pub alt: bool,
}

impl Chord {
    /// A chord without modifiers.
    pub fn plain(key: Key) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    /// Reads `"Ctrl+Shift+S"`, `"F3"`, `"Alt+A"`, `"Delete"`. Modifier words are case-insensitive;
    /// the key is the last token, so `"Ctrl++"` is not supported (use `Plus`).
    pub fn parse(text: &str) -> Option<Chord> {
        let mut chord = Chord::plain(Key::Escape);
        let mut tokens: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = tokens.pop()?;
        for token in tokens {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" => chord.alt = true,
                _ => return None,
            }
        }
        chord.key = Key::from_name(key)?;
        Some(chord)
    }

    /// The chord of a key event, `None` for releases and key repeats.
    pub fn of_event(event: &Event) -> Option<Chord> {
        let Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } = event
        else {
            return None;
        };
        Some(Chord {
            key: *key,
            ctrl: modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
        })
    }

    /// Whether the chord still works while a text field has the keyboard: function keys, and
    /// Ctrl chords that are not text editing (copy, paste, undo and select all stay with the
    /// field).
    pub fn works_while_typing(&self) -> bool {
        let editing = matches!(
            self.key,
            Key::Z | Key::Y | Key::X | Key::C | Key::V | Key::A
        );
        let function = matches!(
            self.key,
            Key::F1
                | Key::F2
                | Key::F3
                | Key::F4
                | Key::F5
                | Key::F6
                | Key::F7
                | Key::F8
                | Key::F9
                | Key::F10
                | Key::F11
                | Key::F12
        );
        function || (self.ctrl && !editing)
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        f.write_str(self.key.name())
    }
}

/// The chords pressed this frame, in order.
pub fn pressed_chords(input: &InputState) -> Vec<Chord> {
    input.events.iter().filter_map(Chord::of_event).collect()
}

/// Commands that belong to the window rather than to the document.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum UiCommand {
    /// Open the command palette.
    Palette,
    /// Show the start screen.
    ShowStart,
    /// Open the preferences.
    Preferences,
    /// Open the preferences at the keymap.
    KeyboardShortcuts,
    /// Show or hide a panel.
    TogglePanel(PanelId),
    /// Put the panels back where they started.
    ResetLayout,
    /// Choose how many views are shown.
    SetViewLayout(ViewLayout),
    /// Switch to a workspace.
    SetMode(Mode),
    /// Switch to the next workspace.
    NextMode,
    /// Choose a tool.
    SetTool(Tool),
    /// Snap the active view to a direction.
    View(ViewPreset),
    /// Frame the selection in the active view.
    FrameSelected,
    /// Start the chosen animation, or stop it where it is.
    PlayPause,
    /// Put the playhead one frame back.
    PreviousFrame,
    /// Put the playhead one frame on.
    NextFrame,
    /// Put the playhead at the start of the animation.
    FirstFrame,
    /// Put the playhead at the end of the animation.
    LastFrame,
    /// Key every channel of the selected bones at the playhead.
    InsertKeyframe,
    /// Add an animation.
    NewAnimation,
    /// Turn snapping on or off.
    ToggleSnap,
    /// Put the selection into a group.
    GroupSelection,
    /// Dissolve the selected groups.
    Ungroup,
    /// Switch between the dark and light theme.
    ToggleTheme,
    /// Show the validator tab of the properties area.
    ShowValidator,
}

/// Every window command, for the palette and for reading saved keymaps.
pub fn all_ui_commands() -> Vec<UiCommand> {
    let mut all = vec![
        UiCommand::Palette,
        UiCommand::ShowStart,
        UiCommand::Preferences,
        UiCommand::KeyboardShortcuts,
    ];
    all.extend(PanelId::ALL.into_iter().map(UiCommand::TogglePanel));
    all.push(UiCommand::ResetLayout);
    all.extend(ViewLayout::ALL.into_iter().map(UiCommand::SetViewLayout));
    all.extend(Mode::ALL.into_iter().map(UiCommand::SetMode));
    all.push(UiCommand::NextMode);
    for mode in Mode::ALL {
        all.extend(mode.tools().iter().copied().map(UiCommand::SetTool));
    }
    all.extend(
        [
            ViewPreset::Front,
            ViewPreset::Back,
            ViewPreset::Right,
            ViewPreset::Left,
            ViewPreset::Top,
            ViewPreset::Bottom,
            ViewPreset::ToggleProjection,
        ]
        .into_iter()
        .map(UiCommand::View),
    );
    all.extend([
        UiCommand::FrameSelected,
        UiCommand::PlayPause,
        UiCommand::PreviousFrame,
        UiCommand::NextFrame,
        UiCommand::FirstFrame,
        UiCommand::LastFrame,
        UiCommand::InsertKeyframe,
        UiCommand::NewAnimation,
        UiCommand::ToggleSnap,
        UiCommand::GroupSelection,
        UiCommand::Ungroup,
        UiCommand::ToggleTheme,
        UiCommand::ShowValidator,
    ]);
    all
}

/// Something a key or a palette entry can run.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Command {
    /// A registry action.
    Action(ActionId),
    /// A window command.
    Ui(UiCommand),
}

impl Command {
    /// A stable text id for saved keymaps.
    pub fn id(self) -> String {
        match self {
            Command::Action(id) => format!("action:{id:?}"),
            Command::Ui(c) => format!("ui:{c:?}"),
        }
    }

    /// Reads an id written by [`Command::id`].
    pub fn from_id(text: &str) -> Option<Command> {
        if let Some(name) = text.strip_prefix("action:") {
            return cuboid_actions::ACTIONS
                .iter()
                .map(|def| def.id)
                .find(|id| format!("{id:?}") == name)
                .map(Command::Action);
        }
        let name = text.strip_prefix("ui:")?;
        all_ui_commands()
            .into_iter()
            .find(|c| format!("{c:?}") == name)
            .map(Command::Ui)
    }
}

/// The built-in keymaps.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum KeymapPreset {
    /// Letter keys for G, R, S style tool changes, `A` to select all.
    #[default]
    Blender,
    /// `V` to move, Ctrl+D to duplicate, Ctrl+A to select all.
    Blockbench,
    /// The shortcuts of Cubik Studio's menus.
    Cubik,
}

impl KeymapPreset {
    /// All presets.
    pub const ALL: [KeymapPreset; 3] = [
        KeymapPreset::Blender,
        KeymapPreset::Blockbench,
        KeymapPreset::Cubik,
    ];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            KeymapPreset::Blender => "Blender-style",
            KeymapPreset::Blockbench => "Blockbench-style",
            KeymapPreset::Cubik => "Cubik-style",
        }
    }
}

/// A user change on top of a preset, stored in the preferences.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyOverride {
    /// A [`Command::id`].
    pub command: String,
    /// The chord text, or `None` to leave the command unbound.
    pub chord: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Binding {
    chord: Chord,
    command: Command,
    /// Only in this mode; `None` works everywhere.
    mode: Option<Mode>,
}

/// Chords to commands.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

type Table = &'static [(&'static str, Command, Option<Mode>)];

const fn act(id: ActionId) -> Command {
    Command::Action(id)
}

const fn ui(c: UiCommand) -> Command {
    Command::Ui(c)
}

const fn tool(t: Tool) -> Command {
    Command::Ui(UiCommand::SetTool(t))
}

const fn view(p: ViewPreset) -> Command {
    Command::Ui(UiCommand::View(p))
}

const M: Option<Mode> = Some(Mode::Model);
const V: Option<Mode> = Some(Mode::Voxel);
const P: Option<Mode> = Some(Mode::Paint);
const A: Option<Mode> = Some(Mode::Animate);

/// The keys of the timeline in Animate, for every preset. Play and pause are on Space in the
/// Blender and Blockbench presets; the Cubik navigation pans with Space.
const PLAYBACK: Table = &[
    ("Left", ui(UiCommand::PreviousFrame), A),
    ("Right", ui(UiCommand::NextFrame), A),
    ("Shift+Left", ui(UiCommand::FirstFrame), A),
    ("Shift+Right", ui(UiCommand::LastFrame), A),
];

/// Bindings of both presets.
const COMMON: Table = &[
    ("Ctrl+N", act(ActionId::NewFile), None),
    ("Ctrl+O", act(ActionId::Open), None),
    ("Ctrl+S", act(ActionId::Save), None),
    ("Ctrl+Shift+S", act(ActionId::SaveAs), None),
    ("Ctrl+E", act(ActionId::Export), None),
    ("Ctrl+Q", act(ActionId::Exit), None),
    ("Ctrl+Z", act(ActionId::Undo), None),
    ("Ctrl+Shift+Z", act(ActionId::Redo), None),
    ("Ctrl+Y", act(ActionId::Redo), None),
    ("Ctrl+X", act(ActionId::Cut), None),
    ("Ctrl+C", act(ActionId::Copy), None),
    ("Ctrl+V", act(ActionId::Paste), None),
    ("Delete", act(ActionId::Delete), None),
    ("F2", act(ActionId::Rename), None),
    ("Ctrl+P", ui(UiCommand::Palette), None),
    ("F3", ui(UiCommand::Palette), None),
    ("Ctrl+Comma", ui(UiCommand::Preferences), None),
    ("Tab", ui(UiCommand::NextMode), None),
    ("Ctrl+G", ui(UiCommand::GroupSelection), None),
    ("Ctrl+Shift+G", ui(UiCommand::Ungroup), None),
    ("Shift+Tab", ui(UiCommand::ToggleSnap), None),
    ("Shift+Z", act(ActionId::WireframeMode), None),
    ("Shift+A", act(ActionId::CreateElementCube), M),
    ("Home", ui(UiCommand::FrameSelected), None),
    ("F", ui(UiCommand::FrameSelected), None),
    ("Period", ui(UiCommand::FrameSelected), None),
    ("1", view(ViewPreset::Front), None),
    ("Ctrl+1", view(ViewPreset::Back), None),
    ("3", view(ViewPreset::Right), None),
    ("Ctrl+3", view(ViewPreset::Left), None),
    ("7", view(ViewPreset::Top), None),
    ("Ctrl+7", view(ViewPreset::Bottom), None),
    ("5", view(ViewPreset::ToggleProjection), None),
    ("F6", ui(UiCommand::TogglePanel(PanelId::Outliner)), None),
    ("F7", ui(UiCommand::TogglePanel(PanelId::Properties)), None),
    ("F8", ui(UiCommand::TogglePanel(PanelId::Uv)), None),
    ("F9", ui(UiCommand::TogglePanel(PanelId::Textures)), None),
    ("F10", ui(UiCommand::TogglePanel(PanelId::History)), None),
    ("Ctrl+Alt+1", ui(UiCommand::SetMode(Mode::Model)), None),
    ("Ctrl+Alt+2", ui(UiCommand::SetMode(Mode::Voxel)), None),
    ("Ctrl+Alt+3", ui(UiCommand::SetMode(Mode::Paint)), None),
    ("Ctrl+Alt+4", ui(UiCommand::SetMode(Mode::Display)), None),
    ("Ctrl+Alt+5", ui(UiCommand::SetMode(Mode::Animate)), None),
];

const BLENDER: Table = &[
    ("Space", ui(UiCommand::PlayPause), A),
    ("I", ui(UiCommand::InsertKeyframe), A),
    ("Ctrl+Shift+I", act(ActionId::Import), None),
    ("W", tool(Tool::Select), M),
    ("G", tool(Tool::Move), M),
    ("R", tool(Tool::Rotate), M),
    ("S", tool(Tool::Scale), M),
    ("Shift+D", act(ActionId::Clone), M),
    ("X", act(ActionId::DeleteConfirm), None),
    ("A", act(ActionId::SelectAllElements), M),
    ("Alt+A", act(ActionId::SelectNone), M),
    ("Ctrl+I", act(ActionId::SelectInverse), M),
    ("H", act(ActionId::ToggleVisibility), M),
    ("Alt+H", act(ActionId::ShowAll), M),
    ("Ctrl+L", act(ActionId::ToggleLock), M),
    ("D", tool(Tool::VoxelPencil), V),
    ("E", tool(Tool::VoxelErase), V),
    ("F", tool(Tool::VoxelFill), V),
    ("B", tool(Tool::VoxelBox), V),
    ("L", tool(Tool::VoxelLine), V),
    ("I", tool(Tool::VoxelPicker), V),
    ("D", tool(Tool::PaintPencil), P),
    ("E", tool(Tool::PaintEraser), P),
    ("F", tool(Tool::PaintFill), P),
    ("I", tool(Tool::PaintPicker), P),
];

const BLOCKBENCH: Table = &[
    ("Space", ui(UiCommand::PlayPause), A),
    ("Ctrl+Alt+O", act(ActionId::Import), None),
    ("Q", tool(Tool::Select), M),
    ("V", tool(Tool::Move), M),
    ("R", tool(Tool::Rotate), M),
    ("S", tool(Tool::Scale), M),
    ("Ctrl+D", act(ActionId::Clone), M),
    ("Ctrl+A", act(ActionId::SelectAllElements), None),
    ("Ctrl+Shift+A", act(ActionId::SelectNone), None),
    ("Ctrl+Shift+I", act(ActionId::SelectInverse), None),
    ("H", act(ActionId::ToggleVisibility), M),
    ("Ctrl+L", act(ActionId::ToggleLock), M),
    ("B", tool(Tool::VoxelPencil), V),
    ("E", tool(Tool::VoxelErase), V),
    ("G", tool(Tool::VoxelFill), V),
    ("X", tool(Tool::VoxelBox), V),
    ("L", tool(Tool::VoxelLine), V),
    ("I", tool(Tool::VoxelPicker), V),
    ("B", tool(Tool::PaintPencil), P),
    ("E", tool(Tool::PaintEraser), P),
    ("G", tool(Tool::PaintFill), P),
    ("I", tool(Tool::PaintPicker), P),
];

/// The chord of a canonical menu shortcut (`Ctrl+Shift+S`, `Alpha1`, `KeypadPlus`, `PageUp`). The
/// window reads egui keys, which cannot tell the keypad from the top row, so keypad keys map to
/// their digit and sign keys.
fn chord_of_menu_shortcut(canonical: &str) -> Option<Chord> {
    let mut tokens: Vec<&str> = canonical.split('+').collect();
    let key = match tokens.pop()? {
        "KeypadPlus" => "Plus",
        "KeypadMinus" => "Minus",
        other => match other
            .strip_prefix("Alpha")
            .or_else(|| other.strip_prefix("Keypad"))
        {
            Some(digit) if digit.len() == 1 => digit,
            _ => other,
        },
    };
    let text: Vec<&str> = tokens.into_iter().chain([key]).collect();
    Chord::parse(&text.join("+"))
}

/// Window commands that Cubik's menus do not have, kept in the Cubik preset where their chord is
/// free. F3 opens the palette here too, so Cubik's hidden F3 (select by name) is Ctrl+F only.
const CUBIK_EXTRAS: Table = &[
    ("F3", ui(UiCommand::Palette), None),
    ("Ctrl+Comma", ui(UiCommand::Preferences), None),
    ("Tab", ui(UiCommand::NextMode), None),
    ("Shift+Tab", ui(UiCommand::ToggleSnap), None),
    ("Ctrl+G", ui(UiCommand::GroupSelection), None),
    ("Ctrl+Shift+G", ui(UiCommand::Ungroup), None),
    ("Ctrl+Shift+Z", act(ActionId::Redo), None),
    ("Ctrl+Q", act(ActionId::Exit), None),
    ("Home", ui(UiCommand::FrameSelected), None),
    ("F", ui(UiCommand::FrameSelected), None),
    ("Period", ui(UiCommand::FrameSelected), None),
    ("Ctrl+Alt+1", ui(UiCommand::SetMode(Mode::Model)), None),
    ("Ctrl+Alt+2", ui(UiCommand::SetMode(Mode::Voxel)), None),
    ("Ctrl+Alt+3", ui(UiCommand::SetMode(Mode::Paint)), None),
    ("Ctrl+Alt+4", ui(UiCommand::SetMode(Mode::Display)), None),
    ("Ctrl+Alt+5", ui(UiCommand::SetMode(Mode::Animate)), None),
    ("P", ui(UiCommand::PlayPause), A),
];

/// Cubik's hidden bindings (menus 6.3), and its held mode keys as presses: the window has no
/// held-key semantics, so 1 and 2 choose the rotate and scale tools and 3 duplicates.
const CUBIK_HIDDEN: Table = &[
    ("Delete", act(ActionId::DeleteConfirm), None),
    ("F2", act(ActionId::Rename), None),
    ("1", tool(Tool::Rotate), M),
    ("2", tool(Tool::Scale), M),
    ("3", act(ActionId::Clone), M),
];

/// The Cubik preset: the shortcut text of every menu row that runs a live action (menus 6.1), the
/// hidden bindings, then the window commands whose chord is still free.
fn cubik_bindings() -> Vec<Binding> {
    let live = |id: ActionId| {
        ACTIONS
            .iter()
            .any(|def| def.id == id && def.kind == ActionKind::Live)
    };
    let rows = MENUS.iter().flat_map(|menu| menu.rows).filter_map(|row| {
        match (row.shortcut, row.target) {
            (Some(shortcut), RowTarget::Action(id)) if live(id) => {
                let chord = chord_of_menu_shortcut(canonical_shortcut(shortcut).as_str());
                if chord.is_none() {
                    log::warn!("ui: Cubik shortcut {shortcut:?} has no window key");
                }
                Some(Binding {
                    chord: chord?,
                    command: act(id),
                    mode: None,
                })
            }
            _ => None,
        }
    });
    let mut out: Vec<Binding> = Vec::new();
    for binding in rows.chain(table_bindings(&[CUBIK_HIDDEN, CUBIK_EXTRAS, PLAYBACK])) {
        if !out
            .iter()
            .any(|b| b.chord == binding.chord && b.mode == binding.mode)
        {
            out.push(binding);
        }
    }
    out
}

fn table_bindings(tables: &[Table]) -> impl Iterator<Item = Binding> + '_ {
    tables
        .iter()
        .flat_map(|table| table.iter())
        .filter_map(|(text, command, mode)| {
            let chord = Chord::parse(text);
            if chord.is_none() {
                log::warn!("ui: built-in chord {text:?} does not parse");
            }
            Some(Binding {
                chord: chord?,
                command: *command,
                mode: *mode,
            })
        })
}

impl Keymap {
    /// The bindings of a preset without user changes.
    pub fn preset(preset: KeymapPreset) -> Keymap {
        let bindings = match preset {
            KeymapPreset::Blender => table_bindings(&[COMMON, PLAYBACK, BLENDER]).collect(),
            KeymapPreset::Blockbench => table_bindings(&[COMMON, PLAYBACK, BLOCKBENCH]).collect(),
            KeymapPreset::Cubik => cubik_bindings(),
        };
        Keymap { bindings }
    }

    /// A preset with the user's changes on top. Unreadable overrides are skipped.
    pub fn with_overrides(preset: KeymapPreset, overrides: &[KeyOverride]) -> Keymap {
        let mut map = Keymap::preset(preset);
        for item in overrides {
            let Some(command) = Command::from_id(&item.command) else {
                log::warn!("ui: keymap override for unknown command {:?}", item.command);
                continue;
            };
            match item.chord.as_deref() {
                None => map.unbind(command),
                Some(text) => match Chord::parse(text) {
                    Some(chord) => {
                        map.bind(command, chord);
                    }
                    None => log::warn!("ui: keymap override with unreadable chord {text:?}"),
                },
            }
        }
        map
    }

    /// The command for a chord in a mode; a binding of the mode wins over a global one. Animate
    /// has the keys of Model besides its own: its tools and the editing of the rest pose work
    /// there as well.
    pub fn lookup(&self, chord: Chord, mode: Mode) -> Option<Command> {
        let (mut shared, mut global) = (None, None);
        for b in &self.bindings {
            if b.chord != chord {
                continue;
            }
            match b.mode {
                Some(m) if m == mode => return Some(b.command),
                Some(Mode::Model) if mode == Mode::Animate => shared = Some(b.command),
                None => global = Some(b.command),
                Some(_) => {}
            }
        }
        shared.or(global)
    }

    /// The chord to show next to a command in `mode`: its mode binding first, then (in Animate)
    /// one of Model, then a global one.
    pub fn shortcut_of(&self, command: Command, mode: Mode) -> Option<Chord> {
        let (mut shared, mut global) = (None, None);
        for b in self.bindings.iter().filter(|b| b.command == command) {
            match b.mode {
                Some(m) if m == mode => return Some(b.chord),
                Some(Mode::Model) if mode == Mode::Animate && shared.is_none() => {
                    shared = Some(b.chord);
                }
                None if global.is_none() => global = Some(b.chord),
                _ => {}
            }
        }
        shared.or(global)
    }

    /// Binds `chord` globally to `command`, replacing the command's other chords. Returns the
    /// command that lost the chord, if another one had it.
    pub fn bind(&mut self, command: Command, chord: Chord) -> Option<Command> {
        let displaced = self
            .bindings
            .iter()
            .find(|b| b.chord == chord && b.mode.is_none() && b.command != command)
            .map(|b| b.command);
        self.bindings
            .retain(|b| b.command != command && !(b.chord == chord && b.mode.is_none()));
        self.bindings.push(Binding {
            chord,
            command,
            mode: None,
        });
        displaced
    }

    /// Removes every chord of a command.
    pub fn unbind(&mut self, command: Command) {
        self.bindings.retain(|b| b.command != command);
    }
}

/// The overrides that turn `preset` into `keymap`, for saving. Compares by command id.
pub fn diff_from_preset(
    preset: KeymapPreset,
    keymap: &Keymap,
    commands: &[Command],
) -> Vec<KeyOverride> {
    let base = Keymap::preset(preset);
    commands
        .iter()
        .filter_map(|&command| {
            let (a, b) = (base.global_chords(command), keymap.global_chords(command));
            (a != b).then(|| KeyOverride {
                command: command.id(),
                chord: b.first().map(ToString::to_string),
            })
        })
        .collect()
}

impl Keymap {
    fn global_chords(&self, command: Command) -> Vec<Chord> {
        self.bindings
            .iter()
            .filter(|b| b.command == command && b.mode.is_none())
            .map(|b| b.chord)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_and_print_round_trip() {
        for text in [
            "Ctrl+S",
            "Ctrl+Shift+Z",
            "F3",
            "Delete",
            "Alt+A",
            "Shift+Tab",
            "1",
            "Period",
        ] {
            let chord = Chord::parse(text).expect(text);
            assert_eq!(chord.to_string(), text);
        }
        assert_eq!(Chord::parse("cmd+x").map(|c| c.ctrl), Some(true));
    }

    #[test]
    fn bad_chords_are_rejected() {
        for text in ["", "Ctrl+", "Hyper+S", "Ctrl+Nope", "+"] {
            assert!(Chord::parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn every_builtin_chord_parses() {
        for table in [
            COMMON,
            PLAYBACK,
            BLENDER,
            BLOCKBENCH,
            CUBIK_EXTRAS,
            CUBIK_HIDDEN,
        ] {
            for (text, _, _) in table {
                assert!(Chord::parse(text).is_some(), "{text}");
            }
        }
    }

    #[test]
    fn presets_have_no_duplicate_chord_in_one_scope() {
        for preset in KeymapPreset::ALL {
            let map = Keymap::preset(preset);
            for (i, a) in map.bindings.iter().enumerate() {
                for b in &map.bindings[i + 1..] {
                    assert!(
                        !(a.chord == b.chord && a.mode == b.mode),
                        "{preset:?}: {} is bound twice",
                        a.chord
                    );
                }
            }
        }
    }

    #[test]
    fn animate_has_the_keys_of_model_and_its_own_that_shadow_none_of_them() {
        for preset in KeymapPreset::ALL {
            let map = Keymap::preset(preset);
            for own in map.bindings.iter().filter(|b| b.mode == A) {
                let shadowed = map.bindings.iter().find(|b| {
                    b.chord == own.chord && (b.mode.is_none() || b.mode == Some(Mode::Model))
                });
                assert!(
                    shadowed.is_none(),
                    "{preset:?}: {} is bound in Animate and in Model",
                    own.chord
                );
            }
        }
        // The tools and the editing keys of Model work in Animate, in each preset.
        let blender = Keymap::preset(KeymapPreset::Blender);
        let g = Chord::plain(Key::G);
        assert_eq!(blender.lookup(g, Mode::Animate), Some(tool(Tool::Move)));
        assert_eq!(
            blender.shortcut_of(tool(Tool::Move), Mode::Animate),
            Some(g)
        );
        let cubik = Keymap::preset(KeymapPreset::Cubik);
        let one = Chord::plain(Key::Num1);
        assert_eq!(cubik.lookup(one, Mode::Animate), Some(tool(Tool::Rotate)));
        // Animate's own keys exist only there.
        let space = Chord::plain(Key::Space);
        assert_eq!(
            blender.lookup(space, Mode::Animate),
            Some(ui(UiCommand::PlayPause))
        );
        assert_eq!(blender.lookup(space, Mode::Model), None);
        assert_eq!(cubik.lookup(space, Mode::Animate), None);
        assert_eq!(
            cubik.lookup(Chord::plain(Key::P), Mode::Animate),
            Some(ui(UiCommand::PlayPause))
        );
        let blockbench = Keymap::preset(KeymapPreset::Blockbench);
        assert_eq!(blockbench.lookup(Chord::plain(Key::I), Mode::Animate), None);
        assert_eq!(
            blender.lookup(Chord::plain(Key::I), Mode::Animate),
            Some(ui(UiCommand::InsertKeyframe))
        );
        // Every preset opens the workspace with Ctrl+Alt+5.
        let open = Chord::parse("Ctrl+Alt+5").expect("chord");
        for preset in KeymapPreset::ALL {
            assert_eq!(
                Keymap::preset(preset).lookup(open, Mode::Model),
                Some(ui(UiCommand::SetMode(Mode::Animate))),
                "{preset:?}"
            );
        }
    }

    #[test]
    fn every_cubik_chord_maps_to_an_existing_command_and_none_repeats() {
        let map = Keymap::preset(KeymapPreset::Cubik);
        assert!(map.bindings.len() > 60, "{}", map.bindings.len());
        let live: Vec<ActionId> = ACTIONS
            .iter()
            .filter(|def| def.kind == ActionKind::Live)
            .map(|def| def.id)
            .collect();
        let ui_commands = all_ui_commands();
        for (i, a) in map.bindings.iter().enumerate() {
            match a.command {
                Command::Action(id) => assert!(live.contains(&id), "{id:?} is not live"),
                Command::Ui(c) => assert!(ui_commands.contains(&c), "{c:?} is unknown"),
            }
            // The chord survives its text form.
            assert_eq!(Chord::parse(&a.chord.to_string()), Some(a.chord));
            for b in &map.bindings[i + 1..] {
                assert!(
                    !(a.chord == b.chord && a.mode == b.mode),
                    "{} is bound twice",
                    a.chord
                );
            }
        }
    }

    #[test]
    fn every_cubik_menu_shortcut_of_a_live_action_is_bound() {
        let map = Keymap::preset(KeymapPreset::Cubik);
        let mut missing = Vec::new();
        for menu in MENUS {
            for row in menu.rows {
                let (Some(shortcut), RowTarget::Action(id)) = (row.shortcut, row.target) else {
                    continue;
                };
                let live = ACTIONS
                    .iter()
                    .any(|def| def.id == id && def.kind == ActionKind::Live);
                if live && map.shortcut_of(act(id), Mode::Model).is_none() {
                    missing.push(format!("{} ({shortcut})", row.label));
                }
            }
        }
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn cubik_keys_follow_the_cubik_menus() {
        let map = Keymap::preset(KeymapPreset::Cubik);
        let key = |text: &str| Chord::parse(text).expect(text);
        let at = |text: &str| map.lookup(key(text), Mode::Model);
        assert_eq!(at("Ctrl+S"), Some(act(ActionId::Save)));
        assert_eq!(at("Ctrl+D"), Some(act(ActionId::SelectNone)));
        assert_eq!(at("Ctrl+F"), Some(act(ActionId::SelectName)));
        assert_eq!(at("Shift+M"), Some(act(ActionId::Move)));
        assert_eq!(at("F10"), Some(act(ActionId::ElementsList)));
        assert_eq!(at("Plus"), Some(act(ActionId::ZoomPlus)));
        assert_eq!(at("Ctrl+1"), Some(act(ActionId::MoveManagersOnLeft)));
        // The hold keys act on press: rotate and scale choose their tool.
        assert_eq!(at("1"), Some(tool(Tool::Rotate)));
        assert_eq!(at("2"), Some(tool(Tool::Scale)));
        // The palette keeps F3 in every preset.
        assert_eq!(at("F3"), Some(ui(UiCommand::Palette)));
    }

    #[test]
    fn mode_bindings_win_over_global_ones() {
        let map = Keymap::preset(KeymapPreset::Blender);
        let d = Chord::plain(Key::D);
        assert_eq!(map.lookup(d, Mode::Voxel), Some(tool(Tool::VoxelPencil)));
        assert_eq!(map.lookup(d, Mode::Paint), Some(tool(Tool::PaintPencil)));
        assert_eq!(map.lookup(d, Mode::Display), None);
        let save = Chord::parse("Ctrl+S").expect("chord");
        assert_eq!(map.lookup(save, Mode::Display), Some(act(ActionId::Save)));
    }

    #[test]
    fn presets_differ_where_they_should() {
        let blender = Keymap::preset(KeymapPreset::Blender);
        let blockbench = Keymap::preset(KeymapPreset::Blockbench);
        let g = Chord::plain(Key::G);
        assert_eq!(blender.lookup(g, Mode::Model), Some(tool(Tool::Move)));
        assert_eq!(blockbench.lookup(g, Mode::Model), None);
        let v = Chord::plain(Key::V);
        assert_eq!(blockbench.lookup(v, Mode::Model), Some(tool(Tool::Move)));
        let select_all = Chord::parse("Ctrl+A").expect("chord");
        assert_eq!(blender.lookup(select_all, Mode::Model), None);
        assert_eq!(
            blockbench.lookup(select_all, Mode::Model),
            Some(act(ActionId::SelectAllElements))
        );
    }

    #[test]
    fn rebinding_moves_a_command_and_reports_the_displaced_one() {
        let mut map = Keymap::preset(KeymapPreset::Blender);
        let chord = Chord::parse("Ctrl+Z").expect("chord");
        let displaced = map.bind(act(ActionId::Redo), chord);
        assert_eq!(displaced, Some(act(ActionId::Undo)));
        assert_eq!(map.lookup(chord, Mode::Model), Some(act(ActionId::Redo)));
        assert_eq!(map.shortcut_of(act(ActionId::Undo), Mode::Model), None);
        assert_eq!(
            map.shortcut_of(act(ActionId::Redo), Mode::Model),
            Some(chord)
        );
    }

    #[test]
    fn overrides_apply_on_top_of_the_preset_and_survive_a_diff() {
        let overrides = vec![
            KeyOverride {
                command: act(ActionId::Save).id(),
                chord: Some("Ctrl+Alt+S".to_owned()),
            },
            KeyOverride {
                command: act(ActionId::Exit).id(),
                chord: None,
            },
            KeyOverride {
                command: "action:NoSuchThing".to_owned(),
                chord: Some("F1".to_owned()),
            },
        ];
        let map = Keymap::with_overrides(KeymapPreset::Blender, &overrides);
        let new_save = Chord::parse("Ctrl+Alt+S").expect("chord");
        assert_eq!(map.lookup(new_save, Mode::Model), Some(act(ActionId::Save)));
        assert_eq!(map.shortcut_of(act(ActionId::Exit), Mode::Model), None);

        let commands = [
            act(ActionId::Save),
            act(ActionId::Exit),
            act(ActionId::Open),
        ];
        let diff = diff_from_preset(KeymapPreset::Blender, &map, &commands);
        assert_eq!(diff.len(), 2);
        let again = Keymap::with_overrides(KeymapPreset::Blender, &diff);
        assert_eq!(
            again.lookup(new_save, Mode::Model),
            Some(act(ActionId::Save))
        );
    }

    #[test]
    fn command_ids_round_trip_for_every_command() {
        for def in cuboid_actions::ACTIONS {
            let command = Command::Action(def.id);
            assert_eq!(Command::from_id(&command.id()), Some(command));
        }
        for c in all_ui_commands() {
            let command = Command::Ui(c);
            assert_eq!(Command::from_id(&command.id()), Some(command));
        }
        assert_eq!(Command::from_id("nonsense"), None);
    }

    #[test]
    fn key_names_survive_a_round_trip() {
        for key in Key::ALL {
            let chord = Chord::plain(*key);
            if let Some(back) = Chord::parse(&chord.to_string()) {
                assert_eq!(back, chord, "{key:?}");
            }
        }
    }
}
