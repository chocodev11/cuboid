//! Chords, commands and the keymap presets.
//!
//! The UI reads egui key events, so a chord is an egui key plus Ctrl (Command on macOS), Shift
//! and Alt. Keypad digits cannot be told from the top row there; the view shortcuts therefore use
//! the digit keys, like the "emulate numpad" option of other editors.

use std::fmt;

use cuboid_actions::ActionId;
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
    /// Turn snapping on or off.
    ToggleSnap,
    /// Put the selection into a group.
    GroupSelection,
    /// Dissolve the selected groups.
    Ungroup,
    /// Switch between the dark and light theme.
    ToggleTheme,
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
        UiCommand::ToggleSnap,
        UiCommand::GroupSelection,
        UiCommand::Ungroup,
        UiCommand::ToggleTheme,
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
}

impl KeymapPreset {
    /// Both presets.
    pub const ALL: [KeymapPreset; 2] = [KeymapPreset::Blender, KeymapPreset::Blockbench];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            KeymapPreset::Blender => "Blender-style",
            KeymapPreset::Blockbench => "Blockbench-style",
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
];

const BLENDER: Table = &[
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

impl Keymap {
    /// The bindings of a preset without user changes.
    pub fn preset(preset: KeymapPreset) -> Keymap {
        let specific = match preset {
            KeymapPreset::Blender => BLENDER,
            KeymapPreset::Blockbench => BLOCKBENCH,
        };
        let bindings = COMMON
            .iter()
            .chain(specific)
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
            .collect();
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

    /// The command for a chord in a mode; a binding of the mode wins over a global one.
    pub fn lookup(&self, chord: Chord, mode: Mode) -> Option<Command> {
        let mut global = None;
        for b in &self.bindings {
            if b.chord != chord {
                continue;
            }
            match b.mode {
                Some(m) if m == mode => return Some(b.command),
                None => global = Some(b.command),
                Some(_) => {}
            }
        }
        global
    }

    /// The chord to show next to a command in `mode`: its mode binding first, then a global one.
    pub fn shortcut_of(&self, command: Command, mode: Mode) -> Option<Chord> {
        let mut global = None;
        for b in self.bindings.iter().filter(|b| b.command == command) {
            match b.mode {
                Some(m) if m == mode => return Some(b.chord),
                None if global.is_none() => global = Some(b.chord),
                _ => {}
            }
        }
        global
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
        for table in [COMMON, BLENDER, BLOCKBENCH] {
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
